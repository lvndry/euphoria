// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Playlists built in the desktop app, listed and exported from the command
//! line: to rekordbox as one playlist in its order, and as M3U8.

mod support;

use std::path::{Path, PathBuf};
use std::process::Output;

use euphoria_core::{PlaylistEntry, PlaylistStore, StoredPlaylist};
use support::{Euphoria, TemporaryFolder, fixtures_directory, standard_error};

fn text(path: &Path) -> &str {
    path.to_str().expect("UTF-8 path")
}

fn assert_success(output: &Output) {
    assert!(output.status.success(), "{}", standard_error(output));
}

/// Three copies of a fixture with different content, and a playlist "Peak
/// time" holding them in the order third, first, second, kept where the app
/// keeps it.
fn playlist_of_three(euphoria: &Euphoria, folder: &TemporaryFolder) -> Vec<PathBuf> {
    let fixture = std::fs::read(fixtures_directory().join("sine-440hz-mono.wav")).unwrap();
    let tracks: Vec<PathBuf> = ["first", "second", "third"]
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let path = folder.path().join(format!("{name}.wav"));
            let mut content = fixture.clone();
            content.extend(std::iter::repeat_n(0, index));
            std::fs::write(&path, content).unwrap();
            path
        })
        .collect();
    let entries = [2, 0, 1]
        .map(|index| PlaylistEntry::for_file(&tracks[index], None).unwrap())
        .to_vec();
    let mut store =
        PlaylistStore::open(&euphoria.data_directory().join("playlists.sqlite3")).unwrap();
    store
        .save(&[
            StoredPlaylist {
                name: "Peak time".to_string(),
                label: Some("\u{1f525}".to_string()),
                entries,
            },
            StoredPlaylist {
                name: "Empty".to_string(),
                label: None,
                entries: Vec::new(),
            },
        ])
        .unwrap();
    tracks
}

#[test]
fn the_playlists_are_listed_with_their_labels_and_counts() {
    let euphoria = Euphoria::new();
    let none = euphoria.run(&["playlists"]);
    assert_success(&none);
    assert!(String::from_utf8_lossy(&none.stdout).contains("No playlists yet"));

    let folder = TemporaryFolder::new();
    playlist_of_three(&euphoria, &folder);
    let listed = euphoria.run(&["playlists"]);
    assert_success(&listed);
    assert_eq!(
        String::from_utf8_lossy(&listed.stdout),
        "\u{1f525} Peak time  (3 tracks)\nEmpty  (0 tracks)\n"
    );
}

#[test]
fn a_playlist_goes_to_rekordbox_in_its_own_order() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let tracks = playlist_of_three(&euphoria, &folder);
    let exported = folder.path().join("set.xml");

    let export = euphoria.run(&[
        "export",
        "rekordbox",
        "--playlist",
        "peak TIME",
        "--output",
        text(&exported),
    ]);
    assert_success(&export);
    let xml = std::fs::read_to_string(&exported).unwrap();
    assert!(xml.contains("Type=\"1\" Name=\"Peak time\""), "{xml}");
    let track_id = |name: &str| {
        let line = xml
            .lines()
            .find(|line| line.contains("TrackID") && line.contains(&format!("/{name}.wav")))
            .unwrap();
        let start = line.find("TrackID=\"").unwrap() + "TrackID=\"".len();
        line[start..].split('"').next().unwrap().to_string()
    };
    let playlist_order: Vec<String> = xml
        .lines()
        .filter_map(|line| line.trim().strip_prefix("<TRACK Key=\""))
        .map(|rest| rest.split('"').next().unwrap().to_string())
        .collect();
    assert_eq!(
        playlist_order,
        [track_id("third"), track_id("first"), track_id("second")],
        "{xml}"
    );
    assert_eq!(xml.matches("<TRACK TrackID").count(), tracks.len());
}

#[test]
fn a_playlist_goes_to_m3u8_and_a_moved_file_is_found() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let tracks = playlist_of_three(&euphoria, &folder);
    assert_success(&euphoria.run(&["analyze", text(folder.path())]));
    let moved = folder.path().join("renamed.wav");
    std::fs::rename(&tracks[0], &moved).unwrap();
    assert_success(&euphoria.run(&["analyze", text(folder.path())]));

    let exported = folder.path().join("set.m3u8");
    let export = euphoria.run(&[
        "export",
        "m3u8",
        "--playlist",
        "Peak time",
        "--output",
        text(&exported),
    ]);
    assert_success(&export);
    let playlist = std::fs::read_to_string(&exported).unwrap();
    let lines: Vec<&str> = playlist.lines().collect();
    assert_eq!(lines[0], "#EXTM3U");
    assert_eq!(lines[1], "#PLAYLIST:Peak time");
    assert_eq!(lines[2], "#EXTINF:1,third");
    assert_eq!(lines[3], text(&tracks[2]));
    assert_eq!(lines[5], text(&moved), "the first track was renamed");
    assert_eq!(lines.len(), 8);
}

#[test]
fn an_unknown_playlist_names_the_ones_there_are() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    playlist_of_three(&euphoria, &folder);
    let output = euphoria.run(&[
        "export",
        "m3u8",
        "--playlist",
        "Closing",
        "--output",
        text(&folder.path().join("set.m3u8")),
    ]);
    assert!(!output.status.success());
    let message = standard_error(&output);
    assert!(message.contains("\"Peak time\", \"Empty\""), "{message}");

    let both = euphoria.run(&[
        "export",
        "rekordbox",
        text(folder.path()),
        "--playlist",
        "Peak time",
        "--output",
        text(&folder.path().join("set.xml")),
    ]);
    assert!(!both.status.success());
}
