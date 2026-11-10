// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Decoding real files of every supported format, and the errors returned for
//! files that are missing, empty, damaged or not audio at all.

use std::path::{Path, PathBuf};

use euphoria_core::{AudioBuffer, DecodeFailure, EuphoriaError, decode_mono};

const FIXTURE_SAMPLE_RATE: u32 = 44_100;
const FIXTURE_DURATION_SECONDS: f32 = 1.0;
const FIXTURE_AMPLITUDE: f32 = 0.125;
// AAC adds encoder priming and padding, so the M4A fixture runs about 45 ms long.
const DURATION_TOLERANCE_SECONDS: f32 = 0.06;
// One step of 8-bit PCM is 1/128.
const PCM_AMPLITUDE_TOLERANCE: f32 = 0.01;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn scratch_file(name: &str, contents: &[u8]) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, contents).expect("cannot write the scratch file");
    path
}

fn peak(audio: &AudioBuffer) -> f32 {
    audio
        .samples()
        .iter()
        .fold(0.0, |loudest, sample| loudest.max(sample.abs()))
}

fn assert_decodes_to_one_second_tone(name: &str) -> AudioBuffer {
    let audio = decode_mono(&fixture(name)).expect("the fixture decodes");
    assert_eq!(audio.sample_rate().get(), FIXTURE_SAMPLE_RATE, "{name}");
    let duration = audio.duration_seconds();
    assert!(
        (duration - FIXTURE_DURATION_SECONDS).abs() < DURATION_TOLERANCE_SECONDS,
        "{name} decoded to {duration} seconds"
    );
    assert!(!audio.is_silent(), "{name} decoded to silence");
    audio
}

fn decode_failure(path: &Path) -> DecodeFailure {
    match decode_mono(path) {
        Err(EuphoriaError::Decode { failure, .. }) => failure,
        other => panic!(
            "expected a decode error for {}, got {other:?}",
            path.display()
        ),
    }
}

#[test]
fn decodes_mono_wav() {
    let audio = assert_decodes_to_one_second_tone("sine-440hz-mono.wav");
    assert!((peak(&audio) - FIXTURE_AMPLITUDE).abs() < PCM_AMPLITUDE_TOLERANCE);
}

#[test]
fn decodes_stereo_wav_to_the_average_of_its_channels() {
    let audio = assert_decodes_to_one_second_tone("sine-440hz-stereo.wav");
    assert!((peak(&audio) - FIXTURE_AMPLITUDE).abs() < PCM_AMPLITUDE_TOLERANCE);
}

#[test]
fn decodes_flac() {
    let audio = assert_decodes_to_one_second_tone("sine-440hz-mono.flac");
    assert!((peak(&audio) - FIXTURE_AMPLITUDE).abs() < PCM_AMPLITUDE_TOLERANCE);
}

#[test]
fn decodes_mp3() {
    assert_decodes_to_one_second_tone("sine-440hz-mono.mp3");
}

#[test]
fn decodes_aac_in_m4a() {
    assert_decodes_to_one_second_tone("sine-440hz-mono.m4a");
}

#[test]
fn decodes_aiff() {
    let audio = assert_decodes_to_one_second_tone("sine-440hz-mono.aiff");
    assert!((peak(&audio) - FIXTURE_AMPLITUDE).abs() < PCM_AMPLITUDE_TOLERANCE);
}

#[test]
fn opposite_phase_channels_cancel_out() {
    let audio = decode_mono(&fixture("sine-440hz-opposite-phase-stereo.flac")).unwrap();
    assert_eq!(audio.sample_rate().get(), FIXTURE_SAMPLE_RATE);
    assert!(
        (audio.duration_seconds() - FIXTURE_DURATION_SECONDS).abs() < DURATION_TOLERANCE_SECONDS
    );
    assert!(audio.is_silent());
}

#[test]
fn missing_file_is_an_open_error() {
    let path = fixture("does-not-exist.wav");
    let error = decode_mono(&path).unwrap_err();
    assert!(
        matches!(&error, EuphoriaError::Open { path: error_path, .. } if *error_path == path),
        "{error:?}"
    );
}

#[test]
fn text_file_named_mp3_is_an_unsupported_format() {
    let path = scratch_file(
        "not-audio.mp3",
        "This is a text file, not audio.\n".repeat(64).as_bytes(),
    );
    assert_eq!(decode_failure(&path), DecodeFailure::UnsupportedFormat);
}

#[test]
fn flac_cut_inside_its_header_is_a_read_error() {
    let bytes = std::fs::read(fixture("sine-440hz-mono.flac")).unwrap();
    let path = scratch_file("truncated-header.flac", &bytes[..100]);
    assert_eq!(decode_failure(&path), DecodeFailure::Read);
}

#[test]
fn m4a_cut_before_its_index_is_an_unsupported_format() {
    let bytes = std::fs::read(fixture("sine-440hz-mono.m4a")).unwrap();
    let path = scratch_file("truncated-index.m4a", &bytes[..bytes.len() / 2]);
    assert_eq!(decode_failure(&path), DecodeFailure::UnsupportedFormat);
}

#[test]
fn wav_cut_inside_its_samples_decodes_what_remains() {
    let bytes = std::fs::read(fixture("sine-440hz-mono.wav")).unwrap();
    let path = scratch_file("truncated-samples.wav", &bytes[..bytes.len() / 2]);
    let audio = decode_mono(&path).unwrap();
    assert!((audio.duration_seconds() - FIXTURE_DURATION_SECONDS / 2.0).abs() < 0.01);
    assert!(!audio.is_silent());
}

#[test]
fn empty_file_is_an_unsupported_format() {
    let path = scratch_file("empty.wav", &[]);
    assert_eq!(decode_failure(&path), DecodeFailure::UnsupportedFormat);
}
