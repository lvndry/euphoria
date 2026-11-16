// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Measures which tempos Needs review lists, per genre: how many octave
//! errors it catches and how many right tempos it asks about, under the
//! low confidence rule, the octave evidence rule and the two combined.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use euphoria_core::confidence::OCTAVE_DOUBT_EVIDENCE;
use euphoria_core::detect::{StratumDetector, TempoReading, detect_beat_grid};
use euphoria_core::{Correction, TempoRange, TrackAnalysis, decode_mono, review_reasons};
use rayon::prelude::*;

use crate::references::{Reference, giantsteps_references, gtzan_references};
use crate::scoring::within_tolerance;

pub const USAGE: &str = "usage: euphoria-bench tempo-review [GIANTSTEPS_TEMPO_DIRECTORY] [--gtzan GTZAN_DIRECTORY] [--range SLOWEST-FASTEST]";

/// Genres with fewer tracks than this are reported together.
const MINIMUM_GENRE_TRACKS: usize = 20;

#[derive(Debug, PartialEq)]
pub struct Arguments {
    giantsteps_directory: PathBuf,
    gtzan_directory: Option<PathBuf>,
    range: TempoRange,
}

pub fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Arguments> {
    let mut giantsteps_directory = None;
    let mut gtzan_directory = None;
    let mut range = TempoRange::DEFAULT;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "--gtzan" {
            let value = arguments
                .next()
                .with_context(|| format!("--gtzan needs a directory\n{USAGE}"))?;
            gtzan_directory = Some(PathBuf::from(value));
        } else if argument == "--range" {
            let value = arguments
                .next()
                .with_context(|| format!("--range needs a value\n{USAGE}"))?;
            range = value
                .parse()
                .map_err(|error| anyhow::anyhow!("{error}\n{USAGE}"))?;
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
        range,
    })
}

/// What the analysis stores about the tempo of a track, with everything
/// else left empty.
fn tempo_analysis(reading: TempoReading, refined: f32) -> TrackAnalysis {
    TrackAnalysis {
        duration_seconds: 0.0,
        key: None,
        bpm: Some(TempoRange::DEFAULT.fold(refined)),
        energy: None,
        tuning_cents: None,
        key_timeline: Vec::new(),
        quality: None,
        key_runner_up: None,
        key_score_margin: None,
        key_source: None,
        tempo_estimate: Some(refined),
        tempo_score_margin: reading.score_margin,
        tempo_octave_evidence: reading.octave_evidence,
        beat_grid: None,
        cues: Vec::new(),
        loudness_range_lu: None,
        pitch_class_profile: None,
    }
}

/// How one track stands under the review rules.
#[derive(Debug, Clone, Copy)]
struct Record {
    right: bool,
    octave_error: bool,
    octave_offered: bool,
    /// Listed by the rule before: rated low with a plausible half or double.
    rated_low: bool,
    /// How much more strongly the onsets back the plausible half or double
    /// than the reported tempo, or `None` without one.
    evidence: Option<f32>,
}

impl Record {
    fn listed_by_evidence(&self, threshold: f32) -> bool {
        self.evidence.is_some_and(|evidence| evidence >= threshold)
    }

    fn listed_combined(&self, threshold: f32) -> bool {
        self.rated_low || self.listed_by_evidence(threshold)
    }
}

/// Counts of listed octave errors and listed right tempos.
#[derive(Debug, Default, Clone, Copy)]
struct Listed {
    octave_errors: usize,
    right: usize,
}

impl Listed {
    fn count(records: &[Record], rule: impl Fn(&Record) -> bool) -> Self {
        let listed: Vec<&Record> = records.iter().filter(|record| rule(record)).collect();
        Self {
            octave_errors: listed.iter().filter(|record| record.octave_error).count(),
            right: listed.iter().filter(|record| record.right).count(),
        }
    }
}

/// How the rules treat one track, or `None` when it cannot be decoded or
/// has no tempo, as one GTZAN excerpt cannot be.
fn measure(reference: &Reference, range: TempoRange) -> Result<Option<Record>> {
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
    let analysis =
        tempo_analysis(reading, reading.refined_by(grid.as_ref())).with_tempo_range(range);
    let Some(reported) = analysis.bpm else {
        return Ok(None);
    };
    let alternative = analysis.tempo_alternative_in(range);
    let octave_error = within_tolerance(reference.tempo, reported / 2.0)
        || within_tolerance(reference.tempo, reported * 2.0);
    let without_evidence = TrackAnalysis {
        tempo_octave_evidence: None,
        ..analysis.clone()
    };
    Ok(Some(Record {
        right: within_tolerance(reference.tempo, reported),
        octave_error,
        octave_offered: octave_error
            && alternative
                .is_some_and(|alternative| within_tolerance(reference.tempo, alternative)),
        rated_low: !review_reasons(&without_evidence, &Correction::default(), None, range)
            .is_empty(),
        evidence: alternative
            .and_then(|alternative| analysis.tempo_octave_evidence_in(range, alternative)),
    }))
}

/// Thresholds of the evidence the sweep tries.
const SWEEP: [f32; 8] = [0.0, 0.05, 0.1, 0.15, 0.2, 0.25, 0.3, 0.4];

fn render(
    title: &str,
    references: &[Reference],
    records: &[Option<Record>],
    threshold: f32,
) -> String {
    let mut by_genre: BTreeMap<&str, Vec<Record>> = BTreeMap::new();
    let mut without_tempo = 0;
    for (reference, record) in references.iter().zip(records) {
        match record {
            Some(record) => by_genre.entry(&reference.genre).or_default().push(*record),
            None => without_tempo += 1,
        }
    }
    let mut rows: Vec<(String, Vec<Record>)> = Vec::new();
    let mut grouped = Vec::new();
    let mut grouped_count = 0;
    let mut total = Vec::new();
    for (genre, genre_records) in &by_genre {
        total.extend_from_slice(genre_records);
        if genre_records.len() >= MINIMUM_GENRE_TRACKS {
            rows.push((genre.to_string(), genre_records.clone()));
        } else {
            grouped.extend_from_slice(genre_records);
            grouped_count += 1;
        }
    }
    rows.sort_by_key(|(_, genre_records)| std::cmp::Reverse(genre_records.len()));
    if grouped_count > 0 {
        rows.push((format!("{grouped_count} other genres"), grouped));
    }
    rows.push(("all".to_string(), total.clone()));
    let mut report = String::new();
    let _ = writeln!(
        report,
        "## {title}
"
    );
    let _ = writeln!(
        report,
        "{} tracks with a tempo, {without_tempo} without a tempo or not decodable. Rated low: the tempo is rated low and has a plausible half or double. Evidence: the onsets back that half or double by at least {threshold}. Combined: either.\n",
        total.len()
    );
    let _ = writeln!(
        report,
        "| Genre | Tracks | Octave errors | Right octave offered | Octave errors listed, rated low | Evidence | Combined | Right tempos listed, rated low | Evidence | Combined |"
    );
    let _ = writeln!(
        report,
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"
    );
    for (genre, genre_records) in rows {
        let rated_low = Listed::count(&genre_records, |record| record.rated_low);
        let evidence = Listed::count(&genre_records, |record| {
            record.listed_by_evidence(threshold)
        });
        let combined = Listed::count(&genre_records, |record| record.listed_combined(threshold));
        let _ = writeln!(
            report,
            "| {genre} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            genre_records.len(),
            genre_records
                .iter()
                .filter(|record| record.octave_error)
                .count(),
            genre_records
                .iter()
                .filter(|record| record.octave_offered)
                .count(),
            rated_low.octave_errors,
            evidence.octave_errors,
            combined.octave_errors,
            rated_low.right,
            evidence.right,
            combined.right,
        );
    }
    let _ = writeln!(
        report,
        "\nAll tracks, by the evidence threshold: octave errors listed and right tempos listed.\n"
    );
    let _ = writeln!(
        report,
        "| Threshold | Evidence alone | Combined with rated low |"
    );
    let _ = writeln!(report, "| --- | --- | --- |");
    let rated_low = Listed::count(&total, |record| record.rated_low);
    let _ = writeln!(
        report,
        "| rated low alone | - | {} and {} |",
        rated_low.octave_errors, rated_low.right
    );
    for sweep_threshold in SWEEP {
        let alone = Listed::count(&total, |record| record.listed_by_evidence(sweep_threshold));
        let combined = Listed::count(&total, |record| record.listed_combined(sweep_threshold));
        let _ = writeln!(
            report,
            "| {sweep_threshold} | {} and {} | {} and {} |",
            alone.octave_errors, alone.right, combined.octave_errors, combined.right
        );
    }
    report.push('\n');
    report
}

fn evaluate(title: &str, references: &[Reference], range: TempoRange) -> Result<String> {
    let records: Vec<Option<Record>> = references
        .par_iter()
        .map(|reference| {
            measure(reference, range)
                .with_context(|| format!("measuring {}", reference.audio_path.display()))
        })
        .collect::<Result<_>>()?;
    Ok(render(title, references, &records, OCTAVE_DOUBT_EVIDENCE))
}

pub fn run(arguments: &Arguments) -> Result<()> {
    let range = arguments.range;
    let giantsteps = giantsteps_references(&arguments.giantsteps_directory)?;
    print!(
        "{}",
        evaluate(
            &format!("Tempo reviews on GiantSteps Tempo, range {range}"),
            &giantsteps,
            range
        )?
    );
    if let Some(directory) = &arguments.gtzan_directory {
        let gtzan = gtzan_references(directory)?;
        print!(
            "{}",
            evaluate(
                &format!("Tempo reviews on GTZAN, range {range}"),
                &gtzan,
                range
            )?
        );
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
    fn reads_the_directories_and_the_range() {
        let arguments = parse(&["giantsteps", "--gtzan", "gtzan", "--range", "70-140"]).unwrap();
        assert_eq!(arguments.giantsteps_directory, PathBuf::from("giantsteps"));
        assert_eq!(arguments.gtzan_directory, Some(PathBuf::from("gtzan")));
        assert_eq!(arguments.range, "70-140".parse().unwrap());
        assert_eq!(parse(&[]).unwrap().range, TempoRange::DEFAULT);
        for bad in [
            &["--range"][..],
            &["--range", "60-200"],
            &["--fast"],
            &["a", "b"],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }
}
