// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A key or a tempo the user set by hand, after checking a track by ear.
//!
//! A correction is kept apart from the analysis: the cache stores it in a
//! table of its own, under the content of the file, with no analyzer
//! version, so analyzing again with a newer Euphoria never loses it. It wins
//! over the analysis wherever a key or a tempo is shown or written.

use crate::analysis::TrackAnalysis;
use crate::error::EuphoriaError;
use crate::key::Key;

/// The values the user set for one track. A field that is `None` keeps the
/// detected value.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Correction {
    /// The key the user set.
    pub key: Option<Key>,
    /// The tempo the user set, in beats per minute. It is shown as set,
    /// never moved into a tempo range.
    pub bpm: Option<f32>,
}

impl Correction {
    /// A correction of the key only.
    pub fn of_key(key: Key) -> Self {
        Self {
            key: Some(key),
            bpm: None,
        }
    }

    /// A correction of the tempo only.
    ///
    /// # Errors
    ///
    /// [`EuphoriaError::InvalidCorrection`] when `bpm` is not a positive
    /// number no faster than 999.
    pub fn of_bpm(bpm: f32) -> Result<Self, EuphoriaError> {
        Self::default().with_bpm(Some(bpm))
    }

    /// The same correction with the tempo set to `bpm`, or back to the
    /// detected tempo with `None`.
    ///
    /// # Errors
    ///
    /// [`EuphoriaError::InvalidCorrection`] when `bpm` is not a positive
    /// number no faster than 999.
    pub fn with_bpm(self, bpm: Option<f32>) -> Result<Self, EuphoriaError> {
        if let Some(bpm) = bpm
            && !(bpm.is_finite() && bpm > 0.0 && bpm <= 999.0)
        {
            return Err(EuphoriaError::InvalidCorrection {
                reason: "a tempo must be a number above 0 and at most 999",
            });
        }
        Ok(Self { bpm, ..self })
    }

    /// The same correction with the key set to `key`, or back to the
    /// detected key with `None`.
    pub fn with_key(self, key: Option<Key>) -> Self {
        Self { key, ..self }
    }

    /// True when nothing is corrected.
    pub fn is_empty(&self) -> bool {
        self.key.is_none() && self.bpm.is_none()
    }

    /// `analysis` with the corrected values in place of the detected ones.
    ///
    /// A corrected key has no runner up and no score margin, and a corrected
    /// tempo no estimate and no margin, so neither is rated for confidence:
    /// the user decided it. A corrected tempo also drops the beat grid,
    /// which was fitted to the detected tempo, and the cues, which sit on its
    /// bars. Place the analysis in a tempo range before
    /// applying a correction, since a corrected tempo is not moved into a
    /// range.
    pub fn apply(&self, mut analysis: TrackAnalysis) -> TrackAnalysis {
        if let Some(key) = self.key {
            analysis.key = Some(key);
            analysis.key_runner_up = None;
            analysis.key_score_margin = None;
        }
        if let Some(bpm) = self.bpm {
            analysis.bpm = Some(bpm);
            analysis.tempo_estimate = None;
            analysis.tempo_score_margin = None;
            // A grid fitted to the detected tempo would put its beats where
            // the corrected tempo has none; a DJ program builds its own.
            analysis.beat_grid = None;
            analysis.cues.clear();
        }
        analysis
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::confidence::Confidence;
    use crate::key::Mode;

    fn doubtful() -> TrackAnalysis {
        TrackAnalysis {
            duration_seconds: 120.0,
            key: Some(Key::new(9, Mode::Minor)),
            bpm: Some(174.0),
            energy: Some(6),
            tuning_cents: None,
            key_timeline: Vec::new(),
            quality: None,
            key_runner_up: Some(Key::new(4, Mode::Minor)),
            key_score_margin: Some(0.001),
            key_source: None,
            tempo_estimate: Some(87.0),
            tempo_score_margin: Some(0.01),
            beat_grid: None,
            cues: Vec::new(),
        }
    }

    #[test]
    fn a_corrected_tempo_drops_the_beat_grid_and_its_cues_and_a_corrected_key_keeps_them() {
        let grid = crate::detect::BeatGrid {
            first_downbeat_seconds: 0.1,
            bpm: 174.0,
            salience: 2.0,
            drift_seconds: Some(0.01),
        };
        let cues = vec![crate::detect::SectionCue {
            seconds: 0.1,
            section: crate::detect::CueSection::Start,
        }];
        let with_grid = TrackAnalysis {
            beat_grid: Some(grid),
            cues: cues.clone(),
            ..doubtful()
        };
        let key_only = Correction::of_key(Key::new(4, Mode::Minor)).apply(with_grid.clone());
        assert_eq!(key_only.beat_grid, Some(grid));
        assert_eq!(key_only.cues, cues);
        let tempo = Correction::of_bpm(87.0).unwrap().apply(with_grid);
        assert_eq!(tempo.beat_grid, None);
        assert!(tempo.cues.is_empty());
    }

    #[test]
    fn a_correction_wins_and_is_no_longer_rated_or_listed_for_review() {
        let analysis = doubtful();
        assert!(analysis.needs_review());
        let corrected = Correction::of_key(Key::new(4, Mode::Minor))
            .with_bpm(Some(87.0))
            .unwrap()
            .apply(analysis);
        assert_eq!(corrected.key, Some(Key::new(4, Mode::Minor)));
        assert_eq!(corrected.bpm, Some(87.0));
        assert_eq!(corrected.key_runner_up, None);
        assert_eq!(corrected.key_confidence(), None);
        assert_eq!(corrected.tempo_confidence(), None);
        assert!(!corrected.needs_review());
        assert_eq!(corrected.energy, Some(6), "the rest of the analysis stays");
    }

    #[test]
    fn correcting_only_the_key_keeps_the_detected_tempo_and_its_rating() {
        let corrected = Correction::of_key(Key::new(4, Mode::Minor)).apply(doubtful());
        assert_eq!(corrected.bpm, Some(174.0));
        assert_eq!(corrected.tempo_confidence(), Some(Confidence::Low));
        assert!(corrected.needs_review(), "the tempo is still doubtful");
    }

    #[test]
    fn an_empty_correction_changes_nothing() {
        assert!(Correction::default().is_empty());
        assert_eq!(Correction::default().apply(doubtful()), doubtful());
    }

    #[test]
    fn a_tempo_that_is_not_a_positive_number_is_refused() {
        for bpm in [0.0, -120.0, f32::NAN, f32::INFINITY, 1000.0] {
            assert!(
                matches!(
                    Correction::of_bpm(bpm),
                    Err(EuphoriaError::InvalidCorrection { .. })
                ),
                "{bpm}"
            );
        }
        assert_eq!(Correction::of_bpm(87.5).unwrap().bpm, Some(87.5));
        assert_eq!(
            Correction::of_bpm(87.5).unwrap().with_bpm(None).unwrap(),
            Correction::default()
        );
    }
}
