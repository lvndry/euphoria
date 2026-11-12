// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Asking the core which tracks and keys mix with the selected track. The
//! harmonic rules live in `euphoria_core::harmony`; this module only passes
//! the library through them.

use euphoria_core::harmony::{MixOptions, MixTrack, Relation, mixes_into};
use euphoria_core::{Key, Mode};

use crate::favorites::Favorites;
use crate::library::{Library, Track};

impl MixTrack for Track {
    fn key(&self) -> Option<Key> {
        Track::key(self)
    }

    fn bpm(&self) -> Option<f32> {
        Track::bpm(self)
    }

    fn energy(&self) -> Option<f32> {
        Track::energy(self).map(f32::from)
    }
}

/// A track that mixes with the selected one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MixSuggestion {
    /// Index of the suggested track in the library.
    pub(crate) track_index: usize,
    /// How its key relates to the selected track's key. Only a marked
    /// transition can have none: its keys do not mix by the rules, but the
    /// user found they do.
    pub(crate) relation: Option<Relation>,
    /// How much faster it is than the selected track, in percent, when both
    /// have a tempo.
    pub(crate) tempo_difference_percent: Option<f32>,
    /// True when the user marked the transition from the selected track
    /// into this one.
    pub(crate) is_favorite: bool,
}

/// A track as it would be with its key shifted, for finding what it mixes
/// with in that key. The tempo and energy do not change with the key.
struct Shifted<'track> {
    track: &'track Track,
    semitones: i8,
}

impl MixTrack for Shifted<'_> {
    fn key(&self) -> Option<Key> {
        self.track.key().map(|key| key.transposed(self.semitones))
    }

    fn bpm(&self) -> Option<f32> {
        self.track.bpm()
    }

    fn energy(&self) -> Option<f32> {
        self.track.energy().map(f32::from)
    }
}

/// The tracks in the library that mix with the track at `selected_index`
/// with its key shifted by `semitones`, at most `limit` of them: first the
/// transitions out of it the user marked, in the order they were marked,
/// then the others, smoothest first as the core ranks them.
pub(crate) fn suggestions(
    library: &Library,
    favorites: &Favorites,
    selected_index: usize,
    semitones: i8,
    limit: usize,
) -> Vec<MixSuggestion> {
    let Some(selected) = library.track(selected_index) else {
        return Vec::new();
    };
    let shifted = Shifted {
        track: selected,
        semitones,
    };
    let mut suggested: Vec<MixSuggestion> = favorites
        .transitions_from(&selected.path)
        .filter_map(|transition| {
            let track_index = library.index_of(&transition.to)?;
            let track = library.track(track_index)?;
            Some(MixSuggestion {
                track_index,
                relation: shifted
                    .key()
                    .zip(track.key())
                    .and_then(|(from, to)| from.relation_to(&to)),
                tempo_difference_percent: selected
                    .bpm()
                    .zip(track.bpm())
                    .filter(|(from, _)| *from > 0.0)
                    .map(|(from, to)| (to / from - 1.0) * 100.0),
                is_favorite: true,
            })
        })
        .collect();
    if shifted.key().is_none() && suggested.is_empty() {
        return suggested;
    }
    let candidates = library.tracks().iter().filter(|candidate| {
        candidate.path != selected.path
            && favorites
                .transition(&selected.path, &candidate.path)
                .is_none()
    });
    let room = limit.saturating_sub(suggested.len());
    suggested.extend(
        mixes_into(&shifted, candidates, &MixOptions::default())
            .into_iter()
            .take(room)
            .filter_map(|mix| {
                Some(MixSuggestion {
                    track_index: library.index_of(&mix.track.path)?,
                    relation: Some(mix.relation),
                    tempo_difference_percent: mix.tempo.map(|tempo| tempo.difference_percent),
                    is_favorite: false,
                })
            }),
    );
    suggested.truncate(limit);
    suggested
}

/// Every other key the core counts as mixing with `key` by default.
pub(crate) fn compatible_keys(key: &Key) -> Vec<Key> {
    let relations = MixOptions::default().relations;
    [Mode::Minor, Mode::Major]
        .into_iter()
        .flat_map(|mode| (0..12).map(move |tonic| Key::new(tonic, mode)))
        .filter(|candidate| candidate != key)
        .filter(|candidate| {
            key.relation_to(candidate)
                .is_some_and(|relation| relations.contains(&relation))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use euphoria_core::TrackAnalysis;

    use super::*;

    fn library_of(tracks: &[(&str, &str, f32)]) -> Library {
        let mut library = Library::default();
        for (name, key, bpm) in tracks {
            let path = PathBuf::from(format!("/music/{name}.flac"));
            library.add_files(vec![path.clone()]);
            library.record_outcome(
                &path,
                Ok(TrackAnalysis {
                    duration_seconds: 300.0,
                    key: Some(key.parse().unwrap()),
                    bpm: Some(*bpm),
                    energy: None,
                    tuning_cents: None,
                    key_runner_up: None,
                    key_score_margin: None,
                    tempo_estimate: None,
                    tempo_score_margin: None,
                    beat_grid: None,
                    cues: Vec::new(),
                    key_timeline: Vec::new(),
                    quality: None,
                }),
            );
        }
        library
    }

    #[test]
    fn a_shifted_key_is_suggested_with_the_tracks_of_that_key() {
        let library = library_of(&[
            ("selected", "A minor", 124.0),
            ("in a minor", "A minor", 124.0),
            ("in b minor", "B minor", 124.0),
        ]);
        let names = |semitones: i8| -> Vec<String> {
            suggestions(&library, &Favorites::default(), 0, semitones, 10)
                .iter()
                .map(|mix| library.tracks()[mix.track_index].file_name.clone())
                .collect()
        };
        assert_eq!(names(0), ["in a minor.flac"]);
        // Two semitones up, A minor becomes B minor.
        assert_eq!(names(2), ["in b minor.flac"]);
    }

    #[test]
    fn suggests_tracks_in_compatible_keys_and_leaves_out_the_selected_one() {
        let library = library_of(&[
            ("selected", "A minor", 124.0),
            ("adjacent", "E minor", 122.0),
            ("relative", "C major", 126.0),
            ("same", "A minor", 125.0),
            ("clashing", "Eb minor", 124.0),
            ("too fast", "A minor", 150.0),
        ]);
        let suggested = suggestions(&library, &Favorites::default(), 0, 0, 10);
        let names: Vec<&str> = suggested
            .iter()
            .map(|mix| library.tracks()[mix.track_index].file_name.as_str())
            .collect();
        assert_eq!(names, ["same.flac", "adjacent.flac", "relative.flac"]);
        assert_eq!(suggested[0].relation, Some(Relation::Same));
        let faster = suggested[0].tempo_difference_percent.unwrap();
        assert!((faster - 100.0 / 124.0).abs() < 0.01, "{faster}");
    }

    #[test]
    fn candidates_that_tie_on_key_and_tempo_are_ordered_by_closest_energy() {
        let mut library = Library::default();
        for (name, energy) in [("selected", 6), ("far", 10), ("near", 7), ("same", 6)] {
            let path = PathBuf::from(format!("/music/{name}.flac"));
            library.add_files(vec![path.clone()]);
            library.record_outcome(
                &path,
                Ok(TrackAnalysis {
                    duration_seconds: 300.0,
                    key: Some("A minor".parse().unwrap()),
                    bpm: Some(124.0),
                    energy: Some(energy),
                    tuning_cents: None,
                    key_runner_up: None,
                    key_score_margin: None,
                    tempo_estimate: None,
                    tempo_score_margin: None,
                    beat_grid: None,
                    cues: Vec::new(),
                    key_timeline: Vec::new(),
                    quality: None,
                }),
            );
        }
        let names: Vec<&str> = suggestions(&library, &Favorites::default(), 0, 0, 10)
            .iter()
            .map(|mix| library.tracks()[mix.track_index].file_name.as_str())
            .collect();
        assert_eq!(names, ["same.flac", "near.flac", "far.flac"]);
    }

    #[test]
    fn a_marked_transition_comes_first_even_when_the_keys_clash() {
        let library = library_of(&[
            ("selected", "A minor", 124.0),
            ("same", "A minor", 124.0),
            ("clashing", "Eb minor", 124.0),
            ("adjacent", "E minor", 124.0),
        ]);
        let mut favorites = Favorites::default();
        let path_of = |name: &str| PathBuf::from(format!("/music/{name}.flac"));
        favorites.toggle_transition(&path_of("selected"), &path_of("clashing"));
        favorites.toggle_transition(&path_of("selected"), &path_of("adjacent"));
        favorites.toggle_transition(&path_of("same"), &path_of("selected"));
        let suggested = suggestions(&library, &favorites, 0, 0, 10);
        let names: Vec<&str> = suggested
            .iter()
            .map(|mix| library.tracks()[mix.track_index].file_name.as_str())
            .collect();
        assert_eq!(
            names,
            ["clashing.flac", "adjacent.flac", "same.flac"],
            "a transition into the selected track is not one out of it"
        );
        assert!(suggested[0].is_favorite && suggested[1].is_favorite);
        assert!(!suggested[2].is_favorite);
        assert_eq!(suggested[0].relation, None);
        assert_eq!(suggested[1].relation, Some(Relation::Adjacent));
        assert_eq!(suggested[0].tempo_difference_percent, Some(0.0));
        assert_eq!(suggestions(&library, &favorites, 0, 0, 1).len(), 1);
    }

    #[test]
    fn keeps_at_most_the_limit() {
        let library = library_of(&[
            ("selected", "A minor", 124.0),
            ("first", "A minor", 124.0),
            ("second", "A minor", 124.0),
        ]);
        assert_eq!(
            suggestions(&library, &Favorites::default(), 0, 0, 1).len(),
            1
        );
        assert!(suggestions(&library, &Favorites::default(), 99, 0, 10).is_empty());
    }

    #[test]
    fn a_track_without_a_key_has_no_suggestions() {
        let mut library = library_of(&[("other", "A minor", 124.0)]);
        library.add_files(vec![PathBuf::from("/music/pending.flac")]);
        let pending = library.index_of(Path::new("/music/pending.flac")).unwrap();
        assert!(suggestions(&library, &Favorites::default(), pending, 0, 10).is_empty());
    }

    #[test]
    fn compatible_keys_are_the_neighbours_and_the_relative_key() {
        let a_minor: Key = "A minor".parse().unwrap();
        let mut codes: Vec<String> = compatible_keys(&a_minor).iter().map(Key::camelot).collect();
        codes.sort();
        assert_eq!(codes, ["7A", "8B", "9A"]);
    }
}
