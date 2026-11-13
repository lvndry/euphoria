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
    let fixture = std::fs::read(fixtures_directory().join("sine-440hz-mono.wav"))
        .expect("the test can read and write its own files");
    let tracks: Vec<PathBuf> = ["first", "second", "third"]
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let path = folder.path().join(format!("{name}.wav"));
            let mut content = fixture.clone();
            content.extend(std::iter::repeat_n(0, index));
            std::fs::write(&path, content).expect("the test can read and write its own files");
            path
        })
        .collect();
    let entries = [2, 0, 1]
        .map(|index| {
            PlaylistEntry::for_file(&tracks[index], None)
                .expect("the test can read and write its own files")
        })
        .to_vec();
    let mut store = PlaylistStore::open(&euphoria.data_directory().join("playlists.sqlite3"))
        .expect("the test can read and write its own files");
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
        .expect("the test can read and write its own files");
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
    let xml =
        std::fs::read_to_string(&exported).expect("the test can read and write its own files");
    assert!(xml.contains("Type=\"1\" Name=\"Peak time\""), "{xml}");
    let track_id = |name: &str| {
        let line = xml
            .lines()
            .find(|line| line.contains("TrackID") && line.contains(&format!("/{name}.wav")))
            .expect("the test can read and write its own files");
        let start = line
            .find("TrackID=\"")
            .expect("the test can read and write its own files")
            + "TrackID=\"".len();
        line[start..]
            .split('"')
            .next()
            .expect("the test can read and write its own files")
            .to_string()
    };
    let playlist_order: Vec<String> = xml
        .lines()
        .filter_map(|line| line.trim().strip_prefix("<TRACK Key=\""))
        .map(|rest| {
            rest.split('"')
                .next()
                .expect("the test can read and write its own files")
                .to_string()
        })
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
    std::fs::rename(&tracks[0], &moved).expect("the test can read and write its own files");
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
    let playlist =
        std::fs::read_to_string(&exported).expect("the test can read and write its own files");
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

#[test]
fn a_file_replaced_at_its_path_by_other_audio_is_left_out_of_an_export() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let tracks = playlist_of_three(&euphoria, &folder);
    std::fs::copy(
        fixtures_directory().join("sine-440hz-stereo.wav"),
        &tracks[0],
    )
    .expect("the test can read and write its own files");

    let exported = folder.path().join("set.m3u8");
    let export = euphoria.run(&[
        "export",
        "m3u8",
        "--playlist",
        "Peak time",
        "--output",
        text(&exported),
    ]);
    assert!(!export.status.success());
    let errors = standard_error(&export);
    assert!(errors.contains("holds other audio"), "{errors}");
    let playlist =
        std::fs::read_to_string(&exported).expect("the test can read and write its own files");
    assert!(!playlist.contains(text(&tracks[0])), "{playlist}");
    assert!(playlist.contains(text(&tracks[2])), "{playlist}");
}

#[test]
fn tag_writes_and_undo_keep_saved_playlists_exportable_without_the_cache() {
    for edits in [
        vec!["tags", "write", "--yes", "--no-cache"],
        vec!["tags", "set", "--field", "genre=Techno", "--yes"],
    ] {
        let euphoria = Euphoria::new();
        let folder = TemporaryFolder::new();
        let tracks = playlist_of_three(&euphoria, &folder);
        let original: Vec<_> = tracks
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect();
        let mut arguments = edits.clone();
        arguments.push(text(folder.path()));
        assert_success(&euphoria.run(&arguments));

        let check_export = |name: &str| {
            let destination = folder.path().join(name);
            let exported = euphoria.run(&[
                "export",
                "m3u8",
                "--playlist",
                "Peak time",
                "-o",
                text(&destination),
                "--no-cache",
            ]);
            assert_success(&exported);
            let contents = std::fs::read_to_string(destination).unwrap();
            let paths: Vec<_> = contents
                .lines()
                .filter(|line| !line.starts_with('#') && !line.is_empty())
                .collect();
            assert_eq!(
                paths,
                [text(&tracks[2]), text(&tracks[0]), text(&tracks[1])],
                "{edits:?}: {contents}"
            );
        };
        check_export("written.m3u8");
        assert_success(&euphoria.run(&["tags", "undo"]));
        check_export("restored.m3u8");
        for (path, bytes) in tracks.iter().zip(original) {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
    }
}

#[test]
fn a_failed_playlist_update_is_reported_only_when_tags_actually_changed() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let track = folder.path().join("track.flac");
    std::fs::copy(fixtures_directory().join("sine-440hz-mono.flac"), &track).unwrap();
    let store = euphoria.data_directory().join("playlists.sqlite3");
    std::fs::write(&store, b"not a database").unwrap();
    let arguments = [
        "tags",
        "set",
        "--field",
        "genre=Techno",
        "--yes",
        text(&track),
    ];

    let changed = euphoria.run(&arguments);
    assert_success(&changed);
    assert!(
        standard_error(&changed)
            .contains("warning: could not keep saved playlists in step with tag changes")
    );
    assert_eq!(
        euphoria_core::tags::read_editable_tags(&track)
            .unwrap()
            .get(euphoria_core::tags::TagField::Genre),
        Some("Techno")
    );
    assert_eq!(std::fs::read(&store).unwrap(), b"not a database");

    let unchanged = euphoria.run(&arguments);
    assert_success(&unchanged);
    assert!(!standard_error(&unchanged).contains("saved playlists"));
}
