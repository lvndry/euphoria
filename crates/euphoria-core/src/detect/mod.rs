// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Key and tempo detectors behind common traits, so the benchmark and the
//! product can swap implementations.

mod beat_grid;
#[cfg(feature = "libkeyfinder")]
mod chromagram;
mod cues;
mod decimate;
#[cfg(feature = "libkeyfinder")]
mod key_sections;
#[cfg(feature = "libkeyfinder")]
mod keyfinder;
mod stratum;
#[cfg(test)]
mod synthesis;
mod tuning;

pub use beat_grid::{BeatGrid, detect_beat_grid};
pub use cues::{CueSection, MAXIMUM_CUES, SectionCue, detect_beat_grid_and_cues};
#[cfg(feature = "libkeyfinder")]
pub use keyfinder::{KeyDecision, KeyFinderDetector, KeyFinderVariant};
pub use stratum::StratumDetector;
pub use tuning::{Tuning, measure_tuning};

use crate::audio::AudioBuffer;
use crate::confidence::{Confidence, key_confidence, tempo_confidence};
use crate::error::EuphoriaError;
use crate::key::Key;
use crate::tempo_range::TempoRange;

/// Estimates the musical key of a track.
pub trait KeyDetector: Sync {
    /// Short name of the implementation, used in reports.
    fn name(&self) -> &'static str;

    /// Returns `Ok(None)` when the audio has no detectable key, such as
    /// silence. Errors are reserved for analysis that failed.
    fn detect_key(&self, audio: &AudioBuffer) -> Result<Option<Key>, EuphoriaError>;
}

/// Estimates the tempo of a track.
pub trait TempoDetector: Sync {
    /// Short name of the implementation, used in reports.
    fn name(&self) -> &'static str;

    /// Tempo in beats per minute, or `Ok(None)` when the audio has no
    /// detectable tempo, such as silence. Errors are reserved for analysis
    /// that failed.
    fn detect_tempo(&self, audio: &AudioBuffer) -> Result<Option<f32>, EuphoriaError>;
}

/// The key and tempo of one track. Either is `None` when it could not
/// be detected.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Analysis {
    /// The detected key.
    pub key: Option<Key>,
    /// The detected tempo in beats per minute.
    pub tempo: Option<f32>,
}

/// A stretch of a track that is in one key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KeySection {
    /// Where the stretch starts, in seconds from the start of the track.
    pub start_seconds: f32,
    /// Where the stretch ends, in seconds from the start of the track.
    pub end_seconds: f32,
    /// The key of the stretch.
    pub key: Key,
}

/// What a key detector found in one track beyond its key.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KeyReading {
    /// The detected key, or `None` when the track has no detectable key.
    pub key: Option<Key>,
    /// The key that matched the track second best, or `None` when there is
    /// no key or the detector does not rank keys.
    pub runner_up: Option<Key>,
    /// How far the score of the key is above that of the runner up, the
    /// basis of [`KeyReading::confidence`]. `None` when there is no
    /// runner up.
    pub score_margin: Option<f32>,
    /// How far the track is tuned from A = 440 Hz, in cents from -50 to 50
    /// with sharp as positive, or `None` when it could not be measured.
    pub tuning_cents: Option<f32>,
    /// The key over time: sections in order that together cover the track,
    /// except where it is silent, which belongs to no section. One section
    /// when no change of key was found and the track has no silence, none
    /// when the track has no key or the detector does not follow the key
    /// over time.
    pub timeline: Vec<KeySection>,
}

/// A tempo estimate before it is placed in a [`TempoRange`], with what its
/// confidence is based on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoReading {
    /// The tempo in beats per minute as the detector estimated it, which is
    /// often half or double the tempo a DJ counts.
    pub estimate: f32,
    /// How far the tempogram scored the estimate above the best tempo that
    /// is not an octave of it, the basis of [`TempoReading::confidence`].
    /// `None` when the tempogram gave no estimate and a fallback did.
    pub score_margin: Option<f32>,
}

impl TempoReading {
    /// The tempo reported in `range`.
    pub fn tempo_in(&self, range: TempoRange) -> f32 {
        range.fold(self.estimate)
    }

    /// How far to trust the beat period, whatever octave it is counted in.
    pub fn confidence(&self) -> Confidence {
        tempo_confidence(self.score_margin)
    }
}

impl KeyReading {
    /// How far to trust the key, or `None` when there is no key or the
    /// detector does not rank keys.
    pub fn confidence(&self) -> Option<Confidence> {
        self.key.and(self.score_margin).map(key_confidence)
    }
}
