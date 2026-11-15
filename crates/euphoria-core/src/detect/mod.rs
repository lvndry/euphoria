// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Key and tempo detectors behind common traits, so the benchmark and the
//! product can swap implementations.

mod beat_grid;
mod cues;
mod decimate;
#[cfg(feature = "drone")]
mod drone;
#[cfg(feature = "drone")]
mod key_sections;
mod note_spectrum;
mod sections;
mod stratum;
#[cfg(test)]
mod synthesis;
mod tempo_choice;
mod tuning;

pub use beat_grid::{BeatGrid, detect_beat_grid};
pub use cues::{CueSection, MAXIMUM_CUES, SectionCue, cues_on_grid, detect_beat_grid_and_cues};
#[cfg(feature = "drone")]
pub use drone::{DroneDetector, KeyProbabilities};
pub(crate) use note_spectrum::pitch_class_profile as pitch_class_profile_at;
pub use stratum::StratumDetector;
pub use tuning::{Tuning, measure_tuning};

use crate::audio::AudioBuffer;
use crate::confidence::{Confidence, key_confidence, tempo_confidence};
use crate::error::EuphoriaError;
use crate::key::Key;
use crate::key_source::KeySource;
use crate::tempo_range::TempoRange;

/// Reads the key of a track: drone-v2 decides the key and follows it over
/// time, after a track measured at least 20 cents from concert pitch is
/// brought back to it. In builds without the `drone` feature, `stratum-dsp`
/// finds the key, with no margin and no key over time, and the tuning is
/// measured without being corrected for.
pub fn read_key(audio: &AudioBuffer) -> Result<KeyReading, EuphoriaError> {
    let tuning_cents = measure_tuning(audio).map(|tuning| tuning.cents);
    #[cfg(feature = "drone")]
    {
        let reading = DroneDetector.read_key_over_time(audio, tuning_cents)?;
        let pitch_class_profile = reading
            .key
            .and_then(|_| crate::pitch_classes::profile_of_audio(audio, tuning_cents));
        Ok(KeyReading {
            tuning_cents,
            pitch_class_profile,
            ..reading
        })
    }
    #[cfg(not(feature = "drone"))]
    {
        let key = StratumDetector.detect_key(audio)?;
        Ok(KeyReading {
            key,
            tuning_cents,
            source: key.map(|_| KeySource::Stratum),
            pitch_class_profile: key
                .and_then(|_| crate::pitch_classes::profile_of_audio(audio, tuning_cents)),
            ..KeyReading::default()
        })
    }
}

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
    /// The detector that decided the key, which sets how its confidence is
    /// rated. `None` when there is no key.
    pub source: Option<KeySource>,
    /// How strongly each of the 12 notes sounds in the track, C first, from
    /// 0 to 1 with the strongest at 1, as Euphoria's note spectrum hears it
    /// on the scale of the measured tuning. `None` when there is no key.
    pub pitch_class_profile: Option<[f32; 12]>,
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
        self.key
            .and(self.score_margin)
            .map(|margin| key_confidence(self.source, margin))
    }
}
