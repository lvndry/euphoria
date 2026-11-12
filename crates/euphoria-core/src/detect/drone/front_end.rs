// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Brings decoded mono audio to what drone-v1 was trained on: drone has
//! ffmpeg convert each file to 16 bit mono at 22,050 Hz. The resampler
//! here builds the filter of ffmpeg's default resampler (a Kaiser windowed
//! sinc, 32 taps per unit of bandwidth, cutoff at 97% of the lower Nyquist
//! frequency, Kaiser beta 9, one phase per step of an exact rational ratio),
//! and the samples are rounded to 16 bits as ffmpeg writes them.

use std::f64::consts::PI;
use std::ops::Range;

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

/// Converts mono audio at one sample rate to [`SAMPLE_RATE`] and 16 bits,
/// any stretch of the output at a time: each output sample is computed from
/// the whole input, so a stretch is the same as that part of a conversion
/// of the whole.
pub(super) struct Converter {
    exact_phases: u64,
    step: u64,
    phase_count: u64,
    tap_count: usize,
    centre: isize,
    bank: Vec<Vec<f32>>,
}

impl Converter {
    /// A converter from `sample_rate`, with ffmpeg's filter for that ratio.
    pub(super) fn new(sample_rate: u32) -> Self {
        let input_rate = u64::from(sample_rate);
        let output_rate = u64::from(SAMPLE_RATE);
        let divisor = greatest_common_divisor(input_rate, output_rate);
        let exact_phases = output_rate / divisor;
        let step = input_rate / divisor;
        if exact_phases == step {
            return Self {
                exact_phases,
                step,
                phase_count: 1,
                tap_count: 1,
                centre: 0,
                bank: vec![vec![1.0]],
            };
        }
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
                    let distance =
                        (index as isize - centre) as f64 - phase as f64 / phase_count as f64;
                    let angle = PI * distance * factor;
                    let sinc = if angle == 0.0 {
                        1.0
                    } else {
                        angle.sin() / angle
                    };
                    let position = 2.0 * angle / (factor * tap_count as f64 * PI);
                    sinc * bessel_i0(KAISER_BETA * (1.0 - position * position).max(0.0).sqrt())
                })
                .collect()
        };
        let norm: f64 = phase_taps(0).iter().sum();
        let bank = (0..phase_count)
            .map(|phase| {
                phase_taps(phase)
                    .into_iter()
                    .map(|tap| (tap / norm) as f32)
                    .collect()
            })
            .collect();
        Self {
            exact_phases,
            step,
            phase_count,
            tap_count,
            centre,
            bank,
        }
    }

    /// How many output samples `input_length` input samples give.
    pub(super) fn output_length(&self, input_length: usize) -> usize {
        (input_length as u64 * self.exact_phases).div_ceil(self.step) as usize
    }

    /// The output samples in `outputs`, which must lie within
    /// [`Converter::output_length`] of the input.
    pub(super) fn convert(&self, samples: &[f32], outputs: Range<usize>) -> Vec<f32> {
        outputs
            .map(|output_index| {
                let numerator = output_index as u64 * self.step;
                let whole = (numerator / self.exact_phases) as isize;
                let remainder = numerator % self.exact_phases;
                let phase = remainder * self.phase_count / self.exact_phases;
                let taps = &self.bank[phase as usize];
                let first = whole - self.centre;
                let value: f32 = if first >= 0 && first as usize + self.tap_count <= samples.len() {
                    let window = &samples[first as usize..first as usize + self.tap_count];
                    window
                        .iter()
                        .zip(taps)
                        .map(|(sample, tap)| sample * tap)
                        .sum()
                } else {
                    taps.iter()
                        .enumerate()
                        .filter_map(|(index, tap)| {
                            let position = usize::try_from(first + index as isize).ok()?;
                            samples.get(position).map(|sample| sample * tap)
                        })
                        .sum()
                };
                to_16_bits(value)
            })
            .collect()
    }
}

/// Mono samples at `sample_rate` converted to [`SAMPLE_RATE`] and 16 bits.
#[cfg(test)]
fn to_model_input(samples: &[f32], sample_rate: u32) -> Vec<f32> {
    let converter = Converter::new(sample_rate);
    converter.convert(samples, 0..converter.output_length(samples.len()))
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
        (middle
            .iter()
            .map(|sample| f64::from(*sample).powi(2))
            .sum::<f64>()
            / middle.len() as f64)
            .sqrt()
    }

    #[test]
    fn rates_convert_to_the_model_rate_with_the_expected_length() {
        for rate in [8_000, 22_050, 44_100, 48_000, 96_000, 44_056] {
            let converted = to_model_input(&tone(440.0, rate, 1.0), rate);
            assert!(
                converted.len().abs_diff(22_050) <= 1,
                "{rate}: {}",
                converted.len()
            );
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
    fn a_stretch_of_the_output_matches_the_whole() {
        for rate in [44_100, 48_000, 22_050] {
            let samples = tone(997.0, rate, 0.5);
            let converter = Converter::new(rate);
            let whole = converter.convert(&samples, 0..converter.output_length(samples.len()));
            assert_eq!(
                converter.convert(&samples, 1_000..3_000),
                whole[1_000..3_000]
            );
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
