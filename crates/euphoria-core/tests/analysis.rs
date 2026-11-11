// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Checks what `analyze_file` reports beyond key, tempo and energy, with and
//! without the `libkeyfinder` feature.

use std::path::PathBuf;

use euphoria_core::analyze_file;

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
