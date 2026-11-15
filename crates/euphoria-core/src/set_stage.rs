// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where a track tends to sit in a set: warm-up, build or peak.
//!
//! The stage comes from the tempo, the integrated loudness and the loudness
//! range of a track, each placed as a percentile among the tracks of the
//! library, then combined with fixed weights. It follows how sets usually
//! rise in tempo and loudness. It is not the energy rating, which says how
//! intense a track sounds, and it has no closing stage: the tracks closing a
//! set sit just below the peak on these features.

use serde::{Deserialize, Serialize};

/// The share of the score carried by the tempo percentile.
pub const TEMPO_WEIGHT: f32 = 0.7;

/// The share of the score carried by the loudness percentile.
pub const LOUDNESS_WEIGHT: f32 = 0.2;

/// The share of the score carried by how narrow the loudness range is.
pub const LOUDNESS_RANGE_WEIGHT: f32 = 0.1;

/// Scores below this are warm-up.
pub const WARM_UP_CUT: f32 = 0.145;

/// Scores from [`WARM_UP_CUT`] up to this are build, and higher ones peak.
pub const PEAK_CUT: f32 = 0.46;

/// The fewest tracks with a tempo a library needs before a track has a stage:
/// percentiles among fewer tracks mean nothing.
pub const MINIMUM_LIBRARY_TRACKS: usize = 10;

/// A stage of a set, in the order a set runs through them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SetStage {
    /// Tracks that open a set: slower and quieter than the library.
    WarmUp,
    /// Tracks in between.
    Build,
    /// The fastest and loudest tracks of the library.
    Peak,
}

impl SetStage {
    /// The stages from the start of a set to its peak.
    pub const ALL: [SetStage; 3] = [SetStage::WarmUp, SetStage::Build, SetStage::Peak];

    /// The name shown to the user.
    pub fn label(self) -> &'static str {
        match self {
            SetStage::WarmUp => "Warm-up",
            SetStage::Build => "Build",
            SetStage::Peak => "Peak",
        }
    }

    /// The name written in JSON and typed on the command line.
    pub fn slug(self) -> &'static str {
        match self {
            SetStage::WarmUp => "warm-up",
            SetStage::Build => "build",
            SetStage::Peak => "peak",
        }
    }

    /// The stage written as `slug` or `label`, in any case, with `warmup` and
    /// `warm up` accepted for warm-up.
    pub fn from_text(text: &str) -> Option<SetStage> {
        match text.trim().to_lowercase().replace([' ', '_'], "-").as_str() {
            "warm-up" | "warmup" => Some(SetStage::WarmUp),
            "build" => Some(SetStage::Build),
            "peak" => Some(SetStage::Peak),
            _ => None,
        }
    }

    /// The stage a score from [`stage_score`] falls in.
    pub fn from_score(score: f32) -> SetStage {
        if score < WARM_UP_CUT {
            SetStage::WarmUp
        } else if score < PEAK_CUT {
            SetStage::Build
        } else {
            SetStage::Peak
        }
    }

    /// Where the stage sits in a set, from 0.0 at the start to 1.0 at the
    /// peak, for ordering tracks.
    pub fn position(self) -> f32 {
        match self {
            SetStage::WarmUp => 0.0,
            SetStage::Build => 0.5,
            SetStage::Peak => 1.0,
        }
    }
}

/// What the stage of a track is read from. A track without a tempo has no
/// stage; a missing loudness or loudness range leaves that part out of the
/// score.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StageFeatures {
    /// The tempo in beats per minute, the corrected one when the user set it.
    pub tempo: Option<f32>,
    /// Integrated loudness in LUFS.
    pub integrated_loudness_lufs: Option<f32>,
    /// Loudness range in LU.
    pub loudness_range_lu: Option<f32>,
}

/// How the tracks of a library spread over each feature, to place a track
/// among them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LibraryDistribution {
    tempos: Vec<f32>,
    loudnesses: Vec<f32>,
    loudness_ranges: Vec<f32>,
}

impl LibraryDistribution {
    /// The distribution of the tracks `features` describes. A value that is
    /// not a number is left out.
    pub fn from_features(features: impl IntoIterator<Item = StageFeatures>) -> Self {
        let mut distribution = Self::default();
        for track in features {
            let keep = |value: Option<f32>, values: &mut Vec<f32>| {
                if let Some(value) = value.filter(|value| value.is_finite()) {
                    values.push(value);
                }
            };
            keep(track.tempo, &mut distribution.tempos);
            keep(track.integrated_loudness_lufs, &mut distribution.loudnesses);
            keep(track.loudness_range_lu, &mut distribution.loudness_ranges);
        }
        for values in [
            &mut distribution.tempos,
            &mut distribution.loudnesses,
            &mut distribution.loudness_ranges,
        ] {
            values.sort_by(f32::total_cmp);
        }
        distribution
    }

    /// How many tracks have a tempo.
    pub fn tempo_count(&self) -> usize {
        self.tempos.len()
    }

    /// Whether the library has enough tracks for percentiles to mean
    /// something. See [`MINIMUM_LIBRARY_TRACKS`].
    pub fn is_usable(&self) -> bool {
        self.tempos.len() >= MINIMUM_LIBRARY_TRACKS
    }
}

/// The share of `sorted` below `value`, counting values equal to it by half,
/// from 0.0 to 1.0. `None` when there is nothing to compare with.
fn percentile(sorted: &[f32], value: f32) -> Option<f32> {
    if sorted.is_empty() {
        return None;
    }
    let below = sorted.partition_point(|other| *other < value);
    let at_or_below = sorted.partition_point(|other| *other <= value);
    Some((below + at_or_below) as f32 / 2.0 / sorted.len() as f32)
}

/// The tempo, loudness and loudness range percentiles of a track in a
/// library, each from 0.0 to 1.0, where a narrow loudness range counts as a
/// high percentile. A part the track or the library lacks is `None`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StagePercentiles {
    /// Among the tempos of the library: higher is faster.
    pub tempo: Option<f32>,
    /// Among the loudnesses: higher is louder.
    pub loudness: Option<f32>,
    /// Among the loudness ranges, turned around: higher is narrower.
    pub narrowness: Option<f32>,
}

/// Where the features of a track fall in `library`.
pub fn stage_percentiles(
    features: &StageFeatures,
    library: &LibraryDistribution,
) -> StagePercentiles {
    let among = |value: Option<f32>, sorted: &[f32]| {
        value
            .filter(|value| value.is_finite())
            .and_then(|value| percentile(sorted, value))
    };
    StagePercentiles {
        tempo: among(features.tempo, &library.tempos),
        loudness: among(features.integrated_loudness_lufs, &library.loudnesses),
        narrowness: among(features.loudness_range_lu, &library.loudness_ranges)
            .map(|percentile| 1.0 - percentile),
    }
}

/// The stage score of a track: 0.0 for a track slower, quieter and wider in
/// range than the rest of the library and 1.0 for the opposite. It is the
/// weighted sum of the percentiles, with the weights of the parts that are
/// missing shared among the others. `None` when the track has no tempo or the
/// library is too small, see [`MINIMUM_LIBRARY_TRACKS`].
pub fn stage_score(features: &StageFeatures, library: &LibraryDistribution) -> Option<f32> {
    if !library.is_usable() {
        return None;
    }
    let percentiles = stage_percentiles(features, library);
    let tempo = percentiles.tempo?;
    let parts = [
        (Some(tempo), TEMPO_WEIGHT),
        (percentiles.loudness, LOUDNESS_WEIGHT),
        (percentiles.narrowness, LOUDNESS_RANGE_WEIGHT),
    ];
    let (sum, weight) = parts
        .iter()
        .filter_map(|(percentile, weight)| {
            percentile.map(|percentile| (percentile * weight, *weight))
        })
        .fold((0.0, 0.0), |(sum, weights), (part, weight)| {
            (sum + part, weights + weight)
        });
    Some(sum / weight)
}

/// The stage of a track in `library`, or `None` when it has no tempo or the
/// library is too small.
pub fn stage_of(features: &StageFeatures, library: &LibraryDistribution) -> Option<SetStage> {
    stage_score(features, library).map(SetStage::from_score)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(tempo: f32, loudness: f32, range: f32) -> StageFeatures {
        StageFeatures {
            tempo: Some(tempo),
            integrated_loudness_lufs: Some(loudness),
            loudness_range_lu: Some(range),
        }
    }

    fn library_of(tempos: impl IntoIterator<Item = f32>) -> LibraryDistribution {
        LibraryDistribution::from_features(tempos.into_iter().map(|tempo| {
            let rise = (tempo - 100.0) / 40.0;
            track(tempo, -16.0 + rise * 6.0, 9.0 - rise * 4.0)
        }))
    }

    fn evenly_spread() -> LibraryDistribution {
        library_of((0..40).map(|step| 100.0 + step as f32))
    }

    #[test]
    fn weights_add_up_to_one() {
        let total = TEMPO_WEIGHT + LOUDNESS_WEIGHT + LOUDNESS_RANGE_WEIGHT;
        assert!((total - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_percentile_counts_equal_values_by_half() {
        let sorted = [1.0, 2.0, 2.0, 3.0];
        assert_eq!(percentile(&sorted, 0.0), Some(0.0));
        assert_eq!(percentile(&sorted, 1.0), Some(0.125));
        assert_eq!(percentile(&sorted, 2.0), Some(0.5));
        assert_eq!(percentile(&sorted, 3.0), Some(0.875));
        assert_eq!(percentile(&sorted, 9.0), Some(1.0));
        assert_eq!(percentile(&[], 1.0), None);
    }

    #[test]
    fn a_narrow_loudness_range_is_a_high_percentile() {
        let library = evenly_spread();
        let narrow = stage_percentiles(&track(120.0, -12.0, 1.0), &library);
        let wide = stage_percentiles(&track(120.0, -12.0, 20.0), &library);
        assert!(narrow.narrowness.unwrap() > 0.9);
        assert!(wide.narrowness.unwrap() < 0.1);
    }

    #[test]
    fn the_score_is_the_weighted_sum_of_the_percentiles() {
        let library = evenly_spread();
        let features = track(130.0, -12.0, 7.0);
        let percentiles = stage_percentiles(&features, &library);
        let expected = percentiles.tempo.unwrap() * TEMPO_WEIGHT
            + percentiles.loudness.unwrap() * LOUDNESS_WEIGHT
            + percentiles.narrowness.unwrap() * LOUDNESS_RANGE_WEIGHT;
        let score = stage_score(&features, &library).unwrap();
        assert!(
            (score - expected).abs() < 1e-6,
            "{score} against {expected}"
        );
    }

    #[test]
    fn a_missing_loudness_shares_its_weight_among_the_other_parts() {
        let library = evenly_spread();
        let without_range = StageFeatures {
            loudness_range_lu: None,
            ..track(130.0, -12.0, 7.0)
        };
        let percentiles = stage_percentiles(&without_range, &library);
        let expected = (percentiles.tempo.unwrap() * TEMPO_WEIGHT
            + percentiles.loudness.unwrap() * LOUDNESS_WEIGHT)
            / (TEMPO_WEIGHT + LOUDNESS_WEIGHT);
        let score = stage_score(&without_range, &library).unwrap();
        assert!((score - expected).abs() < 1e-6);
    }

    #[test]
    fn cut_points_split_the_scores_into_three_stages() {
        assert_eq!(SetStage::from_score(0.0), SetStage::WarmUp);
        assert_eq!(SetStage::from_score(WARM_UP_CUT - 0.001), SetStage::WarmUp);
        assert_eq!(SetStage::from_score(WARM_UP_CUT), SetStage::Build);
        assert_eq!(SetStage::from_score(PEAK_CUT - 0.001), SetStage::Build);
        assert_eq!(SetStage::from_score(PEAK_CUT), SetStage::Peak);
        assert_eq!(SetStage::from_score(1.0), SetStage::Peak);
    }

    #[test]
    fn slow_quiet_tracks_warm_up_and_fast_loud_ones_peak() {
        let library = evenly_spread();
        assert_eq!(
            stage_of(&track(100.0, -16.0, 9.0), &library),
            Some(SetStage::WarmUp)
        );
        assert_eq!(
            stage_of(&track(139.0, -10.0, 5.0), &library),
            Some(SetStage::Peak)
        );
        assert_eq!(
            stage_of(&track(110.0, -14.5, 8.0), &library),
            Some(SetStage::Build)
        );
    }

    #[test]
    fn a_track_without_a_tempo_has_no_stage() {
        let library = evenly_spread();
        let silent = StageFeatures {
            tempo: None,
            ..track(130.0, -10.0, 5.0)
        };
        assert_eq!(stage_score(&silent, &library), None);
        assert_eq!(stage_of(&silent, &library), None);
    }

    #[test]
    fn a_library_too_small_gives_no_stage() {
        let small = library_of((0..MINIMUM_LIBRARY_TRACKS - 1).map(|step| 100.0 + step as f32));
        assert!(!small.is_usable());
        assert_eq!(stage_of(&track(120.0, -12.0, 7.0), &small), None);
    }

    #[test]
    fn a_corrected_tempo_changes_the_stage() {
        let library = evenly_spread();
        let detected = track(70.0, -16.0, 9.0);
        let corrected = StageFeatures {
            tempo: Some(140.0),
            ..detected
        };
        assert_eq!(stage_of(&detected, &library), Some(SetStage::WarmUp));
        assert_eq!(stage_of(&corrected, &library), Some(SetStage::Peak));
    }

    #[test]
    fn the_same_track_has_another_stage_in_another_library() {
        let techno_like = track(137.0, -10.0, 5.0);
        let slow_library = evenly_spread();
        let fast_library = LibraryDistribution::from_features((0..40).map(|step| {
            track(
                135.0 + step as f32,
                -9.0 + step as f32 * 0.05,
                6.0 - step as f32 * 0.05,
            )
        }));
        assert_eq!(stage_of(&techno_like, &slow_library), Some(SetStage::Peak));
        assert_eq!(
            stage_of(&techno_like, &fast_library),
            Some(SetStage::WarmUp)
        );
    }

    #[test]
    fn values_that_are_not_numbers_are_left_out() {
        let library = LibraryDistribution::from_features([
            track(f32::NAN, -12.0, 7.0),
            track(120.0, f32::INFINITY, 7.0),
        ]);
        assert_eq!(library.tempo_count(), 1);
        let nan_tempo = StageFeatures {
            tempo: Some(f32::NAN),
            ..track(0.0, -12.0, 7.0)
        };
        assert_eq!(stage_percentiles(&nan_tempo, &library).tempo, None);
    }

    #[test]
    fn stages_are_read_from_text() {
        assert_eq!(SetStage::from_text("Warm-up"), Some(SetStage::WarmUp));
        assert_eq!(SetStage::from_text("warmup"), Some(SetStage::WarmUp));
        assert_eq!(SetStage::from_text("warm up"), Some(SetStage::WarmUp));
        assert_eq!(SetStage::from_text("PEAK"), Some(SetStage::Peak));
        assert_eq!(SetStage::from_text("closing"), None);
        for stage in SetStage::ALL {
            assert_eq!(SetStage::from_text(stage.slug()), Some(stage));
            assert_eq!(SetStage::from_text(stage.label()), Some(stage));
        }
    }
}
