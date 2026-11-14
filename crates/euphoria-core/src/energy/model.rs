// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/// Version of [`ENERGY_MODEL`]. It changes whenever an anchor, a weight or
/// the way a feature is measured changes, so ratings made with different
/// versions are never compared by mistake.
pub const ENERGY_MODEL_VERSION: u32 = 2;

/// How one feature turns into a score from 0.0 to 1.0, and how much that
/// score counts in the rating.
///
/// The score is 0.0 at `low`, 1.0 at `high`, linear in between and clamped
/// outside. `low` is greater than `high` for a feature where a smaller
/// value means more energy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeatureAnchor {
    /// The feature value that scores 0.0.
    pub low: f32,
    /// The feature value that scores 1.0.
    pub high: f32,
    /// Share of the rating carried by this feature. Weights are relative:
    /// they are divided by the sum of the weights of the features in use. A
    /// weight of 0.0 keeps a feature out of the rating.
    pub weight: f32,
}

impl FeatureAnchor {
    /// The score of `value`, from 0.0 to 1.0.
    pub fn score(&self, value: f32) -> f32 {
        ((value - self.low) / (self.high - self.low)).clamp(0.0, 1.0)
    }
}

/// Every number that decides a rating: one anchor per feature and the range
/// of the weighted average score that is spread over the ten ratings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnergyModel {
    /// Equal to [`ENERGY_MODEL_VERSION`].
    pub version: u32,
    /// Integrated loudness in LUFS. Louder scores higher.
    pub integrated_loudness: FeatureAnchor,
    /// Loudness range in LU. A narrower range scores higher.
    pub loudness_range: FeatureAnchor,
    /// Onsets per second. More scores higher.
    pub onset_rate: FeatureAnchor,
    /// Share of signal power below 150 Hz. More scores higher.
    pub low_frequency_share: FeatureAnchor,
    /// Spectral flux. More scores higher.
    pub spectral_flux: FeatureAnchor,
    /// Tempo in beats per minute, used only when the caller supplies it.
    /// Faster scores higher.
    pub tempo: FeatureAnchor,
    /// The weighted average score at the bottom of rating 1.
    pub combined_low: f32,
    /// The weighted average score at the top of rating 10.
    pub combined_high: f32,
}

/// The model behind every rating this version of the library produces.
///
/// The anchors of the audio features sit near the values reached by the
/// lowest and the highest 3% of the 604 dance music previews of the
/// GiantSteps Key dataset, so ratings of dance music spread over the whole
/// scale. The weights were fitted to the arousal that listeners gave the
/// songs of two public datasets, and spectral flux carries most of them.
/// Low frequency share and tempo carry none: they did not follow arousal in
/// either dataset. They stay in the model, with their anchors, so that a
/// weight can be given back.
pub const ENERGY_MODEL: EnergyModel = EnergyModel {
    version: ENERGY_MODEL_VERSION,
    integrated_loudness: FeatureAnchor {
        low: -14.0,
        high: -6.5,
        weight: 0.2,
    },
    loudness_range: FeatureAnchor {
        low: 12.0,
        high: 1.5,
        weight: 0.05,
    },
    onset_rate: FeatureAnchor {
        low: 2.5,
        high: 7.0,
        weight: 0.1,
    },
    low_frequency_share: FeatureAnchor {
        low: 0.35,
        high: 0.9,
        weight: 0.0,
    },
    spectral_flux: FeatureAnchor {
        low: 0.0045,
        high: 0.0105,
        weight: 0.65,
    },
    tempo: FeatureAnchor {
        low: 100.0,
        high: 140.0,
        weight: 0.0,
    },
    combined_low: 0.1,
    combined_high: 0.9,
};

const fn is_usable(anchor: FeatureAnchor) -> bool {
    anchor.weight >= 0.0 && anchor.low != anchor.high
}

const _: () = {
    assert!(is_usable(ENERGY_MODEL.integrated_loudness));
    assert!(is_usable(ENERGY_MODEL.loudness_range));
    assert!(is_usable(ENERGY_MODEL.onset_rate));
    assert!(is_usable(ENERGY_MODEL.low_frequency_share));
    assert!(is_usable(ENERGY_MODEL.spectral_flux));
    assert!(is_usable(ENERGY_MODEL.tempo));
    assert!(ENERGY_MODEL.integrated_loudness.weight + ENERGY_MODEL.spectral_flux.weight > 0.0);
    assert!(ENERGY_MODEL.combined_low < ENERGY_MODEL.combined_high);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_is_linear_between_the_anchors_and_clamped_outside() {
        let anchor = FeatureAnchor {
            low: 10.0,
            high: 20.0,
            weight: 1.0,
        };
        assert_eq!(anchor.score(5.0), 0.0);
        assert_eq!(anchor.score(10.0), 0.0);
        assert_eq!(anchor.score(15.0), 0.5);
        assert_eq!(anchor.score(20.0), 1.0);
        assert_eq!(anchor.score(99.0), 1.0);
    }

    #[test]
    fn score_falls_as_the_value_rises_when_the_anchors_are_reversed() {
        let anchor = FeatureAnchor {
            low: 12.0,
            high: 2.0,
            weight: 1.0,
        };
        assert_eq!(anchor.score(12.0), 0.0);
        assert_eq!(anchor.score(7.0), 0.5);
        assert_eq!(anchor.score(1.0), 1.0);
    }
}
