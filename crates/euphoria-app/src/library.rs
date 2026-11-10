// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tracks the user has added and the order and subset shown in the table.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use euphoria_core::{Key, TrackAnalysis};

use crate::camelot::CamelotPosition;
use crate::format;

/// Where a track is in its analysis.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TrackStatus {
    /// Queued or being analyzed.
    Pending,
    /// The core returned a result.
    Analyzed(TrackAnalysis),
    /// The core could not analyze the file; the text says why.
    Failed(String),
}

/// One audio file in the library, with the text the table shows for it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Track {
    /// The audio file.
    pub(crate) path: PathBuf,
    /// The file name, shown as the title.
    pub(crate) file_name: String,
    /// The file extension in upper case, for example "FLAC".
    pub(crate) format: String,
    /// The name of the folder holding the file.
    pub(crate) folder: String,
    /// Where the track is in its analysis.
    pub(crate) status: TrackStatus,
    file_name_lowercase: String,
}

impl Track {
    fn new(path: PathBuf) -> Self {
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let format = path
            .extension()
            .map(|extension| extension.to_string_lossy().to_uppercase())
            .unwrap_or_default();
        let folder = path
            .parent()
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self {
            file_name_lowercase: file_name.to_lowercase(),
            path,
            file_name,
            format,
            folder,
            status: TrackStatus::Pending,
        }
    }

    /// The analysis, when the track has one.
    pub(crate) fn analysis(&self) -> Option<&TrackAnalysis> {
        match &self.status {
            TrackStatus::Analyzed(analysis) => Some(analysis),
            TrackStatus::Pending | TrackStatus::Failed(_) => None,
        }
    }

    /// The detected key, when the track has one.
    pub(crate) fn key(&self) -> Option<Key> {
        self.analysis().and_then(|analysis| analysis.key)
    }

    /// The detected tempo, when the track has one.
    pub(crate) fn bpm(&self) -> Option<f32> {
        self.analysis().and_then(|analysis| analysis.bpm)
    }

    /// The length in seconds, once the track is analyzed.
    pub(crate) fn duration_seconds(&self) -> Option<f32> {
        self.analysis().map(|analysis| analysis.duration_seconds)
    }

    /// Why analysis failed, when it did.
    pub(crate) fn failure(&self) -> Option<&str> {
        match &self.status {
            TrackStatus::Failed(reason) => Some(reason),
            TrackStatus::Pending | TrackStatus::Analyzed(_) => None,
        }
    }

    /// True when every word of `query_lowercase` matches the track: as part
    /// of the file name, as the whole Camelot code, or as the start of the
    /// tempo.
    fn matches(&self, query_lowercase: &str) -> bool {
        query_lowercase.split_whitespace().all(|word| {
            self.file_name_lowercase.contains(word)
                || self
                    .key()
                    .is_some_and(|key| key.camelot().eq_ignore_ascii_case(word))
                || self
                    .bpm()
                    .is_some_and(|bpm| format::bpm(bpm).starts_with(word))
        })
    }
}

/// The column the table is sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortColumn {
    /// File name, ignoring letter case.
    Title,
    /// Position around the Camelot wheel: number, then minor before major.
    Key,
    /// Tempo.
    Bpm,
    /// Length.
    Duration,
    /// File format.
    Format,
    /// Name of the containing folder.
    Folder,
}

/// A column and a direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SortOrder {
    /// The column to sort by.
    pub(crate) column: SortColumn,
    /// True for largest first.
    pub(crate) descending: bool,
}

impl SortOrder {
    /// The order after the header of `column` is clicked: ascending on a new
    /// column, reversed on the current one.
    pub(crate) fn after_click(current: Option<SortOrder>, column: SortColumn) -> SortOrder {
        match current {
            Some(order) if order.column == column => SortOrder {
                column,
                descending: !order.descending,
            },
            _ => SortOrder {
                column,
                descending: false,
            },
        }
    }
}

/// Which part of the library the table shows.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum LibraryFilter {
    /// Every track.
    #[default]
    AllTracks,
    /// Tracks whose analysis failed.
    NeedsReview,
    /// Tracks inside one of the folders the user added.
    Folder(PathBuf),
}

/// How many tracks are in each state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct LibraryCounts {
    /// Every track.
    pub(crate) total: usize,
    /// Tracks whose analysis failed.
    pub(crate) failed: usize,
}

/// Every track the user has added, in the order they were added.
#[derive(Debug, Default)]
pub(crate) struct Library {
    tracks: Vec<Track>,
    index_by_path: HashMap<PathBuf, usize>,
    folders: Vec<PathBuf>,
    failed_count: usize,
}

impl Library {
    /// The tracks, in the order they were added. A track keeps its index for
    /// as long as the library lives.
    pub(crate) fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    /// The track at `index`.
    pub(crate) fn track(&self, index: usize) -> Option<&Track> {
        self.tracks.get(index)
    }

    /// The folders the user added, in the order they were added.
    pub(crate) fn folders(&self) -> &[PathBuf] {
        &self.folders
    }

    /// Remembers a folder the user added. Returns false when it was already
    /// there.
    pub(crate) fn add_folder(&mut self, folder: PathBuf) -> bool {
        if self.folders.contains(&folder) {
            return false;
        }
        self.folders.push(folder);
        true
    }

    /// Adds files as pending tracks and returns the ones that were not in
    /// the library yet, which are the ones to analyze.
    pub(crate) fn add_files(&mut self, files: Vec<PathBuf>) -> Vec<PathBuf> {
        let mut added = Vec::new();
        for file in files {
            if self.index_by_path.contains_key(&file) {
                continue;
            }
            self.index_by_path.insert(file.clone(), self.tracks.len());
            self.tracks.push(Track::new(file.clone()));
            added.push(file);
        }
        added
    }

    /// Stores the result of analyzing `path`. Returns false when the file is
    /// not in the library.
    pub(crate) fn record_outcome(
        &mut self,
        path: &Path,
        outcome: Result<TrackAnalysis, String>,
    ) -> bool {
        let Some(track) = self
            .index_by_path
            .get(path)
            .and_then(|index| self.tracks.get_mut(*index))
        else {
            return false;
        };
        if track.failure().is_some() {
            self.failed_count -= 1;
        }
        track.status = match outcome {
            Ok(analysis) => TrackStatus::Analyzed(analysis),
            Err(reason) => {
                self.failed_count += 1;
                TrackStatus::Failed(reason)
            }
        };
        true
    }

    /// How many tracks are in each state.
    pub(crate) fn counts(&self) -> LibraryCounts {
        LibraryCounts {
            total: self.tracks.len(),
            failed: self.failed_count,
        }
    }

    /// Indexes of the tracks to show, filtered and in display order.
    ///
    /// Without a sort order tracks stay in the order they were added. With
    /// one, tracks that have no value in the sorted column come last in
    /// either direction, and ties keep the order they were added in.
    pub(crate) fn visible_tracks(
        &self,
        filter: &LibraryFilter,
        query: &str,
        sort_order: Option<SortOrder>,
    ) -> Vec<usize> {
        let query_lowercase = query.trim().to_lowercase();
        let mut visible: Vec<usize> = self
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, track)| match filter {
                LibraryFilter::AllTracks => true,
                LibraryFilter::NeedsReview => track.failure().is_some(),
                LibraryFilter::Folder(folder) => track.path.starts_with(folder),
            })
            .filter(|(_, track)| track.matches(&query_lowercase))
            .map(|(index, _)| index)
            .collect();
        if let Some(sort_order) = sort_order {
            visible.sort_by(|first, second| {
                compare_tracks(&self.tracks[*first], &self.tracks[*second], sort_order)
            });
        }
        visible
    }
}

fn compare_tracks(first: &Track, second: &Track, sort_order: SortOrder) -> Ordering {
    match sort_order.column {
        SortColumn::Title => directed(
            first.file_name_lowercase.cmp(&second.file_name_lowercase),
            sort_order,
        ),
        SortColumn::Format => directed(first.format.cmp(&second.format), sort_order),
        SortColumn::Folder => directed(
            first
                .folder
                .to_lowercase()
                .cmp(&second.folder.to_lowercase()),
            sort_order,
        ),
        SortColumn::Key => compare_optional(
            first
                .key()
                .map(|key| CamelotPosition::of(&key).wheel_order()),
            second
                .key()
                .map(|key| CamelotPosition::of(&key).wheel_order()),
            sort_order,
            Ord::cmp,
        ),
        SortColumn::Bpm => compare_optional(first.bpm(), second.bpm(), sort_order, f32::total_cmp),
        SortColumn::Duration => compare_optional(
            first.duration_seconds(),
            second.duration_seconds(),
            sort_order,
            f32::total_cmp,
        ),
    }
}

fn directed(ordering: Ordering, sort_order: SortOrder) -> Ordering {
    if sort_order.descending {
        ordering.reverse()
    } else {
        ordering
    }
}

fn compare_optional<Value>(
    first: Option<Value>,
    second: Option<Value>,
    sort_order: SortOrder,
    compare: impl Fn(&Value, &Value) -> Ordering,
) -> Ordering {
    match (first, second) {
        (Some(first), Some(second)) => directed(compare(&first, &second), sort_order),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysis(key: Option<&str>, bpm: Option<f32>, duration_seconds: f32) -> TrackAnalysis {
        TrackAnalysis {
            duration_seconds,
            key: key.map(|text| text.parse().unwrap()),
            bpm,
        }
    }

    /// Six tracks: four analyzed, one failed, one still pending.
    fn sample_library() -> Library {
        let mut library = Library::default();
        library.add_files(
            [
                "/music/house/Low Tide.aiff",
                "/music/house/saffron.mp3",
                "/music/techno/Glasshouse.flac",
                "/music/techno/Undertow.wav",
                "/music/techno/broken.mp3",
                "/music/techno/waiting.m4a",
            ]
            .map(PathBuf::from)
            .to_vec(),
        );
        let outcomes = [
            (
                "/music/house/Low Tide.aiff",
                Some("A minor"),
                Some(124.0),
                408.0,
            ),
            (
                "/music/house/saffron.mp3",
                Some("E minor"),
                Some(122.0),
                432.0,
            ),
            (
                "/music/techno/Glasshouse.flac",
                Some("C major"),
                Some(124.5),
                362.0,
            ),
            ("/music/techno/Undertow.wav", None, None, 370.0),
        ];
        for (path, key, bpm, duration_seconds) in outcomes {
            library.record_outcome(Path::new(path), Ok(analysis(key, bpm, duration_seconds)));
        }
        library.record_outcome(
            Path::new("/music/techno/broken.mp3"),
            Err("cannot decode".to_string()),
        );
        library
    }

    fn names(library: &Library, indexes: &[usize]) -> Vec<String> {
        indexes
            .iter()
            .map(|index| library.tracks()[*index].file_name.clone())
            .collect()
    }

    fn visible(library: &Library, query: &str, sort_order: Option<SortOrder>) -> Vec<String> {
        names(
            library,
            &library.visible_tracks(&LibraryFilter::AllTracks, query, sort_order),
        )
    }

    fn ascending(column: SortColumn) -> Option<SortOrder> {
        Some(SortOrder {
            column,
            descending: false,
        })
    }

    fn descending(column: SortColumn) -> Option<SortOrder> {
        Some(SortOrder {
            column,
            descending: true,
        })
    }

    #[test]
    fn a_track_shows_its_file_name_format_and_folder() {
        let library = sample_library();
        let track = &library.tracks()[0];
        assert_eq!(track.file_name, "Low Tide.aiff");
        assert_eq!(track.format, "AIFF");
        assert_eq!(track.folder, "house");
    }

    #[test]
    fn adding_a_file_twice_keeps_one_track() {
        let mut library = sample_library();
        let added = library.add_files(vec![
            PathBuf::from("/music/house/saffron.mp3"),
            PathBuf::from("/music/house/new.mp3"),
        ]);
        assert_eq!(added, vec![PathBuf::from("/music/house/new.mp3")]);
        assert_eq!(library.counts().total, 7);
    }

    #[test]
    fn counts_follow_outcomes() {
        let mut library = sample_library();
        assert_eq!(
            library.counts(),
            LibraryCounts {
                total: 6,
                failed: 1
            }
        );
        library.record_outcome(
            Path::new("/music/techno/broken.mp3"),
            Ok(analysis(None, None, 1.0)),
        );
        assert_eq!(library.counts().failed, 0);
        assert!(!library.record_outcome(Path::new("/not/in/library.mp3"), Err(String::new())));
    }

    #[test]
    fn without_a_sort_order_tracks_stay_in_the_order_added() {
        let library = sample_library();
        assert_eq!(
            visible(&library, "", None),
            [
                "Low Tide.aiff",
                "saffron.mp3",
                "Glasshouse.flac",
                "Undertow.wav",
                "broken.mp3",
                "waiting.m4a"
            ]
        );
    }

    #[test]
    fn sorts_titles_without_regard_to_letter_case() {
        let library = sample_library();
        assert_eq!(
            visible(&library, "", ascending(SortColumn::Title)),
            [
                "broken.mp3",
                "Glasshouse.flac",
                "Low Tide.aiff",
                "saffron.mp3",
                "Undertow.wav",
                "waiting.m4a"
            ]
        );
    }

    #[test]
    fn sorts_keys_around_the_camelot_wheel_with_unknown_keys_last() {
        let library = sample_library();
        // A minor is 8A, C major is 8B, E minor is 9A.
        assert_eq!(
            visible(&library, "", ascending(SortColumn::Key)),
            [
                "Low Tide.aiff",
                "Glasshouse.flac",
                "saffron.mp3",
                "Undertow.wav",
                "broken.mp3",
                "waiting.m4a"
            ]
        );
        assert_eq!(
            visible(&library, "", descending(SortColumn::Key)),
            [
                "saffron.mp3",
                "Glasshouse.flac",
                "Low Tide.aiff",
                "Undertow.wav",
                "broken.mp3",
                "waiting.m4a"
            ]
        );
    }

    #[test]
    fn sorts_tempo_and_duration_numerically() {
        let library = sample_library();
        assert_eq!(
            visible(&library, "", descending(SortColumn::Bpm))[..3],
            ["Glasshouse.flac", "Low Tide.aiff", "saffron.mp3"]
        );
        assert_eq!(
            visible(&library, "", ascending(SortColumn::Duration))[..4],
            [
                "Glasshouse.flac",
                "Undertow.wav",
                "Low Tide.aiff",
                "saffron.mp3"
            ]
        );
    }

    #[test]
    fn clicking_a_header_sorts_ascending_then_reverses() {
        let first = SortOrder::after_click(None, SortColumn::Bpm);
        assert_eq!(ascending(SortColumn::Bpm), Some(first));
        let second = SortOrder::after_click(Some(first), SortColumn::Bpm);
        assert_eq!(descending(SortColumn::Bpm), Some(second));
        let other_column = SortOrder::after_click(Some(second), SortColumn::Key);
        assert_eq!(ascending(SortColumn::Key), Some(other_column));
    }

    #[test]
    fn search_matches_part_of_the_file_name_in_any_case() {
        let library = sample_library();
        assert_eq!(visible(&library, "TIDE", None), ["Low Tide.aiff"]);
        assert_eq!(visible(&library, "  low   tide ", None), ["Low Tide.aiff"]);
        assert_eq!(
            visible(&library, ".mp3", None),
            ["saffron.mp3", "broken.mp3"]
        );
    }

    #[test]
    fn search_matches_a_whole_key_code() {
        let library = sample_library();
        assert_eq!(visible(&library, "8a", None), ["Low Tide.aiff"]);
        assert_eq!(visible(&library, "8B", None), ["Glasshouse.flac"]);
        assert!(visible(&library, "8", None).is_empty());
    }

    #[test]
    fn search_matches_the_start_of_the_tempo() {
        let library = sample_library();
        assert_eq!(
            visible(&library, "124", None),
            ["Low Tide.aiff", "Glasshouse.flac"]
        );
        assert_eq!(visible(&library, "124.5", None), ["Glasshouse.flac"]);
        assert_eq!(visible(&library, "122 saff", None), ["saffron.mp3"]);
    }

    #[test]
    fn filters_by_folder_and_by_failure() {
        let mut library = sample_library();
        library.add_folder(PathBuf::from("/music/house"));
        assert!(!library.add_folder(PathBuf::from("/music/house")));
        let in_folder = library.visible_tracks(
            &LibraryFilter::Folder(PathBuf::from("/music/house")),
            "",
            None,
        );
        assert_eq!(
            names(&library, &in_folder),
            ["Low Tide.aiff", "saffron.mp3"]
        );
        let needing_review = library.visible_tracks(&LibraryFilter::NeedsReview, "", None);
        assert_eq!(names(&library, &needing_review), ["broken.mp3"]);
    }

    #[test]
    fn ten_thousand_tracks_filter_and_sort_quickly() {
        let mut library = Library::default();
        library.add_files(
            (0..10_000)
                .map(|number| PathBuf::from(format!("/music/set/track {number:05}.flac")))
                .collect(),
        );
        for number in 0..10_000_u32 {
            let key = Key::new((number % 12) as u8, euphoria_core::Mode::Minor);
            library.record_outcome(
                Path::new(&format!("/music/set/track {number:05}.flac")),
                Ok(TrackAnalysis {
                    duration_seconds: 200.0 + (number % 240) as f32,
                    key: Some(key),
                    bpm: Some(90.0 + (number % 80) as f32),
                }),
            );
        }
        let started = std::time::Instant::now();
        let shown = library.visible_tracks(
            &LibraryFilter::AllTracks,
            "track",
            descending(SortColumn::Key),
        );
        assert_eq!(shown.len(), 10_000);
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
    }
}
