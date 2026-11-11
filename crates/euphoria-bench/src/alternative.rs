// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An energy model that the benchmark scores next to the shipped one.
//!
//! `bench/fit_energy_model.py` fitted it to the arousal ratings of the DEAM
//! dataset. It is not what Euphoria reports: `bench/results/energy.md`
//! explains why. It lives here so that the comparison in that file can be
//! run again.

use euphoria_core::{ENERGY_MODEL, EnergyFeatures, FeatureAnchor};

const SPECTRAL_FLUX: FeatureAnchor = FeatureAnchor {
    low: 0.0017,
    high: 0.0104,
    weight: 0.55,
};
const ONSET_RATE: FeatureAnchor = FeatureAnchor {
    low: 0.7,
    high: 7.2,
    weight: 0.26,
};
const INTEGRATED_LOUDNESS: FeatureAnchor = FeatureAnchor {
    low: -23.3,
    high: -6.5,
    weight: 0.19,
};
/// The weighted average at the bottom of rating 1.
const COMBINED_LOW: f32 = 0.41;
/// The weighted average at the top of rating 10.
const COMBINED_HIGH: f32 = 0.90;

/// What a model makes of one track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelOutput {
    /// The weighted average of the feature scores, before it is spread over
    /// the scale.
    pub average: f32,
    /// Position on the scale from 0.0 to 1.0.
    pub score: f32,
    /// The rating from 1 to 10.
    pub rating: u8,
}

/// Rates features with the fitted model.
pub fn rate_with_fitted_model(features: &EnergyFeatures) -> ModelOutput {
    let scored = [
        (SPECTRAL_FLUX, features.spectral_flux),
        (ONSET_RATE, features.onset_rate),
        (INTEGRATED_LOUDNESS, features.integrated_loudness_lufs),
    ];
    spread(weighted_average(&scored), COMBINED_LOW, COMBINED_HIGH)
}

/// The weighted average of the shipped model for the same inputs that
/// `rate_features` reads, before it is spread over the scale. The rating
/// itself comes from `euphoria-core`.
pub fn shipped_average(features: &EnergyFeatures, tempo: Option<f32>) -> f32 {
    let model = &ENERGY_MODEL;
    let mut scored = vec![
        (model.integrated_loudness, features.integrated_loudness_lufs),
        (model.loudness_range, features.loudness_range_lu),
        (model.onset_rate, features.onset_rate),
        (model.low_frequency_share, features.low_frequency_share),
        (model.spectral_flux, features.spectral_flux),
    ];
    if let Some(tempo) = tempo {
        scored.push((model.tempo, tempo));
    }
    weighted_average(&scored)
}

fn weighted_average(scored: &[(FeatureAnchor, f32)]) -> f32 {
    let weight_total: f32 = scored.iter().map(|(anchor, _)| anchor.weight).sum();
    let weighted_score: f32 = scored
        .iter()
        .map(|(anchor, value)| anchor.weight * anchor.score(*value))
        .sum();
    weighted_score / weight_total
}

fn spread(average: f32, low: f32, high: f32) -> ModelOutput {
    let score = ((average - low) / (high - low)).clamp(0.0, 1.0);
    ModelOutput {
        average,
        score,
        rating: ((score * 10.0) as u8 + 1).min(10),
    }
}

#[cfg(test)]
mod tests {
    use euphoria_core::rate_features;

    use super::*;

    fn features(flux: f32, onset_rate: f32, loudness: f32) -> EnergyFeatures {
        EnergyFeatures {
            integrated_loudness_lufs: loudness,
            loudness_range_lu: 6.0,
            onset_rate,
            low_frequency_share: 0.6,
            spectral_flux: flux,
        }
    }

    #[test]
    fn the_fitted_model_spans_the_scale_between_its_anchors() {
        let lowest = rate_with_fitted_model(&features(0.0017, 0.7, -23.3));
        let highest = rate_with_fitted_model(&features(0.0104, 7.2, -6.5));
        assert_eq!((lowest.average, lowest.score, lowest.rating), (0.0, 0.0, 1));
        assert!((highest.average - 1.0).abs() < 1.0e-6);
        assert_eq!((highest.score, highest.rating), (1.0, 10));
    }

    #[test]
    fn the_fitted_model_never_rates_lower_when_a_feature_rises() {
        let base = rate_with_fitted_model(&features(0.007, 4.0, -12.0));
        for raised in [
            features(0.008, 4.0, -12.0),
            features(0.007, 5.0, -12.0),
            features(0.007, 4.0, -9.0),
        ] {
            let output = rate_with_fitted_model(&raised);
            assert!(output.average > base.average, "{raised:?}");
            assert!(output.rating >= base.rating, "{raised:?}");
        }
    }

    #[test]
    fn the_fitted_model_ignores_what_it_does_not_weigh() {
        let base = features(0.007, 4.0, -12.0);
        let different = EnergyFeatures {
            loudness_range_lu: 14.0,
            low_frequency_share: 0.1,
            ..base
        };
        assert_eq!(
            rate_with_fitted_model(&base),
            rate_with_fitted_model(&different)
        );
    }

    #[test]
    fn the_shipped_average_spreads_to_the_score_of_the_shipped_rating() {
        for tempo in [None, Some(126.0)] {
            let track = features(0.008, 5.0, -10.0);
            let rating = rate_features(&track, tempo).unwrap();
            let average = shipped_average(&track, tempo);
            let score = spread(
                average,
                ENERGY_MODEL.combined_low,
                ENERGY_MODEL.combined_high,
            );
            assert!((score.score - rating.score).abs() < 1.0e-6, "{tempo:?}");
            assert_eq!(score.rating, rating.rating, "{tempo:?}");
        }
    }
}
