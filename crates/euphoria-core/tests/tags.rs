// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Planning, applying and undoing tag writes on copies of the fixtures: the
//! audio and every unrelated tag survive, and undo restores the original.

use std::fs::File;
use std::path::{Path, PathBuf};

use euphoria_core::detect::Analysis;
use euphoria_core::tags::{
    DjSoftware, KeyNotation, Label, LabelTarget, TagFormat, TagWriteOptions, apply_plan,
    plan_write, read_tags, undo,
};
use euphoria_core::{EuphoriaError, Key, Mode, decode_mono};
use lofty::TextEncoding;
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::AudioFile;
use lofty::flac::FlacFile;
use lofty::id3::v2::{
    CommentFrame, Frame, FrameId, Id3v2Tag, Id3v2Version, PrivateFrame, TextInformationFrame,
};
use lofty::iff::aiff::AiffFile;
use lofty::iff::wav::WavFile;
use lofty::mp4::{Atom, AtomData, AtomIdent, Ilst, Mp4File};
use lofty::mpeg::MpegFile;
use lofty::ogg::tag::VorbisComments;
use lofty::tag::TagExt;

const MP3: &str = "sine-440hz-mono.mp3";
const WAV: &str = "sine-440hz-mono.wav";
const AIFF: &str = "sine-440hz-mono.aiff";
const M4A: &str = "sine-440hz-mono.m4a";
const FLAC: &str = "sine-440hz-mono.flac";
const ALL_FIXTURES: [&str; 5] = [MP3, WAV, AIFF, M4A, FLAC];

const TITLE: &str = "Test Tone";
const ARTIST: &str = "Sine Wave";
const ALBUM: &str = "Fixtures";
const COMMENT: &str = "warm up";
const CUE_DATA: &str = "cue points of another program";

fn scratch_copy(fixture: &str, test: &str) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(fixture);
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("tags-{test}-{fixture}"));
    std::fs::copy(source, &path).expect("cannot copy the fixture");
    path
}

fn extension(path: &Path) -> &str {
    path.extension()
        .expect("the scratch copy can be read and written")
        .to_str()
        .expect("the scratch copy can be read and written")
}

fn text_frame(id: &'static str, value: &'static str) -> Frame<'static> {
    Frame::Text(TextInformationFrame::new(
        FrameId::new(id).expect("the scratch copy can be read and written"),
        TextEncoding::UTF8,
        value,
    ))
}

/// Gives the file a title, artist, album and comment, plus data of the kind
/// DJ programs keep for themselves. The MP3 gets an ID3v2.3 tag, the other
/// ID3 formats ID3v2.4.
fn seed(path: &Path) {
    match extension(path) {
        "mp3" | "wav" | "aiff" => {
            let version_3 = extension(path) == "mp3";
            let mut tag = Id3v2Tag::new();
            tag.insert(text_frame("TIT2", TITLE));
            tag.insert(text_frame("TPE1", ARTIST));
            tag.insert(text_frame("TALB", ALBUM));
            if version_3 {
                tag.insert(text_frame("TYER", "1999"));
            }
            tag.insert(Frame::Comment(CommentFrame::new(
                TextEncoding::Latin1,
                *b"fra",
                "",
                COMMENT,
            )));
            tag.insert(Frame::Comment(CommentFrame::new(
                TextEncoding::UTF8,
                *b"eng",
                "iTunNORM",
                "0000 0000",
            )));
            tag.insert(Frame::Private(PrivateFrame::new(
                "TRAKTOR4",
                CUE_DATA.as_bytes().to_vec(),
            )));
            tag.save_to_path(path, WriteOptions::new().use_id3v23(version_3))
                .expect("the scratch copy can be read and written");
        }
        "m4a" => {
            let mut ilst = Ilst::new();
            for (fourcc, value) in [
                (*b"\xa9nam", TITLE),
                (*b"\xa9ART", ARTIST),
                (*b"\xa9alb", ALBUM),
                (*b"\xa9cmt", COMMENT),
            ] {
                ilst.insert(Atom::new(
                    AtomIdent::Fourcc(fourcc),
                    AtomData::UTF8(value.to_string()),
                ));
            }
            ilst.insert(Atom::new(
                AtomIdent::Freeform {
                    mean: "com.serato.dj".into(),
                    name: "markersv2".into(),
                },
                AtomData::UTF8(CUE_DATA.to_string()),
            ));
            ilst.save_to_path(path, WriteOptions::new())
                .expect("the scratch copy can be read and written");
        }
        "flac" => {
            let mut comments = VorbisComments::new();
            for (key, value) in [
                ("TITLE", TITLE),
                ("ARTIST", ARTIST),
                ("ALBUM", ALBUM),
                ("COMMENT", COMMENT),
                ("SERATO_MARKERS_V2", CUE_DATA),
            ] {
                comments.push(key.to_string(), value.to_string());
            }
            comments
                .save_to_path(path, WriteOptions::new())
                .expect("the scratch copy can be read and written");
        }
        other => panic!("no seed for {other}"),
    }
}

fn id3v2(path: &Path) -> Option<Id3v2Tag> {
    let options = ParseOptions::new().implicit_conversions(false);
    let mut file = File::open(path).expect("the scratch copy can be read and written");
    match extension(path) {
        "mp3" => MpegFile::read_from(&mut file, options)
            .expect("the scratch copy can be read and written")
            .remove_id3v2(),
        "wav" => WavFile::read_from(&mut file, options)
            .expect("the scratch copy can be read and written")
            .remove_id3v2(),
        "aiff" => AiffFile::read_from(&mut file, options)
            .expect("the scratch copy can be read and written")
            .remove_id3v2(),
        other => panic!("{other} has no ID3v2 tag"),
    }
}

/// Every tag of the file in the form lofty reads it, as text that compares
/// equal only when all of it is equal, order included.
fn all_tags(path: &Path) -> String {
    let options = ParseOptions::new().implicit_conversions(false);
    let mut file = File::open(path).expect("the scratch copy can be read and written");
    match extension(path) {
        "mp3" => {
            let file = MpegFile::read_from(&mut file, options)
                .expect("the scratch copy can be read and written");
            format!("{:?}", (file.id3v2(), file.id3v1(), file.ape()))
        }
        "wav" => {
            let file = WavFile::read_from(&mut file, options)
                .expect("the scratch copy can be read and written");
            format!("{:?}", (file.id3v2(), file.riff_info()))
        }
        "aiff" => {
            let file = AiffFile::read_from(&mut file, options)
                .expect("the scratch copy can be read and written");
            format!("{:?}", (file.id3v2(), file.text_chunks()))
        }
        "m4a" => format!(
            "{:?}",
            Mp4File::read_from(&mut file, options)
                .expect("the scratch copy can be read and written")
                .ilst()
        ),
        "flac" => format!(
            "{:?}",
            FlacFile::read_from(&mut file, options)
                .expect("the scratch copy can be read and written")
                .vorbis_comments()
        ),
        other => panic!("no tag reader for {other}"),
    }
}

fn a_minor_at(tempo: f32) -> Analysis {
    Analysis {
        key: Some(Key::new(9, Mode::Minor)),
        tempo: Some(tempo),
    }
}

fn full_options() -> TagWriteOptions {
    TagWriteOptions {
        key_notation: KeyNotation::Camelot,
        write_bpm: true,
        label: Some(Label {
            text: "8A".to_string(),
            target: LabelTarget::Comment,
        }),
        preset: DjSoftware::Generic,
    }
}

fn changed_fields(path: &Path, analysis: &Analysis, options: &TagWriteOptions) -> Vec<String> {
    plan_write(path, analysis, options)
        .expect("the scratch copy can be read and written")
        .changes()
        .iter()
        .map(|change| change.field.clone())
        .collect()
}

/// Plans, applies and undoes a full write, checking the file at each step.
fn assert_round_trip(fixture: &str, seeded: bool) {
    let test = if seeded { "seeded" } else { "bare" };
    let path = scratch_copy(fixture, &format!("round-trip-{test}"));
    if seeded {
        seed(&path);
    }
    let original_bytes = std::fs::read(&path).expect("the scratch copy can be read and written");
    let original_tags = all_tags(&path);
    let original_read = read_tags(&path).expect("the scratch copy can be read and written");
    let original_sample_count = decode_mono(&path)
        .expect("the scratch copy can be read and written")
        .samples()
        .len();
    let analysis = a_minor_at(127.5);

    let plan = plan_write(&path, &analysis, &full_options())
        .expect("the scratch copy can be read and written");
    assert!(!plan.is_empty(), "{fixture}");
    assert_eq!(
        std::fs::read(&path).expect("the scratch copy can be read and written"),
        original_bytes,
        "planning changed {fixture}"
    );
    for change in plan.changes() {
        assert_ne!(change.current, change.new, "{fixture} {}", change.field);
    }

    let backup = apply_plan(&plan).expect("the scratch copy can be read and written");
    assert_eq!(backup, plan.backup());

    let written = read_tags(&path).expect("the scratch copy can be read and written");
    assert_eq!(written.key.as_deref(), Some("8A"), "{fixture}");
    assert_eq!(written.bpm.as_deref(), Some("127.5"), "{fixture}");
    assert_eq!(written.title, original_read.title, "{fixture}");
    assert_eq!(written.artist, original_read.artist, "{fixture}");
    assert_eq!(written.grouping, original_read.grouping, "{fixture}");
    let expected_comment = if seeded { "8A - warm up" } else { "8A" };
    assert_eq!(
        written.comment.as_deref(),
        Some(expected_comment),
        "{fixture}"
    );
    assert_eq!(
        decode_mono(&path)
            .expect("the scratch copy can be read and written")
            .samples()
            .len(),
        original_sample_count,
        "{fixture}"
    );
    if seeded {
        let tags = all_tags(&path);
        for kept in [TITLE, ARTIST, ALBUM] {
            assert!(tags.contains(kept), "{fixture} lost {kept}");
        }
        let cue_data_kept =
            tags.contains(CUE_DATA) || tags.contains(&format!("{:?}", CUE_DATA.as_bytes()));
        assert!(cue_data_kept, "{fixture} lost the cue data: {tags}");
    }

    assert!(
        plan_write(&path, &analysis, &full_options())
            .expect("the scratch copy can be read and written")
            .is_empty(),
        "{fixture} needs a second write"
    );

    undo(&backup).expect("the scratch copy can be read and written");
    assert_eq!(all_tags(&path), original_tags, "{fixture}");
    assert_eq!(
        read_tags(&path).expect("the scratch copy can be read and written"),
        original_read,
        "{fixture}"
    );
    assert_eq!(
        decode_mono(&path)
            .expect("the scratch copy can be read and written")
            .samples()
            .len(),
        original_sample_count,
        "{fixture}"
    );
}

#[test]
fn mp3_round_trip() {
    assert_round_trip(MP3, true);
    assert_round_trip(MP3, false);
}

#[test]
fn wav_round_trip() {
    assert_round_trip(WAV, true);
    assert_round_trip(WAV, false);
}

#[test]
fn aiff_round_trip() {
    assert_round_trip(AIFF, true);
    assert_round_trip(AIFF, false);
}

#[test]
fn m4a_round_trip() {
    assert_round_trip(M4A, true);
    assert_round_trip(M4A, false);
}

#[test]
fn flac_round_trip() {
    assert_round_trip(FLAC, true);
    assert_round_trip(FLAC, false);
}

#[test]
fn reads_seeded_tags_in_every_format() {
    for fixture in ALL_FIXTURES {
        let path = scratch_copy(fixture, "read");
        seed(&path);
        let tags = read_tags(&path).unwrap();
        assert_eq!(tags.title.as_deref(), Some(TITLE), "{fixture}");
        assert_eq!(tags.artist.as_deref(), Some(ARTIST), "{fixture}");
        assert_eq!(tags.comment.as_deref(), Some(COMMENT), "{fixture}");
        assert_eq!(tags.key, None, "{fixture}");
        assert_eq!(tags.bpm, None, "{fixture}");
        assert_eq!(tags.grouping, None, "{fixture}");
    }
}

#[test]
fn undo_removes_fields_that_did_not_exist() {
    for fixture in ALL_FIXTURES {
        let path = scratch_copy(fixture, "undo-removes");
        let plan = plan_write(&path, &a_minor_at(128.0), &full_options()).unwrap();
        assert!(
            plan.changes().iter().all(|change| change.current.is_none()),
            "{fixture}"
        );
        let backup = apply_plan(&plan).unwrap();
        assert_eq!(read_tags(&path).unwrap().key.as_deref(), Some("8A"));

        undo(&backup).unwrap();
        let restored = read_tags(&path).unwrap();
        assert_eq!(restored.key, None, "{fixture}");
        assert_eq!(restored.bpm, None, "{fixture}");
        assert_eq!(restored.comment, None, "{fixture}");
    }
}

#[test]
fn label_keeps_the_existing_comment_and_grouping() {
    for fixture in ALL_FIXTURES {
        let path = scratch_copy(fixture, "label");
        seed(&path);
        for (target, text) in [
            (LabelTarget::Comment, "8A"),
            (LabelTarget::Grouping, "Energy 7"),
        ] {
            let options = TagWriteOptions {
                label: Some(Label {
                    text: text.to_string(),
                    target,
                }),
                ..TagWriteOptions::default()
            };
            let plan = plan_write(&path, &Analysis::default(), &options).unwrap();
            assert_eq!(plan.changes().len(), 1, "{fixture}");
            apply_plan(&plan).unwrap();
            assert!(
                plan_write(&path, &Analysis::default(), &options)
                    .unwrap()
                    .is_empty(),
                "{fixture} would get the label twice"
            );
        }
        let tags = read_tags(&path).unwrap();
        assert_eq!(tags.comment.as_deref(), Some("8A - warm up"), "{fixture}");
        assert_eq!(tags.grouping.as_deref(), Some("Energy 7"), "{fixture}");
    }
}

#[test]
fn id3_comment_keeps_its_language_and_described_comments() {
    let path = scratch_copy(MP3, "comment-language");
    seed(&path);
    apply_plan(&plan_write(&path, &a_minor_at(128.0), &full_options()).unwrap()).unwrap();

    let tag = id3v2(&path).unwrap();
    let comments: Vec<_> = tag
        .into_iter()
        .filter_map(|frame| match frame {
            Frame::Comment(comment) => Some((
                comment.language,
                comment.description.to_string(),
                comment.content.to_string(),
            )),
            _ => None,
        })
        .collect();
    assert_eq!(
        comments,
        [
            (*b"fra", String::new(), "8A - warm up".to_string()),
            (*b"eng", "iTunNORM".to_string(), "0000 0000".to_string()),
        ]
    );
}

#[test]
fn id3_version_is_kept_and_new_tags_are_version_3() {
    let analysis = a_minor_at(128.0);
    let expectations = [
        (MP3, true, Id3v2Version::V3),
        (WAV, true, Id3v2Version::V4),
        (AIFF, false, Id3v2Version::V3),
    ];
    for (fixture, seeded, version) in expectations {
        let path = scratch_copy(fixture, "id3-version");
        if seeded {
            seed(&path);
        }
        apply_plan(&plan_write(&path, &analysis, &full_options()).unwrap()).unwrap();
        let tag = id3v2(&path).unwrap();
        assert_eq!(tag.original_version(), version, "{fixture}");
        if fixture == MP3 {
            let year = FrameId::new("TYER").unwrap();
            assert_eq!(tag.get_text(&year), Some("1999"));
        }
    }
}

#[test]
fn matching_values_produce_an_empty_plan_that_leaves_the_file_alone() {
    for fixture in ALL_FIXTURES {
        let path = scratch_copy(fixture, "identical");
        let options = TagWriteOptions {
            write_bpm: true,
            preset: DjSoftware::Rekordbox,
            ..TagWriteOptions::default()
        };
        apply_plan(&plan_write(&path, &a_minor_at(128.0), &options).unwrap()).unwrap();
        let bytes = std::fs::read(&path).unwrap();

        let plan = plan_write(&path, &a_minor_at(128.2), &options).unwrap();
        assert!(plan.is_empty(), "{fixture}: {:?}", plan.changes());
        let backup = apply_plan(&plan).unwrap();
        assert!(backup.fields.is_empty());
        undo(&backup).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "{fixture}");
    }
}

#[test]
fn tempo_written_with_decimals_elsewhere_counts_as_matching() {
    let path = scratch_copy(FLAC, "decimal-tempo");
    let mut comments = VorbisComments::new();
    comments.push("BPM".to_string(), "128.00".to_string());
    comments.save_to_path(&path, WriteOptions::new()).unwrap();

    let options = TagWriteOptions {
        write_bpm: true,
        preset: DjSoftware::Traktor,
        ..TagWriteOptions::default()
    };
    let analysis = Analysis {
        key: None,
        tempo: Some(128.0),
    };
    assert!(plan_write(&path, &analysis, &options).unwrap().is_empty());
}

#[test]
fn presets_write_the_documented_fields() {
    let commercial = [
        DjSoftware::Rekordbox,
        DjSoftware::Serato,
        DjSoftware::Traktor,
        DjSoftware::EngineDj,
    ];
    let expectations: [(&str, &[&str], &[&str]); 5] = [
        (MP3, &["TKEY", "TBPM"], &["TKEY", "TBPM"]),
        (WAV, &["TKEY", "TBPM"], &["TKEY", "TBPM"]),
        (AIFF, &["TKEY", "TBPM"], &["TKEY", "TBPM"]),
        (
            M4A,
            &["----:com.apple.iTunes:initialkey", "tmpo"],
            &[
                "----:com.apple.iTunes:initialkey",
                "tmpo",
                "----:com.apple.iTunes:BPM",
            ],
        ),
        (FLAC, &["INITIALKEY", "BPM"], &["INITIALKEY", "KEY", "BPM"]),
    ];
    for (fixture, commercial_fields, generic_fields) in expectations {
        let path = scratch_copy(fixture, "presets");
        let mut options = TagWriteOptions {
            write_bpm: true,
            ..TagWriteOptions::default()
        };
        for preset in commercial {
            options.preset = preset;
            assert_eq!(
                changed_fields(&path, &a_minor_at(127.5), &options),
                commercial_fields,
                "{fixture} {preset:?}"
            );
        }
        options.preset = DjSoftware::Generic;
        assert_eq!(
            changed_fields(&path, &a_minor_at(127.5), &options),
            generic_fields,
            "{fixture} generic"
        );
    }
}

#[test]
fn commercial_presets_write_a_whole_tempo() {
    let path = scratch_copy(MP3, "whole-tempo");
    let options = TagWriteOptions {
        write_bpm: true,
        preset: DjSoftware::Serato,
        ..TagWriteOptions::default()
    };
    apply_plan(&plan_write(&path, &a_minor_at(127.5), &options).unwrap()).unwrap();
    assert_eq!(read_tags(&path).unwrap().bpm.as_deref(), Some("128"));
}

#[test]
fn m4a_tempo_atom_holds_a_two_byte_whole_number() {
    let path = scratch_copy(M4A, "tempo-atom");
    apply_plan(&plan_write(&path, &a_minor_at(127.5), &full_options()).unwrap()).unwrap();

    let bytes = std::fs::read(&path).unwrap();
    let name_position = bytes
        .windows(4)
        .position(|window| window == b"tmpo")
        .expect("the file has a tmpo atom");
    let data_atom = &bytes[name_position + 4..name_position + 22];
    let header_and_type: [u8; 16] = [0, 0, 0, 18, b'd', b'a', b't', b'a', 0, 0, 0, 21, 0, 0, 0, 0];
    assert_eq!(data_atom[..16], header_and_type);
    assert_eq!(data_atom[16..], 128_u16.to_be_bytes());
}

#[test]
fn musical_notation_and_no_tempo() {
    let path = scratch_copy(FLAC, "musical");
    let options = TagWriteOptions {
        key_notation: KeyNotation::Musical,
        preset: DjSoftware::Traktor,
        ..TagWriteOptions::default()
    };
    apply_plan(&plan_write(&path, &a_minor_at(128.0), &options).unwrap()).unwrap();
    let tags = read_tags(&path).unwrap();
    assert_eq!(tags.format, TagFormat::VorbisComments);
    assert_eq!(tags.key.as_deref(), Some("Am"));
    assert_eq!(tags.bpm, None);
}

#[test]
fn analysis_without_results_plans_nothing() {
    let path = scratch_copy(MP3, "no-results");
    let options = TagWriteOptions {
        write_bpm: true,
        ..TagWriteOptions::default()
    };
    assert!(
        plan_write(&path, &Analysis::default(), &options)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn stale_plan_is_refused_and_writes_nothing() {
    let path = scratch_copy(FLAC, "stale-plan");
    let stale = plan_write(&path, &a_minor_at(128.0), &full_options()).unwrap();
    let other_key = Analysis {
        key: Some(Key::new(0, Mode::Major)),
        tempo: None,
    };
    apply_plan(&plan_write(&path, &other_key, &full_options()).unwrap()).unwrap();
    let bytes = std::fs::read(&path).unwrap();

    let error = apply_plan(&stale).unwrap_err();
    assert!(
        matches!(&error, EuphoriaError::TagConflict { field, .. } if field == "INITIALKEY"),
        "{error:?}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn undo_refuses_to_overwrite_a_later_edit() {
    let path = scratch_copy(MP3, "undo-conflict");
    let backup =
        apply_plan(&plan_write(&path, &a_minor_at(128.0), &full_options()).unwrap()).unwrap();
    let mut tag = id3v2(&path).unwrap();
    tag.insert(text_frame("TKEY", "Cm"));
    tag.save_to_path(&path, WriteOptions::new()).unwrap();
    let bytes = std::fs::read(&path).unwrap();

    let error = undo(&backup).unwrap_err();
    assert!(
        matches!(&error, EuphoriaError::TagConflict { field, .. } if field == "TKEY"),
        "{error:?}"
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn missing_file_is_an_open_error() {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("tags-does-not-exist.mp3");
    assert!(matches!(
        read_tags(&path).unwrap_err(),
        EuphoriaError::Open { .. }
    ));
}

#[test]
fn text_file_is_a_tag_read_error_with_a_cause() {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("tags-not-audio.mp3");
    std::fs::write(&path, "This is a text file, not audio.\n".repeat(64)).unwrap();
    let error = plan_write(&path, &a_minor_at(128.0), &full_options()).unwrap_err();
    assert!(
        matches!(&error, EuphoriaError::TagRead { path: error_path, .. } if *error_path == path),
        "{error:?}"
    );
    assert!(std::error::Error::source(&error).is_some());
}

#[test]
fn invalid_tempo_is_a_tag_write_error() {
    let path = scratch_copy(MP3, "invalid-tempo");
    let error = plan_write(&path, &a_minor_at(f32::NAN), &full_options()).unwrap_err();
    assert!(
        matches!(&error, EuphoriaError::TagWrite { .. }),
        "{error:?}"
    );
}

#[test]
fn no_temporary_file_is_left_behind() {
    let path = scratch_copy(WAV, "temporary");
    let backup =
        apply_plan(&plan_write(&path, &a_minor_at(128.0), &full_options()).unwrap()).unwrap();
    undo(&backup).unwrap();
    let leftovers: Vec<_> = std::fs::read_dir(env!("CARGO_TARGET_TMPDIR"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.contains("tags-temporary") && name.contains("euphoria-tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}
