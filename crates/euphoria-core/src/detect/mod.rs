//! Key and tempo detectors behind common traits, so the benchmark and the
//! product can swap implementations.

mod keyfinder;
mod stratum;

pub use keyfinder::KeyFinderDetector;
pub use stratum::StratumDetector;

use crate::audio::AudioBuffer;
use crate::error::EuphoriaError;
use crate::key::Key;

pub trait KeyDetector: Sync {
    fn name(&self) -> &'static str;

    /// Returns `None` when the audio has no detectable key, such as silence.
    fn detect_key(&self, audio: &AudioBuffer) -> Result<Option<Key>, EuphoriaError>;
}

pub trait TempoDetector: Sync {
    fn name(&self) -> &'static str;

    /// Tempo in beats per minute.
    fn detect_tempo(&self, audio: &AudioBuffer) -> Result<f32, EuphoriaError>;
}
