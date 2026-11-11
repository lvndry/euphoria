// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The stages an analysis reports to an observer, and that watching changes
//! nothing about the result.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use euphoria_core::{
    AnalysisObserver, AnalysisStage, Cache, CachePolicy, analyze_file, analyze_file_observed,
    analyze_files_cached_observed, analyze_files_observed,
};

#[derive(Debug, Clone, PartialEq)]
enum Event {
    Stage(PathBuf, AnalysisStage),
    Decoded(PathBuf, f32),
}

#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<Event>>,
}

impl AnalysisObserver for Recorder {
    fn stage(&self, path: &Path, stage: AnalysisStage) {
        self.events
            .lock()
            .expect("no recording panicked")
            .push(Event::Stage(path.to_path_buf(), stage));
    }

    fn decoded(&self, path: &Path, share: f32) {
        self.events
            .lock()
            .expect("no recording panicked")
            .push(Event::Decoded(path.to_path_buf(), share));
    }
}

impl Recorder {
    fn events_of(&self, path: &Path) -> Vec<Event> {
        self.events
            .lock()
            .expect("no recording panicked")
            .iter()
            .filter(|event| match event {
                Event::Stage(event_path, _) | Event::Decoded(event_path, _) => event_path == path,
            })
            .cloned()
            .collect()
    }
}

const ORDER: [AnalysisStage; 6] = [
    AnalysisStage::Decoding,
    AnalysisStage::Tempo,
    AnalysisStage::Key,
    AnalysisStage::Energy,
    AnalysisStage::BeatGrid,
    AnalysisStage::Quality,
];

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn stages(events: &[Event]) -> Vec<AnalysisStage> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Stage(_, stage) => Some(*stage),
            Event::Decoded(..) => None,
        })
        .collect()
}

#[test]
fn a_file_reports_each_stage_once_in_order() {
    for name in [
        "sine-440hz-mono.wav",
        "sine-440hz-mono.flac",
        "sine-440hz-mono.mp3",
        "sine-440hz-mono.m4a",
        "sine-440hz-mono.aiff",
    ] {
        let path = fixture(name);
        let recorder = Recorder::default();
        analyze_file_observed(&path, &recorder).unwrap();
        let events = recorder.events_of(&path);
        assert_eq!(stages(&events), ORDER, "{name}");

        // Decoding shares come while decoding, rise, and stay within 0 to 1.
        let tempo_at = events
            .iter()
            .position(|event| *event == Event::Stage(path.clone(), AnalysisStage::Tempo))
            .unwrap();
        let shares: Vec<f32> = events
            .iter()
            .enumerate()
            .filter_map(|(index, event)| match event {
                Event::Decoded(_, share) => {
                    assert!(index > 0 && index < tempo_at, "{name}: {events:?}");
                    Some(*share)
                }
                Event::Stage(..) => None,
            })
            .collect();
        assert!(
            shares.windows(2).all(|pair| pair[0] < pair[1]),
            "{name}: {shares:?}"
        );
        assert!(
            shares.iter().all(|share| (0.0..=1.0).contains(share)),
            "{name}"
        );
    }
}

#[test]
fn a_file_that_declares_its_length_reports_decoding_to_the_end() {
    let path = fixture("sine-440hz-mono.wav");
    let recorder = Recorder::default();
    analyze_file_observed(&path, &recorder).unwrap();
    let last_share = recorder
        .events_of(&path)
        .iter()
        .rev()
        .find_map(|event| match event {
            Event::Decoded(_, share) => Some(*share),
            Event::Stage(..) => None,
        });
    assert_eq!(last_share, Some(1.0));
}

#[test]
fn watching_changes_nothing_about_the_result() {
    let path = fixture("sine-440hz-mono.flac");
    let watched = analyze_file_observed(&path, &Recorder::default()).unwrap();
    let unwatched = analyze_file(&path).unwrap();
    assert_eq!(watched, unwatched);
}

#[test]
fn a_file_that_fails_stops_reporting_where_it_failed() {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("stages-not-audio.mp3");
    std::fs::write(&path, "This is a text file, not audio.\n".repeat(64)).unwrap();
    let recorder = Recorder::default();
    assert!(analyze_file_observed(&path, &recorder).is_err());
    assert_eq!(
        stages(&recorder.events_of(&path)),
        [AnalysisStage::Decoding]
    );
}

#[test]
fn a_batch_reports_every_file_and_a_cached_file_reports_nothing() {
    let paths = vec![
        fixture("sine-440hz-mono.wav"),
        fixture("sine-440hz-mono.mp3"),
    ];
    let recorder = Recorder::default();
    let results = analyze_files_observed(&paths, &recorder, |_| {});
    assert!(results.iter().all(|result| result.outcome.is_ok()));
    for path in &paths {
        assert_eq!(stages(&recorder.events_of(path)), ORDER);
    }

    let cache_path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("stages-cache.sqlite3");
    for suffix in ["", "-shm", "-wal"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", cache_path.display()));
    }
    let mut cache = Cache::open(&cache_path).unwrap();
    let first = Recorder::default();
    let batch =
        analyze_files_cached_observed(&mut cache, &paths, CachePolicy::UseCached, &first, |_| {})
            .unwrap();
    assert_eq!(batch.cached_count, 0);
    for path in &paths {
        assert_eq!(stages(&first.events_of(path)), ORDER);
    }
    let second = Recorder::default();
    let batch =
        analyze_files_cached_observed(&mut cache, &paths, CachePolicy::UseCached, &second, |_| {})
            .unwrap();
    assert_eq!(batch.cached_count, paths.len());
    assert!(second.events.lock().unwrap().is_empty());
}
