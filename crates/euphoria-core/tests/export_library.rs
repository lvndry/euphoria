// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Building an export from analysis results on real files, and writing it to
//! a file that is only ever whole.

use std::path::{Path, PathBuf};
use std::time::Duration;

use euphoria_core::export::rekordbox::{
    ExistingFile, ExportOptions, Library, Playlist, PlaylistNode, Track, write_xml_file,
};
use euphoria_core::{
    EuphoriaError, ExportProblem, FileResult, Key, Mode, PlaylistLayout, TrackAnalysis,
    assemble_library,
};

fn tests_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

fn analyzed(path: &Path) -> FileResult {
    FileResult {
        path: path.to_path_buf(),
        outcome: Ok(TrackAnalysis {
            duration_seconds: 61.4,
            key: Some(Key::new(9, Mode::Minor)),
            bpm: Some(124.0),
            energy: Some(7),
            tuning_cents: None,
            key_runner_up: None,
            key_score_margin: None,
            tempo_estimate: None,
            tempo_score_margin: None,
            beat_grid: None,
            cues: Vec::new(),
            key_timeline: Vec::new(),
            quality: None,
        }),
    }
}

fn scratch_folder(name: &str) -> PathBuf {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("the scratch folder is created");
    folder
}

fn file_names(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .expect("the folder is read")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn one_track_library() -> Library {
    Library {
        tracks: vec![Track::new("/music/a.mp3", Duration::from_secs(60))],
        playlists: Vec::new(),
    }
}

#[test]
fn tracks_carry_the_analysis_and_the_tags_and_only_failed_files_are_skipped() {
    let tagged = tests_directory()
        .join("tagged-fixtures")
        .join("tagged.flac");
    let untagged = tests_directory()
        .join("fixtures")
        .join("sine-440hz-mono.wav");
    let failed = tests_directory().join("fixtures").join("failed.mp3");
    let damaged_tag = scratch_folder("export-damaged-tag").join("Damaged Tag.mp3");
    let mut damaged_bytes =
        std::fs::read(tests_directory().join("tagged-fixtures").join("tagged.mp3"))
            .expect("the fixture is read");
    assert_eq!(&damaged_bytes[..3], b"ID3");
    // No ID3v2 tag has version 9, so the tag cannot be read.
    damaged_bytes[3] = 9;
    std::fs::write(&damaged_tag, damaged_bytes).expect("the damaged copy is written");
    let results = vec![
        analyzed(&tagged),
        FileResult {
            path: failed.clone(),
            outcome: Err(EuphoriaError::EmptyAudio {
                path: failed.clone(),
            }),
        },
        FileResult {
            path: untagged.clone(),
            outcome: Ok(TrackAnalysis {
                duration_seconds: 1.0,
                key: None,
                bpm: None,
                energy: None,
                tuning_cents: None,
                key_runner_up: None,
                key_score_margin: None,
                tempo_estimate: None,
                tempo_score_margin: None,
                beat_grid: None,
                cues: Vec::new(),
                key_timeline: Vec::new(),
                quality: None,
            }),
        },
        analyzed(&damaged_tag),
    ];

    let assembled = assemble_library(results, PlaylistLayout::CollectionOnly);

    assert_eq!(
        assembled.library.tracks,
        vec![
            Track {
                title: Some("Low Tide".to_string()),
                artist: Some("Saffron & Co".to_string()),
                key: Some(Key::new(9, Mode::Minor)),
                bpm: Some(124.0),
                comment: Some("Energy 7".to_string()),
                ..Track::new(&tagged, Duration::from_secs_f32(61.4))
            },
            Track {
                title: Some("sine-440hz-mono".to_string()),
                ..Track::new(&untagged, Duration::from_secs(1))
            },
            Track {
                title: Some("Damaged Tag".to_string()),
                key: Some(Key::new(9, Mode::Minor)),
                bpm: Some(124.0),
                comment: Some("Energy 7".to_string()),
                ..Track::new(&damaged_tag, Duration::from_secs_f32(61.4))
            },
        ]
    );
    assert!(assembled.library.playlists.is_empty());
    assert_eq!(assembled.skipped.len(), 1);
    assert_eq!(assembled.skipped[0].path, failed);
    assert!(matches!(
        assembled.skipped[0].reason,
        EuphoriaError::EmptyAudio { .. }
    ));
    assert_eq!(assembled.unread_tags.len(), 1);
    assert_eq!(assembled.unread_tags[0].path, damaged_tag);
    assert!(matches!(
        assembled.unread_tags[0].reason,
        EuphoriaError::Decode { .. }
    ));
}

#[test]
fn playlists_hold_only_the_tracks_that_were_exported() {
    let fixtures = tests_directory().join("fixtures");
    let exported = fixtures.join("sine-440hz-mono.flac");
    let failed = fixtures.join("failed.mp3");
    let folders = [fixtures.clone()];
    let failed_result = FileResult {
        path: failed.clone(),
        outcome: Err(EuphoriaError::EmptyAudio { path: failed }),
    };

    let assembled = assemble_library(
        vec![analyzed(&exported), failed_result],
        PlaylistLayout::MirrorFolders {
            folders: &folders,
            loose_playlist_name: "export",
        },
    );

    assert_eq!(
        assembled.library.playlists,
        vec![PlaylistNode::Playlist(Playlist {
            name: "fixtures".to_string(),
            tracks: vec![exported],
        })]
    );
}

#[test]
fn writes_the_file_and_leaves_nothing_else_behind() {
    let folder = scratch_folder("export-writes");
    let destination = folder.join("library.xml");

    write_xml_file(
        &destination,
        &one_track_library(),
        &ExportOptions::default(),
        ExistingFile::Keep,
    )
    .expect("the export is written");

    let document = std::fs::read_to_string(&destination).expect("the export is read");
    assert!(document.contains("Location=\"file://localhost/music/a.mp3\""));
    assert_eq!(file_names(&folder), ["library.xml"]);
}

#[test]
fn an_existing_file_is_kept_unless_it_is_to_be_replaced() {
    let folder = scratch_folder("export-existing");
    let destination = folder.join("library.xml");
    std::fs::write(&destination, "mine").expect("the existing file is written");

    let kept = write_xml_file(
        &destination,
        &one_track_library(),
        &ExportOptions::default(),
        ExistingFile::Keep,
    );
    assert!(matches!(
        kept,
        Err(EuphoriaError::ExportDestinationExists { path }) if path == destination
    ));
    assert_eq!(
        std::fs::read_to_string(&destination).expect("the file is read"),
        "mine"
    );

    write_xml_file(
        &destination,
        &one_track_library(),
        &ExportOptions::default(),
        ExistingFile::Replace,
    )
    .expect("the export replaces the file");
    assert!(
        std::fs::read_to_string(&destination)
            .expect("the file is read")
            .contains("<DJ_PLAYLISTS")
    );
    assert_eq!(file_names(&folder), ["library.xml"]);
}

#[test]
fn a_library_that_cannot_be_exported_leaves_the_existing_file_untouched() {
    let folder = scratch_folder("export-refused");
    let destination = folder.join("library.xml");
    std::fs::write(&destination, "mine").expect("the existing file is written");
    let library = Library {
        tracks: vec![Track::new("relative/a.mp3", Duration::from_secs(60))],
        playlists: Vec::new(),
    };

    let refused = write_xml_file(
        &destination,
        &library,
        &ExportOptions::default(),
        ExistingFile::Replace,
    );

    assert!(matches!(
        refused,
        Err(EuphoriaError::Export {
            problem: ExportProblem::NotAbsolutePath,
            ..
        })
    ));
    assert_eq!(
        std::fs::read_to_string(&destination).expect("the file is read"),
        "mine"
    );
    assert_eq!(file_names(&folder), ["library.xml"]);
}

#[test]
fn a_destination_that_cannot_be_written_is_an_error_and_leaves_no_temporary_file() {
    let folder = scratch_folder("export-unwritable");
    let missing_folder = folder.join("missing").join("library.xml");
    let in_the_way = folder.join("taken");
    std::fs::create_dir_all(in_the_way.join("inside")).expect("the folder is created");

    for (destination, existing) in [
        (&missing_folder, ExistingFile::Keep),
        (&in_the_way, ExistingFile::Replace),
    ] {
        let outcome = write_xml_file(
            destination,
            &one_track_library(),
            &ExportOptions::default(),
            existing,
        );
        assert!(
            matches!(&outcome, Err(EuphoriaError::ExportFile { path, .. }) if path == destination),
            "{outcome:?}"
        );
    }
    assert_eq!(file_names(&folder), ["taken"]);
}
