// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `euphoria plan` refuses what it cannot plan and says why.

mod support;

use support::{Euphoria, TemporaryFolder, fixtures_directory, standard_error};

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
