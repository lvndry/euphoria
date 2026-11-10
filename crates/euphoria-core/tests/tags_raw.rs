// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A tag write compared at the level of raw bytes, with parsers written
//! here that share nothing with the library: every item outside the plan
//! keeps its exact bytes and its place, and undo gives back the original
//! file byte for byte. The inputs hold the kinds of items that tag
//! libraries tend to normalise when they read and write a tag.

use std::path::{Path, PathBuf};

use euphoria_core::decode_mono;
use euphoria_core::detect::Analysis;
use euphoria_core::tags::{
    DjSoftware, KeyNotation, Label, LabelTarget, TagWriteOptions, apply_plan, plan_write,
    read_tags, undo,
};
use euphoria_core::{Key, Mode};

const MP3: &str = "sine-440hz-mono.mp3";
const WAV: &str = "sine-440hz-mono.wav";
const AIFF: &str = "sine-440hz-mono.aiff";
const M4A: &str = "sine-440hz-mono.m4a";
const FLAC: &str = "sine-440hz-mono.flac";
const WORKS: &str = "the scratch file can be read and written";

fn fixture_bytes(fixture: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(fixture);
    std::fs::read(path).expect("the fixture can be read")
}

fn scratch_file(name: &str, bytes: &[u8]) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("raw-{name}"));
    std::fs::write(&path, bytes).expect(WORKS);
    path
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

fn samples(path: &Path) -> Vec<f32> {
    decode_mono(path).expect(WORKS).samples().to_vec()
}

/// Applies the full plan and undoes it. Returns the file's bytes after the
/// write, and checks that undo gives back exactly `original`.
fn write_then_undo(path: &Path, original: &[u8]) -> Vec<u8> {
    let plan = plan_write(path, &a_minor(), &full_options()).expect(WORKS);
    assert_eq!(
        std::fs::read(path).expect(WORKS),
        original,
        "planning wrote"
    );
    let backup = apply_plan(&plan).expect(WORKS);
    let written = std::fs::read(path).expect(WORKS);
    assert_ne!(written, original);
    undo(&backup).expect(WORKS);
    let restored = std::fs::read(path).expect(WORKS);
    assert!(
        restored == original,
        "undo did not restore the original bytes"
    );
    written
}

/// `after` must be `before` with only the items at `changed` different,
/// followed by `added` new items.
fn assert_only_changed(before: &[Vec<u8>], after: &[Vec<u8>], changed: &[usize], added: usize) {
    assert_eq!(after.len(), before.len() + added, "item count");
    for (index, item) in before.iter().enumerate() {
        if changed.contains(&index) {
            assert_ne!(&after[index], item, "item {index} was planned to change");
        } else {
            assert!(&after[index] == item, "item {index} changed: {item:?}");
        }
    }
}

// MP4

fn big_endian_u32(bytes: &[u8], offset: usize) -> usize {
    let word: [u8; 4] = bytes[offset..offset + 4].try_into().expect(WORKS);
    u32::from_be_bytes(word) as usize
}

fn atom(kind: &[u8; 4], content: &[u8]) -> Vec<u8> {
    let mut bytes = (content.len() as u32 + 8).to_be_bytes().to_vec();
    bytes.extend(kind);
    bytes.extend(content);
    bytes
}

/// A `data` atom with every header field chosen by the caller.
fn data(type_word: u32, locale: u32, payload: &[u8]) -> Vec<u8> {
    let mut content = type_word.to_be_bytes().to_vec();
    content.extend(locale.to_be_bytes());
    content.extend(payload);
    atom(b"data", &content)
}

fn freeform(mean: &str, name: &str, data_atoms: &[u8]) -> Vec<u8> {
    let text = |kind: &[u8; 4], text: &str| atom(kind, &[&[0; 4], text.as_bytes()].concat());
    atom(
        b"----",
        &[
            text(b"mean", mean),
            text(b"name", name),
            data_atoms.to_vec(),
        ]
        .concat(),
    )
}

/// The start and end of each atom in `bytes[start..end]`, with its name.
fn atoms(bytes: &[u8], start: usize, end: usize) -> Vec<([u8; 4], usize, usize)> {
    let mut found = Vec::new();
    let mut offset = start;
    while offset + 8 <= end {
        let size = big_endian_u32(bytes, offset);
        let kind: [u8; 4] = bytes[offset + 4..offset + 8].try_into().expect(WORKS);
        found.push((kind, offset, offset + size));
        offset += size;
    }
    assert_eq!(offset, end, "atoms do not fill their parent");
    found
}

fn find(bytes: &[u8], start: usize, end: usize, kind: &[u8; 4]) -> (usize, usize) {
    atoms(bytes, start, end)
        .into_iter()
        .find(|(found, _, _)| found == kind)
        .map(|(_, start, end)| (start, end))
        .unwrap_or_else(|| panic!("no {} atom", String::from_utf8_lossy(kind)))
}

/// The ranges of `moov`, `udta`, `meta` and `ilst`, outermost first.
fn metadata_path(bytes: &[u8]) -> [(usize, usize); 4] {
    let moov = find(bytes, 0, bytes.len(), b"moov");
    let udta = find(bytes, moov.0 + 8, moov.1, b"udta");
    let meta = find(bytes, udta.0 + 8, udta.1, b"meta");
    let ilst = find(bytes, meta.0 + 12, meta.1, b"ilst");
    [moov, udta, meta, ilst]
}

fn ilst_items(bytes: &[u8]) -> Vec<Vec<u8>> {
    let ilst = metadata_path(bytes)[3];
    atoms(bytes, ilst.0 + 8, ilst.1)
        .into_iter()
        .map(|(_, start, end)| bytes[start..end].to_vec())
        .collect()
}

/// The M4A fixture with the items of its `ilst` exchanged for `items`. The
/// fixture keeps `moov` after the audio, so nothing else has to move.
fn m4a_with_items(items: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = fixture_bytes(M4A);
    let path = metadata_path(&bytes);
    let ilst = path[3];
    let content = items.concat();
    let growth = content.len() as i64 - (ilst.1 - ilst.0 - 8) as i64;
    for (start, end) in path {
        let size = ((end - start) as i64 + growth) as u32;
        bytes[start..start + 4].copy_from_slice(&size.to_be_bytes());
    }
    bytes.splice(ilst.0 + 8..ilst.1, content);
    bytes
}

/// `bytes` with `moov` moved in front of the audio, followed by a `free`
/// atom of `padding` bytes when `padding` is not zero. The chunk offsets
/// are moved along, as a program that optimises for streaming does.
fn with_moov_first(bytes: &[u8], padding: usize) -> Vec<u8> {
    let top = atoms(bytes, 0, bytes.len());
    let (_, moov_start, moov_end) = *top.last().expect(WORKS);
    assert_eq!(&bytes[moov_start + 4..moov_start + 8], b"moov");
    let (_, _, ftyp_end) = top[0];
    let mut moov = bytes[moov_start..moov_end].to_vec();
    let free = if padding == 0 {
        Vec::new()
    } else {
        atom(b"free", &vec![0; padding - 8])
    };

    let distance = (moov.len() + free.len()) as u32;
    let table = moov
        .windows(4)
        .position(|window| window == b"stco")
        .expect("a chunk offset table");
    let count = big_endian_u32(&moov, table + 8);
    for entry in 0..count {
        let at = table + 12 + entry * 4;
        let moved = big_endian_u32(&moov, at) as u32 + distance;
        moov[at..at + 4].copy_from_slice(&moved.to_be_bytes());
    }
    [
        &bytes[..ftyp_end],
        &moov[..],
        &free[..],
        &bytes[ftyp_end..moov_start],
    ]
    .concat()
}

/// Items in the forms real files hold them, several of which a tag library
/// rewrites differently from how it read them.
fn odd_items() -> Vec<Vec<u8>> {
    const INTEGER: u32 = 21;
    const TEXT: u32 = 1;
    const UTF16: u32 = 2;
    let utf16: Vec<u8> = "warm up"
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    vec![
        atom(b"\xa9nam", &data(TEXT, 0x0000_0409, b"Localised title")),
        atom(
            b"\xa9ART",
            &data(TEXT, 0x0001_0000, b"Artist with a country"),
        ),
        atom(b"\xa9gen", &data(TEXT, 0, b"")),
        atom(b"rtng", &data(INTEGER, 0, &[2])),
        atom(b"stik", &data(INTEGER, 0, &[1])),
        atom(b"pgap", &data(INTEGER, 0, &[1])),
        atom(b"cpil", &data(INTEGER, 0, &[0])),
        // Four bytes where two are usual.
        atom(b"tmpo", &data(INTEGER, 0, &[0, 0, 0, 120])),
        atom(b"trkn", &data(0, 0, &[0, 0, 0, 3, 0, 12, 0, 0])),
        atom(b"disk", &data(0, 0, &[0, 0, 0, 1, 0, 2])),
        atom(b"\xa9cmt", &data(UTF16, 0, &utf16)),
        freeform(
            "com.apple.iTunes",
            "MULTIPLE",
            &[data(TEXT, 0, b"one"), data(TEXT, 0, b"two")].concat(),
        ),
        freeform("com.apple.iTunes", "INITIALKEY", &data(TEXT, 0, b"Cm")),
        freeform(
            "com.apple.iTunes",
            "iTunSMPB",
            &data(TEXT, 0, b" 00000000 00000840 000001C0"),
        ),
        atom(
            b"covr",
            &[
                data(14, 0, b"\x89PNG not really"),
                data(13, 0, b"\xFF\xD8\xFF\xE0 not really \xFF\xD9"),
            ]
            .concat(),
        ),
        atom(b"xyzw", &data(0, 0, &[1, 2, 3, 0xFF])),
        atom(b"purd", &data(TEXT, 0, b"2020-01-02 03:04:05")),
    ]
}

const TEMPO_INDEX: usize = 7;
const COMMENT_INDEX: usize = 10;
const KEY_INDEX: usize = 12;

fn assert_m4a_write_is_exact(name: &str, original: &[u8]) -> Vec<u8> {
    let path = scratch_file(name, original);
    let before = ilst_items(original);
    let original_samples = samples(&path);
    let tags = read_tags(&path).expect(WORKS);
    assert_eq!(tags.key.as_deref(), Some("Cm"));
    assert_eq!(tags.bpm.as_deref(), Some("120"));
    assert_eq!(tags.comment.as_deref(), Some("warm up"));

    let plan = plan_write(&path, &a_minor(), &full_options()).expect(WORKS);
    let planned: Vec<&str> = plan
        .changes()
        .iter()
        .map(|change| change.field.as_str())
        .collect();
    assert_eq!(
        planned,
        [
            "----:com.apple.iTunes:initialkey",
            "tmpo",
            "----:com.apple.iTunes:BPM",
            "©cmt"
        ]
    );
    let backup = apply_plan(&plan).expect(WORKS);

    let written = std::fs::read(&path).expect(WORKS);
    assert_only_changed(
        &before,
        &ilst_items(&written),
        &[TEMPO_INDEX, COMMENT_INDEX, KEY_INDEX],
        1,
    );
    let tags = read_tags(&path).expect(WORKS);
    assert_eq!(tags.key.as_deref(), Some("8A"));
    assert_eq!(tags.bpm.as_deref(), Some("127.5"));
    assert_eq!(tags.comment.as_deref(), Some("{8A} warm up"));
    assert!(samples(&path) == original_samples, "the audio changed");
    // The freeform item keeps the spelling of its name.
    assert!(
        ilst_items(&written)[KEY_INDEX]
            .windows(10)
            .any(|window| window == b"INITIALKEY")
    );

    undo(&backup).expect(WORKS);
    let restored = std::fs::read(&path).expect(WORKS);
    assert!(ilst_items(&restored) == before, "items differ after undo");
    assert!(
        restored == original,
        "undo did not restore the original bytes"
    );
    written
}

#[test]
fn m4a_items_outside_the_plan_keep_their_bytes() {
    let original = m4a_with_items(&odd_items());
    assert_eq!(ilst_items(&original), odd_items());
    assert_m4a_write_is_exact("odd-items.m4a", &original);
}

#[test]
fn m4a_with_moov_first_gets_its_chunk_offsets_moved() {
    let original = with_moov_first(&m4a_with_items(&odd_items()), 0);
    let written = assert_m4a_write_is_exact("moov-first.m4a", &original);
    assert_ne!(written.len(), original.len());
    let audio = find(&original, 0, original.len(), b"mdat");
    let moved_audio = find(&written, 0, written.len(), b"mdat");
    assert_ne!(moved_audio.0, audio.0);
    assert!(written[moved_audio.0..moved_audio.1] == original[audio.0..audio.1]);
}

#[test]
fn m4a_padding_after_moov_keeps_the_audio_in_place() {
    let original = with_moov_first(&m4a_with_items(&odd_items()), 4096);
    let written = assert_m4a_write_is_exact("moov-first-padded.m4a", &original);
    assert_eq!(written.len(), original.len());
    assert_eq!(
        find(&written, 0, written.len(), b"mdat"),
        find(&original, 0, original.len(), b"mdat")
    );
}

#[test]
fn m4a_padding_next_to_the_items_keeps_moov_the_same_length() {
    let mut original = m4a_with_items(&odd_items());
    let path = metadata_path(&original);
    let padding = atom(b"free", &[0; 2040]);
    for (start, end) in &path[..3] {
        let size = (end - start + padding.len()) as u32;
        original[*start..*start + 4].copy_from_slice(&size.to_be_bytes());
    }
    let after_items = path[3].1;
    original.splice(after_items..after_items, padding);

    let written = assert_m4a_write_is_exact("padded-items.m4a", &original);
    assert_eq!(written.len(), original.len());
    assert_eq!(metadata_path(&written)[0], metadata_path(&original)[0]);
}

#[test]
fn m4a_without_any_metadata_gets_it_and_loses_it_again() {
    let mut original = fixture_bytes(M4A);
    let [moov, udta, _, _] = metadata_path(&original);
    let size = (moov.1 - moov.0 - (udta.1 - udta.0)) as u32;
    original[moov.0..moov.0 + 4].copy_from_slice(&size.to_be_bytes());
    original.drain(udta.0..udta.1);
    let path = scratch_file("no-metadata.m4a", &original);
    let original_samples = samples(&path);
    assert_eq!(read_tags(&path).expect(WORKS).key, None);

    let backup =
        apply_plan(&plan_write(&path, &a_minor(), &full_options()).expect(WORKS)).expect(WORKS);
    assert_eq!(read_tags(&path).expect(WORKS).key.as_deref(), Some("8A"));
    assert_eq!(ilst_items(&std::fs::read(&path).expect(WORKS)).len(), 4);
    assert!(samples(&path) == original_samples);

    undo(&backup).expect(WORKS);
    let restored = std::fs::read(&path).expect(WORKS);
    assert!(ilst_items(&restored).is_empty());
    assert!(samples(&path) == original_samples);
}

// ID3v2

fn syncsafe(size: usize) -> [u8; 4] {
    [
        (size >> 21) as u8 & 0x7F,
        (size >> 14) as u8 & 0x7F,
        (size >> 7) as u8 & 0x7F,
        size as u8 & 0x7F,
    ]
}

fn frame(name: &str, content: &[u8]) -> Vec<u8> {
    let mut frame = name.as_bytes().to_vec();
    frame.extend((content.len() as u32).to_be_bytes());
    frame.extend([0, 0]);
    frame.extend(content);
    frame
}

/// Frames in the forms real files hold them: every text encoding,
/// terminators, an empty frame, ID3v2.3 dates, a frame name from ID3v2.4,
/// binary frames with bytes that look like MPEG audio, and several
/// comments.
fn odd_frames() -> Vec<Vec<u8>> {
    vec![
        frame("TIT2", b"\x00Latin title\x00"),
        frame("TPE1", b"\x01\xFE\xFF\x00B\x00i\x00g\x00 \x00e\x00n\x00d"),
        frame("TALB", b""),
        frame("TYER", b"\x001999"),
        frame("TDAT", b"\x000102"),
        frame("TSOP", b"\x00Sort, Name"),
        frame("TCON", b"\x00(17)\x00"),
        frame("TXXX", b"\x00replaygain_track_gain\x00-6.5 dB"),
        frame("COMM", b"\x00engiTunNORM\x00 000002A1 000002B7"),
        frame("COMM", b"\x00fra\x00warm up"),
        frame("TKEY", b"\x01\xFF\xFEC\x00m\x00"),
        frame(
            "APIC",
            b"\x00image/png\x00\x03\x00\xFF\xE0\xFF\xFB\x90\x00\xFF",
        ),
        frame("PRIV", b"TRAKTOR4\x00\xFF\xFF\x00\x00cue points"),
        frame(
            "GEOB",
            b"\x00application/octet-stream\x00\x00Serato Markers2\x00\x01\x01",
        ),
        frame("TPE2", b"\x03UTF-8 in an old tag"),
    ]
}

const PLAIN_COMMENT_INDEX: usize = 9;
const ID3_KEY_INDEX: usize = 10;

fn add_unsynchronisation(bytes: &[u8]) -> Vec<u8> {
    let mut safe = Vec::new();
    for (index, byte) in bytes.iter().enumerate() {
        safe.push(*byte);
        let next = bytes.get(index + 1);
        if *byte == 0xFF && next.is_none_or(|next| *next == 0 || *next >= 0xE0) {
            safe.push(0);
        }
    }
    safe
}

fn id3v23_tag(frames: &[Vec<u8>], padding: usize, unsynchronise: bool) -> Vec<u8> {
    let mut body = frames.concat();
    if unsynchronise {
        body = add_unsynchronisation(&body);
    }
    body.resize(body.len() + padding, 0);
    let mut tag = b"ID3\x03\x00".to_vec();
    tag.push(if unsynchronise { 0x80 } else { 0 });
    tag.extend(syncsafe(body.len()));
    tag.extend(body);
    tag
}

/// The frames of the ID3v2.3 tag at the start of `bytes`, and the tag's
/// whole length.
fn id3v23_frames(bytes: &[u8]) -> (Vec<Vec<u8>>, usize) {
    assert_eq!(&bytes[..4], b"ID3\x03");
    let size = bytes[6..10]
        .iter()
        .fold(0, |size, byte| (size << 7) | usize::from(*byte));
    let mut body = bytes[10..10 + size].to_vec();
    if bytes[5] & 0x80 != 0 {
        let mut plain = Vec::new();
        let mut after_ff = false;
        for byte in body {
            if !(after_ff && byte == 0) {
                plain.push(byte);
            }
            after_ff = byte == 0xFF && !(after_ff && byte == 0);
        }
        body = plain;
    }
    let mut frames = Vec::new();
    let mut offset = 0;
    while offset < body.len() && body[offset] != 0 {
        let end = offset + 10 + big_endian_u32(&body, offset + 4);
        frames.push(body[offset..end].to_vec());
        offset = end;
    }
    assert!(body[offset..].iter().all(|byte| *byte == 0), "bad padding");
    (frames, 10 + size)
}

fn mp3_audio() -> Vec<u8> {
    let fixture = fixture_bytes(MP3);
    let size = fixture[6..10]
        .iter()
        .fold(0, |size, byte| (size << 7) | usize::from(*byte));
    fixture[10 + size..].to_vec()
}

fn assert_id3_frames_are_exact(before: &[Vec<u8>], after: &[Vec<u8>]) {
    assert_only_changed(before, after, &[PLAIN_COMMENT_INDEX, ID3_KEY_INDEX], 1);
    // The comment keeps its language and its Latin-1 encoding.
    assert_eq!(
        after[PLAIN_COMMENT_INDEX],
        frame("COMM", b"\x00fra\x00{8A} warm up")
    );
    // The key keeps its UTF-16 encoding.
    assert_eq!(
        after[ID3_KEY_INDEX],
        frame("TKEY", b"\x01\xFF\xFE8\x00A\x00")
    );
    assert_eq!(after[before.len()], frame("TBPM", b"\x00127.5"));
}

#[test]
fn mp3_frames_outside_the_plan_keep_their_bytes() {
    let audio = mp3_audio();
    for (name, padding, unsynchronise) in [
        ("padded", 300, false),
        ("unpadded", 0, false),
        ("unsynchronised", 300, true),
    ] {
        let tag = id3v23_tag(&odd_frames(), padding, unsynchronise);
        let original = [&tag[..], &audio[..]].concat();
        assert_eq!(id3v23_frames(&original).0, odd_frames(), "{name}");
        let path = scratch_file(&format!("odd-frames-{name}.mp3"), &original);
        let tags = read_tags(&path).expect(WORKS);
        assert_eq!(tags.title.as_deref(), Some("Latin title"), "{name}");
        assert_eq!(tags.artist.as_deref(), Some("Big end"), "{name}");
        assert_eq!(tags.key.as_deref(), Some("Cm"), "{name}");
        assert_eq!(tags.comment.as_deref(), Some("warm up"), "{name}");

        let written = write_then_undo(&path, &original);
        let (frames, tag_length) = id3v23_frames(&written);
        assert_id3_frames_are_exact(&odd_frames(), &frames);
        assert!(
            written[tag_length..] == audio[..],
            "{name}: the audio changed"
        );
        assert_eq!(
            written[..6],
            original[..6],
            "{name}: the tag header changed"
        );
        if padding > 0 {
            assert_eq!(
                written.len(),
                original.len(),
                "{name}: padding was not used"
            );
        }
    }
}

#[test]
fn version_4_tag_with_a_footer_keeps_its_frames() {
    let key = [b"TKEY".as_slice(), &syncsafe(3), b"\x00\x00\x00Cm"].concat();
    let binary_content = [0xFF; 200];
    let binary = [
        b"PRIV".as_slice(),
        &syncsafe(200),
        b"\x00\x00",
        &binary_content,
    ]
    .concat();
    let body = [&binary[..], &key[..]].concat();
    let header = [b"ID3\x04\x00\x10".as_slice(), &syncsafe(body.len())].concat();
    let footer = [b"3DI\x04\x00\x10".as_slice(), &syncsafe(body.len())].concat();
    let original = [&header[..], &body[..], &footer[..], &mp3_audio()[..]].concat();
    let path = scratch_file("footer.mp3", &original);

    let written = write_then_undo(&path, &original);
    assert!(written.starts_with(b"ID3\x04\x00\x10"));
    assert!(written[10..].starts_with(&binary));
    let new_key = [b"TKEY".as_slice(), &syncsafe(3), b"\x00\x00\x008A"].concat();
    assert!(written[10 + binary.len()..].starts_with(&new_key));
    assert!(
        written
            .windows(6)
            .any(|window| window == b"3DI\x04\x00\x10")
    );
    assert!(written.ends_with(&mp3_audio()));
}

#[test]
fn wav_and_aiff_chunks_outside_the_tag_keep_their_bytes() {
    // An odd tag length makes the chunk need a padding byte.
    let frames_length = odd_frames().concat().len();
    let mut tag = id3v23_tag(&odd_frames(), 301 - frames_length % 2, false);
    assert_eq!(tag.len() % 2, 1);
    let tag_length = tag.len() as u32;
    tag.push(0);
    for fixture in [WAV, AIFF] {
        let mut original = fixture_bytes(fixture);
        let container_size = (original.len() - 8 + 8 + tag.len()) as u32;
        // Lower case, as some programs name the chunk.
        original.extend(b"id3 ");
        if fixture == WAV {
            original.extend(tag_length.to_le_bytes());
            original[4..8].copy_from_slice(&container_size.to_le_bytes());
        } else {
            original.extend(tag_length.to_be_bytes());
            original[4..8].copy_from_slice(&container_size.to_be_bytes());
        }
        let chunk_start = original.len() - 8;
        original.extend(&tag);
        let path = scratch_file(&format!("odd-frames-{fixture}"), &original);
        let original_samples = samples(&path);

        let plan = plan_write(&path, &a_minor(), &full_options()).expect(WORKS);
        let backup = apply_plan(&plan).expect(WORKS);
        let written = std::fs::read(&path).expect(WORKS);
        assert_eq!(written.len(), original.len(), "{fixture}");
        assert!(
            written[..chunk_start] == original[..chunk_start],
            "{fixture}"
        );
        assert_eq!(&written[chunk_start..chunk_start + 4], b"id3 ", "{fixture}");
        let (frames, _) = id3v23_frames(&written[chunk_start + 8..]);
        assert_id3_frames_are_exact(&odd_frames(), &frames);
        assert!(samples(&path) == original_samples, "{fixture}");

        undo(&backup).expect(WORKS);
        assert!(std::fs::read(&path).expect(WORKS) == original, "{fixture}");
    }
}

#[test]
fn files_without_a_tag_are_restored_to_their_exact_bytes() {
    for fixture in [WAV, AIFF, FLAC, M4A] {
        let original = fixture_bytes(fixture);
        let path = scratch_file(&format!("bare-{fixture}"), &original);
        write_then_undo(&path, &original);
    }
    let original = mp3_audio();
    let path = scratch_file("bare.mp3", &original);
    let written = write_then_undo(&path, &original);
    assert!(written.ends_with(&original));
    assert!(written.starts_with(b"ID3\x03\x00\x00"));
}

// FLAC

fn flac_block(kind: u8, content: &[u8]) -> Vec<u8> {
    let mut block = vec![kind];
    block.extend(&(content.len() as u32).to_be_bytes()[1..]);
    block.extend(content);
    block
}

/// The content of a PICTURE block around `image`.
fn flac_picture(picture_type: u32, image: &[u8]) -> Vec<u8> {
    let mime = b"image/png";
    let mut content = picture_type.to_be_bytes().to_vec();
    content.extend((mime.len() as u32).to_be_bytes());
    content.extend(mime);
    content.extend(5_u32.to_be_bytes());
    content.extend(b"cover");
    for dimension in [1_u32, 1, 32, 0] {
        content.extend(dimension.to_be_bytes());
    }
    content.extend((image.len() as u32).to_be_bytes());
    content.extend(image);
    content
}

fn vorbis_comments(vendor: &str, comments: &[&str]) -> Vec<u8> {
    let mut content = (vendor.len() as u32).to_le_bytes().to_vec();
    content.extend(vendor.as_bytes());
    content.extend((comments.len() as u32).to_le_bytes());
    for comment in comments {
        content.extend((comment.len() as u32).to_le_bytes());
        content.extend(comment.as_bytes());
    }
    content
}

/// The metadata blocks of `bytes` as kind and content, and the offset
/// where the audio starts.
fn flac_blocks(bytes: &[u8]) -> (Vec<(u8, Vec<u8>)>, usize) {
    assert_eq!(&bytes[..4], b"fLaC");
    let mut blocks = Vec::new();
    let mut offset = 4;
    loop {
        let is_last = bytes[offset] & 0x80 != 0;
        let size = big_endian_u32(bytes, offset) & 0x00FF_FFFF;
        let content = bytes[offset + 4..offset + 4 + size].to_vec();
        blocks.push((bytes[offset] & 0x7F, content));
        offset += 4 + size;
        if is_last {
            return (blocks, offset);
        }
    }
}

fn comment_strings(block: &[u8]) -> (String, Vec<String>) {
    let little_endian = |offset: usize| {
        let word: [u8; 4] = block[offset..offset + 4].try_into().expect(WORKS);
        u32::from_le_bytes(word) as usize
    };
    let text = |start: usize, length: usize| {
        String::from_utf8(block[start..start + length].to_vec()).expect(WORKS)
    };
    let vendor_length = little_endian(0);
    let mut offset = 4 + vendor_length;
    let count = little_endian(offset);
    offset += 4;
    let mut comments = Vec::new();
    for _ in 0..count {
        let length = little_endian(offset);
        comments.push(text(offset + 4, length));
        offset += 4 + length;
    }
    assert_eq!(offset, block.len());
    (text(4, vendor_length), comments)
}

const ODD_COMMENTS: [&str; 9] = [
    "TITLE=Tone",
    "ARTIST=One",
    "ARTIST=Two",
    "initialkey=Cm",
    "Comment=warm up",
    "EMPTY=",
    "Bpm=120.00",
    "no equals sign",
    "REPLAYGAIN_TRACK_GAIN=-6.50 dB",
];

fn odd_flac(padding: Option<usize>) -> Vec<u8> {
    let fixture = fixture_bytes(FLAC);
    let (fixture_blocks, audio_start) = flac_blocks(&fixture);
    let (_, stream_info) = &fixture_blocks[0];
    let mut bytes = b"fLaC".to_vec();
    bytes.extend(flac_block(0, stream_info));
    bytes.extend(flac_block(2, b"testapplication data \xFF\xF8"));
    bytes.extend(flac_block(
        6,
        &flac_picture(3, b"\x89PNG front \xFF\xF8\xFF"),
    ));
    bytes.extend(flac_block(
        4,
        &vorbis_comments("reference libFLAC 1.4.3", &ODD_COMMENTS),
    ));
    let back_cover = flac_picture(4, b"\x89PNG back");
    bytes.extend(flac_block(6, &back_cover));
    if let Some(padding) = padding {
        bytes.extend(flac_block(1, &vec![0; padding]));
    }
    let last_header = bytes.len() - 4 - padding.unwrap_or(back_cover.len());
    bytes[last_header] |= 0x80;
    bytes.extend(&fixture[audio_start..]);
    bytes
}

#[test]
fn flac_comments_and_blocks_outside_the_plan_keep_their_bytes() {
    for padding in [Some(500), Some(4), None] {
        let original = odd_flac(padding);
        let (before, audio_start) = flac_blocks(&original);
        let path = scratch_file(&format!("odd-{padding:?}.flac"), &original);
        let original_samples = samples(&path);
        let tags = read_tags(&path).expect(WORKS);
        assert_eq!(tags.key.as_deref(), Some("Cm"));
        assert_eq!(tags.artist.as_deref(), Some("One"));

        let written = write_then_undo(&path, &original);
        let (after, written_audio_start) = flac_blocks(&written);
        let kinds = |blocks: &[(u8, Vec<u8>)]| -> Vec<u8> {
            blocks.iter().map(|(kind, _)| *kind).collect()
        };
        assert_eq!(kinds(&after), kinds(&before), "{padding:?}");
        for index in [0, 1, 2, 4] {
            assert!(after[index] == before[index], "{padding:?}: block {index}");
        }
        assert!(written[written_audio_start..] == original[audio_start..]);
        if padding == Some(500) {
            assert_eq!(written.len(), original.len());
        }

        let (vendor, comments) = comment_strings(&after[3].1);
        assert_eq!(vendor, "reference libFLAC 1.4.3");
        assert_eq!(
            comments,
            [
                "TITLE=Tone",
                "ARTIST=One",
                "ARTIST=Two",
                "initialkey=8A",
                "Comment={8A} warm up",
                "EMPTY=",
                "Bpm=127.5",
                "no equals sign",
                "REPLAYGAIN_TRACK_GAIN=-6.50 dB",
                "KEY=8A",
            ],
            "{padding:?}"
        );

        std::fs::write(&path, &written).expect(WORKS);
        assert!(samples(&path) == original_samples, "{padding:?}");
    }
}

#[test]
fn wav_whose_last_chunk_lacks_its_padding_byte_is_still_written() {
    let mut original = fixture_bytes(WAV);
    original.pop();
    let data_size_at = original
        .windows(4)
        .position(|window| window == b"data")
        .expect("a data chunk")
        + 4;
    let data_size = (original.len() - data_size_at - 4) as u32;
    assert_eq!(data_size % 2, 1);
    original[data_size_at..data_size_at + 4].copy_from_slice(&data_size.to_le_bytes());
    let container_size = (original.len() - 8) as u32;
    original[4..8].copy_from_slice(&container_size.to_le_bytes());
    let path = scratch_file("unpadded-last-chunk.wav", &original);
    let original_samples = samples(&path);

    let plan = plan_write(&path, &a_minor(), &full_options()).expect(WORKS);
    let backup = apply_plan(&plan).expect(WORKS);
    let written = std::fs::read(&path).expect(WORKS);
    assert!(written[8..original.len()] == original[8..]);
    assert_eq!(written[original.len()], 0, "the missing padding byte");
    assert_eq!(&written[original.len() + 1..original.len() + 5], b"ID3 ");
    assert_eq!(read_tags(&path).expect(WORKS).key.as_deref(), Some("8A"));
    assert!(samples(&path) == original_samples);

    undo(&backup).expect(WORKS);
    let restored = std::fs::read(&path).expect(WORKS);
    assert!(restored[8..original.len()] == original[8..]);
    assert_eq!(read_tags(&path).expect(WORKS).key, None);
    assert!(samples(&path) == original_samples);
}
