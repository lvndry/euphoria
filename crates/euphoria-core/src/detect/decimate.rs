// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Lowers the sample rate of audio by a whole factor.

use std::f64::consts::{PI, TAU};

/// Filter taps on each side of the centre tap, per unit of decimation factor.
const HALF_LENGTH_PER_FACTOR: usize = 16;

/// Where the low pass reaches half amplitude, as a fraction of the output
/// sample rate. The filter is flat below 0.35 and attenuates everything above
/// 0.6 by more than 50 dB, so what aliasing remains lands above 0.4 of the
/// output rate.
const CUTOFF_FRACTION_OF_OUTPUT_RATE: f64 = 0.45;

/// Keeps every `factor`th sample after a low pass that removes what the lower
/// sample rate cannot represent. The audio outside the slice counts as
/// silence. A factor of one returns the samples unchanged.
pub(crate) fn decimate(samples: &[f32], factor: usize) -> Vec<f32> {
    if factor <= 1 {
        return samples.to_vec();
    }
    let taps = low_pass_taps(factor);
    let half_length = taps.len() / 2;
    (0..samples.len().div_ceil(factor))
        .map(|output_index| {
            let centre = output_index * factor;
            let first_sample = centre.saturating_sub(half_length);
            let last_sample = (centre + half_length).min(samples.len() - 1);
            let first_tap = first_sample + half_length - centre;
            let window = &samples[first_sample..=last_sample];
            window
                .iter()
                .zip(&taps[first_tap..])
                .map(|(sample, tap)| sample * tap)
                .sum()
        })
        .collect()
}

/// A Blackman windowed sinc low pass with unit gain at zero frequency.
fn low_pass_taps(factor: usize) -> Vec<f32> {
    let half_length = HALF_LENGTH_PER_FACTOR * factor;
    let cutoff = CUTOFF_FRACTION_OF_OUTPUT_RATE / factor as f64;
    let taps: Vec<f64> = (0..=2 * half_length)
        .map(|index| {
            let offset = index as f64 - half_length as f64;
            let sinc = if offset == 0.0 {
                2.0 * cutoff
            } else {
                (TAU * cutoff * offset).sin() / (PI * offset)
            };
            let phase = TAU * index as f64 / (2 * half_length) as f64;
            let window = 0.42 - 0.5 * phase.cos() + 0.08 * (2.0 * phase).cos();
            sinc * window
        })
        .collect();
    let gain: f64 = taps.iter().sum();
    taps.iter().map(|tap| (tap / gain) as f32).collect()
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;

    const INPUT_RATE: f32 = 44_100.0;

    fn tone(frequency: f32, sample_count: usize) -> Vec<f32> {
        (0..sample_count)
            .map(|index| (TAU * frequency * index as f32 / INPUT_RATE).sin())
            .collect()
    }

    /// Amplitude of a sine wave, measured away from the edges, where the
    /// filter sees real audio on both sides.
    fn settled_amplitude(samples: &[f32]) -> f32 {
        let margin = samples.len() / 4;
        let settled = &samples[margin..samples.len() - margin];
        let mean_square =
            settled.iter().map(|sample| sample * sample).sum::<f32>() / settled.len() as f32;
        (2.0 * mean_square).sqrt()
    }

    #[test]
    fn a_factor_of_one_returns_the_samples_unchanged() {
        let samples = tone(440.0, 1_000);
        assert_eq!(decimate(&samples, 1), samples);
    }

    #[test]
    fn output_length_is_the_input_length_divided_by_the_factor_rounded_up() {
        assert_eq!(decimate(&[0.5; 1_000], 2).len(), 500);
        assert_eq!(decimate(&[0.5; 1_001], 2).len(), 501);
        assert_eq!(decimate(&[0.5; 1_001], 4).len(), 251);
        assert_eq!(decimate(&[0.5; 3], 4).len(), 1);
        assert!(decimate(&[], 2).is_empty());
    }

    #[test]
    fn a_constant_signal_keeps_its_level() {
        for factor in [2, 3, 4, 8] {
            let decimated = decimate(&[0.5; 8_000], factor);
            let settled = &decimated[100..decimated.len() - 100];
            for sample in settled {
                assert!((sample - 0.5).abs() < 1.0e-4, "factor {factor}: {sample}");
            }
        }
    }

    #[test]
    fn tones_the_lower_rate_can_carry_keep_their_level() {
        for factor in [2, 4] {
            let output_rate = INPUT_RATE / factor as f32;
            for fraction in [0.01, 0.1, 0.25, 0.35] {
                let decimated = decimate(&tone(fraction * output_rate, 44_100), factor);
                let amplitude = settled_amplitude(&decimated);
                assert!(
                    (amplitude - 1.0).abs() < 0.02,
                    "factor {factor}, {fraction} of the output rate: amplitude {amplitude}"
                );
            }
        }
    }

    #[test]
    fn tones_the_lower_rate_cannot_carry_are_removed() {
        let fifty_decibels_down = 3.0e-3;
        for factor in [2, 4] {
            let output_rate = INPUT_RATE / factor as f32;
            for fraction in [0.6, 0.75, 0.95] {
                let decimated = decimate(&tone(fraction * output_rate, 44_100), factor);
                let amplitude = settled_amplitude(&decimated);
                assert!(
                    amplitude < fifty_decibels_down,
                    "factor {factor}, {fraction} of the output rate: amplitude {amplitude}"
                );
            }
        }
    }

    #[test]
    fn a_decimated_tone_keeps_its_frequency() {
        let frequency = 1_000.0;
        let decimated = decimate(&tone(frequency, 44_100), 2);
        let output_rate = INPUT_RATE / 2.0;
        let margin = 200;
        for (index, sample) in decimated
            .iter()
            .enumerate()
            .take(decimated.len() - margin)
            .skip(margin)
        {
            let expected = (TAU * frequency * index as f32 / output_rate).sin();
            assert!((sample - expected).abs() < 0.01, "sample {index}");
        }
    }
}
