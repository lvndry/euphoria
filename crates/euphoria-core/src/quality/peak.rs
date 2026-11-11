// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::f32::consts::PI;

/// Taps of the interpolation filter on each side of the point between two
/// samples, as in the four times oversampling EBU R128 asks for.
const HALF_TAPS: usize = 6;
const TAPS: usize = 2 * HALF_TAPS;
/// Points computed between two samples: a quarter, a half and three
/// quarters of the way.
const PHASES: usize = 3;
/// The points between two samples are computed only when one of the two
/// samples reaches this share of the highest peak found so far. Between
/// two smaller samples, a band limited signal does not rise that far: the
/// overshoot of an inter-sample peak stays well under 6 dB for music.
const NEAR_PEAK: f32 = 0.5;

/// The highest true peak over every channel, estimated by interpolating
/// four times between samples with a windowed sinc filter. Interpolation
/// runs only near the loudest samples, which keeps it cheap: a loud master
/// has few samples within 6 dB of its peak.
pub(crate) struct TruePeakMeter {
    coefficients: [[f32; TAPS]; PHASES],
    /// The last `TAPS` samples of each channel, oldest first, in a ring.
    history: Vec<[f32; TAPS]>,
    position: usize,
    filled: usize,
    peak: f32,
}

impl Default for TruePeakMeter {
    fn default() -> Self {
        Self {
            coefficients: coefficients(),
            history: Vec::new(),
            position: 0,
            filled: 0,
            peak: 0.0,
        }
    }
}

impl TruePeakMeter {
    /// Adds one chunk of interleaved samples.
    pub(crate) fn add(&mut self, interleaved: &[f32], channel_count: usize) {
        let channel_count = channel_count.max(1);
        if self.history.len() != channel_count {
            self.history = vec![[0.0; TAPS]; channel_count];
            self.position = 0;
            self.filled = 0;
        }
        for frame in interleaved.chunks_exact(channel_count) {
            for (channel, sample) in frame.iter().enumerate() {
                self.history[channel][self.position] = *sample;
                self.peak = self.peak.max(sample.abs());
            }
            self.position = (self.position + 1) % TAPS;
            self.filled = (self.filled + 1).min(TAPS);
            if self.filled == TAPS {
                for channel in 0..channel_count {
                    self.interpolate(channel);
                }
            }
        }
    }

    /// Checks the points between the two middle samples of a channel's
    /// history.
    fn interpolate(&mut self, channel: usize) {
        let ring = &self.history[channel];
        let sample_at = |offset: usize| ring[(self.position + offset) % TAPS];
        let before = sample_at(HALF_TAPS - 1).abs();
        let after = sample_at(HALF_TAPS).abs();
        if before.max(after) < self.peak * NEAR_PEAK {
            return;
        }
        for phase in &self.coefficients {
            let value: f32 = phase
                .iter()
                .enumerate()
                .map(|(tap, weight)| weight * sample_at(tap))
                .sum();
            self.peak = self.peak.max(value.abs());
        }
    }

    /// The highest true peak in dB relative to full scale, or `None` for
    /// silence.
    pub(crate) fn decibels(&self) -> Option<f32> {
        (self.peak > 0.0).then(|| 20.0 * self.peak.log10())
    }
}

/// A Hann windowed sinc for each point between two samples. Tap `tap`
/// weighs the sample `tap - HALF_TAPS + 1` places after the earlier of the
/// two samples.
fn coefficients() -> [[f32; TAPS]; PHASES] {
    let mut table = [[0.0; TAPS]; PHASES];
    for (phase, weights) in table.iter_mut().enumerate() {
        let fraction = (phase + 1) as f32 / (PHASES + 1) as f32;
        for (tap, weight) in weights.iter_mut().enumerate() {
            let distance = tap as f32 - (HALF_TAPS - 1) as f32 - fraction;
            let sinc = if distance == 0.0 {
                1.0
            } else {
                (PI * distance).sin() / (PI * distance)
            };
            let window = 0.5 + 0.5 * (PI * distance / (HALF_TAPS as f32 + 1.0)).cos();
            *weight = sinc * window;
        }
        let total: f32 = weights.iter().sum();
        for weight in weights.iter_mut() {
            *weight /= total;
        }
    }
    table
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use ebur128::{EbuR128, Mode};

    use super::*;

    const RATE: u32 = 48_000;

    fn stereo_sine(frequency: f32, amplitude: f32, phase: f32, seconds: f32) -> Vec<f32> {
        (0..(RATE as f32 * seconds) as usize)
            .flat_map(|index| {
                let value =
                    (TAU * frequency * index as f32 / RATE as f32 + phase).sin() * amplitude;
                [value, value * 0.5]
            })
            .collect()
    }

    fn reference_true_peak(interleaved: &[f32]) -> f32 {
        let mut meter = EbuR128::new(2, RATE, Mode::TRUE_PEAK).unwrap();
        meter.add_frames_f32(interleaved).unwrap();
        let peak = meter.true_peak(0).unwrap().max(meter.true_peak(1).unwrap());
        (20.0 * peak.log10()) as f32
    }

    fn measured(interleaved: &[f32]) -> f32 {
        let mut meter = TruePeakMeter::default();
        for chunk in interleaved.chunks(2 * 1_000) {
            meter.add(chunk, 2);
        }
        meter.decibels().unwrap()
    }

    #[test]
    fn an_inter_sample_peak_is_found() {
        // A sine at a quarter of the sample rate, sampled 45 degrees away
        // from its crests, never shows its peak in a sample: the samples
        // are 3 dB below it.
        let tricky = stereo_sine(RATE as f32 / 4.0, 0.9, PI / 4.0, 1.0);
        let sample_peak = 20.0
            * tricky
                .iter()
                .fold(0.0_f32, |peak, value| peak.max(value.abs()))
                .log10();
        let actual = 20.0 * 0.9_f32.log10();
        let found = measured(&tricky);
        assert!(actual - sample_peak > 2.9, "{actual} {sample_peak}");
        assert!((found - actual).abs() < 0.1, "{found} against {actual}");
        assert!((found - reference_true_peak(&tricky)).abs() < 0.3);
    }

    #[test]
    fn tones_peak_at_their_amplitude() {
        for frequency in [100.0, 997.0, 5_000.0, 15_000.0, 19_000.0] {
            let tone = stereo_sine(frequency, 0.5, 0.3, 0.5);
            let found = measured(&tone);
            let actual = 20.0 * 0.5_f32.log10();
            assert!(
                (found - actual).abs() < 0.1,
                "{frequency}: {found} against {actual}"
            );
        }
    }

    #[test]
    fn silence_has_no_peak() {
        let mut meter = TruePeakMeter::default();
        meter.add(&[0.0; 400], 2);
        assert_eq!(meter.decibels(), None);
    }

    #[test]
    fn each_phase_of_the_filter_keeps_the_level() {
        for weights in coefficients() {
            assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1.0e-5);
        }
    }
}
