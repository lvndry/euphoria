// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Audio with a known key and tuning, for the detector tests.

use std::f32::consts::TAU;
use std::num::NonZeroU32;

use crate::audio::AudioBuffer;
use crate::key::{Key, Mode};

pub(crate) const SAMPLE_RATE: u32 = 22_050;
const CHORD_SECONDS: f32 = 2.0;
const HARMONIC_COUNT: usize = 5;
/// Midi number of the A tuned to 440 Hz.
const CONCERT_A: f32 = 69.0;
const MIDDLE_C: i32 = 60;

pub(crate) fn buffer(samples: Vec<f32>) -> AudioBuffer {
    AudioBuffer::new(samples, NonZeroU32::new(SAMPLE_RATE).unwrap())
}

/// The frequency of a midi note in a scale tuned `cents` away from 440 Hz.
fn frequency(midi_note: i32, cents: f32) -> f32 {
    440.0 * 2.0_f32.powf((midi_note as f32 - CONCERT_A + cents / 100.0) / 12.0)
}

/// Adds one note with a few overtones and a short fade at both ends.
fn add_note(samples: &mut [f32], midi_note: i32, cents: f32, amplitude: f32) {
    let fade_samples = SAMPLE_RATE as usize / 50;
    let sample_count = samples.len();
    let fundamental = frequency(midi_note, cents);
    for (index, sample) in samples.iter_mut().enumerate() {
        let time = index as f32 / SAMPLE_RATE as f32;
        let fade = (index.min(sample_count - 1 - index) as f32 / fade_samples as f32).min(1.0);
        let tone: f32 = (1..=HARMONIC_COUNT)
            .map(|harmonic| {
                (TAU * fundamental * harmonic as f32 * time).sin() / (harmonic * harmonic) as f32
            })
            .sum();
        *sample += amplitude * fade * tone;
    }
}

/// The tonic, subdominant, dominant and tonic chords of a key, each held for
/// two seconds with its root in the bass, repeated to fill `seconds`.
pub(crate) fn chord_progression(key: Key, cents: f32, seconds: f32) -> Vec<f32> {
    let third = match key.mode() {
        Mode::Major => 4,
        Mode::Minor => 3,
    };
    let tonic = MIDDLE_C + i32::from(key.tonic());
    let chord_roots = [tonic, tonic + 5, tonic + 7, tonic];
    // The dominant chord of a minor key is major, as in the harmonic minor
    // scale, so the progression belongs to one key only.
    let chord_thirds = [third, third, 4, third];
    let chord_samples = (CHORD_SECONDS * SAMPLE_RATE as f32) as usize;
    let chord_count = (seconds / CHORD_SECONDS).ceil() as usize;
    let mut samples = vec![0.0_f32; chord_count * chord_samples];
    for (chord_index, chord) in samples.chunks_mut(chord_samples).enumerate() {
        let root = chord_roots[chord_index % chord_roots.len()];
        let chord_third = chord_thirds[chord_index % chord_thirds.len()];
        add_note(chord, root - 24, cents, 0.2);
        add_note(chord, root, cents, 0.15);
        add_note(chord, root + chord_third, cents, 0.15);
        add_note(chord, root + 7, cents, 0.15);
    }
    samples.truncate((seconds * SAMPLE_RATE as f32) as usize);
    samples
}

/// White noise between `-amplitude` and `amplitude`, the same on every run.
pub(crate) fn white_noise(sample_count: usize, amplitude: f32) -> Vec<f32> {
    let mut state = 0x2545_F491_4F6C_DD1D_u64;
    (0..sample_count)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let unit = (state >> 40) as f32 / (1_u64 << 24) as f32;
            amplitude * (2.0 * unit - 1.0)
        })
        .collect()
}

/// Noise whose power falls with frequency, as the spectrum of music does.
pub(crate) fn pink_noise(sample_count: usize, amplitude: f32) -> Vec<f32> {
    let mut smoothed = [0.0_f32; 3];
    white_noise(sample_count, amplitude)
        .into_iter()
        .map(|sample| {
            smoothed[0] = 0.99765 * smoothed[0] + sample * 0.099_046;
            smoothed[1] = 0.96300 * smoothed[1] + sample * 0.296_516_4;
            smoothed[2] = 0.57000 * smoothed[2] + sample * 1.052_691_3;
            (smoothed[0] + smoothed[1] + smoothed[2] + sample * 0.1848) / 3.0
        })
        .collect()
}

pub(crate) fn mixed(left: &[f32], right: &[f32]) -> Vec<f32> {
    left.iter()
        .zip(right)
        .map(|(left_sample, right_sample)| left_sample + right_sample)
        .collect()
}
