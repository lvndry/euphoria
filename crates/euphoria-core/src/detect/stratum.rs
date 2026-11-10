// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use stratum_dsp::{AnalysisConfig, Key as StratumKey, analyze_audio};

use crate::audio::AudioBuffer;
use crate::detect::{Analysis, KeyDetector, TempoDetector};
use crate::error::EuphoriaError;
use crate::key::{Key, Mode};

/// Key and tempo detection through the pure Rust `stratum-dsp` crate.
///
/// `stratum-dsp` computes key and tempo in one pass, so callers that need
/// both should use [`StratumDetector::analyze`] rather than the two trait
/// methods, which each run the full analysis.
#[derive(Debug, Default, Clone, Copy)]
pub struct StratumDetector;

/// Below this length `stratum-dsp` still answers, but with a key and tempo
/// that do not come from the audio.
const MINIMUM_ANALYSIS_SECONDS: f32 = 1.0;

impl StratumDetector {
    /// Runs one analysis and returns both key and tempo. Silent audio, or
    /// audio shorter than one second, yields an analysis with neither.
    pub fn analyze(&self, audio: &AudioBuffer) -> Result<Analysis, EuphoriaError> {
        if audio.is_silent() || audio.duration_seconds() < MINIMUM_ANALYSIS_SECONDS {
            return Ok(Analysis::default());
        }
        let result = analyze_audio(
            audio.samples(),
            audio.sample_rate().get(),
            AnalysisConfig::default(),
        )
        .map_err(|error| EuphoriaError::Detection {
            detector: "stratum-dsp",
            reason: "analysis failed".to_string(),
            source: Some(Box::new(error)),
        })?;
        let key = match result.key {
            StratumKey::Major(tonic) => Key::new((tonic % 12) as u8, Mode::Major),
            StratumKey::Minor(tonic) => Key::new((tonic % 12) as u8, Mode::Minor),
        };
        Ok(Analysis {
            key: Some(key),
            tempo: Some(result.bpm).filter(|tempo| tempo.is_finite() && *tempo > 0.0),
        })
    }
}

impl KeyDetector for StratumDetector {
    fn name(&self) -> &'static str {
        "stratum-dsp"
    }

    fn detect_key(&self, audio: &AudioBuffer) -> Result<Option<Key>, EuphoriaError> {
        Ok(self.analyze(audio)?.key)
    }
}

impl TempoDetector for StratumDetector {
    fn name(&self) -> &'static str {
        "stratum-dsp"
    }

    fn detect_tempo(&self, audio: &AudioBuffer) -> Result<Option<f32>, EuphoriaError> {
        Ok(self.analyze(audio)?.tempo)
    }
}
