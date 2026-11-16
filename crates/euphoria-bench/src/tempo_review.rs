// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Measures which tempos Needs review lists, per genre: how many octave
//! errors it catches and how many right tempos it asks about, under the
//! octave evidence rule and under the low confidence rule it replaced.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use euphoria_core::detect::{StratumDetector, TempoReading, detect_beat_grid};
use euphoria_core::{Correction, TempoRange, TrackAnalysis, decode_mono, review_reasons};
use rayon::prelude::*;

use crate::dataset;
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

/// A track with its reference tempo and genre.
struct Reference {
    audio_path: PathBuf,
    tempo: f32,
    genre: String,
}

fn giantsteps_references(directory: &Path) -> Result<Vec<Reference>> {
    let dataset = dataset::load_tempo_tracks(directory)?;
    dataset
        .tracks
        .into_iter()
        .map(|track| {
            let genre_path = directory
                .join("annotations/genre")
                .join(format!("{}.genre", track.name));
            let genre = std::fs::read_to_string(&genre_path)
                .with_context(|| format!("reading {}", genre_path.display()))?;
            Ok(Reference {
                audio_path: track.audio_path,
                tempo: track.annotation,
                genre: genre.trim().to_string(),
            })
        })
        .collect()
}

/// The GTZAN excerpts in `directory`: `annotations/gtzan_<genre>_<number>.beats`
/// with `audio/<genre>.<number>.wav`, the tempo taken from the median time
/// between the annotated beats.
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
        let mut intervals: Vec<f64> = Vec::new();
        let mut previous: Option<f64> = None;
        for line in text.lines() {
            let Some(time) = line
                .split_whitespace()
                .next()
                .and_then(|field| field.parse::<f64>().ok())
            else {
                continue;
            };
            if let Some(previous) = previous {
                intervals.push(time - previous);
            }
            previous = Some(time);
        }
        if intervals.is_empty() {
            continue;
        }
        intervals.sort_by(f64::total_cmp);
        references.push(Reference {
            audio_path,
            tempo: (60.0 / intervals[intervals.len() / 2]) as f32,
            genre: genre.to_string(),
        });
    }
    references.sort_by(|first, second| first.audio_path.cmp(&second.audio_path));
    Ok(references)
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

#[derive(Debug, Default, Clone, Copy)]
struct Tally {
    tracks: usize,
    listed_before: usize,
    listed_after: usize,
    octave_errors: usize,
    octave_with_alternative: usize,
    octave_listed_before: usize,
    octave_listed_after: usize,
    right_listed_before: usize,
    right_listed_after: usize,
}

impl Tally {
    fn add(&mut self, other: &Self) {
        self.tracks += other.tracks;
        self.listed_before += other.listed_before;
        self.listed_after += other.listed_after;
        self.octave_errors += other.octave_errors;
        self.octave_with_alternative += other.octave_with_alternative;
        self.octave_listed_before += other.octave_listed_before;
        self.octave_listed_after += other.octave_listed_after;
        self.right_listed_before += other.right_listed_before;
        self.right_listed_after += other.right_listed_after;
    }
}

/// How the rules treat one track, or `None` when it cannot be decoded or
/// has no tempo, as one GTZAN excerpt cannot be.
fn measure(reference: &Reference, range: TempoRange) -> Result<Option<Tally>> {
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
    let after = tempo_analysis(reading, reading.refined_by(grid.as_ref())).with_tempo_range(range);
    let before = TrackAnalysis {
        tempo_octave_evidence: None,
        ..after.clone()
    };
    let listed = |analysis: &TrackAnalysis| {
        !review_reasons(analysis, &Correction::default(), None, range).is_empty()
    };
    let Some(reported) = after.bpm else {
        return Ok(None);
    };
    let right = within_tolerance(reference.tempo, reported);
    let octave = within_tolerance(reference.tempo, reported / 2.0)
        || within_tolerance(reference.tempo, reported * 2.0);
    let (listed_before, listed_after) = (listed(&before), listed(&after));
    Ok(Some(Tally {
        tracks: 1,
        listed_before: usize::from(listed_before),
        listed_after: usize::from(listed_after),
        octave_errors: usize::from(octave),
        octave_with_alternative: usize::from(
            octave
                && after
                    .tempo_alternative_in(range)
                    .is_some_and(|alternative| within_tolerance(reference.tempo, alternative)),
        ),
        octave_listed_before: usize::from(octave && listed_before),
        octave_listed_after: usize::from(octave && listed_after),
        right_listed_before: usize::from(right && listed_before),
        right_listed_after: usize::from(right && listed_after),
    }))
}

fn render(title: &str, references: &[Reference], tallies: &[Option<Tally>]) -> String {
    let mut by_genre: BTreeMap<&str, Tally> = BTreeMap::new();
    let mut without_tempo = 0;
    for (reference, tally) in references.iter().zip(tallies) {
        match tally {
            Some(tally) => by_genre.entry(&reference.genre).or_default().add(tally),
            None => without_tempo += 1,
        }
    }
    let mut rows: Vec<(String, Tally)> = Vec::new();
    let mut grouped = Tally::default();
    let mut grouped_count = 0;
    for (genre, tally) in &by_genre {
        if tally.tracks >= MINIMUM_GENRE_TRACKS {
            rows.push((genre.to_string(), *tally));
        } else {
            grouped.add(tally);
            grouped_count += 1;
        }
    }
    rows.sort_by_key(|(_, tally)| std::cmp::Reverse(tally.tracks));
    if grouped_count > 0 {
        rows.push((format!("{grouped_count} other genres"), grouped));
    }
    let mut total = Tally::default();
    for tally in by_genre.values() {
        total.add(tally);
    }
    rows.push(("all".to_string(), total));
    let mut report = String::new();
    let _ = writeln!(report, "## {title}\n");
    let _ = writeln!(
        report,
        "{} tracks with a tempo, {without_tempo} without a tempo or not decodable. Before: listed when the tempo is rated low and has a plausible half or double. After: listed when the onsets back that half or double.\n",
        total.tracks
    );
    let _ = writeln!(
        report,
        "| Genre | Tracks | Tempo reviews before | After | Octave errors | Of those, the alternative is right | Listed before | Listed after | Right tempos listed before | After |"
    );
    let _ = writeln!(
        report,
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"
    );
    for (genre, tally) in rows {
        let _ = writeln!(
            report,
            "| {genre} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            tally.tracks,
            tally.listed_before,
            tally.listed_after,
            tally.octave_errors,
            tally.octave_with_alternative,
            tally.octave_listed_before,
            tally.octave_listed_after,
            tally.right_listed_before,
            tally.right_listed_after,
        );
    }
    report.push('\n');
    report
}

fn evaluate(title: &str, references: &[Reference], range: TempoRange) -> Result<String> {
    let tallies: Vec<Option<Tally>> = references
        .par_iter()
        .map(|reference| {
            measure(reference, range)
                .with_context(|| format!("measuring {}", reference.audio_path.display()))
        })
        .collect::<Result<_>>()?;
    Ok(render(title, references, &tallies))
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
