// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How energetic a track feels, as a rating from 1 to 10.
//!
//! The rating is a weighted average of a few measured features, each mapped
//! to a score through the anchors in [`ENERGY_MODEL`]. It is Euphoria's own
//! scale and is not comparable to the energy numbers of other tools.
//!
//! The rating reads the audio only. Tempo takes no part in it: on the
//! reference data the model is fitted to, tempo told nothing about energy
//! that the audio features had not already told.

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

/// The lowest sample rate the energy functions accept, the same as the
/// decoder's.
pub const ENERGY_MINIMUM_SAMPLE_RATE: u32 = crate::audio::MINIMUM_SAMPLE_RATE;
/// The highest sample rate the energy functions accept, the same as the
/// decoder's. A buffer built by a caller with a rate outside the range is
/// refused with [`EuphoriaError::UnsupportedSampleRate`] before anything is
/// allocated.
pub const ENERGY_MAXIMUM_SAMPLE_RATE: u32 = crate::audio::MAXIMUM_SAMPLE_RATE;

/// Audio shorter than this has no features and no rating.
const MINIMUM_ANALYSIS_SECONDS: f32 = 1.0;
const LOWEST_RATING: u8 = 1;
const HIGHEST_RATING: u8 = 10;

/// The measurements a rating is built from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnergyFeatures {
    /// How much the spectrum changes from one moment to the next, relative
    /// to the level of the audio. It is near 0.0 for a held tone and grows
    /// as the sound gets busier and brighter.
    pub spectral_flux: f32,
    /// Percussive events per second.
    pub onset_rate: f32,
    /// Integrated loudness in LUFS, measured as EBU R128 describes.
    pub integrated_loudness_lufs: f32,
}

impl EnergyFeatures {
    /// Every feature with its name, in the order of the fields.
    pub fn named(&self) -> [(&'static str, f32); 3] {
        [
            ("spectral flux", self.spectral_flux),
            ("onset rate", self.onset_rate),
            ("integrated loudness", self.integrated_loudness_lufs),
        ]
    }
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
///
/// Fails with [`EuphoriaError::UnsupportedSampleRate`] when the sample rate
/// is outside [`ENERGY_MINIMUM_SAMPLE_RATE`] to [`ENERGY_MAXIMUM_SAMPLE_RATE`],
/// whatever the length of the audio.
pub fn energy_features(audio: &AudioBuffer) -> Result<Option<EnergyFeatures>, EuphoriaError> {
    check_sample_rate(audio.sample_rate())?;
    if is_too_short(audio.samples(), audio.sample_rate()) {
        return Ok(None);
    }
    let mut analyzer = SpectralAnalyzer::new(audio.sample_rate());
    measure_features(audio.samples(), audio.sample_rate(), &mut analyzer)
}

/// Rates features with [`ENERGY_MODEL`].
///
/// Fails with [`EuphoriaError::InvalidEnergyInput`] when a feature is not a
/// finite number, so a rating that is returned always has a finite score.
pub fn rate_features(features: &EnergyFeatures) -> Result<EnergyRating, EuphoriaError> {
    let model = &ENERGY_MODEL;
    for (name, value) in features.named() {
        if !value.is_finite() {
            return Err(EuphoriaError::InvalidEnergyInput { name, value });
        }
    }
    let scored = [
        (model.spectral_flux, features.spectral_flux),
        (model.onset_rate, features.onset_rate),
        (model.integrated_loudness, features.integrated_loudness_lufs),
    ];
    let mut weighted_score = 0.0;
    let mut weight_total = 0.0;
    for (anchor, value) in scored {
        weighted_score += anchor.weight * anchor.score(value);
        weight_total += anchor.weight;
    }
    let combined = weighted_score / weight_total;
    let score = ((combined - model.combined_low) / (model.combined_high - model.combined_low))
        .clamp(0.0, 1.0);
    Ok(EnergyRating {
        rating: rating_of(score),
        score,
        features: *features,
        model_version: ENERGY_MODEL_VERSION,
    })
}

/// Rates the whole buffer. Returns `Ok(None)` for silence and for audio
/// shorter than one second, as the key and tempo detectors do.
///
/// Fails as [`energy_features`] does for an unsupported sample rate.
pub fn rate_energy(audio: &AudioBuffer) -> Result<Option<EnergyRating>, EuphoriaError> {
    energy_features(audio)?
        .map(|features| rate_features(&features))
        .transpose()
}

/// Rates the audio over a sliding window, for drawing energy over time.
///
/// Windows are [`ENERGY_CURVE_WINDOW_SECONDS`] long and start every
/// [`ENERGY_CURVE_HOP_SECONDS`]. Each point is exactly what [`rate_energy`]
/// returns for that stretch of audio. Audio shorter than one window gives a
/// single point that covers all of it, and the last partial hop of a longer
/// track is covered only by the windows before it.
///
/// Fails as [`rate_energy`] does for an unsupported sample rate.
pub fn energy_curve(audio: &AudioBuffer) -> Result<Vec<EnergyCurvePoint>, EuphoriaError> {
    let sample_rate = audio.sample_rate();
    check_sample_rate(sample_rate)?;
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
            rating: features
                .map(|features| rate_features(&features))
                .transpose()?,
        });
    }
    Ok(points)
}

fn check_sample_rate(sample_rate: NonZeroU32) -> Result<(), EuphoriaError> {
    let supported = ENERGY_MINIMUM_SAMPLE_RATE..=ENERGY_MAXIMUM_SAMPLE_RATE;
    if supported.contains(&sample_rate.get()) {
        return Ok(());
    }
    Err(EuphoriaError::UnsupportedSampleRate {
        sample_rate: sample_rate.get(),
        minimum: ENERGY_MINIMUM_SAMPLE_RATE,
        maximum: ENERGY_MAXIMUM_SAMPLE_RATE,
    })
}

fn is_too_short(samples: &[f32], sample_rate: NonZeroU32) -> bool {
    (samples.len() as f64) < f64::from(MINIMUM_ANALYSIS_SECONDS) * f64::from(sample_rate.get())
}

fn measure_features(
    samples: &[f32],
    sample_rate: NonZeroU32,
    analyzer: &mut SpectralAnalyzer,
) -> Result<Option<EnergyFeatures>, EuphoriaError> {
    if is_too_short(samples, sample_rate) {
        return Ok(None);
    }
    let Some(integrated_loudness_lufs) = measure_loudness(samples, sample_rate)? else {
        return Ok(None);
    };
    let Some(spectral) = analyzer.analyze(samples) else {
        return Ok(None);
    };
    Ok(Some(EnergyFeatures {
        spectral_flux: spectral.spectral_flux,
        onset_rate: spectral.onset_rate,
        integrated_loudness_lufs,
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

    /// Features that all sit at the same position between their anchors,
    /// which is then also the weighted average of their scores.
    fn features_at(position: f32) -> EnergyFeatures {
        let between = |anchor: FeatureAnchor| anchor.low + (anchor.high - anchor.low) * position;
        EnergyFeatures {
            spectral_flux: between(ENERGY_MODEL.spectral_flux),
            onset_rate: between(ENERGY_MODEL.onset_rate),
            integrated_loudness_lufs: between(ENERGY_MODEL.integrated_loudness),
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
    fn the_range_of_the_model_spans_rating_one_to_rating_ten() {
        let lowest = rate_features(&features_at(ENERGY_MODEL.combined_low)).unwrap();
        let highest = rate_features(&features_at(ENERGY_MODEL.combined_high)).unwrap();
        assert_eq!(lowest.rating, 1);
        assert!(lowest.score < 1.0e-3, "{lowest:?}");
        assert_eq!(highest.rating, 10);
        assert!(highest.score > 1.0 - 1.0e-3, "{highest:?}");

        let middle = (ENERGY_MODEL.combined_low + ENERGY_MODEL.combined_high) / 2.0;
        let halfway = rate_features(&features_at(middle)).unwrap();
        assert!((halfway.score - 0.5).abs() < 1.0e-3, "{halfway:?}");
    }

    #[test]
    fn features_beyond_the_anchors_rate_one_and_ten() {
        assert_eq!(rate_features(&features_at(-1.0)).unwrap().rating, 1);
        assert_eq!(rate_features(&features_at(0.0)).unwrap().rating, 1);
        assert_eq!(rate_features(&features_at(1.0)).unwrap().rating, 10);
        assert_eq!(rate_features(&features_at(2.0)).unwrap().rating, 10);
    }

    #[test]
    fn a_rating_carries_its_features_and_the_model_version() {
        let features = features_at(0.5);
        let rating = rate_features(&features).unwrap();
        assert_eq!(rating.features, features);
        assert_eq!(rating.model_version, ENERGY_MODEL_VERSION);
    }

    #[test]
    fn the_score_never_falls_as_the_features_rise() {
        let mut previous = rate_features(&features_at(-0.5)).unwrap();
        for step in -4..=15 {
            let rating = rate_features(&features_at(step as f32 / 10.0)).unwrap();
            assert!(rating.score >= previous.score);
            assert!(rating.rating >= previous.rating);
            previous = rating;
        }
    }

    #[test]
    fn raising_one_feature_alone_raises_the_score() {
        let base = features_at(0.6);
        let base_score = rate_features(&base).unwrap().score;
        let raised = [
            EnergyFeatures {
                spectral_flux: base.spectral_flux * 1.2,
                ..base
            },
            EnergyFeatures {
                onset_rate: base.onset_rate + 1.0,
                ..base
            },
            EnergyFeatures {
                integrated_loudness_lufs: base.integrated_loudness_lufs + 3.0,
                ..base
            },
        ];
        for features in raised {
            assert!(
                rate_features(&features).unwrap().score > base_score,
                "{features:?}"
            );
        }
    }

    #[test]
    fn a_feature_that_is_not_finite_is_an_error_naming_it() {
        let base = features_at(0.5);
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let cases = [
                (
                    "spectral flux",
                    EnergyFeatures {
                        spectral_flux: value,
                        ..base
                    },
                ),
                (
                    "onset rate",
                    EnergyFeatures {
                        onset_rate: value,
                        ..base
                    },
                ),
                (
                    "integrated loudness",
                    EnergyFeatures {
                        integrated_loudness_lufs: value,
                        ..base
                    },
                ),
            ];
            for (expected_name, features) in cases {
                let outcome = rate_features(&features);
                assert!(
                    matches!(
                        outcome,
                        Err(EuphoriaError::InvalidEnergyInput { name, .. })
                            if name == expected_name
                    ),
                    "{expected_name} = {value} gave {outcome:?}"
                );
            }
        }
    }

    #[test]
    fn every_rating_of_finite_features_has_a_finite_score() {
        for value in [f32::MIN, -1.0e30, -1.0, 0.0, 1.0, 1.0e30, f32::MAX] {
            let features = EnergyFeatures {
                spectral_flux: value,
                onset_rate: value,
                integrated_loudness_lufs: value,
            };
            let rating = rate_features(&features).unwrap();
            assert!(rating.score.is_finite(), "{value} gave {rating:?}");
            assert!((1..=10).contains(&rating.rating), "{value} gave {rating:?}");
        }
    }

    #[test]
    fn features_are_named_in_field_order() {
        let names = features_at(0.5).named().map(|(name, _)| name);
        assert_eq!(
            names,
            ["spectral flux", "onset rate", "integrated loudness"]
        );
    }
}
