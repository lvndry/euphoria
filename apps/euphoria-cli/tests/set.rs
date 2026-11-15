// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `euphoria set build` on the core crate's audio fixtures.

mod support;

use support::{Euphoria, TemporaryFolder, fixtures_directory, standard_error};

fn pool_of_two_tones() -> TemporaryFolder {
    let folder = TemporaryFolder::new();
    let pool = folder.path().join("pool");
    std::fs::create_dir(&pool).expect("the pool folder is created");
    for fixture in ["sine-440hz-mono.wav", "sine-440hz-stereo.wav"] {
        std::fs::copy(fixtures_directory().join(fixture), pool.join(fixture))
            .expect("the fixture is copied");
    }
    folder
}

#[test]
fn a_relative_pool_is_exported_to_m3u8() {
    let euphoria = Euphoria::new();
    let folder = pool_of_two_tones();
    let output = euphoria
        .command(&["set", "build", "pool", "--tracks", "2", "--m3u", "out.m3u8"])
        .current_dir(folder.path())
        .output()
        .expect("the binary runs");
    assert!(output.status.success(), "{}", standard_error(&output));

    let playlist =
        std::fs::read_to_string(folder.path().join("out.m3u8")).expect("the playlist is written");
    let entries: Vec<&str> = playlist
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .collect();
    assert_eq!(entries.len(), 2, "{playlist}");
    for entry in entries {
        assert!(std::path::Path::new(entry).is_absolute(), "{entry}");
    }
}

#[test]
fn the_json_has_a_schema_version_and_absolute_paths() {
    let euphoria = Euphoria::new();
    let folder = pool_of_two_tones();
    let output = euphoria
        .command(&["set", "build", "pool", "--tracks", "2", "--json"])
        .current_dir(folder.path())
        .output()
        .expect("the binary runs");
    assert!(output.status.success(), "{}", standard_error(&output));
    let set: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON");
    assert_eq!(set["schema_version"], 1);
    for track in set["tracks"].as_array().expect("tracks") {
        let path = track["path"].as_str().expect("a path");
        assert!(std::path::Path::new(path).is_absolute(), "{path}");
    }
}

#[test]
fn a_move_is_named_in_words_without_a_wrong_article() {
    let euphoria = Euphoria::new();
    let folder = pool_of_two_tones();
    let output = euphoria.run(&[
        "set",
        "build",
        folder.path().join("pool").to_str().expect("UTF-8 path"),
        "--tracks",
        "2",
    ]);
    assert!(output.status.success());
    let table = String::from_utf8_lossy(&output.stdout);
    assert!(!table.contains("Adjacent"), "{table}");
    assert!(!standard_error(&output).contains("a adjacent"));
}
