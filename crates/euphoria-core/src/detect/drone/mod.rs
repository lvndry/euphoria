// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Key detection with drone-v1, a small convolutional network trained on
//! labelled dance music previews, run on the CPU in plain Rust.

mod cqt;
mod front_end;
mod halving;
mod network;

use std::sync::OnceLock;

use crate::audio::AudioBuffer;
use crate::detect::{KeyDetector, KeyReading};
use crate::error::EuphoriaError;
use crate::key::{Key, Mode};
use crate::key_engine::KeySource;

use network::{KEY_COUNT, Network};

/// Name of the drone-v1 detector in reports.
pub(crate) const DETECTOR_NAME: &str = "drone-v1";

/// How likely the network finds each of the 24 keys in a track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeyProbabilities {
    probabilities: [f32; KEY_COUNT],
}

impl KeyProbabilities {
    fn from_logits(logits: &[f32; KEY_COUNT]) -> Self {
        let highest = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let exponentials: [f64; KEY_COUNT] =
            std::array::from_fn(|index| f64::from(logits[index] - highest).exp());
        let total: f64 = exponentials.iter().sum();
        Self {
            probabilities: std::array::from_fn(|index| (exponentials[index] / total) as f32),
        }
    }

    /// The probability of `key`, from 0 to 1. The 24 sum to one. They are
    /// the network's softmax output and are not calibrated: a probability
    /// of 0.8 does not mean the key is right four times in five.
    pub fn of(&self, key: Key) -> f32 {
        let index = usize::from(key.tonic()) * 2 + usize::from(key.mode() == Mode::Minor);
        self.probabilities[index]
    }

    /// The 24 probabilities in drone's class order: C major, C minor,
    /// C sharp major and so on.
    pub fn in_class_order(&self) -> [f32; 24] {
        self.probabilities
    }

    /// Every key with its probability, most likely first. Equal
    /// probabilities keep the network's order, C major first.
    pub fn ranked(&self) -> Vec<(Key, f32)> {
        let mut ranked: Vec<(Key, f32)> = self
            .probabilities
            .iter()
            .enumerate()
            .map(|(index, probability)| (key_of_class(index), *probability))
            .collect();
        ranked.sort_by(|left, right| right.1.total_cmp(&left.1));
        ranked
    }
}

fn key_of_class(class: usize) -> Key {
    let mode = if class % 2 == 1 {
        Mode::Minor
    } else {
        Mode::Major
    };
    Key::new((class / 2) as u8, mode)
}

fn network() -> Result<&'static Network, EuphoriaError> {
    static NETWORK: OnceLock<Result<Network, String>> = OnceLock::new();
    NETWORK
        .get_or_init(|| Network::shipped().map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|reason| EuphoriaError::Detection {
            detector: DETECTOR_NAME,
            reason: reason.clone(),
            source: None,
        })
}

/// Key detection with the drone-v1 network.
#[derive(Debug, Default, Clone, Copy)]
pub struct DroneDetector;

impl DroneDetector {
    /// How likely each key is, or `None` when the track is silent or
    /// shorter than about 1.5 seconds, too short for the network.
    pub fn probabilities(
        &self,
        audio: &AudioBuffer,
    ) -> Result<Option<KeyProbabilities>, EuphoriaError> {
        if audio.is_silent() {
            return Ok(None);
        }
        let network = network()?;
        let (values, frame_count) = features_of_audio(audio, SEGMENT_FRAMES);
        Ok(network
            .logits(&values, frame_count)
            .map(|logits| KeyProbabilities::from_logits(&logits)))
    }

    /// The features and key probabilities of audio already converted to
    /// what the network reads: mono, 16 bit, at 22,050 Hz. The features are
    /// 192 rows from C1 upwards stored row after row, and the frame count.
    /// For checking the front end against drone's own code.
    #[doc(hidden)]
    pub fn inspect_model_input(
        &self,
        samples: &[f32],
    ) -> Result<(Vec<f32>, usize, Option<KeyProbabilities>), EuphoriaError> {
        read_model_input(samples.to_vec())
    }

    /// Like [`DroneDetector::inspect_model_input`] for decoded audio at any
    /// sample rate, converted as [`DroneDetector::probabilities`] does.
    #[doc(hidden)]
    pub fn inspect_decoded(
        &self,
        audio: &AudioBuffer,
    ) -> Result<(Vec<f32>, usize, Option<KeyProbabilities>), EuphoriaError> {
        let network = network()?;
        let (values, frame_count) = features_of_audio(audio, SEGMENT_FRAMES);
        let probabilities = network
            .logits(&values, frame_count)
            .map(|logits| KeyProbabilities::from_logits(&logits));
        Ok((values, frame_count, probabilities))
    }

    /// The most likely key and the runner up, with the margin between their
    /// probabilities. No tuning is measured and no key over time is given.
    pub fn read_key(&self, audio: &AudioBuffer) -> Result<KeyReading, EuphoriaError> {
        Ok(self
            .probabilities(audio)?
            .map(|probabilities| reading_of(&probabilities))
            .unwrap_or_default())
    }
}

/// Frames of features computed at once, about three minutes. Memory stays
/// near what one segment needs however long the track is.
const SEGMENT_FRAMES: usize = 1_024;
/// Frames of audio read on either side of a segment. A frame of the lowest
/// octave depends on about 16 frames of audio on either side, through seven
/// halvings of the rate and its own window, so the features of a segment are
/// those of the whole track.
const CONTEXT_FRAMES: usize = 24;

/// The features of a track, converted to the network's input a segment at
/// a time, as 192 rows of frames stored row after row, and the frame count.
fn features_of_audio(audio: &AudioBuffer, segment_frames: usize) -> (Vec<f32>, usize) {
    let samples = audio.samples();
    let converter = front_end::Converter::new(audio.sample_rate().get());
    let total_samples = converter.output_length(samples.len());
    let frame_count = cqt::frame_count(total_samples);
    let mut values = vec![0.0; cqt::CORE_BINS * frame_count];
    let mut first_frame = 0;
    while first_frame < frame_count {
        let end_frame = first_frame.saturating_add(segment_frames).min(frame_count);
        let read_from_frame = first_frame.saturating_sub(CONTEXT_FRAMES);
        let read_from = read_from_frame * cqt::HOP;
        let read_to = ((end_frame + CONTEXT_FRAMES) * cqt::HOP).min(total_samples);
        let segment = cqt::features(converter.convert(samples, read_from..read_to));
        let offset = first_frame - read_from_frame;
        let width = end_frame - first_frame;
        for row in 0..cqt::CORE_BINS {
            let source_start = row * segment.frame_count + offset;
            let target_start = row * frame_count + first_frame;
            values[target_start..target_start + width]
                .copy_from_slice(&segment.values[source_start..source_start + width]);
        }
        first_frame = end_frame;
    }
    (values, frame_count)
}

/// The features of audio in the network's input form, their frame count,
/// and the key probabilities when there are enough frames.
fn read_model_input(
    samples: Vec<f32>,
) -> Result<(Vec<f32>, usize, Option<KeyProbabilities>), EuphoriaError> {
    let network = network()?;
    let features = cqt::features(samples);
    let probabilities = network
        .logits(&features.values, features.frame_count)
        .map(|logits| KeyProbabilities::from_logits(&logits));
    Ok((features.values, features.frame_count, probabilities))
}

/// The reading the probabilities give: the most likely key, the runner up,
/// and how far apart their probabilities are.
pub(crate) fn reading_of(probabilities: &KeyProbabilities) -> KeyReading {
    let ranked = probabilities.ranked();
    KeyReading {
        key: Some(ranked[0].0),
        runner_up: Some(ranked[1].0),
        score_margin: Some(ranked[0].1 - ranked[1].1),
        source: Some(KeySource::Drone),
        ..KeyReading::default()
    }
}

impl KeyDetector for DroneDetector {
    fn name(&self) -> &'static str {
        DETECTOR_NAME
    }

    fn detect_key(&self, audio: &AudioBuffer) -> Result<Option<Key>, EuphoriaError> {
        Ok(self.read_key(audio)?.key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_follow_the_drone_label_schema() {
        assert_eq!(key_of_class(0), Key::new(0, Mode::Major));
        assert_eq!(key_of_class(1), Key::new(0, Mode::Minor));
        assert_eq!(key_of_class(19), Key::new(9, Mode::Minor));
        let mut logits = [0.0; KEY_COUNT];
        logits[19] = 5.0;
        let probabilities = KeyProbabilities::from_logits(&logits);
        assert_eq!(probabilities.ranked()[0].0, Key::new(9, Mode::Minor));
        let total: f32 = probabilities.probabilities.iter().sum();
        assert!((total - 1.0).abs() < 1e-5);
    }

    #[test]
    fn segments_give_the_features_of_the_whole_track() {
        let rate = 44_100;
        let samples: Vec<f32> = (0..rate * 30)
            .map(|index| {
                let time = index as f32 / rate as f32;
                0.3 * (std::f32::consts::TAU * 220.0 * time).sin()
                    + 0.2 * (std::f32::consts::TAU * 61.7 * time * (1.0 + time / 60.0)).sin()
            })
            .collect();
        let audio = AudioBuffer::new(samples, std::num::NonZeroU32::new(rate as u32).unwrap());
        let (whole, whole_frames) = features_of_audio(&audio, usize::MAX);
        let (segmented, segmented_frames) = features_of_audio(&audio, 40);
        assert_eq!(whole_frames, segmented_frames);
        let largest_difference = whole
            .iter()
            .zip(&segmented)
            .map(|(left, right)| (left - right).abs())
            .fold(0.0_f32, f32::max);
        assert!(largest_difference <= 0.002, "{largest_difference}");
    }

    #[test]
    fn silence_and_short_audio_have_no_key() {
        let rate = std::num::NonZeroU32::new(44_100).unwrap();
        let silent = AudioBuffer::new(vec![0.0; 44_100 * 3], rate);
        assert_eq!(DroneDetector.detect_key(&silent).unwrap(), None);
        let short = AudioBuffer::new(vec![0.1; 44_100], rate);
        assert_eq!(DroneDetector.detect_key(&short).unwrap(), None);
    }
}
