// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The filters of a track list: a tempo range, quality verdicts and an
//! energy range, combined with AND. The desktop app's filter bar and the
//! command line's `--bpm` and `--energy` both match tracks with these.

use serde::{Deserialize, Serialize};

use crate::quality::{QualityReport, QualityVerdict};

/// The lowest energy rating.
pub const MINIMUM_ENERGY: u8 = 1;

/// The highest energy rating.
pub const MAXIMUM_ENERGY: u8 = 10;

/// The slowest tempo a filter reads, in beats per minute.
pub const MINIMUM_FILTER_BPM: f32 = 30.0;

/// The fastest tempo a filter reads, in beats per minute.
pub const MAXIMUM_FILTER_BPM: f32 = 300.0;

/// What a quality chip of a filter bar stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum QualityChoice {
    /// Tracks with this verdict.
    Verdict(QualityVerdict),
    /// Tracks whose file is damaged.
    Damaged,
}

impl QualityChoice {
    /// The choices, from the best verdict to damaged files.
    pub const ALL: [QualityChoice; 7] = [
        QualityChoice::Verdict(QualityVerdict::Lossless),
        QualityChoice::Verdict(QualityVerdict::Good),
        QualityChoice::Verdict(QualityVerdict::Acceptable),
        QualityChoice::Verdict(QualityVerdict::WeakOnABigSystem),
        QualityChoice::Verdict(QualityVerdict::LikelyReencoded),
        QualityChoice::Verdict(QualityVerdict::LimitedBandwidth),
        QualityChoice::Damaged,
    ];

    /// The full name of the choice.
    pub fn full_label(self) -> &'static str {
        match self {
            QualityChoice::Verdict(verdict) => verdict.label(),
            QualityChoice::Damaged => "Damaged",
        }
    }

    /// Whether `report` is what the choice stands for.
    pub fn fits(self, report: &QualityReport) -> bool {
        match self {
            QualityChoice::Verdict(verdict) => report.verdict == verdict,
            QualityChoice::Damaged => report.is_damaged(),
        }
    }
}

/// What a filter needs to know about a track, with corrected values in
/// place of detected ones.
pub trait FilterableTrack {
    /// True when the track has an analysis.
    fn is_analyzed(&self) -> bool;
    /// The tempo the track is shown at.
    fn bpm(&self) -> Option<f32>;
    /// The energy rating from 1 to 10.
    fn energy(&self) -> Option<u8>;
    /// How the file sounds, once analyzed.
    fn quality(&self) -> Option<&QualityReport>;
}

impl<Track: FilterableTrack + ?Sized> FilterableTrack for &Track {
    fn is_analyzed(&self) -> bool {
        (**self).is_analyzed()
    }

    fn bpm(&self) -> Option<f32> {
        (**self).bpm()
    }

    fn energy(&self) -> Option<u8> {
        (**self).energy()
    }

    fn quality(&self) -> Option<&QualityReport> {
        (**self).quality()
    }
}

/// The text of the four range fields as the user typed it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TypedTrackFilters {
    /// The slowest tempo.
    pub bpm_from: String,
    /// The fastest tempo.
    pub bpm_to: String,
    /// The lowest energy.
    pub energy_from: String,
    /// The highest energy.
    pub energy_to: String,
}

/// The filters on a track list. A field left empty is no filter.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackFilters {
    /// The slowest tempo shown, in beats per minute.
    pub bpm_min: Option<f32>,
    /// The fastest tempo shown.
    pub bpm_max: Option<f32>,
    /// Whether a track at half or double the tempo range is shown too.
    pub half_or_double: bool,
    /// The verdicts shown. Empty shows every quality.
    pub qualities: Vec<QualityChoice>,
    /// The lowest energy shown.
    pub energy_min: Option<u8>,
    /// The highest energy shown.
    pub energy_max: Option<u8>,
}

impl TrackFilters {
    /// True when the filters narrow the list. The half and double toggle
    /// alone does not.
    pub fn is_active(&self) -> bool {
        self.has_tempo() || self.has_energy() || !self.qualities.is_empty()
    }

    fn has_tempo(&self) -> bool {
        self.bpm_min.is_some() || self.bpm_max.is_some()
    }

    fn has_energy(&self) -> bool {
        self.energy_min.is_some() || self.energy_max.is_some()
    }

    /// Whether `track` passes every filter. A track without an analysis
    /// passes only while no tempo, energy or quality filter is set.
    pub fn matches(&self, track: &impl FilterableTrack) -> bool {
        if !self.is_active() {
            return true;
        }
        if !track.is_analyzed() {
            return false;
        }
        if self.has_tempo() && !track.bpm().is_some_and(|bpm| self.tempo_fits(bpm)) {
            return false;
        }
        if self.has_energy()
            && !track
                .energy()
                .is_some_and(|energy| self.energy_fits(energy))
        {
            return false;
        }
        self.qualities.is_empty()
            || track
                .quality()
                .is_some_and(|report| self.qualities.iter().any(|choice| choice.fits(report)))
    }

    fn tempo_fits(&self, bpm: f32) -> bool {
        let within = |bpm: f32| {
            self.bpm_min.is_none_or(|minimum| bpm >= minimum)
                && self.bpm_max.is_none_or(|maximum| bpm <= maximum)
        };
        within(bpm) || (self.half_or_double && (within(bpm * 2.0) || within(bpm / 2.0)))
    }

    fn energy_fits(&self, energy: u8) -> bool {
        self.energy_min.is_none_or(|minimum| energy >= minimum)
            && self.energy_max.is_none_or(|maximum| energy <= maximum)
    }

    /// Adds `choice` to the verdicts shown, or takes it out when it is there.
    pub fn toggle_quality(&mut self, choice: QualityChoice) {
        match self.qualities.iter().position(|chosen| *chosen == choice) {
            Some(position) => {
                self.qualities.remove(position);
            }
            None => self.qualities.push(choice),
        }
    }

    /// These filters with the ranges read from `typed`. A field that is
    /// empty or does not read leaves that end open.
    pub fn with_typed(&self, typed: &TypedTrackFilters) -> TrackFilters {
        TrackFilters {
            bpm_min: parse_bpm(&typed.bpm_from),
            bpm_max: parse_bpm(&typed.bpm_to),
            energy_min: parse_energy(&typed.energy_from),
            energy_max: parse_energy(&typed.energy_to),
            ..self.clone()
        }
    }

    /// These filters with the tempo range of `text` such as `120-126`,
    /// `120-` or `-126`. A single tempo is that tempo only.
    ///
    /// # Errors
    ///
    /// A sentence saying how a range is written when `text` is not one.
    pub fn with_bpm_text(mut self, text: &str) -> Result<Self, String> {
        let (minimum, maximum) = parse_range(text, parse_bpm, || {
            format!(
                "a tempo range is written 120-126, 120- or -126, in beats per minute from {} to {}",
                MINIMUM_FILTER_BPM, MAXIMUM_FILTER_BPM
            )
        })?;
        self.bpm_min = minimum;
        self.bpm_max = maximum;
        Ok(self)
    }

    /// These filters with the energy range of `text` such as `6-8`, `6-`
    /// or `-8`. A single rating is that rating only.
    ///
    /// # Errors
    ///
    /// A sentence saying how a range is written when `text` is not one.
    pub fn with_energy_text(mut self, text: &str) -> Result<Self, String> {
        let (minimum, maximum) = parse_range(text, parse_energy, || {
            format!(
                "an energy range is written 6-8, 6- or -8, with ratings from {MINIMUM_ENERGY} to {MAXIMUM_ENERGY}"
            )
        })?;
        self.energy_min = minimum;
        self.energy_max = maximum;
        Ok(self)
    }
}

/// The tempo typed in a field, or `None` when it is not a number from 30 to
/// 300. A comma or a point both start the decimals, and the tempo is
/// rounded to a tenth.
pub fn parse_bpm(text: &str) -> Option<f32> {
    let bpm: f32 = text.trim().replace(',', ".").parse().ok()?;
    if !bpm.is_finite() || !(MINIMUM_FILTER_BPM..=MAXIMUM_FILTER_BPM).contains(&bpm) {
        return None;
    }
    Some((bpm * 10.0).round() / 10.0)
}

/// The energy typed in a field, or `None` when it is not a number from 1 to
/// 10. A comma or a point both start the decimals, and the rating is rounded
/// to a whole number.
pub fn parse_energy(text: &str) -> Option<u8> {
    let typed: f32 = text.trim().replace(',', ".").parse().ok()?;
    if !typed.is_finite() {
        return None;
    }
    let rounded = typed.round();
    if rounded < f32::from(MINIMUM_ENERGY) || rounded > f32::from(MAXIMUM_ENERGY) {
        return None;
    }
    Some(rounded as u8)
}

/// Reads `from-to`, `from-` or `-to` with `parse` for each end, or one
/// value for both ends. A comma is a decimal sign, never a separator.
fn parse_range<Value: PartialOrd + Copy>(
    text: &str,
    parse: impl Fn(&str) -> Option<Value>,
    problem: impl Fn() -> String,
) -> Result<(Option<Value>, Option<Value>), String> {
    let text = text.trim();
    let (from, to) = match text.split_once('-') {
        Some((from, to)) => (from.trim(), to.trim()),
        None => (text, text),
    };
    if from.is_empty() && to.is_empty() {
        return Err(problem());
    }
    let read = |part: &str| -> Result<Option<Value>, String> {
        if part.is_empty() {
            Ok(None)
        } else {
            parse(part).map(Some).ok_or_else(&problem)
        }
    };
    let (minimum, maximum) = (read(from)?, read(to)?);
    if let (Some(minimum), Some(maximum)) = (minimum, maximum)
        && minimum > maximum
    {
        return Err(format!("{}: the lower end comes first", problem()));
    }
    Ok((minimum, maximum))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Track {
        analyzed: bool,
        bpm: Option<f32>,
        energy: Option<u8>,
    }

    impl FilterableTrack for Track {
        fn is_analyzed(&self) -> bool {
            self.analyzed
        }
        fn bpm(&self) -> Option<f32> {
            self.bpm
        }
        fn energy(&self) -> Option<u8> {
            self.energy
        }
        fn quality(&self) -> Option<&QualityReport> {
            None
        }
    }

    fn track(bpm: f32, energy: u8) -> Track {
        Track {
            analyzed: true,
            bpm: Some(bpm),
            energy: Some(energy),
        }
    }

    #[test]
    fn no_filter_passes_every_track_and_an_active_one_needs_an_analysis() {
        let unanalyzed = Track {
            analyzed: false,
            bpm: None,
            energy: None,
        };
        assert!(TrackFilters::default().matches(&unanalyzed));
        let tempo = TrackFilters::default().with_bpm_text("120-").unwrap();
        assert!(!tempo.matches(&unanalyzed));
    }

    #[test]
    fn tempo_ranges_read_closed_open_and_with_comma_decimals() {
        let closed = TrackFilters::default().with_bpm_text("120-126").unwrap();
        assert_eq!((closed.bpm_min, closed.bpm_max), (Some(120.0), Some(126.0)));
        assert!(closed.matches(&track(126.0, 5)));
        assert!(!closed.matches(&track(126.1, 5)));
        let open = TrackFilters::default().with_bpm_text("120-").unwrap();
        assert!(open.matches(&track(170.0, 5)) && !open.matches(&track(119.0, 5)));
        let upto = TrackFilters::default().with_bpm_text("-100").unwrap();
        assert!(upto.matches(&track(80.0, 5)) && !upto.matches(&track(101.0, 5)));
        let comma = TrackFilters::default()
            .with_bpm_text("120,5-126,5")
            .unwrap();
        assert_eq!((comma.bpm_min, comma.bpm_max), (Some(120.5), Some(126.5)));
        let single = TrackFilters::default().with_bpm_text("124").unwrap();
        assert!(single.matches(&track(124.0, 5)) && !single.matches(&track(125.0, 5)));
    }

    #[test]
    fn a_range_that_does_not_read_says_how_to_write_one() {
        for text in ["", "-", "fast", "126-120", "10-20", "120-400"] {
            assert!(
                TrackFilters::default().with_bpm_text(text).is_err(),
                "{text:?}"
            );
        }
        for text in ["", "-", "0-5", "8-6", "6-11", "x"] {
            assert!(
                TrackFilters::default().with_energy_text(text).is_err(),
                "{text:?}"
            );
        }
    }

    #[test]
    fn half_or_double_matches_the_range_at_half_and_double_speed() {
        let mut filters = TrackFilters::default().with_bpm_text("70-75").unwrap();
        assert!(!filters.matches(&track(142.0, 5)));
        filters.half_or_double = true;
        assert!(filters.matches(&track(142.0, 5)), "double of 71");
        assert!(filters.matches(&track(35.0, 5)), "half of 70");
        assert!(!filters.matches(&track(100.0, 5)));
    }

    #[test]
    fn the_half_and_double_toggle_alone_narrows_nothing() {
        let filters = TrackFilters {
            half_or_double: true,
            ..TrackFilters::default()
        };
        assert!(!filters.is_active());
        assert!(filters.matches(&track(100.0, 1)));
    }

    #[test]
    fn energy_ranges_are_closed_or_open_and_both_filters_must_pass() {
        let from_six = TrackFilters::default().with_energy_text("6-").unwrap();
        assert!(from_six.matches(&track(120.0, 6)) && !from_six.matches(&track(120.0, 5)));
        let between = TrackFilters::default().with_energy_text("6-8").unwrap();
        assert!(between.matches(&track(120.0, 8)) && !between.matches(&track(120.0, 9)));
        let both = between.with_bpm_text("120-126").unwrap();
        assert!(both.matches(&track(124.0, 7)));
        assert!(!both.matches(&track(130.0, 7)));
        assert!(!both.matches(&track(124.0, 3)));
    }

    #[test]
    fn typed_fields_read_with_a_comma_or_a_point_and_bad_ends_stay_open() {
        let typed = |from: &str, to: &str| TypedTrackFilters {
            bpm_from: from.to_string(),
            bpm_to: to.to_string(),
            ..TypedTrackFilters::default()
        };
        let comma = TrackFilters::default().with_typed(&typed("70,5", "71,5"));
        let point = TrackFilters::default().with_typed(&typed("70.5", "71.5"));
        assert_eq!(comma, point);
        assert_eq!(comma.bpm_min, Some(70.5));
        let open = TrackFilters::default().with_typed(&typed("20", "fast"));
        assert_eq!(open, TrackFilters::default());
    }
}
