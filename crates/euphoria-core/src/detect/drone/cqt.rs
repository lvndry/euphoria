// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The drone-v1 features: librosa's constant-Q transform (version 1.0, as
//! drone calls it) with 24 bins per octave, 208 bins from four semitones
//! below C1, a hop of 4096 samples at 22,050 Hz, and the log of one plus the
//! magnitude, rounded to half precision as drone stores it.
//!
//! librosa computes the top two octaves at the full rate and each lower one
//! after halving the rate again, with filters made for that rate. Every step
//! here follows it: the filter bank, its sparsification, the centred frames,
//! the halving filter of libsoxr and the scaling by filter length.

use std::f64::consts::TAU;
use std::sync::OnceLock;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use super::halving::halve;

/// The sample rate the features are computed at.
pub(super) const SAMPLE_RATE: u32 = 22_050;
/// Samples between the starts of two frames at [`SAMPLE_RATE`].
pub(super) const HOP: usize = 4_096;
/// Bins in each octave, two per semitone.
const BINS_PER_OCTAVE: usize = 24;
/// Bins computed, including the four semitones of padding at either end
/// that drone uses to shift pitch in training.
const TOTAL_BINS: usize = 208;
/// Bins the network reads: eight octaves from C1.
pub(super) const CORE_BINS: usize = 192;
/// The first bin the network reads, C1.
const FIRST_CORE_BIN: usize = 8;
/// The share of each filter's spectrum, by magnitude, that librosa drops.
const SPARSITY_QUANTILE: f64 = 0.01;
/// Octaves computed separately, the last one partial.
const OCTAVE_COUNT: usize = TOTAL_BINS.div_ceil(BINS_PER_OCTAVE);

/// The features of a track: [`CORE_BINS`] rows from C1 upwards, one column
/// per frame, stored row after row.
pub(super) struct Features {
    pub(super) values: Vec<f32>,
    pub(super) frame_count: usize,
}

impl Features {
    /// The values of one bin over time.
    #[cfg(test)]
    pub(super) fn row(&self, bin: usize) -> &[f32] {
        &self.values[bin * self.frame_count..(bin + 1) * self.frame_count]
    }
}

/// The lowest bin centre, four semitones below C1, as drone computes it.
fn lowest_frequency() -> f64 {
    let c1 = 16.351_597_933_828_117 * 2.0_f64.powf(12.0 / 12.0);
    c1 * 2.0_f64.powf(-4.0 / 12.0)
}

/// The centre frequency of every bin, as librosa tiles one octave of ratios.
fn bin_frequencies() -> [f64; TOTAL_BINS] {
    let fmin = lowest_frequency();
    std::array::from_fn(|bin| {
        let octave = 2.0_f64.powf((bin / BINS_PER_OCTAVE) as f64);
        let ratio = 2.0_f64.powf((bin % BINS_PER_OCTAVE) as f64 / BINS_PER_OCTAVE as f64);
        octave * ratio * fmin
    })
}

/// librosa's relative bandwidth of each bin, from the spacing of its
/// neighbours on a log scale.
fn relative_bandwidths(frequencies: &[f64; TOTAL_BINS]) -> [f64; TOTAL_BINS] {
    let logs: Vec<f64> = frequencies.iter().map(|frequency| frequency.log2()).collect();
    std::array::from_fn(|bin| {
        let bins_per_octave = if bin == 0 {
            1.0 / (logs[1] - logs[0])
        } else if bin == TOTAL_BINS - 1 {
            1.0 / (logs[bin] - logs[bin - 1])
        } else {
            2.0 / (logs[bin + 1] - logs[bin - 1])
        };
        let ratio = 2.0_f64.powf(2.0 / bins_per_octave);
        (ratio - 1.0) / (ratio + 1.0)
    })
}

/// The filters of one octave in the frequency domain, kept sparse.
struct OctaveBasis {
    /// The first bin of the octave.
    first_bin: usize,
    /// The FFT length, and the frame length.
    frame_length: usize,
    /// For each bin of the octave, its nonzero FFT coefficients.
    rows: Vec<Vec<(usize, Complex<f64>)>>,
}

/// Everything about the transform that does not depend on the audio.
struct Plan {
    octaves: Vec<OctaveBasis>,
    /// Whether the rate is halved after each octave.
    halves_after: Vec<bool>,
    /// The filter length of each bin at [`SAMPLE_RATE`], which scales its output.
    lengths: [f64; TOTAL_BINS],
}

fn plan() -> &'static Plan {
    static PLAN: OnceLock<Plan> = OnceLock::new();
    PLAN.get_or_init(build_plan)
}

fn build_plan() -> Plan {
    let frequencies = bin_frequencies();
    let bandwidths = relative_bandwidths(&frequencies);
    let full_rate = f64::from(SAMPLE_RATE);
    let lengths = std::array::from_fn(|bin| full_rate / (bandwidths[bin] * frequencies[bin]));
    let mut planner = FftPlanner::new();
    let mut octaves = Vec::with_capacity(OCTAVE_COUNT);
    let mut halves_after = Vec::with_capacity(OCTAVE_COUNT);
    let mut rate = full_rate;
    let mut hop = HOP;
    for octave in 0..OCTAVE_COUNT {
        let end = TOTAL_BINS - octave * BINS_PER_OCTAVE;
        let first_bin = end.saturating_sub(BINS_PER_OCTAVE);
        let bins = first_bin..end;
        let octave_lengths: Vec<f64> = bins
            .clone()
            .map(|bin| rate / (bandwidths[bin] * frequencies[bin]))
            .collect();
        let longest = octave_lengths.iter().copied().fold(0.0, f64::max);
        let frame_length = 1_usize << (longest.log2().ceil() as u32);
        let fft = planner.plan_fft_forward(frame_length);
        let gain = (full_rate / rate).sqrt();
        let rows = bins
            .clone()
            .zip(&octave_lengths)
            .map(|(bin, length)| {
                let mut filter = wavelet(frequencies[bin], rate, *length, frame_length);
                fft.process(&mut filter);
                filter.truncate(frame_length / 2 + 1);
                sparsify(&filter)
                    .into_iter()
                    .map(|(index, value)| (index, value * gain))
                    .collect()
            })
            .collect();
        octaves.push(OctaveBasis {
            first_bin,
            frame_length,
            rows,
        });
        let halves = octave + 1 < OCTAVE_COUNT
            && hop.is_multiple_of(2)
            && frequencies[first_bin - 1] <= rate / 5.0;
        halves_after.push(halves);
        if halves {
            hop /= 2;
            rate /= 2.0;
        }
    }
    Plan {
        octaves,
        halves_after,
        lengths,
    }
}

/// librosa's filter for one bin: a complex sinusoid under a periodic Hann
/// window, normalized to unit sum of magnitudes, centred in the frame, then
/// scaled by its length over the frame length.
fn wavelet(frequency: f64, rate: f64, length: f64, frame_length: usize) -> Vec<Complex<f64>> {
    let start = (-length / 2.0).floor() as i64;
    let end = (length / 2.0).floor() as i64;
    let size = (end - start) as usize;
    let window = |index: usize| 0.5 - 0.5 * (TAU * index as f64 / size as f64).cos();
    let total: f64 = (0..size).map(window).sum();
    let scale = length / frame_length as f64 / total;
    let left_padding = (frame_length - size) / 2;
    let mut filter = vec![Complex::new(0.0, 0.0); frame_length];
    for (index, position) in (start..end).enumerate() {
        let phase = position as f64 * TAU * frequency / rate;
        filter[left_padding + index] = Complex::from_polar(window(index) * scale, phase);
    }
    filter
}

/// librosa's `sparsify_rows`: drops the smallest coefficients that together
/// hold less than [`SPARSITY_QUANTILE`] of the sum of magnitudes.
fn sparsify(coefficients: &[Complex<f64>]) -> Vec<(usize, Complex<f64>)> {
    let magnitudes: Vec<f64> = coefficients.iter().map(|value| value.norm()).collect();
    let total: f64 = magnitudes.iter().sum();
    let mut sorted = magnitudes.clone();
    sorted.sort_by(f64::total_cmp);
    let mut cumulative = 0.0;
    let mut threshold = sorted[0];
    for magnitude in &sorted {
        cumulative += magnitude / total;
        if cumulative >= SPARSITY_QUANTILE {
            threshold = *magnitude;
            break;
        }
    }
    coefficients
        .iter()
        .zip(&magnitudes)
        .enumerate()
        .filter(|(_, (_, magnitude))| **magnitude >= threshold)
        .map(|(index, (value, _))| (index, *value))
        .collect()
}

/// The features of mono audio at [`SAMPLE_RATE`].
pub(super) fn features(samples: &[f32]) -> Features {
    let plan = plan();
    let mut responses: Vec<Vec<f32>> = vec![Vec::new(); TOTAL_BINS];
    let mut octave_samples = samples.to_vec();
    let mut hop = HOP;
    let mut planner = FftPlanner::new();
    for (octave, halves) in plan.octaves.iter().zip(&plan.halves_after) {
        let frame_count = 1 + octave_samples.len() / hop;
        let fft = planner.plan_fft_forward(octave.frame_length);
        let half = octave.frame_length / 2;
        let mut frame = vec![Complex::new(0.0_f64, 0.0); octave.frame_length];
        let mut magnitudes: Vec<Vec<f32>> = vec![Vec::with_capacity(frame_count); octave.rows.len()];
        for frame_index in 0..frame_count {
            let centre = frame_index * hop;
            for (offset, value) in frame.iter_mut().enumerate() {
                let sample = (centre + offset)
                    .checked_sub(half)
                    .and_then(|index| octave_samples.get(index))
                    .copied()
                    .unwrap_or(0.0);
                *value = Complex::new(f64::from(sample), 0.0);
            }
            fft.process(&mut frame);
            for (row, output) in octave.rows.iter().zip(&mut magnitudes) {
                let response: Complex<f64> = row
                    .iter()
                    .map(|(index, coefficient)| coefficient * frame[*index])
                    .sum();
                output.push(response.norm() as f32);
            }
        }
        for (offset, output) in magnitudes.into_iter().enumerate() {
            responses[octave.first_bin + offset] = output;
        }
        if *halves {
            octave_samples = halve(&octave_samples)
                .into_iter()
                .map(|sample| sample * std::f32::consts::SQRT_2)
                .collect();
            hop /= 2;
        }
    }

    let frame_count = responses.iter().map(Vec::len).min().unwrap_or(0);
    let mut values = Vec::with_capacity(CORE_BINS * frame_count);
    for bin in FIRST_CORE_BIN..FIRST_CORE_BIN + CORE_BINS {
        let scale = 1.0 / plan.lengths[bin].sqrt();
        values.extend(
            responses[bin][..frame_count]
                .iter()
                .map(|magnitude| round_to_half_precision((f64::from(*magnitude) * scale).ln_1p() as f32)),
        );
    }
    Features {
        values,
        frame_count,
    }
}

/// Rounds to the nearest value a 16 bit float holds, ties to even, as
/// numpy's conversion to `float16` does for the finite non-negative values
/// the features take.
pub(super) fn round_to_half_precision(value: f32) -> f32 {
    const SMALLEST_NORMAL: f32 = 6.103_515_6e-5;
    const SUBNORMAL_STEP: f32 = 5.960_464_5e-8;
    if !value.is_finite() {
        return value;
    }
    if value.abs() < SMALLEST_NORMAL {
        return (value / SUBNORMAL_STEP).round_ties_even() * SUBNORMAL_STEP;
    }
    let bits = value.to_bits();
    let dropped_bits = 13;
    let lowest_kept = 1 << dropped_bits;
    let round_bit = (bits >> dropped_bits) & 1;
    let rounded = bits.wrapping_add(lowest_kept / 2 - 1 + round_bit) & !(lowest_kept - 1);
    f32::from_bits(rounded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lowest_bin_is_four_semitones_below_c1() {
        assert!((lowest_frequency() - 25.956_543_761_519_793).abs() < 1e-12);
        let frequencies = bin_frequencies();
        assert!((frequencies[FIRST_CORE_BIN] - 32.703_195_867_656_234).abs() < 1e-9);
    }

    #[test]
    fn the_plan_follows_librosa() {
        let plan = plan();
        assert_eq!(plan.octaves.len(), 9);
        let frame_lengths: Vec<usize> = plan.octaves.iter().map(|octave| octave.frame_length).collect();
        assert_eq!(frame_lengths, [256, 512, 512, 512, 512, 512, 512, 512, 256]);
        assert_eq!(
            plan.halves_after,
            [false, true, true, true, true, true, true, true, false]
        );
        assert_eq!(plan.octaves[8].rows.len(), 16);
        assert!((plan.lengths[0] - 29_421.732_229_671_09).abs() < 1e-6);
        assert!((plan.lengths[207] - 74.522_004_144_117_02).abs() < 1e-9);
    }

    #[test]
    fn a_tone_lights_its_own_bin() {
        let frequency = 440.0;
        let samples: Vec<f32> = (0..SAMPLE_RATE as usize * 4)
            .map(|index| (TAU * frequency * index as f64 / f64::from(SAMPLE_RATE)).sin() as f32 * 0.5)
            .collect();
        let features = features(&samples);
        let middle = features.frame_count / 2;
        let loudest = (0..CORE_BINS)
            .max_by(|left, right| features.row(*left)[middle].total_cmp(&features.row(*right)[middle]))
            .unwrap();
        let semitones_from_c1_to_a4 = 3 * 12 + 9;
        assert_eq!(loudest, semitones_from_c1_to_a4 * 2);
    }

    #[test]
    fn rounding_matches_half_precision() {
        assert_eq!(round_to_half_precision(1.0), 1.0);
        assert_eq!(round_to_half_precision(1.000_4), 1.0);
        assert_eq!(round_to_half_precision(1.000_6), 1.000_976_6);
        assert_eq!(round_to_half_precision(3.141_592_7), 3.140_625);
        assert_eq!(round_to_half_precision(0.0), 0.0);
        assert_eq!(round_to_half_precision(1e-7), 1.192_092_9e-7);
    }
}
