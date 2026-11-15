// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The filters of the track list: a tempo range, quality verdicts and an
//! energy range, combined with AND, on top of the sidebar selection and the
//! search box. The matching lives in the core, shared with the command line;
//! this module adds what only the window needs.

use euphoria_core::quality::QualityReport;
pub(crate) use euphoria_core::track_filter::{MAXIMUM_ENERGY, MINIMUM_ENERGY};
pub(crate) use euphoria_core::{FilterableTrack, QualityChoice, TrackFilters, TypedTrackFilters};

use crate::format;
use crate::library::Track;

/// The word the chip of a quality choice shows, the same as the Quality
/// column.
pub(crate) fn chip_label(choice: QualityChoice) -> &'static str {
    match choice {
        QualityChoice::Verdict(verdict) => format::quality_verdict(verdict),
        QualityChoice::Damaged => "Damaged",
    }
}

impl FilterableTrack for Track {
    fn is_analyzed(&self) -> bool {
        self.analysis().is_some()
    }

    fn bpm(&self) -> Option<f32> {
        Track::bpm(self)
    }

    fn energy(&self) -> Option<u8> {
        Track::energy(self)
    }

    fn quality(&self) -> Option<&QualityReport> {
        Track::quality(self)
    }
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

    use euphoria_core::quality::{QualityReport, QualityVerdict};
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
            loudness_range_lu: None,
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
                chip_label(QualityChoice::Verdict(verdict)),
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
