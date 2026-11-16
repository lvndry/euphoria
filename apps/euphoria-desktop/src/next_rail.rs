// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tracks to play after the one playing: the core's ranking of what
//! mixes into it, narrowed by the filter bar, with the energy going the
//! way the user chose. Nothing here ranks tracks itself; it passes the
//! library through `euphoria_core::harmony` and words the result.

use std::path::PathBuf;

use euphoria_core::harmony::{
    EnergyDirection, MixTrack, Relation, energy_move, energy_move_hint, next_tracks_favoring,
};
use serde::{Deserialize, Serialize};

use crate::decks::SuggestionFilter;
use crate::favorites::Favorites;
use crate::library::{Library, Track};
use crate::track_filters::TrackFilters;

/// Where the user wants the energy to go with the next track.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum EnergyChoice {
    #[default]
    Keep,
    Lift,
    Drop,
}

impl EnergyChoice {
    pub(crate) const ALL: [EnergyChoice; 3] =
        [EnergyChoice::Keep, EnergyChoice::Lift, EnergyChoice::Drop];

    pub(crate) fn label(self) -> &'static str {
        match self {
            EnergyChoice::Keep => "Keep",
            EnergyChoice::Lift => "Lift",
            EnergyChoice::Drop => "Drop",
        }
    }

    pub(crate) fn hint(self) -> &'static str {
        match self {
            EnergyChoice::Keep => "Rank tracks at the same energy first",
            EnergyChoice::Lift => "Rank tracks with more energy first",
            EnergyChoice::Drop => "Rank tracks with less energy first",
        }
    }

    pub(crate) fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|choice| *choice == self)
            .unwrap_or(0)
    }

    fn direction(self) -> EnergyDirection {
        match self {
            EnergyChoice::Keep => EnergyDirection::Same,
            EnergyChoice::Lift => EnergyDirection::Up,
            EnergyChoice::Drop => EnergyDirection::Down,
        }
    }
}

/// A track to play after the one playing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NextRow {
    /// Index of the track in the library.
    pub(crate) track_index: usize,
    pub(crate) relation: Relation,
    /// How much faster it is than the playing track, in percent.
    pub(crate) tempo_difference_percent: Option<f32>,
    /// Its own energy rating.
    pub(crate) energy: Option<u8>,
    /// Its energy minus the playing track's, in whole points.
    pub(crate) energy_change: Option<i32>,
    pub(crate) is_favorite: bool,
}

/// A small label of a row, with what it means in words when it needs them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RowPill {
    pub(crate) text: String,
    pub(crate) hint: Option<String>,
}

impl NextRow {
    /// The pills the row shows: how the keys relate, then its energy and
    /// how far that is from the energy of `other`, such as "the track
    /// playing", when both are known.
    pub(crate) fn pills(&self, other: &str) -> Vec<RowPill> {
        let mut pills = vec![RowPill {
            text: relation_pill(self.relation).to_string(),
            hint: None,
        }];
        if let Some((energy, change)) = self.energy.zip(self.energy_change) {
            pills.push(energy_pill(energy, change, other));
        }
        pills
    }
}

/// The words for how a key relates to the playing one.
pub(crate) fn relation_pill(relation: Relation) -> &'static str {
    match relation {
        Relation::Same => "same key",
        Relation::Adjacent => "one step",
        Relation::Relative => "relative",
        Relation::Diagonal => "wheel diagonal",
        Relation::EnergyBoost => "energy boost",
        Relation::MoodChange => "mood change",
        Relation::SemitoneBoost => "semitone boost",
    }
}

/// A track's energy and its change from `other`'s as a pill: "energy 3 ↓6".
pub(crate) fn energy_pill(energy: u8, change: i32, other: &str) -> RowPill {
    let (energy, change) = (f32::from(energy), change as f32);
    RowPill {
        text: format!("energy {}", energy_move(energy, change)),
        hint: Some(energy_move_hint(energy, change, other)),
    }
}

/// The energy of a track in the Live panel's list, with how it differs from
/// what is heard: "energy 7 \u{2191}1", "energy 3 \u{2193}6" or "energy 7 =".
/// This is the wording the main list uses once energy shows its rating.
pub(crate) fn energy_text(energy: Option<u8>, change: Option<i32>) -> Option<String> {
    let energy = energy?;
    Some(format!(
        "energy {}",
        energy_move(f32::from(energy), change.unwrap_or(0) as f32)
    ))
}

/// What the ranking works from.
pub(crate) struct NextQuery<'inputs> {
    pub(crate) library: &'inputs Library,
    pub(crate) favorites: &'inputs Favorites,
    pub(crate) playing_index: usize,
    pub(crate) energy: EnergyChoice,
    pub(crate) track_filters: &'inputs TrackFilters,
    pub(crate) suggestion_filter: &'inputs SuggestionFilter,
    /// The library indexes the candidates come from.
    pub(crate) pool: &'inputs [usize],
    /// Tracks already played this session.
    pub(crate) played: &'inputs [PathBuf],
    pub(crate) limit: usize,
}

/// The best tracks to play after the playing one, best first. The playing
/// track, those already played and those the filter bar hides are never
/// among them.
pub(crate) fn next_after(query: &NextQuery<'_>) -> Vec<NextRow> {
    let Some(playing) = query.library.track(query.playing_index) else {
        return Vec::new();
    };
    rank_after(
        playing,
        Some(&playing.path),
        |candidate| {
            query
                .favorites
                .transition(&playing.path, &candidate.path)
                .is_some()
        },
        query,
    )
}

/// The ranking of [`next_after`] for any `reference` that mixes: a library
/// track, or the key and tempo heard live. `playing_path` is left out of the
/// candidates, and `is_favorite` marks the candidates the user starred after
/// it. The playing track and favorites of `query` are not read.
pub(crate) fn rank_after<Reference: MixTrack + ?Sized>(
    reference: &Reference,
    playing_path: Option<&PathBuf>,
    is_favorite: impl Fn(&Track) -> bool,
    query: &NextQuery<'_>,
) -> Vec<NextRow> {
    let candidates: Vec<&Track> = query
        .pool
        .iter()
        .filter_map(|index| query.library.track(*index))
        .filter(|candidate| {
            Some(&candidate.path) != playing_path
                && !query.played.contains(&candidate.path)
                && query.track_filters.matches(candidate)
                && query.suggestion_filter.genre_fits(candidate.genre())
        })
        .collect();
    next_tracks_favoring(
        reference,
        candidates.iter().copied(),
        &query.suggestion_filter.mix_options(),
        query.energy.direction(),
        |candidate| is_favorite(candidate),
    )
    .into_iter()
    .take(query.limit)
    .filter_map(|mix| {
        Some(NextRow {
            track_index: query.library.index_of(&mix.track.path)?,
            relation: mix.relation,
            tempo_difference_percent: mix.tempo.map(|tempo| tempo.difference_percent),
            energy: mix.track.energy(),
            energy_change: mix.energy_difference.map(|change| change.round() as i32),
            is_favorite: is_favorite(mix.track),
        })
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use euphoria_core::TrackAnalysis;

    use super::*;

    fn library_of(tracks: &[(&str, &str, f32, u8)]) -> Library {
        let mut library = Library::default();
        for (name, key, bpm, energy) in tracks {
            let path = PathBuf::from(format!("/music/{name}.flac"));
            library.add_files(vec![path.clone()]);
            library.record_outcome(
                &path,
                Ok(TrackAnalysis {
                    duration_seconds: 300.0,
                    key: Some(key.parse().unwrap()),
                    bpm: Some(*bpm),
                    energy: Some(*energy),
                    tuning_cents: None,
                    key_runner_up: None,
                    key_score_margin: None,
                    key_source: None,
                    loudness_range_lu: None,
                    pitch_class_profile: None,
                    tempo_estimate: None,
                    tempo_score_margin: None,
                    tempo_octave_evidence: None,
                    beat_grid: None,
                    cues: Vec::new(),
                    key_timeline: Vec::new(),
                    quality: None,
                }),
            );
        }
        library
    }

    fn names(library: &Library, rows: &[NextRow]) -> Vec<String> {
        rows.iter()
            .map(|row| {
                let name = &library.tracks()[row.track_index].file_name;
                name.trim_end_matches(".flac").to_string()
            })
            .collect()
    }

    fn rank(
        library: &Library,
        energy: EnergyChoice,
        track_filters: &TrackFilters,
        played: &[PathBuf],
    ) -> Vec<NextRow> {
        let pool: Vec<usize> = (0..library.tracks().len()).collect();
        next_after(&NextQuery {
            library,
            favorites: &Favorites::default(),
            playing_index: 0,
            energy,
            track_filters,
            suggestion_filter: &SuggestionFilter::default(),
            pool: &pool,
            played,
            limit: 10,
        })
    }

    fn same_key_library() -> Library {
        library_of(&[
            ("playing", "8A", 124.0, 6),
            ("calmer", "8A", 124.0, 5),
            ("level", "8A", 124.0, 6),
            ("lifted", "8A", 124.0, 7),
        ])
    }

    #[test]
    fn the_playing_track_is_never_suggested() {
        let library = same_key_library();
        let rows = rank(&library, EnergyChoice::Keep, &TrackFilters::default(), &[]);
        assert!(!names(&library, &rows).contains(&"playing".to_string()));
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn keep_lift_and_drop_order_the_same_candidates_differently() {
        let library = same_key_library();
        let order = |energy| {
            names(
                &library,
                &rank(&library, energy, &TrackFilters::default(), &[]),
            )
        };
        assert_eq!(order(EnergyChoice::Keep)[0], "level");
        assert_eq!(order(EnergyChoice::Lift)[0], "lifted");
        assert_eq!(order(EnergyChoice::Drop)[0], "calmer");
        assert_eq!(order(EnergyChoice::Lift).last().unwrap(), "calmer");
        assert_eq!(order(EnergyChoice::Drop).last().unwrap(), "lifted");
    }

    #[test]
    fn rows_say_the_energy_change_in_points() {
        let library = same_key_library();
        let rows = rank(&library, EnergyChoice::Lift, &TrackFilters::default(), &[]);
        let changes: Vec<Option<i32>> = rows.iter().map(|row| row.energy_change).collect();
        assert_eq!(changes, [Some(1), Some(0), Some(-1)]);
    }

    #[test]
    fn the_energy_text_shows_the_rating_and_which_way_it_moves() {
        assert_eq!(
            energy_text(Some(7), Some(1)).as_deref(),
            Some("energy 7 \u{2191}1")
        );
        assert_eq!(
            energy_text(Some(3), Some(-6)).as_deref(),
            Some("energy 3 \u{2193}6")
        );
        assert_eq!(energy_text(Some(7), Some(0)).as_deref(), Some("energy 7 ="));
        assert_eq!(energy_text(Some(5), None).as_deref(), Some("energy 5 ="));
        assert_eq!(energy_text(None, Some(2)), None);
    }

    #[test]
    fn pills_use_plain_words_and_the_energy_with_an_arrow() {
        assert_eq!(relation_pill(Relation::Same), "same key");
        assert_eq!(relation_pill(Relation::Adjacent), "one step");
        assert_eq!(relation_pill(Relation::Relative), "relative");
        assert_eq!(relation_pill(Relation::Diagonal), "wheel diagonal");
        let calmer = energy_pill(3, -6, "the track playing");
        assert_eq!(calmer.text, "energy 3 \u{2193}6");
        assert_eq!(
            calmer.hint.as_deref(),
            Some("Energy 3, 6 calmer than the track playing")
        );
        assert_eq!(energy_pill(8, 1, "A").text, "energy 8 \u{2191}1");
        assert_eq!(energy_pill(7, 0, "A").text, "energy 7 =");
        let row = NextRow {
            track_index: 0,
            relation: Relation::Adjacent,
            tempo_difference_percent: None,
            energy: Some(8),
            energy_change: Some(2),
            is_favorite: false,
        };
        let texts = |row: &NextRow| -> Vec<String> {
            row.pills("A").into_iter().map(|pill| pill.text).collect()
        };
        assert_eq!(texts(&row), ["one step", "energy 8 \u{2191}2"]);
        let level = NextRow {
            energy: Some(6),
            energy_change: Some(0),
            relation: Relation::Same,
            ..row
        };
        assert_eq!(texts(&level), ["same key", "energy 6 ="]);
        let unknown = NextRow {
            energy: None,
            ..row
        };
        assert_eq!(texts(&unknown), ["one step"]);
    }

    #[test]
    fn the_filter_bar_keeps_tracks_out_of_the_list() {
        let library = library_of(&[
            ("playing", "8A", 124.0, 6),
            ("in range", "8A", 125.0, 6),
            ("too fast", "8A", 130.0, 6),
            ("too calm", "8A", 124.0, 2),
        ]);
        let filters = TrackFilters {
            bpm_min: Some(120.0),
            bpm_max: Some(126.0),
            energy_min: Some(5),
            ..TrackFilters::default()
        };
        let rows = rank(&library, EnergyChoice::Keep, &filters, &[]);
        assert_eq!(names(&library, &rows), ["in range"]);
    }

    #[test]
    fn tracks_played_this_session_are_left_out() {
        let library = same_key_library();
        let played = [PathBuf::from("/music/level.flac")];
        let rows = rank(
            &library,
            EnergyChoice::Keep,
            &TrackFilters::default(),
            &played,
        );
        let listed = names(&library, &rows);
        assert!(!listed.contains(&"level".to_string()));
        assert_eq!(listed.len(), 2);
    }

    #[test]
    fn the_choice_is_stored_by_name_and_defaults_to_keep() {
        assert_eq!(
            serde_json::to_string(&EnergyChoice::Lift).unwrap(),
            "\"lift\""
        );
        assert_eq!(EnergyChoice::default(), EnergyChoice::Keep);
    }
}
