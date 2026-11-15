// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Playing a track faster or slower while it plays, with and without key
//! lock, against a steady tone written here.

use std::f64::consts::TAU;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::{PlaybackTempo, TempoMode, TrackSource, VoiceState};

const SAMPLE_RATE: u32 = 44_100;
const TONE_HERTZ: f64 = 440.0;
const TRACK_SECONDS: usize = 8;

struct Tone(PathBuf);

impl Tone {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("euphoria-tempo-{name}-{}.wav", std::process::id()));
        let frames = SAMPLE_RATE as usize * TRACK_SECONDS;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + frames as u32 * 2).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
        bytes.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(frames as u32 * 2).to_le_bytes());
        for frame in 0..frames {
            let sample = (TAU * TONE_HERTZ * frame as f64 / f64::from(SAMPLE_RATE)).sin() * 0.5;
            bytes.extend_from_slice(&((sample * 32_767.0) as i16).to_le_bytes());
        }
        std::fs::write(&path, bytes).unwrap();
        Self(path)
    }

    fn source(&self) -> TrackSource {
        TrackSource::open(&self.0, 0.0).unwrap()
    }
}

impl Drop for Tone {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn next_sound(source: &mut TrackSource, count: usize) -> Vec<f32> {
    let state = source.state();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut samples = Vec::with_capacity(count);
    while samples.len() < count {
        assert!(Instant::now() < deadline, "the decoder fell silent");
        let silence_before = state.silent_frames();
        let Some(sample) = source.next() else { break };
        if state.silent_frames() == silence_before {
            samples.push(sample);
        }
    }
    samples
}

fn tone_frequency(samples: &[f32]) -> f64 {
    let crossings: Vec<usize> = samples
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] < 0.0 && pair[1] >= 0.0)
        .map(|(index, _)| index)
        .collect();
    let span = (crossings[crossings.len() - 1] - crossings[0]) as f64;
    (crossings.len() - 1) as f64 * f64::from(SAMPLE_RATE) / span
}

fn playing_at(rate: f64, key_lock: bool, tone: &Tone) -> (TrackSource, std::sync::Arc<VoiceState>) {
    let source = tone.source().patient();
    let state = source.state();
    state.set_tempo(PlaybackTempo { rate, key_lock });
    (source, state)
}

#[test]
fn the_playhead_advances_at_the_rate_times_the_time_heard() {
    let tone = Tone::new("position");
    for key_lock in [true, false] {
        for rate in [0.84, 0.92, 1.0, 1.08, 1.16] {
            let (mut source, state) = playing_at(rate, key_lock, &tone);
            next_sound(&mut source, 4_000);
            let before = state.playhead_frames();
            let heard = 22_050;
            next_sound(&mut source, heard);
            let advanced = (state.playhead_frames() - before) as f64;
            let expected = rate * heard as f64;
            assert!(
                (advanced - expected).abs() <= 2.0 + expected * 0.002,
                "rate {rate}, key lock {key_lock}: the playhead moved {advanced} frames, expected {expected}"
            );
        }
    }
}

#[test]
fn key_lock_stretches_and_keeps_the_pitch_while_resampling_follows_the_speed() {
    let tone = Tone::new("pitch");
    let (mut locked, locked_state) = playing_at(1.1, true, &tone);
    let heard = next_sound(&mut locked, 30_000);
    assert_eq!(locked_state.tempo_mode(), TempoMode::Stretching);
    let steady = tone_frequency(&heard[8_000..]);
    assert!((steady - TONE_HERTZ).abs() < 3.0, "{steady}");

    let (mut free, free_state) = playing_at(1.1, false, &tone);
    let heard = next_sound(&mut free, 30_000);
    assert_eq!(free_state.tempo_mode(), TempoMode::Resampling);
    let followed = tone_frequency(&heard[2_000..]);
    assert!((followed - TONE_HERTZ * 1.1).abs() < 3.0, "{followed}");
}

#[test]
fn at_one_hundred_percent_the_sound_passes_untouched_and_without_delay() {
    let tone = Tone::new("neutral");
    let mut plain = tone.source().patient();
    let reference = next_sound(&mut plain, 2_000);
    for key_lock in [true, false] {
        let (mut source, state) = playing_at(1.0, key_lock, &tone);
        assert_eq!(next_sound(&mut source, 2_000), reference);
        assert_eq!(state.tempo_mode(), TempoMode::Bypass);
    }
}

#[test]
fn the_tempo_changes_while_playing_and_the_playhead_carries_on() {
    let tone = Tone::new("live");
    let mut source = tone.source().patient();
    let state = source.state();
    next_sound(&mut source, 10_000);
    assert_eq!(state.tempo_mode(), TempoMode::Bypass);
    let before = state.playhead_frames();
    state.set_tempo(PlaybackTempo {
        rate: 1.16,
        key_lock: true,
    });
    next_sound(&mut source, 10_000);
    assert_eq!(state.tempo_mode(), TempoMode::Stretching);
    let after = state.playhead_frames();
    assert!(
        after > before + 10_000 && after < before + 14_000,
        "{before} -> {after}"
    );
    state.set_tempo(PlaybackTempo::default());
    next_sound(&mut source, 5_000);
    assert_eq!(state.tempo_mode(), TempoMode::Bypass);
}

#[test]
fn a_stretched_track_ends_after_its_length_divided_by_the_rate() {
    let tone = Tone::new("length");
    for (rate, key_lock) in [(1.16, true), (0.84, true), (1.16, false), (0.84, false)] {
        let (mut source, _) = playing_at(rate, key_lock, &tone);
        let mut frames = 0_usize;
        while source.next().is_some() {
            frames += 1;
        }
        let expected = SAMPLE_RATE as f64 * TRACK_SECONDS as f64 / rate;
        assert!(
            (frames as f64 - expected).abs() < expected * 0.01,
            "rate {rate}, key lock {key_lock}: {frames} frames, expected {expected}"
        );
    }
}

#[test]
fn a_seek_keeps_the_tempo_and_lands_in_track_time() {
    let tone = Tone::new("seek");
    let (mut source, state) = playing_at(1.1, true, &tone);
    next_sound(&mut source, 5_000);
    state.seek_to(4.0);
    next_sound(&mut source, 22_050);
    let seconds = state.position_seconds();
    assert!(seconds > 4.2 && seconds < 4.8, "{seconds}");
}
