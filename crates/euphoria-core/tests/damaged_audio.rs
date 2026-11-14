// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Truncated and corrupt files are flagged as damaged instead of analyzed as
//! if they were fine, and the mono mixdown keeps phase inverted and surround
//! files from turning silent or muddy.

use std::path::{Path, PathBuf};

use euphoria_core::quality::analyze_quality;
use euphoria_core::{analyze_file, decode_mono};

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

/// A 16 bit PCM WAV of `channels` channels, one sample per channel from
/// `sample_at(frame, channel)`.
fn wav_bytes(
    channels: u16,
    sample_rate: u32,
    frame_count: usize,
    sample_at: impl Fn(usize, u16) -> f32,
) -> Vec<u8> {
    let data_bytes = frame_count as u32 * u32::from(channels) * 2;
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + data_bytes).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(channels.to_le_bytes());
    bytes.extend(sample_rate.to_le_bytes());
    bytes.extend((sample_rate * u32::from(channels) * 2).to_le_bytes());
    bytes.extend((channels * 2).to_le_bytes());
    bytes.extend(16_u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data_bytes.to_le_bytes());
    for frame in 0..frame_count {
        for channel in 0..channels {
            let sample = (sample_at(frame, channel).clamp(-1.0, 1.0) * 32_767.0) as i16;
            bytes.extend(sample.to_le_bytes());
        }
    }
    bytes
}

fn tone(frame: usize, sample_rate: u32, hertz: f32) -> f32 {
    (std::f32::consts::TAU * hertz * frame as f32 / sample_rate as f32).sin()
}

const FIXTURES: [&str; 9] = [
    "sine-440hz-mono.wav",
    "sine-440hz-stereo.wav",
    "sine-440hz-mono.flac",
    "sine-440hz-mono.mp3",
    "sine-440hz-mono.m4a",
    "sine-440hz-mono.mp4",
    "sine-440hz-mono-alac.m4a",
    "sine-440hz-mono.ogg",
    "sine-440hz-mono.aiff",
];

#[test]
fn intact_files_are_not_flagged() {
    for name in FIXTURES {
        let report = analyze_quality(&fixture(name)).unwrap();
        assert!(!report.is_damaged(), "{name}: {:?}", report.damage);
        assert!(report.notes().is_empty(), "{name}: {:?}", report.notes());
    }
}

#[test]
fn a_truncated_wav_is_damaged_and_says_how_much_is_missing() {
    let bytes = std::fs::read(fixture("sine-440hz-mono.wav")).unwrap();
    let path = scratch_file("damaged-truncated.wav", &bytes[..bytes.len() / 2]);

    let report = analyze_quality(&path).unwrap();

    assert!(report.is_damaged());
    assert_eq!(report.damage.dropped_packets, 0);
    assert!((report.damage.missing_seconds() - 0.5).abs() < 0.05);
    assert!((report.damage.declared_seconds.unwrap() - 1.0).abs() < 0.01);
    assert!(
        report
            .notes()
            .iter()
            .any(|note| note.starts_with("Damaged file:") && note.contains("truncated"))
    );
}

#[test]
fn a_truncated_mp3_is_damaged() {
    let bytes = std::fs::read(fixture("sine-440hz-mono.mp3")).unwrap();
    let path = scratch_file("damaged-truncated.mp3", &bytes[..bytes.len() / 2]);

    let report = analyze_quality(&path).unwrap();

    assert!(report.is_damaged(), "{:?}", report.damage);
    assert!(report.damage.missing_seconds() > 0.3, "{:?}", report.damage);
}

#[test]
fn corrupt_flac_frames_are_counted_and_filled_with_silence() {
    let sample_rate = 44_100;
    let bytes = std::fs::read(fixture("sine-440hz-mono.flac")).unwrap();
    let mut corrupt = bytes.clone();
    let middle = corrupt.len() / 2;
    for byte in &mut corrupt[middle..middle + 40] {
        *byte ^= 0xA5;
    }
    let path = scratch_file("damaged-corrupt.flac", &corrupt);

    let report = analyze_quality(&path).unwrap();
    let audio = decode_mono(&path).unwrap();

    assert!(report.is_damaged(), "{:?}", report.damage);
    assert!(report.is_heavily_damaged());
    assert!(report.damage.dropped_packets > 0, "{:?}", report.damage);
    assert!(report.damage.concealed_seconds > 0.0);
    assert!(
        (audio.duration_seconds() - 1.0).abs() < 0.02,
        "a dropped frame must not shift the times after it: {}",
        audio.duration_seconds()
    );
    assert_eq!(audio.sample_rate().get(), sample_rate);
}

#[test]
fn a_heavily_damaged_file_gets_no_beat_grid_and_no_cues() {
    let sample_rate = 22_050;
    let clicks = |frame: usize, _channel: u16| {
        let beat = sample_rate as usize / 2;
        let into_beat = frame % beat;
        if into_beat < 400 {
            (1.0 - into_beat as f32 / 400.0) * tone(frame, sample_rate, 90.0)
        } else {
            0.0
        }
    };
    let whole = wav_bytes(1, sample_rate, sample_rate as usize * 24, clicks);
    let path = scratch_file("damaged-clicks.wav", &whole[..whole.len() / 3]);

    let analysis = analyze_file(&path).unwrap();
    let quality = analysis.quality.unwrap();

    assert!(quality.is_damaged());
    assert!(quality.is_heavily_damaged());
    assert!(analysis.beat_grid.is_none());
    assert!(analysis.cues.is_empty());
    assert!(
        quality
            .notes()
            .iter()
            .any(|note| note.contains("beat grid and cues were not written"))
    );
}

#[test]
fn phase_inverted_stereo_is_analyzed_from_the_left_channel() {
    let sample_rate = 44_100;
    let bytes = wav_bytes(
        2,
        sample_rate,
        sample_rate as usize * 2,
        |frame, channel| {
            let sample = 0.3 * tone(frame, sample_rate, 440.0);
            if channel == 0 { sample } else { -sample }
        },
    );
    let path = scratch_file("inverted-phase.wav", &bytes);

    let audio = decode_mono(&path).unwrap();
    let report = analyze_quality(&path).unwrap();

    assert!(!audio.is_silent());
    let peak = audio
        .samples()
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    assert!((peak - 0.3).abs() < 0.01, "{peak}");
    assert!(report.cancelled_share > 0.99, "{}", report.cancelled_share);
    assert!(
        report
            .notes()
            .iter()
            .any(|note| note.contains("cancel each other") && note.contains("left channel"))
    );
    assert!(!report.is_damaged());
}

#[test]
fn ordinary_stereo_is_averaged_and_not_noted() {
    let sample_rate = 44_100;
    let bytes = wav_bytes(
        2,
        sample_rate,
        sample_rate as usize * 2,
        |frame, channel| {
            let hertz = if channel == 0 { 440.0 } else { 660.0 };
            0.3 * tone(frame, sample_rate, hertz)
        },
    );
    let path = scratch_file("ordinary-stereo.wav", &bytes);

    let audio = decode_mono(&path).unwrap();
    let report = analyze_quality(&path).unwrap();

    assert_eq!(report.cancelled_share, 0.0);
    assert!(report.notes().is_empty());
    let expected_third =
        (0.3 * tone(10, sample_rate, 440.0) + 0.3 * tone(10, sample_rate, 660.0)) / 2.0;
    assert!((audio.samples()[10] - expected_third).abs() < 0.001);
}

#[test]
fn five_point_one_uses_only_the_front_left_and_right() {
    let sample_rate = 22_050;
    let bytes = wav_bytes(
        6,
        sample_rate,
        sample_rate as usize,
        |frame, channel| match channel {
            0 | 1 => 0.2 * tone(frame, sample_rate, 440.0),
            3 => 0.9 * tone(frame, sample_rate, 50.0),
            _ => 0.5 * tone(frame, sample_rate, 3_000.0),
        },
    );
    let path = scratch_file("surround.wav", &bytes);

    let audio = decode_mono(&path).unwrap();

    let peak = audio
        .samples()
        .iter()
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    assert!(
        (peak - 0.2).abs() < 0.01,
        "the centre, bass and surrounds leaked in: {peak}"
    );
}
