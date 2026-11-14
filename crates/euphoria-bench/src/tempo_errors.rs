// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How the shipped tempo detector is wrong on GiantSteps Tempo: by the kind
//! of error, and by genre, with 95% intervals on the accuracies.

use std::collections::BTreeMap;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::Path;

use euphoria_core::TempoRange;
use euphoria_core::detect::{StratumDetector, TempoDetector};

use crate::dataset::Dataset;
use crate::evaluate::measure_tracks;
use crate::scoring::within_tolerance;
use crate::statistics::wilson_interval;

/// Genres with fewer tracks than this are reported together.
const MINIMUM_GENRE_TRACKS: usize = 20;

/// How a reported tempo relates to the annotated one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TempoError {
    Right,
    Half,
    Double,
    ThreeToTwo,
    FourToThree,
    FiveToFour,
    TripleOrThird,
    Other,
}

impl TempoError {
    const ALL: [Self; 8] = [
        Self::Right,
        Self::Half,
        Self::Double,
        Self::ThreeToTwo,
        Self::FourToThree,
        Self::FiveToFour,
        Self::TripleOrThird,
        Self::Other,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Right => "Right, within 4%",
            Self::Half => "Half the annotated tempo",
            Self::Double => "Double the annotated tempo",
            Self::ThreeToTwo => "3:2 or 2:3 of the annotated tempo",
            Self::FourToThree => "4:3 or 3:4 of the annotated tempo",
            Self::FiveToFour => "5:4 or 4:5 of the annotated tempo",
            Self::TripleOrThird => "Triple or a third of the annotated tempo",
            Self::Other => "Other",
        }
    }
}

/// The first kind of error whose ratio the reported tempo matches.
pub fn classify(reference: f32, reported: f32) -> TempoError {
    let ratios = [
        (TempoError::Right, &[1.0_f32][..]),
        (TempoError::Half, &[0.5]),
        (TempoError::Double, &[2.0]),
        (TempoError::ThreeToTwo, &[1.5, 2.0 / 3.0]),
        (TempoError::FourToThree, &[4.0 / 3.0, 0.75]),
        (TempoError::FiveToFour, &[1.25, 0.8]),
        (TempoError::TripleOrThird, &[3.0, 1.0 / 3.0]),
    ];
    ratios
        .iter()
        .find(|(_, factors)| {
            factors
                .iter()
                .any(|factor| within_tolerance(reference * factor, reported))
        })
        .map_or(TempoError::Other, |(kind, _)| *kind)
}

#[derive(Debug, Default, Clone, Copy)]
struct Accuracy {
    tracks: usize,
    accurate: usize,
    accurate_with_octaves: usize,
}

impl Accuracy {
    fn record(&mut self, reference: f32, reported: f32) {
        self.tracks += 1;
        self.accurate += usize::from(within_tolerance(reference, reported));
        self.accurate_with_octaves += usize::from(
            [1.0, 2.0, 0.5, 3.0, 1.0 / 3.0]
                .iter()
                .any(|factor| within_tolerance(reference * factor, reported)),
        );
    }
}

fn share_with_interval(count: usize, total: usize) -> String {
    match wilson_interval(count, total) {
        Some((low, high)) => format!(
            "{:.1}% ({:.1} to {:.1})",
            100.0 * count as f64 / total as f64,
            100.0 * low,
            100.0 * high
        ),
        None => "-".to_string(),
    }
}

/// Errors and accuracy of the shipped tempo detector, in the default range.
#[derive(Debug, Default)]
pub struct TempoErrorReport {
    overall: Accuracy,
    kinds: BTreeMap<TempoError, usize>,
    genres: BTreeMap<String, Accuracy>,
    /// Tracks reported at double their annotated tempo, which is below the
    /// slowest tempo of the default range.
    doubled_below_range: usize,
    missing: usize,
}

/// Runs the shipped tempo detector over every track and sorts its errors.
/// Genres come from `annotations/genre` in `dataset_directory`.
pub fn evaluate_tempo_errors(dataset: &Dataset<f32>, dataset_directory: &Path) -> TempoErrorReport {
    let outcomes = measure_tracks(&dataset.tracks, 1, |_, audio| {
        StratumDetector.detect_tempo(audio)
    });
    let mut report = TempoErrorReport::default();
    for (track, outcome) in dataset.tracks.iter().zip(outcomes) {
        let reported = outcome
            .ok()
            .and_then(|measurements| measurements.into_iter().next())
            .and_then(|measurement| measurement.estimate.ok().flatten());
        let Some(reported) = reported else {
            report.missing += 1;
            continue;
        };
        report.overall.record(track.annotation, reported);
        let kind = classify(track.annotation, reported);
        *report.kinds.entry(kind).or_default() += 1;
        report.doubled_below_range += usize::from(
            kind == TempoError::Double && track.annotation < TempoRange::DEFAULT.slowest(),
        );
        let genre = fs::read_to_string(
            dataset_directory
                .join("annotations/genre")
                .join(format!("{}.genre", track.name)),
        )
        .map(|text| text.trim().to_string())
        .unwrap_or_default();
        report
            .genres
            .entry(genre)
            .or_default()
            .record(track.annotation, reported);
    }
    report
}

impl Display for TempoErrorReport {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let tracks = self.overall.tracks;
        writeln!(
            formatter,
            "## Tempo errors, tempos between 90 and 180 BPM\n"
        )?;
        writeln!(
            formatter,
            "{tracks} tracks scored, {} without a tempo or failed. Intervals are 95% Wilson intervals.\n",
            self.missing
        )?;
        writeln!(formatter, "| Accuracy | Tracks | Share |")?;
        writeln!(formatter, "| --- | --- | --- |")?;
        writeln!(
            formatter,
            "| Accuracy 1 | {} | {} |",
            self.overall.accurate,
            share_with_interval(self.overall.accurate, tracks)
        )?;
        writeln!(
            formatter,
            "| Accuracy 2 | {} | {} |\n",
            self.overall.accurate_with_octaves,
            share_with_interval(self.overall.accurate_with_octaves, tracks)
        )?;
        writeln!(formatter, "| Kind of result | Tracks | Share |")?;
        writeln!(formatter, "| --- | --- | --- |")?;
        for kind in TempoError::ALL {
            let count = self.kinds.get(&kind).copied().unwrap_or(0);
            writeln!(
                formatter,
                "| {} | {count} | {} |",
                kind.label(),
                share_with_interval(count, tracks)
            )?;
        }
        writeln!(
            formatter,
            "Of the tracks reported at double, {} are annotated below {} BPM, which the default range reports at double.\n",
            self.doubled_below_range,
            TempoRange::DEFAULT.slowest()
        )?;
        writeln!(formatter, "| Genre | Tracks | Accuracy 1 | Accuracy 2 |")?;
        writeln!(formatter, "| --- | --- | --- | --- |")?;
        let mut rest = Accuracy::default();
        let mut sorted: Vec<(&String, &Accuracy)> = self.genres.iter().collect();
        sorted.sort_by_key(|(_, accuracy)| std::cmp::Reverse(accuracy.tracks));
        for (genre, accuracy) in sorted {
            if accuracy.tracks < MINIMUM_GENRE_TRACKS {
                rest.tracks += accuracy.tracks;
                rest.accurate += accuracy.accurate;
                rest.accurate_with_octaves += accuracy.accurate_with_octaves;
                continue;
            }
            write_genre_row(formatter, genre, accuracy)?;
        }
        if rest.tracks > 0 {
            write_genre_row(formatter, "other genres", &rest)?;
        }
        writeln!(formatter)
    }
}

fn write_genre_row(formatter: &mut Formatter<'_>, genre: &str, accuracy: &Accuracy) -> fmt::Result {
    writeln!(
        formatter,
        "| {genre} | {} | {} | {} |",
        accuracy.tracks,
        share_with_interval(accuracy.accurate, accuracy.tracks),
        share_with_interval(accuracy.accurate_with_octaves, accuracy.tracks)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_the_ratio_between_reported_and_annotated_tempo() {
        assert_eq!(classify(128.0, 129.0), TempoError::Right);
        assert_eq!(classify(87.0, 174.0), TempoError::Double);
        assert_eq!(classify(174.0, 87.0), TempoError::Half);
        assert_eq!(classify(130.0, 173.0), TempoError::FourToThree);
        assert_eq!(classify(173.0, 130.0), TempoError::FourToThree);
        assert_eq!(classify(120.0, 150.0), TempoError::FiveToFour);
        assert_eq!(classify(100.0, 150.0), TempoError::ThreeToTwo);
        assert_eq!(classify(60.0, 180.0), TempoError::TripleOrThird);
        assert_eq!(classify(100.0, 117.0), TempoError::Other);
    }
}
