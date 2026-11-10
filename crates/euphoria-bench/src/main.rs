// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Measures key and tempo detectors against the GiantSteps datasets and
//! prints a Markdown report.
//!
//! Each track is decoded, run through every detector and dropped before the
//! next one, so memory stays bounded by the number of worker threads.

mod scoring;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use cpu_time::ThreadTime;
use euphoria_core::detect::{KeyDetector, KeyFinderDetector, StratumDetector, TempoDetector};
use euphoria_core::{EuphoriaError, Key, decode_mono};
use rayon::prelude::*;

use scoring::{KeyTally, TempoTally};

struct Track {
    name: String,
    audio_path: PathBuf,
    annotation: String,
}

fn load_tracks(dataset_directory: &Path, annotation_subdirectory: &str) -> Result<Vec<Track>> {
    let annotation_directory = dataset_directory.join(annotation_subdirectory);
    let mut tracks = Vec::new();
    for entry in fs::read_dir(&annotation_directory)
        .with_context(|| format!("reading {}", annotation_directory.display()))?
    {
        let annotation_path = entry?.path();
        let Some(name) = annotation_path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let audio_path = dataset_directory.join("audio").join(format!("{name}.mp3"));
        if !audio_path.is_file() {
            continue;
        }
        tracks.push(Track {
            name: name.to_string(),
            audio_path,
            annotation: fs::read_to_string(&annotation_path)?.trim().to_string(),
        });
    }
    tracks.sort_by(|first, second| first.name.cmp(&second.name));
    Ok(tracks)
}

/// One detector's answer for one track, with the CPU time the call used on
/// its own thread.
struct Measured<T> {
    estimate: Result<Option<T>, EuphoriaError>,
    cpu_time: Duration,
}

fn measure<T>(detect: impl FnOnce() -> Result<Option<T>, EuphoriaError>) -> Measured<T> {
    let started = ThreadTime::now();
    let estimate = detect();
    Measured {
        estimate,
        cpu_time: started.elapsed(),
    }
}

/// Every detector's measurement for one decoded track, in detector order.
struct TrackMeasurements<'track, T> {
    track: &'track Track,
    measurements: Vec<Measured<T>>,
}

/// Decodes every track and runs each detector on it, in parallel across
/// tracks. Returns one row of measurements per decoded track, plus the
/// messages of tracks that failed to decode.
fn run_detectors<T: Send>(
    tracks: &[Track],
    detector_count: usize,
    detect: impl Fn(usize, &euphoria_core::AudioBuffer) -> Result<Option<T>, EuphoriaError> + Sync,
) -> (Vec<TrackMeasurements<'_, T>>, Vec<String>) {
    let outcomes: Vec<Result<TrackMeasurements<'_, T>, String>> = tracks
        .par_iter()
        .map(|track| {
            let audio = decode_mono(&track.audio_path)
                .map_err(|error| format!("{}: {error}", track.name))?;
            let measurements = (0..detector_count)
                .map(|detector_index| measure(|| detect(detector_index, &audio)))
                .collect();
            Ok(TrackMeasurements {
                track,
                measurements,
            })
        })
        .collect();

    let mut rows = Vec::new();
    let mut decode_failures = Vec::new();
    for outcome in outcomes {
        match outcome {
            Ok(row) => rows.push(row),
            Err(message) => decode_failures.push(message),
        }
    }
    (rows, decode_failures)
}

fn report_errors(detector_name: &str, messages: &BTreeMap<String, usize>) {
    for (message, count) in messages {
        eprintln!("{detector_name}: {count} tracks failed with: {message}");
    }
}

fn benchmark_keys(data_directory: &Path) -> Result<()> {
    let tracks = load_tracks(&data_directory.join("key"), "annotations/key")?;
    if tracks.is_empty() {
        bail!("no key dataset audio found, run bench/download_datasets.sh first");
    }
    let detectors: [&dyn KeyDetector; 2] = [&KeyFinderDetector, &StratumDetector];
    let (rows, decode_failures) = run_detectors(&tracks, detectors.len(), |index, audio| {
        detectors[index].detect_key(audio)
    });

    println!("## Key detection, GiantSteps Key\n");
    println!(
        "{} tracks with audio, {} decoded, {} failed to decode.\n",
        tracks.len(),
        rows.len(),
        decode_failures.len()
    );
    println!(
        "| Detector | Exact | Fifth | Relative | Parallel | Other | No key or error | MIREX score | CPU seconds per track |"
    );
    println!("| --- | --- | --- | --- | --- | --- | --- | --- | --- |");

    for (detector_index, detector) in detectors.iter().enumerate() {
        let mut tally = KeyTally::default();
        let mut total_cpu_time = Duration::ZERO;
        let mut error_messages: BTreeMap<String, usize> = BTreeMap::new();
        for TrackMeasurements {
            track,
            measurements,
        } in &rows
        {
            let reference: Key = track
                .annotation
                .parse()
                .with_context(|| format!("bad key annotation for {}", track.name))?;
            let measured = &measurements[detector_index];
            total_cpu_time += measured.cpu_time;
            match &measured.estimate {
                Ok(Some(estimated)) => tally.record(reference, *estimated),
                Ok(None) => tally.record_missing(),
                Err(error) => {
                    tally.record_missing();
                    *error_messages.entry(error.to_string()).or_default() += 1;
                }
            }
        }
        println!(
            "| {} | {} | {} | {} | {} | {} | {} | {:.1}% | {:.2} |",
            detector.name(),
            tally.percent(tally.exact),
            tally.percent(tally.fifth),
            tally.percent(tally.relative),
            tally.percent(tally.parallel),
            tally.percent(tally.other),
            tally.percent(tally.missing),
            tally.mirex_score() * 100.0,
            total_cpu_time.as_secs_f64() / rows.len() as f64,
        );
        report_errors(detector.name(), &error_messages);
    }
    println!();
    Ok(())
}

fn benchmark_tempo(data_directory: &Path) -> Result<()> {
    let tracks = load_tracks(&data_directory.join("tempo"), "annotations_v2/tempo")?;
    if tracks.is_empty() {
        bail!("no tempo dataset audio found, run bench/download_datasets.sh first");
    }
    let detectors: [&dyn TempoDetector; 1] = [&StratumDetector];
    let (rows, decode_failures) = run_detectors(&tracks, detectors.len(), |index, audio| {
        detectors[index].detect_tempo(audio)
    });

    println!("## Tempo detection, GiantSteps Tempo\n");
    println!(
        "{} tracks with audio, {} decoded, {} failed to decode.\n",
        tracks.len(),
        rows.len(),
        decode_failures.len()
    );
    println!("| Detector | Accuracy 1 | Accuracy 2 | No tempo or error | CPU seconds per track |");
    println!("| --- | --- | --- | --- | --- |");

    for (detector_index, detector) in detectors.iter().enumerate() {
        let mut tally = TempoTally::default();
        let mut total_cpu_time = Duration::ZERO;
        let mut error_messages: BTreeMap<String, usize> = BTreeMap::new();
        for TrackMeasurements {
            track,
            measurements,
        } in &rows
        {
            let reference: f32 = track
                .annotation
                .split_whitespace()
                .next()
                .and_then(|text| text.parse().ok())
                .with_context(|| format!("bad tempo annotation for {}", track.name))?;
            let measured = &measurements[detector_index];
            total_cpu_time += measured.cpu_time;
            match &measured.estimate {
                Ok(Some(estimated)) => tally.record(reference, *estimated),
                Ok(None) => tally.record_missing(),
                Err(error) => {
                    tally.record_missing();
                    *error_messages.entry(error.to_string()).or_default() += 1;
                }
            }
        }
        println!(
            "| {} | {} | {} | {} | {:.2} |",
            detector.name(),
            tally.percent(tally.accurate),
            tally.percent(tally.accurate_with_octaves),
            tally.percent(tally.missing),
            total_cpu_time.as_secs_f64() / rows.len() as f64,
        );
        report_errors(detector.name(), &error_messages);
    }
    println!();
    println!(
        "Accuracy 1 counts an estimate within 4% of the annotated tempo. Accuracy 2 also accepts 2, 1/2, 3 and 1/3 times the annotated tempo, as in the standard tempo evaluation.\n"
    );
    Ok(())
}

fn main() -> Result<()> {
    let data_directory = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("bench/data"));
    benchmark_keys(&data_directory)?;
    benchmark_tempo(&data_directory)?;
    Ok(())
}
