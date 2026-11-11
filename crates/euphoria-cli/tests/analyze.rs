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
            assert_eq!(timeline[0]["key"], track["key"], "{track}");
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
    assert!(header.contains("BPM  Energy  Quality"), "{table}");
    assert!(table.contains("Lossless"), "{table}");
    assert!(table.contains("sine-440hz-mono.flac"), "{table}");
    let summary = String::from_utf8_lossy(&output.stderr);
    assert!(summary.contains("Quality: 1 lossless."), "{summary}");
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
