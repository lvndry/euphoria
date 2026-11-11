// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A key and a tempo the user set in the desktop app, kept in the cache,
//! replace the detected ones in every command and are marked as set by the
//! user.

mod support;

use std::path::Path;
use std::process::Output;

use euphoria_core::{Cache, Correction, Key, Mode};
use support::{Euphoria, TemporaryFolder, fixtures_directory, standard_error};

fn text(path: &Path) -> &str {
    path.to_str().expect("UTF-8 path")
}

fn assert_success(output: &Output) {
    assert!(output.status.success(), "{}", standard_error(output));
}

/// A copy of a fixture analyzed once, with a correction of its key and
/// tempo kept in the cache as the app keeps it.
fn corrected_track(euphoria: &Euphoria, folder: &TemporaryFolder) -> std::path::PathBuf {
    let track = folder.path().join("track.wav");
    std::fs::copy(fixtures_directory().join("sine-440hz-mono.wav"), &track)
        .expect("the fixture is copied");
    assert_success(&euphoria.run(&["analyze", text(&track)]));
    let mut cache = Cache::open(&euphoria.cache_path()).expect("the cache opens");
    let correction = Correction::of_key(Key::new(4, Mode::Minor))
        .with_bpm(Some(87.0))
        .expect("a tempo that can be kept");
    cache
        .set_correction(&track, &correction)
        .expect("the correction is kept");
    track
}

#[test]
fn analyze_shows_and_marks_the_values_the_user_set() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let track = corrected_track(&euphoria, &folder);

    let json = euphoria.run(&["analyze", "--json", text(&track)]);
    assert_success(&json);
    let tracks: serde_json::Value = serde_json::from_slice(&json.stdout).expect("JSON");
    assert_eq!(tracks[0]["key"]["camelot"], "9A");
    assert_eq!(tracks[0]["bpm"], 87.0);
    assert_eq!(tracks[0]["set_by_user"], serde_json::json!(["key", "bpm"]));
    assert_eq!(tracks[0]["needs_review"], false);

    let table = euphoria.run(&["analyze", text(&track)]);
    assert_success(&table);
    let table = String::from_utf8_lossy(&table.stdout);
    assert!(table.contains("9A") && table.contains("87.0"), "{table}");
    assert!(table.contains("U/U"), "{table}");
}

#[test]
fn a_correction_outlives_a_new_analysis_and_is_ignored_without_the_cache() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let track = corrected_track(&euphoria, &folder);

    let reanalyzed = euphoria.run(&["analyze", "--json", "--reanalyze", text(&track)]);
    assert_success(&reanalyzed);
    let tracks: serde_json::Value = serde_json::from_slice(&reanalyzed.stdout).expect("JSON");
    assert_eq!(tracks[0]["bpm"], 87.0);

    let without_cache = euphoria.run(&["analyze", "--json", "--no-cache", text(&track)]);
    assert_success(&without_cache);
    let tracks: serde_json::Value = serde_json::from_slice(&without_cache.stdout).expect("JSON");
    assert_eq!(tracks[0]["set_by_user"], serde_json::json!([]));
}

#[test]
fn tags_and_the_rekordbox_export_get_the_values_the_user_set() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let track = corrected_track(&euphoria, &folder);

    let preview = euphoria.run(&["tags", "write", "--dry-run", text(&track)]);
    assert_success(&preview);
    let preview = String::from_utf8_lossy(&preview.stdout);
    assert!(preview.contains("-> 9A"), "{preview}");
    assert!(preview.contains("-> 87"), "{preview}");

    let exported = folder.path().join("rekordbox.xml");
    let export = euphoria.run(&[
        "export",
        "rekordbox",
        text(&track),
        "--output",
        text(&exported),
    ]);
    assert_success(&export);
    let xml = std::fs::read_to_string(&exported).expect("the export is written");
    assert!(xml.contains("AverageBpm=\"87.00\""), "{xml}");
    assert!(xml.contains("Tonality=\"9A\""), "{xml}");
}
