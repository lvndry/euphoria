// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `euphoria analyze --collection` on the core crate's audio fixtures, with
//! a collection written to the rekordbox XML format description (not
//! exported by rekordbox itself).

mod support;

use std::path::Path;

use support::{Euphoria, TemporaryFolder, fixtures_directory};

fn location(path: &Path) -> String {
    let text = path.to_str().expect("UTF-8 path");
    format!(
        "file://localhost{}",
        text.replace(' ', "%20").replace('&', "&amp;")
    )
}

fn collection_file(folder: &TemporaryFolder, entries: &[(&str, &str)]) -> String {
    let tracks: String = entries
        .iter()
        .map(|(file, tempo)| {
            format!(
                "<TRACK Name=\"{file}\" Location=\"{}\">{tempo}</TRACK>",
                location(&fixtures_directory().join(file))
            )
        })
        .collect();
    let path = folder.path().join("collection.xml");
    std::fs::write(
        &path,
        format!("<DJ_PLAYLISTS><COLLECTION>{tracks}</COLLECTION></DJ_PLAYLISTS>"),
    )
    .expect("the collection is written");
    path.to_str().expect("UTF-8 path").to_string()
}

fn analyze_json(collection: &str) -> serde_json::Value {
    let fixtures = fixtures_directory();
    let output = Euphoria::new().run(&[
        "analyze",
        "--json",
        "--no-cache",
        "--collection",
        collection,
        fixtures.to_str().expect("UTF-8 path"),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("the output is JSON")
}

fn track<'tracks>(tracks: &'tracks serde_json::Value, file: &str) -> &'tracks serde_json::Value {
    tracks
        .as_array()
        .expect("an array")
        .iter()
        .find(|track| {
            track["path"]
                .as_str()
                .is_some_and(|path| path.ends_with(file))
        })
        .expect("the file is in the output")
}

#[test]
fn json_names_the_source_of_every_grid() {
    let folder = TemporaryFolder::new();
    let collection = collection_file(
        &folder,
        &[
            (
                "sine-440hz-mono.wav",
                r#"<TEMPO Inizio="0.250" Bpm="120.00" Metro="4/4" Battito="1"/>"#,
            ),
            (
                "sine-440hz-mono.flac",
                r#"<TEMPO Inizio="0.0" Bpm="120.00" Metro="4/4" Battito="1"/><TEMPO Inizio="10.0" Bpm="126.00" Metro="4/4" Battito="1"/>"#,
            ),
            ("sine-440hz-mono.mp3", ""),
        ],
    );
    let tracks = analyze_json(&collection);

    let used = track(&tracks, "sine-440hz-mono.wav");
    assert_eq!(used["beat_grid_source"], "rekordbox", "{used}");
    assert_eq!(used["rekordbox_grid"], "used", "{used}");
    assert_eq!(used["bpm"], 120.0, "{used}");
    assert_eq!(used["beat_grid"]["bpm"], 120.0, "{used}");
    assert_eq!(used["beat_grid"]["first_downbeat_seconds"], 0.25, "{used}");
    assert!(used["beat_grid"]["confidence"].is_null(), "{used}");

    let variable = track(&tracks, "sine-440hz-mono.flac");
    assert_eq!(variable["rekordbox_grid"], "variable", "{variable}");
    assert_ne!(variable["beat_grid_source"], "rekordbox", "{variable}");

    assert_eq!(
        track(&tracks, "sine-440hz-mono.mp3")["rekordbox_grid"],
        "no grid"
    );
    assert_eq!(
        track(&tracks, "sine-440hz-mono.ogg")["rekordbox_grid"],
        "not in collection"
    );
}

#[test]
fn without_a_collection_there_is_no_rekordbox_status() {
    let fixtures = fixtures_directory();
    let output = Euphoria::new().run(&[
        "analyze",
        "--json",
        "--no-cache",
        fixtures.to_str().expect("UTF-8 path"),
    ]);
    let tracks: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON");
    for track in tracks.as_array().expect("an array") {
        assert!(track["rekordbox_grid"].is_null(), "{track}");
        assert_ne!(track["beat_grid_source"], "rekordbox", "{track}");
    }
}

#[test]
fn a_file_that_is_not_a_collection_is_refused_before_analyzing() {
    let folder = TemporaryFolder::new();
    let path = folder.path().join("not-a-collection.xml");
    std::fs::write(&path, "<other/>").expect("written");
    let fixtures = fixtures_directory();
    let output = Euphoria::new().run(&[
        "analyze",
        "--collection",
        path.to_str().expect("UTF-8 path"),
        fixtures.to_str().expect("UTF-8 path"),
    ]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("not a rekordbox XML collection"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
