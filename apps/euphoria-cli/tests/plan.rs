// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `euphoria plan` refuses what it cannot plan and says why.

mod support;

use support::{Euphoria, TemporaryFolder, fixtures_directory, standard_error, write_kick_track};

#[test]
fn plan_reports_a_missing_file() {
    let app = Euphoria::new();
    let folder = TemporaryFolder::new();
    let missing = folder.path().join("missing.flac");
    let present = fixtures_directory().join("sine-440hz-mono.flac");
    let output = app.run(&["plan", present.to_str().unwrap(), missing.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(standard_error(&output).contains("is not a file"));
}

#[test]
fn plan_says_when_a_track_has_no_beat_grid() {
    let app = Euphoria::new();
    let sine = fixtures_directory().join("sine-440hz-mono.flac");
    let path = sine.to_str().unwrap();
    let output = app.run(&["plan", path, path]);
    assert!(!output.status.success());
    assert!(
        standard_error(&output).contains("has no beat grid"),
        "{}",
        standard_error(&output)
    );
}

fn two_kick_tracks() -> (TemporaryFolder, std::path::PathBuf, std::path::PathBuf) {
    let folder = TemporaryFolder::new();
    let first = folder.path().join("first.wav");
    let second = folder.path().join("second.wav");
    write_kick_track(&first, 120.0, 0.25);
    write_kick_track(&second, 123.0, 0.25);
    (folder, first, second)
}

#[test]
fn plan_says_why_no_window_fits() {
    let app = Euphoria::new();
    let (_folder, first, second) = two_kick_tracks();
    let output = app.run(&[
        "plan",
        "--no-cache",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", standard_error(&output));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("No window of 16, 32 or 64 bars fits these two tracks: A has 1"),
        "{text}"
    );
    assert!(
        text.contains(" bars, and the smallest window needs 16."),
        "{text}"
    );
}

#[test]
fn plan_json_has_a_schema_version_and_rounded_numbers() {
    let app = Euphoria::new();
    let (_folder, first, second) = two_kick_tracks();
    let output = app.run(&[
        "plan",
        "--json",
        "--no-cache",
        first.to_str().unwrap(),
        second.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", standard_error(&output));
    let text = String::from_utf8_lossy(&output.stdout);
    let plan: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!(plan["schema_version"], 1);
    let decimals = |value: &serde_json::Value| {
        let text = value.to_string();
        text.split_once('.').map_or(0, |(_, after)| after.len())
    };
    assert!(decimals(&plan["elapsed_milliseconds"]) <= 3, "{text}");
    assert!(decimals(&plan["plan"]["alignment"]["bpm_a"]) <= 2, "{text}");
    assert!(decimals(&plan["plan"]["alignment"]["bpm_b"]) <= 2, "{text}");
    assert!(decimals(&plan["plan"]["alignment"]["rate"]) <= 3, "{text}");
}

#[test]
fn plan_json_into_a_closed_pipe_is_quiet() {
    use std::process::Stdio;

    let app = Euphoria::new();
    let (_folder, first, second) = two_kick_tracks();
    let mut child = app
        .command(&[
            "plan",
            "--json",
            "--no-cache",
            first.to_str().unwrap(),
            second.to_str().unwrap(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary runs");
    drop(child.stdout.take());
    let output = child.wait_with_output().expect("the command ends");
    assert!(output.status.success(), "{}", standard_error(&output));
    assert!(!standard_error(&output).contains("Broken pipe"));
}
