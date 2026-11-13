// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The key a file's tags carry, read back as a key: every format the tag
//! writers cover gets its key field written with the usual writer, and the
//! reader returns the same key, whether the field is a bare "8A" or carries
//! a tempo behind it, such as "8A 114".

use std::path::{Path, PathBuf};

use euphoria_core::tags::{FieldEdit, TextChange, apply_plan, plan_edit};
use euphoria_core::{Key, Mode, read_track_name};

const FIXTURES: [&str; 6] = [
    "sine-440hz-mono.mp3",
    "sine-440hz-mono.m4a",
    "sine-440hz-mono.flac",
    "sine-440hz-mono.ogg",
    "sine-440hz-mono.wav",
    "sine-440hz-mono.aiff",
];

fn scratch_copy(test: &str, fixture: &str) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(fixture);
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("track-name-key-{test}"));
    std::fs::create_dir_all(&directory).expect("the scratch folder is made");
    let path = directory.join(fixture);
    std::fs::copy(&source, &path).expect("the fixture is copied");
    path
}

fn key(text: &str) -> FieldEdit {
    FieldEdit {
        field: euphoria_core::tags::TagField::Key,
        change: TextChange::Set(text.to_string()),
    }
}

#[test]
fn the_written_key_reads_back_in_every_format() {
    for fixture in FIXTURES {
        let a_minor = Key::new(9, Mode::Minor);
        let c_major = Key::new(0, Mode::Major);

        for (test, text, expected) in [
            ("bare", "8A", a_minor),
            ("tempo", "8A 114", a_minor),
            ("musical", "A minor", a_minor),
            ("other", "8B", c_major),
        ] {
            let path = scratch_copy(test, fixture);
            let plan = plan_edit(&path, &[key(text)]).expect("the edit is planned");
            apply_plan(&plan).expect("the key is written");

            let name = read_track_name(&path).expect("the name is read");
            assert_eq!(name.key, Some(expected), "{fixture} {text}");
        }
    }
}

#[test]
fn a_file_with_no_key_reports_no_key() {
    for fixture in FIXTURES {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(fixture);
        let name = read_track_name(&source).expect("the name is read");
        assert_eq!(name.key, None, "{fixture}");
    }
}
