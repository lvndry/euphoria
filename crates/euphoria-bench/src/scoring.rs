// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Scoring rules shared by published key and tempo evaluations.

use euphoria_core::Key;

#[derive(Debug, Default)]
pub struct KeyTally {
    pub exact: usize,
    pub fifth: usize,
    pub relative: usize,
    pub parallel: usize,
    pub other: usize,
    pub missing: usize,
}

impl KeyTally {
    pub fn record(&mut self, reference: Key, estimate: Key) {
        let interval = (i32::from(estimate.tonic) - i32::from(reference.tonic)).rem_euclid(12);
        let same_mode = estimate.mode == reference.mode;
        let reference_is_major = matches!(reference.mode, euphoria_core::Mode::Major);
        let relative_interval = if reference_is_major { 9 } else { 3 };

        if same_mode && interval == 0 {
            self.exact += 1;
        } else if same_mode && (interval == 7 || interval == 5) {
            self.fifth += 1;
        } else if !same_mode && interval == relative_interval {
            self.relative += 1;
        } else if !same_mode && interval == 0 {
            self.parallel += 1;
        } else {
            self.other += 1;
        }
    }

    pub fn record_missing(&mut self) {
        self.missing += 1;
    }

    pub fn total(&self) -> usize {
        self.exact + self.fifth + self.relative + self.parallel + self.other + self.missing
    }

    /// MIREX weighted score: exact 1.0, fifth 0.5, relative 0.3, parallel 0.2.
    pub fn mirex_score(&self) -> f64 {
        let weighted = self.exact as f64
            + 0.5 * self.fifth as f64
            + 0.3 * self.relative as f64
            + 0.2 * self.parallel as f64;
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
const OCTAVE_FACTORS: [f32; 5] = [1.0, 2.0, 0.5, 3.0, 1.0 / 3.0];

fn within_tolerance(reference: f32, estimate: f32) -> bool {
    (estimate - reference).abs() <= TOLERANCE * reference
}

impl TempoTally {
    pub fn record(&mut self, reference: f32, estimate: f32) {
        self.total += 1;
        if within_tolerance(reference, estimate) {
            self.accurate += 1;
        }
        if OCTAVE_FACTORS
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
    fn tempo_octave_errors_only_count_in_the_lenient_score() {
        let mut tally = TempoTally::default();
        tally.record(124.0, 62.0);
        tally.record(124.0, 125.0);
        tally.record(124.0, 140.0);
        assert_eq!(tally.accurate, 1);
        assert_eq!(tally.accurate_with_octaves, 2);
    }
}
