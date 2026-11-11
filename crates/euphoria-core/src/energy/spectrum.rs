// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::f32::consts::{SQRT_2, TAU};
use std::num::NonZeroU32;
use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use super::onsets::count_onsets;

/// Frames are the power of two number of samples closest to this length.
const FRAME_SECONDS: f32 = 0.04;
const MINIMUM_FRAME_LENGTH: usize = 64;
/// The frame at the highest supported sample rate. No frame is longer, so
/// the working buffers stay small whatever sample rate a buffer claims.
const MAXIMUM_FRAME_LENGTH: usize = 16_384;
/// Each frame starts half a frame after the one before it.
const FRAMES_PER_FRAME_LENGTH: usize = 2;

/// The features read from the spectrum of a stretch of audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SpectralFeatures {
    /// Mean rise per frequency bin and frame of the square root of the
    /// magnitude, with magnitudes relative to the level of the signal.
    pub(crate) spectral_flux: f32,
    /// Onsets per second.
    pub(crate) onset_rate: f32,
}

/// Reads spectral features from audio one short frame at a time. The
/// working buffers are allocated once and reused for every frame and every
/// call.
pub(crate) struct SpectralAnalyzer {
    transform: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    frame: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    compressed: Vec<f32>,
    previous_compressed: Vec<f32>,
    onset_strength: Vec<f32>,
    hop_length: usize,
    sample_rate: f32,
}

impl SpectralAnalyzer {
    pub(crate) fn new(sample_rate: NonZeroU32) -> Self {
        let sample_rate = sample_rate.get() as f32;
        let frame_length = frame_length(sample_rate);
        let transform = FftPlanner::new().plan_fft_forward(frame_length);
        let scratch_length = transform.get_inplace_scratch_len();
        let bin_count = frame_length / 2;
        Self {
            transform,
            window: hann_window(frame_length),
            frame: vec![Complex::default(); frame_length],
            scratch: vec![Complex::default(); scratch_length],
            compressed: vec![0.0; bin_count],
            previous_compressed: vec![0.0; bin_count],
            onset_strength: Vec::new(),
            hop_length: frame_length / FRAMES_PER_FRAME_LENGTH,
            sample_rate,
        }
    }

    /// Returns `None` when the audio is shorter than one frame or carries
    /// no signal.
    pub(crate) fn analyze(&mut self, samples: &[f32]) -> Option<SpectralFeatures> {
        let frame_length = self.window.len();
        let level = root_mean_square(samples);
        if samples.len() < frame_length || level <= 0.0 || !level.is_finite() {
            return None;
        }
        // A sine at the level of the whole signal has magnitude 1.0 after
        // this scaling, which makes flux and onsets independent of volume.
        let magnitude_scale = 2.0 / (frame_length as f32 * level * SQRT_2);
        let compressed_scale = magnitude_scale.sqrt() / self.compressed.len() as f32;

        self.onset_strength.clear();
        for start in (0..=samples.len() - frame_length).step_by(self.hop_length) {
            self.transform_frame(&samples[start..start + frame_length]);
            let is_first_frame = self.onset_strength.is_empty();
            let mut rise = 0.0_f32;
            // Bin 0 is skipped: a constant offset is not sound.
            for bin in 1..self.compressed.len() {
                // The square root of the magnitude lets quiet partials
                // count next to loud ones.
                let compressed = self.frame[bin].norm_sqr().sqrt().sqrt();
                rise += (compressed - self.previous_compressed[bin]).max(0.0);
                self.compressed[bin] = compressed;
            }
            let strength = if is_first_frame {
                0.0
            } else {
                rise * compressed_scale
            };
            self.onset_strength.push(strength);
            std::mem::swap(&mut self.compressed, &mut self.previous_compressed);
        }
        let frames_per_second = self.sample_rate / self.hop_length as f32;
        let duration_seconds = samples.len() as f32 / self.sample_rate;
        let onset_count = count_onsets(&self.onset_strength, frames_per_second);
        Some(SpectralFeatures {
            spectral_flux: self.onset_strength.iter().sum::<f32>()
                / self.onset_strength.len() as f32,
            onset_rate: onset_count as f32 / duration_seconds,
        })
    }

    fn transform_frame(&mut self, samples: &[f32]) {
        for ((slot, sample), weight) in self.frame.iter_mut().zip(samples).zip(&self.window) {
            *slot = Complex::new(sample * weight, 0.0);
        }
        self.transform
            .process_with_scratch(&mut self.frame, &mut self.scratch);
    }
}

fn frame_length(sample_rate: f32) -> usize {
    let target = (sample_rate * FRAME_SECONDS).min(MAXIMUM_FRAME_LENGTH as f32);
    let above = (target.ceil() as usize).next_power_of_two();
    let below = above / 2;
    let nearest = if target - below as f32 <= above as f32 - target {
        below
    } else {
        above
    };
    nearest.clamp(MINIMUM_FRAME_LENGTH, MAXIMUM_FRAME_LENGTH)
}

fn hann_window(length: usize) -> Vec<f32> {
    (0..length)
        .map(|index| 0.5 - 0.5 * (TAU * index as f32 / length as f32).cos())
        .collect()
}

fn root_mean_square(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_of_squares: f64 = samples
        .iter()
        .map(|sample| f64::from(*sample) * f64::from(*sample))
        .sum();
    (sum_of_squares / samples.len() as f64).sqrt() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: u32 = 44_100;

    fn analyzer() -> SpectralAnalyzer {
        SpectralAnalyzer::new(NonZeroU32::new(SAMPLE_RATE).unwrap())
    }

    fn sine(frequency: f32, amplitude: f32, seconds: f32) -> Vec<f32> {
        let sample_count = (SAMPLE_RATE as f32 * seconds) as usize;
        (0..sample_count)
            .map(|index| (TAU * frequency * index as f32 / SAMPLE_RATE as f32).sin() * amplitude)
            .collect()
    }

    fn noise(amplitude: f32, seconds: f32) -> Vec<f32> {
        let sample_count = (SAMPLE_RATE as f32 * seconds) as usize;
        let mut state = 0x2545_f491_u32;
        (0..sample_count)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * amplitude
            })
            .collect()
    }

    /// Bursts of a decaying 60 Hz tone, like a kick drum.
    fn pulse_train(pulses_per_second: f32, seconds: f32) -> Vec<f32> {
        let sample_count = (SAMPLE_RATE as f32 * seconds) as usize;
        let period = 1.0 / pulses_per_second;
        (0..sample_count)
            .map(|index| {
                let time = index as f32 / SAMPLE_RATE as f32;
                // Starting half a period in keeps the first pulse off the
                // first frame, which has nothing before it to rise from.
                let since_pulse = (time + period / 2.0) % period;
                (TAU * 60.0 * since_pulse).sin() * (-since_pulse * 30.0).exp() * 0.8
            })
            .collect()
    }

    #[test]
    fn frame_length_is_the_nearest_power_of_two_to_forty_milliseconds() {
        assert_eq!(frame_length(44_100.0), 2_048);
        assert_eq!(frame_length(48_000.0), 2_048);
        assert_eq!(frame_length(22_050.0), 1_024);
        assert_eq!(frame_length(96_000.0), 4_096);
        assert_eq!(frame_length(100.0), MINIMUM_FRAME_LENGTH);
    }

    #[test]
    fn frame_length_never_exceeds_the_frame_of_the_highest_supported_rate() {
        let highest_supported = crate::energy::ENERGY_MAXIMUM_SAMPLE_RATE as f32;
        assert_eq!(frame_length(highest_supported), MAXIMUM_FRAME_LENGTH);
        assert_eq!(frame_length(1_000_000.0), MAXIMUM_FRAME_LENGTH);
        assert_eq!(frame_length(u32::MAX as f32), MAXIMUM_FRAME_LENGTH);
    }

    #[test]
    fn an_analyzer_for_an_extreme_sample_rate_has_small_buffers() {
        let analyzer = SpectralAnalyzer::new(NonZeroU32::new(u32::MAX).unwrap());
        assert_eq!(analyzer.window.len(), MAXIMUM_FRAME_LENGTH);
        assert_eq!(analyzer.frame.len(), MAXIMUM_FRAME_LENGTH);
        assert!(analyzer.scratch.len() <= MAXIMUM_FRAME_LENGTH);
    }

    #[test]
    fn root_mean_square_of_a_sine_is_its_amplitude_over_root_two() {
        let level = root_mean_square(&sine(440.0, 0.5, 1.0));
        assert!((level - 0.5 / 2.0_f32.sqrt()).abs() < 1.0e-3, "{level}");
        assert_eq!(root_mean_square(&[]), 0.0);
    }

    #[test]
    fn silence_and_audio_shorter_than_a_frame_have_no_features() {
        assert_eq!(analyzer().analyze(&vec![0.0; SAMPLE_RATE as usize]), None);
        assert_eq!(analyzer().analyze(&[0.5; 100]), None);
    }

    #[test]
    fn noise_has_more_flux_than_a_steady_tone() {
        let tone = analyzer().analyze(&sine(440.0, 0.5, 2.0)).unwrap();
        let hiss = analyzer().analyze(&noise(0.5, 2.0)).unwrap();
        assert!(tone.spectral_flux < 1.0e-3, "{tone:?}");
        assert!(hiss.spectral_flux > 10.0 * tone.spectral_flux, "{hiss:?}");
    }

    #[test]
    fn steady_signals_have_no_onsets() {
        let tone = analyzer().analyze(&sine(440.0, 0.5, 4.0)).unwrap();
        let hiss = analyzer().analyze(&noise(0.5, 4.0)).unwrap();
        assert_eq!(tone.onset_rate, 0.0);
        assert_eq!(hiss.onset_rate, 0.0);
    }

    #[test]
    fn onset_rate_follows_the_pulse_rate() {
        for pulses_per_second in [1.0, 2.0, 4.0, 8.0] {
            let features = analyzer()
                .analyze(&pulse_train(pulses_per_second, 8.0))
                .unwrap();
            assert!(
                (features.onset_rate - pulses_per_second).abs() < 0.2,
                "{pulses_per_second} pulses per second gave {features:?}"
            );
        }
    }

    #[test]
    fn faster_pulses_have_more_flux() {
        let slow = analyzer().analyze(&pulse_train(2.0, 8.0)).unwrap();
        let fast = analyzer().analyze(&pulse_train(4.0, 8.0)).unwrap();
        assert!(fast.spectral_flux > slow.spectral_flux);
    }

    #[test]
    fn features_do_not_depend_on_volume() {
        let loud = pulse_train(4.0, 4.0);
        let quiet: Vec<f32> = loud.iter().map(|sample| sample * 0.05).collect();
        let loud = analyzer().analyze(&loud).unwrap();
        let quiet = analyzer().analyze(&quiet).unwrap();
        assert_eq!(loud.onset_rate, quiet.onset_rate);
        assert!((loud.spectral_flux - quiet.spectral_flux).abs() < 1.0e-4);
    }

    #[test]
    fn an_analyzer_gives_the_same_answer_when_reused() {
        let samples = pulse_train(4.0, 4.0);
        let mut analyzer = analyzer();
        let first = analyzer.analyze(&samples).unwrap();
        analyzer.analyze(&noise(0.5, 1.0)).unwrap();
        assert_eq!(analyzer.analyze(&samples).unwrap(), first);
    }
}
