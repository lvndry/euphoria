// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `euphoria analyze` compares the key already in a file's tags with the
//! detected key, and never changes the file.

mod support;

use std::path::Path;

use euphoria_core::Key;
use euphoria_core::tags::{FieldEdit, TagField, TextChange, apply_plan, plan_edit};
use support::{Euphoria, TemporaryFolder, fixtures_directory};

fn analyze_json(euphoria: &Euphoria, arguments: &[&str]) -> Vec<serde_json::Value> {
    let mut all_arguments = vec!["analyze", "--json"];
    all_arguments.extend_from_slice(arguments);
    let output = euphoria.run(&all_arguments);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tracks: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON");
    tracks.as_array().expect("an array").clone()
}

fn write_key(path: &Path, text: &str) {
    let edit = FieldEdit {
        field: TagField::Key,
        change: TextChange::Set(text.to_string()),
    };
    apply_plan(&plan_edit(path, &[edit]).expect("planned")).expect("written");
}

#[test]
fn a_key_a_fifth_from_the_detected_one_is_flagged_and_the_file_is_left_alone() {
    let folder = TemporaryFolder::new();
    let path = folder.path().join("tone.mp3");
    std::fs::copy(fixtures_directory().join("sine-440hz-mono.mp3"), &path).expect("copied");
    let path_text = path.to_str().expect("UTF-8 path");
    let euphoria = Euphoria::new();

    let untagged = analyze_json(&euphoria, &[path_text]);
    assert!(untagged[0]["tag_key"].is_null(), "{}", untagged[0]);
    let detected: Key = untagged[0]["key"]["name"]
        .as_str()
        .expect("a 440 Hz tone has a key")
        .parse()
        .expect("a key name");

    let a_fifth_up = detected.transposed(7);
    write_key(&path, &a_fifth_up.short_name());
    let tagged_bytes = std::fs::read(&path).expect("read");

    let tagged = analyze_json(&euphoria, &[path_text]);
    let tag_key = &tagged[0]["tag_key"];
    assert_eq!(tag_key["text"], a_fifth_up.short_name(), "{tag_key}");
    assert_eq!(tag_key["key"]["camelot"], a_fifth_up.camelot(), "{tag_key}");
    assert_eq!(tag_key["agreement"], "fifth", "{tag_key}");
    assert_eq!(tagged[0]["needs_review"], true);

    let listed = analyze_json(&euphoria, &["--needs-review", path_text]);
    assert_eq!(listed.len(), 1, "the conflict is listed for review");

    assert_eq!(
        std::fs::read(&path).expect("read"),
        tagged_bytes,
        "analyze never changes the file"
    );
}

#[test]
fn the_key_euphoria_wrote_agrees_with_the_detected_key() {
    let folder = TemporaryFolder::new();
    let path = folder.path().join("tone.flac");
    std::fs::copy(fixtures_directory().join("sine-440hz-mono.flac"), &path).expect("copied");
    let path_text = path.to_str().expect("UTF-8 path");
    let euphoria = Euphoria::new();

    let untagged = analyze_json(&euphoria, &[path_text]);
    let camelot = untagged[0]["key"]["camelot"]
        .as_str()
        .expect("a key")
        .to_string();
    write_key(&path, &camelot);

    let tagged = analyze_json(&euphoria, &[path_text]);
    assert_eq!(tagged[0]["tag_key"]["agreement"], "same");
    assert_eq!(tagged[0]["needs_review"], untagged[0]["needs_review"]);
}
