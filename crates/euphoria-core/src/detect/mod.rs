// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Key and tempo detectors behind common traits, so the benchmark and the
//! product can swap implementations.

#[cfg(feature = "libkeyfinder")]
mod keyfinder;
mod stratum;

#[cfg(feature = "libkeyfinder")]
pub use keyfinder::KeyFinderDetector;
pub use stratum::StratumDetector;

use crate::audio::AudioBuffer;
use crate::error::EuphoriaError;
use crate::key::Key;

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
