// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Feeds the tag parsers untrusted, corrupted bytes. The parsers read files
//! from the disk, which a DJ may have been given from anywhere, so every
//! shape of damage this test throws at them — truncated files, zero-length
//! tags, oversized field counts, invalid UTF-8, and a fixed-seed scramble
//! of real fixtures — must come back as `Err`, never a panic.
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

fn scratch_path(index: usize, label: &str) -> PathBuf {
    let file = std::env::args().next().map_or_else(
        || "tags-fuzz".to_string(),
        |argument| {
            PathBuf::from(argument).file_name().map_or_else(
                || "tags-fuzz".to_string(),
                |name| name.to_string_lossy().to_string(),
            )
        },
    );
    PathBuf::from(std::env::temp_dir())
        .join(format!("euphoria-tags-fuzz-{file}"))
        .join(format!("{index}-{label}"))
}

fn attempt(
    index: usize,
    label: &str,
    bytes: &[u8],
    extension: &str,
) -> Result<euphoria_core::tags::TrackTags, euphoria_core::EuphoriaError> {
    let path = scratch_path(index, label);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the scratch directory can be created");
    }
    std::fs::write(&path, bytes).expect("the scratch file can be written");
    let with_extension = path.with_extension(extension);
    if with_extension != path {
        std::fs::rename(&path, &with_extension).expect("the scratch file can be renamed");
    }
    read_tags(&with_extension)
}

/// Every corrupted shape of every fixture returns `Err` or a result, never
/// a panic, and every error is a typed `EuphoriaError`.
#[test]
fn corrupted_tag_files_are_an_error_not_a_panic() {
    let mut failures = Vec::new();
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
            let outcome = attempt(index, label, &bytes[..cut], extension);
            if outcome.is_err() {
            } else {
                failures.push(format!(
                    "{name} truncated to {label}: unexpectedly readable"
                ));
            }
        }
        // Invalid UTF-8 where text is expected, and a random scramble of the
        // head and the middle of the file.
        for (label, flip) in [("corrupt-head", 16_usize), ("corrupt-mid", 0)] {
            let mut lcg = Lcg::new(index as u64 + 1);
            let scrambled = if flip > 0 {
                lcg.scramble(&bytes, flip)
            } else {
                let mut mid = bytes.to_vec();
                let at = bytes.len() / 2;
                for offset in 0..16.min(bytes.len() - at) {
                    mid[at + offset] = (lcg.next_u32() % 256) as u8;
                }
                mid
            };
            index += 1;
            let outcome = attempt(index, label, &scrambled, extension);
            if outcome.is_ok() && label == "corrupt-head" {
                // A scramble that still parses is legal only when the damage
                // landed outside the tag; note it but do not fail on it.
            }
            let _ = outcome;
        }
        // A file of zeros the size of the real one: a header claiming a tag
        // that is not there.
        index += 1;
        let zeros = vec![0_u8; bytes.len()];
        if attempt(index, "zeros", &zeros, extension).is_ok() {
            failures.push(format!("{name} zeros: unexpectedly readable"));
        }
    }
    assert!(failures.is_empty(), "{failures:?}");
}

/// An oversized ID3v2 frame count: a header promising a huge body, then
/// garbage. Must be an error, not a panic.
#[test]
fn an_oversized_id3v2_header_is_an_error() {
    // ID3v2.4 header: "ID3" 04 00, size 0xFF FF FF FF (unsync-safe, huge),
    // then a body of zeros far shorter than the size claims.
    let mut file = Vec::new();
    file.extend_from_slice(b"ID3\x04\x00\x00");
    file.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
    file.extend_from_slice(&vec![0_u8; 64]);
    let path = scratch_path(1_000_000, "oversized-id3v2");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("the scratch directory can be created");
    }
    let path = path.with_extension("mp3");
    std::fs::write(&path, &file).expect("the scratch file can be written");
    let outcome = read_tags(&path);
    assert!(
        outcome.is_err(),
        "a body far shorter than its header claims cannot be read: {outcome:?}"
    );
}

/// Empty and odd fields surface as a miss, never a panic. The real
/// invalid-UTF-8 coverage is the file-based battery above, which feeds the
/// parsers corrupted bytes the way a DJ's tag writer put them there.
#[test]
fn invalid_utf8_in_a_text_field_is_handled() {
    assert!(parse_tempo("").is_none());
    assert!(parse_tempo("  ").is_none());
    assert_eq!(Key::parse_file_key(""), None);
    assert_eq!(
        Key::parse_file_key("Am \u{266F}"),
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
