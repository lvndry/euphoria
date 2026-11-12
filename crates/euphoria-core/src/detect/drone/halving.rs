// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Halves the sample rate the way the drone-v1 features were made: librosa's
//! constant-Q transform halves the rate once per octave with libsoxr at its
//! high quality setting, which for an exact factor of two is one linear
//! phase low pass followed by keeping every other sample.
//!
//! The low pass is designed here with libsoxr's own formulas, so the taps
//! match it to rounding, and applied by overlap-save convolution with an FFT.

use std::f64::consts::PI;
use std::sync::{Arc, OnceLock};

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

/// Bits of precision of libsoxr's high quality setting.
const PRECISION_BITS: f64 = 20.0;

/// The FFT length of each overlap-save block.
const BLOCK_LENGTH: usize = 4_096;

/// The low pass of libsoxr's high quality setting for an exact halving,
/// with unit gain at zero frequency, and its FFT for overlap-save.
struct HalvingFilter {
    tap_count: usize,
    spectrum: Vec<Complex<f32>>,
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
}

fn filter() -> &'static HalvingFilter {
    static FILTER: OnceLock<HalvingFilter> = OnceLock::new();
    FILTER.get_or_init(|| {
        let taps = soxr_halving_taps();
        let mut planner = FftPlanner::new();
        let forward = planner.plan_fft_forward(BLOCK_LENGTH);
        let inverse = planner.plan_fft_inverse(BLOCK_LENGTH);
        let scale = 1.0 / BLOCK_LENGTH as f64;
        let mut spectrum: Vec<Complex<f32>> = (0..BLOCK_LENGTH)
            .map(|index| {
                let tap = taps.get(index).map_or(0.0, |tap| tap * scale);
                Complex::new(tap as f32, 0.0)
            })
            .collect();
        forward.process(&mut spectrum);
        HalvingFilter {
            tap_count: taps.len(),
            spectrum,
            forward,
            inverse,
        }
    })
}

/// Keeps every other sample after the low pass, centred so that output
/// sample `n` sits on input sample `2n`. The audio outside the slice counts
/// as silence, and there are `samples.len().div_ceil(2)` outputs, as librosa
/// asks of libsoxr.
pub(super) fn halve(samples: &[f32]) -> Vec<f32> {
    let output_count = samples.len().div_ceil(2);
    if output_count == 0 {
        return Vec::new();
    }
    let filter = filter();
    let delay = (filter.tap_count - 1) / 2;
    let overlap = filter.tap_count - 1;
    let valid_per_block = BLOCK_LENGTH - overlap;
    let first_needed = delay;
    let last_needed = delay + 2 * (output_count - 1);
    let input_at = |position: isize| -> f32 {
        usize::try_from(position)
            .ok()
            .and_then(|index| samples.get(index))
            .copied()
            .unwrap_or(0.0)
    };

    let mut output = vec![0.0_f32; output_count];
    let mut buffer = vec![Complex::new(0.0_f32, 0.0); BLOCK_LENGTH];
    let mut scratch = vec![
        Complex::new(0.0_f32, 0.0);
        filter
            .forward
            .get_inplace_scratch_len()
            .max(filter.inverse.get_inplace_scratch_len())
    ];
    let mut block_start = first_needed;
    while block_start <= last_needed {
        let paired_start = block_start + valid_per_block;
        for (offset, value) in buffer.iter_mut().enumerate() {
            let input_offset = offset as isize - overlap as isize;
            *value = Complex::new(
                input_at(block_start as isize + input_offset),
                input_at(paired_start as isize + input_offset),
            );
        }
        filter
            .forward
            .process_with_scratch(&mut buffer, &mut scratch);
        for (value, response) in buffer.iter_mut().zip(&filter.spectrum) {
            *value *= response;
        }
        filter
            .inverse
            .process_with_scratch(&mut buffer, &mut scratch);
        for (start, part) in [(block_start, 0), (paired_start, 1)] {
            for (offset, value) in buffer[overlap..].iter().enumerate() {
                let convolved_index = start + offset;
                if convolved_index < first_needed
                    || convolved_index > last_needed
                    || !(convolved_index - delay).is_multiple_of(2)
                {
                    continue;
                }
                output[(convolved_index - delay) / 2] = if part == 0 { value.re } else { value.im };
            }
        }
        block_start += 2 * valid_per_block;
    }
    output
}

/// libsoxr's linear phase low pass for halving at the high quality setting:
/// the passband ends where libsoxr's `soxr_quality_spec` puts it, the
/// stopband begins at the output Nyquist frequency, and the Kaiser window
/// reaches the attenuation of 20 bits plus one.
fn soxr_halving_taps() -> Vec<f64> {
    let rejection = PRECISION_BITS * linear_to_decibels(2.0);
    let passband_end = 1.0 - 0.05 / (1.0 - inverse_frequency_response(-3.0, rejection));
    let attenuation = (PRECISION_BITS + 1.0) * linear_to_decibels(2.0);
    let nyquist = 2.0;
    let modulo = 4;

    let passband = passband_end / nyquist;
    let stopband = 1.0 / nyquist;
    let transition = (0.5 * (stopband - passband)).min(0.5 * stopband);
    let cutoff = stopband - transition;
    let beta = kaiser_beta(attenuation, transition * 0.5 / cutoff);
    let length_factor = ((0.000_752_835_8 - 1.577_737e-5 * beta) * beta + 0.624_802_2) * beta
        + 0.061_869_02;
    let estimated = (length_factor / transition + 1.0).ceil() as usize;
    let tap_count = (estimated + modulo - 2) / modulo * modulo + 1;

    let last = (tap_count - 1) as f64;
    let window_scale = 1.0 / bessel_i0(beta);
    let position_scale = 1.0 / (0.5 * last + 0.5);
    (0..tap_count)
        .map(|index| {
            let offset = index as f64 - 0.5 * last;
            let angle = offset * PI;
            let sinc = if angle == 0.0 {
                cutoff
            } else {
                (cutoff * angle).sin() / angle
            };
            let position = offset * position_scale;
            sinc * bessel_i0(beta * (1.0 - position * position).sqrt()) * window_scale
        })
        .collect()
}

fn linear_to_decibels(ratio: f64) -> f64 {
    ratio.log10() * 20.0
}

/// libsoxr's estimate of where its filters fall by `drop` decibels.
fn inverse_frequency_response(drop: f64, attenuation: f64) -> f64 {
    let phi = ((2.0517e-7 * attenuation - 1.1303e-4) * attenuation + 0.023_154) * attenuation
        + 0.559_24;
    let power = 0.5_f64.ln() / (phi * 0.5).sin().ln();
    let level = (drop * (std::f64::consts::LN_10 * 0.05)).exp();
    let part = if level > 0.5 { 1.0 - level } else { level };
    let position = part.powf(1.0 / power).asin() / phi;
    if level > 0.5 {
        position
    } else {
        1.0 - position
    }
}

/// libsoxr's fitted Kaiser window parameter for an attenuation of 60 dB or
/// more and a transition band width relative to the cutoff.
fn kaiser_beta(attenuation: f64, transition: f64) -> f64 {
    const COEFFICIENTS: [[f64; 4]; 10] = [
        [-6.784957e-10, 1.02856e-05, 0.108_755_6, -0.898_836_5 + 0.001],
        [-6.897885e-10, 1.027433e-05, 0.10876, -0.899_465_8 + 0.002],
        [-1.000683e-09, 1.030092e-05, 0.108_767_7, -0.900_789_8 + 0.003],
        [-3.654474e-10, 1.040631e-05, 0.108_708_5, -0.897_776_6 + 0.006],
        [8.106988e-09, 6.983091e-06, 0.109_138_7, -0.917_204_8 + 0.015],
        [9.519571e-09, 7.272678e-06, 0.109_006_8, -0.914_076_8 + 0.025],
        [-5.626821e-09, 1.342186e-05, 0.108_399_9, -0.906_545_2 + 0.05],
        [-9.965946e-08, 5.073548e-05, 0.104_096_7, -0.767_277_8 + 0.085],
        [1.604808e-07, -5.856462e-05, 0.118_599_8, -1.34824 + 0.1],
        [-1.511964e-07, 6.363034e-05, 0.106_462_7, -0.987_666_5 + 0.18],
    ];
    let position = (transition / 0.0005).ln() / 2.0_f64.ln();
    let row = |index: i64| COEFFICIENTS[index.clamp(0, COEFFICIENTS.len() as i64 - 1) as usize];
    let whole = position as i64;
    let evaluate =
        |coefficients: [f64; 4]| -> f64 {
            ((coefficients[0] * attenuation + coefficients[1]) * attenuation + coefficients[2])
                * attenuation
                + coefficients[3]
        };
    let low = evaluate(row(whole));
    let high = evaluate(row(whole + 1));
    low + (high - low) * (position - whole as f64)
}

/// The modified Bessel function of the first kind of order zero, by its
/// power series, which converges for every argument the Kaiser window uses.
pub(super) fn bessel_i0(argument: f64) -> f64 {
    let quarter_square = argument * argument / 4.0;
    let mut term = 1.0;
    let mut sum = 1.0;
    for order in 1..500 {
        term *= quarter_square / (order * order) as f64;
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    fn direct_halving(samples: &[f32]) -> Vec<f64> {
        let taps = soxr_halving_taps();
        let delay = (taps.len() - 1) / 2;
        (0..samples.len().div_ceil(2))
            .map(|output_index| {
                taps.iter()
                    .enumerate()
                    .filter_map(|(tap_index, tap)| {
                        let position = (2 * output_index + delay).checked_sub(tap_index)?;
                        samples.get(position).map(|sample| tap * f64::from(*sample))
                    })
                    .sum()
            })
            .collect()
    }

    #[test]
    fn the_filter_has_the_length_and_gain_of_libsoxr() {
        let taps = soxr_halving_taps();
        assert_eq!(taps.len(), 389);
        let gain: f64 = taps.iter().sum();
        assert!((gain - 1.0).abs() < 1e-6, "{gain}");
        assert!((taps[194] - 0.478_435_913_318_326_7).abs() < 1e-12);
    }

    #[test]
    fn the_fft_convolution_matches_the_direct_one() {
        for length in [1_usize, 2, 7, 388, 389, 5_000, 20_001] {
            let samples: Vec<f32> = (0..length)
                .map(|index| ((index * 7_919) % 1_000) as f32 / 1_000.0 - 0.5)
                .collect();
            let fast = halve(&samples);
            let direct = direct_halving(&samples);
            assert_eq!(fast.len(), length.div_ceil(2));
            for (fast_value, direct_value) in fast.iter().zip(&direct) {
                assert!((f64::from(*fast_value) - direct_value).abs() < 2e-6);
            }
        }
    }

    #[test]
    fn bessel_matches_known_values() {
        assert!((bessel_i0(0.0) - 1.0).abs() < 1e-15);
        assert!((bessel_i0(1.0) - 1.266_065_877_752_008_4).abs() < 1e-14);
        assert!((bessel_i0(13.0) / 49_444.489_582_217_575 - 1.0).abs() < 1e-12);
    }
}
