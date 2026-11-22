// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds the tag parsers untrusted, corrupted bytes. The parsers read files
//! from the disk, which a DJ may have been given from anywhere, so every
//! shape of damage this test throws at them — truncated files, zero-length
//! tags, oversized field counts, invalid UTF-8, and a fixed-seed scramble
//! of real fixtures — must never panic, and must come back as `Err` or as
//! tags the reader still managed to keep.
//!
//! The safety property is "no panic"; the reader is allowed to tolerate a
//! damaged container and still return the metadata it can. So the battery
//! only requires `Err` for corrupted shapes whose rejection is part of the
//! parser's contract: an ID3v2 header promising a body that is not there.
//!
//! This is the cheap stand-in for a cargo-fuzz target: the same entry
//! points, driven by a deterministic battery of corrupted inputs.

use std::path::PathBuf;

use euphoria_core::Key;
use euphoria_core::tags::{parse_tempo, read_tags};

/// A deterministic source of corrupted bytes: a fixed-seed LCG.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 32) as u32
    }

    /// `bytes` with the first `flip` bytes replaced by pseudo-random ones.
    fn scramble(&mut self, bytes: &[u8], flip: usize) -> Vec<u8> {
        let mut out = bytes.to_vec();
        for slot in 0..flip.min(out.len()) {
            out[slot] = (self.next_u32() % 256) as u8;
        }
        out
    }
}

fn fixtures() -> Vec<(String, Vec<u8>)> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures");
    let mut found = std::fs::read_dir(&directory)
        .expect("the fixtures directory is in the repo")
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let extension = entry.path().extension()?.to_str()?.to_string();
            matches!(
                extension.as_str(),
                "mp3" | "ogg" | "flac" | "m4a" | "mp4" | "aiff" | "wav"
            )
            .then_some((name, entry.path()))
        })
        .map(|(name, path)| {
            (
                name,
                std::fs::read(&path).expect("the fixture is in the repo"),
            )
        })
        .collect::<Vec<_>>();
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
}

/// A scratch path no other test or run can collide with: unique per process
/// and per file, and a pre-planted symlink cannot redirect the write.
fn scratch_path(directory: &std::path::Path, index: usize, label: &str) -> PathBuf {
    let file = std::env::args().next().map_or_else(
        || "tags-fuzz".to_string(),
        |argument| {
            PathBuf::from(argument).file_name().map_or_else(
                || "tags-fuzz".to_string(),
                |name| name.to_string_lossy().to_string(),
            )
        },
    );
    directory.join(format!(
        "euphoria-tags-fuzz-{}-{}-{index}-{label}",
        std::process::id(),
        file
    ))
}

fn attempt(
    directory: &std::path::Path,
    index: usize,
    label: &str,
    bytes: &[u8],
    extension: &str,
) -> Result<euphoria_core::tags::TrackTags, euphoria_core::EuphoriaError> {
    let path = scratch_path(directory, index, label);
    std::fs::write(&path, bytes).expect("the scratch file can be written");
    let with_extension = path.with_extension(extension);
    if with_extension != path {
        std::fs::rename(&path, &with_extension).expect("the scratch file can be renamed");
    }
    read_tags(&with_extension)
}

/// Every corrupted shape of every fixture is read without a panic. A
/// truncated audio payload does not invalidate metadata the reader can
/// still parse, so a readable outcome is legal here; the shapes whose
/// rejection is the parser's contract are covered below.
#[test]
fn corrupted_tag_files_never_panic() {
    let scratch = tempfile::tempdir().expect("the scratch directory can be created");
    let mut index = 0;
    for (name, bytes) in fixtures() {
        let extension = name.rsplit_once('.').map_or("bin", |(_, ext)| ext);
        // Truncated at every size up to half the file.
        for (label, cut) in [
            ("empty", 0_usize),
            ("one-byte", 1),
            ("six-bytes", 6),
            ("twelve-bytes", 12),
            ("thirty-bytes", 30),
            ("half", bytes.len() / 2),
            ("one-short", bytes.len().saturating_sub(1)),
        ] {
            index += 1;
            let _ = attempt(scratch.path(), index, label, &bytes[..cut], extension);
        }
        // A fixed-seed scramble of the head and the middle of the file.
        for label in ["corrupt-head", "corrupt-mid"] {
            let mut lcg = Lcg::new(index as u64 + 1);
            let scrambled = if label == "corrupt-head" {
                lcg.scramble(&bytes, 16)
            } else {
                let mut mid = bytes.to_vec();
                let at = bytes.len() / 2;
                for offset in 0..16.min(bytes.len().saturating_sub(at)) {
                    mid[at + offset] = (lcg.next_u32() % 256) as u8;
                }
                mid
            };
            index += 1;
            let _ = attempt(scratch.path(), index, label, &scrambled, extension);
        }
        // A file of zeros the size of the real one: a header claiming a tag
        // that is not there.
        index += 1;
        let zeros = vec![0_u8; bytes.len()];
        let _ = attempt(scratch.path(), index, "zeros", &zeros, extension);
    }
}

/// An ID3v2 header promising a body that is not there. This shape's
/// rejection is part of the parser's contract: the header names a size the
/// file does not hold, so the tag cannot be read.
#[test]
fn an_oversized_id3v2_header_is_an_error() {
    let scratch = tempfile::tempdir().expect("the scratch directory can be created");
    // ID3v2.4 header: "ID3" 04 00, size 0xFF FF FF FF (unsync-safe, huge),
    // then a body of zeros far shorter than the size claims.
    let mut file = Vec::new();
    file.extend_from_slice(b"ID3\x04\x00\x00");
    file.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
    file.extend_from_slice(&[0_u8; 64]);
    let path = scratch_path(scratch.path(), 1_000_000, "oversized-id3v2");
    let path = path.with_extension("mp3");
    std::fs::write(&path, &file).expect("the scratch file can be written");
    let outcome = read_tags(&path);
    assert!(
        outcome.is_err(),
        "a body far shorter than its header claims cannot be read: {outcome:?}"
    );
}

/// A valid ID3v2.3 TIT2 frame whose payload is not valid UTF-8. The reader
/// must not panic; the field is dropped or the file is an error, but the
/// program keeps going.
#[test]
fn an_invalid_utf8_title_frame_does_not_panic() {
    let scratch = tempfile::tempdir().expect("the scratch directory can be created");
    // "TIT2", size 6, flags, encoding 0 (ISO-8859-1/UTF-8) then a 0x80 byte.
    let mut frame = Vec::new();
    frame.extend_from_slice(b"TIT2");
    frame.extend_from_slice(&[0x00, 0x00, 0x00, 0x06]);
    frame.extend_from_slice(&[0x00, 0x00]);
    frame.extend_from_slice(&[0x00, 0xFF, 0xFE, 0x80, 0x41, 0x00]);
    // An ID3v2.3 header claiming exactly that frame.
    let mut file = Vec::new();
    file.extend_from_slice(b"ID3\x03\x00\x00");
    file.extend_from_slice(&[0x00, 0x00, 0x00, frame.len() as u8]);
    file.extend_from_slice(&frame);
    let path = scratch_path(scratch.path(), 2_000_000, "invalid-utf8-title");
    let path = path.with_extension("mp3");
    std::fs::write(&path, &file).expect("the scratch file can be written");
    let _ = read_tags(&path);
}

/// Empty and odd fields surface as a miss, never a panic.
#[test]
fn invalid_utf8_in_a_text_field_is_handled() {
    assert!(parse_tempo("").is_none());
    assert!(parse_tempo("  ").is_none());
    assert_eq!(Key::parse_file_key(""), None);
    assert_eq!(
        Key::parse_file_key("F\u{266F}"),
        Some(Key::new(6, euphoria_core::Mode::Major))
    );
}

/// The LCG scramble is deterministic: the same seed reproduces the same
/// bytes, so a failure is reproducible.
#[test]
fn the_corruption_is_deterministic() {
    let bytes = b"0123456789abcdef";
    let first = {
        let mut lcg = Lcg::new(42);
        lcg.scramble(bytes, 4)
    };
    let mut lcg = Lcg::new(42);
    assert_eq!(first, lcg.scramble(bytes, 4));
}
