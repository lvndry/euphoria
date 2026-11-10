// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How energetic a track feels, as a rating from 1 to 10.
//!
//! The rating is a weighted average of a few measured features, each mapped
//! to a score through the anchors in [`ENERGY_MODEL`]. It is Euphoria's own
//! scale and is not comparable to the energy numbers of other tools.

mod loudness;
mod model;
mod onsets;
mod spectrum;

pub use model::{ENERGY_MODEL, ENERGY_MODEL_VERSION, EnergyModel, FeatureAnchor};

use std::num::NonZeroU32;

use crate::audio::AudioBuffer;
use crate::error::EuphoriaError;
use loudness::measure_loudness;
use spectrum::SpectralAnalyzer;

/// Length of each window of the energy curve.
pub const ENERGY_CURVE_WINDOW_SECONDS: f32 = 8.0;
/// Time from the start of one window of the energy curve to the next.
pub const ENERGY_CURVE_HOP_SECONDS: f32 = 4.0;

/// Audio shorter than this has no features and no rating.
const MINIMUM_ANALYSIS_SECONDS: f32 = 1.0;
const LOWEST_RATING: u8 = 1;
const HIGHEST_RATING: u8 = 10;

/// The measurements a rating is built from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnergyFeatures {
    /// Integrated loudness in LUFS, measured as EBU R128 describes.
    pub integrated_loudness_lufs: f32,
    /// Loudness range in LU, measured as EBU R128 describes: how far the
    /// loud and the quiet passages are apart.
    pub loudness_range_lu: f32,
    /// Percussive events per second.
    pub onset_rate: f32,
    /// Share of signal power below 150 Hz, from 0.0 to 1.0: the weight of
    /// the kick drum and the bass.
    pub low_frequency_share: f32,
    /// How much the spectrum changes from one moment to the next. It is
    /// near 0.0 for a held tone and grows as the sound gets busier.
    pub spectral_flux: f32,
}

/// An energy rating with everything needed to reproduce it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnergyRating {
    /// The rating, from 1 (calm) to 10 (intense).
    pub rating: u8,
    /// Position on the scale from 0.0 to 1.0 before it is cut into ten
    /// ratings. Useful for ordering tracks that share a rating.
    pub score: f32,
    /// The features the rating was computed from.
    pub features: EnergyFeatures,
    /// The tempo in beats per minute that took part in the rating, when the
    /// caller supplied a usable one.
    pub tempo: Option<f32>,
    /// The [`ENERGY_MODEL_VERSION`] that produced the rating.
    pub model_version: u32,
}

/// The rating of one window of an energy curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnergyCurvePoint {
    /// Where the window starts, in seconds from the start of the audio.
    pub start_seconds: f32,
    /// Length of the window in seconds.
    pub duration_seconds: f32,
    /// The rating of the window, or `None` when the window is silent.
    pub rating: Option<EnergyRating>,
}

/// Measures the features of the whole buffer. Returns `Ok(None)` when there
/// is nothing to measure: silence, or less than one second of audio.
pub fn energy_features(audio: &AudioBuffer) -> Result<Option<EnergyFeatures>, EuphoriaError> {
    let mut analyzer = SpectralAnalyzer::new(audio.sample_rate());
    measure_features(audio.samples(), audio.sample_rate(), &mut analyzer)
}

/// Rates features with [`ENERGY_MODEL`].
///
/// `tempo` is in beats per minute. When it is `None`, not finite or not
/// positive, the rating is built from the audio features alone. A rating
/// made with a tempo and one made without are both valid, but a library is
/// best rated one way throughout.
pub fn rate_features(features: &EnergyFeatures, tempo: Option<f32>) -> EnergyRating {
    let model = &ENERGY_MODEL;
    let tempo = tempo.filter(|tempo| tempo.is_finite() && *tempo > 0.0);
    let scored = [
        (model.integrated_loudness, features.integrated_loudness_lufs),
        (model.loudness_range, features.loudness_range_lu),
        (model.onset_rate, features.onset_rate),
        (model.low_frequency_share, features.low_frequency_share),
        (model.spectral_flux, features.spectral_flux),
    ];
    let mut weighted_score = 0.0;
    let mut weight_total = 0.0;
    for (anchor, value) in scored
        .into_iter()
        .chain(tempo.map(|tempo| (model.tempo, tempo)))
    {
        weighted_score += anchor.weight * anchor.score(value);
        weight_total += anchor.weight;
    }
    let combined = weighted_score / weight_total;
    let score = ((combined - model.combined_low) / (model.combined_high - model.combined_low))
        .clamp(0.0, 1.0);
    EnergyRating {
        rating: rating_of(score),
        score,
        features: *features,
        tempo,
        model_version: ENERGY_MODEL_VERSION,
    }
}

/// Rates the whole buffer. Returns `Ok(None)` for silence and for audio
/// shorter than one second, as the key and tempo detectors do.
pub fn rate_energy(
    audio: &AudioBuffer,
    tempo: Option<f32>,
) -> Result<Option<EnergyRating>, EuphoriaError> {
    Ok(energy_features(audio)?.map(|features| rate_features(&features, tempo)))
}

/// Rates the audio over a sliding window, for drawing energy over time.
///
/// Windows are [`ENERGY_CURVE_WINDOW_SECONDS`] long and start every
/// [`ENERGY_CURVE_HOP_SECONDS`]. Each point is exactly what [`rate_energy`]
/// returns for that stretch of audio. Audio shorter than one window gives a
/// single point that covers all of it, and the last partial hop of a longer
/// track is covered only by the windows before it.
pub fn energy_curve(
    audio: &AudioBuffer,
    tempo: Option<f32>,
) -> Result<Vec<EnergyCurvePoint>, EuphoriaError> {
    let sample_rate = audio.sample_rate();
    let samples = audio.samples();
    let rate = sample_rate.get() as f32;
    let window_length = ((ENERGY_CURVE_WINDOW_SECONDS * rate) as usize).min(samples.len());
    let hop_length = ((ENERGY_CURVE_HOP_SECONDS * rate) as usize).max(1);
    if window_length == 0 {
        return Ok(Vec::new());
    }

    let mut analyzer = SpectralAnalyzer::new(sample_rate);
    let mut points = Vec::with_capacity((samples.len() - window_length) / hop_length + 1);
    for start in (0..=samples.len() - window_length).step_by(hop_length) {
        let window = &samples[start..start + window_length];
        let features = measure_features(window, sample_rate, &mut analyzer)?;
        points.push(EnergyCurvePoint {
            start_seconds: start as f32 / rate,
            duration_seconds: window_length as f32 / rate,
            rating: features.map(|features| rate_features(&features, tempo)),
        });
    }
    Ok(points)
}

fn measure_features(
    samples: &[f32],
    sample_rate: NonZeroU32,
    analyzer: &mut SpectralAnalyzer,
) -> Result<Option<EnergyFeatures>, EuphoriaError> {
    if (samples.len() as f32) < MINIMUM_ANALYSIS_SECONDS * sample_rate.get() as f32 {
        return Ok(None);
    }
    let Some(loudness) = measure_loudness(samples, sample_rate)? else {
        return Ok(None);
    };
    let Some(spectral) = analyzer.analyze(samples) else {
        return Ok(None);
    };
    Ok(Some(EnergyFeatures {
        integrated_loudness_lufs: loudness.integrated_lufs,
        loudness_range_lu: loudness.range_lu,
        onset_rate: spectral.onset_rate,
        low_frequency_share: spectral.low_frequency_share,
        spectral_flux: spectral.spectral_flux,
    }))
}

/// Cuts the scale into ten equal steps.
fn rating_of(score: f32) -> u8 {
    let step_count = f32::from(HIGHEST_RATING - LOWEST_RATING + 1);
    let step = (score * step_count) as u8;
    (LOWEST_RATING + step).min(HIGHEST_RATING)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn features_at(position: f32) -> EnergyFeatures {
        let between = |anchor: FeatureAnchor| anchor.low + (anchor.high - anchor.low) * position;
        EnergyFeatures {
            integrated_loudness_lufs: between(ENERGY_MODEL.integrated_loudness),
            loudness_range_lu: between(ENERGY_MODEL.loudness_range),
            onset_rate: between(ENERGY_MODEL.onset_rate),
            low_frequency_share: between(ENERGY_MODEL.low_frequency_share),
            spectral_flux: between(ENERGY_MODEL.spectral_flux),
        }
    }

    #[test]
    fn rating_of_cuts_the_scale_into_ten_equal_steps() {
        assert_eq!(rating_of(0.0), 1);
        assert_eq!(rating_of(0.099), 1);
        assert_eq!(rating_of(0.101), 2);
        assert_eq!(rating_of(0.55), 6);
        assert_eq!(rating_of(0.95), 10);
        assert_eq!(rating_of(1.0), 10);
    }

    #[test]
    fn features_at_the_low_anchors_rate_one_and_at_the_high_anchors_rate_ten() {
        let lowest = rate_features(&features_at(0.0), None);
        let highest = rate_features(&features_at(1.0), None);
        assert_eq!(lowest.rating, 1);
        assert_eq!(lowest.score, 0.0);
        assert_eq!(highest.rating, 10);
        assert_eq!(highest.score, 1.0);
    }

    #[test]
    fn a_rating_carries_its_inputs_and_the_model_version() {
        let features = features_at(0.5);
        let rating = rate_features(&features, Some(126.0));
        assert_eq!(rating.features, features);
        assert_eq!(rating.tempo, Some(126.0));
        assert_eq!(rating.model_version, ENERGY_MODEL_VERSION);
    }

    #[test]
    fn the_score_never_falls_as_the_features_rise() {
        let mut previous = rate_features(&features_at(-0.5), None);
        for step in -4..=15 {
            let rating = rate_features(&features_at(step as f32 / 10.0), None);
            assert!(rating.score >= previous.score);
            assert!(rating.rating >= previous.rating);
            previous = rating;
        }
    }

    #[test]
    fn raising_one_feature_alone_never_lowers_the_score() {
        let base = features_at(0.5);
        let base_score = rate_features(&base, None).score;
        let raised = [
            EnergyFeatures {
                integrated_loudness_lufs: base.integrated_loudness_lufs + 3.0,
                ..base
            },
            EnergyFeatures {
                loudness_range_lu: base.loudness_range_lu - 3.0,
                ..base
            },
            EnergyFeatures {
                onset_rate: base.onset_rate + 2.0,
                ..base
            },
            EnergyFeatures {
                low_frequency_share: base.low_frequency_share + 0.2,
                ..base
            },
            EnergyFeatures {
                spectral_flux: base.spectral_flux * 1.5,
                ..base
            },
        ];
        for features in raised {
            assert!(
                rate_features(&features, None).score > base_score,
                "{features:?}"
            );
        }
    }

    #[test]
    fn a_faster_tempo_never_lowers_the_score() {
        let features = features_at(0.5);
        let mut previous_score = 0.0;
        for tempo in [60.0, 100.0, 120.0, 128.0, 140.0, 174.0, 300.0] {
            let score = rate_features(&features, Some(tempo)).score;
            assert!(score >= previous_score, "{tempo}");
            previous_score = score;
        }
        let slow = rate_features(&features, Some(ENERGY_MODEL.tempo.low)).score;
        let fast = rate_features(&features, Some(ENERGY_MODEL.tempo.high)).score;
        assert!(fast > slow);
    }

    #[test]
    fn an_unusable_tempo_is_ignored() {
        let features = features_at(0.5);
        let without_tempo = rate_features(&features, None);
        for tempo in [0.0, -120.0, f32::NAN, f32::INFINITY] {
            assert_eq!(rate_features(&features, Some(tempo)), without_tempo);
        }
    }
}
