// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Scoring rules shared by published key and tempo evaluations.

use std::fmt::{self, Display, Formatter};

use euphoria_core::Key;

#[derive(Debug, Default)]
pub struct KeyTally {
    pub exact: usize,
    pub fifth: usize,
    pub relative: usize,
    pub parallel: usize,
    pub other: usize,
    pub missing: usize,
    /// The MIREX weight of every track, in the order they were recorded.
    track_scores: Vec<f64>,
}

/// How one detector did against another on the same tracks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Comparison {
    /// Tracks this detector scored higher on.
    pub better: usize,
    /// Tracks this detector scored lower on.
    pub worse: usize,
}

/// Two tallies that cannot be compared track by track, because they did not
/// record the same number of tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DifferentTrackCounts {
    /// Tracks the tally being compared recorded.
    pub track_count: usize,
    /// Tracks the tally it was compared with recorded.
    pub other_track_count: usize,
}

impl Display for DifferentTrackCounts {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "not comparable: {} tracks against {}",
            self.track_count, self.other_track_count
        )
    }
}

const FIFTH_WEIGHT: f64 = 0.5;
const RELATIVE_WEIGHT: f64 = 0.3;
const PARALLEL_WEIGHT: f64 = 0.2;

/// How an estimated key relates to a reference key, in the categories of the
/// standard key evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyRelation {
    Exact,
    Fifth,
    Relative,
    Parallel,
    Other,
}

impl KeyRelation {
    /// Every relation, in the order reports list them.
    pub const ALL: [Self; 5] = [
        Self::Exact,
        Self::Fifth,
        Self::Relative,
        Self::Parallel,
        Self::Other,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Exact => "same key",
            Self::Fifth => "a fifth away",
            Self::Relative => "relative key",
            Self::Parallel => "parallel key",
            Self::Other => "other",
        }
    }
}

/// How `estimate` relates to `reference`.
pub fn key_relation(reference: Key, estimate: Key) -> KeyRelation {
    let interval = (i32::from(estimate.tonic()) - i32::from(reference.tonic())).rem_euclid(12);
    let same_mode = estimate.mode() == reference.mode();
    let reference_is_major = matches!(reference.mode(), euphoria_core::Mode::Major);
    let relative_interval = if reference_is_major { 9 } else { 3 };
    if same_mode && interval == 0 {
        KeyRelation::Exact
    } else if same_mode && (interval == 7 || interval == 5) {
        KeyRelation::Fifth
    } else if !same_mode && interval == relative_interval {
        KeyRelation::Relative
    } else if !same_mode && interval == 0 {
        KeyRelation::Parallel
    } else {
        KeyRelation::Other
    }
}

impl KeyTally {
    pub fn record(&mut self, reference: Key, estimate: Key) {
        let track_score = match key_relation(reference, estimate) {
            KeyRelation::Exact => {
                self.exact += 1;
                1.0
            }
            KeyRelation::Fifth => {
                self.fifth += 1;
                FIFTH_WEIGHT
            }
            KeyRelation::Relative => {
                self.relative += 1;
                RELATIVE_WEIGHT
            }
            KeyRelation::Parallel => {
                self.parallel += 1;
                PARALLEL_WEIGHT
            }
            KeyRelation::Other => {
                self.other += 1;
                0.0
            }
        };
        self.track_scores.push(track_score);
    }

    pub fn record_missing(&mut self) {
        self.missing += 1;
        self.track_scores.push(0.0);
    }

    /// Counts the tracks this tally scored higher and lower than `other`,
    /// which must have recorded the same tracks in the same order. Fails
    /// when the two recorded a different number of tracks.
    pub fn compared_with(&self, other: &Self) -> Result<Comparison, DifferentTrackCounts> {
        if self.track_scores.len() != other.track_scores.len() {
            return Err(DifferentTrackCounts {
                track_count: self.track_scores.len(),
                other_track_count: other.track_scores.len(),
            });
        }
        let mut comparison = Comparison::default();
        for (score, other_score) in self.track_scores.iter().zip(&other.track_scores) {
            if score > other_score {
                comparison.better += 1;
            } else if score < other_score {
                comparison.worse += 1;
            }
        }
        Ok(comparison)
    }

    pub fn total(&self) -> usize {
        self.exact + self.fifth + self.relative + self.parallel + self.other + self.missing
    }

    /// MIREX weighted score: exact 1.0, fifth 0.5, relative 0.3, parallel 0.2.
    pub fn mirex_score(&self) -> f64 {
        let weighted = self.exact as f64
            + FIFTH_WEIGHT * self.fifth as f64
            + RELATIVE_WEIGHT * self.relative as f64
            + PARALLEL_WEIGHT * self.parallel as f64;
        weighted / self.total() as f64
    }

    pub fn percent(&self, count: usize) -> String {
        format!("{:.1}%", 100.0 * count as f64 / self.total() as f64)
    }
}

#[derive(Debug, Default)]
pub struct TempoTally {
    pub accurate: usize,
    pub accurate_with_octaves: usize,
    pub missing: usize,
    total: usize,
}

const TOLERANCE: f32 = 0.04;
/// Tempo multiples accepted by Accuracy 2, the standard lenient tempo score.
/// Triple and third tempo count as correct along with double and half.
const ACCURACY_2_FACTORS: [f32; 5] = [1.0, 2.0, 0.5, 3.0, 1.0 / 3.0];

/// Whether a tempo estimate counts as the reference tempo: within 4% of it.
pub fn within_tolerance(reference: f32, estimate: f32) -> bool {
    (estimate - reference).abs() <= TOLERANCE * reference
}

impl TempoTally {
    pub fn record(&mut self, reference: f32, estimate: f32) {
        self.total += 1;
        if within_tolerance(reference, estimate) {
            self.accurate += 1;
        }
        if ACCURACY_2_FACTORS
            .iter()
            .any(|factor| within_tolerance(reference * factor, estimate))
        {
            self.accurate_with_octaves += 1;
        }
    }

    pub fn record_missing(&mut self) {
        self.total += 1;
        self.missing += 1;
    }

    pub fn percent(&self, count: usize) -> String {
        format!("{:.1}%", 100.0 * count as f64 / self.total as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tally_for(reference: &str, estimate: &str) -> KeyTally {
        let mut tally = KeyTally::default();
        tally.record(reference.parse().unwrap(), estimate.parse().unwrap());
        tally
    }

    #[test]
    fn classifies_key_errors() {
        assert_eq!(tally_for("A minor", "A minor").exact, 1);
        assert_eq!(tally_for("A minor", "E minor").fifth, 1);
        assert_eq!(tally_for("A minor", "D minor").fifth, 1);
        assert_eq!(tally_for("A minor", "C major").relative, 1);
        assert_eq!(tally_for("C major", "A minor").relative, 1);
        assert_eq!(tally_for("A minor", "A major").parallel, 1);
        assert_eq!(tally_for("A minor", "B minor").other, 1);
    }

    #[test]
    fn compares_two_detectors_track_by_track() {
        let reference: Key = "A minor".parse().unwrap();
        let mut first = KeyTally::default();
        let mut second = KeyTally::default();
        for (first_estimate, second_estimate) in [
            ("A minor", "A minor"),
            ("B minor", "E minor"),
            ("A minor", "C major"),
            ("A major", "A minor"),
        ] {
            first.record(reference, first_estimate.parse().unwrap());
            second.record(reference, second_estimate.parse().unwrap());
        }
        first.record_missing();
        second.record(reference, reference);

        assert_eq!(
            second.compared_with(&first),
            Ok(Comparison {
                better: 3,
                worse: 1
            })
        );
        assert_eq!(
            first.compared_with(&second),
            Ok(Comparison {
                better: 1,
                worse: 3
            })
        );
        assert_eq!(first.compared_with(&first), Ok(Comparison::default()));
    }

    #[test]
    fn tallies_of_different_lengths_are_not_compared() {
        let reference: Key = "A minor".parse().unwrap();
        let mut longer = KeyTally::default();
        let mut shorter = KeyTally::default();
        for _ in 0..3 {
            longer.record(reference, reference);
        }
        shorter.record(reference, "E minor".parse().unwrap());
        shorter.record_missing();

        let mismatch = DifferentTrackCounts {
            track_count: 3,
            other_track_count: 2,
        };
        assert_eq!(longer.compared_with(&shorter), Err(mismatch));
        assert_eq!(
            shorter.compared_with(&longer),
            Err(DifferentTrackCounts {
                track_count: 2,
                other_track_count: 3,
            })
        );
        assert_eq!(mismatch.to_string(), "not comparable: 3 tracks against 2");
    }

    #[test]
    fn accuracy_2_accepts_double_half_triple_and_third_tempo() {
        let mut tally = TempoTally::default();
        tally.record(120.0, 121.0);
        tally.record(120.0, 60.0);
        tally.record(120.0, 240.0);
        tally.record(120.0, 360.0);
        tally.record(120.0, 40.0);
        tally.record(120.0, 140.0);
        assert_eq!(tally.accurate, 1);
        assert_eq!(tally.accurate_with_octaves, 5);
    }
}
