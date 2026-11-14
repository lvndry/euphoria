// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The filters of the track list: a tempo range, quality verdicts and an
//! energy range, combined with AND, on top of the sidebar selection and the
//! search box.

use euphoria_core::quality::QualityVerdict;
use serde::{Deserialize, Serialize};

use crate::format;
use crate::library::Track;
use crate::metronome::parse_bpm;

/// The lowest energy rating.
pub(crate) const MINIMUM_ENERGY: u8 = 1;

/// The highest energy rating.
pub(crate) const MAXIMUM_ENERGY: u8 = 10;

/// What a quality chip of the filter bar stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum QualityChoice {
    /// Tracks with this verdict.
    Verdict(QualityVerdict),
    /// Tracks whose file is damaged.
    Damaged,
}

impl QualityChoice {
    /// The chips, from the best verdict to damaged files.
    pub(crate) const ALL: [QualityChoice; 7] = [
        QualityChoice::Verdict(QualityVerdict::Lossless),
        QualityChoice::Verdict(QualityVerdict::Good),
        QualityChoice::Verdict(QualityVerdict::Acceptable),
        QualityChoice::Verdict(QualityVerdict::WeakOnABigSystem),
        QualityChoice::Verdict(QualityVerdict::LikelyReencoded),
        QualityChoice::Verdict(QualityVerdict::LimitedBandwidth),
        QualityChoice::Damaged,
    ];

    /// The word the chip shows, the same as the Quality column.
    pub(crate) fn chip_label(self) -> &'static str {
        match self {
            QualityChoice::Verdict(verdict) => format::quality_verdict(verdict),
            QualityChoice::Damaged => "Damaged",
        }
    }

    /// The full name, for the chip's tooltip.
    pub(crate) fn full_label(self) -> &'static str {
        match self {
            QualityChoice::Verdict(verdict) => verdict.label(),
            QualityChoice::Damaged => "Damaged",
        }
    }

    fn fits(self, track: &Track) -> bool {
        let Some(report) = track.quality() else {
            return false;
        };
        match self {
            QualityChoice::Verdict(verdict) => report.verdict == verdict,
            QualityChoice::Damaged => report.is_damaged(),
        }
    }
}

/// The text of the four range fields as the user typed it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TypedTrackFilters {
    pub(crate) bpm_from: String,
    pub(crate) bpm_to: String,
    pub(crate) energy_from: String,
    pub(crate) energy_to: String,
}

/// The filters on the track list. A field left empty is no filter.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct TrackFilters {
    /// The slowest tempo shown, in beats per minute.
    pub(crate) bpm_min: Option<f32>,
    /// The fastest tempo shown.
    pub(crate) bpm_max: Option<f32>,
    /// Whether a track at half or double the tempo range is shown too.
    pub(crate) half_or_double: bool,
    /// The verdicts shown. Empty shows every quality.
    pub(crate) qualities: Vec<QualityChoice>,
    /// The lowest energy shown.
    pub(crate) energy_min: Option<u8>,
    /// The highest energy shown.
    pub(crate) energy_max: Option<u8>,
}

impl TrackFilters {
    /// True when the filters narrow the list. The half and double toggle
    /// alone does not.
    pub(crate) fn is_active(&self) -> bool {
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
    pub(crate) fn matches(&self, track: &Track) -> bool {
        if !self.is_active() {
            return true;
        }
        if track.analysis().is_none() {
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
        self.qualities.is_empty() || self.qualities.iter().any(|choice| choice.fits(track))
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
    pub(crate) fn toggle_quality(&mut self, choice: QualityChoice) {
        match self.qualities.iter().position(|chosen| *chosen == choice) {
            Some(position) => {
                self.qualities.remove(position);
            }
            None => self.qualities.push(choice),
        }
    }

    /// These filters with the ranges read from `typed`. A field that is
    /// empty or does not read leaves that end open.
    pub(crate) fn with_typed(&self, typed: &TypedTrackFilters) -> TrackFilters {
        TrackFilters {
            bpm_min: parse_bpm(&typed.bpm_from),
            bpm_max: parse_bpm(&typed.bpm_to),
            energy_min: parse_energy(&typed.energy_from),
            energy_max: parse_energy(&typed.energy_to),
            ..self.clone()
        }
    }
}

/// The energy typed in a field, or `None` when it is not a number from 1 to
/// 10. A comma or a point both start the decimals, and the rating is rounded
/// to a whole number.
pub(crate) fn parse_energy(text: &str) -> Option<u8> {
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

/// The four fields the ranges are typed in.
pub(crate) struct TrackFilterInputs {
    pub(crate) bpm_from: gpui::Entity<gpui::component::input::InputState>,
    pub(crate) bpm_to: gpui::Entity<gpui::component::input::InputState>,
    pub(crate) energy_from: gpui::Entity<gpui::component::input::InputState>,
    pub(crate) energy_to: gpui::Entity<gpui::component::input::InputState>,
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use euphoria_core::quality::QualityReport;
    use euphoria_core::{Correction, DecodeDamage, TrackAnalysis};

    use super::*;
    use crate::folder_tree::FolderColumn;
    use crate::library::{Library, LibraryFilter};

    fn report(verdict: QualityVerdict, dropped_packets: u32) -> QualityReport {
        QualityReport {
            codec: "mp3".to_string(),
            lossless: false,
            bitrate_kbps: Some(320),
            sample_rate: 44_100,
            bit_depth: None,
            channels: Some(2),
            cutoff_hz: None,
            clipped_share: 0.0,
            integrated_loudness_lufs: None,
            true_peak_dbtp: None,
            verdict,
            damage: DecodeDamage {
                dropped_packets,
                ..DecodeDamage::default()
            },
            cancelled_share: 0.0,
        }
    }

    fn analysis(bpm: f32, energy: u8, quality: Option<QualityReport>) -> TrackAnalysis {
        TrackAnalysis {
            duration_seconds: 300.0,
            key: None,
            bpm: Some(bpm),
            energy: Some(energy),
            tuning_cents: None,
            key_runner_up: None,
            key_score_margin: None,
            key_source: None,
            tempo_estimate: None,
            tempo_score_margin: None,
            beat_grid: None,
            cues: Vec::new(),
            key_timeline: Vec::new(),
            quality,
        }
    }

    const HOUSE: &str = "/music/house.flac";
    const HIP_HOP: &str = "/music/hip-hop.mp3";
    const DRUM_AND_BASS: &str = "/music/dnb.mp3";
    const SLOW: &str = "/music/slow.mp3";
    const DAMAGED: &str = "/music/damaged.mp3";
    const QUEUED: &str = "/music/queued.mp3";
    const FAILED: &str = "/music/failed.mp3";

    fn library() -> Library {
        let mut library = Library::default();
        let paths = [HOUSE, HIP_HOP, DRUM_AND_BASS, SLOW, DAMAGED, QUEUED, FAILED];
        library.add_files(paths.map(PathBuf::from).to_vec());
        let analyzed = [
            (HOUSE, 124.0, 7, QualityVerdict::Lossless, 0),
            (HIP_HOP, 71.0, 4, QualityVerdict::Good, 0),
            (DRUM_AND_BASS, 142.0, 9, QualityVerdict::WeakOnABigSystem, 0),
            (SLOW, 35.0, 2, QualityVerdict::LikelyReencoded, 0),
            (DAMAGED, 100.0, 5, QualityVerdict::Good, 3),
        ];
        for (path, bpm, energy, verdict, dropped) in analyzed {
            library.record_outcome(
                Path::new(path),
                Ok(analysis(bpm, energy, Some(report(verdict, dropped)))),
            );
            // A tempo the user set is shown as set; a detected one is moved
            // into the tempo range on show, 90 to 180 by default.
            library.set_correction(
                Path::new(path),
                Correction {
                    bpm: Some(bpm),
                    ..Correction::default()
                },
            );
        }
        library.record_outcome(Path::new(FAILED), Err("cannot decode".into()));
        library
    }

    fn shown(library: &Library, filters: &TrackFilters) -> Vec<String> {
        library
            .visible_tracks(
                LibraryFilter::AllTracks,
                None,
                "",
                filters,
                None,
                &FolderColumn::new(library.tracks(), library.folders(), None),
            )
            .into_iter()
            .map(|index| library.tracks()[index].path.display().to_string())
            .collect()
    }

    fn typed(
        bpm_from: &str,
        bpm_to: &str,
        energy_from: &str,
        energy_to: &str,
    ) -> TypedTrackFilters {
        TypedTrackFilters {
            bpm_from: bpm_from.to_string(),
            bpm_to: bpm_to.to_string(),
            energy_from: energy_from.to_string(),
            energy_to: energy_to.to_string(),
        }
    }

    #[test]
    fn no_filter_shows_every_track_including_unanalyzed_ones() {
        let library = library();
        let filters = TrackFilters::default();
        assert!(!filters.is_active());
        assert_eq!(shown(&library, &filters).len(), 7);
    }

    #[test]
    fn a_tempo_range_alone_keeps_the_tracks_inside_it() {
        let library = library();
        let filters = TrackFilters::default().with_typed(&typed("120", "130", "", ""));
        assert_eq!(shown(&library, &filters), [HOUSE]);
    }

    #[test]
    fn an_empty_tempo_side_is_no_bound() {
        let library = library();
        let only_min = TrackFilters::default().with_typed(&typed("100", "", "", ""));
        assert_eq!(shown(&library, &only_min), [HOUSE, DRUM_AND_BASS, DAMAGED]);
        let only_max = TrackFilters::default().with_typed(&typed("", "71", "", ""));
        assert_eq!(shown(&library, &only_max), [HIP_HOP, SLOW]);
    }

    #[test]
    fn tempos_read_with_a_comma_or_a_point() {
        let comma = TrackFilters::default().with_typed(&typed("70,5", "71,5", "", ""));
        let point = TrackFilters::default().with_typed(&typed("70.5", "71.5", "", ""));
        assert_eq!(comma, point);
        assert_eq!(comma.bpm_min, Some(70.5));
        assert_eq!(shown(&library(), &comma), [HIP_HOP]);
    }

    #[test]
    fn a_tempo_outside_30_to_300_or_not_a_number_leaves_that_side_open() {
        let filters = TrackFilters::default().with_typed(&typed("20", "fast", "", ""));
        assert_eq!(filters, TrackFilters::default());
    }

    #[test]
    fn the_corrected_tempo_and_energy_are_used() {
        let mut library = library();
        library.set_correction(
            Path::new(SLOW),
            Correction {
                bpm: Some(125.0),
                energy: Some(8),
                ..Correction::default()
            },
        );
        assert_eq!(library.tracks()[3].detected_bpm(), Some(140.0));
        let tempo = TrackFilters::default().with_typed(&typed("120", "130", "", ""));
        assert_eq!(shown(&library, &tempo), [HOUSE, SLOW]);
        let energy = TrackFilters::default().with_typed(&typed("", "", "8", ""));
        assert_eq!(shown(&library, &energy), [DRUM_AND_BASS, SLOW]);
    }

    #[test]
    fn a_detected_tempo_is_matched_as_shown_in_the_tempo_range() {
        let mut library = Library::default();
        library.add_files(vec![PathBuf::from(HIP_HOP)]);
        library.record_outcome(Path::new(HIP_HOP), Ok(analysis(71.0, 4, None)));
        assert_eq!(library.tracks()[0].bpm(), Some(142.0));
        let at_142 = TrackFilters::default().with_typed(&typed("140", "145", "", ""));
        assert_eq!(shown(&library, &at_142), [HIP_HOP]);
        let at_71 = TrackFilters::default().with_typed(&typed("70", "75", "", ""));
        assert!(shown(&library, &at_71).is_empty());
        let mut halved = at_71;
        halved.half_or_double = true;
        assert_eq!(shown(&library, &halved), [HIP_HOP]);
    }

    #[test]
    fn half_or_double_matches_the_range_at_half_and_double_speed() {
        let library = library();
        let mut filters = TrackFilters::default().with_typed(&typed("70", "75", "", ""));
        assert_eq!(shown(&library, &filters), [HIP_HOP]);
        filters.half_or_double = true;
        assert_eq!(
            shown(&library, &filters),
            [HIP_HOP, DRUM_AND_BASS, SLOW],
            "142 is double 71, and 35 is half of 70"
        );
    }

    #[test]
    fn an_energy_range_alone_keeps_the_tracks_inside_it() {
        let library = library();
        let filters = TrackFilters::default().with_typed(&typed("", "", "4", "7"));
        assert_eq!(shown(&library, &filters), [HOUSE, HIP_HOP, DAMAGED]);
        let comma = TrackFilters::default().with_typed(&typed("", "", "4,2", "6.8"));
        assert_eq!(comma.energy_min, Some(4));
        assert_eq!(comma.energy_max, Some(7));
        assert_eq!(
            TrackFilters::default().with_typed(&typed("", "", "0", "11")),
            TrackFilters::default()
        );
    }

    #[test]
    fn quality_chips_show_any_of_the_chosen_verdicts() {
        let library = library();
        let mut filters = TrackFilters::default();
        filters.toggle_quality(QualityChoice::Verdict(QualityVerdict::Lossless));
        assert_eq!(shown(&library, &filters), [HOUSE]);
        filters.toggle_quality(QualityChoice::Verdict(QualityVerdict::WeakOnABigSystem));
        assert_eq!(shown(&library, &filters), [HOUSE, DRUM_AND_BASS]);
        filters.toggle_quality(QualityChoice::Verdict(QualityVerdict::Lossless));
        assert_eq!(shown(&library, &filters), [DRUM_AND_BASS]);
    }

    #[test]
    fn the_damaged_chip_shows_damaged_files_whatever_their_verdict() {
        let library = library();
        let mut filters = TrackFilters::default();
        filters.toggle_quality(QualityChoice::Damaged);
        assert_eq!(shown(&library, &filters), [DAMAGED]);
    }

    #[test]
    fn filters_combine_with_and() {
        let library = library();
        let mut filters = TrackFilters::default().with_typed(&typed("90", "", "5", ""));
        assert_eq!(shown(&library, &filters), [HOUSE, DRUM_AND_BASS, DAMAGED]);
        filters.toggle_quality(QualityChoice::Verdict(QualityVerdict::Good));
        assert_eq!(shown(&library, &filters), [DAMAGED]);
        filters.energy_min = Some(6);
        assert!(shown(&library, &filters).is_empty());
    }

    #[test]
    fn filters_combine_with_the_search_and_the_folder_and_the_sidebar_selection() {
        let library = library();
        let filters = TrackFilters::default().with_typed(&typed("", "", "5", ""));
        let column = FolderColumn::new(library.tracks(), library.folders(), None);
        let rows = |selection, folder: Option<&Path>, query| {
            library.visible_tracks(selection, folder, query, &filters, None, &column)
        };
        assert_eq!(rows(LibraryFilter::AllTracks, None, "").len(), 3);
        assert_eq!(rows(LibraryFilter::AllTracks, None, "dnb").len(), 1);
        assert_eq!(
            rows(LibraryFilter::AllTracks, Some(Path::new("/elsewhere")), "").len(),
            0
        );
        assert_eq!(
            rows(LibraryFilter::NeedsReview, None, "").len(),
            0,
            "the failed track needs review but has no analysis to pass a filter"
        );
    }

    #[test]
    fn tracks_without_an_analysis_are_hidden_while_a_filter_is_set() {
        let library = library();
        for typed in [typed("30", "300", "", ""), typed("", "", "1", "10")] {
            let filters = TrackFilters::default().with_typed(&typed);
            let names = shown(&library, &filters);
            assert!(!names.contains(&QUEUED.to_string()), "{names:?}");
            assert!(!names.contains(&FAILED.to_string()), "{names:?}");
        }
        let mut quality = TrackFilters::default();
        quality.toggle_quality(QualityChoice::Damaged);
        quality.toggle_quality(QualityChoice::Verdict(QualityVerdict::Good));
        let names = shown(&library, &quality);
        assert!(!names.contains(&QUEUED.to_string()));
        assert!(!names.contains(&FAILED.to_string()));
    }

    #[test]
    fn the_half_or_double_toggle_alone_is_not_a_filter() {
        let filters = TrackFilters {
            half_or_double: true,
            ..TrackFilters::default()
        };
        assert!(!filters.is_active());
        assert_eq!(shown(&library(), &filters).len(), 7);
    }

    #[test]
    fn the_chips_use_the_verdict_names_of_the_table() {
        for verdict in [
            QualityVerdict::Lossless,
            QualityVerdict::Good,
            QualityVerdict::Acceptable,
            QualityVerdict::WeakOnABigSystem,
            QualityVerdict::LikelyReencoded,
            QualityVerdict::LimitedBandwidth,
        ] {
            assert!(QualityChoice::ALL.contains(&QualityChoice::Verdict(verdict)));
            assert_eq!(
                QualityChoice::Verdict(verdict).chip_label(),
                format::quality_verdict(verdict)
            );
            assert_eq!(
                QualityChoice::Verdict(verdict).full_label(),
                verdict.label()
            );
        }
    }

    #[test]
    fn filters_survive_a_round_trip_through_json() {
        let mut filters = TrackFilters::default().with_typed(&typed("70", "75,5", "3", ""));
        filters.half_or_double = true;
        filters.toggle_quality(QualityChoice::Damaged);
        let text = serde_json::to_string(&filters).unwrap();
        assert_eq!(
            serde_json::from_str::<TrackFilters>(&text).unwrap(),
            filters
        );
        assert_eq!(
            serde_json::from_str::<TrackFilters>("{}").unwrap(),
            TrackFilters::default()
        );
    }
}
