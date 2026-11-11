// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Scores the energy rating against reference ratings from a labels file.
//!
//! A labels file has one track per line: the path of an audio file, relative
//! to the labels file, then optionally a tab and a reference number where
//! higher means more energy. The benchmark does not care where the
//! references come from. Without references it reports only how the ratings
//! and the features are distributed.
//!
//! The report holds aggregates only, never a track name or a path, so that a
//! private music library can serve as reference data.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use cpu_time::ThreadTime;
use euphoria_core::{
    ENERGY_MODEL_VERSION, EnergyRating, EuphoriaError, decode_mono, energy_features, rate_features,
};
use rayon::prelude::*;

use crate::statistics::{mean, ordered_pair_share, percentile, spearman_correlation};

/// Usage of the energy mode.
pub const USAGE: &str = "usage: euphoria-bench energy LABELS_FILE [--dump FILE]";

/// More distinct reference values than this are reported in four groups of
/// equal size, lowest to highest, and not one by one.
const MOST_SEPARATE_REFERENCES: usize = 10;
const GROUP_NAMES: [&str; 4] = [
    "Lowest quarter",
    "Second quarter",
    "Third quarter",
    "Highest quarter",
];

/// The parsed command line of the energy mode.
#[derive(Debug, PartialEq, Eq)]
pub struct Arguments {
    labels_file: PathBuf,
    dump_file: Option<PathBuf>,
}

/// One line of a labels file.
#[derive(Debug, PartialEq)]
struct LabelledTrack {
    path: PathBuf,
    reference: Option<f64>,
}

/// What analysis made of one track.
#[derive(Debug)]
enum Outcome {
    Rated(Measurement),
    /// Nothing to rate, such as silence.
    Unrated,
    /// The kind of failure, without the path of the file.
    Failed(&'static str),
}

/// A rated track, stripped of everything that identifies it.
#[derive(Debug, Clone)]
struct Measurement {
    reference: Option<f64>,
    rating: EnergyRating,
    /// CPU time of measuring the features and rating them, without
    /// decoding.
    energy_cpu_time: Duration,
}

impl Measurement {
    /// The features by name.
    fn inputs(&self) -> Vec<(&'static str, f64)> {
        self.rating
            .features
            .named()
            .into_iter()
            .map(|(name, value)| (name, f64::from(value)))
            .collect()
    }
}

/// Parses the arguments that follow `energy` on the command line.
pub fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Arguments> {
    let mut labels_file = None;
    let mut dump_file = None;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "--dump" {
            let value = arguments
                .next()
                .with_context(|| format!("--dump needs a file\n{USAGE}"))?;
            dump_file = Some(PathBuf::from(value));
        } else if argument.starts_with("--") {
            bail!("unknown option {argument:?}\n{USAGE}");
        } else if labels_file.is_some() {
            bail!("more than one labels file given\n{USAGE}");
        } else {
            labels_file = Some(PathBuf::from(argument));
        }
    }
    Ok(Arguments {
        labels_file: labels_file.with_context(|| format!("no labels file given\n{USAGE}"))?,
        dump_file,
    })
}

/// Analyzes every track of the labels file and prints the report.
pub fn run(arguments: &Arguments) -> Result<()> {
    let text = std::fs::read_to_string(&arguments.labels_file)
        .with_context(|| format!("cannot read {}", arguments.labels_file.display()))?;
    let folder = arguments.labels_file.parent().unwrap_or(Path::new("."));
    let tracks = parse_labels(&text, folder)
        .with_context(|| format!("in {}", arguments.labels_file.display()))?;

    let outcomes: Vec<Outcome> = tracks.par_iter().map(measure).collect();

    if let Some(dump_file) = &arguments.dump_file {
        std::fs::write(dump_file, render_dump(&outcomes))
            .with_context(|| format!("cannot write {}", dump_file.display()))?;
    }
    print!("{}", render_report(&outcomes));
    Ok(())
}

fn parse_labels(text: &str, folder: &Path) -> Result<Vec<LabelledTrack>> {
    let mut tracks = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let (path, reference) = match line.split_once('\t') {
            Some((path, reference)) => {
                let reference: f64 = reference
                    .trim()
                    .parse()
                    .ok()
                    .filter(|value: &f64| value.is_finite())
                    .with_context(|| {
                        format!("line {}: the reference is not a number", index + 1)
                    })?;
                (path, Some(reference))
            }
            None => (line, None),
        };
        tracks.push(LabelledTrack {
            path: folder.join(path),
            reference,
        });
    }
    let with_reference = tracks
        .iter()
        .filter(|track| track.reference.is_some())
        .count();
    if with_reference != 0 && with_reference != tracks.len() {
        bail!("some lines have a reference and some do not");
    }
    Ok(tracks)
}

fn measure(track: &LabelledTrack) -> Outcome {
    let audio = match decode_mono(&track.path) {
        Ok(audio) => audio,
        Err(error) => return Outcome::Failed(failure_kind(&error)),
    };
    let started = ThreadTime::now();
    let rating = energy_features(&audio).and_then(|features| {
        features
            .map(|features| rate_features(&features))
            .transpose()
    });
    let energy_cpu_time = started.elapsed();
    match rating {
        Ok(Some(rating)) => Outcome::Rated(Measurement {
            reference: track.reference,
            rating,
            energy_cpu_time,
        }),
        Ok(None) => Outcome::Unrated,
        Err(error) => Outcome::Failed(failure_kind(&error)),
    }
}

/// Names the kind of error without the path that its message carries.
fn failure_kind(error: &EuphoriaError) -> &'static str {
    match error {
        EuphoriaError::Open { .. } => "could not be opened",
        EuphoriaError::Decode { .. } | EuphoriaError::EmptyAudio { .. } => "could not be decoded",
        _ => "could not be analyzed",
    }
}

fn rated(outcomes: &[Outcome]) -> Vec<&Measurement> {
    outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            Outcome::Rated(measurement) => Some(measurement),
            _ => None,
        })
        .collect()
}

/// One line per rated track with its reference, rating and inputs, and
/// nothing that names the track. A fitting script reads it.
fn render_dump(outcomes: &[Outcome]) -> String {
    let measurements = rated(outcomes);
    let mut dump = String::new();
    let Some(first) = measurements.first() else {
        return dump;
    };
    dump.push_str("reference\trating\tscore");
    for (name, _) in first.rating.features.named() {
        let _ = write!(dump, "\t{}", name.replace(' ', "_"));
    }
    dump.push('\n');
    for measurement in &measurements {
        let reference = measurement
            .reference
            .map(|reference| reference.to_string())
            .unwrap_or_default();
        let _ = write!(
            dump,
            "{reference}\t{}\t{}",
            measurement.rating.rating, measurement.rating.score
        );
        for (_, value) in measurement.rating.features.named() {
            let _ = write!(dump, "\t{value}");
        }
        dump.push('\n');
    }
    dump
}

fn render_report(outcomes: &[Outcome]) -> String {
    let measurements = rated(outcomes);
    let mut report = String::new();
    let _ = writeln!(report, "# Energy rating benchmark\n");
    let unrated = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, Outcome::Unrated))
        .count();
    let _ = writeln!(
        report,
        "Energy model version {ENERGY_MODEL_VERSION}. {} tracks listed, {} rated, {unrated} with nothing to rate.",
        outcomes.len(),
        measurements.len(),
    );
    for kind in [
        "could not be opened",
        "could not be decoded",
        "could not be analyzed",
    ] {
        let count = outcomes
            .iter()
            .filter(|outcome| matches!(outcome, Outcome::Failed(found) if *found == kind))
            .count();
        if count > 0 {
            let _ = writeln!(report, "{count} tracks {kind}.");
        }
    }
    if measurements.is_empty() {
        return report;
    }

    let references: Option<Vec<f64>> = measurements
        .iter()
        .map(|measurement| measurement.reference)
        .collect();
    if let Some(references) = &references {
        render_agreement(&mut report, &measurements, references);
        render_groups(&mut report, &measurements, references);
    }
    render_histogram(&mut report, &measurements);
    render_distributions(&mut report, &measurements);

    let cpu_seconds: Vec<f64> = measurements
        .iter()
        .map(|measurement| measurement.energy_cpu_time.as_secs_f64())
        .collect();
    let _ = writeln!(
        report,
        "\n## Cost\n\n{:.3} CPU seconds per track for measuring and rating energy, after decoding.",
        mean(&cpu_seconds).unwrap_or(0.0)
    );
    report
}

/// The columns to compare with the reference: the rating, the score it is
/// cut from, then every feature.
fn columns(measurements: &[&Measurement]) -> Vec<(&'static str, Vec<f64>)> {
    let mut columns = vec![
        (
            "Rating",
            measurements
                .iter()
                .map(|measurement| f64::from(measurement.rating.rating))
                .collect(),
        ),
        (
            "Score",
            measurements
                .iter()
                .map(|measurement| f64::from(measurement.rating.score))
                .collect(),
        ),
    ];
    columns.extend(input_columns(measurements));
    columns
}

/// One column per feature.
fn input_columns(measurements: &[&Measurement]) -> Vec<(&'static str, Vec<f64>)> {
    let Some(first) = measurements.first() else {
        return Vec::new();
    };
    first
        .inputs()
        .iter()
        .enumerate()
        .map(|(index, (name, _))| {
            (
                *name,
                measurements
                    .iter()
                    .map(|measurement| measurement.inputs()[index].1)
                    .collect(),
            )
        })
        .collect()
}

fn render_agreement(report: &mut String, measurements: &[&Measurement], references: &[f64]) {
    let _ = writeln!(
        report,
        "\n## Agreement with the reference\n\n| | Spearman correlation | Pairs in the right order |\n| --- | --- | --- |"
    );
    for (name, values) in columns(measurements) {
        let correlation = spearman_correlation(&values, references)
            .map(|correlation| format!("{correlation:+.3}"))
            .unwrap_or_else(|| "undefined".to_string());
        let pair_share = ordered_pair_share(&values, references)
            .map(|share| format!("{:.1}%", share * 100.0))
            .unwrap_or_else(|| "undefined".to_string());
        let _ = writeln!(
            report,
            "| {} | {correlation} | {pair_share} |",
            capitalized(name)
        );
    }
    let _ = writeln!(
        report,
        "\nA pair is two tracks with different references. 50% is what chance scores. A negative correlation means the value falls as the reference rises."
    );
}

fn render_groups(report: &mut String, measurements: &[&Measurement], references: &[f64]) {
    let groups = reference_groups(references);
    let columns = columns(measurements);
    let _ = write!(
        report,
        "\n## Averages by reference\n\n| Reference | Tracks |"
    );
    for (name, _) in &columns {
        let _ = write!(report, " {} |", capitalized(name));
    }
    let _ = write!(report, "\n| --- | --- |");
    for _ in &columns {
        let _ = write!(report, " --- |");
    }
    let _ = writeln!(report);
    for (group_name, members) in groups {
        let _ = write!(report, "| {group_name} | {} |", members.len());
        for (_, values) in &columns {
            let group_values: Vec<f64> = members.iter().map(|index| values[*index]).collect();
            let _ = write!(
                report,
                " {} |",
                significant(mean(&group_values).unwrap_or(0.0))
            );
        }
        let _ = writeln!(report);
    }
}

/// Splits track indices by reference: one group per value when there are
/// few distinct values, otherwise four groups of equal size from the lowest
/// references to the highest.
fn reference_groups(references: &[f64]) -> Vec<(String, Vec<usize>)> {
    let mut order: Vec<usize> = (0..references.len()).collect();
    order.sort_by(|first, second| references[*first].total_cmp(&references[*second]));
    let mut distinct: Vec<f64> = order.iter().map(|index| references[*index]).collect();
    distinct.dedup();
    if distinct.len() <= MOST_SEPARATE_REFERENCES {
        return distinct
            .into_iter()
            .map(|value| {
                let members = order
                    .iter()
                    .copied()
                    .filter(|index| references[*index] == value)
                    .collect();
                (significant(value), members)
            })
            .collect();
    }
    GROUP_NAMES
        .iter()
        .enumerate()
        .map(|(group, name)| {
            let start = order.len() * group / GROUP_NAMES.len();
            let end = order.len() * (group + 1) / GROUP_NAMES.len();
            (name.to_string(), order[start..end].to_vec())
        })
        .collect()
}

fn render_histogram(report: &mut String, measurements: &[&Measurement]) {
    let mut counts = [0_usize; 10];
    for measurement in measurements {
        counts[usize::from(measurement.rating.rating.clamp(1, 10)) - 1] += 1;
    }
    let _ = writeln!(
        report,
        "\n## Rating histogram\n\n| Rating | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"
    );
    let _ = write!(report, "| Tracks |");
    for count in counts {
        let _ = write!(report, " {count} |");
    }
    let _ = writeln!(report);
}

fn render_distributions(report: &mut String, measurements: &[&Measurement]) {
    let _ = writeln!(
        report,
        "\n## Feature values\n\n| Feature | 3rd percentile | Median | 97th percentile |\n| --- | --- | --- | --- |"
    );
    for (name, values) in input_columns(measurements) {
        let _ = writeln!(
            report,
            "| {} | {} | {} | {} |",
            capitalized(name),
            significant(percentile(&values, 0.03).unwrap_or(0.0)),
            significant(percentile(&values, 0.5).unwrap_or(0.0)),
            significant(percentile(&values, 0.97).unwrap_or(0.0)),
        );
    }
}

fn capitalized(name: &str) -> String {
    let mut characters = name.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

/// Formats a number with about three significant digits.
fn significant(value: f64) -> String {
    let magnitude = value.abs();
    if magnitude >= 100.0 {
        format!("{value:.0}")
    } else if magnitude >= 10.0 {
        format!("{value:.1}")
    } else if magnitude >= 1.0 {
        format!("{value:.2}")
    } else if magnitude >= 0.01 {
        format!("{value:.3}")
    } else {
        format!("{value:.5}")
    }
}

#[cfg(test)]
mod tests {
    use euphoria_core::EnergyFeatures;

    use super::*;

    fn parse(arguments: &[&str]) -> Result<Arguments> {
        parse_arguments(arguments.iter().map(|argument| argument.to_string()))
    }

    /// A rated track whose features all rise with `level`, from 0.0 to 1.0.
    fn measurement(level: f32, reference: Option<f64>) -> Measurement {
        let features = EnergyFeatures {
            spectral_flux: 0.002 + 0.008 * level,
            onset_rate: 1.0 + 6.0 * level,
            integrated_loudness_lufs: -22.0 + 15.0 * level,
        };
        Measurement {
            reference,
            rating: rate_features(&features).unwrap(),
            energy_cpu_time: Duration::from_millis(250),
        }
    }

    fn rated_outcomes(references: &[Option<f64>]) -> Vec<Outcome> {
        references
            .iter()
            .enumerate()
            .map(|(index, reference)| {
                let level = index as f32 / (references.len() - 1) as f32;
                Outcome::Rated(measurement(level, *reference))
            })
            .collect()
    }

    #[test]
    fn arguments_need_a_labels_file_and_accept_the_options() {
        assert_eq!(
            parse(&["labels.tsv"]).unwrap(),
            Arguments {
                labels_file: PathBuf::from("labels.tsv"),
                dump_file: None,
            }
        );
        assert_eq!(
            parse(&["--dump", "out.tsv", "labels.tsv"]).unwrap(),
            Arguments {
                labels_file: PathBuf::from("labels.tsv"),
                dump_file: Some(PathBuf::from("out.tsv")),
            }
        );
        for arguments in [&[][..], &["--dump"], &["one", "two"], &["--fast", "one"]] {
            assert!(parse(arguments).is_err(), "{arguments:?} should not parse");
        }
    }

    #[test]
    fn labels_are_paths_relative_to_the_file_with_an_optional_reference() {
        let folder = Path::new("/data");
        let labelled = parse_labels("# comment\naudio/one.mp3\t3.5\n\ntwo.flac\t7\n", folder);
        assert_eq!(
            labelled.unwrap(),
            vec![
                LabelledTrack {
                    path: PathBuf::from("/data/audio/one.mp3"),
                    reference: Some(3.5),
                },
                LabelledTrack {
                    path: PathBuf::from("/data/two.flac"),
                    reference: Some(7.0),
                },
            ]
        );
        let unlabelled = parse_labels("one.mp3\ntwo.mp3\n", folder).unwrap();
        assert!(unlabelled.iter().all(|track| track.reference.is_none()));
    }

    #[test]
    fn labels_with_a_bad_or_partly_missing_reference_are_refused() {
        let folder = Path::new("/data");
        for text in ["one.mp3\tloud\n", "one.mp3\tNaN\n", "one.mp3\t3\ntwo.mp3\n"] {
            assert!(parse_labels(text, folder).is_err(), "{text:?}");
        }
    }

    #[test]
    fn the_report_scores_values_that_follow_the_reference_as_perfect() {
        let references: Vec<Option<f64>> = (0..12).map(|step| Some(f64::from(step))).collect();
        let report = render_report(&rated_outcomes(&references));

        assert!(report.contains("12 tracks listed, 12 rated, 0 with nothing to rate."));
        assert!(report.contains("| Rating | +0.9"), "{report}");
        assert!(
            report.contains("| Onset rate | +1.000 | 100.0% |"),
            "{report}"
        );
        assert!(
            report.contains("| Integrated loudness | +1.000 | 100.0% |"),
            "{report}"
        );
        assert!(report.contains("| Lowest quarter | 3 |"), "{report}");
        assert!(report.contains("| Highest quarter | 3 |"), "{report}");
        assert!(report.contains("## Rating histogram"));
        assert!(report.contains("0.250 CPU seconds per track"));
    }

    #[test]
    fn few_distinct_references_are_reported_one_by_one() {
        let references = [1.0, 1.0, 2.0, 3.0, 3.0, 3.0].map(Some);
        let report = render_report(&rated_outcomes(&references));
        assert!(report.contains("| 1.00 | 2 |"), "{report}");
        assert!(report.contains("| 2.00 | 1 |"), "{report}");
        assert!(report.contains("| 3.00 | 3 |"), "{report}");
    }

    #[test]
    fn without_references_the_report_has_only_distributions() {
        let report = render_report(&rated_outcomes(&[None, None, None]));
        assert!(!report.contains("## Agreement"));
        assert!(!report.contains("## Averages by reference"));
        assert!(report.contains("## Rating histogram"));
        assert!(report.contains("## Feature values"));
        assert!(report.contains("| Onset rate |"));
    }

    #[test]
    fn failures_are_counted_by_kind_and_never_named() {
        let mut outcomes = rated_outcomes(&[Some(1.0), Some(2.0)]);
        outcomes.push(Outcome::Unrated);
        outcomes.push(Outcome::Failed("could not be decoded"));
        outcomes.push(Outcome::Failed("could not be decoded"));
        let report = render_report(&outcomes);
        assert!(report.contains("5 tracks listed, 2 rated, 1 with nothing to rate."));
        assert!(report.contains("2 tracks could not be decoded."));

        let missing = measure(&LabelledTrack {
            path: PathBuf::from("/secret/folder/track name.mp3"),
            reference: Some(1.0),
        });
        assert!(matches!(missing, Outcome::Failed("could not be opened")));
    }

    #[test]
    fn the_dump_has_a_header_and_one_nameless_line_per_rated_track() {
        let mut outcomes = rated_outcomes(&[Some(1.0), Some(2.0)]);
        outcomes.push(Outcome::Unrated);
        let dump = render_dump(&outcomes);
        let lines: Vec<&str> = dump.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(
            lines[0],
            "reference\trating\tscore\tspectral_flux\tonset_rate\tintegrated_loudness"
        );
        assert!(
            lines[1].starts_with("1\t1\t0\t0.002\t1\t-22"),
            "{}",
            lines[1]
        );
        assert_eq!(render_dump(&[Outcome::Unrated]), "");
    }

    #[test]
    fn numbers_are_shown_with_about_three_significant_digits() {
        assert_eq!(significant(123.456), "123");
        assert_eq!(significant(-12.345), "-12.3");
        assert_eq!(significant(1.2345), "1.23");
        assert_eq!(significant(0.12345), "0.123");
        assert_eq!(significant(0.0012345), "0.00123");
    }
}
