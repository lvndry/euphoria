// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Planning, applying and undoing tag writes on copies of the fixtures: the
//! audio and every unrelated tag survive, and undo restores the original.

use std::fs::File;
use std::path::{Path, PathBuf};

use euphoria_core::detect::Analysis;
use euphoria_core::tags::{
    DjSoftware, FieldChange, KeyNotation, LabelPlacement, TagFormat, TagValues, TagWriteOptions,
    WritePlan, apply_plan, plan_write, read_tags, undo,
};
use euphoria_core::{EuphoriaError, Key, Mode, decode_mono};
use lofty::TextEncoding;
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::AudioFile;
use lofty::flac::FlacFile;
use lofty::id3::v2::{
    AttachedPictureFrame, CommentFrame, Frame, FrameId, Id3v2Tag, Id3v2Version, PrivateFrame,
    TextInformationFrame,
};
use lofty::iff::aiff::AiffFile;
use lofty::iff::wav::WavFile;
use lofty::mp4::{Atom, AtomData, AtomIdent, Ilst, Mp4File};
use lofty::mpeg::MpegFile;
use lofty::ogg::tag::VorbisComments;
use lofty::ogg::{OggPictureStorage, VorbisFile};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::TagExt;

const MP3: &str = "sine-440hz-mono.mp3";
const WAV: &str = "sine-440hz-mono.wav";
const AIFF: &str = "sine-440hz-mono.aiff";
const M4A: &str = "sine-440hz-mono.m4a";
const FLAC: &str = "sine-440hz-mono.flac";
const OGG: &str = "sine-440hz-mono.ogg";
const ALAC: &str = "sine-440hz-mono-alac.m4a";
const MP4: &str = "sine-440hz-mono.mp4";
const ALL_FIXTURES: [&str; 8] = [MP3, WAV, AIFF, M4A, FLAC, OGG, ALAC, MP4];

const TITLE: &str = "Test Tone";
const ARTIST: &str = "Sine Wave";
const ALBUM: &str = "Fixtures";
const COMMENT: &str = "warm up";
const CUE_DATA: &str = "cue points of another program";
/// A PNG image of one transparent pixel.
const ARTWORK: [u8; 67] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn artwork() -> Picture {
    Picture::unchecked(ARTWORK.to_vec())
        .pic_type(PictureType::CoverFront)
        .mime_type(MimeType::Png)
        .description("cover")
        .build()
}

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

/// Gives the file a title, artist, album, comment and artwork, plus data of
/// the kind DJ programs keep for themselves. The MP3 gets an ID3v2.3 tag, the other
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
            tag.insert(Frame::Picture(AttachedPictureFrame::new(
                TextEncoding::Latin1,
                artwork(),
            )));
            tag.save_to_path(path, WriteOptions::new().use_id3v23(version_3))
                .expect("the scratch copy can be read and written");
        }
        "m4a" | "mp4" => {
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
            ilst.insert_picture(artwork());
            ilst.save_to_path(path, WriteOptions::new())
                .expect("the scratch copy can be read and written");
        }
        "flac" | "ogg" => {
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
                .insert_picture(artwork(), None)
                .expect("the scratch copy can be read and written");
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
        "m4a" | "mp4" => format!(
            "{:?}",
            Mp4File::read_from(&mut file, options)
                .expect("the scratch copy can be read and written")
                .ilst()
        ),
        "flac" => {
            let file = FlacFile::read_from(&mut file, options)
                .expect("the scratch copy can be read and written");
            format!("{:?}", (file.vorbis_comments(), file.pictures()))
        }
        "ogg" => format!(
            "{:?}",
            VorbisFile::read_from(&mut file, options)
                .expect("the scratch copy can be read and written")
                .vorbis_comments()
        ),
        other => panic!("no tag reader for {other}"),
    }
}

/// The bytes of every picture embedded in the file.
fn pictures(path: &Path) -> Vec<Vec<u8>> {
    let options = ParseOptions::new().implicit_conversions(false);
    let mut file = File::open(path).expect("the scratch copy can be read and written");
    match extension(path) {
        "mp3" | "wav" | "aiff" => id3v2(path)
            .into_iter()
            .flatten()
            .filter_map(|frame| match frame {
                Frame::Picture(frame) => Some(frame.picture.data().to_vec()),
                _ => None,
            })
            .collect(),
        "m4a" | "mp4" => Mp4File::read_from(&mut file, options)
            .expect("the scratch copy can be read and written")
            .ilst()
            .and_then(Ilst::pictures)
            .into_iter()
            .flatten()
            .map(|picture| picture.data().to_vec())
            .collect(),
        "flac" => FlacFile::read_from(&mut file, options)
            .expect("the scratch copy can be read and written")
            .pictures()
            .iter()
            .map(|(picture, _)| picture.data().to_vec())
            .collect(),
        "ogg" => VorbisFile::read_from(&mut file, options)
            .expect("the scratch copy can be read and written")
            .vorbis_comments()
            .pictures()
            .iter()
            .map(|(picture, _)| picture.data().to_vec())
            .collect(),
        other => panic!("no picture reader for {other}"),
    }
}

fn a_minor_at(tempo: f32) -> Analysis {
    Analysis {
        key: Some(Key::new(9, Mode::Minor)),
        tempo: Some(tempo),
    }
}

/// Options that write the key field and nothing else.
fn bare_options() -> TagWriteOptions {
    TagWriteOptions {
        write_bpm: false,
        label: LabelPlacement::None,
        ..TagWriteOptions::default()
    }
}

/// Options that write every kind of field, with the preset that keeps
/// decimals in the tempo.
fn full_options() -> TagWriteOptions {
    TagWriteOptions {
        key_notation: KeyNotation::Camelot,
        write_bpm: true,
        label: LabelPlacement::CommentStart,
        preset: DjSoftware::Mixxx,
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
    let original_pictures = pictures(&path);
    let expected_pictures: &[Vec<u8>] = if seeded { &[ARTWORK.to_vec()] } else { &[] };
    assert_eq!(original_pictures, expected_pictures, "{fixture}");
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
    assert_eq!(pictures(&path), original_pictures, "{fixture}");
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
    assert_eq!(pictures(&path), original_pictures, "{fixture}");
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
fn ogg_vorbis_round_trip() {
    assert_round_trip(OGG, true);
    assert_round_trip(OGG, false);
}

#[test]
fn alac_round_trip() {
    assert_round_trip(ALAC, true);
    assert_round_trip(ALAC, false);
}

#[test]
fn mp4_round_trip() {
    assert_round_trip(MP4, true);
    assert_round_trip(MP4, false);
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

/// Options that write the label and nothing else.
fn label_options(placement: LabelPlacement) -> TagWriteOptions {
    TagWriteOptions {
        label: placement,
        ..bare_options()
    }
}

fn a_minor_with_energy(energy: u8) -> TagValues {
    TagValues {
        key: Some(Key::new(9, Mode::Minor)),
        bpm: None,
        energy: Some(energy),
    }
}

/// The field a placement writes to, and what the seeded file's own text in
/// it looks like around a label.
fn labelled(path: &Path, placement: LabelPlacement, label: &str) -> (Option<String>, String) {
    let tags = read_tags(path).expect("the scratch copy can be read and written");
    match placement {
        LabelPlacement::CommentStart => (tags.comment, format!("{label} - {COMMENT}")),
        LabelPlacement::CommentEnd => (tags.comment, format!("{COMMENT} - {label}")),
        LabelPlacement::Grouping => (tags.grouping, label.to_string()),
        LabelPlacement::None => (tags.comment, COMMENT.to_string()),
    }
}

/// The changes of a plan apart from those to the key fields.
/// The changes of a plan apart from those to the key fields and to the
/// comment of a WAV file's RIFF INFO list, which `tags_riff.rs` covers.
fn label_changes(plan: &WritePlan) -> Vec<&FieldChange> {
    plan.changes()
        .iter()
        .filter(|change| !change.field.to_ascii_uppercase().contains("KEY"))
        .filter(|change| change.field != "INFO ICMT")
        .collect()
}

const PLACEMENTS: [LabelPlacement; 3] = [
    LabelPlacement::CommentStart,
    LabelPlacement::CommentEnd,
    LabelPlacement::Grouping,
];

#[test]
fn default_options_put_the_key_and_energy_in_front_of_the_comment() {
    for fixture in ALL_FIXTURES {
        let path = scratch_copy(fixture, "label-default");
        seed(&path);
        let values = TagValues {
            bpm: Some(127.5),
            ..a_minor_with_energy(7)
        };
        apply_plan(&plan_write(&path, values, &TagWriteOptions::default()).unwrap()).unwrap();
        let tags = read_tags(&path).unwrap();
        assert_eq!(
            tags.comment.as_deref(),
            Some("8A - Energy 7 - warm up"),
            "{fixture}"
        );
        assert_eq!(tags.key.as_deref(), Some("8A"), "{fixture}");
        assert_eq!(tags.bpm.as_deref(), Some("128"), "{fixture}");
        assert_eq!(tags.grouping, None, "{fixture}");
    }
}

#[test]
fn label_keeps_the_existing_text_wherever_it_is_placed() {
    for fixture in ALL_FIXTURES {
        for placement in PLACEMENTS {
            let path = scratch_copy(fixture, "label");
            seed(&path);
            let options = label_options(placement);
            let plan = plan_write(&path, a_minor_with_energy(7), &options).unwrap();
            assert_eq!(label_changes(&plan).len(), 1, "{fixture} {placement:?}");
            apply_plan(&plan).unwrap();
            let (found, expected) = labelled(&path, placement, "8A - Energy 7");
            assert_eq!(found, Some(expected), "{fixture} {placement:?}");
        }
    }
}

#[test]
fn label_follows_the_key_notation() {
    let path = scratch_copy(FLAC, "label-musical");
    seed(&path);
    let options = TagWriteOptions {
        key_notation: KeyNotation::Musical,
        ..label_options(LabelPlacement::CommentStart)
    };
    apply_plan(&plan_write(&path, a_minor_with_energy(7), &options).unwrap()).unwrap();
    assert_eq!(
        read_tags(&path).unwrap().comment.as_deref(),
        Some("Am - Energy 7 - warm up")
    );
}

#[test]
fn placement_none_leaves_the_comment_and_grouping_alone() {
    for fixture in ALL_FIXTURES {
        let path = scratch_copy(fixture, "label-none");
        seed(&path);
        let options = label_options(LabelPlacement::None);
        let plan = plan_write(&path, a_minor_with_energy(7), &options).unwrap();
        assert!(
            label_changes(&plan).is_empty(),
            "{fixture}: {:?}",
            plan.changes()
        );
        assert!(!plan.is_empty(), "{fixture}");
    }
}

#[test]
fn same_label_twice_plans_nothing() {
    for fixture in ALL_FIXTURES {
        for placement in PLACEMENTS {
            let path = scratch_copy(fixture, "label-twice");
            seed(&path);
            let options = label_options(placement);
            apply_plan(&plan_write(&path, a_minor_with_energy(7), &options).unwrap()).unwrap();
            let plan = plan_write(&path, a_minor_with_energy(7), &options).unwrap();
            assert!(
                plan.is_empty(),
                "{fixture} {placement:?}: {:?}",
                plan.changes()
            );
        }
    }
}

#[test]
fn changed_label_replaces_the_previous_one() {
    for fixture in ALL_FIXTURES {
        for placement in PLACEMENTS {
            let path = scratch_copy(fixture, "label-changed");
            seed(&path);
            for energy in 7..=9 {
                let options = label_options(placement);
                let plan = plan_write(&path, a_minor_with_energy(energy), &options).unwrap();
                let changes = label_changes(&plan);
                assert_eq!(changes.len(), 1, "{fixture} {placement:?}");
                let new = changes[0].new.clone().unwrap();
                assert_eq!(new.matches("Energy").count(), 1, "{fixture}: {new}");
                assert_eq!(new.matches("8A").count(), 1, "{fixture}: {new}");
                apply_plan(&plan).unwrap();
                let (found, expected) =
                    labelled(&path, placement, &format!("8A - Energy {energy}"));
                assert_eq!(found, Some(expected), "{fixture} {placement:?}");
            }
        }
    }
}

/// Replaces the file's tag with one that holds only `comment`, written the
/// way another program would.
fn write_user_comment(path: &Path, comment: &str) {
    let saved = match extension(path) {
        "mp3" | "wav" | "aiff" => {
            let mut tag = Id3v2Tag::new();
            tag.insert(Frame::Comment(CommentFrame::new(
                TextEncoding::UTF8,
                *b"eng",
                "",
                comment.to_string(),
            )));
            tag.save_to_path(path, WriteOptions::new())
        }
        "m4a" | "mp4" => {
            let mut ilst = Ilst::new();
            ilst.insert(Atom::new(
                AtomIdent::Fourcc(*b"\xa9cmt"),
                AtomData::UTF8(comment.to_string()),
            ));
            ilst.save_to_path(path, WriteOptions::new())
        }
        "flac" | "ogg" => {
            let mut comments = VorbisComments::new();
            comments.push("COMMENT".to_string(), comment.to_string());
            comments.save_to_path(path, WriteOptions::new())
        }
        other => panic!("no comment writer for {other}"),
    };
    saved.expect("the scratch copy can be read and written");
}

#[test]
fn long_comment_with_a_url_is_kept_whole_through_repeated_writes() {
    let user_text = "https://example.com/track/1234567?utm_source=a-b&x=1 - bought 2019 - A side";
    for fixture in ALL_FIXTURES {
        for placement in [LabelPlacement::CommentStart, LabelPlacement::CommentEnd] {
            let path = scratch_copy(fixture, "label-url");
            write_user_comment(&path, user_text);
            for energy in [3, 10, 3] {
                let options = label_options(placement);
                apply_plan(&plan_write(&path, a_minor_with_energy(energy), &options).unwrap())
                    .unwrap();
                let comment = read_tags(&path).unwrap().comment.unwrap();
                let expected = if placement == LabelPlacement::CommentStart {
                    format!("8A - Energy {energy} - {user_text}")
                } else {
                    format!("{user_text} - 8A - Energy {energy}")
                };
                assert_eq!(comment, expected, "{fixture} {placement:?}");
            }
        }
    }
}

#[test]
fn user_text_that_resembles_a_label_is_kept() {
    let look_alikes = [
        "8A-side remix",
        "A - side",
        "Am - my favourite",
        "Energy 11 - warm up",
        "High Energy 7",
        "warm 8A - Energy 7 up",
    ];
    for fixture in ALL_FIXTURES {
        for user_text in look_alikes {
            let path = scratch_copy(fixture, "label-look-alike");
            write_user_comment(&path, user_text);
            for energy in [6, 7] {
                let options = label_options(LabelPlacement::CommentStart);
                let plan = plan_write(&path, a_minor_with_energy(energy), &options).unwrap();
                apply_plan(&plan).unwrap();
                assert_eq!(
                    read_tags(&path).unwrap().comment,
                    Some(format!("8A - Energy {energy} - {user_text}")),
                    "{fixture}"
                );
            }
        }
    }
}

#[test]
fn undoing_a_changed_label_brings_back_the_previous_one() {
    for fixture in [MP3, FLAC, OGG] {
        let path = scratch_copy(fixture, "label-undo");
        seed(&path);
        let options = label_options(LabelPlacement::CommentStart);
        apply_plan(&plan_write(&path, a_minor_with_energy(7), &options).unwrap()).unwrap();
        let backup =
            apply_plan(&plan_write(&path, a_minor_with_energy(8), &options).unwrap()).unwrap();

        undo(&backup).unwrap();
        assert_eq!(
            read_tags(&path).unwrap().comment.as_deref(),
            Some("8A - Energy 7 - warm up"),
            "{fixture}"
        );
    }
}

#[test]
fn energy_outside_the_scale_is_refused() {
    let path = scratch_copy(FLAC, "label-energy-range");
    for energy in [0, 11] {
        let options = label_options(LabelPlacement::CommentStart);
        let error = plan_write(&path, a_minor_with_energy(energy), &options).unwrap_err();
        assert!(
            matches!(&error, EuphoriaError::TagWrite { .. }),
            "{error:?}"
        );
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
            ..bare_options()
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
        ..bare_options()
    };
    let analysis = Analysis {
        key: None,
        tempo: Some(128.0),
    };
    assert!(plan_write(&path, &analysis, &options).unwrap().is_empty());
}

const M4A_INITIAL_KEY: &str = "----:com.apple.iTunes:initialkey";
const M4A_KEY: &str = "----:com.apple.iTunes:KEY";
const M4A_FREEFORM_BPM: &str = "----:com.apple.iTunes:BPM";

/// The fields each preset creates in a file that has none of them, by
/// format, as `docs/reference/tags.md` lists them.
fn documented_fields(preset: DjSoftware, fixture: &str) -> Vec<&'static str> {
    let keeps_decimals = matches!(preset, DjSoftware::Mixxx | DjSoftware::Generic);
    match fixture {
        MP3 | WAV | AIFF => vec!["TKEY", "TBPM"],
        M4A | ALAC | MP4 if keeps_decimals => {
            vec![M4A_INITIAL_KEY, M4A_KEY, "tmpo", M4A_FREEFORM_BPM]
        }
        M4A | ALAC | MP4 => vec![M4A_INITIAL_KEY, M4A_KEY, "tmpo"],
        FLAC | OGG => vec!["INITIALKEY", "BPM"],
        _ => unreachable!("{fixture} is not a fixture"),
    }
}

#[test]
fn every_preset_writes_its_documented_fields_in_every_format() {
    for fixture in ALL_FIXTURES {
        let path = scratch_copy(fixture, "presets");
        for preset in DjSoftware::ALL {
            let options = TagWriteOptions {
                write_bpm: true,
                preset,
                ..bare_options()
            };
            assert_eq!(
                changed_fields(&path, &a_minor_at(127.5), &options),
                documented_fields(preset, fixture),
                "{fixture} {preset:?}"
            );
        }
    }
}

#[test]
fn only_the_mixxx_preset_keeps_decimals_in_the_tempo() {
    for preset in DjSoftware::ALL {
        let path = scratch_copy(M4A, &format!("decimals-{preset:?}"));
        let options = TagWriteOptions {
            write_bpm: true,
            preset,
            ..bare_options()
        };
        let plan = plan_write(&path, &a_minor_at(127.5), &options).unwrap();
        for change in plan.changes() {
            let expected = match change.field.as_str() {
                "tmpo" => "128",
                M4A_FREEFORM_BPM if preset == DjSoftware::Mixxx => "127.5",
                M4A_FREEFORM_BPM => "128",
                _ => continue,
            };
            assert_eq!(
                change.new.as_deref(),
                Some(expected),
                "{preset:?} {}",
                change.field
            );
        }
    }
}

#[test]
fn only_the_serato_preset_leaves_the_tempo_out_unless_asked() {
    for preset in DjSoftware::ALL {
        assert_eq!(
            preset.writes_bpm_by_default(),
            preset != DjSoftware::Serato,
            "{preset:?}"
        );
    }
}

#[test]
fn an_existing_vorbis_key_field_is_updated_but_never_created() {
    for fixture in [FLAC, OGG] {
        assert_vorbis_key_field_is_updated_but_never_created(fixture);
    }
}

fn assert_vorbis_key_field_is_updated_but_never_created(fixture: &str) {
    let without = scratch_copy(fixture, "vorbis-key-absent");
    let with = scratch_copy(fixture, "vorbis-key-present");
    let mut comments = VorbisComments::new();
    comments.push("KEY".to_string(), "5A".to_string());
    comments.push("TEMPO".to_string(), "90".to_string());
    comments
        .save_to_path(&with, WriteOptions::new())
        .expect("the scratch copy can be read and written");

    for preset in DjSoftware::ALL {
        let options = TagWriteOptions {
            write_bpm: true,
            preset,
            ..bare_options()
        };
        assert_eq!(
            changed_fields(&without, &a_minor_at(128.0), &options),
            ["INITIALKEY", "BPM"],
            "{fixture} {preset:?}"
        );
        let plan = plan_write(&with, &a_minor_at(128.0), &options)
            .expect("the scratch copy can be read and written");
        let changes: Vec<(&str, Option<&str>)> = plan
            .changes()
            .iter()
            .map(|change| (change.field.as_str(), change.new.as_deref()))
            .collect();
        assert_eq!(
            changes,
            [
                ("INITIALKEY", Some("8A")),
                ("KEY", Some("8A")),
                ("BPM", Some("128")),
                ("TEMPO", Some("128")),
            ],
            "{fixture} {preset:?}"
        );
    }
}

#[test]
fn an_existing_freeform_m4a_tempo_is_updated_by_every_preset() {
    let path = scratch_copy(M4A, "freeform-tempo-present");
    let mut file =
        Mp4File::read_from(&mut File::open(&path).unwrap(), ParseOptions::new()).unwrap();
    let mut ilst = file.ilst().cloned().unwrap_or_default();
    ilst.insert(Atom::new(
        AtomIdent::Freeform {
            mean: "com.apple.iTunes".into(),
            name: "BPM".into(),
        },
        AtomData::UTF8("90".to_string()),
    ));
    file.set_ilst(ilst);
    file.save_to_path(&path, WriteOptions::new()).unwrap();

    for preset in DjSoftware::ALL {
        let options = TagWriteOptions {
            write_bpm: true,
            preset,
            ..bare_options()
        };
        let plan = plan_write(&path, &a_minor_at(128.0), &options).unwrap();
        let freeform: Vec<Option<&str>> = plan
            .changes()
            .iter()
            .filter(|change| change.field.eq_ignore_ascii_case(M4A_FREEFORM_BPM))
            .map(|change| change.new.as_deref())
            .collect();
        assert_eq!(freeform, [Some("128")], "{preset:?}");
    }
}

#[test]
fn open_key_notation_reaches_the_key_field_and_the_label() {
    let path = scratch_copy(MP3, "open-key");
    seed(&path);
    let options = TagWriteOptions {
        key_notation: KeyNotation::OpenKey,
        write_bpm: false,
        label: LabelPlacement::CommentStart,
        preset: DjSoftware::Traktor,
    };
    apply_plan(&plan_write(&path, a_minor_with_energy(7), &options).unwrap()).unwrap();
    let tags = read_tags(&path).unwrap();
    assert_eq!(tags.key.as_deref(), Some("1m"));
    assert_eq!(tags.comment.as_deref(), Some("1m - Energy 7 - warm up"));

    let camelot = TagWriteOptions {
        key_notation: KeyNotation::Camelot,
        ..options
    };
    apply_plan(&plan_write(&path, a_minor_with_energy(8), &camelot).unwrap()).unwrap();
    let tags = read_tags(&path).unwrap();
    assert_eq!(tags.comment.as_deref(), Some("8A - Energy 8 - warm up"));
}

#[test]
fn commercial_presets_write_a_whole_tempo() {
    let path = scratch_copy(MP3, "whole-tempo");
    let options = TagWriteOptions {
        write_bpm: true,
        preset: DjSoftware::Serato,
        ..bare_options()
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
        ..bare_options()
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
        ..bare_options()
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
