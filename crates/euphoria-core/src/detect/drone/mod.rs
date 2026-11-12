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
    let mode = if class % 2 == 1 { Mode::Minor } else { Mode::Major };
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
    pub fn probabilities(&self, audio: &AudioBuffer) -> Result<Option<KeyProbabilities>, EuphoriaError> {
        if audio.is_silent() {
            return Ok(None);
        }
        let network = network()?;
        let samples = front_end::to_model_input(audio.samples(), audio.sample_rate().get());
        let features = cqt::features(&samples);
        Ok(network
            .logits(&features.values, features.frame_count)
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
        let network = network()?;
        let features = cqt::features(samples);
        let probabilities = network
            .logits(&features.values, features.frame_count)
            .map(|logits| KeyProbabilities::from_logits(&logits));
        Ok((features.values, features.frame_count, probabilities))
    }

    /// Like [`DroneDetector::inspect_model_input`] for decoded audio at any
    /// sample rate, converted as [`DroneDetector::probabilities`] does.
    #[doc(hidden)]
    pub fn inspect_decoded(
        &self,
        audio: &AudioBuffer,
    ) -> Result<(Vec<f32>, usize, Option<KeyProbabilities>), EuphoriaError> {
        let samples = front_end::to_model_input(audio.samples(), audio.sample_rate().get());
        self.inspect_model_input(&samples)
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

/// The reading the probabilities give: the most likely key, the runner up,
/// and how far apart their probabilities are.
pub(crate) fn reading_of(probabilities: &KeyProbabilities) -> KeyReading {
    let ranked = probabilities.ranked();
    KeyReading {
        key: Some(ranked[0].0),
        runner_up: Some(ranked[1].0),
        score_margin: Some(ranked[0].1 - ranked[1].1),
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
    fn silence_and_short_audio_have_no_key() {
        let rate = std::num::NonZeroU32::new(44_100).unwrap();
        let silent = AudioBuffer::new(vec![0.0; 44_100 * 3], rate);
        assert_eq!(DroneDetector.detect_key(&silent).unwrap(), None);
        let short = AudioBuffer::new(vec![0.1; 44_100], rate);
        assert_eq!(DroneDetector.detect_key(&short).unwrap(), None);
    }
}
