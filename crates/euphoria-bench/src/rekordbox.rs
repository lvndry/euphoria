// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Scores the keys and tempos rekordbox found for the benchmark previews,
//! read from a collection that rekordbox exported as XML, with the same
//! rules as Euphoria's own detectors.
//!
//! Tracks are matched by file name, so the previews can have been copied
//! anywhere before rekordbox analyzed them. 43 previews are in both
//! datasets under the same name; when the export lists a name more than
//! once, the copy in a folder named after the dataset (`key` or `tempo`) is
//! the one scored.

use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use euphoria_core::{Key, Mode};

use crate::dataset::{self, Dataset};
use crate::evaluate::{Failure, Tally};
use crate::render::{FailureLine, group_failures};
use crate::scoring::{KeyTally, TempoTally};

const USAGE: &str =
    "usage: euphoria-bench rekordbox COLLECTION_XML [DATA_DIRECTORY] [--only key|tempo]";

/// Folder names of the two datasets, in the data directory and in the kit
/// that `bench/rekordbox_accuracy_kit.py` builds.
const KEY_FOLDER: &str = "key";
const TEMPO_FOLDER: &str = "tempo";

/// The parsed command line of the rekordbox mode.
#[derive(Debug, PartialEq, Eq)]
pub struct Arguments {
    collection_file: PathBuf,
    data_directory: PathBuf,
    score_key: bool,
    score_tempo: bool,
}

/// Parses the arguments that follow `rekordbox`.
pub fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Arguments> {
    let mut positional = Vec::new();
    let mut score_key = true;
    let mut score_tempo = true;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "--only" {
            if !score_key || !score_tempo {
                bail!("--only may be given only once\n{USAGE}");
            }
            let value = arguments
                .next()
                .with_context(|| format!("--only needs a value\n{USAGE}"))?;
            match value.as_str() {
                "key" => score_tempo = false,
                "tempo" => score_key = false,
                _ => bail!("--only accepts key or tempo, not {value:?}\n{USAGE}"),
            }
        } else if argument.starts_with("--") {
            bail!("unknown option {argument:?}\n{USAGE}");
        } else {
            positional.push(PathBuf::from(argument));
        }
    }
    let mut positional = positional.into_iter();
    let collection_file = positional
        .next()
        .with_context(|| format!("no collection XML given\n{USAGE}"))?;
    let data_directory = positional
        .next()
        .unwrap_or_else(|| PathBuf::from("bench/data"));
    if positional.next().is_some() {
        bail!("more than one data directory given\n{USAGE}");
    }
    Ok(Arguments {
        collection_file,
        data_directory,
        score_key,
        score_tempo,
    })
}

/// Reads the export, scores it against the datasets and prints the report.
pub fn run(arguments: &Arguments) -> Result<()> {
    let text = std::fs::read_to_string(&arguments.collection_file)
        .with_context(|| format!("cannot read {}", arguments.collection_file.display()))?;
    let collection = Collection::parse(&text)
        .with_context(|| format!("in {}", arguments.collection_file.display()))?;
    let key = arguments
        .score_key
        .then(|| dataset::load_key_tracks(&arguments.data_directory.join(KEY_FOLDER)))
        .transpose()?
        .map(|dataset| score_keys(&dataset, &collection));
    let tempo = arguments
        .score_tempo
        .then(|| dataset::load_tempo_tracks(&arguments.data_directory.join(TEMPO_FOLDER)))
        .transpose()?
        .map(|dataset| score_tempos(&dataset, &collection));
    print!(
        "{}",
        RekordboxReport {
            collection: &collection,
            key,
            tempo,
        }
    );
    Ok(())
}

/// One track of the exported collection, with the values that are scored.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    /// The decoded path, as the export gives it.
    path: String,
    /// The name of the folder that holds the file.
    folder: String,
    /// The `Tonality` attribute, empty when rekordbox found no key.
    tonality: String,
    /// The `AverageBpm` attribute, empty when it is absent.
    average_bpm: String,
}

/// The tracks of a rekordbox collection export, by file name.
#[derive(Debug)]
pub struct Collection {
    /// The program and version that wrote the export, such as
    /// "rekordbox 7.2.19".
    product: String,
    /// Every track with a location, by file name.
    entries_by_file_name: HashMap<String, Vec<Entry>>,
    /// Tracks in the collection, with or without a location.
    track_count: usize,
}

/// What the export holds for one dataset track.
#[derive(Debug, PartialEq, Eq)]
enum Lookup<'collection> {
    Found(&'collection Entry),
    Absent,
    /// Listed more than once with different values, and not once in a
    /// folder named after the dataset.
    Ambiguous(Vec<&'collection str>),
}

impl Collection {
    /// Parses the text of a rekordbox XML export.
    pub fn parse(text: &str) -> Result<Self> {
        let document = roxmltree::Document::parse(text).context("not an XML document")?;
        let root = document.root_element();
        if !root.has_tag_name("DJ_PLAYLISTS") {
            bail!(
                "the document is {:?}, not a rekordbox DJ_PLAYLISTS export",
                root.tag_name().name()
            );
        }
        let product = root
            .children()
            .find(|node| node.has_tag_name("PRODUCT"))
            .map(|node| {
                let name = node.attribute("Name").unwrap_or("unknown program");
                match node.attribute("Version") {
                    Some(version) => format!("{name} {version}"),
                    None => name.to_string(),
                }
            })
            .unwrap_or_else(|| "unknown program".to_string());
        let collection = root
            .children()
            .find(|node| node.has_tag_name("COLLECTION"))
            .context("the export has no COLLECTION element")?;

        let mut entries_by_file_name: HashMap<String, Vec<Entry>> = HashMap::new();
        let mut track_count = 0;
        for track in collection
            .children()
            .filter(|node| node.has_tag_name("TRACK"))
        {
            track_count += 1;
            let Some(location) = track.attribute("Location") else {
                continue;
            };
            let path = location_path(location);
            let mut components = path.rsplit('/');
            let file_name = components.next().unwrap_or_default().to_string();
            let folder = components.next().unwrap_or_default().to_string();
            entries_by_file_name
                .entry(file_name)
                .or_default()
                .push(Entry {
                    folder,
                    tonality: track.attribute("Tonality").unwrap_or_default().to_string(),
                    average_bpm: track
                        .attribute("AverageBpm")
                        .unwrap_or_default()
                        .to_string(),
                    path,
                });
        }
        Ok(Self {
            product,
            entries_by_file_name,
            track_count,
        })
    }

    fn lookup(&self, file_name: &str, dataset_folder: &str) -> Lookup<'_> {
        let Some(entries) = self.entries_by_file_name.get(file_name) else {
            return Lookup::Absent;
        };
        let in_dataset_folder: Vec<&Entry> = entries
            .iter()
            .filter(|entry| entry.folder == dataset_folder)
            .collect();
        let candidates: Vec<&Entry> = if in_dataset_folder.is_empty() {
            entries.iter().collect()
        } else {
            in_dataset_folder
        };
        let first = candidates[0];
        let all_agree = candidates.iter().all(|entry| {
            entry.tonality == first.tonality && entry.average_bpm == first.average_bpm
        });
        if all_agree {
            Lookup::Found(first)
        } else {
            Lookup::Ambiguous(candidates.iter().map(|entry| entry.path.as_str()).collect())
        }
    }
}

/// The path of a `Location` URL such as `file://localhost/Users/dj/a%20b.mp3`,
/// percent decoded. Bytes that do not form UTF-8 are replaced.
fn location_path(location: &str) -> String {
    let without_scheme = location
        .strip_prefix("file://localhost")
        .or_else(|| location.strip_prefix("file://"))
        .unwrap_or(location);
    let bytes = without_scheme.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = (bytes[index] == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                index += 3;
            }
            None => {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

/// Reads a `Tonality` value: a Camelot code such as "8A", an Open Key code
/// such as "1m", or a musical name such as "Am", "F#m", "Db" or "A minor".
/// An empty value is no key.
fn parse_tonality(text: &str) -> Result<Option<Key>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    if let Ok(key) = text.parse::<Key>() {
        return Ok(Some(key));
    }
    parse_short_key_name(text)
        .map(Some)
        .ok_or_else(|| format!("Tonality {text:?} is not a key"))
}

/// Reads a short musical name: a note letter, an optional sharp or flat,
/// and "m" for minor.
fn parse_short_key_name(text: &str) -> Option<Key> {
    let mut characters = text.chars();
    let natural: i32 = match characters.next()?.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let rest = characters.as_str();
    let (accidental, mode_text) = match rest.chars().next() {
        Some(sign @ ('#' | '\u{266F}')) => (1, &rest[sign.len_utf8()..]),
        Some(sign @ ('b' | '\u{266D}')) => (-1, &rest[sign.len_utf8()..]),
        _ => (0, rest),
    };
    let mode = match mode_text {
        "" => Mode::Major,
        "m" => Mode::Minor,
        _ => return None,
    };
    Some(Key::new((natural + accidental).rem_euclid(12) as u8, mode))
}

/// Reads an `AverageBpm` value. Empty or zero is no tempo, which is what
/// rekordbox writes for a track it has not analyzed.
fn parse_average_bpm(text: &str) -> Result<Option<f32>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    match text.parse::<f32>() {
        Ok(tempo) if tempo.is_finite() && tempo > 0.0 => Ok(Some(tempo)),
        Ok(0.0) => Ok(None),
        _ => Err(format!("AverageBpm {text:?} is not a tempo")),
    }
}

/// rekordbox's results on one dataset.
#[derive(Debug)]
pub struct Scored<Scores> {
    /// Scores over every dataset track. A track with no usable value counts
    /// in the tally as having no estimate.
    pub tally: Scores,
    /// Tracks in the dataset.
    pub track_count: usize,
    /// Tracks the export holds with an empty value.
    pub empty_count: usize,
    /// Tracks absent from the export, in track order.
    pub absent: Vec<String>,
    /// Tracks whose value could not be read or was ambiguous, in track
    /// order.
    pub problems: Vec<Failure>,
}

fn score<Scores: Tally>(
    dataset: &Dataset<Scores::Reference>,
    collection: &Collection,
    dataset_folder: &str,
    read: impl Fn(&Entry) -> Result<Option<Scores::Estimate>, String>,
) -> Scored<Scores> {
    let mut scored = Scored {
        tally: Scores::default(),
        track_count: dataset.tracks.len(),
        empty_count: 0,
        absent: Vec::new(),
        problems: Vec::new(),
    };
    for track in &dataset.tracks {
        let file_name = file_name_of(&track.audio_path);
        let reason = match collection.lookup(&file_name, dataset_folder) {
            Lookup::Found(entry) => match read(entry) {
                Ok(Some(estimate)) => {
                    scored.tally.record_estimate(&track.annotation, estimate);
                    continue;
                }
                Ok(None) => {
                    scored.empty_count += 1;
                    None
                }
                Err(reason) => Some(reason),
            },
            Lookup::Absent => {
                scored.absent.push(track.name.clone());
                None
            }
            Lookup::Ambiguous(paths) => Some(format!(
                "listed {} times with different values: {}",
                paths.len(),
                paths.join(", ")
            )),
        };
        scored.tally.record_no_estimate();
        if let Some(reason) = reason {
            scored.problems.push(Failure {
                track: track.name.clone(),
                reason,
            });
        }
    }
    scored
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Scores the export's keys against GiantSteps Key.
pub fn score_keys(dataset: &Dataset<Key>, collection: &Collection) -> Scored<KeyTally> {
    score(dataset, collection, KEY_FOLDER, |entry| {
        parse_tonality(&entry.tonality)
    })
}

/// Scores the export's tempos against GiantSteps Tempo.
pub fn score_tempos(dataset: &Dataset<f32>, collection: &Collection) -> Scored<TempoTally> {
    score(dataset, collection, TEMPO_FOLDER, |entry| {
        parse_average_bpm(&entry.average_bpm)
    })
}

struct RekordboxReport<'collection> {
    collection: &'collection Collection,
    key: Option<Scored<KeyTally>>,
    tempo: Option<Scored<TempoTally>>,
}

impl Display for RekordboxReport<'_> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let product = &self.collection.product;
        writeln!(
            formatter,
            "Values read from a collection exported by {product}, which holds {} tracks.\n",
            self.collection.track_count
        )?;
        if let Some(key) = &self.key {
            writeln!(formatter, "## Key detection, GiantSteps Key\n")?;
            write_coverage(formatter, key, "key")?;
            writeln!(
                formatter,
                "| Detector | Exact | Fifth | Relative | Parallel | Other | No key | MIREX score |"
            )?;
            writeln!(
                formatter,
                "| --- | --- | --- | --- | --- | --- | --- | --- |"
            )?;
            let tally = &key.tally;
            writeln!(
                formatter,
                "| {product} | {} | {} | {} | {} | {} | {} | {:.1}% |\n",
                tally.percent(tally.exact),
                tally.percent(tally.fifth),
                tally.percent(tally.relative),
                tally.percent(tally.parallel),
                tally.percent(tally.other),
                tally.percent(tally.missing),
                tally.mirex_score() * 100.0,
            )?;
            write_track_lists(formatter, key)?;
        }
        if let Some(tempo) = &self.tempo {
            writeln!(formatter, "## Tempo detection, GiantSteps Tempo\n")?;
            write_coverage(formatter, tempo, "tempo")?;
            writeln!(
                formatter,
                "| Detector | Accuracy 1 | Accuracy 2 | No tempo |"
            )?;
            writeln!(formatter, "| --- | --- | --- | --- |")?;
            let tally = &tempo.tally;
            writeln!(
                formatter,
                "| {product} | {} | {} | {} |\n",
                tally.percent(tally.accurate),
                tally.percent(tally.accurate_with_octaves),
                tally.percent(tally.missing),
            )?;
            writeln!(
                formatter,
                "Accuracy 1 counts an estimate within 4% of the annotated tempo. Accuracy 2 also accepts 2, 1/2, 3 and 1/3 times the annotated tempo, as in the standard tempo evaluation.\n"
            )?;
            write_track_lists(formatter, tempo)?;
        }
        Ok(())
    }
}

fn write_coverage<Scores>(
    formatter: &mut Formatter<'_>,
    scored: &Scored<Scores>,
    value_name: &str,
) -> fmt::Result {
    let found = scored.track_count - scored.absent.len();
    writeln!(
        formatter,
        "{} tracks in the dataset, {found} found in the export. {} have an empty {value_name}, {} are not in the export and {} have a value that could not be scored; all of them count as no {value_name}.\n",
        scored.track_count,
        scored.empty_count,
        scored.absent.len(),
        scored.problems.len(),
    )
}

fn write_track_lists<Scores>(
    formatter: &mut Formatter<'_>,
    scored: &Scored<Scores>,
) -> fmt::Result {
    if !scored.absent.is_empty() {
        writeln!(formatter, "Tracks not in the export:\n")?;
        writeln!(formatter, "{}\n", scored.absent.join(", "))?;
    }
    if !scored.problems.is_empty() {
        writeln!(formatter, "Tracks with a value that could not be scored:\n")?;
        for line in group_failures(&scored.problems) {
            match line {
                FailureLine::Track(failure) => {
                    writeln!(formatter, "- {}: {}", failure.track, failure.reason)?;
                }
                FailureLine::Grouped {
                    reason,
                    track_count,
                } => writeln!(formatter, "- {track_count} tracks: {reason}")?,
            }
        }
        writeln!(formatter)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::dataset::Track;

    use super::*;

    const EXPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <PRODUCT Name="rekordbox" Version="7.2.19" Company="AlphaTheta"/>
  <COLLECTION Entries="9">
    <TRACK TrackID="1" Name="exact" AverageBpm="128.00" Tonality="Am" Location="file://localhost/Users/dj/kit/key/exact.LOFI.mp3"/>
    <TRACK TrackID="2" Name="camelot fifth" AverageBpm="0.00" Tonality="9A" Location="file://localhost/Users/dj/kit/key/fifth.LOFI.mp3"/>
    <TRACK TrackID="3" Name="relative" Tonality="C" Location="file://localhost/Volumes/My%20Disk/k%C3%AFt/key/relative.LOFI.mp3"/>
    <TRACK TrackID="4" Name="empty" AverageBpm="0.00" Tonality="" Location="file://localhost/Users/dj/kit/key/empty.LOFI.mp3"/>
    <TRACK TrackID="5" Name="unreadable" Tonality="H#m" Location="file://localhost/Users/dj/kit/key/unreadable.LOFI.mp3"/>
    <TRACK TrackID="6" Name="shared, key copy" AverageBpm="120.00" Tonality="F#m" Location="file://localhost/Users/dj/kit/key/shared.LOFI.mp3"/>
    <TRACK TrackID="7" Name="shared, tempo copy" AverageBpm="60.00" Tonality="Ebm" Location="file://localhost/Users/dj/kit/tempo/shared.LOFI.mp3"/>
    <TRACK TrackID="8" Name="double" AverageBpm="250.00" Tonality="1A" Location="file://localhost/Users/dj/kit/tempo/double.LOFI.mp3"/>
    <TRACK TrackID="9" Name="no location" Tonality="8A"/>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="1">
      <NODE Name="key" Type="1" KeyType="0" Entries="1">
        <TRACK Key="1"/>
      </NODE>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>
"#;

    fn dataset<Annotation>(tracks: Vec<(&str, Annotation)>) -> Dataset<Annotation> {
        Dataset {
            tracks: tracks
                .into_iter()
                .map(|(name, annotation)| Track {
                    name: name.to_string(),
                    audio_path: PathBuf::from(format!("bench/data/somewhere/audio/{name}.mp3")),
                    annotation,
                })
                .collect(),
            unannotated_count: 0,
        }
    }

    fn key(text: &str) -> Key {
        text.parse().unwrap()
    }

    #[test]
    fn decodes_locations_with_or_without_localhost() {
        assert_eq!(
            location_path("file://localhost/Volumes/My%20Disk/k%C3%AFt/a%25b.mp3"),
            "/Volumes/My Disk/k\u{EF}t/a%b.mp3"
        );
        assert_eq!(location_path("file:///Users/dj/a.mp3"), "/Users/dj/a.mp3");
        assert_eq!(
            location_path("file://localhost/C:/Music/a%2.mp3"),
            "/C:/Music/a%2.mp3"
        );
    }

    #[test]
    fn reads_every_notation_rekordbox_can_write() {
        let expectations = [
            ("Am", "A minor"),
            ("F#m", "F# minor"),
            ("Ebm", "Eb minor"),
            ("Db", "Db major"),
            ("Bb", "Bb major"),
            ("B", "B major"),
            ("C\u{266F}m", "C# minor"),
            ("8A", "A minor"),
            ("08B", "C major"),
            ("12A", "Db minor"),
            ("1m", "A minor"),
            (" A minor ", "A minor"),
        ];
        for (text, expected) in expectations {
            assert_eq!(parse_tonality(text), Ok(Some(key(expected))), "{text}");
        }
        assert_eq!(parse_tonality(""), Ok(None));
        assert_eq!(parse_tonality("  "), Ok(None));
        for text in ["H", "Amaj7", "13A", "A#b", "m"] {
            assert!(parse_tonality(text).is_err(), "{text:?} should not parse");
        }
    }

    #[test]
    fn reads_tempo_and_takes_zero_as_none() {
        assert_eq!(parse_average_bpm("127.95"), Ok(Some(127.95)));
        assert_eq!(parse_average_bpm("0.00"), Ok(None));
        assert_eq!(parse_average_bpm(""), Ok(None));
        assert!(parse_average_bpm("fast").is_err());
        assert!(parse_average_bpm("-1").is_err());
    }

    #[test]
    fn reads_the_product_and_every_collection_track() {
        let collection = Collection::parse(EXPORT).unwrap();
        assert_eq!(collection.product, "rekordbox 7.2.19");
        assert_eq!(collection.track_count, 9);
        assert_eq!(
            collection.lookup("relative.LOFI.mp3", KEY_FOLDER),
            Lookup::Found(&Entry {
                path: "/Volumes/My Disk/k\u{EF}t/key/relative.LOFI.mp3".to_string(),
                folder: "key".to_string(),
                tonality: "C".to_string(),
                average_bpm: String::new(),
            })
        );
    }

    #[test]
    fn rejects_a_document_that_is_not_an_export() {
        assert!(Collection::parse("<NML/>").is_err());
        assert!(Collection::parse("<DJ_PLAYLISTS/>").is_err());
        assert!(Collection::parse("not xml").is_err());
    }

    #[test]
    fn scores_keys_like_the_detectors() {
        let collection = Collection::parse(EXPORT).unwrap();
        let dataset = dataset(vec![
            ("exact.LOFI", key("A minor")),
            ("fifth.LOFI", key("A minor")),
            ("relative.LOFI", key("A minor")),
            ("empty.LOFI", key("A minor")),
            ("unreadable.LOFI", key("A minor")),
            ("shared.LOFI", key("F# minor")),
            ("absent.LOFI", key("A minor")),
        ]);

        let scored = score_keys(&dataset, &collection);

        let tally = &scored.tally;
        assert_eq!(
            (tally.exact, tally.fifth, tally.relative, tally.parallel),
            (2, 1, 1, 0)
        );
        assert_eq!((tally.other, tally.missing, tally.total()), (0, 3, 7));
        assert!((tally.mirex_score() - (2.0 + 0.5 + 0.3) / 7.0).abs() < 1.0e-9);
        assert_eq!(scored.empty_count, 1);
        assert_eq!(scored.absent, ["absent.LOFI"]);
        assert_eq!(
            scored.problems,
            [Failure {
                track: "unreadable.LOFI".to_string(),
                reason: "Tonality \"H#m\" is not a key".to_string(),
            }]
        );
    }

    #[test]
    fn scores_tempos_like_the_detectors() {
        let collection = Collection::parse(EXPORT).unwrap();
        let dataset = dataset(vec![
            ("exact.LOFI", 125.0),
            ("shared.LOFI", 120.0),
            ("double.LOFI", 125.0),
            ("empty.LOFI", 120.0),
        ]);

        let scored = score_tempos(&dataset, &collection);

        assert_eq!(scored.tally.accurate, 1);
        assert_eq!(scored.tally.accurate_with_octaves, 3);
        assert_eq!(scored.tally.missing, 1);
        assert_eq!(scored.empty_count, 1);
        assert!(scored.absent.is_empty() && scored.problems.is_empty());
    }

    #[test]
    fn a_name_listed_twice_takes_the_copy_in_the_dataset_folder() {
        let collection = Collection::parse(EXPORT).unwrap();
        let Lookup::Found(key_copy) = collection.lookup("shared.LOFI.mp3", KEY_FOLDER) else {
            panic!("the key copy should be found");
        };
        let Lookup::Found(tempo_copy) = collection.lookup("shared.LOFI.mp3", TEMPO_FOLDER) else {
            panic!("the tempo copy should be found");
        };
        assert_eq!(key_copy.tonality, "F#m");
        assert_eq!(tempo_copy.tonality, "Ebm");
    }

    #[test]
    fn a_name_listed_twice_elsewhere_with_different_values_is_not_scored() {
        let export = EXPORT
            .replace("/kit/key/shared", "/one/shared")
            .replace("/kit/tempo/shared", "/two/shared");
        let collection = Collection::parse(&export).unwrap();
        assert_eq!(
            collection.lookup("shared.LOFI.mp3", KEY_FOLDER),
            Lookup::Ambiguous(vec![
                "/Users/dj/one/shared.LOFI.mp3",
                "/Users/dj/two/shared.LOFI.mp3"
            ])
        );

        let scored = score_keys(
            &dataset(vec![("shared.LOFI", key("F# minor"))]),
            &collection,
        );
        assert_eq!(scored.tally.missing, 1);
        assert!(scored.problems[0].reason.starts_with("listed 2 times"));
    }

    #[test]
    fn renders_the_tables_of_the_detector_reports() {
        let collection = Collection::parse(EXPORT).unwrap();
        let report = RekordboxReport {
            collection: &collection,
            key: Some(score_keys(
                &dataset(vec![
                    ("exact.LOFI", key("A minor")),
                    ("absent.LOFI", key("A minor")),
                ]),
                &collection,
            )),
            tempo: Some(score_tempos(
                &dataset(vec![("double.LOFI", 125.0)]),
                &collection,
            )),
        };

        let expected = "\
Values read from a collection exported by rekordbox 7.2.19, which holds 9 tracks.

## Key detection, GiantSteps Key

2 tracks in the dataset, 1 found in the export. 0 have an empty key, 1 are not in the export and 0 have a value that could not be scored; all of them count as no key.

| Detector | Exact | Fifth | Relative | Parallel | Other | No key | MIREX score |
| --- | --- | --- | --- | --- | --- | --- | --- |
| rekordbox 7.2.19 | 50.0% | 0.0% | 0.0% | 0.0% | 0.0% | 50.0% | 50.0% |

Tracks not in the export:

absent.LOFI

## Tempo detection, GiantSteps Tempo

1 tracks in the dataset, 1 found in the export. 0 have an empty tempo, 0 are not in the export and 0 have a value that could not be scored; all of them count as no tempo.

| Detector | Accuracy 1 | Accuracy 2 | No tempo |
| --- | --- | --- | --- |
| rekordbox 7.2.19 | 0.0% | 100.0% | 0.0% |

Accuracy 1 counts an estimate within 4% of the annotated tempo. Accuracy 2 also accepts 2, 1/2, 3 and 1/3 times the annotated tempo, as in the standard tempo evaluation.

";
        assert_eq!(report.to_string(), expected);
    }

    #[test]
    fn parses_the_command_line() {
        let parse = |arguments: &[&str]| {
            parse_arguments(arguments.iter().map(|argument| argument.to_string()))
        };
        assert_eq!(
            parse(&["export.xml"]).unwrap(),
            Arguments {
                collection_file: PathBuf::from("export.xml"),
                data_directory: PathBuf::from("bench/data"),
                score_key: true,
                score_tempo: true,
            }
        );
        let only_tempo = parse(&["--only", "tempo", "export.xml", "elsewhere"]).unwrap();
        assert_eq!(only_tempo.data_directory, PathBuf::from("elsewhere"));
        assert!(!only_tempo.score_key && only_tempo.score_tempo);
        for arguments in [
            &[][..],
            &["export.xml", "one", "two"],
            &["export.xml", "--only", "energy"],
            &["export.xml", "--fast"],
        ] {
            assert!(parse(arguments).is_err(), "{arguments:?} should not parse");
        }
    }

    #[test]
    fn repeated_only_flags_cannot_disable_both_score_sections() {
        for (first, second) in [
            ("key", "tempo"),
            ("tempo", "key"),
            ("key", "key"),
            ("tempo", "tempo"),
        ] {
            let error = parse_arguments(
                ["export.xml", "--only", first, "--only", second].map(str::to_string),
            )
            .unwrap_err();
            assert!(error.to_string().contains("--only may be given only once"));
        }
    }
}
