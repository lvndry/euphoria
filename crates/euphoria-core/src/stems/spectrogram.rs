// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The short-time Fourier transforms around the htdemucs network, computed
//! exactly as Demucs computes them with PyTorch: a periodic Hann window of
//! 4096 samples, a hop of 1024, frames centred with reflected padding and
//! both directions scaled by the square root of the window length.

use std::sync::Arc;

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};

/// Samples in one segment the network separates, 7.8 seconds at 44.1 kHz.
pub(crate) const SEGMENT_SAMPLES: usize = 343_980;
/// Frequency bins the network sees: the bins of a 4096 point transform,
/// without the last one.
pub(crate) const FREQUENCY_BINS: usize = 2048;
/// Frames the network sees in one segment.
pub(crate) const FRAMES: usize = SEGMENT_SAMPLES.div_ceil(HOP);
/// Channels of the spectrogram input: real and imaginary parts of the left
/// and of the right channel.
pub(crate) const SPECTROGRAM_CHANNELS: usize = 4;

const WINDOW_LENGTH: usize = 4096;
const HOP: usize = WINDOW_LENGTH / 4;
const SEGMENT_PADDING: usize = HOP / 2 * 3;
const CENTRE_PADDING: usize = WINDOW_LENGTH / 2;
const SKIPPED_FRAMES: usize = 2;
const PADDED_LENGTH: usize = FRAMES * HOP + 2 * SEGMENT_PADDING;
const ALL_FRAMES: usize = FRAMES + 2 * SKIPPED_FRAMES;
const OVERLAP_ADDED_LENGTH: usize = WINDOW_LENGTH + HOP * (ALL_FRAMES - 1);

/// Forward and inverse transforms of one window length, planned once.
pub(crate) struct SpectrogramTransforms {
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    envelope: Vec<f32>,
    scale: f32,
}

impl SpectrogramTransforms {
    pub(crate) fn new() -> Self {
        let mut planner = FftPlanner::new();
        let window: Vec<f32> = (0..WINDOW_LENGTH)
            .map(|index| {
                let phase = 2.0 * std::f64::consts::PI * index as f64 / WINDOW_LENGTH as f64;
                (0.5 - 0.5 * phase.cos()) as f32
            })
            .collect();
        let mut envelope = vec![0.0f32; OVERLAP_ADDED_LENGTH];
        for frame in 0..ALL_FRAMES {
            let start = frame * HOP;
            for (offset, weight) in window.iter().enumerate() {
                envelope[start + offset] += weight * weight;
            }
        }
        Self {
            forward: planner.plan_fft_forward(WINDOW_LENGTH),
            inverse: planner.plan_fft_inverse(WINDOW_LENGTH),
            window,
            envelope,
            scale: (WINDOW_LENGTH as f32).sqrt(),
        }
    }

    /// The spectrogram input of the network for one stereo segment of
    /// [`SEGMENT_SAMPLES`] frames per channel, laid out as
    /// `[channel * 2 + part][bin][frame]` where part 0 is the real part.
    pub(crate) fn spectrogram(&self, channels: [&[f32]; 2], output: &mut Vec<f32>) {
        output.clear();
        output.resize(SPECTROGRAM_CHANNELS * FREQUENCY_BINS * FRAMES, 0.0);
        let mut buffer = vec![Complex32::default(); WINDOW_LENGTH];
        let mut scratch = vec![Complex32::default(); self.forward.get_inplace_scratch_len()];
        for (channel_index, samples) in channels.iter().enumerate() {
            debug_assert_eq!(samples.len(), SEGMENT_SAMPLES);
            let real_offset = (channel_index * 2) * FREQUENCY_BINS * FRAMES;
            let imaginary_offset = (channel_index * 2 + 1) * FREQUENCY_BINS * FRAMES;
            for frame in 0..FRAMES {
                let start = (frame + SKIPPED_FRAMES) * HOP;
                for (offset, value) in buffer.iter_mut().enumerate() {
                    let sample = samples[padded_source_index(start + offset)];
                    *value = Complex32::new(sample * self.window[offset], 0.0);
                }
                self.forward.process_with_scratch(&mut buffer, &mut scratch);
                for (bin, value) in buffer.iter().take(FREQUENCY_BINS).enumerate() {
                    output[real_offset + bin * FRAMES + frame] = value.re / self.scale;
                    output[imaginary_offset + bin * FRAMES + frame] = value.im / self.scale;
                }
            }
        }
    }

    /// The waveform of one channel of one source from the frequency branch
    /// output, `real` and `imaginary` each laid out as `[bin][frame]`. Adds
    /// it to `output`, which holds [`SEGMENT_SAMPLES`] samples.
    pub(crate) fn add_inverse(&self, real: &[f32], imaginary: &[f32], output: &mut [f32]) {
        debug_assert_eq!(output.len(), SEGMENT_SAMPLES);
        let mut overlap_added = vec![0.0f32; OVERLAP_ADDED_LENGTH];
        let mut buffer = vec![Complex32::default(); WINDOW_LENGTH];
        let mut scratch = vec![Complex32::default(); self.inverse.get_inplace_scratch_len()];
        let inverse_scale = self.scale / WINDOW_LENGTH as f32;
        for frame in 0..FRAMES {
            buffer.fill(Complex32::default());
            for bin in 0..FREQUENCY_BINS {
                let value =
                    Complex32::new(real[bin * FRAMES + frame], imaginary[bin * FRAMES + frame]);
                buffer[bin] = value;
                if bin > 0 {
                    buffer[WINDOW_LENGTH - bin] = value.conj();
                }
            }
            buffer[0].im = 0.0;
            self.inverse.process_with_scratch(&mut buffer, &mut scratch);
            let start = (frame + SKIPPED_FRAMES) * HOP;
            for (offset, value) in buffer.iter().enumerate() {
                overlap_added[start + offset] += value.re * inverse_scale * self.window[offset];
            }
        }
        let first = CENTRE_PADDING + SEGMENT_PADDING;
        for (index, sample) in output.iter_mut().enumerate() {
            let position = first + index;
            *sample += overlap_added[position] / self.envelope[position];
        }
    }
}

/// The index into a segment of the sample at `index` of the signal the
/// transform reads: the segment reflected at both ends to [`PADDED_LENGTH`]
/// samples, then reflected again by half a window for centred frames.
fn padded_source_index(index: usize) -> usize {
    let in_padded = reflect(index as isize - CENTRE_PADDING as isize, PADDED_LENGTH);
    reflect(
        in_padded as isize - SEGMENT_PADDING as isize,
        SEGMENT_SAMPLES,
    )
}

/// Reflects `index` into `0..length` without repeating the edge sample,
/// as numpy and PyTorch do in their reflect padding.
fn reflect(index: isize, length: usize) -> usize {
    let last = length as isize - 1;
    let mut position = index;
    loop {
        if position < 0 {
            position = -position;
        } else if position > last {
            position = 2 * last - position;
        } else {
            return position as usize;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflection_skips_the_edge_sample() {
        assert_eq!(reflect(-1, 5), 1);
        assert_eq!(reflect(-2, 5), 2);
        assert_eq!(reflect(5, 5), 3);
        assert_eq!(reflect(6, 5), 2);
        assert_eq!(reflect(3, 5), 3);
    }

    #[test]
    fn the_segment_has_the_frames_the_network_was_trained_on() {
        assert_eq!(FRAMES, 336);
        assert_eq!(PADDED_LENGTH, 347_136);
    }

    /// Demucs drops the two outermost frames at each end, so only the
    /// middle of a segment comes back whole; overlap-add gives the ends a
    /// weight near zero.
    #[test]
    fn the_inverse_transform_gives_back_the_middle_of_the_segment() {
        let left: Vec<f32> = (0..SEGMENT_SAMPLES)
            .map(|index| (index as f32 * 0.013).sin() * 0.5 + (index as f32 * 0.0007).cos() * 0.2)
            .collect();
        let right: Vec<f32> = (0..SEGMENT_SAMPLES)
            .map(|index| ((index * 7919 % 1000) as f32 / 1000.0 - 0.5) * 0.3)
            .collect();
        let transforms = SpectrogramTransforms::new();
        let mut spectrogram = Vec::new();
        transforms.spectrogram([&left, &right], &mut spectrogram);
        let plane = FREQUENCY_BINS * FRAMES;
        for (channel, original) in [&left, &right].into_iter().enumerate() {
            let mut rebuilt = vec![0.0f32; SEGMENT_SAMPLES];
            transforms.add_inverse(
                &spectrogram[channel * 2 * plane..(channel * 2 + 1) * plane],
                &spectrogram[(channel * 2 + 1) * plane..(channel * 2 + 2) * plane],
                &mut rebuilt,
            );
            let middle = WINDOW_LENGTH..SEGMENT_SAMPLES - WINDOW_LENGTH;
            let largest_error = original[middle.clone()]
                .iter()
                .zip(&rebuilt[middle])
                .map(|(expected, actual)| (expected - actual).abs())
                .fold(0.0f32, f32::max);
            assert!(largest_error < 1e-3, "channel {channel}: {largest_error}");
        }
    }
}
