// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Checks what `analyze_file` reports beyond key, tempo and energy, with and
//! without the `libkeyfinder` feature.

use std::path::PathBuf;

use euphoria_core::{Confidence, Key, TempoRange, TrackAnalysis, analyze_file};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn a_tone_at_concert_pitch_has_a_tuning_of_about_zero() {
    let analysis = analyze_file(&fixture("sine-440hz-mono.wav")).unwrap();
    let tuning = analysis.tuning_cents.expect("a 440 Hz tone has a tuning");
    assert!(tuning.abs() < 2.0, "{tuning}");
}

#[test]
fn silence_has_no_tuning_and_no_key_over_time() {
    let analysis = analyze_file(&fixture("sine-440hz-opposite-phase-stereo.flac")).unwrap();
    assert_eq!(analysis.key, None);
    assert_eq!(analysis.tuning_cents, None);
    assert!(analysis.key_timeline.is_empty());
}

#[cfg(feature = "libkeyfinder")]
#[test]
fn a_track_in_one_key_is_one_section_in_that_key_from_start_to_end() {
    let analysis = analyze_file(&fixture("sine-440hz-mono.wav")).unwrap();
    let [section] = analysis.key_timeline[..] else {
        panic!("{:?}", analysis.key_timeline);
    };
    assert_eq!(Some(section.key), analysis.key);
    assert_eq!(section.start_seconds, 0.0);
    assert_eq!(section.end_seconds, analysis.duration_seconds);
}

#[cfg(not(feature = "libkeyfinder"))]
#[test]
fn without_libkeyfinder_there_is_no_key_over_time() {
    let analysis = analyze_file(&fixture("sine-440hz-mono.wav")).unwrap();
    assert!(analysis.key.is_some());
    assert!(analysis.key_timeline.is_empty());
}

#[test]
fn an_analysis_rates_its_tempo_and_keeps_the_estimate_behind_it() {
    let analysis = analyze_file(&fixture("sine-440hz-mono.wav")).unwrap();
    let estimate = analysis
        .tempo_estimate
        .expect("the tone has a tempo estimate");
    assert_eq!(analysis.bpm, Some(TempoRange::DEFAULT.fold(estimate)));
    assert!(analysis.tempo_confidence().is_some());
}

#[test]
fn silence_has_no_confidence_and_nothing_to_review() {
    let analysis = analyze_file(&fixture("sine-440hz-opposite-phase-stereo.flac")).unwrap();
    assert_eq!(analysis.key_confidence(), None);
    assert_eq!(analysis.tempo_confidence(), None);
    assert_eq!(analysis.key_runner_up, None);
    assert!(!analysis.needs_review());
}

#[cfg(feature = "libkeyfinder")]
#[test]
fn an_analysis_names_the_runner_up_key_and_rates_the_key() {
    let analysis = analyze_file(&fixture("sine-440hz-mono.wav")).unwrap();
    let runner_up = analysis.key_runner_up.expect("a tone has a runner up key");
    assert_ne!(Some(runner_up), analysis.key);
    assert!(analysis.key_score_margin.is_some());
    assert!(analysis.key_confidence().is_some());
}

fn rated(key_score_margin: f32, tempo_estimate: f32, tempo_score_margin: f32) -> TrackAnalysis {
    TrackAnalysis {
        duration_seconds: 300.0,
        key: Some("A minor".parse::<Key>().expect("a key name")),
        bpm: Some(TempoRange::DEFAULT.fold(tempo_estimate)),
        energy: Some(6),
        tuning_cents: None,
        key_timeline: Vec::new(),
        key_runner_up: Some("C major".parse().expect("a key name")),
        key_score_margin: Some(key_score_margin),
        tempo_estimate: Some(tempo_estimate),
        tempo_score_margin: Some(tempo_score_margin),
    }
}

#[test]
fn another_range_moves_the_tempo_without_a_new_analysis() {
    let analysis = rated(0.02, 87.0, 0.5);
    assert_eq!(analysis.bpm, Some(174.0));
    assert_eq!(
        analysis.tempo_alternative_in(TempoRange::DEFAULT),
        Some(87.0)
    );

    let hip_hop: TempoRange = "70-140".parse().unwrap();
    assert_eq!(analysis.tempo_in(hip_hop), Some(87.0));
    let moved = analysis.clone().with_tempo_range(hip_hop);
    assert_eq!(moved.bpm, Some(87.0));
    assert_eq!(moved.tempo_estimate, analysis.tempo_estimate);
    assert_eq!(moved.energy, analysis.energy);
    assert_eq!(moved.tempo_alternative_in(hip_hop), Some(174.0));
    assert_eq!(moved.with_tempo_range(TempoRange::DEFAULT), analysis);
}

#[test]
fn a_track_needs_review_when_its_key_or_its_tempo_is_rated_low() {
    let confident = rated(0.02, 124.0, 0.5);
    assert_eq!(confident.key_confidence(), Some(Confidence::High));
    assert_eq!(confident.tempo_confidence(), Some(Confidence::High));
    assert!(!confident.needs_review());

    assert!(rated(0.001, 124.0, 0.5).needs_review());
    assert!(rated(0.02, 124.0, 0.01).needs_review());
    assert!(!rated(0.005, 124.0, 0.2).needs_review());

    let fallback_tempo = TrackAnalysis {
        tempo_score_margin: None,
        ..confident
    };
    assert_eq!(fallback_tempo.tempo_confidence(), Some(Confidence::Low));
    assert!(fallback_tempo.needs_review());
}
