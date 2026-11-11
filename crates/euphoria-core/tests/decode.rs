// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Decoding real files of every supported format, and the errors returned for
//! files that are missing, empty, damaged or not audio at all.

use std::path::{Path, PathBuf};

use euphoria_core::quality::{QualityVerdict, analyze_quality};
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
fn decodes_aac_in_mp4() {
    assert_decodes_to_one_second_tone("sine-440hz-mono.mp4");
    let report = analyze_quality(&fixture("sine-440hz-mono.mp4")).expect("the fixture decodes");
    assert_eq!(report.codec, "aac");
    assert!(!report.lossless);
}

#[test]
fn decodes_alac_exactly_with_its_bit_depth() {
    let audio = assert_decodes_to_one_second_tone("sine-440hz-mono-alac.m4a");
    assert!((peak(&audio) - FIXTURE_AMPLITUDE).abs() < PCM_AMPLITUDE_TOLERANCE);
    // ALAC is lossless, so it decodes to the same samples as the WAV it
    // was made from.
    let wav = decode_mono(&fixture("sine-440hz-mono.wav")).expect("the fixture decodes");
    assert_eq!(audio.samples(), wav.samples());

    let report =
        analyze_quality(&fixture("sine-440hz-mono-alac.m4a")).expect("the fixture decodes");
    assert_eq!(report.codec, "alac");
    assert!(report.lossless);
    assert_eq!(report.bit_depth, Some(16));
    assert_eq!(report.verdict, QualityVerdict::Lossless);
}

#[test]
fn decodes_ogg_vorbis() {
    let audio = assert_decodes_to_one_second_tone("sine-440hz-mono.ogg");
    // A lossy encoding of the tone keeps its level.
    assert!(
        (peak(&audio) - FIXTURE_AMPLITUDE).abs() < 0.03,
        "{}",
        peak(&audio)
    );
    let report = analyze_quality(&fixture("sine-440hz-mono.ogg")).expect("the fixture decodes");
    assert_eq!(report.codec, "vorbis");
    assert!(!report.lossless);
    assert_eq!(report.bit_depth, None);
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

/// A copy of the mono WAV fixture whose header declares another sample rate.
fn wav_declaring_sample_rate(name: &str, sample_rate: u32) -> PathBuf {
    let mut bytes =
        std::fs::read(fixture("sine-440hz-mono.wav")).expect("the WAV fixture is readable");
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[12..16], b"fmt ");
    bytes[24..28].copy_from_slice(&sample_rate.to_le_bytes());
    scratch_file(name, &bytes)
}

#[test]
fn a_file_declaring_an_absurd_sample_rate_is_refused() {
    for (name, sample_rate) in [
        ("rate-too-high.wav", 4_000_000_000_u32),
        ("rate-just-too-high.wav", 384_001),
        ("rate-too-low.wav", 100),
    ] {
        let path = wav_declaring_sample_rate(name, sample_rate);
        match decode_mono(&path) {
            Err(EuphoriaError::UnsupportedSampleRate {
                sample_rate: reported,
                ..
            }) => assert_eq!(reported, sample_rate),
            other => panic!("{name}: expected an unsupported sample rate, got {other:?}"),
        }
    }
}

#[test]
fn a_file_at_the_edge_of_the_supported_range_decodes() {
    for (name, sample_rate) in [
        ("rate-lowest.wav", 8_000_u32),
        ("rate-highest.wav", 384_000),
    ] {
        let path = wav_declaring_sample_rate(name, sample_rate);
        let audio = decode_mono(&path).unwrap();
        assert_eq!(audio.sample_rate().get(), sample_rate);
    }
}

#[test]
fn the_unsupported_sample_rate_message_reads_as_a_full_sentence() {
    let path = wav_declaring_sample_rate("rate-message.wav", 100);
    let message = decode_mono(&path).unwrap_err().to_string();
    assert_eq!(
        message,
        "a sample rate of 100 Hz is not supported: Euphoria reads audio from 8000 to 384000 Hz"
    );
}
