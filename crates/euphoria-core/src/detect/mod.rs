// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Key and tempo detectors behind common traits, so the benchmark and the
//! product can swap implementations.

mod keyfinder;
mod stratum;

pub use keyfinder::KeyFinderDetector;
pub use stratum::StratumDetector;

use crate::audio::AudioBuffer;
use crate::error::EuphoriaError;
use crate::key::Key;

/// Estimates the musical key of a track.
pub trait KeyDetector: Sync {
    /// Short name of the implementation, used in reports.
    fn name(&self) -> &'static str;

    /// Returns `None` when the audio has no detectable key, such as silence.
    fn detect_key(&self, audio: &AudioBuffer) -> Result<Option<Key>, EuphoriaError>;
}

/// Estimates the tempo of a track.
pub trait TempoDetector: Sync {
    /// Short name of the implementation, used in reports.
    fn name(&self) -> &'static str;

    /// Tempo in beats per minute.
    fn detect_tempo(&self, audio: &AudioBuffer) -> Result<f32, EuphoriaError>;
}
