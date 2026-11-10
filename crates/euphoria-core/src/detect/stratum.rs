// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use stratum_dsp::{AnalysisConfig, Key as StratumKey, analyze_audio};

use crate::audio::AudioBuffer;
use crate::detect::{KeyDetector, TempoDetector};
use crate::error::EuphoriaError;
use crate::key::{Key, Mode};

/// Key and tempo detection through the pure Rust `stratum-dsp` crate.
#[derive(Debug, Default, Clone, Copy)]
pub struct StratumDetector;

fn analysis_error(error: stratum_dsp::AnalysisError) -> EuphoriaError {
    EuphoriaError::Detection {
        detector: "stratum-dsp",
        reason: error.to_string(),
    }
}

impl KeyDetector for StratumDetector {
    fn name(&self) -> &'static str {
        "stratum-dsp"
    }

    fn detect_key(&self, audio: &AudioBuffer) -> Result<Option<Key>, EuphoriaError> {
        if audio.is_silent() {
            return Ok(None);
        }
        let result = analyze_audio(&audio.samples, audio.sample_rate, AnalysisConfig::default())
            .map_err(analysis_error)?;
        let key = match result.key {
            StratumKey::Major(tonic) => Key::new((tonic % 12) as u8, Mode::Major),
            StratumKey::Minor(tonic) => Key::new((tonic % 12) as u8, Mode::Minor),
        };
        Ok(Some(key))
    }
}

impl TempoDetector for StratumDetector {
    fn name(&self) -> &'static str {
        "stratum-dsp"
    }

    fn detect_tempo(&self, audio: &AudioBuffer) -> Result<Option<f32>, EuphoriaError> {
        if audio.is_silent() {
            return Ok(None);
        }
        let result = analyze_audio(&audio.samples, audio.sample_rate, AnalysisConfig::default())
            .map_err(analysis_error)?;
        Ok(Some(result.bpm))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_has_no_key_and_no_tempo() {
        let silence = AudioBuffer {
            samples: vec![0.0; 44_100 * 2],
            sample_rate: 44_100,
        };
        assert_eq!(StratumDetector.detect_key(&silence).unwrap(), None);
        assert_eq!(StratumDetector.detect_tempo(&silence).unwrap(), None);
    }
}
