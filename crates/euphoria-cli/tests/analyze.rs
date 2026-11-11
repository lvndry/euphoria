// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs the `euphoria` binary on the core crate's audio fixtures.

mod support;

use support::{Euphoria, fixtures_directory};

#[test]
fn analyzes_a_folder_as_json() {
    let fixtures = fixtures_directory();
    let output =
        Euphoria::new().run(&["analyze", "--json", fixtures.to_str().expect("UTF-8 path")]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let tracks: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the output is JSON");
    let tracks = tracks.as_array().expect("the output is an array");
    assert_eq!(
        tracks.len(),
        7,
        "one entry per audio fixture, README skipped"
    );
    for track in tracks {
        assert!(track["error"].is_null(), "{track}");
        let duration = track["duration_seconds"].as_f64().expect("a duration");
        assert!((duration - 1.0).abs() < 0.1, "{track}");
        let path = track["path"].as_str().expect("a path");
        let energy = &track["energy"];
        if path.contains("opposite-phase") {
            assert!(energy.is_null(), "silence has no energy rating: {track}");
        } else {
            let rating = energy.as_u64().expect("a track with sound has a rating");
            assert!((1..=10).contains(&rating), "{track}");
        }
        let quality = &track["quality"];
        assert_eq!(quality["sample_rate"], 44_100, "{track}");
        let lossless = quality["lossless"].as_bool().expect("a lossless flag");
        assert_eq!(
            lossless,
            !(path.ends_with(".mp3") || path.ends_with(".m4a")),
            "{track}"
        );
        assert!(quality["reason"].as_str().is_some(), "{track}");
        let tuning = &track["tuning_cents"];
        let timeline = track["key_timeline"].as_array().expect("a key timeline");
        if path.contains("opposite-phase") {
            assert!(tuning.is_null(), "silence has no tuning: {track}");
            assert!(timeline.is_empty(), "silence has no key over time: {track}");
        } else {
            let cents = tuning.as_f64().expect("a 440 Hz tone has a tuning");
            assert!(
                cents.abs() < 2.0,
                "a 440 Hz tone is at concert pitch: {track}"
            );
            assert_eq!(timeline.len(), 1, "{track}");
            assert_eq!(timeline[0]["key"]["name"], track["key"]["name"], "{track}");
            assert_eq!(timeline[0]["start_seconds"], 0.0, "{track}");
            assert_eq!(timeline[0]["end_seconds"], track["duration_seconds"]);
        }
    }
}

#[test]
fn prints_a_table_with_a_header() {
    let fixture = fixtures_directory().join("sine-440hz-mono.flac");
    let output = Euphoria::new().run(&["analyze", fixture.to_str().expect("UTF-8 path")]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let table = String::from_utf8(output.stdout).expect("UTF-8 output");
    assert!(table.starts_with("Key"), "{table}");
    let header = table.lines().next().expect("a header line");
    assert!(header.contains("BPM  Energy  Conf  Quality"), "{table}");
    assert!(table.contains("Lossless"), "{table}");
    assert!(table.contains("sine-440hz-mono.flac"), "{table}");
    let summary = String::from_utf8_lossy(&output.stderr);
    assert!(summary.contains("Quality: 1 lossless."), "{summary}");
}

fn analyze_json(arguments: &[&str]) -> Vec<serde_json::Value> {
    let fixtures = fixtures_directory();
    let mut command = vec!["analyze", "--json", "--no-cache"];
    command.extend_from_slice(arguments);
    command.push(fixtures.to_str().expect("UTF-8 path"));
    let output = Euphoria::new().run(&command);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tracks: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the output is JSON");
    tracks.as_array().expect("the output is an array").clone()
}

#[test]
fn json_rates_the_key_and_the_tempo_and_names_the_runner_up() {
    for track in analyze_json(&[]) {
        if track["path"].as_str().unwrap().contains("opposite-phase") {
            assert!(track["runner_up_key"].is_null(), "{track}");
            assert_eq!(track["needs_review"], false, "{track}");
            continue;
        }
        let confidence = track["key"]["confidence"].as_str().expect("a key rating");
        assert!(["high", "medium", "low"].contains(&confidence), "{track}");
        assert!(track["key"]["open_key"].is_string(), "{track}");
        assert!(track["runner_up_key"]["camelot"].is_string(), "{track}");
        assert_ne!(track["runner_up_key"]["name"], track["key"]["name"]);
        let tempo_confidence = track["bpm_confidence"].as_str().expect("a tempo rating");
        assert!(
            ["high", "medium", "low"].contains(&tempo_confidence),
            "{track}"
        );
        assert!(track["needs_review"].is_boolean(), "{track}");
    }
}

#[test]
fn a_tempo_range_reports_every_tempo_inside_it() {
    let in_default = analyze_json(&[]);
    let in_hip_hop_range = analyze_json(&["--tempo-range", "70-140"]);
    let mut compared = 0;
    for (default, hip_hop) in in_default.iter().zip(&in_hip_hop_range) {
        assert_eq!(default["path"], hip_hop["path"]);
        let (Some(default_bpm), Some(hip_hop_bpm)) =
            (default["bpm"].as_f64(), hip_hop["bpm"].as_f64())
        else {
            continue;
        };
        assert!((70.0..140.0).contains(&hip_hop_bpm), "{hip_hop}");
        let octaves = (hip_hop_bpm / default_bpm).log2();
        assert!(
            (octaves - octaves.round()).abs() < 0.01,
            "{default_bpm} and {hip_hop_bpm} are not octaves of each other"
        );
        compared += 1;
    }
    assert!(compared > 0, "no fixture has a tempo");
}

#[test]
fn a_tempo_range_wider_than_an_octave_is_refused() {
    let fixture = fixtures_directory().join("sine-440hz-mono.flac");
    let output = Euphoria::new().run(&[
        "analyze",
        "--tempo-range",
        "60-200",
        fixture.to_str().expect("UTF-8 path"),
    ]);
    assert!(!output.status.success());
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(errors.contains("at most an octave wide"), "{errors}");
}

#[test]
fn a_large_finite_tempo_range_does_not_turn_a_detected_tempo_into_null() {
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    let output = Euphoria::new().run(&[
        "analyze",
        "--json",
        "--no-cache",
        "--tempo-range",
        "2e38-3e38",
        fixture.to_str().expect("UTF-8 path"),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tracks: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let tempo = tracks[0]["bpm"]
        .as_f64()
        .expect("a detected tempo stays a number");
    assert!(tempo.is_finite() && tempo > 0.0, "{tracks}");
    assert!(tracks[0]["error"].is_null(), "{tracks}");
}

#[test]
fn needs_review_lists_only_tracks_rated_low() {
    let all = analyze_json(&[]);
    let to_review = analyze_json(&["--needs-review"]);
    let expected: Vec<&serde_json::Value> = all
        .iter()
        .filter(|track| track["needs_review"] == true)
        .collect();
    assert_eq!(to_review.len(), expected.len());
    for (listed, expected) in to_review.iter().zip(expected) {
        assert_eq!(listed["path"], expected["path"]);
        let rated_low = listed["key"]["confidence"] == "low" || listed["bpm_confidence"] == "low";
        assert!(rated_low, "{listed}");
    }
    assert!(
        to_review
            .iter()
            .all(|track| !track["path"].as_str().unwrap().contains("opposite-phase")),
        "silence has nothing to review"
    );
}

#[test]
fn reports_an_unreadable_file_and_exits_with_failure() {
    let readme = fixtures_directory().join("README.md");
    let output = Euphoria::new().run(&["analyze", readme.to_str().expect("UTF-8 path")]);
    assert!(!output.status.success());
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(errors.contains("Could not analyze 1 file:"), "{errors}");
    assert!(errors.contains("README.md"), "{errors}");
}

#[test]
fn a_missing_path_is_an_error() {
    let output = Euphoria::new().run(&["analyze", "/definitely/not/here"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("/definitely/not/here"));
}

#[cfg(unix)]
#[test]
fn a_reader_that_stops_early_is_not_an_error() {
    use std::io::Read;
    use std::process::Stdio;

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_euphoria"))
        .args(["analyze", "--no-cache"])
        .arg(fixtures_directory())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the euphoria binary runs");
    drop(child.stdout.take());

    let mut errors = String::new();
    child
        .stderr
        .take()
        .expect("standard error is piped")
        .read_to_string(&mut errors)
        .expect("standard error is readable");
    let status = child.wait().expect("the process exits");

    assert!(status.success(), "{errors}");
    assert!(!errors.contains("Broken pipe"), "{errors}");
}

#[test]
fn piped_output_has_no_progress_lines() {
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    let folder = fixtures_directory();
    let euphoria = Euphoria::new();
    for arguments in [
        vec!["analyze", fixture.to_str().expect("UTF-8 path")],
        vec!["analyze", "--json", fixture.to_str().expect("UTF-8 path")],
        vec!["analyze", folder.to_str().expect("UTF-8 path")],
    ] {
        let output = euphoria.run(&arguments);
        assert!(output.status.success(), "{arguments:?}");
        let standard_error = String::from_utf8_lossy(&output.stderr);
        // No stage, spinner, bar or debug build note when standard error is
        // not a terminal.
        for unwanted in [
            "decoding",
            "finding the",
            "Analyzing",
            "debug build",
            "\r",
            "\u{1b}",
        ] {
            assert!(
                !standard_error.contains(unwanted),
                "{arguments:?} wrote {unwanted:?} to standard error: {standard_error:?}"
            );
        }
        let standard_output = String::from_utf8_lossy(&output.stdout);
        assert!(!standard_output.contains("decoding"), "{standard_output}");
        assert!(!standard_output.contains('\u{1b}'), "{standard_output:?}");
    }
}
