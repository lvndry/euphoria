// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Measures how often keys and tempos rated at each confidence level are
//! right, how often the runner up key is, and how often the half or double
//! tempo offered as an alternative is.

use std::fmt::{self, Display, Formatter};

use euphoria_core::detect::{KeyReading, StratumDetector, TempoReading, read_key};
use euphoria_core::{Confidence, Key, KeyEngine, TempoRange};

use crate::dataset::Dataset;
use crate::evaluate::{TrackOutcome, measure_tracks};
use crate::scoring::{KeyRelation, KeyTally, key_relation, within_tolerance};

/// The levels in the order reports list them.
const LEVELS: [Confidence; 3] = [Confidence::High, Confidence::Medium, Confidence::Low];

fn level_index(confidence: Confidence) -> usize {
    match confidence {
        Confidence::High => 0,
        Confidence::Medium => 1,
        Confidence::Low => 2,
    }
}

/// Key accuracy per confidence level of the shipped key detector.
#[derive(Debug, Default)]
pub struct KeyConfidenceReport {
    levels: [KeyLevel; 3],
    /// How the runner up relates to the detected key, by [`KeyRelation`].
    runner_up_relations: [usize; 5],
    /// Tracks whose detected key is not the annotated one.
    wrong: usize,
    /// Of those, tracks whose runner up is the annotated key.
    runner_up_right_when_wrong: usize,
    /// Tracks with no key, or no confidence for their key.
    without_confidence: usize,
    /// Tracks that failed to decode or to analyze.
    failures: usize,
}

#[derive(Debug, Default)]
struct KeyLevel {
    tally: KeyTally,
    key_or_runner_up_exact: usize,
}

impl KeyConfidenceReport {
    fn record(&mut self, reference: Key, reading: &KeyReading) {
        let (Some(key), Some(runner_up), Some(confidence)) =
            (reading.key, reading.runner_up, reading.confidence())
        else {
            self.without_confidence += 1;
            return;
        };
        let level = &mut self.levels[level_index(confidence)];
        level.tally.record(reference, key);
        let relation_index = |relation: KeyRelation| {
            KeyRelation::ALL
                .iter()
                .position(|candidate| *candidate == relation)
                .unwrap_or(KeyRelation::ALL.len() - 1)
        };
        self.runner_up_relations[relation_index(key_relation(key, runner_up))] += 1;
        let runner_up_right = key_relation(reference, runner_up) == KeyRelation::Exact;
        if key_relation(reference, key) == KeyRelation::Exact {
            level.key_or_runner_up_exact += 1;
        } else {
            self.wrong += 1;
            if runner_up_right {
                level.key_or_runner_up_exact += 1;
                self.runner_up_right_when_wrong += 1;
            }
        }
    }

    fn rated(&self) -> usize {
        self.levels.iter().map(|level| level.tally.total()).sum()
    }
}

/// Runs the shipped key detector over every track and tallies its accuracy
/// by confidence level.
pub fn evaluate_key_confidence(dataset: &Dataset<Key>) -> KeyConfidenceReport {
    let outcomes = measure_tracks(&dataset.tracks, 1, |_, audio| {
        read_key(KeyEngine::Auto, audio).map(Some)
    });
    let mut report = KeyConfidenceReport::default();
    for (track, outcome) in dataset.tracks.iter().zip(outcomes) {
        match first_estimate(outcome) {
            Some(reading) => report.record(track.annotation, &reading),
            None => report.failures += 1,
        }
    }
    report
}

/// Tempo accuracy per confidence level of the shipped tempo detector, with
/// tempos reported in the default range.
#[derive(Debug, Default)]
pub struct TempoConfidenceReport {
    levels: [TempoLevel; 3],
    /// Tracks offered a half or double tempo as an alternative.
    alternative_offered: usize,
    /// Tracks whose reported tempo is half or double the annotated one.
    octave_errors: usize,
    /// Of those, tracks whose alternative is the annotated tempo.
    resolved_by_alternative: usize,
    /// Tracks with no tempo.
    without_tempo: usize,
    /// Tracks that failed to decode or to analyze.
    failures: usize,
}

#[derive(Debug, Default, Clone, Copy)]
struct TempoLevel {
    tracks: usize,
    accurate: usize,
    right_modulo_octaves: usize,
}

/// Factors that leave a tempo in the same octave class, allowing up to two
/// doublings or halvings. This rates the beat period, not its reported octave.
const OCTAVE_FACTORS: [f32; 5] = [0.25, 0.5, 1.0, 2.0, 4.0];

impl TempoConfidenceReport {
    fn record(&mut self, reference: f32, reading: TempoReading) {
        let range = TempoRange::DEFAULT;
        let reported = reading.tempo_in(range);
        let level = &mut self.levels[level_index(reading.confidence())];
        level.tracks += 1;
        let accurate = within_tolerance(reference, reported);
        level.accurate += usize::from(accurate);
        level.right_modulo_octaves += usize::from(
            OCTAVE_FACTORS
                .iter()
                .any(|factor| within_tolerance(reference, reported * factor)),
        );
        let alternative = range.alternative(reported);
        self.alternative_offered += usize::from(alternative.is_some());
        let off_by_an_octave = !accurate
            && [0.5, 2.0]
                .iter()
                .any(|factor| within_tolerance(reference, reported * factor));
        if off_by_an_octave {
            self.octave_errors += 1;
            if alternative.is_some_and(|alternative| within_tolerance(reference, alternative)) {
                self.resolved_by_alternative += 1;
            }
        }
    }

    fn rated(&self) -> usize {
        self.levels.iter().map(|level| level.tracks).sum()
    }
}

/// Runs the shipped tempo detector over every track and tallies its
/// accuracy by confidence level.
pub fn evaluate_tempo_confidence(dataset: &Dataset<f32>) -> TempoConfidenceReport {
    let outcomes = measure_tracks(&dataset.tracks, 1, |_, audio| {
        StratumDetector.read_tempo(audio).map(Some)
    });
    let mut report = TempoConfidenceReport::default();
    for (track, outcome) in dataset.tracks.iter().zip(outcomes) {
        match first_estimate(outcome) {
            Some(Some(reading)) => report.record(track.annotation, reading),
            Some(None) => report.without_tempo += 1,
            None => report.failures += 1,
        }
    }
    report
}

/// The estimate of the only detector run on a track, or `None` when the
/// track failed to decode or the detector failed.
fn first_estimate<Estimate>(outcome: TrackOutcome<Estimate>) -> Option<Estimate> {
    outcome.ok()?.into_iter().next()?.estimate.ok()?
}

fn percent(count: usize, total: usize) -> String {
    if total == 0 {
        return "-".to_string();
    }
    format!("{:.1}%", 100.0 * count as f64 / total as f64)
}

impl Display for KeyConfidenceReport {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "## Key confidence, libkeyfinder\n")?;
        writeln!(
            formatter,
            "| Confidence | Tracks | Exact | MIREX score | Key or runner up exact |"
        )?;
        writeln!(formatter, "| --- | --- | --- | --- | --- |")?;
        let rated = self.rated();
        for (confidence, level) in LEVELS.iter().zip(&self.levels) {
            let total = level.tally.total();
            writeln!(
                formatter,
                "| {confidence} | {total} ({}) | {} | {} | {} |",
                percent(total, rated),
                percent(level.tally.exact, total),
                if total == 0 {
                    "-".to_string()
                } else {
                    format!("{:.1}%", 100.0 * level.tally.mirex_score())
                },
                percent(level.key_or_runner_up_exact, total),
            )?;
        }
        let relations: Vec<String> = KeyRelation::ALL
            .iter()
            .zip(&self.runner_up_relations)
            .filter(|(relation, _)| **relation != KeyRelation::Exact)
            .map(|(relation, count)| format!("{} {count}", relation.name()))
            .collect();
        writeln!(
            formatter,
            "\nThe runner up, against the detected key: {}.",
            relations.join(", ")
        )?;
        writeln!(
            formatter,
            "Of the {} tracks whose detected key is wrong, the runner up is the annotated key for {}.",
            self.wrong, self.runner_up_right_when_wrong
        )?;
        write_left_out(formatter, self.without_confidence, "no key", self.failures)
    }
}

impl Display for TempoConfidenceReport {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        writeln!(
            formatter,
            "## Tempo confidence, stratum-dsp, reported between 90 and 180 BPM\n"
        )?;
        writeln!(
            formatter,
            "| Confidence | Tracks | Accuracy 1 | Right modulo octaves |"
        )?;
        writeln!(formatter, "| --- | --- | --- | --- |")?;
        let rated = self.rated();
        for (confidence, level) in LEVELS.iter().zip(&self.levels) {
            writeln!(
                formatter,
                "| {confidence} | {} ({}) | {} | {} |",
                level.tracks,
                percent(level.tracks, rated),
                percent(level.accurate, level.tracks),
                percent(level.right_modulo_octaves, level.tracks),
            )?;
        }
        writeln!(
            formatter,
            "\nA half or double tempo is offered as an alternative for {} tracks ({}). Of the {} tracks reported at half or double the annotated tempo, the alternative is the annotated tempo for {}.",
            self.alternative_offered,
            percent(self.alternative_offered, rated),
            self.octave_errors,
            self.resolved_by_alternative
        )?;
        write_left_out(formatter, self.without_tempo, "no tempo", self.failures)
    }
}

fn write_left_out(
    formatter: &mut Formatter<'_>,
    without: usize,
    what: &str,
    failures: usize,
) -> fmt::Result {
    if without > 0 || failures > 0 {
        writeln!(
            formatter,
            "Left out: {without} tracks with {what}, {failures} that failed."
        )?;
    }
    writeln!(formatter)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> Key {
        text.parse().unwrap()
    }

    fn key_reading(detected: &str, runner_up: &str, score_margin: f32) -> KeyReading {
        KeyReading {
            key: Some(key(detected)),
            runner_up: Some(key(runner_up)),
            score_margin: Some(score_margin),
            ..KeyReading::default()
        }
    }

    #[test]
    fn key_tracks_are_tallied_by_level_with_their_runner_up() {
        let mut report = KeyConfidenceReport::default();
        let reference = key("A minor");
        report.record(reference, &key_reading("A minor", "C major", 0.05));
        report.record(reference, &key_reading("E minor", "A minor", 0.05));
        report.record(reference, &key_reading("D major", "B minor", 0.001));
        report.record(reference, &KeyReading::default());

        let high = &report.levels[0];
        assert_eq!(high.tally.total(), 2);
        assert_eq!(high.tally.exact, 1);
        assert_eq!(high.key_or_runner_up_exact, 2);
        assert_eq!(report.levels[2].tally.total(), 1);
        assert_eq!(report.levels[2].key_or_runner_up_exact, 0);
        assert_eq!((report.wrong, report.runner_up_right_when_wrong), (2, 1));
        assert_eq!(report.without_confidence, 1);
        // A minor to C major is relative, E minor to A minor a fifth, D major
        // to B minor relative.
        assert_eq!(report.runner_up_relations, [0, 1, 2, 0, 0]);

        let rendered = report.to_string();
        assert!(
            rendered.contains("| high | 2 (66.7%) | 50.0% |"),
            "{rendered}"
        );
        assert!(
            rendered.contains("| medium | 0 (0.0%) | - | - | - |"),
            "{rendered}"
        );
        assert!(
            rendered.contains("the runner up is the annotated key for 1."),
            "{rendered}"
        );
        assert!(rendered.contains("Left out: 1 tracks with no key, 0 that failed."));
    }

    fn tempo_reading(estimate: f32, score_margin: f32) -> TempoReading {
        TempoReading {
            estimate,
            score_margin: Some(score_margin),
        }
    }

    #[test]
    fn tempo_confidence_accuracy_accepts_two_octaves_but_not_a_triple() {
        let mut report = TempoConfidenceReport::default();
        // The reported tempo is always 120; its octave is the range's choice.
        for reference in [30.0, 60.0, 120.0, 240.0, 480.0, 360.0] {
            report.record(reference, tempo_reading(120.0, 0.6));
        }

        let high = report.levels[0];
        assert_eq!(high.tracks, 6);
        assert_eq!(high.accurate, 1);
        assert_eq!(high.right_modulo_octaves, 5);
        assert!(report.to_string().contains("Right modulo octaves"));
    }

    #[test]
    fn tempo_tracks_are_tallied_by_level_with_their_alternative() {
        let mut report = TempoConfidenceReport::default();
        report.record(128.0, tempo_reading(64.0, 0.6));
        report.record(85.0, tempo_reading(85.0, 0.6));
        report.record(140.0, tempo_reading(93.3, 0.2));
        report.record(128.0, tempo_reading(128.0, 0.01));

        let high = report.levels[0];
        assert_eq!(
            (high.tracks, high.accurate, high.right_modulo_octaves),
            (2, 1, 2)
        );
        assert_eq!(report.levels[1].right_modulo_octaves, 0);
        assert_eq!(report.levels[2].accurate, 1);
        assert_eq!(
            (report.octave_errors, report.resolved_by_alternative),
            (1, 1)
        );
        assert_eq!(report.alternative_offered, 2);

        let rendered = report.to_string();
        assert!(
            rendered.contains("| high | 2 (50.0%) | 50.0% | 100.0% |"),
            "{rendered}"
        );
        assert!(
            rendered.contains("alternative for 2 tracks (50.0%)"),
            "{rendered}"
        );
    }
}
