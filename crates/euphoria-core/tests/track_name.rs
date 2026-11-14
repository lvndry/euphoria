// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading the title and artist of real files in every supported format, and
//! of files that carry no tags or are not audio.

use std::path::{Path, PathBuf};

use euphoria_core::{EuphoriaError, TrackName, read_track_name};

fn tests_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

#[test]
fn reads_title_and_artist_in_every_supported_format() {
    let expected = TrackName {
        title: Some("Low Tide".to_string()),
        artist: Some("Saffron & Co".to_string()),
        genre: None,
    };
    for name in [
        "tagged.mp3",
        "tagged.m4a",
        "tagged.flac",
        "tagged.wav",
        "tagged.aiff",
    ] {
        let path = tests_directory().join("tagged-fixtures").join(name);
        assert_eq!(
            read_track_name(&path).expect("the tags are read"),
            expected,
            "{name}"
        );
    }
}

#[test]
fn a_file_without_tags_has_neither_title_nor_artist() {
    for name in [
        "sine-440hz-mono.mp3",
        "sine-440hz-mono.m4a",
        "sine-440hz-mono.flac",
        "sine-440hz-mono.wav",
        "sine-440hz-mono.aiff",
    ] {
        let path = tests_directory().join("fixtures").join(name);
        assert_eq!(
            read_track_name(&path).expect("the tags are read"),
            TrackName::default(),
            "{name}"
        );
    }
}

#[test]
fn a_file_that_is_not_audio_is_a_decode_error() {
    let readme = tests_directory().join("fixtures").join("README.md");
    assert!(matches!(
        read_track_name(&readme),
        Err(EuphoriaError::Decode { .. })
    ));
}

#[test]
fn a_missing_file_is_reported_as_missing() {
    assert!(matches!(
        read_track_name(Path::new("/definitely/not/here.mp3")),
        Err(EuphoriaError::Missing { .. })
    ));
}
