// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tag writes to Ogg Vorbis files, checked with an Ogg reader and writer
//! written here that share nothing with the library. The files are built
//! page by page from the fixture's packets in the layouts encoders use, and
//! with the kinds of comments tag libraries tend to normalise. Every page
//! must still match its checksum and follow on from the one before, the
//! audio pages must keep every byte, and undo must give back the original
//! file byte for byte.

use std::path::{Path, PathBuf};

use euphoria_core::detect::Analysis;
use euphoria_core::tags::{
    DjSoftware, KeyNotation, LabelPlacement, TagWriteOptions, apply_plan, plan_write, read_tags,
    undo,
};
use euphoria_core::{Key, Mode, decode_mono};

const OGG: &str = "sine-440hz-mono.ogg";
const WORKS: &str = "the scratch file can be read and written";
const SERIAL_NUMBER: u32 = 0x2026_1111;

fn fixture_bytes(fixture: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(fixture);
    std::fs::read(path).expect("the fixture can be read")
}

fn scratch_file(name: &str, bytes: &[u8]) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("ogg-{name}.ogg"));
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
        label: LabelPlacement::CommentStart,
        preset: DjSoftware::Mixxx,
    }
}

/// The page checksum, bit by bit as the Ogg specification describes it.
fn checksum(bytes: &[u8]) -> u32 {
    let mut crc = 0_u32;
    for byte in bytes {
        crc ^= u32::from(*byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x04C1_1DB7
            };
        }
    }
    crc
}

/// One page as this test reads it.
#[derive(Debug, Clone)]
struct Page {
    flags: u8,
    granule_position: u64,
    serial_number: u32,
    sequence_number: u32,
    lacing: Vec<u8>,
    data: Vec<u8>,
    bytes: Vec<u8>,
}

fn pages(bytes: &[u8]) -> Vec<Page> {
    let mut offset = 0;
    let mut found = Vec::new();
    while offset < bytes.len() {
        assert_eq!(&bytes[offset..offset + 4], b"OggS", "page at {offset}");
        let word = |at: usize| {
            u32::from_le_bytes(bytes[offset + at..offset + at + 4].try_into().expect(WORKS))
        };
        let segments = usize::from(bytes[offset + 26]);
        let lacing = bytes[offset + 27..offset + 27 + segments].to_vec();
        let length: usize = lacing.iter().map(|value| usize::from(*value)).sum();
        let data_start = offset + 27 + segments;
        let page_bytes = bytes[offset..data_start + length].to_vec();
        found.push(Page {
            flags: bytes[offset + 5],
            granule_position: u64::from(word(6)) | (u64::from(word(10)) << 32),
            serial_number: word(14),
            sequence_number: word(18),
            data: bytes[data_start..data_start + length].to_vec(),
            lacing,
            bytes: page_bytes,
        });
        offset = data_start + length;
    }
    found
}

/// Every page matches its checksum and its sequence number follows on.
fn assert_pages_are_sound(bytes: &[u8], context: &str) {
    let all = pages(bytes);
    for (index, page) in all.iter().enumerate() {
        let mut zeroed = page.bytes.clone();
        zeroed[22..26].fill(0);
        let stored = u32::from_le_bytes(page.bytes[22..26].try_into().expect(WORKS));
        assert_eq!(
            checksum(&zeroed),
            stored,
            "{context}: page {index} checksum"
        );
        assert_eq!(
            page.sequence_number, index as u32,
            "{context}: page {index}"
        );
        let continues = index > 0 && all[index - 1].lacing.last() == Some(&255);
        assert_eq!(
            page.flags & 1 != 0,
            continues,
            "{context}: page {index} continuation flag"
        );
    }
}

/// The packets of the stream, in order.
fn packets(all: &[Page]) -> Vec<Vec<u8>> {
    let mut found = Vec::new();
    let mut packet = Vec::new();
    for page in all {
        let mut position = 0;
        for value in &page.lacing {
            let end = position + usize::from(*value);
            packet.extend(&page.data[position..end]);
            position = end;
            if *value < 255 {
                found.push(std::mem::take(&mut packet));
            }
        }
    }
    found
}

fn page_bytes(
    flags: u8,
    granule_position: u64,
    sequence_number: u32,
    lacing: &[u8],
    data: &[u8],
) -> Vec<u8> {
    let mut bytes = b"OggS\x00".to_vec();
    bytes.push(flags);
    bytes.extend(granule_position.to_le_bytes());
    bytes.extend(SERIAL_NUMBER.to_le_bytes());
    bytes.extend(sequence_number.to_le_bytes());
    bytes.extend([0; 4]);
    bytes.push(lacing.len() as u8);
    bytes.extend(lacing);
    bytes.extend(data);
    let crc = checksum(&bytes);
    bytes[22..26].copy_from_slice(&crc.to_le_bytes());
    bytes
}

fn lacing(length: usize) -> Vec<u8> {
    let mut values = vec![255; length / 255];
    values.push((length % 255) as u8);
    values
}

/// A comment header with these comments and `after` following the
/// framing bit.
fn comment_header(vendor: &str, comments: &[&[u8]], after: &[u8]) -> Vec<u8> {
    let mut packet = b"\x03vorbis".to_vec();
    packet.extend((vendor.len() as u32).to_le_bytes());
    packet.extend(vendor.as_bytes());
    packet.extend((comments.len() as u32).to_le_bytes());
    for comment in comments {
        packet.extend((comment.len() as u32).to_le_bytes());
        packet.extend(*comment);
    }
    packet.push(1);
    packet.extend(after);
    packet
}

/// An Ogg Vorbis file with the fixture's identification header, setup
/// header and audio, and `comment` as its comment header. The comment and
/// setup headers are cut into pages of at most `page_segments` lacing
/// values, starting the setup header on a new page when `setup_alone`.
/// The audio pages are numbered to follow.
fn ogg_file(comment: &[u8], page_segments: usize, setup_alone: bool) -> Vec<u8> {
    let fixture = pages(&fixture_bytes(OGG));
    let fixture_packets = packets(&fixture);
    let header_pages = {
        let mut count = 0;
        let mut ended = 0;
        for page in &fixture {
            count += 1;
            ended += page.lacing.iter().filter(|value| **value < 255).count();
            if ended >= 3 {
                break;
            }
        }
        count
    };

    let mut bytes = page_bytes(
        0x02,
        0,
        0,
        &lacing(fixture_packets[0].len()),
        &fixture_packets[0],
    );
    let mut groups: Vec<(Vec<u8>, Vec<u8>)> = vec![(lacing(comment.len()), comment.to_vec())];
    let setup = (lacing(fixture_packets[2].len()), fixture_packets[2].clone());
    if setup_alone {
        groups.push(setup);
    } else {
        groups[0].0.extend(setup.0);
        groups[0].1.extend(setup.1);
    }
    let mut sequence_number = 1;
    for (values, data) in groups {
        let mut value_start = 0;
        let mut data_start = 0;
        let mut continues = false;
        while value_start < values.len() {
            let end = (value_start + page_segments).min(values.len());
            let page_values = &values[value_start..end];
            let length: usize = page_values.iter().map(|value| usize::from(*value)).sum();
            let ends_a_packet = page_values.iter().any(|value| *value < 255);
            bytes.extend(page_bytes(
                u8::from(continues),
                if ends_a_packet { 0 } else { u64::MAX },
                sequence_number,
                page_values,
                &data[data_start..data_start + length],
            ));
            continues = page_values.last() == Some(&255);
            sequence_number += 1;
            value_start = end;
            data_start += length;
        }
    }
    for page in &fixture[header_pages..] {
        bytes.extend(page_bytes(
            page.flags,
            page.granule_position,
            sequence_number,
            &page.lacing,
            &page.data,
        ));
        sequence_number += 1;
    }
    bytes
}

/// The bytes of the pages after the comment and setup headers.
fn audio_pages(bytes: &[u8]) -> Vec<Vec<u8>> {
    let all = pages(bytes);
    let mut ended = 0;
    let mut audio = Vec::new();
    for page in all {
        if ended >= 3 {
            audio.push(page.bytes.clone());
        }
        ended += page.lacing.iter().filter(|value| **value < 255).count();
    }
    audio
}

/// The comments of the comment header, each as its raw bytes.
fn comments(bytes: &[u8]) -> (Vec<u8>, Vec<Vec<u8>>, Vec<u8>) {
    let packet = &packets(&pages(bytes))[1];
    assert_eq!(&packet[..7], b"\x03vorbis");
    let word = |at: usize| u32::from_le_bytes(packet[at..at + 4].try_into().expect(WORKS)) as usize;
    let vendor_length = word(7);
    let vendor = packet[11..11 + vendor_length].to_vec();
    let mut offset = 11 + vendor_length;
    let count = word(offset);
    offset += 4;
    let mut found = Vec::new();
    for _ in 0..count {
        let length = word(offset);
        found.push(packet[offset + 4..offset + 4 + length].to_vec());
        offset += 4 + length;
    }
    (vendor, found, packet[offset..].to_vec())
}

const ODD_COMMENTS: [&[u8]; 7] = [
    b"title=Lower case name",
    b"ARTIST=First artist",
    b"ARTIST=Second artist",
    b"EMPTY=",
    b"SERATO_MARKERS_V2=AQFDT0xPUgAAAAAEAP///w",
    b"DESCRIPTION=Caf\xc3\xa9 \xc2\xb7 not plain ASCII",
    b"comment=already here",
];

/// Writes, undoes and checks the file built from `comment`.
fn assert_write_is_exact(name: &str, original: &[u8]) {
    let path = scratch_file(name, original);
    assert_pages_are_sound(original, name);
    let samples = decode_mono(&path).expect(WORKS).samples().to_vec();
    let (vendor, before, after_comments) = comments(original);

    let plan = plan_write(&path, &a_minor(), &full_options()).expect(WORKS);
    let backup = apply_plan(&plan).expect(WORKS);
    let written = std::fs::read(&path).expect(WORKS);
    assert_pages_are_sound(&written, name);
    assert_eq!(
        pages(&written).len(),
        pages(original).len(),
        "{name}: page count"
    );
    assert!(
        audio_pages(&written) == audio_pages(original),
        "{name}: the audio pages changed"
    );
    assert!(
        decode_mono(&path).expect(WORKS).samples() == samples.as_slice(),
        "{name}: samples"
    );
    let (written_vendor, written_comments, written_after) = comments(&written);
    assert_eq!(written_vendor, vendor, "{name}");
    assert_eq!(written_after, after_comments, "{name}");
    let untouched: Vec<&Vec<u8>> = written_comments
        .iter()
        .filter(|comment| {
            let name = comment
                .split(|byte| *byte == b'=')
                .next()
                .unwrap_or_default();
            !["INITIALKEY", "BPM", "COMMENT"]
                .iter()
                .any(|field| name.eq_ignore_ascii_case(field.as_bytes()))
        })
        .collect();
    let expected: Vec<&Vec<u8>> = before
        .iter()
        .filter(|comment| !comment.to_ascii_uppercase().starts_with(b"COMMENT="))
        .collect();
    assert_eq!(untouched, expected, "{name}: other comments");
    let tags = read_tags(&path).expect(WORKS);
    assert_eq!(tags.key.as_deref(), Some("8A"), "{name}");
    assert_eq!(tags.bpm.as_deref(), Some("127.5"), "{name}");

    undo(&backup).expect(WORKS);
    assert!(
        std::fs::read(&path).expect(WORKS) == original,
        "{name}: undo did not restore the original bytes"
    );
}

#[test]
fn comment_and_setup_on_one_page_as_oggenc_writes_them() {
    let comment = comment_header("Xiph.Org libVorbis I 20200704", &ODD_COMMENTS, b"");
    assert_write_is_exact("shared-page", &ogg_file(&comment, 255, false));
}

#[test]
fn comment_and_setup_on_pages_of_their_own_as_ffmpeg_writes_them() {
    let comment = comment_header("Lavf63.1.102", &ODD_COMMENTS, b"");
    assert_write_is_exact("own-pages", &ogg_file(&comment, 255, true));
}

#[test]
fn comment_header_spread_over_many_small_pages() {
    let comment = comment_header("small pages", &ODD_COMMENTS, b"");
    assert_write_is_exact("small-pages", &ogg_file(&comment, 3, false));
}

#[test]
fn comment_header_larger_than_a_page_as_artwork_makes_it() {
    let artwork = [b"METADATA_BLOCK_PICTURE=".to_vec(), vec![b'A'; 90_000]].concat();
    let comment = comment_header("artwork", &[&artwork, b"TITLE=With artwork"], b"");
    let file = ogg_file(&comment, 255, true);
    assert!(pages(&file).len() > 4, "the comments span several pages");
    assert_write_is_exact("artwork", &file);
}

#[test]
fn bytes_after_the_framing_bit_are_kept() {
    let comment = comment_header("padded", &[b"TITLE=Padded"], &[0; 40]);
    assert_write_is_exact("after-framing", &ogg_file(&comment, 255, false));
}

#[test]
fn file_without_comments_gets_them_and_loses_them_again() {
    let comment = comment_header("", &[], b"");
    assert_write_is_exact("no-comments", &ogg_file(&comment, 255, false));
}

#[test]
fn fixture_made_by_oggenc_is_written_and_restored_exactly() {
    assert_write_is_exact("fixture", &fixture_bytes(OGG));
}

/// Planning refuses the file, so the refusal shows before anything is
/// written.
fn assert_refused(name: &str, bytes: &[u8], reason: &str) {
    let path = scratch_file(name, bytes);
    let error = plan_write(&path, &a_minor(), &full_options()).expect_err("the file is refused");
    assert!(error.chain().contains(reason), "{name}: {}", error.chain());
    assert!(std::fs::read(&path).expect(WORKS) == bytes, "{name}");
}

#[test]
fn header_page_that_does_not_match_its_checksum_is_read_but_not_rewritten() {
    let comment = comment_header("damaged", &[b"TITLE=Damaged page"], b"");
    let mut bytes = ogg_file(&comment, 255, false);
    let all = pages(&bytes);
    let comment_data = all[0].bytes.len() + 27 + all[1].lacing.len();
    // A letter of the vendor text, which the checksum covers.
    bytes[comment_data + 7 + 4 + 2] ^= 0x01;
    let path = scratch_file("bad-checksum", &bytes);
    assert_eq!(
        read_tags(&path).expect(WORKS).title.as_deref(),
        Some("Damaged page")
    );
    assert_refused("bad-checksum", &bytes, "checksum");
}

#[test]
fn header_sequence_starts_at_zero_and_includes_the_first_transition() {
    let comment = comment_header("sequence", &[b"TITLE=Sequence"], b"");
    let file = ogg_file(&comment, 255, false);
    let control = scratch_file("sequence-control", &file);
    plan_write(&control, &a_minor(), &full_options()).expect(WORKS);
    for changed_page in [0, 1] {
        let mut bytes = Vec::new();
        for (index, page) in pages(&file).iter().enumerate() {
            bytes.extend(page_bytes(
                page.flags,
                page.granule_position,
                if index == changed_page {
                    7
                } else {
                    page.sequence_number
                },
                &page.lacing,
                &page.data,
            ));
        }
        let name = format!("bad-sequence-{changed_page}");
        let path = scratch_file(&name, &bytes);
        assert_eq!(
            read_tags(&path).expect(WORKS).title.as_deref(),
            Some("Sequence")
        );
        assert_refused(&name, &bytes, "out of order");
    }
}

#[test]
fn comments_that_would_need_more_pages_are_refused() {
    // The comments end on a full page with the setup header after them, so
    // there is no room for another lacing value without renumbering the
    // audio pages.
    let filler = [b"FILLER=".to_vec(), vec![b'x'; 255 * 254 - 30]].concat();
    let mut length = 255 * 255 - 1;
    let comment = loop {
        let candidate = comment_header("full", &[&filler], &vec![0; length]);
        if lacing(candidate.len()).len() == 255 {
            break candidate;
        }
        length -= 1;
    };
    let bytes = ogg_file(&comment, 255, true);
    assert_eq!(pages(&bytes)[1].lacing.len(), 255);
    assert_refused("full-page", &bytes, "do not fit");
}

#[test]
fn setup_header_sharing_a_page_with_audio_is_refused() {
    let comment = comment_header("shared", &[b"TITLE=Shared"], b"");
    let file = ogg_file(&comment, 255, false);
    let all = pages(&file);
    // Join the first audio page onto the page that ends the setup header.
    let mut bytes = all[0].bytes.clone();
    let mut lacing = all[1].lacing.clone();
    lacing.extend(&all[2].lacing);
    let data = [all[1].data.clone(), all[2].data.clone()].concat();
    bytes.extend(page_bytes(0, all[2].granule_position, 1, &lacing, &data));
    for (index, page) in all[3..].iter().enumerate() {
        bytes.extend(page_bytes(
            page.flags,
            page.granule_position,
            index as u32 + 2,
            &page.lacing,
            &page.data,
        ));
    }
    assert_refused("shared-audio", &bytes, "shares a page");
}

#[test]
fn empty_packet_sharing_a_page_with_the_setup_header_is_refused() {
    let comment = comment_header("shared-empty", &[b"TITLE=Shared"], b"");
    let file = ogg_file(&comment, 255, false);
    let all = pages(&file);
    let mut bytes = all[0].bytes.clone();
    let mut lacing = all[1].lacing.clone();
    // An empty packet takes a lacing value but no data bytes. It must not
    // bypass the same-page audio guard or make previewing the write panic.
    lacing.push(0);
    bytes.extend(page_bytes(0, 0, 1, &lacing, &all[1].data));
    for page in &all[2..] {
        bytes.extend(&page.bytes);
    }
    assert_refused("shared-empty-packet", &bytes, "shares a page");
}

#[test]
fn stream_that_is_not_vorbis_is_not_read() {
    let mut opus_head = b"OpusHead".to_vec();
    opus_head.extend([1, 1, 0x38, 0x01, 0x80, 0xBB, 0, 0, 0, 0, 0]);
    let mut bytes = page_bytes(0x02, 0, 0, &lacing(opus_head.len()), &opus_head);
    let tags = b"OpusTags\x00\x00\x00\x00\x00\x00\x00\x00";
    bytes.extend(page_bytes(0, 0, 1, &lacing(tags.len()), tags));
    bytes.extend(page_bytes(0x04, 0, 2, &[1], &[0]));
    let path = scratch_file("opus", &bytes);
    let error = read_tags(&path).unwrap_err();
    assert!(error.chain().contains("Vorbis"), "{}", error.chain());
}

#[test]
fn second_stream_among_the_header_pages_is_not_read() {
    let comment = comment_header("multiplexed", &[b"TITLE=Two streams"], b"");
    let file = ogg_file(&comment, 255, false);
    let all = pages(&file);
    let mut bytes = all[0].bytes.clone();
    let mut other = page_bytes(0x02, 0, 0, &[4], b"skel");
    other[14..18].copy_from_slice(&7_u32.to_le_bytes());
    other[22..26].fill(0);
    let crc = checksum(&other);
    other[22..26].copy_from_slice(&crc.to_le_bytes());
    bytes.extend(other);
    for page in &all[1..] {
        bytes.extend(&page.bytes);
    }
    let path = scratch_file("multiplexed", &bytes);
    let error = read_tags(&path).unwrap_err();
    assert!(
        error.chain().contains("more than one stream"),
        "{}",
        error.chain()
    );
    assert_eq!(all[0].serial_number, SERIAL_NUMBER);
}

/// `bytes` with every page renumbered by `number`, which gets the page's
/// index, and its checksum computed again.
fn renumbered(bytes: &[u8], number: impl Fn(usize) -> u32) -> Vec<u8> {
    let mut output = Vec::new();
    for (index, page) in pages(bytes).iter().enumerate() {
        let mut page_bytes = page.bytes.clone();
        page_bytes[18..22].copy_from_slice(&number(index).to_le_bytes());
        page_bytes[22..26].fill(0);
        let crc = checksum(&page_bytes);
        page_bytes[22..26].copy_from_slice(&crc.to_le_bytes());
        output.extend(page_bytes);
    }
    output
}

#[test]
fn header_pages_out_of_order_are_read_but_not_rewritten() {
    let comment = comment_header("numbered", &[b"TITLE=Numbered"], b"");
    let file = ogg_file(&comment, 255, true);
    let cases = [
        (
            "first-page-not-zero",
            renumbered(&file, |index| index as u32 + 1),
        ),
        (
            "gap-after-first-page",
            renumbered(&file, |index| if index == 0 { 0 } else { index as u32 + 1 }),
        ),
    ];
    for (name, bytes) in cases {
        let path = scratch_file(name, &bytes);
        assert_eq!(
            read_tags(&path).expect(WORKS).title.as_deref(),
            Some("Numbered"),
            "{name}"
        );
        assert_refused(name, &bytes, "out of order");
    }
}

#[test]
fn second_stream_after_the_first_is_read_but_not_rewritten() {
    let comment = comment_header("chained", &[b"TITLE=Chained"], b"");
    let mut bytes = ogg_file(&comment, 255, false);
    let mut other = page_bytes(0x02, 0, 0, &[4], b"next");
    other[14..18].copy_from_slice(&9_u32.to_le_bytes());
    other[22..26].fill(0);
    let crc = checksum(&other);
    other[22..26].copy_from_slice(&crc.to_le_bytes());
    bytes.extend(other);
    let path = scratch_file("chained", &bytes);
    assert_eq!(
        read_tags(&path).expect(WORKS).title.as_deref(),
        Some("Chained")
    );
    assert_refused("chained", &bytes, "more than one stream");
}

#[test]
fn data_that_is_not_ogg_after_the_pages_is_refused() {
    let comment = comment_header("trailing", &[b"TITLE=Trailing"], b"");
    let mut bytes = ogg_file(&comment, 255, false);
    bytes.extend(b"TAG".iter().chain([0_u8; 125].iter()));
    assert_refused("trailing", &bytes, "other than Ogg pages");
}

#[test]
fn a_header_packet_that_never_ends_is_refused_before_it_fills_memory() {
    // The comment header continues from page to page and never ends,
    // for more than 64 MiB.
    let mut bytes = ogg_file(&comment_header("short", &[], b""), 255, false);
    bytes.truncate(pages(&bytes)[0].bytes.len());
    let data = vec![b'x'; 255 * 255];
    let mut packet_start = b"\x03vorbis".to_vec();
    packet_start.extend(&data[packet_start.len()..]);
    for index in 0..1100_u32 {
        let flags = u8::from(index > 0);
        let page = if index == 0 { &packet_start } else { &data };
        bytes.extend(page_bytes(flags, u64::MAX, index + 1, &[255; 255], page));
    }
    let path = scratch_file("endless", &bytes);
    let error = read_tags(&path).expect_err("the file is refused");
    assert!(
        error.chain().contains("larger than any real file"),
        "{}",
        error.chain()
    );
    std::fs::remove_file(&path).expect(WORKS);
}
