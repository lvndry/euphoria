// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Measures key and tempo detectors against the GiantSteps datasets and
//! prints a Markdown report.

mod scoring;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use euphoria_core::detect::{KeyDetector, KeyFinderDetector, StratumDetector, TempoDetector};
use euphoria_core::{AudioBuffer, Key, decode_mono};
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

struct Decoded<'track> {
    track: &'track Track,
    audio: AudioBuffer,
}

fn decode_all(tracks: &[Track]) -> (Vec<Decoded<'_>>, Vec<String>) {
    let outcomes: Vec<Result<Decoded<'_>, String>> = tracks
        .par_iter()
        .map(|track| {
            decode_mono(&track.audio_path)
                .map(|audio| Decoded { track, audio })
                .map_err(|error| format!("{}: {error}", track.name))
        })
        .collect();
    let mut decoded = Vec::new();
    let mut failures = Vec::new();
    for outcome in outcomes {
        match outcome {
            Ok(item) => decoded.push(item),
            Err(message) => failures.push(message),
        }
    }
    (decoded, failures)
}

fn benchmark_keys(data_directory: &Path) -> Result<()> {
    let tracks = load_tracks(&data_directory.join("key"), "annotations/key")?;
    if tracks.is_empty() {
        bail!("no key dataset audio found, run bench/download_datasets.sh first");
    }
    let (decoded, decode_failures) = decode_all(&tracks);
    let detectors: [&dyn KeyDetector; 2] = [&KeyFinderDetector, &StratumDetector];

    println!("## Key detection, GiantSteps Key\n");
    println!(
        "{} tracks with audio, {} decoded, {} failed to decode.\n",
        tracks.len(),
        decoded.len(),
        decode_failures.len()
    );
    println!(
        "| Detector | Exact | Fifth | Relative | Parallel | Other | No key or error | MIREX score | Seconds per track |"
    );
    println!("| --- | --- | --- | --- | --- | --- | --- | --- | --- |");

    for detector in detectors {
        let started = Instant::now();
        let estimates: Vec<(Key, Result<Option<Key>, String>)> = decoded
            .par_iter()
            .map(|item| {
                let reference: Key = item
                    .track
                    .annotation
                    .parse()
                    .unwrap_or_else(|_| panic!("bad annotation for {}", item.track.name));
                let estimate = detector
                    .detect_key(&item.audio)
                    .map_err(|error| error.to_string());
                (reference, estimate)
            })
            .collect();
        let elapsed = started.elapsed().as_secs_f64();

        let mut tally = KeyTally::default();
        for (reference, estimate) in &estimates {
            match estimate {
                Ok(Some(estimated)) => tally.record(*reference, *estimated),
                Ok(None) | Err(_) => tally.record_missing(),
            }
        }
        // Wall clock time divided by track count understates the cost per
        // track on a multi core machine, so report CPU seconds instead.
        let cpu_seconds_per_track =
            elapsed * rayon::current_num_threads() as f64 / estimates.len() as f64;
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
            cpu_seconds_per_track,
        );
    }
    println!();
    Ok(())
}

fn benchmark_tempo(data_directory: &Path) -> Result<()> {
    let tracks = load_tracks(&data_directory.join("tempo"), "annotations_v2/tempo")?;
    if tracks.is_empty() {
        bail!("no tempo dataset audio found, run bench/download_datasets.sh first");
    }
    let (decoded, decode_failures) = decode_all(&tracks);
    let detectors: [&dyn TempoDetector; 1] = [&StratumDetector];

    println!("## Tempo detection, GiantSteps Tempo\n");
    println!(
        "{} tracks with audio, {} decoded, {} failed to decode.\n",
        tracks.len(),
        decoded.len(),
        decode_failures.len()
    );
    println!(
        "| Detector | Within 4% | Within 4%, octave errors allowed | Error | Seconds per track |"
    );
    println!("| --- | --- | --- | --- | --- |");

    for detector in detectors {
        let started = Instant::now();
        let estimates: Vec<(f32, Result<f32, String>)> = decoded
            .par_iter()
            .map(|item| {
                let reference: f32 = item
                    .track
                    .annotation
                    .split_whitespace()
                    .next()
                    .and_then(|text| text.parse().ok())
                    .unwrap_or_else(|| panic!("bad annotation for {}", item.track.name));
                let estimate = detector
                    .detect_tempo(&item.audio)
                    .map_err(|error| error.to_string());
                (reference, estimate)
            })
            .collect();
        let elapsed = started.elapsed().as_secs_f64();

        let mut tally = TempoTally::default();
        let mut error_messages: BTreeMap<String, usize> = BTreeMap::new();
        for (reference, estimate) in &estimates {
            match estimate {
                Ok(estimated) => tally.record(*reference, *estimated),
                Err(message) => {
                    tally.record_missing();
                    *error_messages.entry(message.clone()).or_default() += 1;
                }
            }
        }
        let cpu_seconds_per_track =
            elapsed * rayon::current_num_threads() as f64 / estimates.len() as f64;
        println!(
            "| {} | {} | {} | {} | {:.2} |",
            detector.name(),
            tally.percent(tally.accurate),
            tally.percent(tally.accurate_with_octaves),
            tally.percent(tally.missing),
            cpu_seconds_per_track,
        );
        for (message, count) in error_messages {
            eprintln!("{}: {count} tracks failed with: {message}", detector.name());
        }
    }
    println!();
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
