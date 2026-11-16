// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Measures, per genre, how the beat grid rates and how precise the tempo it
//! gives is: the share of tracks whose tempo is within 0.05 BPM of the
//! reference, Accuracy 1 and 2, and on GTZAN how well the grid's beats fit
//! the annotated ones.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use euphoria_core::detect::{StratumDetector, detect_beat_grid};
use euphoria_core::{Confidence, TempoRange, decode_mono};
use rayon::prelude::*;

use crate::beats::{RIGHT_F_MEASURE, annotated_tempo, f_measure, times_within};
use crate::dataset;
use crate::scoring::within_tolerance;

pub const USAGE: &str = "usage: euphoria-bench grid-tempo [GIANTSTEPS_TEMPO_DIRECTORY] [--gtzan GTZAN_DIRECTORY] [--dump FILE]";

/// Genres with fewer tracks than this are reported together.
const MINIMUM_GENRE_TRACKS: usize = 20;
/// A tempo this close to the reference, in beats per minute, is precise.
const PRECISE_BPM: f32 = 0.05;

#[derive(Debug, PartialEq)]
pub struct Arguments {
    giantsteps_directory: PathBuf,
    gtzan_directory: Option<PathBuf>,
    dump: Option<PathBuf>,
}

pub fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Arguments> {
    let mut giantsteps_directory = None;
    let mut gtzan_directory = None;
    let mut dump = None;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "--gtzan" || argument == "--dump" {
            let value = arguments
                .next()
                .with_context(|| format!("{argument} needs a path\n{USAGE}"))?;
            if argument == "--gtzan" {
                gtzan_directory = Some(PathBuf::from(value));
            } else {
                dump = Some(PathBuf::from(value));
            }
        } else if argument.starts_with("--") {
            bail!("unknown option {argument:?}\n{USAGE}");
        } else if giantsteps_directory.is_some() {
            bail!("more than one data directory given\n{USAGE}");
        } else {
            giantsteps_directory = Some(PathBuf::from(argument));
        }
    }
    Ok(Arguments {
        giantsteps_directory: giantsteps_directory
            .unwrap_or_else(|| PathBuf::from("bench/data/tempo")),
        gtzan_directory,
        dump,
    })
}

/// A track with its reference tempo, its genre, and for GTZAN its
/// annotated beats.
struct Reference {
    name: String,
    audio_path: PathBuf,
    tempo: f32,
    genre: String,
    beats: Option<Vec<f64>>,
}

fn giantsteps_references(directory: &Path) -> Result<Vec<Reference>> {
    dataset::load_tempo_tracks(directory)?
        .tracks
        .into_iter()
        .map(|track| {
            let genre_path = directory
                .join("annotations/genre")
                .join(format!("{}.genre", track.name));
            let genre = std::fs::read_to_string(&genre_path)
                .with_context(|| format!("reading {}", genre_path.display()))?;
            Ok(Reference {
                name: track.name,
                audio_path: track.audio_path,
                tempo: track.annotation,
                genre: genre.trim().to_string(),
                beats: None,
            })
        })
        .collect()
}

/// The GTZAN excerpts in `directory`, of any genres: an annotation
/// `annotations/gtzan_<genre>_<number>.beats` with its audio
/// `audio/<genre>.<number>.wav`. The reference tempo is the median beat
/// interval.
fn gtzan_references(directory: &Path) -> Result<Vec<Reference>> {
    let annotations = directory.join("annotations");
    let mut references = Vec::new();
    for entry in std::fs::read_dir(&annotations)
        .with_context(|| format!("reading {}", annotations.display()))?
    {
        let path = entry?.path();
        let Some(name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".beats"))
        else {
            continue;
        };
        let Some((genre, number)) = name
            .strip_prefix("gtzan_")
            .and_then(|rest| rest.split_once('_'))
        else {
            continue;
        };
        let audio_path = directory
            .join("audio")
            .join(format!("{genre}.{number}.wav"));
        if !audio_path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let beats = beat_times(&text).with_context(|| format!("reading {}", path.display()))?;
        if beats.len() < 2 {
            continue;
        }
        references.push(Reference {
            name: name.to_string(),
            audio_path,
            tempo: annotated_tempo(&beats) as f32,
            genre: genre.to_string(),
            beats: Some(beats),
        });
    }
    references.sort_by(|first, second| first.name.cmp(&second.name));
    Ok(references)
}

/// The beat times of an annotation, one per line in the first column. Some
/// GTZAN annotations give no position in the bar, so further columns are
/// not read.
fn beat_times(text: &str) -> Result<Vec<f64>> {
    let mut times: Vec<f64> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let Some(field) = line.split_whitespace().next() else {
            continue;
        };
        let time: f64 = field
            .parse()
            .ok()
            .filter(|time: &f64| time.is_finite() && *time >= 0.0)
            .with_context(|| format!("line {}: {field:?} is not a time", index + 1))?;
        if times.last().is_some_and(|previous| time <= *previous) {
            bail!(
                "line {}: {time} does not come after the beat before it",
                index + 1
            );
        }
        times.push(time);
    }
    Ok(times)
}

/// What the analysis made of one track.
struct Outcome {
    confidence: Option<Confidence>,
    tempo: f32,
    beat_f_measure: Option<f64>,
}

/// The grid and tempo of one track, or `None` when it cannot be decoded or
/// has no tempo.
fn measure(reference: &Reference) -> Result<Option<Outcome>> {
    let Ok(audio) = decode_mono(&reference.audio_path) else {
        return Ok(None);
    };
    let Some(reading) = StratumDetector
        .read_tempo(&audio)
        .map_err(|error| anyhow::anyhow!(error.chain()))?
    else {
        return Ok(None);
    };
    let grid = detect_beat_grid(&audio, f64::from(reading.tempo_in(TempoRange::DEFAULT)));
    let tempo = TempoRange::DEFAULT.fold(reading.refined_by(grid.as_ref()));
    let beat_f_measure = reference.beats.as_ref().map(|beats| {
        grid.map_or(0.0, |grid| {
            let first = beats[0];
            let last = beats[beats.len() - 1];
            let predicted = times_within(
                grid.first_downbeat_seconds,
                grid.beat_seconds(),
                first,
                last,
            );
            f_measure(&predicted, beats)
        })
    });
    Ok(Some(Outcome {
        confidence: grid.map(|grid| grid.confidence()),
        tempo,
        beat_f_measure,
    }))
}

#[derive(Debug, Default, Clone)]
struct Tally {
    tracks: usize,
    levels: [usize; 3],
    precise: usize,
    accurate: usize,
    accurate_with_octaves: usize,
    f_measures: Vec<f64>,
}

impl Tally {
    fn record(&mut self, reference: &Reference, outcome: &Outcome) {
        self.tracks += 1;
        match outcome.confidence {
            Some(Confidence::High) => self.levels[0] += 1,
            Some(Confidence::Medium) => self.levels[1] += 1,
            Some(Confidence::Low) => self.levels[2] += 1,
            None => {}
        }
        self.precise += usize::from((outcome.tempo - reference.tempo).abs() <= PRECISE_BPM);
        self.accurate += usize::from(within_tolerance(reference.tempo, outcome.tempo));
        self.accurate_with_octaves += usize::from(
            [1.0, 2.0, 0.5, 3.0, 1.0 / 3.0]
                .iter()
                .any(|factor| within_tolerance(reference.tempo * factor, outcome.tempo)),
        );
        if let Some(f_measure) = outcome.beat_f_measure {
            self.f_measures.push(f_measure);
        }
    }

    fn add(&mut self, other: &Self) {
        self.tracks += other.tracks;
        for (level, count) in self.levels.iter_mut().zip(other.levels) {
            *level += count;
        }
        self.precise += other.precise;
        self.accurate += other.accurate;
        self.accurate_with_octaves += other.accurate_with_octaves;
        self.f_measures.extend(&other.f_measures);
    }
}

fn share(count: usize, total: usize) -> String {
    if total == 0 {
        return "-".to_string();
    }
    format!("{:.1}%", 100.0 * count as f64 / total as f64)
}

fn render(title: &str, references: &[Reference], outcomes: &[Option<Outcome>]) -> String {
    let mut by_genre: BTreeMap<&str, Tally> = BTreeMap::new();
    let mut missing = 0;
    for (reference, outcome) in references.iter().zip(outcomes) {
        match outcome {
            Some(outcome) => by_genre
                .entry(&reference.genre)
                .or_default()
                .record(reference, outcome),
            None => missing += 1,
        }
    }
    let mut rows: Vec<(String, Tally)> = Vec::new();
    let mut grouped = Tally::default();
    let mut grouped_count = 0;
    let mut total = Tally::default();
    for (genre, tally) in &by_genre {
        total.add(tally);
        if tally.tracks >= MINIMUM_GENRE_TRACKS {
            rows.push((genre.to_string(), tally.clone()));
        } else {
            grouped.add(tally);
            grouped_count += 1;
        }
    }
    rows.sort_by_key(|(_, tally)| std::cmp::Reverse(tally.tracks));
    if grouped_count > 0 {
        rows.push((format!("{grouped_count} other genres"), grouped));
    }
    rows.push(("all".to_string(), total));
    let with_beats = rows.iter().any(|(_, tally)| !tally.f_measures.is_empty());
    let mut report = String::new();
    let _ = writeln!(report, "## {title}\n");
    let _ = writeln!(
        report,
        "{} tracks with a tempo, {missing} without a tempo or not decodable. The tempo is the one Euphoria reports, in the default range.\n",
        rows.last().map_or(0, |(_, tally)| tally.tracks)
    );
    let beat_header = if with_beats {
        " Beat F-measure | Grid right |"
    } else {
        ""
    };
    let beat_rule = if with_beats { " --- | --- |" } else { "" };
    let _ = writeln!(
        report,
        "| Genre | Tracks | Grid high | Medium | Low | Within 0.05 BPM | Accuracy 1 | Accuracy 2 |{beat_header}"
    );
    let _ = writeln!(
        report,
        "| --- | --- | --- | --- | --- | --- | --- | --- |{beat_rule}"
    );
    for (genre, tally) in rows {
        let mut line = format!(
            "| {genre} | {} | {} | {} | {} | {} | {} | {} |",
            tally.tracks,
            tally.levels[0],
            tally.levels[1],
            tally.levels[2],
            share(tally.precise, tally.tracks),
            share(tally.accurate, tally.tracks),
            share(tally.accurate_with_octaves, tally.tracks),
        );
        if with_beats {
            let count = tally.f_measures.len();
            let mean = tally.f_measures.iter().sum::<f64>() / count.max(1) as f64;
            let right = tally
                .f_measures
                .iter()
                .filter(|f_measure| **f_measure >= RIGHT_F_MEASURE)
                .count();
            let _ = write!(line, " {mean:.3} | {} |", share(right, count));
        }
        let _ = writeln!(report, "{line}");
    }
    report.push('\n');
    report
}

fn dump_lines(references: &[Reference], outcomes: &[Option<Outcome>]) -> String {
    let mut text = String::new();
    for (reference, outcome) in references.iter().zip(outcomes) {
        let Some(outcome) = outcome else { continue };
        let _ = writeln!(
            text,
            "{}\t{}\t{}\t{}\t{}",
            reference.name,
            reference.genre,
            outcome
                .confidence
                .map_or("none".to_string(), |level| level.to_string()),
            outcome.tempo,
            outcome
                .beat_f_measure
                .map_or("-".to_string(), |f_measure| format!("{f_measure:.4}")),
        );
    }
    text
}

fn evaluate(references: &[Reference]) -> Result<Vec<Option<Outcome>>> {
    references
        .par_iter()
        .map(|reference| {
            measure(reference)
                .with_context(|| format!("measuring {}", reference.audio_path.display()))
        })
        .collect()
}

pub fn run(arguments: &Arguments) -> Result<()> {
    let mut dump = String::new();
    let giantsteps = giantsteps_references(&arguments.giantsteps_directory)?;
    let outcomes = evaluate(&giantsteps)?;
    print!(
        "{}",
        render(
            "Beat grid and tempo on GiantSteps Tempo",
            &giantsteps,
            &outcomes
        )
    );
    dump.push_str(&dump_lines(&giantsteps, &outcomes));
    if let Some(directory) = &arguments.gtzan_directory {
        let gtzan = gtzan_references(directory)?;
        let outcomes = evaluate(&gtzan)?;
        print!(
            "{}",
            render("Beat grid and tempo on GTZAN", &gtzan, &outcomes)
        );
        dump.push_str(&dump_lines(&gtzan, &outcomes));
    }
    if let Some(path) = &arguments.dump {
        std::fs::write(path, dump).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Result<Arguments> {
        parse_arguments(arguments.iter().map(|argument| argument.to_string()))
    }

    #[test]
    fn beat_times_read_the_first_column_with_or_without_a_position() {
        assert_eq!(beat_times("0.5 1\n1.0 2\n").unwrap(), [0.5, 1.0]);
        assert_eq!(beat_times("0.144\n0.6\n\n").unwrap(), [0.144, 0.6]);
        assert!(beat_times("1.0\n0.5\n").is_err());
        assert!(beat_times("soon\n").is_err());
    }

    #[test]
    fn reads_the_directories_and_the_dump_file() {
        let arguments = parse(&["giantsteps", "--gtzan", "gtzan", "--dump", "out.tsv"]).unwrap();
        assert_eq!(arguments.giantsteps_directory, PathBuf::from("giantsteps"));
        assert_eq!(arguments.gtzan_directory, Some(PathBuf::from("gtzan")));
        assert_eq!(arguments.dump, Some(PathBuf::from("out.tsv")));
        assert_eq!(
            parse(&[]).unwrap().giantsteps_directory,
            PathBuf::from("bench/data/tempo")
        );
        for bad in [&["--gtzan"][..], &["--dump"], &["--fast"], &["a", "b"]] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }
}
