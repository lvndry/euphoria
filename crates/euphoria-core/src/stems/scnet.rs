// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Advanced mode network, SCNet XL IHF: the transforms around it and the
//! segment it separates.
//!
//! The network takes the short-time Fourier transform of a segment of 5.5
//! seconds and returns one spectrum per stem. The transform has a window of
//! 4096 samples with no tapering, a hop of 1024, reflection padding at both
//! ends and a scale of one over the square root of the window length, as
//! `torch.stft` with `normalized=True` computes it.

use std::path::Path;
use std::sync::Arc;

use ort::session::Session;
use ort::value::Tensor;
use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use super::segments::{OverlapWindow, SegmentPlan};
use super::{
    STEMS_CHANNELS, SeparationObserver, SeparationStage, Stem, StemSink, build_session,
    check_channels, model, runtime_error, torch_checkpoint, write_blocks,
};
use crate::error::EuphoriaError;

/// How many samples of each channel the network separates at a time.
pub(crate) const SEGMENT_SAMPLES: usize = 242_550;
const WINDOW_LENGTH: usize = 4096;
const HOP: usize = 1024;
/// The frequency bins of the transform.
pub(crate) const FREQUENCY_BINS: usize = WINDOW_LENGTH / 2 + 1;

/// The zeros the network appends to a segment so that its transform has an
/// even number of hops.
const fn tail_padding(samples: usize) -> usize {
    let mut padding = HOP - samples % HOP;
    if ((samples + padding) / HOP).is_multiple_of(2) {
        padding += HOP;
    }
    padding
}

const TAIL_PADDING: usize = tail_padding(SEGMENT_SAMPLES);
const PADDED_SAMPLES: usize = SEGMENT_SAMPLES + TAIL_PADDING;
/// The frames of the transform of one segment.
pub(crate) const FRAMES: usize = PADDED_SAMPLES / HOP + 1;
/// The real and imaginary parts of the left and the right channel.
pub(crate) const SPECTRUM_CHANNELS: usize = 4;

/// The transform that feeds the network and the inverse that turns its
/// output back into audio.
pub(crate) struct ScnetTransforms {
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
}

impl ScnetTransforms {
    pub(crate) fn new() -> Self {
        let mut planner = FftPlanner::<f32>::new();
        Self {
            forward: planner.plan_fft_forward(WINDOW_LENGTH),
            inverse: planner.plan_fft_inverse(WINDOW_LENGTH),
        }
    }

    /// Fills `output` with the network's input for one segment of both
    /// channels, each [`SEGMENT_SAMPLES`] long: four planes of
    /// [`FREQUENCY_BINS`] by [`FRAMES`] values, the left real part first,
    /// then the left imaginary part, the right real part and the right
    /// imaginary part.
    pub(crate) fn spectrum(&self, channels: [&[f32]; 2], output: &mut Vec<f32>) {
        let plane = FREQUENCY_BINS * FRAMES;
        output.clear();
        output.resize(SPECTRUM_CHANNELS * plane, 0.0);
        let scale = 1.0 / (WINDOW_LENGTH as f32).sqrt();
        let mut padded = vec![0.0f32; PADDED_SAMPLES];
        let mut buffer = vec![Complex::new(0.0f32, 0.0f32); WINDOW_LENGTH];
        for (channel_index, channel) in channels.iter().enumerate() {
            padded[..SEGMENT_SAMPLES].copy_from_slice(channel);
            padded[SEGMENT_SAMPLES..].fill(0.0);
            let (real_plane, imaginary_plane) = {
                let start = channel_index * 2 * plane;
                let (real, rest) = output[start..].split_at_mut(plane);
                (real, &mut rest[..plane])
            };
            for frame in 0..FRAMES {
                let origin = (frame * HOP) as isize - (WINDOW_LENGTH / 2) as isize;
                for (offset, slot) in buffer.iter_mut().enumerate() {
                    let position = reflect(origin + offset as isize, PADDED_SAMPLES);
                    *slot = Complex::new(padded[position], 0.0);
                }
                self.forward.process(&mut buffer);
                for (bin, value) in buffer.iter().take(FREQUENCY_BINS).enumerate() {
                    real_plane[bin * FRAMES + frame] = value.re * scale;
                    imaginary_plane[bin * FRAMES + frame] = value.im * scale;
                }
            }
        }
    }

    /// Turns one channel of one stem from the network's output, given as
    /// its real and imaginary planes of [`FREQUENCY_BINS`] by [`FRAMES`]
    /// values, into `output`, [`SEGMENT_SAMPLES`] long.
    pub(crate) fn waveform(&self, real: &[f32], imaginary: &[f32], output: &mut [f32]) {
        let length = WINDOW_LENGTH + HOP * (FRAMES - 1);
        let mut accumulator = vec![0.0f32; length];
        let mut overlap = vec![0.0f32; length];
        let mut buffer = vec![Complex::new(0.0f32, 0.0f32); WINDOW_LENGTH];
        let scale = (WINDOW_LENGTH as f32).sqrt() / WINDOW_LENGTH as f32;
        for frame in 0..FRAMES {
            for bin in 0..FREQUENCY_BINS {
                let real_part = real[bin * FRAMES + frame];
                let imaginary_part = if bin == 0 || bin == FREQUENCY_BINS - 1 {
                    0.0
                } else {
                    imaginary[bin * FRAMES + frame]
                };
                buffer[bin] = Complex::new(real_part, imaginary_part);
                if bin > 0 && bin < FREQUENCY_BINS - 1 {
                    buffer[WINDOW_LENGTH - bin] = Complex::new(real_part, -imaginary_part);
                }
            }
            self.inverse.process(&mut buffer);
            let origin = frame * HOP;
            for (offset, value) in buffer.iter().enumerate() {
                accumulator[origin + offset] += value.re * scale;
                overlap[origin + offset] += 1.0;
            }
        }
        let start = WINDOW_LENGTH / 2;
        for (index, sample) in output.iter_mut().enumerate() {
            *sample = accumulator[start + index] / overlap[start + index];
        }
    }
}

/// The index of the sample that reflection padding puts at `index`: the
/// signal mirrored around its first and last samples, which are not
/// repeated.
fn reflect(index: isize, length: usize) -> usize {
    let last = length as isize - 1;
    let mut position = index.abs();
    if position > last {
        position = 2 * last - position;
    }
    position as usize
}

/// The SCNet XL IHF network loaded and ready to separate tracks. Loading
/// takes a few seconds and holds the weights in memory, so keep one for many
/// tracks.
pub struct AdvancedSeparator {
    session: Session,
    _environment: ort::environment::Environment,
    transforms: ScnetTransforms,
}

impl std::fmt::Debug for AdvancedSeparator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AdvancedSeparator")
            .finish_non_exhaustive()
    }
}

impl AdvancedSeparator {
    #[cfg(test)]
    pub(crate) fn for_tests(session: Session, environment: ort::environment::Environment) -> Self {
        Self {
            session,
            _environment: environment,
            transforms: ScnetTransforms::new(),
        }
    }

    /// Loads the model with the weights at `weights_path`, downloading them
    /// there first if the file is missing.
    pub(crate) fn load_from(
        weights_path: &Path,
        observer: &dyn SeparationObserver,
    ) -> Result<Self, EuphoriaError> {
        observer.stage(SeparationStage::LoadingModel);
        let bytes = model::load_weights(
            &model::ADVANCED_WEIGHTS,
            weights_path,
            &|received, total| observer.downloading(received, total),
        )?;
        let tensors = torch_checkpoint::read_torch_checkpoint(&bytes).map_err(|reason| {
            EuphoriaError::Separation {
                reason: format!(
                    "cannot read the weights at {}: {reason}",
                    weights_path.display()
                ),
                source: None,
            }
        })?;
        drop(bytes);
        let (session, environment) = build_session(model::ADVANCED_GRAPH, tensors)?;
        Ok(Self {
            session,
            _environment: environment,
            transforms: ScnetTransforms::new(),
        })
    }

    /// Separates stereo audio at 44.1 kHz, given as one slice per channel of
    /// equal length, and hands the stems to `sink` in order as soon as their
    /// blend is final, so that only about one segment of output is held at a
    /// time. Returns the frames written. The audio is not normalised first:
    /// this network was trained on audio as it comes.
    pub(crate) fn separate_into(
        &mut self,
        left: &[f32],
        right: &[f32],
        observer: &dyn SeparationObserver,
        sink: &mut dyn StemSink,
    ) -> Result<usize, EuphoriaError> {
        check_channels(left, right)?;
        observer.stage(SeparationStage::Separating);
        let frames = left.len();
        let plan = SegmentPlan::new(frames, SEGMENT_SAMPLES);
        let source_channels = Stem::ALL.len() * STEMS_CHANNELS;
        let mut window = OverlapWindow::new(source_channels);
        let mut finished = vec![Vec::new(); source_channels];
        let mut blocks: [Vec<f32>; 4] = Default::default();
        let mut spectrum = Vec::new();
        let mut segment_waveform = vec![vec![0.0f32; SEGMENT_SAMPLES]; STEMS_CHANNELS];
        let mut separated_segment = vec![0.0f32; SEGMENT_SAMPLES];
        let plane = FREQUENCY_BINS * FRAMES;
        observer.separated(0, plan.len());
        for (done, segment) in plan.segments().enumerate() {
            for (channel, waveform) in segment_waveform.iter_mut().enumerate() {
                let samples = if channel == 0 { left } else { right };
                segment.read_padded(samples, waveform, |sample| sample);
            }
            self.transforms
                .spectrum([&segment_waveform[0], &segment_waveform[1]], &mut spectrum);
            let input = Tensor::from_array((
                [1usize, SPECTRUM_CHANNELS, FREQUENCY_BINS, FRAMES],
                std::mem::take(&mut spectrum),
            ))
            .map_err(runtime_error("cannot hold the spectrum"))?;
            let outputs = self
                .session
                .run(ort::inputs!["spectrum" => input])
                .map_err(runtime_error("the network failed on a segment"))?;
            let (_, features) = outputs["features"]
                .try_extract_tensor::<f32>()
                .map_err(runtime_error("the network returned no spectrum"))?;
            for stem in Stem::ALL {
                for channel in 0..STEMS_CHANNELS {
                    let source_channel = stem.index() * STEMS_CHANNELS + channel;
                    let start = (stem.index() * SPECTRUM_CHANNELS + channel * 2) * plane;
                    self.transforms.waveform(
                        &features[start..start + plane],
                        &features[start + plane..start + 2 * plane],
                        &mut separated_segment,
                    );
                    window.add(&segment, source_channel, &separated_segment);
                }
            }
            drop(outputs);
            window.take_until(segment.final_until(), &mut finished);
            write_blocks(&finished, &mut blocks, |sample| sample, sink)?;
            observer.separated(done + 1, plan.len());
        }
        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_segment_has_the_frames_the_network_was_exported_for() {
        assert_eq!(TAIL_PADDING, 138);
        assert_eq!(PADDED_SAMPLES, 242_688);
        assert_eq!(FRAMES, 238);
        assert_eq!(FREQUENCY_BINS, 2049);
    }

    #[test]
    fn reflection_mirrors_around_the_edge_samples() {
        assert_eq!(reflect(-1, 10), 1);
        assert_eq!(reflect(-3, 10), 3);
        assert_eq!(reflect(0, 10), 0);
        assert_eq!(reflect(9, 10), 9);
        assert_eq!(reflect(10, 10), 8);
        assert_eq!(reflect(12, 10), 6);
    }

    #[test]
    fn a_constant_signal_has_all_its_energy_in_the_first_bin() {
        let transforms = ScnetTransforms::new();
        let signal = vec![0.5f32; SEGMENT_SAMPLES];
        let mut spectrum = Vec::new();
        transforms.spectrum([&signal, &signal], &mut spectrum);
        let plane = FREQUENCY_BINS * FRAMES;
        assert_eq!(spectrum.len(), SPECTRUM_CHANNELS * plane);
        let frame = 100;
        let expected = 0.5 * WINDOW_LENGTH as f32 / (WINDOW_LENGTH as f32).sqrt();
        for channel_plane in [0, 2] {
            let real = &spectrum[channel_plane * plane..(channel_plane + 1) * plane];
            assert!((real[frame] - expected).abs() < 1e-3, "{}", real[frame]);
            for bin in [1, 2, 100, 2048] {
                assert!(real[bin * FRAMES + frame].abs() < 1e-3, "bin {bin}");
            }
        }
    }

    #[test]
    fn the_inverse_gives_back_the_segment_that_went_in() {
        let transforms = ScnetTransforms::new();
        let signal: Vec<f32> = (0..SEGMENT_SAMPLES)
            .map(|index| (index as f32 * 0.01).sin() * 0.3 + (index as f32 * 0.37).cos() * 0.1)
            .collect();
        let other: Vec<f32> = signal.iter().map(|sample| -sample * 0.5).collect();
        let mut spectrum = Vec::new();
        transforms.spectrum([&signal, &other], &mut spectrum);
        let plane = FREQUENCY_BINS * FRAMES;
        let mut restored = vec![0.0f32; SEGMENT_SAMPLES];
        transforms.waveform(
            &spectrum[..plane],
            &spectrum[plane..2 * plane],
            &mut restored,
        );
        let worst = signal
            .iter()
            .zip(&restored)
            .map(|(expected, actual)| (expected - actual).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-4, "{worst}");
        transforms.waveform(
            &spectrum[2 * plane..3 * plane],
            &spectrum[3 * plane..],
            &mut restored,
        );
        let worst = other
            .iter()
            .zip(&restored)
            .map(|(expected, actual)| (expected - actual).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-4, "{worst}");
    }

    fn signal_to_error_db(estimate: &[f32], expected: &[f32]) -> f64 {
        let signal: f64 = expected.iter().map(|value| f64::from(*value).powi(2)).sum();
        let noise: f64 = estimate
            .iter()
            .zip(expected)
            .map(|(estimated, wanted)| f64::from(estimated - wanted).powi(2))
            .sum();
        10.0 * ((signal + 1e-12) / (noise + 1e-12)).log10()
    }

    #[test]
    fn the_real_network_matches_the_python_reference_when_given_its_files() {
        let (Some(weights), Some(golden)) = (
            std::env::var_os("STEMS_ADVANCED_WEIGHTS"),
            std::env::var_os("STEMS_ADVANCED_GOLDEN"),
        ) else {
            eprintln!("set STEMS_ADVANCED_WEIGHTS and STEMS_ADVANCED_GOLDEN to run this test");
            return;
        };
        let golden = std::path::PathBuf::from(golden);
        let mut separator =
            AdvancedSeparator::load_from(Path::new(&weights), &crate::stems::Unobserved).unwrap();
        let [left, right] = crate::stems::input::decode_stereo(
            &golden.join("excerpt.wav"),
            crate::stems::STEMS_SAMPLE_RATE,
        )
        .unwrap();
        let mut collected = crate::stems::CollectedStems::default();
        separator
            .separate_into(&left, &right, &crate::stems::Unobserved, &mut collected)
            .unwrap();
        for stem in Stem::ALL {
            let [expected_left, expected_right] = crate::stems::input::decode_stereo(
                &golden.join(format!("expected_{}.wav", stem.name())),
                crate::stems::STEMS_SAMPLE_RATE,
            )
            .unwrap();
            let interleaved = &collected.interleaved[stem.index()];
            let estimate_left: Vec<f32> = interleaved.iter().step_by(2).copied().collect();
            let estimate_right: Vec<f32> = interleaved.iter().skip(1).step_by(2).copied().collect();
            let left_db = signal_to_error_db(&estimate_left, &expected_left);
            let right_db = signal_to_error_db(&estimate_right, &expected_right);
            eprintln!(
                "{}: left {left_db:.1} dB, right {right_db:.1} dB",
                stem.name()
            );
            assert!(left_db > 35.0 && right_db > 35.0, "{}", stem.name());
        }
    }
}
