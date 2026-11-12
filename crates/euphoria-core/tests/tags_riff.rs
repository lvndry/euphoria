// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The label in the comment of a WAV file's RIFF INFO list, where rekordbox
//! reads a WAV file's comment, next to the one in its ID3 chunk. Every check
//! walks the RIFF chunks itself, sharing nothing with the writer.

use std::path::{Path, PathBuf};

use euphoria_core::tags::{
    DjSoftware, LabelPlacement, TagValues, TagWriteOptions, apply_plan, plan_write, read_tags, undo,
};
use euphoria_core::{EuphoriaError, Key, Mode, decode_mono, read_track_name};

const WAV: &str = "sine-440hz-mono.wav";
const WORKS: &str = "the scratch file can be read and written";
const RIFF_COMMENT: &str = "INFO ICMT";

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    std::fs::read(path).expect("the fixture can be read")
}

fn scratch_file(name: &str, bytes: &[u8]) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("riff-{name}"));
    std::fs::write(&path, bytes).expect(WORKS);
    path
}

fn values(energy: u8) -> TagValues {
    TagValues {
        key: Some(Key::new(9, Mode::Minor)),
        bpm: Some(126.0),
        energy: Some(energy),
    }
}

fn little_endian(bytes: &[u8], offset: usize) -> usize {
    let word: [u8; 4] = bytes[offset..offset + 4].try_into().expect(WORKS);
    u32::from_le_bytes(word) as usize
}

/// The chunks of a RIFF file: name, start and end, padding byte included.
fn chunks(bytes: &[u8]) -> Vec<([u8; 4], usize, usize)> {
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(little_endian(bytes, 4) + 8, bytes.len(), "the RIFF size");
    let mut found = Vec::new();
    let mut offset = 12;
    while offset < bytes.len() {
        let size = little_endian(bytes, offset + 4);
        let end = (offset + 8 + size + size % 2).min(bytes.len());
        let name: [u8; 4] = bytes[offset..offset + 4].try_into().expect(WORKS);
        found.push((name, offset, end));
        offset = end;
    }
    assert_eq!(offset, bytes.len(), "the chunks fill the file");
    found
}

/// The bytes of the first chunk named `name`.
fn chunk<'a>(bytes: &'a [u8], name: &[u8; 4]) -> Option<&'a [u8]> {
    chunks(bytes)
        .into_iter()
        .find(|(found, start, _)| {
            found == name && (name != b"LIST" || &bytes[start + 8..start + 12] == b"INFO")
        })
        .map(|(_, start, end)| &bytes[start..end])
}

/// The values of the INFO list, each whole, in order.
fn info_values(bytes: &[u8]) -> Vec<Vec<u8>> {
    let Some(list) = chunk(bytes, b"LIST") else {
        return Vec::new();
    };
    let body_end = 8 + little_endian(list, 4);
    let mut values = Vec::new();
    let mut offset = 12;
    while offset < body_end {
        let size = little_endian(list, offset + 4);
        let end = (offset + 8 + size + size % 2).min(body_end);
        values.push(list[offset..end].to_vec());
        offset = end;
    }
    values
}

fn info_value(id: &[u8; 4], text: &[u8]) -> Vec<u8> {
    let mut value = id.to_vec();
    value.extend((text.len() as u32).to_le_bytes());
    value.extend(text);
    if !value.len().is_multiple_of(2) {
        value.push(0);
    }
    value
}

/// The text of the INFO comment, up to its first zero byte.
fn riff_comment(bytes: &[u8]) -> Option<Vec<u8>> {
    let value = info_values(bytes)
        .into_iter()
        .find(|value| value.starts_with(b"ICMT"))?;
    let text = &value[8..8 + little_endian(&value, 4)];
    let end = text
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(text.len());
    Some(text[..end].to_vec())
}

/// The WAV fixture with its INFO list replaced by one holding `values`, in
/// the same place, or with no INFO list when `values` is empty.
fn wav_with_info(values: &[Vec<u8>]) -> Vec<u8> {
    let fixture = fixture_bytes(WAV);
    let mut bytes = fixture[..12].to_vec();
    for (name, start, end) in chunks(&fixture) {
        if &name == b"LIST" {
            if !values.is_empty() {
                let body = values.concat();
                bytes.extend(b"LIST");
                bytes.extend(((4 + body.len()) as u32).to_le_bytes());
                bytes.extend(b"INFO");
                bytes.extend(body);
            }
        } else {
            bytes.extend(&fixture[start..end]);
        }
    }
    let size = (bytes.len() - 8) as u32;
    bytes[4..8].copy_from_slice(&size.to_le_bytes());
    bytes
}

/// Applies a plan for `options`, checks the audio, and returns the bytes
/// after the write and the backup.
fn write(
    path: &Path,
    energy: u8,
    options: &TagWriteOptions,
) -> (Vec<u8>, euphoria_core::tags::TagBackup) {
    let before = std::fs::read(path).expect(WORKS);
    let samples = decode_mono(path).expect(WORKS).samples().to_vec();
    let plan = plan_write(path, values(energy), options).expect(WORKS);
    assert!(
        std::fs::read(path).expect(WORKS) == before,
        "planning wrote"
    );
    let backup = apply_plan(&plan).expect(WORKS);
    let written = std::fs::read(path).expect(WORKS);
    assert!(
        chunk(&written, b"data") == chunk(&before, b"data"),
        "the audio data changed"
    );
    assert!(decode_mono(path).expect(WORKS).samples() == samples);
    read_track_name(path).expect("the track name is still read");
    (written, backup)
}

#[test]
fn label_goes_into_the_riff_comment_next_to_the_users_own_text() {
    let original = wav_with_info(&[
        info_value(b"INAM", b"Track title\0"),
        info_value(b"ICMT", b"https://example.com/track/42\0"),
        info_value(b"ISFT", b"Lavf63.1.102\0"),
    ]);
    let path = scratch_file("users-comment.wav", &original);
    let options = TagWriteOptions::default();

    let plan = plan_write(&path, values(7), &options).unwrap();
    let riff_change = plan
        .changes()
        .iter()
        .find(|change| change.field == RIFF_COMMENT)
        .expect("the preview has a line for the RIFF comment");
    assert_eq!(
        riff_change.current.as_deref(),
        Some("https://example.com/track/42")
    );
    assert_eq!(
        riff_change.new.as_deref(),
        Some("8A - Energy 7 - https://example.com/track/42")
    );

    let (written, backup) = write(&path, 7, &options);
    let after = info_values(&written);
    let before = info_values(&original);
    assert_eq!(after.len(), 3);
    assert_eq!(after[0], before[0], "the title keeps its bytes");
    assert_eq!(after[2], before[2], "the software name keeps its bytes");
    assert_eq!(
        riff_comment(&written).unwrap(),
        b"8A - Energy 7 - https://example.com/track/42"
    );
    // The ID3 chunk gets the label too, on its own comment.
    assert_eq!(
        read_tags(&path).unwrap().comment.as_deref(),
        Some("8A - Energy 7")
    );
    assert_eq!(
        read_track_name(&path).unwrap().title.as_deref(),
        Some("Track title")
    );

    undo(&backup).unwrap();
    assert!(std::fs::read(&path).unwrap() == original);
}

#[test]
fn wav_without_an_info_list_gets_one_in_front_of_the_audio_and_loses_it_on_undo() {
    let original = wav_with_info(&[]);
    assert!(chunk(&original, b"LIST").is_none());
    let path = scratch_file("no-info.wav", &original);

    let (written, backup) = write(&path, 7, &TagWriteOptions::default());
    let names: Vec<[u8; 4]> = chunks(&written).iter().map(|(name, _, _)| *name).collect();
    assert_eq!(names, [*b"fmt ", *b"LIST", *b"data", *b"ID3 "]);
    assert_eq!(
        info_values(&written),
        [info_value(b"ICMT", b"8A - Energy 7\0")]
    );
    assert_eq!(read_track_name(&path).unwrap().title, None);

    undo(&backup).unwrap();
    assert!(std::fs::read(&path).unwrap() == original);
}

#[test]
fn writing_again_changes_nothing_and_a_new_label_replaces_the_old_one() {
    let original = wav_with_info(&[info_value(b"ICMT", b"warm up\0")]);
    let path = scratch_file("again.wav", &original);
    let options = TagWriteOptions::default();
    let (first, _) = write(&path, 7, &options);
    assert!(plan_write(&path, values(7), &options).unwrap().is_empty());
    assert!(std::fs::read(&path).unwrap() == first);

    let (_, backup) = write(&path, 9, &options);
    assert_eq!(
        riff_comment(&std::fs::read(&path).unwrap()).unwrap(),
        b"8A - Energy 9 - warm up"
    );
    undo(&backup).unwrap();
    assert!(std::fs::read(&path).unwrap() == first);
}

#[test]
fn label_at_the_end_of_the_comment_and_no_riff_change_for_grouping_or_none() {
    let original = wav_with_info(&[info_value(b"ICMT", b"warm up\0")]);
    let path = scratch_file("placements.wav", &original);
    let at_end = TagWriteOptions {
        label: LabelPlacement::CommentEnd,
        ..TagWriteOptions::default()
    };
    let (written, backup) = write(&path, 7, &at_end);
    assert_eq!(riff_comment(&written).unwrap(), b"warm up - 8A - Energy 7");
    undo(&backup).unwrap();

    for placement in [LabelPlacement::Grouping, LabelPlacement::None] {
        let options = TagWriteOptions {
            label: placement,
            ..TagWriteOptions::default()
        };
        let plan = plan_write(&path, values(7), &options).unwrap();
        assert!(
            plan.changes()
                .iter()
                .all(|change| change.field != RIFF_COMMENT),
            "{placement:?}"
        );
        let (written, backup) = write(&path, 7, &options);
        assert_eq!(chunk(&written, b"LIST"), chunk(&original, b"LIST"));
        undo(&backup).unwrap();
    }
    assert!(std::fs::read(&path).unwrap() == original);
}

#[test]
fn every_preset_writes_the_riff_comment() {
    let presets = [
        DjSoftware::Rekordbox,
        DjSoftware::Serato,
        DjSoftware::Traktor,
        DjSoftware::EngineDj,
        DjSoftware::VirtualDj,
        DjSoftware::Mixxx,
        DjSoftware::Generic,
    ];
    let path = scratch_file("presets.wav", &fixture_bytes(WAV));
    for preset in presets {
        let options = TagWriteOptions {
            preset,
            ..TagWriteOptions::default()
        };
        let plan = plan_write(&path, values(7), &options).unwrap();
        let fields: Vec<&str> = plan
            .changes()
            .iter()
            .map(|change| change.field.as_str())
            .collect();
        assert!(fields.contains(&RIFF_COMMENT), "{preset:?}: {fields:?}");
    }
}

#[test]
fn only_wav_files_get_a_riff_comment() {
    for fixture in [
        "sine-440hz-mono.mp3",
        "sine-440hz-mono.aiff",
        "sine-440hz-mono.m4a",
        "sine-440hz-mono.flac",
    ] {
        let path = scratch_file(&format!("not-wav-{fixture}"), &fixture_bytes(fixture));
        let plan = plan_write(&path, values(7), &TagWriteOptions::default()).unwrap();
        assert!(
            plan.changes()
                .iter()
                .all(|change| change.field != RIFF_COMMENT),
            "{fixture}"
        );
    }
}

#[test]
fn latin1_riff_comment_keeps_its_bytes() {
    let original = wav_with_info(&[info_value(b"ICMT", b"caf\xE9 \xAB live \xBB\0")]);
    let path = scratch_file("latin1.wav", &original);
    let plan = plan_write(&path, values(7), &TagWriteOptions::default()).unwrap();
    let riff_change = plan
        .changes()
        .iter()
        .find(|change| change.field == RIFF_COMMENT)
        .unwrap();
    assert_eq!(riff_change.current.as_deref(), Some("café « live »"));
    let (written, backup) = write(&path, 7, &TagWriteOptions::default());
    assert_eq!(
        riff_comment(&written).unwrap(),
        b"8A - Energy 7 - caf\xE9 \xAB live \xBB"
    );
    undo(&backup).unwrap();
    assert!(std::fs::read(&path).unwrap() == original);
}

#[test]
fn windows_1252_comment_keeps_its_bytes_through_write_and_undo() {
    // The euro sign and curly quotes as Windows programs write them.
    let original = wav_with_info(&[info_value(b"ICMT", b"10 \x80 \x93live\x94\0")]);
    let path = scratch_file("windows-1252.wav", &original);
    let (written, backup) = write(&path, 7, &TagWriteOptions::default());
    assert_eq!(
        riff_comment(&written).unwrap(),
        b"8A - Energy 7 - 10 \x80 \x93live\x94"
    );
    undo(&backup).unwrap();
    assert!(std::fs::read(&path).unwrap() == original);
}

#[test]
fn riff_comment_is_read_when_the_id3_chunk_has_none() {
    let original = wav_with_info(&[info_value(b"ICMT", b"only in RIFF\0")]);
    let path = scratch_file("read.wav", &original);
    assert_eq!(
        read_tags(&path).unwrap().comment.as_deref(),
        Some("only in RIFF")
    );
}

#[test]
fn riff_comment_changed_after_planning_is_not_overwritten() {
    let original = wav_with_info(&[info_value(b"ICMT", b"warm up\0")]);
    let path = scratch_file("stale.wav", &original);
    let plan = plan_write(&path, values(7), &TagWriteOptions::default()).unwrap();
    // Another program edits the RIFF comment.
    let edited = wav_with_info(&[info_value(b"ICMT", b"edited elsewhere\0")]);
    std::fs::write(&path, &edited).unwrap();

    let error = apply_plan(&plan).unwrap_err();
    assert!(
        matches!(&error, EuphoriaError::TagConflict { field, .. } if field == RIFF_COMMENT),
        "{error:?}"
    );
    assert!(std::fs::read(&path).unwrap() == edited);
}

#[test]
fn damaged_info_list_is_refused_and_the_file_is_left_alone() {
    let damaged = wav_with_info(&[b"ICMT\xFF\x00\x00\x00short!".to_vec()]);
    let path = scratch_file("damaged.wav", &damaged);
    let error = plan_write(&path, values(7), &TagWriteOptions::default()).unwrap_err();
    assert!(error.chain().contains("RIFF INFO"), "{}", error.chain());
    assert!(std::fs::read(&path).unwrap() == damaged);
    read_tags(&path).expect("the tags are still read");
}

#[test]
fn duplicate_riff_comments_are_refused_without_changing_either_value() {
    let original = wav_with_info(&[
        info_value(b"ICMT", b"first comment\0"),
        info_value(b"INAM", b"Title\0"),
        info_value(b"ICMT", b"conflicting comment\0"),
    ]);
    let path = scratch_file("duplicate-comments.wav", &original);
    let error = plan_write(&path, values(7), &TagWriteOptions::default()).unwrap_err();
    assert!(
        error.chain().contains("more than one RIFF INFO comment"),
        "{}",
        error.chain()
    );
    assert_eq!(
        read_tags(&path).unwrap().comment.as_deref(),
        Some("first comment")
    );
    assert!(std::fs::read(&path).unwrap() == original);
}

#[cfg(unix)]
#[test]
fn wav_with_several_hard_links_is_refused() {
    let path = scratch_file("linked.wav", &fixture_bytes(WAV));
    let other_name = path.with_file_name("riff-linked-other-name.wav");
    let _ = std::fs::remove_file(&other_name);
    std::fs::hard_link(&path, &other_name).unwrap();
    let error = plan_write(&path, values(7), &TagWriteOptions::default()).unwrap_err();
    assert!(error.chain().contains("hard link"), "{}", error.chain());
    assert!(std::fs::read(&path).unwrap() == fixture_bytes(WAV));
    std::fs::remove_file(&other_name).unwrap();
}
