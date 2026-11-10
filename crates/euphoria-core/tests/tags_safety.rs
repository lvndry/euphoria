// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A tag write changes the planned fields and nothing else: other tags,
//! pictures, unknown data, the audio, and the way the file is linked all
//! survive, and a file that cannot be rewritten without loss is refused.

use std::fs::File;
use std::path::{Path, PathBuf};

use euphoria_core::detect::Analysis;
use euphoria_core::tags::{
    DjSoftware, KeyNotation, Label, LabelTarget, TagWriteOptions, apply_plan, plan_write,
    read_tags, undo,
};
use euphoria_core::{EuphoriaError, Key, Mode, decode_mono};
use lofty::TextEncoding;
use lofty::ape::{ApeItem, ApeTag};
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::AudioFile;
use lofty::flac::FlacFile;
use lofty::id3::v1::Id3v1Tag;
use lofty::id3::v2::{
    AttachedPictureFrame, Frame, FrameId, Id3v2Tag, Id3v2Version, PrivateFrame,
    TextInformationFrame,
};
use lofty::iff::aiff::{AiffFile, AiffTextChunks};
use lofty::iff::wav::{RiffInfoList, WavFile};
use lofty::mp4::{Atom, AtomData, AtomIdent, Ilst, Mp4File};
use lofty::mpeg::MpegFile;
use lofty::ogg::OggPictureStorage;
use lofty::ogg::tag::VorbisComments;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::tag::{ItemValue, TagExt};

const MP3: &str = "sine-440hz-mono.mp3";
const WAV: &str = "sine-440hz-mono.wav";
const AIFF: &str = "sine-440hz-mono.aiff";
const M4A: &str = "sine-440hz-mono.m4a";
const FLAC: &str = "sine-440hz-mono.flac";
const ALL_FIXTURES: [&str; 5] = [MP3, WAV, AIFF, M4A, FLAC];

const TITLE: &str = "Test Tone";
const CUE_DATA: &str = "cue points of another program";
const CHAPTER_ATOM_BODY: &[u8] = b"chapters that no tag library knows about";
const WORKS: &str = "the scratch copy can be read and written";
/// A PNG image of one transparent pixel.
const ARTWORK: [u8; 67] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn fixture_bytes(fixture: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(fixture);
    std::fs::read(path).expect("the fixture can be read")
}

fn scratch_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("safety-{name}"))
}

fn scratch_copy(fixture: &str, test: &str) -> PathBuf {
    let path = scratch_path(&format!("{test}-{fixture}"));
    std::fs::write(&path, fixture_bytes(fixture)).expect(WORKS);
    path
}

fn extension(path: &Path) -> &str {
    path.extension()
        .and_then(|text| text.to_str())
        .expect(WORKS)
}

fn artwork(picture_type: PictureType) -> Picture {
    Picture::unchecked(ARTWORK.to_vec())
        .pic_type(picture_type)
        .mime_type(MimeType::Png)
        .description("cover")
        .build()
}

fn big_endian_u32(bytes: &[u8], offset: usize) -> usize {
    let word: [u8; 4] = bytes[offset..offset + 4].try_into().expect(WORKS);
    u32::from_be_bytes(word) as usize
}

/// The offset and size of the first atom named `name` among the atoms that
/// fill `bytes[start..end]`.
fn find_atom(bytes: &[u8], start: usize, end: usize, name: &[u8; 4]) -> Option<(usize, usize)> {
    let mut offset = start;
    while offset + 8 <= end {
        let size = big_endian_u32(bytes, offset);
        if &bytes[offset + 4..offset + 8] == name {
            return Some((offset, size));
        }
        offset += size.max(8);
    }
    None
}

/// Puts a `chpl` atom, which lofty does not model, next to the tags in
/// `moov.udta`.
fn add_chapter_atom(path: &Path) {
    let mut bytes = std::fs::read(path).expect(WORKS);
    let (moov, moov_size) = find_atom(&bytes, 0, bytes.len(), b"moov").expect("a moov atom");
    let (mdat, _) = find_atom(&bytes, 0, bytes.len(), b"mdat").expect("an mdat atom");
    assert!(mdat < moov, "growing moov would move the audio");
    let (udta, udta_size) =
        find_atom(&bytes, moov + 8, moov + moov_size, b"udta").expect("a udta atom");

    let mut atom = Vec::new();
    atom.extend((CHAPTER_ATOM_BODY.len() as u32 + 8).to_be_bytes());
    atom.extend(b"chpl");
    atom.extend(CHAPTER_ATOM_BODY);
    for (offset, size) in [(moov, moov_size), (udta, udta_size)] {
        let grown = (size + atom.len()) as u32;
        bytes[offset..offset + 4].copy_from_slice(&grown.to_be_bytes());
    }
    let end_of_udta = udta + udta_size;
    bytes.splice(end_of_udta..end_of_udta, atom);
    std::fs::write(path, bytes).expect(WORKS);
}

fn seeded_id3v2() -> Id3v2Tag {
    let mut tag = Id3v2Tag::new();
    tag.insert(Frame::Text(TextInformationFrame::new(
        FrameId::new("TIT2").expect(WORKS),
        TextEncoding::UTF8,
        TITLE,
    )));
    tag.insert(Frame::Private(PrivateFrame::new(
        "TRAKTOR4",
        CUE_DATA.as_bytes().to_vec(),
    )));
    tag.insert(Frame::Picture(AttachedPictureFrame::new(
        TextEncoding::Latin1,
        artwork(PictureType::CoverFront),
    )));
    tag
}

/// Fills the file with every kind of metadata its format can hold next to
/// the tag Euphoria writes: artwork, private data of other programs, and
/// the format's other tag types.
fn seed_everything(path: &Path) {
    let options = WriteOptions::new();
    match extension(path) {
        "mp3" => {
            seeded_id3v2().save_to_path(path, options).expect(WORKS);
            let version_1 = Id3v1Tag {
                title: Some("Version one title".to_string()),
                artist: Some("Version one artist".to_string()),
                ..Id3v1Tag::default()
            };
            version_1.save_to_path(path, options).expect(WORKS);
            let mut ape = ApeTag::new();
            ape.insert(
                ApeItem::new(
                    "Album".to_string(),
                    ItemValue::Text("APE album".to_string()),
                )
                .expect(WORKS),
            );
            ape.save_to_path(path, options).expect(WORKS);
        }
        "wav" => {
            seeded_id3v2().save_to_path(path, options).expect(WORKS);
            let mut info = RiffInfoList::new();
            info.insert("INAM".to_string(), "RIFF title".to_string());
            info.insert("IART".to_string(), "RIFF artist".to_string());
            info.save_to_path(path, options).expect(WORKS);
        }
        "aiff" => {
            seeded_id3v2().save_to_path(path, options).expect(WORKS);
            let chunks = AiffTextChunks {
                name: Some("AIFF name".to_string()),
                author: Some("AIFF author".to_string()),
                annotations: Some(vec!["AIFF annotation".to_string()]),
                ..AiffTextChunks::default()
            };
            chunks.save_to_path(path, options).expect(WORKS);
        }
        "m4a" => {
            let mut ilst = Ilst::new();
            ilst.insert(Atom::new(
                AtomIdent::Fourcc(*b"\xa9nam"),
                AtomData::UTF8(TITLE.to_string()),
            ));
            ilst.insert(Atom::new(
                AtomIdent::Freeform {
                    mean: "com.serato.dj".into(),
                    name: "markersv2".into(),
                },
                AtomData::UTF8(CUE_DATA.to_string()),
            ));
            ilst.insert_picture(artwork(PictureType::CoverFront));
            ilst.save_to_path(path, options).expect(WORKS);
            add_chapter_atom(path);
        }
        "flac" => {
            let mut comments = VorbisComments::new();
            comments.push("TITLE".to_string(), TITLE.to_string());
            comments.push("SERATO_MARKERS_V2".to_string(), CUE_DATA.to_string());
            for picture_type in [PictureType::CoverFront, PictureType::CoverBack] {
                comments
                    .insert_picture(artwork(picture_type), None)
                    .expect(WORKS);
            }
            comments.save_to_path(path, options).expect(WORKS);
        }
        other => panic!("no seed for {other}"),
    }
}

/// The encoded audio of the file, located by walking the container, without
/// any of the metadata around it.
fn audio_payload(path: &Path) -> Vec<u8> {
    let bytes = std::fs::read(path).expect(WORKS);
    match extension(path) {
        "mp3" => {
            let start = id3v2_length(&bytes);
            let ape = bytes[start..]
                .windows(8)
                .position(|window| window == b"APETAGEX")
                .map(|position| start + position);
            let has_version_1 = bytes.len() >= 128 && &bytes[bytes.len() - 128..][..3] == b"TAG";
            let version_1 = has_version_1.then(|| bytes.len() - 128);
            let end = ape.or(version_1).unwrap_or(bytes.len());
            bytes[start..end].to_vec()
        }
        "wav" => {
            let mut offset = 12;
            loop {
                let size_bytes: [u8; 4] = bytes[offset + 4..offset + 8].try_into().expect(WORKS);
                let size = u32::from_le_bytes(size_bytes) as usize;
                if &bytes[offset..offset + 4] == b"data" {
                    return bytes[offset + 8..offset + 8 + size].to_vec();
                }
                offset += 8 + size + size % 2;
            }
        }
        "aiff" => {
            let mut offset = 12;
            loop {
                let size = big_endian_u32(&bytes, offset + 4);
                if &bytes[offset..offset + 4] == b"SSND" {
                    return bytes[offset + 8..offset + 8 + size].to_vec();
                }
                offset += 8 + size + size % 2;
            }
        }
        "m4a" => {
            let (offset, size) = find_atom(&bytes, 0, bytes.len(), b"mdat").expect("an mdat atom");
            bytes[offset + 8..offset + size].to_vec()
        }
        "flac" => {
            let mut offset = id3v2_length(&bytes);
            assert_eq!(&bytes[offset..offset + 4], b"fLaC");
            offset += 4;
            loop {
                let is_last = bytes[offset] & 0x80 != 0;
                let size = big_endian_u32(&bytes, offset) & 0x00FF_FFFF;
                offset += 4 + size;
                if is_last {
                    return bytes[offset..].to_vec();
                }
            }
        }
        other => panic!("no audio locator for {other}"),
    }
}

/// The length of the ID3v2 tag at the start of `bytes`, or zero.
fn id3v2_length(bytes: &[u8]) -> usize {
    if &bytes[..3] != b"ID3" {
        return 0;
    }
    let size = bytes[6..10]
        .iter()
        .fold(0_usize, |size, byte| (size << 7) | usize::from(*byte));
    10 + size
}

/// Everything about a file that a tag write must not change, plus the tags.
#[derive(PartialEq)]
struct Snapshot {
    tags: String,
    pictures: Vec<Vec<u8>>,
    properties: String,
    samples: Vec<f32>,
    audio: Vec<u8>,
    has_chapter_atom: bool,
}

fn id3v2_pictures(tag: Option<&Id3v2Tag>) -> Vec<Vec<u8>> {
    tag.into_iter()
        .flatten()
        .filter_map(|frame| match frame {
            Frame::Picture(frame) => Some(frame.picture.data().to_vec()),
            _ => None,
        })
        .collect()
}

fn snapshot(path: &Path) -> Snapshot {
    let options = ParseOptions::new().implicit_conversions(false);
    let mut file = File::open(path).expect(WORKS);
    let (tags, pictures) = match extension(path) {
        "mp3" => {
            let file = MpegFile::read_from(&mut file, options).expect(WORKS);
            (
                format!("{:?}", (file.id3v2(), file.id3v1(), file.ape())),
                id3v2_pictures(file.id3v2()),
            )
        }
        "wav" => {
            let file = WavFile::read_from(&mut file, options).expect(WORKS);
            (
                format!("{:?}", (file.id3v2(), file.riff_info())),
                id3v2_pictures(file.id3v2()),
            )
        }
        "aiff" => {
            let file = AiffFile::read_from(&mut file, options).expect(WORKS);
            (
                format!("{:?}", (file.id3v2(), file.text_chunks())),
                id3v2_pictures(file.id3v2()),
            )
        }
        "m4a" => {
            let file = Mp4File::read_from(&mut file, options).expect(WORKS);
            let pictures = file
                .ilst()
                .and_then(Ilst::pictures)
                .into_iter()
                .flatten()
                .map(|picture| picture.data().to_vec())
                .collect();
            (format!("{:?}", file.ilst()), pictures)
        }
        "flac" => {
            let file = FlacFile::read_from(&mut file, options).expect(WORKS);
            let pictures = file
                .pictures()
                .iter()
                .map(|(picture, _)| picture.data().to_vec())
                .collect();
            (
                format!(
                    "{:?}",
                    (file.id3v2(), file.vorbis_comments(), file.pictures())
                ),
                pictures,
            )
        }
        other => panic!("no tag reader for {other}"),
    };

    let tagged = lofty::read_from_path(path).expect(WORKS);
    let properties = tagged.properties();
    let bytes = std::fs::read(path).expect(WORKS);
    Snapshot {
        tags,
        pictures,
        properties: format!(
            "{:?}",
            (
                properties.duration(),
                properties.sample_rate(),
                properties.bit_depth(),
                properties.channels()
            )
        ),
        samples: decode_mono(path).expect(WORKS).samples().to_vec(),
        audio: audio_payload(path),
        has_chapter_atom: bytes
            .windows(CHAPTER_ATOM_BODY.len())
            .any(|window| window == CHAPTER_ATOM_BODY),
    }
}

/// Compares field by field, so a failure names what changed without
/// printing megabytes of audio.
fn assert_same_except_tags(found: &Snapshot, expected: &Snapshot, context: &str) {
    assert_eq!(found.pictures, expected.pictures, "{context}: pictures");
    assert_eq!(
        found.properties, expected.properties,
        "{context}: properties"
    );
    assert!(found.samples == expected.samples, "{context}: samples");
    assert!(found.audio == expected.audio, "{context}: audio bytes");
    assert_eq!(
        found.has_chapter_atom, expected.has_chapter_atom,
        "{context}: chapter atom"
    );
}

fn a_minor() -> Analysis {
    Analysis {
        key: Some(Key::new(9, Mode::Minor)),
        tempo: Some(127.5),
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

fn syncsafe(size: usize) -> [u8; 4] {
    [
        (size >> 21) as u8 & 0x7F,
        (size >> 14) as u8 & 0x7F,
        (size >> 7) as u8 & 0x7F,
        size as u8 & 0x7F,
    ]
}

fn latin1_text(text: &str) -> Vec<u8> {
    let mut body = vec![0];
    body.extend(text.bytes());
    body
}

/// An ID3v2.3 tag built byte by byte, so it can hold frames a tag library
/// would not write.
fn id3v23_tag(frames: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (id, content) in frames {
        body.extend(id.bytes());
        body.extend((content.len() as u32).to_be_bytes());
        body.extend([0, 0]);
        body.extend(content);
    }
    let mut tag = b"ID3\x03\x00\x00".to_vec();
    tag.extend(syncsafe(body.len()));
    tag.extend(body);
    tag
}

fn mp3_with_tag(name: &str, tag: &[u8]) -> PathBuf {
    let fixture = fixture_bytes(MP3);
    let mut bytes = tag.to_vec();
    bytes.extend(&fixture[id3v2_length(&fixture)..]);
    let path = scratch_path(name);
    std::fs::write(&path, bytes).expect(WORKS);
    path
}

fn id3v2_text(path: &Path, id: &str) -> Option<String> {
    let options = ParseOptions::new().implicit_conversions(false);
    let mut file = File::open(path).expect(WORKS);
    let file = MpegFile::read_from(&mut file, options).expect(WORKS);
    file.id3v2()?
        .get_text(&FrameId::new(id).expect(WORKS))
        .map(str::to_string)
}

#[test]
fn everything_else_survives_apply_and_undo() {
    for fixture in ALL_FIXTURES {
        for seeded in [false, true] {
            let context = format!("{fixture}, seeded: {seeded}");
            let path = scratch_copy(fixture, &format!("guard-{seeded}"));
            if seeded {
                seed_everything(&path);
            }
            let original = snapshot(&path);
            assert_eq!(original.pictures.is_empty(), !seeded, "{context}");
            assert_eq!(original.has_chapter_atom, seeded && fixture == M4A);

            let plan = plan_write(&path, &a_minor(), &full_options())
                .unwrap_or_else(|error| panic!("{context}: {}", error.chain()));
            assert!(!plan.is_empty(), "{context}");
            let unchanged = snapshot(&path);
            assert!(
                unchanged == original,
                "{context}: planning changed the file"
            );

            let backup = apply_plan(&plan).unwrap();
            let written = snapshot(&path);
            assert_ne!(written.tags, original.tags, "{context}");
            assert_same_except_tags(&written, &original, &context);

            undo(&backup).unwrap();
            let restored = snapshot(&path);
            assert_eq!(restored.tags, original.tags, "{context}");
            assert_same_except_tags(&restored, &original, &context);
        }
    }
}

#[test]
fn id3_artwork_survives_in_every_id3_format() {
    for fixture in [MP3, WAV, AIFF] {
        let path = scratch_copy(fixture, "id3-artwork");
        seed_everything(&path);
        assert_eq!(snapshot(&path).pictures, [ARTWORK.to_vec()], "{fixture}");
        let backup = apply_plan(&plan_write(&path, &a_minor(), &full_options()).unwrap()).unwrap();
        assert_eq!(snapshot(&path).pictures, [ARTWORK.to_vec()], "{fixture}");
        undo(&backup).unwrap();
        assert_eq!(snapshot(&path).pictures, [ARTWORK.to_vec()], "{fixture}");
    }
}

#[test]
fn flac_keeps_every_picture_block_in_order() {
    let path = scratch_copy(FLAC, "flac-pictures");
    seed_everything(&path);
    let picture_types = |path: &Path| -> Vec<PictureType> {
        let mut file = File::open(path).unwrap();
        FlacFile::read_from(&mut file, ParseOptions::new())
            .unwrap()
            .pictures()
            .iter()
            .map(|(picture, _)| picture.pic_type())
            .collect()
    };
    let expected = [PictureType::CoverFront, PictureType::CoverBack];
    assert_eq!(picture_types(&path), expected);

    let backup = apply_plan(&plan_write(&path, &a_minor(), &full_options()).unwrap()).unwrap();
    assert_eq!(picture_types(&path), expected);
    assert_eq!(
        snapshot(&path).pictures,
        [ARTWORK.to_vec(), ARTWORK.to_vec()]
    );
    undo(&backup).unwrap();
    assert_eq!(picture_types(&path), expected);
}

#[test]
fn flac_with_pictures_but_no_comments_keeps_them() {
    let path = scratch_copy(FLAC, "flac-pictures-only");
    let mut comments = VorbisComments::new();
    comments
        .insert_picture(artwork(PictureType::CoverFront), None)
        .unwrap();
    comments.save_to_path(&path, WriteOptions::new()).unwrap();

    let backup = apply_plan(&plan_write(&path, &a_minor(), &full_options()).unwrap()).unwrap();
    assert_eq!(snapshot(&path).pictures, [ARTWORK.to_vec()]);
    undo(&backup).unwrap();
    assert_eq!(snapshot(&path).pictures, [ARTWORK.to_vec()]);
    assert_eq!(read_tags(&path).unwrap().key, None);
}

#[test]
fn id3v2_tag_in_front_of_a_flac_stream_is_left_in_place() {
    let leading_tag = id3v23_tag(&[("TIT2", latin1_text("Leading tag title"))]);
    let mut bytes = leading_tag.clone();
    bytes.extend(fixture_bytes(FLAC));
    let path = scratch_path("flac-leading-id3.flac");
    std::fs::write(&path, bytes).unwrap();
    let original_audio = audio_payload(&path);

    let backup = apply_plan(&plan_write(&path, &a_minor(), &full_options()).unwrap()).unwrap();
    assert_eq!(read_tags(&path).unwrap().key.as_deref(), Some("8A"));
    assert!(std::fs::read(&path).unwrap().starts_with(&leading_tag));
    assert!(audio_payload(&path) == original_audio);

    undo(&backup).unwrap();
    assert!(std::fs::read(&path).unwrap().starts_with(&leading_tag));
    assert_eq!(read_tags(&path).unwrap().key, None);
}

#[test]
fn tag_with_an_unparsable_frame_is_read_but_not_rewritten() {
    let tag = id3v23_tag(&[
        ("TIT2", latin1_text("Odd tag")),
        ("TPE1", Vec::new()),
        ("TALB", latin1_text("Kept album")),
    ]);
    let path = mp3_with_tag("odd-frame.mp3", &tag);
    let bytes = std::fs::read(&path).unwrap();

    assert_eq!(read_tags(&path).unwrap().title.as_deref(), Some("Odd tag"));

    let error = plan_write(&path, &a_minor(), &full_options()).unwrap_err();
    assert!(
        matches!(&error, EuphoriaError::TagWrite { path: error_path, .. } if *error_path == path),
        "{error:?}"
    );
    assert!(
        error.chain().contains("refusing to rewrite"),
        "{}",
        error.chain()
    );
    assert!(
        std::error::Error::source(&error)
            .unwrap()
            .source()
            .is_some()
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

#[test]
fn unparsable_frame_in_a_wav_or_aiff_chunk_is_refused_too() {
    let tag = id3v23_tag(&[("TIT2", latin1_text("Odd tag")), ("TPE1", Vec::new())]);
    for fixture in [WAV, AIFF] {
        let mut bytes = fixture_bytes(fixture);
        let chunk_size = tag.len() as u32;
        let container_size = (bytes.len() - 8 + 8 + tag.len()) as u32;
        bytes.extend(b"ID3 ");
        if fixture == WAV {
            bytes.extend(chunk_size.to_le_bytes());
            bytes[4..8].copy_from_slice(&container_size.to_le_bytes());
        } else {
            bytes.extend(chunk_size.to_be_bytes());
            bytes[4..8].copy_from_slice(&container_size.to_be_bytes());
        }
        bytes.extend(&tag);
        let path = scratch_path(&format!("odd-chunk-{fixture}"));
        std::fs::write(&path, &bytes).unwrap();

        assert_eq!(
            read_tags(&path).unwrap().title.as_deref(),
            Some("Odd tag"),
            "{fixture}"
        );
        let error = plan_write(&path, &a_minor(), &full_options()).unwrap_err();
        assert!(
            error.chain().contains("refusing to rewrite"),
            "{fixture}: {}",
            error.chain()
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "{fixture}");
    }
}

#[test]
fn flaw_in_a_tag_that_is_not_rewritten_does_not_stop_a_write() {
    let path = scratch_copy(MP3, "flawed-bystander");
    let without_year = Id3v1Tag {
        title: Some("No year".to_string()),
        ..Id3v1Tag::default()
    };
    without_year
        .save_to_path(&path, WriteOptions::new())
        .unwrap();
    let version_1 = |path: &Path| {
        let mut file = File::open(path).unwrap();
        let file = MpegFile::read_from(&mut file, ParseOptions::new()).unwrap();
        format!("{:?}", file.id3v1())
    };
    let original = version_1(&path);
    assert!(original.contains("No year"));

    let backup = apply_plan(&plan_write(&path, &a_minor(), &full_options()).unwrap()).unwrap();
    assert_eq!(read_tags(&path).unwrap().key.as_deref(), Some("8A"));
    assert_eq!(version_1(&path), original);
    undo(&backup).unwrap();
    assert_eq!(version_1(&path), original);
}

#[test]
fn version_4_frames_in_a_version_3_tag_are_kept() {
    let tag = id3v23_tag(&[
        ("TIT2", latin1_text("Sorted")),
        ("TSOP", latin1_text("Wave, Sine")),
        ("TYER", latin1_text("1999")),
    ]);
    let path = mp3_with_tag("sort-frame.mp3", &tag);

    let backup = apply_plan(&plan_write(&path, &a_minor(), &full_options()).unwrap()).unwrap();
    assert_eq!(id3v2_text(&path, "TKEY").as_deref(), Some("8A"));
    for check in ["after the write", "after undo"] {
        assert_eq!(
            id3v2_text(&path, "TSOP").as_deref(),
            Some("Wave, Sine"),
            "{check}"
        );
        assert_eq!(
            id3v2_text(&path, "TYER").as_deref(),
            Some("1999"),
            "{check}"
        );
        assert_eq!(
            id3v2_text(&path, "TIT2").as_deref(),
            Some("Sorted"),
            "{check}"
        );
        if check == "after the write" {
            undo(&backup).unwrap();
        }
    }
    assert_eq!(id3v2_text(&path, "TKEY"), None);

    let mut file = File::open(&path).unwrap();
    let version = MpegFile::read_from(&mut file, ParseOptions::new())
        .unwrap()
        .id3v2()
        .unwrap()
        .original_version();
    assert_eq!(version, Id3v2Version::V4);
}

#[cfg(unix)]
#[test]
fn symbolic_link_stays_a_link_and_the_real_file_is_written() {
    for fixture in ALL_FIXTURES {
        let real = scratch_copy(fixture, "symlink-real");
        let link = scratch_path(&format!("symlink-link-{fixture}"));
        if link.symlink_metadata().is_ok() {
            std::fs::remove_file(&link).unwrap();
        }
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let is_link = || link.symlink_metadata().unwrap().file_type().is_symlink();

        let backup = apply_plan(&plan_write(&link, &a_minor(), &full_options()).unwrap()).unwrap();
        assert!(is_link(), "{fixture}: the link was replaced by a file");
        assert_eq!(
            read_tags(&real).unwrap().key.as_deref(),
            Some("8A"),
            "{fixture}"
        );
        assert_eq!(
            read_tags(&link).unwrap().key.as_deref(),
            Some("8A"),
            "{fixture}"
        );

        undo(&backup).unwrap();
        assert!(is_link(), "{fixture}");
        assert_eq!(read_tags(&real).unwrap().key, None, "{fixture}");
    }
}

#[cfg(unix)]
#[test]
fn hard_links_keep_sharing_one_file() {
    use std::os::unix::fs::MetadataExt;

    for fixture in ALL_FIXTURES {
        let first = scratch_copy(fixture, "hardlink-first");
        let second = scratch_path(&format!("hardlink-second-{fixture}"));
        if second.exists() {
            std::fs::remove_file(&second).unwrap();
        }
        std::fs::hard_link(&first, &second).unwrap();
        let inode = std::fs::metadata(&first).unwrap().ino();

        let backup = apply_plan(&plan_write(&first, &a_minor(), &full_options()).unwrap()).unwrap();
        for path in [&first, &second] {
            let metadata = std::fs::metadata(path).unwrap();
            assert_eq!(metadata.ino(), inode, "{fixture}");
            assert_eq!(metadata.nlink(), 2, "{fixture}");
            assert_eq!(
                read_tags(path).unwrap().key.as_deref(),
                Some("8A"),
                "{fixture}"
            );
            decode_mono(path).unwrap();
        }

        undo(&backup).unwrap();
        assert_eq!(read_tags(&second).unwrap().key, None, "{fixture}");
        assert_eq!(
            std::fs::metadata(&second).unwrap().ino(),
            inode,
            "{fixture}"
        );
    }
}

#[test]
fn writes_leave_no_temporary_files() {
    let directory = scratch_path("leftovers");
    if directory.exists() {
        std::fs::remove_dir_all(&directory).unwrap();
    }
    std::fs::create_dir(&directory).unwrap();
    for fixture in ALL_FIXTURES {
        let path = directory.join(fixture);
        std::fs::write(&path, fixture_bytes(fixture)).unwrap();
        let backup = apply_plan(&plan_write(&path, &a_minor(), &full_options()).unwrap()).unwrap();
        undo(&backup).unwrap();
    }
    let refused = directory.join("odd.mp3");
    let tag = id3v23_tag(&[("TPE1", Vec::new())]);
    std::fs::copy(mp3_with_tag("leftovers-odd.mp3", &tag), &refused).unwrap();
    plan_write(&refused, &a_minor(), &full_options()).unwrap_err();

    let mut names: Vec<String> = std::fs::read_dir(&directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    let mut expected: Vec<String> = ALL_FIXTURES.iter().map(|name| name.to_string()).collect();
    expected.push("odd.mp3".to_string());
    expected.sort();
    assert_eq!(names, expected);
}
