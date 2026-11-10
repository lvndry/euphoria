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
    assert!(table.contains("sine-440hz-mono.flac"), "{table}");
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
