// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How strongly each note of six octaves, C1 to B6, sounds in a track,
//! measured on the scale of the track's own tuning, and the 12 notes that
//! gives with every octave added together.
//!
//! The audio is brought down to about 5.5 kHz, cut into overlapping frames
//! of about three seconds and transformed. Each note gathers the spectrum
//! within a narrow window around its frequency. Steady sounds with no pitch,
//! such as noise, cymbals and the body of a drum, spread over every note
//! alike, so the level that most notes around a note reach is taken off it
//! first, and what stays is the peaks that pitched sounds make.

use std::f64::consts::TAU;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use crate::audio::AudioBuffer;
use crate::detect::decimate::decimate;

/// Notes in an octave.
const SEMITONES: usize = 12;
/// Octaves measured, from C1.
const OCTAVE_COUNT: usize = 6;
/// Notes measured, C1 first.
const BAND_COUNT: usize = SEMITONES * OCTAVE_COUNT;
/// C1 on the scale where A4 is 440 Hz.
const LOWEST_NOTE_HZ: f64 = 32.703_195_662_574_83;
/// The rate the audio is brought down towards before it is transformed. It
/// keeps B6, about 1,976 Hz, and the window around it well below the
/// frequency the lower rate starts to cut.
const ANALYSIS_RATE: u32 = 5_500;
/// Frames are the largest power of two number of samples that fits in this
/// length: long enough to tell apart notes a semitone apart in the lowest
/// octave, where they are 2 Hz apart.
const FRAME_SECONDS: f64 = 3.0;
/// Frames start a quarter of a frame apart.
const HOPS_PER_FRAME: usize = 4;
/// Each note gathers the spectrum this many cents either side of its
/// frequency, weighted by a raised cosine that falls to zero at the edge.
const NOTE_HALF_WIDTH_CENTS: f64 = 35.0;
/// The level taken off each note is the median of the notes this many
/// semitones either side of it in the same frame.
const FLOOR_HALF_WIDTH_SEMITONES: usize = 6;

/// How strongly each of the 12 notes sounds over the whole of `audio`, C
/// first, from 0 to 1 with the strongest at 1, heard on a scale tuned
/// `tuning_cents` away from A = 440 Hz, sharp as positive. Every octave
/// counts alike. `None` when no note sounds, as in silence, in noise with
/// no pitch, and when there are no samples.
pub(crate) fn pitch_class_profile(
    audio: &AudioBuffer,
    tuning_cents: f32,
) -> Option<[f32; SEMITONES]> {
    let totals = folded(&note_totals(audio, tuning_cents));
    let largest = totals.iter().copied().fold(0.0_f64, f64::max);
    (largest > 0.0 && largest.is_finite())
        .then(|| std::array::from_fn(|pitch_class| (totals[pitch_class] / largest) as f32))
}

/// The level of every note, C1 first, added up over the frames of `audio`.
fn note_totals(audio: &AudioBuffer, tuning_cents: f32) -> [f64; BAND_COUNT] {
    let input_rate = audio.sample_rate().get();
    let factor = (input_rate / ANALYSIS_RATE).max(1) as usize;
    let analysis_rate = f64::from(input_rate) / factor as f64;
    let samples = decimate(audio.samples(), factor);
    let frame_length = frame_length(analysis_rate);
    let hop_length = frame_length / HOPS_PER_FRAME;
    let kernels = note_kernels(analysis_rate, frame_length, f64::from(tuning_cents));
    let frame_count = if samples.is_empty() {
        0
    } else {
        samples
            .len()
            .saturating_sub(frame_length)
            .div_ceil(hop_length)
            + 1
    };
    let transform = FftPlanner::<f32>::new().plan_fft_forward(frame_length);
    let window = blackman_window(frame_length);
    let mut frame = vec![Complex::default(); frame_length];
    let mut scratch = vec![Complex::default(); transform.get_inplace_scratch_len()];
    let mut spectrum = vec![0.0_f32; frame_length / 2];
    let mut notes = [0.0_f32; BAND_COUNT];
    let mut totals = [0.0_f64; BAND_COUNT];
    for frame_index in 0..frame_count {
        let start = frame_index * hop_length;
        let available = &samples[start..samples.len().min(start + frame_length)];
        for (index, slot) in frame.iter_mut().enumerate() {
            let sample = available.get(index).copied().unwrap_or(0.0);
            *slot = Complex::new(sample * window[index], 0.0);
        }
        transform.process_with_scratch(&mut frame, &mut scratch);
        for (magnitude, value) in spectrum.iter_mut().zip(&frame) {
            *magnitude = value.norm();
        }
        for (note, kernel) in notes.iter_mut().zip(&kernels) {
            *note = kernel
                .weights
                .iter()
                .zip(&spectrum[kernel.first_bin..])
                .map(|(weight, magnitude)| weight * magnitude)
                .sum();
        }
        for (total, level) in totals.iter_mut().zip(above_floor(&notes)) {
            *total += f64::from(level);
        }
    }
    totals
}

/// The 12 notes, every octave added together, C first.
fn folded(bands: &[f64; BAND_COUNT]) -> [f64; SEMITONES] {
    let mut pitch_classes = [0.0_f64; SEMITONES];
    for (band, level) in bands.iter().enumerate() {
        pitch_classes[band % SEMITONES] += level;
    }
    pitch_classes
}

/// Each note less the median of the notes around it, and no less than zero.
fn above_floor(notes: &[f32; BAND_COUNT]) -> [f32; BAND_COUNT] {
    let mut neighbours = Vec::with_capacity(2 * FLOOR_HALF_WIDTH_SEMITONES + 1);
    std::array::from_fn(|band| {
        let first = band.saturating_sub(FLOOR_HALF_WIDTH_SEMITONES);
        let last = (band + FLOOR_HALF_WIDTH_SEMITONES).min(BAND_COUNT - 1);
        neighbours.clear();
        neighbours.extend_from_slice(&notes[first..=last]);
        let middle = neighbours.len() / 2;
        let (_, median, _) = neighbours.select_nth_unstable_by(middle, f32::total_cmp);
        (notes[band] - *median).max(0.0)
    })
}

/// The weights one note gives the spectrum bins from `first_bin` on.
#[derive(Debug, Clone)]
struct NoteKernel {
    first_bin: usize,
    weights: Vec<f32>,
}

fn note_kernels(analysis_rate: f64, frame_length: usize, tuning_cents: f64) -> Vec<NoteKernel> {
    let bin_width = analysis_rate / frame_length as f64;
    let last_bin = frame_length / 2 - 1;
    (0..BAND_COUNT)
        .map(|band| {
            let centre_hz =
                LOWEST_NOTE_HZ * 2.0_f64.powf((band as f64 * 100.0 + tuning_cents) / 1_200.0);
            let edge_ratio = 2.0_f64.powf(NOTE_HALF_WIDTH_CENTS / 1_200.0);
            let first_bin =
                ((centre_hz / edge_ratio / bin_width).ceil() as usize).clamp(1, last_bin);
            let last_kernel_bin =
                ((centre_hz * edge_ratio / bin_width).floor() as usize).clamp(first_bin, last_bin);
            let weights = (first_bin..=last_kernel_bin)
                .map(|bin| {
                    let cents = 1_200.0 * (bin as f64 * bin_width / centre_hz).log2();
                    let phase = cents / NOTE_HALF_WIDTH_CENTS;
                    if phase.abs() >= 1.0 {
                        0.0
                    } else {
                        (0.5 + 0.5 * (std::f64::consts::PI * phase).cos()) as f32
                    }
                })
                .collect();
            NoteKernel { first_bin, weights }
        })
        .collect()
}

fn frame_length(analysis_rate: f64) -> usize {
    let longest = (FRAME_SECONDS * analysis_rate) as usize;
    (longest.max(2) + 1).next_power_of_two() / 2
}

fn blackman_window(length: usize) -> Vec<f32> {
    (0..length)
        .map(|index| {
            let phase = TAU * index as f64 / length as f64;
            (0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos()) as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;
    use crate::detect::synthesis::{SAMPLE_RATE, buffer, chord_progression, mixed, pink_noise};
    use crate::key::Key;

    fn key(text: &str) -> Key {
        text.parse().unwrap()
    }

    fn strongest(profile: &[f32; SEMITONES], count: usize) -> Vec<usize> {
        let mut ranked: Vec<usize> = (0..SEMITONES).collect();
        ranked.sort_by(|left, right| profile[*right].total_cmp(&profile[*left]));
        let mut top = ranked[..count].to_vec();
        top.sort_unstable();
        top
    }

    #[test]
    fn an_a_minor_chord_lights_a_c_and_e() {
        for sample_rate in [22_050, 44_100, 48_000, 96_000] {
            let samples: Vec<f32> = (0..sample_rate * 6)
                .map(|index| {
                    let time = index as f32 / sample_rate as f32;
                    [220.0_f32, 261.63, 329.63]
                        .iter()
                        .map(|frequency| 0.2 * (std::f32::consts::TAU * frequency * time).sin())
                        .sum()
                })
                .collect();
            let audio = AudioBuffer::new(samples, NonZeroU32::new(sample_rate as u32).unwrap());
            let profile = pitch_class_profile(&audio, 0.0).unwrap();
            assert_eq!(
                strongest(&profile, 3),
                [0, 4, 9],
                "{sample_rate} Hz: {profile:?}"
            );
            assert_eq!(profile.iter().copied().fold(0.0, f32::max), 1.0);
        }
    }

    #[test]
    fn the_notes_of_a_progression_stand_out_under_noise() {
        let chords = chord_progression(key("A minor"), 0.0, 24.0);
        let noisy = mixed(&chords, &pink_noise(chords.len(), 0.5));
        let profile = pitch_class_profile(&buffer(noisy), 0.0).unwrap();
        let harmonic_minor_scale = [9, 11, 0, 2, 4, 5, 8];
        let weakest_in_scale = harmonic_minor_scale
            .iter()
            .map(|pitch_class| profile[*pitch_class])
            .fold(f32::MAX, f32::min);
        let strongest_outside = (0..SEMITONES)
            .filter(|pitch_class| !harmonic_minor_scale.contains(pitch_class))
            .map(|pitch_class| profile[pitch_class])
            .fold(0.0, f32::max);
        assert!(weakest_in_scale > strongest_outside, "{profile:?}");
    }

    #[test]
    fn a_track_a_quarter_tone_sharp_is_heard_on_its_own_scale() {
        let audio = buffer(chord_progression(key("C major"), 45.0, 24.0));
        let on_its_scale = pitch_class_profile(&audio, 45.0).unwrap();
        let major_scale = [0, 2, 4, 5, 7, 9, 11];
        assert_eq!(strongest(&on_its_scale, 7), major_scale, "{on_its_scale:?}");
        let at_concert_pitch = pitch_class_profile(&audio, 0.0).unwrap();
        assert_ne!(strongest(&at_concert_pitch, 7), major_scale);
    }

    #[test]
    fn silence_and_no_samples_have_no_profile() {
        assert_eq!(
            pitch_class_profile(&buffer(vec![0.0; 3 * SAMPLE_RATE as usize]), 0.0),
            None
        );
        assert_eq!(pitch_class_profile(&buffer(Vec::new()), 0.0), None);
    }

    #[test]
    fn audio_shorter_than_a_frame_is_measured() {
        let audio = buffer(chord_progression(key("C major"), 0.0, 1.0));
        assert!(pitch_class_profile(&audio, 0.0).is_some());
    }

    #[test]
    fn frames_are_the_largest_power_of_two_within_three_seconds() {
        assert_eq!(frame_length(5_512.5), 16_384);
        assert_eq!(frame_length(6_000.0), 16_384);
        assert_eq!(frame_length(8_000.0), 16_384);
        assert_eq!(frame_length(5_000.0), 8_192);
    }
}
