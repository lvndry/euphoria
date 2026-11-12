// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Checks what `analyze_file` reports beyond key, tempo and energy, with and
//! without the `libkeyfinder` feature.

use std::path::PathBuf;

use euphoria_core::{BeatGrid, Confidence, Key, TempoRange, TrackAnalysis, analyze_file};

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
        quality: None,
        key_runner_up: Some("C major".parse().expect("a key name")),
        key_score_margin: Some(key_score_margin),
        key_source: None,
        tempo_estimate: Some(tempo_estimate),
        tempo_score_margin: Some(tempo_score_margin),
        beat_grid: None,
        cues: Vec::new(),
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
fn another_range_moves_the_beat_grid_by_whole_octaves_and_keeps_its_downbeat_in_the_first_bar() {
    let grid = BeatGrid {
        first_downbeat_seconds: 1.8,
        bpm: 100.0,
        salience: 12.0,
        drift_seconds: None,
    };
    let analysis = TrackAnalysis {
        beat_grid: Some(grid),
        cues: Vec::new(),
        ..rated(0.02, 100.0, 0.5)
    };

    let doubled = analysis
        .clone()
        .with_tempo_range("120-220".parse().unwrap())
        .beat_grid
        .expect("a grid");
    assert!((doubled.bpm - 200.0).abs() < 1e-9);
    assert!((doubled.first_downbeat_seconds - 0.6).abs() < 1e-9);
    assert!(doubled.first_downbeat_seconds < doubled.bar_seconds());

    let fast = TrackAnalysis {
        beat_grid: Some(BeatGrid {
            bpm: 170.0,
            first_downbeat_seconds: 1.2,
            ..grid
        }),
        ..rated(0.02, 170.0, 0.5)
    };
    let halved = fast
        .clone()
        .with_tempo_range("70-140".parse().unwrap())
        .beat_grid
        .expect("a grid");
    assert!((halved.bpm - 85.0).abs() < 1e-9);
    assert!((halved.first_downbeat_seconds - 1.2).abs() < 1e-9);
    assert_eq!(fast.clone().with_tempo_range(TempoRange::DEFAULT), fast);
}

#[test]
fn a_tempo_that_is_not_a_positive_number_has_no_place_in_a_range() {
    for unusable in [0.0, -128.0, f32::NAN, f32::INFINITY] {
        let analysis = TrackAnalysis {
            bpm: Some(unusable),
            tempo_estimate: None,
            ..rated(0.02, 124.0, 0.5)
        };
        assert_eq!(analysis.tempo_in(TempoRange::DEFAULT), None, "{unusable}");
        assert_eq!(analysis.tempo_alternative_in(TempoRange::DEFAULT), None);
    }
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
        beat_grid: None,
        cues: Vec::new(),
        ..confident
    };
    assert_eq!(fallback_tempo.tempo_confidence(), Some(Confidence::Low));
    assert!(fallback_tempo.needs_review());
}

#[test]
fn a_fitted_grid_follows_the_reported_tempos_octave_across_range_boundaries() {
    // Grid fitting refines the estimate by up to 2%, so either boundary
    // can lie between the two tempos even though they count the same beats.
    for (estimated, fitted) in [(90.1, 89.8), (179.9, 180.2)] {
        let analysis = TrackAnalysis {
            beat_grid: Some(BeatGrid {
                first_downbeat_seconds: 0.5,
                bpm: fitted,
                salience: 12.0,
                drift_seconds: Some(0.0),
            }),
            ..rated(0.02, estimated, 0.5)
        };
        for range in [
            TempoRange::DEFAULT,
            "70-140".parse().unwrap(),
            "120-220".parse().unwrap(),
        ] {
            let expected_bpm = range.fold(estimated);
            let expected_grid = fitted * f64::from(expected_bpm) / f64::from(estimated);
            let shown = analysis.clone().with_tempo_range(range);
            assert_eq!(shown.bpm, Some(expected_bpm));
            assert!(
                (shown.beat_grid.unwrap().bpm - expected_grid).abs() < 1e-5,
                "estimate {estimated}, fitted {fitted}, range {range}: {:?}",
                shown.beat_grid
            );
            let restored = shown.with_tempo_range(TempoRange::DEFAULT);
            assert_eq!(restored.bpm, analysis.bpm);
            assert!((restored.beat_grid.unwrap().bpm - fitted).abs() < 1e-9);
        }
    }
}

#[test]
fn grid_octave_projection_uses_f64_at_both_extremes_of_valid_tempo_ranges() {
    let grid = BeatGrid {
        first_downbeat_seconds: 0.5,
        bpm: 89.8,
        salience: 12.0,
        drift_seconds: None,
    };
    let analysis = TrackAnalysis {
        beat_grid: Some(grid),
        ..rated(0.02, 90.1, 0.5)
    };
    for range in [
        TempoRange::new(f32::MAX / 2.0, f32::MAX).unwrap(),
        TempoRange::new(f32::from_bits(1), f32::from_bits(2)).unwrap(),
    ] {
        let shown = analysis.clone().with_tempo_range(range);
        assert_eq!(shown.bpm, analysis.tempo_in(range));
        let projected = shown.beat_grid.unwrap();
        assert!(projected.bpm.is_finite() && projected.bpm > 0.0);
        assert!((0.0..projected.bar_seconds()).contains(&projected.first_downbeat_seconds));
        let shift = (projected.bpm / grid.bpm).log2();
        assert!((shift - shift.round()).abs() < 1e-9);
        let restored = shown.with_tempo_range(TempoRange::DEFAULT);
        assert!((restored.beat_grid.unwrap().bpm - grid.bpm).abs() < 1e-9);
    }
}
