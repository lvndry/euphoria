// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Brings decoded mono audio to what drone-v1 was trained on: drone has
//! ffmpeg convert each file to 16 bit mono at 22,050 Hz. The resampler
//! here builds the filter of ffmpeg's default resampler (a Kaiser windowed
//! sinc, 32 taps per unit of bandwidth, cutoff at 97% of the lower Nyquist
//! frequency, Kaiser beta 9, one phase per step of an exact rational ratio),
//! and the samples are rounded to 16 bits as ffmpeg writes them.

use std::f64::consts::PI;

use super::cqt::SAMPLE_RATE;
use super::halving::bessel_i0;

/// ffmpeg's default filter size, in taps per unit of relative bandwidth.
const FILTER_SIZE: f64 = 32.0;
/// ffmpeg's default cutoff, as a share of the lower Nyquist frequency.
const CUTOFF: f64 = 0.97;
/// ffmpeg's default Kaiser window parameter.
const KAISER_BETA: f64 = 9.0;
/// ffmpeg's largest number of filter phases.
const MAXIMUM_PHASES: u64 = 1_024;

/// Mono samples at `sample_rate` converted to [`SAMPLE_RATE`] and 16 bits.
pub(super) fn to_model_input(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    let converted = if sample_rate == SAMPLE_RATE {
        samples.to_vec()
    } else {
        resample(samples, u64::from(sample_rate), u64::from(SAMPLE_RATE))
    };
    converted.into_iter().map(to_16_bits).collect()
}

/// Rounds to the nearest 16 bit sample, ties to even, clipping at full scale.
fn to_16_bits(sample: f32) -> f32 {
    let scaled = (f64::from(sample) * 32_768.0).round_ties_even();
    (scaled.clamp(-32_768.0, 32_767.0) / 32_768.0) as f32
}

fn greatest_common_divisor(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        (left, right) = (right, left % right);
    }
    left
}

/// ffmpeg's polyphase resampler, centred on each output's position.
fn resample(samples: &[f32], input_rate: u64, output_rate: u64) -> Vec<f32> {
    let divisor = greatest_common_divisor(input_rate, output_rate);
    let exact_phases = output_rate / divisor;
    let step = input_rate / divisor;
    let phase_count = exact_phases.min(MAXIMUM_PHASES);
    let factor = (output_rate as f64 * CUTOFF / input_rate as f64).min(1.0);
    let mut tap_count = ((FILTER_SIZE / factor).ceil() as usize).max(1);
    if tap_count > 1 {
        tap_count += tap_count % 2;
    }
    let centre = (tap_count as isize - 1) / 2;
    let phase_taps = |phase: u64| -> Vec<f64> {
        (0..tap_count)
            .map(|index| {
                let distance = (index as isize - centre) as f64 - phase as f64 / phase_count as f64;
                let angle = PI * distance * factor;
                let sinc = if angle == 0.0 { 1.0 } else { angle.sin() / angle };
                let position = 2.0 * angle / (factor * tap_count as f64 * PI);
                sinc * bessel_i0(KAISER_BETA * (1.0 - position * position).max(0.0).sqrt())
            })
            .collect()
    };
    let norm: f64 = phase_taps(0).iter().sum();
    let bank: Vec<Vec<f32>> = (0..phase_count)
        .map(|phase| {
            phase_taps(phase)
                .into_iter()
                .map(|tap| (tap / norm) as f32)
                .collect()
        })
        .collect();

    let output_count = (samples.len() as u64 * exact_phases).div_ceil(step) as usize;
    (0..output_count as u64)
        .map(|output_index| {
            let numerator = output_index * step;
            let whole = (numerator / exact_phases) as isize;
            let remainder = numerator % exact_phases;
            let phase = remainder * phase_count / exact_phases;
            let taps = &bank[phase as usize];
            let first = whole - centre;
            if first >= 0 && first as usize + tap_count <= samples.len() {
                let window = &samples[first as usize..first as usize + tap_count];
                window.iter().zip(taps).map(|(sample, tap)| sample * tap).sum()
            } else {
                taps.iter()
                    .enumerate()
                    .filter_map(|(index, tap)| {
                        let position = usize::try_from(first + index as isize).ok()?;
                        samples.get(position).map(|sample| sample * tap)
                    })
                    .sum()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frequency: f64, rate: u32, seconds: f64) -> Vec<f32> {
        (0..(f64::from(rate) * seconds) as usize)
            .map(|index| (2.0 * PI * frequency * index as f64 / f64::from(rate)).sin() as f32 * 0.5)
            .collect()
    }

    fn level(samples: &[f32]) -> f64 {
        let middle = &samples[samples.len() / 4..samples.len() * 3 / 4];
        (middle.iter().map(|sample| f64::from(*sample).powi(2)).sum::<f64>() / middle.len() as f64)
            .sqrt()
    }

    #[test]
    fn rates_convert_to_the_model_rate_with_the_expected_length() {
        for rate in [8_000, 22_050, 44_100, 48_000, 96_000, 44_056] {
            let converted = to_model_input(&tone(440.0, rate, 1.0), rate);
            assert!(converted.len().abs_diff(22_050) <= 1, "{rate}: {}", converted.len());
        }
    }

    #[test]
    fn a_tone_in_the_passband_keeps_its_level_and_one_above_is_removed() {
        for rate in [44_100, 48_000] {
            let kept = to_model_input(&tone(1_000.0, rate, 1.0), rate);
            assert!((level(&kept) - 0.5 / 2.0_f64.sqrt()).abs() < 1e-3, "{rate}");
            let removed = to_model_input(&tone(15_000.0, rate, 1.0), rate);
            assert!(level(&removed) < 1e-3, "{rate}");
        }
    }

    #[test]
    fn samples_are_rounded_to_16_bits() {
        assert_eq!(to_16_bits(0.5), 0.5);
        assert_eq!(to_16_bits(1.0), 32_767.0 / 32_768.0);
        assert_eq!(to_16_bits(-2.0), -1.0);
        assert_eq!(to_16_bits(1.4 / 32_768.0), 1.0 / 32_768.0);
    }
}
