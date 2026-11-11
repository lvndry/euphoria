// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A key and a tempo the user set in the desktop app, kept in the cache,
//! replace the detected ones in every command and are marked as set by the
//! user.

mod support;

use std::path::Path;
use std::process::Output;

use euphoria_core::{ANALYZER_VERSION, Cache, ContentHash, Correction, Key, Mode};
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

#[test]
fn written_tags_hold_the_correction_and_a_later_analysis_still_reports_it() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let track = corrected_track(&euphoria, &folder);

    let written = euphoria.run(&["tags", "write", "--yes", text(&track)]);
    assert_success(&written);
    let tags = euphoria_core::tags::read_tags(&track).expect("the tags are read back");
    assert_eq!(tags.key.as_deref(), Some("9A"));
    assert_eq!(tags.bpm.as_deref(), Some("87"));

    let json = euphoria.run(&["analyze", "--json", text(&track)]);
    assert_success(&json);
    let tracks: serde_json::Value = serde_json::from_slice(&json.stdout).expect("JSON");
    assert_eq!(tracks[0]["key"]["camelot"], "9A");
    assert_eq!(tracks[0]["bpm"], 87.0);
    assert_eq!(tracks[0]["set_by_user"], serde_json::json!(["key", "bpm"]));
    let again = euphoria.run(&["tags", "write", "--dry-run", text(&track)]);
    assert_success(&again);
    assert!(
        !String::from_utf8_lossy(&again.stdout).contains("->"),
        "the tags already hold the corrected values"
    );
}

#[test]
fn a_correction_that_cannot_be_checked_leaves_the_file_out_of_tags_and_exports() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let track = corrected_track(&euphoria, &folder);
    let before = std::fs::read(&track).expect("the track is read");
    let connection = rusqlite::Connection::open(euphoria.cache_path()).expect("the cache opens");
    connection
        .execute_batch("ALTER TABLE corrections RENAME COLUMN bpm TO tempo")
        .expect("the cache is damaged on purpose");
    drop(connection);

    let written = euphoria.run(&["tags", "write", "--yes", text(&track)]);
    assert!(!written.status.success());
    assert!(
        standard_error(&written).contains("was corrected, so it was left out"),
        "{}",
        standard_error(&written)
    );
    assert_eq!(std::fs::read(&track).expect("the track is read"), before);

    let exported = folder.path().join("rekordbox.xml");
    let export = euphoria.run(&[
        "export",
        "rekordbox",
        text(&track),
        "--output",
        text(&exported),
    ]);
    assert!(!export.status.success());
    assert!(
        standard_error(&export).contains("was corrected, so it was left out"),
        "{}",
        standard_error(&export)
    );
    assert!(!exported.exists(), "the only file was left out");
}

#[cfg(unix)]
#[test]
fn an_unreadable_file_is_left_out_rather_than_written_with_detected_values() {
    use std::os::unix::fs::PermissionsExt;

    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let track = corrected_track(&euphoria, &folder);
    let other = folder.path().join("other.wav");
    std::fs::copy(fixtures_directory().join("sine-440hz-stereo.wav"), &other)
        .expect("the fixture is copied");
    assert_success(&euphoria.run(&["analyze", text(folder.path())]));
    std::fs::set_permissions(&track, std::fs::Permissions::from_mode(0o000))
        .expect("the track is made unreadable");

    let exported = folder.path().join("rekordbox.xml");
    let export = euphoria.run(&[
        "export",
        "rekordbox",
        text(folder.path()),
        "--output",
        text(&exported),
    ]);
    std::fs::set_permissions(&track, std::fs::Permissions::from_mode(0o644))
        .expect("the track is made readable again");
    assert!(!export.status.success());
    let xml = std::fs::read_to_string(&exported).expect("the other track is exported");
    assert!(xml.contains("other.wav"), "{xml}");
    assert!(!xml.contains("track.wav"), "{xml}");
    assert!(
        standard_error(&export).contains("Left 1 file out"),
        "{}",
        standard_error(&export)
    );
}

#[cfg(unix)]
#[test]
fn a_cache_made_unreadable_after_analysis_stops_tag_writing_but_not_analysis() {
    use std::os::unix::fs::PermissionsExt;

    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let track = corrected_track(&euphoria, &folder);
    let before = std::fs::read(&track).expect("the track is read");
    std::fs::set_permissions(
        euphoria.cache_path(),
        std::fs::Permissions::from_mode(0o000),
    )
    .expect("the cache is made unreadable");

    let written = euphoria.run(&["tags", "write", "--yes", text(&track)]);
    let shown = euphoria.run(&["analyze", text(&track)]);
    std::fs::set_permissions(
        euphoria.cache_path(),
        std::fs::Permissions::from_mode(0o644),
    )
    .expect("the cache is made readable again");

    assert!(!written.status.success());
    let errors = standard_error(&written);
    assert!(
        errors.contains("--no-cache") && errors.contains("Nothing was written"),
        "{errors}"
    );
    assert_eq!(std::fs::read(&track).expect("the track is read"), before);
    assert_success(&shown);
    assert!(
        standard_error(&shown).contains("warning: analyzing without the cache"),
        "{}",
        standard_error(&shown)
    );
}

#[test]
fn manual_tag_edits_after_cache_clear_keep_the_users_corrections() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let track = corrected_track(&euphoria, &folder);
    assert_success(&euphoria.run(&["cache", "clear"]));
    assert_success(&euphoria.run(&[
        "tags",
        "set",
        "--field",
        "genre=Techno",
        "--yes",
        text(&track),
    ]));
    let shown = euphoria.run(&["analyze", "--json", text(&track)]);
    assert_success(&shown);
    let tracks: serde_json::Value = serde_json::from_slice(&shown.stdout).unwrap();
    assert_eq!(tracks[0]["key"]["camelot"], "9A");
    assert_eq!(tracks[0]["bpm"], 87.0);
    assert_eq!(tracks[0]["set_by_user"], serde_json::json!(["key", "bpm"]));
}

fn export_lines(output: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(output)
        .lines()
        .map(|line| serde_json::from_str(line).expect("each line is JSON"))
        .collect()
}

#[test]
fn corrections_export_writes_one_line_per_correction_named_by_content_and_no_path() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let track = corrected_track(&euphoria, &folder);
    let detected_output = euphoria.run(&["analyze", "--json", "--no-cache", text(&track)]);
    let detected: serde_json::Value =
        serde_json::from_slice(&detected_output.stdout).expect("JSON");

    let exported = euphoria.run(&["corrections", "export"]);
    assert_success(&exported);
    let lines = export_lines(&exported.stdout);
    assert_eq!(lines.len(), 1);
    let line = &lines[0];
    assert_eq!(line["schema_version"], 1);
    assert_eq!(
        line["content_hash"],
        ContentHash::of_file(&track)
            .expect("the track is read")
            .to_hex()
    );
    assert_eq!(line["corrected_key"]["name"], "E minor");
    assert_eq!(line["corrected_key"]["tonic"], 4);
    assert_eq!(line["corrected_key"]["mode"], "minor");
    assert_eq!(line["corrected_bpm"], 87.0);
    assert_eq!(
        line["detected_key"]["camelot"],
        detected[0]["key"]["camelot"]
    );
    assert_eq!(line["analyzer_version"], ANALYZER_VERSION);
    assert!(
        line["corrected_at"]
            .as_i64()
            .is_some_and(|seconds| seconds > 0)
    );
    assert!(line.get("path").is_none(), "{line}");
    let raw = String::from_utf8_lossy(&exported.stdout);
    assert!(!raw.contains(text(folder.path())), "{raw}");
    assert!(
        standard_error(&exported).contains("Exported 1 correction"),
        "{}",
        standard_error(&exported)
    );
}

#[test]
fn corrections_export_to_a_file_keeps_an_existing_one_and_adds_paths_when_asked() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let track = corrected_track(&euphoria, &folder);
    let destination = folder.path().join("corrections.jsonl");
    let destination_text = text(&destination);

    assert_success(&euphoria.run(&["corrections", "export", "-o", destination_text]));
    let without_paths = std::fs::read(&destination).expect("the export is written");
    assert_eq!(export_lines(&without_paths).len(), 1);

    let refused = euphoria.run(&[
        "corrections",
        "export",
        "--include-paths",
        "-o",
        destination_text,
    ]);
    assert!(!refused.status.success());
    assert!(
        standard_error(&refused).contains("--force"),
        "{}",
        standard_error(&refused)
    );
    assert_eq!(std::fs::read(&destination).expect("kept"), without_paths);

    assert_success(&euphoria.run(&[
        "corrections",
        "export",
        "--include-paths",
        "--force",
        "-o",
        destination_text,
    ]));
    let with_paths = export_lines(&std::fs::read(&destination).expect("replaced"));
    assert_eq!(
        with_paths[0]["path"],
        text(&std::path::absolute(&track).expect("absolute"))
    );
}

#[test]
fn corrections_export_without_a_cache_writes_nothing_and_creates_no_cache() {
    let euphoria = Euphoria::new();
    let exported = euphoria.run(&["corrections", "export"]);
    assert_success(&exported);
    assert!(exported.stdout.is_empty());
    assert!(
        standard_error(&exported).contains("no corrections yet"),
        "{}",
        standard_error(&exported)
    );
    assert!(!euphoria.cache_path().exists());
}
