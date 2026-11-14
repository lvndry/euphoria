// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The key a file's tags carry, read back as a key: Euphoria's own writer
//! in every format, and the fields rekordbox and Mixed In Key write, set
//! here with lofty the way those programs store them.

use std::path::{Path, PathBuf};

use euphoria_core::tags::{FieldEdit, TagField, TextChange, apply_plan, plan_edit};
use euphoria_core::{Key, Mode, TagKey, TagKeyAgreement, read_track_name};
use lofty::TextEncoding;
use lofty::config::WriteOptions;
use lofty::id3::v2::{Frame, FrameId, Id3v2Tag, TextInformationFrame};
use lofty::mp4::{Atom, AtomData, AtomIdent, Ilst};
use lofty::ogg::tag::VorbisComments;
use lofty::tag::TagExt;

const FIXTURES: [&str; 8] = [
    "sine-440hz-mono.mp3",
    "sine-440hz-mono.m4a",
    "sine-440hz-mono-alac.m4a",
    "sine-440hz-mono.mp4",
    "sine-440hz-mono.flac",
    "sine-440hz-mono.ogg",
    "sine-440hz-mono.wav",
    "sine-440hz-mono.aiff",
];

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn scratch_copy(test: &str, name: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("track-name-key-{test}"));
    std::fs::create_dir_all(&directory).expect("the scratch folder is made");
    let path = directory.join(name);
    std::fs::copy(fixture(name), &path).expect("the fixture is copied");
    path
}

fn tag_key_of(path: &Path) -> Option<TagKey> {
    read_track_name(path).expect("the tags are read").key
}

fn write_id3_key(path: &Path, text: &str) {
    let mut tag = Id3v2Tag::new();
    tag.insert(Frame::Text(TextInformationFrame::new(
        FrameId::new("TKEY").expect("TKEY is a frame id"),
        TextEncoding::UTF8,
        text.to_string(),
    )));
    tag.save_to_path(path, WriteOptions::new().use_id3v23(true))
        .expect("the ID3 tag is written");
}

fn write_mp4_freeform(path: &Path, name: &str, text: &str) {
    let mut list = Ilst::new();
    list.insert(Atom::new(
        AtomIdent::Freeform {
            mean: "com.apple.iTunes".into(),
            name: name.to_string().into(),
        },
        AtomData::UTF8(text.to_string()),
    ));
    list.save_to_path(path, WriteOptions::new())
        .expect("the MP4 atom is written");
}

fn write_vorbis(path: &Path, field: &str, text: &str) {
    let mut comments = VorbisComments::new();
    comments.push(field.to_string(), text.to_string());
    comments
        .save_to_path(path, WriteOptions::new())
        .expect("the Vorbis comment is written");
}

#[test]
fn the_key_euphoria_writes_reads_back_in_every_format() {
    let a_minor = Key::new(9, Mode::Minor);
    let c_major = Key::new(0, Mode::Major);
    for name in FIXTURES {
        for (test, text, expected) in [
            ("camelot", "8A", a_minor),
            ("open-key", "1m", a_minor),
            ("musical", "Am", a_minor),
            ("other", "8B", c_major),
        ] {
            let path = scratch_copy(test, name);
            let edit = FieldEdit {
                field: TagField::Key,
                change: TextChange::Set(text.to_string()),
            };
            let plan = plan_edit(&path, &[edit]).expect("the edit is planned");
            apply_plan(&plan).expect("the key is written");

            let tag_key = tag_key_of(&path).unwrap_or_else(|| panic!("{name} {text}: no key"));
            assert_eq!(tag_key.text, text, "{name}");
            assert_eq!(tag_key.key, Some(expected), "{name} {text}");
        }
    }
}

#[test]
fn a_file_with_no_key_reports_no_key() {
    for name in FIXTURES {
        assert_eq!(tag_key_of(&fixture(name)), None, "{name}");
    }
}

#[test]
fn a_rekordbox_style_id3_key_is_read() {
    let path = scratch_copy("rekordbox-id3", "sine-440hz-mono.mp3");
    write_id3_key(&path, "Abm");
    assert_eq!(
        tag_key_of(&path).and_then(|tag_key| tag_key.key),
        Some(Key::new(8, Mode::Minor))
    );
}

#[test]
fn a_mixed_in_key_style_mp4_key_atom_is_read() {
    let path = scratch_copy("mixed-in-key-mp4", "sine-440hz-mono.m4a");
    write_mp4_freeform(&path, "KEY", "08A");
    assert_eq!(
        tag_key_of(&path).and_then(|tag_key| tag_key.key),
        Some(Key::new(9, Mode::Minor))
    );
}

#[test]
fn a_vorbis_key_field_is_read_when_initialkey_is_missing() {
    for name in ["sine-440hz-mono.flac", "sine-440hz-mono.ogg"] {
        let path = scratch_copy("vorbis-key", name);
        write_vorbis(&path, "KEY", "F#m");
        assert_eq!(
            tag_key_of(&path).and_then(|tag_key| tag_key.key),
            Some(Key::new(6, Mode::Minor)),
            "{name}"
        );
    }
}

#[test]
fn a_garbage_key_field_is_kept_as_text_and_compared_as_unreadable() {
    let path = scratch_copy("garbage", "sine-440hz-mono.mp3");
    write_id3_key(&path, "banger");
    let tag_key = tag_key_of(&path).expect("the field is kept");
    assert_eq!(tag_key.text, "banger");
    assert_eq!(tag_key.key, None);
    assert_eq!(
        tag_key.compare(Key::new(9, Mode::Minor)),
        TagKeyAgreement::Unreadable
    );
}

#[test]
fn an_empty_key_field_is_no_key() {
    let path = scratch_copy("empty", "sine-440hz-mono.flac");
    write_vorbis(&path, "INITIALKEY", "   ");
    assert_eq!(tag_key_of(&path), None);
}
