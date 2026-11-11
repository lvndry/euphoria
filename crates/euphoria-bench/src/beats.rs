// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Measures the beat grid against beat and downbeat annotations: the GTZAN
//! disco and hip hop excerpts that `bench/download_beat_dataset.sh` fetches.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use euphoria_core::detect::{BeatGrid, StratumDetector, detect_beat_grid};
use euphoria_core::{Confidence, TempoRange, decode_mono};
use rayon::prelude::*;

pub const USAGE: &str =
    "usage: euphoria-bench beats [DATA_DIRECTORY] [--tracks all|development|held-out]";

/// A beat counts as found within this many seconds of an annotated one, as
/// in the standard beat evaluation.
const WINDOW_SECONDS: f64 = 0.07;
/// A grid or a choice of downbeat counts as right when its F-measure
/// reaches this.
const RIGHT_F_MEASURE: f64 = 0.9;

/// Which excerpts a run covers, every other one in name order for a half.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Half {
    All,
    Development,
    HeldOut,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Arguments {
    data_directory: PathBuf,
    half: Half,
}

pub fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Arguments> {
    let mut data_directory = None;
    let mut half = Half::All;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "--tracks" {
            let value = arguments
                .next()
                .with_context(|| format!("--tracks needs a value\n{USAGE}"))?;
            half = match value.as_str() {
                "all" => Half::All,
                "development" => Half::Development,
                "held-out" => Half::HeldOut,
                _ => bail!("--tracks accepts all, development or held-out, not {value:?}\n{USAGE}"),
            };
        } else if argument.starts_with("--") {
            bail!("unknown option {argument:?}\n{USAGE}");
        } else if data_directory.is_some() {
            bail!("more than one data directory given\n{USAGE}");
        } else {
            data_directory = Some(PathBuf::from(argument));
        }
    }
    Ok(Arguments {
        data_directory: data_directory.unwrap_or_else(|| PathBuf::from("bench/data/beats")),
        half,
    })
}

/// One annotated excerpt.
#[derive(Debug)]
struct Excerpt {
    name: String,
    audio_path: PathBuf,
    beats: Vec<f64>,
    downbeats: Vec<f64>,
}

/// How a grid did on one excerpt.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Score {
    beat_f_measure: f64,
    downbeat_f_measure: f64,
    confidence: Option<Confidence>,
}

impl Score {
    fn grid_right(&self) -> bool {
        self.beat_f_measure >= RIGHT_F_MEASURE
    }

    fn downbeat_right(&self) -> bool {
        self.downbeat_f_measure >= RIGHT_F_MEASURE
    }
}

/// Reads an annotation file of one beat per line: its time in seconds and
/// its position in the bar, where 1 is the downbeat. Times must be finite,
/// not negative and increasing, and every line must have exactly these two
/// fields.
fn read_annotation(path: &Path) -> Result<(Vec<f64>, Vec<f64>)> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse_annotation(&text).with_context(|| format!("reading {}", path.display()))
}

fn parse_annotation(text: &str) -> Result<(Vec<f64>, Vec<f64>)> {
    let mut beats: Vec<f64> = Vec::new();
    let mut downbeats = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let number = index + 1;
        let fields: Vec<&str> = line.split_whitespace().collect();
        let [time, position] = fields[..] else {
            bail!("line {number}: expected a time and a position in the bar, found {line:?}");
        };
        let time: f64 = time
            .parse()
            .ok()
            .filter(|time: &f64| time.is_finite() && *time >= 0.0)
            .with_context(|| format!("line {number}: {time:?} is not a time in seconds"))?;
        let position: u32 = position
            .parse()
            .ok()
            .filter(|position| (1..=BEATS_PER_BAR).contains(position))
            .with_context(|| {
                format!("line {number}: {position:?} is not a position in a bar of 4/4")
            })?;
        if beats.last().is_some_and(|previous| time <= *previous) {
            bail!("line {number}: {time} does not come after the beat before it");
        }
        beats.push(time);
        if position == 1 {
            downbeats.push(time);
        }
    }
    Ok((beats, downbeats))
}

/// The excerpts, sorted by name: an annotation `gtzan_disco_00007.beats`
/// belongs to `disco.00007.wav`. Every annotation needs its audio and every
/// audio file its annotation, with at least two beats, so that a damaged
/// download cannot shrink the set without notice.
fn load_excerpts(data_directory: &Path) -> Result<Vec<Excerpt>> {
    let annotation_directory = data_directory.join("annotations");
    let audio_directory = data_directory.join("audio");
    let listing = |directory: &Path| -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(directory).with_context(|| {
            format!(
                "reading {}; run bench/download_beat_dataset.sh first",
                directory.display()
            )
        })? {
            paths.push(entry?.path());
        }
        Ok(paths)
    };
    let mut excerpts = Vec::new();
    for path in listing(&annotation_directory)? {
        if path.extension().and_then(|extension| extension.to_str()) != Some("beats") {
            continue;
        }
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .with_context(|| format!("{} has no usable name", path.display()))?;
        let rest = name
            .strip_prefix("gtzan_")
            .with_context(|| format!("{} is not a GTZAN annotation", path.display()))?;
        let audio_path = audio_directory.join(format!("{}.wav", rest.replacen('_', ".", 1)));
        if !audio_path.is_file() {
            bail!(
                "{} has no audio at {}; run bench/download_beat_dataset.sh again",
                path.display(),
                audio_path.display()
            );
        }
        let (beats, downbeats) = read_annotation(&path)?;
        if beats.len() < 2 {
            bail!("{} has fewer than two beats", path.display());
        }
        excerpts.push(Excerpt {
            name: name.to_string(),
            audio_path,
            beats,
            downbeats,
        });
    }
    for path in listing(&audio_directory)? {
        if path.extension().and_then(|extension| extension.to_str()) != Some("wav") {
            continue;
        }
        if !excerpts.iter().any(|excerpt| excerpt.audio_path == path) {
            bail!("{} has no annotation", path.display());
        }
    }
    excerpts.sort_by(|first, second| first.name.cmp(&second.name));
    if excerpts.is_empty() {
        bail!("no beat dataset found, run bench/download_beat_dataset.sh first");
    }
    Ok(excerpts)
}

/// The genres of GTZAN that are measured, each with excerpts numbered from
/// 0 to [`EXCERPTS_PER_GENRE`] - 1.
const GENRES: [&str; 2] = ["disco", "hiphop"];
const EXCERPTS_PER_GENRE: usize = 100;
/// Positions in a bar of 4/4, the metre the grid assumes.
const BEATS_PER_BAR: u32 = 4;

/// Fails unless `excerpts` are exactly the 200 the benchmark is defined on,
/// so that a download missing both halves of some pairs cannot pass for
/// the whole set.
fn check_manifest(excerpts: &[Excerpt]) -> Result<()> {
    let expected: Vec<String> = GENRES
        .iter()
        .flat_map(|genre| {
            (0..EXCERPTS_PER_GENRE).map(move |number| format!("gtzan_{genre}_{number:05}"))
        })
        .collect();
    let found: Vec<&str> = excerpts
        .iter()
        .map(|excerpt| excerpt.name.as_str())
        .collect();
    let missing: Vec<&String> = expected
        .iter()
        .filter(|name| !found.contains(&name.as_str()))
        .collect();
    let unexpected: Vec<&&str> = found
        .iter()
        .filter(|name| !expected.iter().any(|expected| expected == **name))
        .collect();
    if !missing.is_empty() || !unexpected.is_empty() {
        bail!(
            "the beat dataset is not the expected 200 excerpts: {} missing (first {:?}), {} unexpected (first {:?}); run bench/download_beat_dataset.sh again",
            missing.len(),
            missing.first(),
            unexpected.len(),
            unexpected.first()
        );
    }
    Ok(())
}

/// The share of annotated times found by predicted ones and of predicted
/// times that match an annotated one, combined: the standard beat F-measure.
fn f_measure(predicted: &[f64], reference: &[f64]) -> f64 {
    let hits = matched_count(predicted, reference);
    if hits == 0 {
        return 0.0;
    }
    let precision = hits as f64 / predicted.len() as f64;
    let recall = hits as f64 / reference.len() as f64;
    2.0 * precision * recall / (precision + recall)
}

/// The most pairs of one predicted and one annotated time within the window
/// of each other, each time used at most once. Both lists are in order, so
/// matching every annotated time to the earliest unused prediction it can
/// still reach finds the largest matching, as the bipartite matching of the
/// standard evaluation does.
fn matched_count(predicted: &[f64], reference: &[f64]) -> usize {
    let mut next_prediction = 0;
    let mut hits = 0;
    for time in reference {
        while next_prediction < predicted.len()
            && predicted[next_prediction] < time - WINDOW_SECONDS
        {
            next_prediction += 1;
        }
        if next_prediction < predicted.len() && predicted[next_prediction] <= time + WINDOW_SECONDS
        {
            hits += 1;
            next_prediction += 1;
        }
    }
    hits
}

/// The times from `start` every `step` seconds that fall within the span
/// of the annotated beats, widened by the window.
fn times_within(start: f64, step: f64, first: f64, last: f64) -> Vec<f64> {
    let earliest = first - WINDOW_SECONDS;
    let latest = last + WINDOW_SECONDS;
    let mut time = start - ((start - earliest) / step).ceil().max(0.0) * step;
    let mut times = Vec::new();
    while time <= latest {
        if time >= earliest {
            times.push(time);
        }
        time += step;
    }
    times
}

fn score(grid: Option<BeatGrid>, excerpt: &Excerpt) -> Score {
    let Some(grid) = grid else {
        return Score::default();
    };
    let first = excerpt.beats[0];
    let last = excerpt.beats[excerpt.beats.len() - 1];
    let beats = times_within(
        grid.first_downbeat_seconds,
        grid.beat_seconds(),
        first,
        last,
    );
    let downbeats = times_within(grid.first_downbeat_seconds, grid.bar_seconds(), first, last);
    Score {
        beat_f_measure: f_measure(&beats, &excerpt.beats),
        downbeat_f_measure: f_measure(&downbeats, &excerpt.downbeats),
        confidence: Some(grid.confidence()),
    }
}

/// The tempo of the annotated beats, from the median time between them.
fn annotated_tempo(beats: &[f64]) -> f64 {
    let mut intervals: Vec<f64> = beats.windows(2).map(|pair| pair[1] - pair[0]).collect();
    intervals.sort_by(f64::total_cmp);
    60.0 / intervals[intervals.len() / 2]
}

/// The octave of `estimate` nearest to `target`.
fn nearest_octave(estimate: f64, target: f64) -> f64 {
    let octaves = (target / estimate).log2().round();
    estimate * 2.0_f64.powf(octaves)
}

/// How the grid did on one excerpt: with the tempo as Euphoria reports it,
/// and with the octave of the tempo taken from the annotation, which leaves
/// the grid alone to be judged.
struct ExcerptResult {
    as_shipped: Score,
    octave_matched: Score,
}

/// How the grid did on one excerpt. An excerpt without a tempo, which gets
/// no grid, scores zero, so every rate covers the whole set.
fn evaluate(excerpt: &Excerpt) -> Result<ExcerptResult> {
    let audio = decode_mono(&excerpt.audio_path).map_err(|error| anyhow::anyhow!(error.chain()))?;
    let Some(reading) = StratumDetector
        .read_tempo(&audio)
        .map_err(|error| anyhow::anyhow!(error.chain()))?
    else {
        return Ok(ExcerptResult {
            as_shipped: Score::default(),
            octave_matched: Score::default(),
        });
    };
    let reported = f64::from(reading.tempo_in(TempoRange::DEFAULT));
    let matched = nearest_octave(f64::from(reading.estimate), annotated_tempo(&excerpt.beats));
    Ok(ExcerptResult {
        as_shipped: score(detect_beat_grid(&audio, reported), excerpt),
        octave_matched: score(detect_beat_grid(&audio, matched), excerpt),
    })
}

fn share(count: usize, total: usize) -> String {
    if total == 0 {
        return "-".to_string();
    }
    format!("{:.1}%", 100.0 * count as f64 / total as f64)
}

fn render(results: &[ExcerptResult], half: Half) -> String {
    let mut report = String::new();
    let _ = writeln!(report, "## Beat grid, GTZAN disco and hip hop\n");
    let covered = match half {
        Half::All => "All excerpts",
        Half::Development => {
            "Development half: every other excerpt in name order, starting with the first"
        }
        Half::HeldOut => {
            "Held out half: every other excerpt in name order, starting with the second"
        }
    };
    let without_grid = |pick: fn(&ExcerptResult) -> Score| {
        results
            .iter()
            .filter(|result| pick(result).confidence.is_none())
            .count()
    };
    let _ = writeln!(
        report,
        "{covered}. Every rate is out of all {} excerpts; {} got no grid with the tempo as reported and {} with the octave from the annotation, and count as missed.\n",
        results.len(),
        without_grid(|result| result.as_shipped),
        without_grid(|result| result.octave_matched),
    );
    let _ = writeln!(
        report,
        "| Tempo | Beat F-measure | Grid right | Downbeat right | Downbeat right when the grid is right |"
    );
    let _ = writeln!(report, "| --- | --- | --- | --- | --- |");
    let as_shipped: Vec<Score> = results.iter().map(|result| result.as_shipped).collect();
    let octave_matched: Vec<Score> = results.iter().map(|result| result.octave_matched).collect();
    let rows = [
        ("As reported, between 90 and 180 BPM", as_shipped),
        ("Octave taken from the annotation", octave_matched),
    ];
    for (label, scores) in rows {
        let mean = scores.iter().map(|score| score.beat_f_measure).sum::<f64>()
            / scores.len().max(1) as f64;
        let right: Vec<&Score> = scores.iter().filter(|score| score.grid_right()).collect();
        let _ = writeln!(
            report,
            "| {label} | {mean:.3} | {} | {} | {} |",
            share(right.len(), scores.len()),
            share(
                scores.iter().filter(|score| score.downbeat_right()).count(),
                scores.len()
            ),
            share(
                right.iter().filter(|score| score.downbeat_right()).count(),
                right.len()
            ),
        );
    }
    let _ = writeln!(
        report,
        "\nA grid is right when its beat F-measure is at least 0.9, with beats found within 70 ms; its downbeat is right when the F-measure of the downbeats alone is at least 0.9.\n"
    );
    let _ = writeln!(
        report,
        "| Grid confidence, octave from the annotation | Excerpts | Grid right |"
    );
    let _ = writeln!(report, "| --- | --- | --- |");
    for level in [Confidence::High, Confidence::Medium, Confidence::Low] {
        let chosen: Vec<Score> = results
            .iter()
            .map(|result| result.octave_matched)
            .filter(|score| score.confidence == Some(level))
            .collect();
        let _ = writeln!(
            report,
            "| {level} | {} ({}) | {} |",
            chosen.len(),
            share(chosen.len(), results.len()),
            share(
                chosen.iter().filter(|score| score.grid_right()).count(),
                chosen.len()
            ),
        );
    }
    let ungridded = without_grid(|result| result.octave_matched);
    let _ = writeln!(
        report,
        "| no grid | {ungridded} ({}) | - |",
        share(ungridded, results.len())
    );
    report
}

pub fn run(arguments: &Arguments) -> Result<()> {
    let excerpts = load_excerpts(&arguments.data_directory)?;
    check_manifest(&excerpts)?;
    let chosen: Vec<&Excerpt> = excerpts
        .iter()
        .enumerate()
        .filter(|(index, _)| match arguments.half {
            Half::All => true,
            Half::Development => index % 2 == 0,
            Half::HeldOut => index % 2 == 1,
        })
        .map(|(_, excerpt)| excerpt)
        .collect();
    let outcomes: Vec<Result<ExcerptResult>> =
        chosen.par_iter().map(|excerpt| evaluate(excerpt)).collect();
    let mut results = Vec::with_capacity(chosen.len());
    for (excerpt, outcome) in chosen.iter().zip(outcomes) {
        results.push(outcome.with_context(|| format!("measuring {}", excerpt.name))?);
    }
    print!("{}", render(&results, arguments.half));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_f_measure_counts_each_prediction_once_within_the_window() {
        assert_eq!(f_measure(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]), 1.0);
        assert_eq!(f_measure(&[1.05, 2.06], &[1.0, 2.0]), 1.0);
        assert_eq!(f_measure(&[1.1], &[1.0]), 0.0);
        let half_found = f_measure(&[1.0, 2.0, 3.0, 4.0], &[1.0, 3.0]);
        assert!((half_found - 2.0 / 3.0).abs() < 1e-12, "{half_found}");
        assert_eq!(f_measure(&[1.0], &[1.0, 1.01]), 2.0 / 3.0);
        assert_eq!(f_measure(&[], &[1.0]), 0.0);
    }

    #[test]
    fn grid_times_cover_the_annotated_span_on_both_sides_of_the_start() {
        assert_eq!(times_within(1.0, 0.5, 0.2, 2.0), vec![0.5, 1.0, 1.5, 2.0]);
        assert!(
            times_within(10.0, 1.0, 0.0, 2.0)
                .iter()
                .all(|time| *time <= 2.07)
        );
        assert_eq!(times_within(10.0, 1.0, 0.0, 2.0).len(), 3);
    }

    #[test]
    fn the_annotated_tempo_and_the_nearest_octave() {
        let beats: Vec<f64> = (0..20).map(|beat| f64::from(beat) * 0.5).collect();
        assert!((annotated_tempo(&beats) - 120.0).abs() < 1e-9);
        assert!((nearest_octave(61.0, 120.0) - 122.0).abs() < 1e-9);
        assert!((nearest_octave(170.0, 86.0) - 85.0).abs() < 1e-9);
    }

    #[test]
    fn parses_the_data_directory_and_the_half() {
        let parse = |arguments: &[&str]| {
            parse_arguments(arguments.iter().map(|argument| argument.to_string()))
        };
        assert_eq!(
            parse(&[]).unwrap(),
            Arguments {
                data_directory: PathBuf::from("bench/data/beats"),
                half: Half::All,
            }
        );
        assert_eq!(
            parse(&["--tracks", "held-out"]).unwrap().half,
            Half::HeldOut
        );
        assert!(parse(&["--tracks", "half"]).is_err());
        assert!(parse(&["--fast"]).is_err());
    }

    #[test]
    fn malformed_annotation_lines_are_errors() {
        for (text, message) in [
            ("NaN\t1\n", "is not a time"),
            ("inf\t1\n", "is not a time"),
            ("-0.5\t1\n", "is not a time"),
            ("0.5\n", "expected a time and a position"),
            ("0.5\t1\textra\n", "expected a time and a position"),
            ("0.5\tone\n", "is not a position"),
            ("0.5\t0\n", "is not a position"),
            ("0.5\t5\n", "is not a position in a bar of 4/4"),
            ("1.0\t1\n0.5\t2\n", "does not come after"),
        ] {
            let error = parse_annotation(text).unwrap_err().to_string();
            assert!(error.contains(message), "{text:?}: {error}");
        }
    }

    #[test]
    fn the_matching_finds_every_pair_a_nearest_first_matching_misses() {
        assert_eq!(matched_count(&[0.95, 1.02], &[1.00, 1.06]), 2);
        assert_eq!(f_measure(&[0.95, 1.02], &[1.00, 1.06]), 1.0);
        assert_eq!(matched_count(&[1.0, 1.01, 1.02], &[1.0]), 1);
        assert_eq!(matched_count(&[0.5, 2.0], &[1.0, 1.5]), 0);
    }

    fn data_folder(audio: &[&str], annotations: &[(&str, &str)]) -> PathBuf {
        let folder = std::env::temp_dir().join(format!(
            "euphoria-beats-data-{}-{}",
            std::process::id(),
            audio.len() * 10 + annotations.len()
        ));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("audio")).unwrap();
        std::fs::create_dir_all(folder.join("annotations")).unwrap();
        for name in audio {
            std::fs::write(folder.join("audio").join(name), b"").unwrap();
        }
        for (name, text) in annotations {
            std::fs::write(folder.join("annotations").join(name), text).unwrap();
        }
        folder
    }

    #[test]
    fn a_missing_or_short_half_of_a_pair_is_an_error() {
        let complete = data_folder(
            &["disco.00001.wav"],
            &[("gtzan_disco_00001.beats", "0.5\t1\n1.0\t2\n")],
        );
        assert_eq!(load_excerpts(&complete).unwrap().len(), 1);
        std::fs::remove_dir_all(&complete).unwrap();

        let without_audio = data_folder(&[], &[("gtzan_disco_00001.beats", "0.5\t1\n1.0\t2\n")]);
        let error = load_excerpts(&without_audio).unwrap_err().to_string();
        std::fs::remove_dir_all(&without_audio).unwrap();
        assert!(error.contains("has no audio"), "{error}");

        let without_annotation = data_folder(
            &["disco.00001.wav", "disco.00002.wav"],
            &[("gtzan_disco_00001.beats", "0.5\t1\n1.0\t2\n")],
        );
        let error = load_excerpts(&without_annotation).unwrap_err().to_string();
        std::fs::remove_dir_all(&without_annotation).unwrap();
        assert!(error.contains("has no annotation"), "{error}");

        let one_beat = data_folder(
            &["disco.00001.wav", "disco.00002.wav", "disco.00003.wav"],
            &[
                ("gtzan_disco_00001.beats", "0.5\t1\n1.0\t2\n"),
                ("gtzan_disco_00002.beats", "0.5\t1\n1.0\t2\n"),
                ("gtzan_disco_00003.beats", "0.5\t1\n"),
            ],
        );
        let error = load_excerpts(&one_beat).unwrap_err().to_string();
        std::fs::remove_dir_all(&one_beat).unwrap();
        assert!(error.contains("fewer than two beats"), "{error}");
    }

    #[test]
    fn only_the_full_set_of_excerpts_passes_the_manifest() {
        let excerpt = |name: String| Excerpt {
            name,
            audio_path: PathBuf::new(),
            beats: Vec::new(),
            downbeats: Vec::new(),
        };
        let mut excerpts: Vec<Excerpt> = GENRES
            .iter()
            .flat_map(|genre| (0..EXCERPTS_PER_GENRE).map(move |number| (genre, number)))
            .map(|(genre, number)| excerpt(format!("gtzan_{genre}_{number:05}")))
            .collect();
        assert!(check_manifest(&excerpts).is_ok());

        let removed = excerpts.remove(42);
        let error = check_manifest(&excerpts).unwrap_err().to_string();
        assert!(error.contains("1 missing"), "{error}");
        assert!(error.contains(&removed.name), "{error}");

        excerpts.push(excerpt("gtzan_rock_00001".to_string()));
        let error = check_manifest(&excerpts).unwrap_err().to_string();
        assert!(error.contains("1 unexpected"), "{error}");
    }

    #[test]
    fn excerpts_without_a_grid_count_in_every_rate() {
        let right = Score {
            beat_f_measure: 1.0,
            downbeat_f_measure: 1.0,
            confidence: Some(Confidence::High),
        };
        let results = [
            ExcerptResult {
                as_shipped: right,
                octave_matched: right,
            },
            ExcerptResult {
                as_shipped: Score::default(),
                octave_matched: Score::default(),
            },
        ];

        let report = render(&results, Half::All);

        assert!(report.contains("all 2 excerpts; 1 got no grid"), "{report}");
        assert!(
            report.contains(
                "| As reported, between 90 and 180 BPM | 0.500 | 50.0% | 50.0% | 100.0% |"
            ),
            "{report}"
        );
        assert!(report.contains("| high | 1 (50.0%) | 100.0% |"), "{report}");
        assert!(report.contains("| no grid | 1 (50.0%) | - |"), "{report}");
    }

    #[test]
    fn reads_beat_times_and_downbeats() {
        let folder = std::env::temp_dir().join(format!("euphoria-beats-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("annotation.beats");
        std::fs::write(&path, "0.045\t2\n0.515\t3\n0.98\t4\n1.438\t1\n\n").unwrap();
        let (beats, downbeats) = read_annotation(&path).unwrap();
        std::fs::remove_dir_all(&folder).unwrap();
        assert_eq!(beats, vec![0.045, 0.515, 0.98, 1.438]);
        assert_eq!(downbeats, vec![1.438]);
    }
}
