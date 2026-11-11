// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `euphoria tags set`: hand edits of tag fields across many files, with
//! the preview, confirmation and undo of `tags write`.

mod support;

use std::path::{Path, PathBuf};
use std::process::Output;

use euphoria_core::tags::{TagField, read_editable_tags};
use support::{Euphoria, TemporaryFolder, fixtures_directory, standard_error};

fn text(path: &Path) -> &str {
    path.to_str().expect("UTF-8 path")
}

fn assert_success(output: &Output) {
    assert!(output.status.success(), "{}", standard_error(output));
}

/// Copies of the tagged fixtures in every format.
fn tagged_copies(folder: &TemporaryFolder) -> Vec<PathBuf> {
    let source = fixtures_directory().join("../tagged-fixtures");
    ["mp3", "wav", "aiff", "m4a", "flac"]
        .iter()
        .map(|extension| {
            let copy = folder.path().join(format!("tagged.{extension}"));
            std::fs::copy(source.join(format!("tagged.{extension}")), &copy)
                .expect("the fixture is copied");
            copy
        })
        .collect()
}

fn field(path: &Path, field: TagField) -> Option<String> {
    read_editable_tags(path)
        .expect("the tags are read")
        .get(field)
        .map(str::to_string)
}

#[test]
fn set_writes_every_file_and_undo_gives_back_the_original_bytes() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let files = tagged_copies(&folder);
    let originals: Vec<Vec<u8>> = files
        .iter()
        .map(|path| std::fs::read(path).expect("the copy is read"))
        .collect();

    let set = euphoria.run(&[
        "tags",
        "set",
        "--field",
        "genre=Deep House",
        "--field",
        "year=2024",
        "--add-prefix",
        "title=[Edit] ",
        "--yes",
        text(folder.path()),
    ]);
    assert_success(&set);
    let preview = String::from_utf8_lossy(&set.stdout);
    assert!(preview.contains("-> Deep House"), "{preview}");
    for path in &files {
        assert_eq!(field(path, TagField::Genre).as_deref(), Some("Deep House"));
        assert_eq!(field(path, TagField::Year).as_deref(), Some("2024"));
        assert!(
            field(path, TagField::Title).is_some_and(|title| title.starts_with("[Edit] ")),
            "{}",
            path.display()
        );
    }

    assert_success(&euphoria.run(&["tags", "undo"]));
    for (path, original) in files.iter().zip(&originals) {
        assert_eq!(
            &std::fs::read(path).expect("the file is read"),
            original,
            "{} is byte identical after the undo",
            path.display()
        );
    }
}

#[test]
fn a_dry_run_and_an_unconfirmed_run_write_nothing() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let files = tagged_copies(&folder);
    let originals: Vec<Vec<u8>> = files
        .iter()
        .map(|path| std::fs::read(path).expect("the copy is read"))
        .collect();

    let dry_run = euphoria.run(&[
        "tags",
        "set",
        "--clear",
        "album",
        "--dry-run",
        text(folder.path()),
    ]);
    assert_success(&dry_run);
    assert!(standard_error(&dry_run).contains("Nothing was written"));

    let unconfirmed = euphoria.run(&[
        "tags",
        "set",
        "--field",
        "genre=Techno",
        text(folder.path()),
    ]);
    assert!(!unconfirmed.status.success());
    assert!(standard_error(&unconfirmed).contains("--yes"));

    for (path, original) in files.iter().zip(&originals) {
        assert_eq!(&std::fs::read(path).expect("the file is read"), original);
    }
}

#[test]
fn find_and_replace_follows_the_case_option() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let files = tagged_copies(&folder);
    let mp3 = &files[0];
    assert_success(&euphoria.run(&[
        "tags",
        "set",
        "--field",
        "genre=deep house",
        "--yes",
        text(mp3),
    ]));

    let exact = euphoria.run(&[
        "tags",
        "set",
        "--replace",
        "genre",
        "--find",
        "HOUSE",
        "--with",
        "House",
        "--match-case",
        "--yes",
        text(mp3),
    ]);
    assert_success(&exact);
    assert_eq!(field(mp3, TagField::Genre).as_deref(), Some("deep house"));

    let any_case = euphoria.run(&[
        "tags",
        "set",
        "--replace",
        "genre",
        "--find",
        "HOUSE",
        "--with",
        "House",
        "--yes",
        text(mp3),
    ]);
    assert_success(&any_case);
    assert_eq!(field(mp3, TagField::Genre).as_deref(), Some("deep House"));
}

#[test]
fn a_comment_edit_keeps_the_label_that_tags_write_put_there() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let track = folder.path().join("track.wav");
    std::fs::copy(fixtures_directory().join("sine-440hz-mono.wav"), &track)
        .expect("the fixture is copied");
    assert_success(&euphoria.run(&["tags", "write", "--yes", text(&track)]));
    let labelled = euphoria_core::tags::read_tags(&track)
        .expect("the tags are read")
        .comment
        .expect("the label is written");

    assert_success(&euphoria.run(&[
        "tags",
        "set",
        "--field",
        "comment=Peak opener",
        "--yes",
        text(&track),
    ]));
    let comment = euphoria_core::tags::read_tags(&track)
        .expect("the tags are read")
        .comment
        .expect("there is a comment");
    assert_eq!(comment, format!("{labelled} - Peak opener"));

    let again = euphoria.run(&["tags", "write", "--dry-run", text(&track)]);
    assert_success(&again);
    assert!(
        !String::from_utf8_lossy(&again.stdout).contains("->"),
        "the label is still recognised, so the analysis write has nothing to do"
    );
}

#[test]
fn an_edited_file_is_still_known_to_the_cache() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let track = folder.path().join("track.flac");
    std::fs::copy(fixtures_directory().join("sine-440hz-mono.flac"), &track)
        .expect("the fixture is copied");
    assert_success(&euphoria.run(&["analyze", text(&track)]));
    assert_success(&euphoria.run(&[
        "tags",
        "set",
        "--field",
        "genre=House",
        "--yes",
        text(&track),
    ]));
    let again = euphoria.run(&["analyze", text(&track)]);
    assert_success(&again);
    assert!(
        standard_error(&again).contains("1 of 1 track came from the cache"),
        "{}",
        standard_error(&again)
    );
}

#[test]
fn mistakes_in_the_arguments_are_explained() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let files = tagged_copies(&folder);
    let unknown = euphoria.run(&["tags", "set", "--field", "mood=happy", text(&files[0])]);
    assert!(!unknown.status.success());
    assert!(
        standard_error(&unknown).contains("title, artist, album"),
        "{}",
        standard_error(&unknown)
    );
    let shapeless = euphoria.run(&["tags", "set", "--field", "genre", text(&files[0])]);
    assert!(standard_error(&shapeless).contains("FIELD=TEXT"));
    let nothing = euphoria.run(&["tags", "set", text(&files[0])]);
    assert!(standard_error(&nothing).contains("nothing to change"));
    let tempo = euphoria.run(&[
        "tags",
        "set",
        "--field",
        "bpm=fast",
        "--yes",
        text(&files[0]),
    ]);
    assert!(!tempo.status.success());
    assert!(
        standard_error(&tempo).contains("not a tempo"),
        "{}",
        standard_error(&tempo)
    );
}
