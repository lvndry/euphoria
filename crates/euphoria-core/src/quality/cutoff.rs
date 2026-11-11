// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::f32::consts::TAU;
use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

/// Frames are the power of two number of samples closest to this length,
/// about 11 Hz per frequency bin at 44.1 kHz.
const FRAME_SECONDS: f32 = 0.093;
const MINIMUM_FRAME_LENGTH: usize = 256;
const MAXIMUM_FRAME_LENGTH: usize = 32_768;
/// Frames quieter than this, in dB below full scale, are left out, so that
/// silence and fades do not dilute the spectrum.
const QUIET_FRAME_DB: f64 = -50.0;
/// The width of the band compared on each side of a candidate cutoff.
const SIDE_BAND_HZ: f32 = 1_000.0;
/// The lowest frequency a cutoff is looked for at. Below it, a fall in
/// level is more likely the music than an encoder.
const LOWEST_CUTOFF_HZ: f32 = 8_000.0;
/// How far the average level must fall from one side band to the next for
/// the fall to count as a cutoff. Lossy encoders remove everything above
/// their cutoff, which leaves a fall of 30 to 60 dB; music that is merely
/// dark falls far more gradually.
pub(crate) const MINIMUM_FALL_DB: f64 = 20.0;

/// Accumulates channel powers before any mixdown, keeping at most one
/// FFT frame of interleaved samples. Channel phase cannot cancel energy.
#[derive(Default)]
pub(crate) struct SpectralCutoffMeter {
    spectrum: Option<Spectrum>,
}

impl SpectralCutoffMeter {
    pub(crate) fn add(&mut self, samples: &[f32], channels: usize, sample_rate: u32) {
        let channels = channels.max(1);
        if self.spectrum.as_ref().is_none_or(|spectrum| {
            spectrum.channels != channels || spectrum.sample_rate != sample_rate
        }) {
            self.spectrum = Some(Spectrum::new(sample_rate, channels));
        }
        if let Some(spectrum) = &mut self.spectrum {
            spectrum.add(samples);
        }
    }

    /// The steepest fall above 8 kHz, or no cutoff for full-band, short or
    /// quiet audio. The top 1 kHz below Nyquist is excluded.
    pub(crate) fn finish(self) -> Option<f32> {
        let spectrum = self.spectrum?;
        let rate = spectrum.sample_rate as f32;
        let level = spectrum.levels()?;
        cutoff_from_levels(&level, rate)
    }
}

fn cutoff_from_levels(level: &[f64], rate: f32) -> Option<f32> {
    let bin_width = rate / (2 * (level.len() - 1)) as f32;
    let side = ((SIDE_BAND_HZ / bin_width).round() as usize).max(1);
    let lowest = ((LOWEST_CUTOFF_HZ / bin_width).ceil() as usize).max(side);
    if lowest + side >= level.len() {
        return None;
    }
    // Prefix sums give the mean level of any band in constant time.
    let mut prefix = Vec::with_capacity(level.len() + 1);
    prefix.push(0.0);
    for value in level {
        prefix.push(prefix[prefix.len() - 1] + value);
    }
    let mean = |start: usize, end: usize| (prefix[end] - prefix[start]) / (end - start) as f64;
    let mut steepest: Option<(usize, f64)> = None;
    for bin in lowest..=level.len() - side {
        let fall = mean(bin - side, bin) - mean(bin, bin + side);
        if steepest.is_none_or(|(_, best)| fall > best) {
            steepest = Some((bin, fall));
        }
    }
    let (bin, fall) = steepest?;
    (fall >= MINIMUM_FALL_DB).then_some(bin as f32 * bin_width)
}

struct Spectrum {
    sample_rate: u32,
    channels: usize,
    transform: Arc<dyn Fft<f32>>,
    scratch: Vec<Complex<f32>>,
    frame: Vec<Complex<f32>>,
    window: Vec<f32>,
    pending: Vec<f32>,
    power: Vec<f64>,
    loud_frames: usize,
}

impl Spectrum {
    fn new(sample_rate: u32, channels: usize) -> Self {
        let frame_length = frame_length(sample_rate as f32);
        let transform = FftPlanner::<f32>::new().plan_fft_forward(frame_length);
        let scratch = vec![Complex::default(); transform.get_inplace_scratch_len()];
        Self {
            sample_rate,
            channels,
            transform,
            scratch,
            frame: vec![Complex::default(); frame_length],
            window: (0..frame_length)
                .map(|index| 0.5 - 0.5 * (TAU * index as f32 / frame_length as f32).cos())
                .collect(),
            pending: Vec::with_capacity(frame_length * channels),
            power: vec![0.0; frame_length / 2 + 1],
            loud_frames: 0,
        }
    }

    fn add(&mut self, mut samples: &[f32]) {
        let frame_samples = self.frame.len() * self.channels;
        while !samples.is_empty() {
            let taken = samples.len().min(frame_samples - self.pending.len());
            self.pending.extend_from_slice(&samples[..taken]);
            samples = &samples[taken..];
            if self.pending.len() == frame_samples {
                self.measure_frame();
                self.pending.clear();
            }
        }
    }

    fn measure_frame(&mut self) {
        let quiet_mean_square = 10.0_f64.powf(QUIET_FRAME_DB / 10.0);
        for channel in 0..self.channels {
            let mean_square = self
                .pending
                .chunks_exact(self.channels)
                .map(|frame| f64::from(frame[channel]).powi(2))
                .sum::<f64>()
                / self.frame.len() as f64;
            if mean_square < quiet_mean_square || !mean_square.is_finite() {
                continue;
            }
            for ((slot, samples), weight) in self
                .frame
                .iter_mut()
                .zip(self.pending.chunks_exact(self.channels))
                .zip(&self.window)
            {
                *slot = Complex::new(samples[channel] * weight, 0.0);
            }
            self.transform
                .process_with_scratch(&mut self.frame, &mut self.scratch);
            for (total, value) in self.power.iter_mut().zip(&self.frame) {
                *total += f64::from(value.norm_sqr());
            }
            self.loud_frames += 1;
        }
    }

    fn levels(self) -> Option<Vec<f64>> {
        if self.loud_frames == 0 {
            return None;
        }
        Some(
            self.power
                .into_iter()
                .map(|total| 10.0 * (total / self.loud_frames as f64 + 1.0e-30).log10())
                .collect(),
        )
    }
}

fn frame_length(rate: f32) -> usize {
    let target = rate * FRAME_SECONDS;
    let above = (target.ceil() as usize).next_power_of_two();
    let below = above / 2;
    let nearest = if target - below as f32 <= above as f32 - target {
        below
    } else {
        above
    };
    nearest.clamp(MINIMUM_FRAME_LENGTH, MAXIMUM_FRAME_LENGTH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroU32;

    fn spectral_cutoff(samples: &[f32], sample_rate: NonZeroU32) -> Option<f32> {
        let mut meter = SpectralCutoffMeter::default();
        meter.add(samples, 1, sample_rate.get());
        meter.finish()
    }

    const RATE: u32 = 44_100;

    fn rate() -> NonZeroU32 {
        NonZeroU32::new(RATE).unwrap()
    }

    /// White noise from a fixed seed.
    fn noise(seconds: f32) -> Vec<f32> {
        let mut state = 0x2545_f491_u32;
        (0..(RATE as f32 * seconds) as usize)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.3
            })
            .collect()
    }

    /// `samples` with everything above `cutoff_hz` removed, as an encoder's
    /// lowpass does, by zeroing those bins of one long transform.
    fn brickwall(samples: &[f32], cutoff_hz: f32) -> Vec<f32> {
        let length = samples.len();
        let mut planner = FftPlanner::<f32>::new();
        let mut spectrum: Vec<Complex<f32>> = samples
            .iter()
            .map(|sample| Complex::new(*sample, 0.0))
            .collect();
        planner.plan_fft_forward(length).process(&mut spectrum);
        let last_kept = (cutoff_hz / RATE as f32 * length as f32) as usize;
        for (bin, value) in spectrum.iter_mut().enumerate() {
            let frequency_bin = bin.min(length - bin);
            if frequency_bin > last_kept {
                *value = Complex::default();
            }
        }
        planner.plan_fft_inverse(length).process(&mut spectrum);
        spectrum
            .iter()
            .map(|value| value.re / length as f32)
            .collect()
    }

    /// `samples` through a one pole lowpass: 6 dB less per octave above
    /// `corner_hz`, the gentle slope of music that is simply dark.
    fn gentle_lowpass(samples: &[f32], corner_hz: f32) -> Vec<f32> {
        let coefficient = (-TAU * corner_hz / RATE as f32).exp();
        let mut state = 0.0;
        samples
            .iter()
            .map(|sample| {
                state = (1.0 - coefficient) * sample + coefficient * state;
                state
            })
            .collect()
    }

    #[test]
    fn full_band_noise_has_no_cutoff() {
        assert_eq!(spectral_cutoff(&noise(5.0), rate()), None);
    }

    #[test]
    fn a_brickwall_lowpass_is_found_within_a_few_hundred_hertz() {
        for cutoff in [11_000.0, 15_500.0, 16_500.0, 19_000.0, 20_000.0] {
            let filtered = brickwall(&noise(5.0), cutoff);
            let found = spectral_cutoff(&filtered, rate()).expect("a cutoff");
            assert!((found - cutoff).abs() < 300.0, "{cutoff} found at {found}");
        }
    }

    #[test]
    fn a_gentle_slope_is_not_a_cutoff() {
        let dark = gentle_lowpass(&noise(5.0), 2_000.0);
        assert_eq!(spectral_cutoff(&dark, rate()), None);
    }

    #[test]
    fn silence_and_short_audio_have_no_cutoff() {
        assert_eq!(spectral_cutoff(&vec![0.0; RATE as usize * 3], rate()), None);
        assert_eq!(spectral_cutoff(&noise(0.01), rate()), None);
    }

    #[test]
    fn the_cutoff_does_not_depend_on_the_level() {
        let filtered = brickwall(&noise(5.0), 16_500.0);
        let quieter: Vec<f32> = filtered.iter().map(|sample| sample * 0.05).collect();
        assert_eq!(
            spectral_cutoff(&filtered, rate()),
            spectral_cutoff(&quieter, rate())
        );
    }

    #[test]
    fn channel_power_and_cutoff_do_not_depend_on_decoder_packet_boundaries() {
        let mono = brickwall(&noise(1.0), 16_500.0);
        let expected = spectral_cutoff(&mono, rate()).unwrap();
        // Phase-opposed channels cancel completely in a mono mixdown.
        let interleaved: Vec<f32> = mono
            .iter()
            .flat_map(|sample| [*sample, -*sample, 0.0, *sample * 0.5])
            .collect();
        for packet_frames in [1, 17, 1_152, 4_096, 5_000] {
            let mut meter = SpectralCutoffMeter::default();
            for packet in interleaved.chunks(packet_frames * 4) {
                meter.add(packet, 4, RATE);
            }
            assert_eq!(meter.finish(), Some(expected), "{packet_frames}");
        }
    }

    #[test]
    fn frame_length_follows_the_sample_rate() {
        assert_eq!(frame_length(44_100.0), 4_096);
        assert_eq!(frame_length(48_000.0), 4_096);
        assert_eq!(frame_length(96_000.0), 8_192);
        assert_eq!(frame_length(1_000.0), MINIMUM_FRAME_LENGTH);
    }
}
