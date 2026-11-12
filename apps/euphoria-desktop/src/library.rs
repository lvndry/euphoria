// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tracks the user has added and the order and subset shown in the table.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use euphoria_core::quality::{QualityReport, QualityVerdict};
use euphoria_core::{
    BeatGrid, Confidence, Correction, Cue, Key, TempoRange, TrackAnalysis, TrackCues,
};

use crate::camelot::CamelotPosition;
use crate::folder_tree::FolderColumn;
use crate::format;

/// The dash shown where analysis found no value.
pub(crate) const NOT_FOUND: &str = "\u{2013}";

/// Where a track is in its analysis.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum TrackStatus {
    /// Waiting for a free worker.
    Queued,
    /// A worker is analyzing the file now.
    Analyzing,
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
    /// The file name, shown as the title when the tags have none.
    pub(crate) file_name: String,
    /// The title in the file's tags, once read.
    pub(crate) tag_title: Option<String>,
    /// The artist in the file's tags, once read.
    pub(crate) artist: Option<String>,
    /// The file extension in upper case, for example "FLAC".
    pub(crate) format: String,
    /// The name of the folder holding the file.
    pub(crate) folder: String,
    /// Where the track is in its analysis.
    pub(crate) status: TrackStatus,
    /// The key and tempo the user set, which win over the detected ones.
    pub(crate) correction: Correction,
    /// The cues the user set, which replace those analysis placed, even
    /// when empty. `None` keeps the automatic cues.
    pub(crate) edited_cues: Option<Vec<Cue>>,

    /// The beat grid as analysis fitted it, the source every tempo range is
    /// applied to. Projecting from the stored grid each time, instead of the
    /// grid another range already reported, keeps changing the range back
    /// and forth from moving the first downbeat.
    canonical_beat_grid: Option<BeatGrid>,
    file_name_lowercase: String,
    title_lowercase: String,
    artist_lowercase: Option<String>,
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
            title_lowercase: file_name.to_lowercase(),
            artist_lowercase: None,
            path,
            tag_title: None,
            artist: None,
            file_name,
            format,
            folder,
            status: TrackStatus::Queued,
            correction: Correction::default(),
            edited_cues: None,
            canonical_beat_grid: None,
        }
    }

    /// The title to show: the one in the tags, or the file name.
    pub(crate) fn title(&self) -> &str {
        self.tag_title.as_deref().unwrap_or(&self.file_name)
    }

    /// Takes the title and artist from the file's tags.
    fn set_names(&mut self, title: Option<String>, artist: Option<String>) {
        self.title_lowercase = title.as_deref().unwrap_or(&self.file_name).to_lowercase();
        self.artist_lowercase = artist.as_deref().map(str::to_lowercase);
        self.tag_title = title;
        self.artist = artist;
    }

    /// True while the track has no result yet: queued or being analyzed.
    pub(crate) fn is_unfinished(&self) -> bool {
        matches!(self.status, TrackStatus::Queued | TrackStatus::Analyzing)
    }

    /// The analysis, when the track has one.
    pub(crate) fn analysis(&self) -> Option<&TrackAnalysis> {
        match &self.status {
            TrackStatus::Analyzed(analysis) => Some(analysis),
            TrackStatus::Queued | TrackStatus::Analyzing | TrackStatus::Failed(_) => None,
        }
    }

    /// The key of the track: the one the user set, or the detected one.
    pub(crate) fn key(&self) -> Option<Key> {
        self.correction.key.or_else(|| self.detected_key())
    }

    /// The tempo of the track: the one the user set, or the detected one in
    /// the tempo range on show.
    pub(crate) fn bpm(&self) -> Option<f32> {
        self.correction.bpm.or_else(|| self.detected_bpm())
    }

    /// The key analysis found, whatever the user set.
    pub(crate) fn detected_key(&self) -> Option<Key> {
        self.analysis().and_then(|analysis| analysis.key)
    }

    /// The tempo analysis found, whatever the user set.
    pub(crate) fn detected_bpm(&self) -> Option<f32> {
        self.analysis().and_then(|analysis| analysis.bpm)
    }

    /// Where the beats and bars fall, while the tempo is the detected one:
    /// the grid was fitted to that tempo, and a tempo the user set has none.
    pub(crate) fn beat_grid(&self) -> Option<BeatGrid> {
        self.analysis()
            .and_then(|analysis| analysis.beat_grid)
            .filter(|_| self.correction.bpm.is_none())
    }

    /// The cues of the track: the user's set when there is one, and
    /// otherwise those analysis placed on the grid of the detected tempo.
    /// A user's cue past the end of the analyzed track is left out, the
    /// same rule as [`TrackCues::of`]; until the track has an analysis
    /// every cue of the set is shown, to be dropped once the length is
    /// known.
    pub(crate) fn cues(&self) -> TrackCues {
        let duration = self
            .analysis()
            .map(|analysis| f64::from(analysis.duration_seconds));
        match (&self.edited_cues, self.analysis()) {
            (Some(edited), _) => TrackCues::within(edited.clone(), duration.unwrap_or(f64::NAN)),
            (None, Some(analysis)) if self.correction.bpm.is_none() => {
                TrackCues::automatic(&analysis.cues)
            }
            (None, _) => TrackCues::automatic(&[]),
        }
    }

    /// The length in seconds, once the track is analyzed.
    pub(crate) fn duration_seconds(&self) -> Option<f32> {
        self.analysis().map(|analysis| analysis.duration_seconds)
    }

    /// The energy rating from 1 to 10, when the track has one.
    pub(crate) fn energy(&self) -> Option<u8> {
        self.analysis().and_then(|analysis| analysis.energy)
    }

    /// How the file is encoded and how good it sounds, once it is analyzed.
    pub(crate) fn quality(&self) -> Option<&QualityReport> {
        self.analysis()
            .and_then(|analysis| analysis.quality.as_ref())
    }

    /// Whether the quality verdict is one a DJ may want to act on: a low bit
    /// rate, or a file that was likely made from a lower quality one.
    pub(crate) fn has_quality_to_check(&self) -> bool {
        self.quality().is_some_and(|quality| {
            matches!(
                quality.verdict,
                QualityVerdict::WeakOnABigSystem | QualityVerdict::LikelyReencoded
            )
        })
    }

    /// What a cell shows for a value analysis may not find: the value, a
    /// dash when the track was analyzed and has none, and nothing while the
    /// track is waiting, being analyzed or failed.
    pub(crate) fn value_or_dash(&self, value: Option<String>) -> Option<String> {
        value.or_else(|| self.analysis().map(|_| NOT_FOUND.to_string()))
    }

    /// Whether the track is worth checking: its analysis failed, or the
    /// core rates its key or its tempo low.
    pub(crate) fn needs_review(&self) -> bool {
        match &self.status {
            TrackStatus::Failed(_) => true,
            TrackStatus::Analyzed(analysis) => {
                let doubtful_key = self.correction.key.is_none()
                    && analysis.key_confidence() == Some(Confidence::Low);
                let doubtful_tempo = self.correction.bpm.is_none()
                    && analysis.tempo_confidence() == Some(Confidence::Low);
                doubtful_key || doubtful_tempo
            }
            TrackStatus::Queued | TrackStatus::Analyzing => false,
        }
    }

    /// Why analysis failed, when it did.
    #[cfg(test)]
    pub(crate) fn failure(&self) -> Option<&str> {
        match &self.status {
            TrackStatus::Failed(reason) => Some(reason),
            TrackStatus::Queued | TrackStatus::Analyzing | TrackStatus::Analyzed(_) => None,
        }
    }

    /// True when every word of `query_lowercase` matches the track: as part
    /// of the file name, the title or the artist, as the whole Camelot code,
    /// or as the start of the tempo.
    fn matches(&self, query_lowercase: &str) -> bool {
        query_lowercase.split_whitespace().all(|word| {
            self.file_name_lowercase.contains(word)
                || self.title_lowercase.contains(word)
                || self
                    .artist_lowercase
                    .as_deref()
                    .is_some_and(|artist| artist.contains(word))
                || self.key().is_some_and(|key| {
                    [key.camelot(), key.open_key(), key.short_name()]
                        .iter()
                        .any(|code| code.eq_ignore_ascii_case(word))
                })
                || self
                    .bpm()
                    .is_some_and(|bpm| format::bpm(bpm).starts_with(word))
        })
    }
}

/// The column the table is sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortColumn {
    /// Title, or file name where the tags have none, ignoring letter case.
    Title,
    /// Artist, ignoring letter case.
    Artist,
    /// Position around the Camelot wheel: number, then minor before major.
    Key,
    /// Tempo.
    Bpm,
    /// Length.
    Duration,
    /// Energy rating.
    Energy,
    /// Quality verdict, from likely re-encoded up to lossless.
    Quality,
    /// File format.
    Format,
    /// Path of the containing folder, as the Folder column shows it.
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

/// Which tracks the table shows, within the folder on show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LibraryFilter {
    /// Every track.
    #[default]
    AllTracks,
    /// Tracks whose key or tempo is rated low, and tracks whose analysis
    /// failed.
    NeedsReview,
    /// Tracks with a low bit rate, or likely made from a lower quality file.
    QualityToCheck,
}

/// How many tracks are in each state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct LibraryCounts {
    /// Every track.
    pub(crate) total: usize,
    /// Tracks that need review: a key or tempo rated low, or a failure.
    pub(crate) needs_review: usize,
    /// Tracks with a low bit rate, or likely made from a lower quality file.
    pub(crate) quality_to_check: usize,
}

/// Every track the user has added, in the order they were added.
#[derive(Debug, Default)]
pub(crate) struct Library {
    tracks: Vec<Track>,
    index_by_path: HashMap<PathBuf, usize>,
    folders: Vec<PathBuf>,
    review_count: usize,
    /// The range tempos are shown in.
    tempo_range: TempoRange,
    revision: u64,
    structure_revision: u64,
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

    /// The track of the file at `path`.
    pub(crate) fn track_by_path(&self, path: &Path) -> Option<&Track> {
        self.index_by_path
            .get(path)
            .and_then(|index| self.tracks.get(*index))
    }

    /// The index of the track for the file at `path`.
    pub(crate) fn index_of(&self, path: &Path) -> Option<usize> {
        self.index_by_path.get(path).copied()
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
        self.revision += 1;
        self.structure_revision += 1;
        true
    }

    /// Adds files as queued tracks and returns the ones that were not in
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
        if !added.is_empty() {
            self.revision += 1;
            self.structure_revision += 1;
        }
        added
    }

    /// Marks a queued track as being analyzed now. Returns false when the
    /// file is not in the library or is no longer queued.
    pub(crate) fn mark_analyzing(&mut self, path: &Path) -> bool {
        let Some(track) = self
            .index_by_path
            .get(path)
            .and_then(|index| self.tracks.get_mut(*index))
        else {
            return false;
        };
        if track.status != TrackStatus::Queued {
            return false;
        }
        track.status = TrackStatus::Analyzing;
        true
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
        if track.needs_review() {
            self.review_count -= 1;
        }
        track.status = match outcome {
            Ok(analysis) => {
                track.canonical_beat_grid = analysis.beat_grid;
                TrackStatus::Analyzed(analysis.with_tempo_range(self.tempo_range))
            }
            Err(reason) => {
                track.canonical_beat_grid = None;
                TrackStatus::Failed(reason)
            }
        };
        if track.needs_review() {
            self.review_count += 1;
        }
        self.revision += 1;
        true
    }

    /// Sets the key and tempo the user chose for the track at `path`; an
    /// empty correction goes back to the detected values. Returns false when
    /// the file is not in the library.
    pub(crate) fn set_correction(&mut self, path: &Path, correction: Correction) -> bool {
        let Some(track) = self
            .index_by_path
            .get(path)
            .and_then(|index| self.tracks.get_mut(*index))
        else {
            return false;
        };
        if track.needs_review() {
            self.review_count -= 1;
        }
        track.correction = correction;
        if track.needs_review() {
            self.review_count += 1;
        }
        self.revision += 1;
        true
    }

    /// Sets the cues the user chose for the track at `path`; `None` goes
    /// back to the cues analysis placed. Returns false when the file is not
    /// in the library.
    pub(crate) fn set_edited_cues(&mut self, path: &Path, cues: Option<Vec<Cue>>) -> bool {
        let Some(track) = self
            .index_by_path
            .get(path)
            .and_then(|index| self.tracks.get_mut(*index))
        else {
            return false;
        };
        track.edited_cues = cues.map(|cues| TrackCues::edited(cues).cues);
        self.revision += 1;
        true
    }

    /// The range tempos are shown in.
    pub(crate) fn tempo_range(&self) -> TempoRange {
        self.tempo_range
    }

    /// Shows every tempo in `range`. Nothing is analyzed again: each tempo
    /// is placed in the range from the estimate the analysis kept.
    pub(crate) fn set_tempo_range(&mut self, range: TempoRange) {
        if self.tempo_range == range {
            return;
        }
        self.tempo_range = range;
        for track in &mut self.tracks {
            if let TrackStatus::Analyzed(analysis) = &mut track.status {
                // The same as `with_tempo_range`, in place, so the rest of
                // the analysis, such as its key timeline, is not copied.
                analysis.set_tempo_range_from(track.canonical_beat_grid, range);
            }
        }
        self.revision += 1;
    }

    /// A number that changes whenever a folder or a track is added or a
    /// track's result comes in.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// A number that changes only when a folder or a track is added, not
    /// when a result comes in.
    pub(crate) fn structure_revision(&self) -> u64 {
        self.structure_revision
    }

    /// Takes the title and artist of the track at `path` from its tags.
    /// Returns false when the file is not in the library.
    pub(crate) fn set_names(
        &mut self,
        path: &Path,
        title: Option<String>,
        artist: Option<String>,
    ) -> bool {
        let Some(track) = self
            .index_by_path
            .get(path)
            .and_then(|index| self.tracks.get_mut(*index))
        else {
            return false;
        };
        track.set_names(title, artist);
        true
    }

    /// How many tracks are in each state.
    pub(crate) fn counts(&self) -> LibraryCounts {
        LibraryCounts {
            total: self.tracks.len(),
            needs_review: self.review_count,
            quality_to_check: self.quality_to_check_in(None),
        }
    }

    /// How many tracks inside `folder`, or in the whole library without one,
    /// have a quality to check.
    pub(crate) fn quality_to_check_in(&self, folder: Option<&Path>) -> usize {
        self.tracks
            .iter()
            .filter(|track| folder.is_none_or(|folder| track.path.starts_with(folder)))
            .filter(|track| track.has_quality_to_check())
            .count()
    }

    /// Indexes of the tracks to show, filtered and in display order.
    ///
    /// Without a sort order tracks stay in the order they were added. With
    /// one, tracks that have no value in the sorted column come last in
    /// either direction, and ties keep the order they were added in.
    /// Only tracks inside `folder` are shown when it is given, and the
    /// Folder column sorts by `folder_column`.
    pub(crate) fn visible_tracks(
        &self,
        filter: LibraryFilter,
        folder: Option<&Path>,
        query: &str,
        sort_order: Option<SortOrder>,
        folder_column: &FolderColumn,
    ) -> Vec<usize> {
        let query_lowercase = query.trim().to_lowercase();
        let mut visible: Vec<usize> = self
            .tracks
            .iter()
            .enumerate()
            .filter(|(_, track)| match filter {
                LibraryFilter::AllTracks => true,
                LibraryFilter::NeedsReview => track.needs_review(),
                LibraryFilter::QualityToCheck => track.has_quality_to_check(),
            })
            .filter(|(_, track)| folder.is_none_or(|folder| track.path.starts_with(folder)))
            .filter(|(_, track)| track.matches(&query_lowercase))
            .map(|(index, _)| index)
            .collect();
        if let Some(sort_order) = sort_order {
            visible.sort_by(|first, second| match sort_order.column {
                SortColumn::Folder => directed(folder_column.compare(*first, *second), sort_order),
                _ => compare_tracks(&self.tracks[*first], &self.tracks[*second], sort_order),
            });
        }
        visible
    }
}

impl Library {
    /// The rows of a playlist holding `paths` in order: the tracks of the
    /// library among them that match the search, in the playlist's order or
    /// sorted by `sort_order`. Files the library does not hold are left out.
    pub(crate) fn playlist_rows(
        &self,
        paths: &[PathBuf],
        query: &str,
        sort_order: Option<SortOrder>,
        folder_column: &FolderColumn,
    ) -> Vec<usize> {
        let query_lowercase = query.trim().to_lowercase();
        let mut rows: Vec<usize> = paths
            .iter()
            .filter_map(|path| self.index_of(path))
            .filter(|index| self.tracks[*index].matches(&query_lowercase))
            .collect();
        if let Some(sort_order) = sort_order {
            rows.sort_by(|first, second| match sort_order.column {
                SortColumn::Folder => directed(folder_column.compare(*first, *second), sort_order),
                _ => compare_tracks(&self.tracks[*first], &self.tracks[*second], sort_order),
            });
        }
        rows
    }
}

fn compare_tracks(first: &Track, second: &Track, sort_order: SortOrder) -> Ordering {
    match sort_order.column {
        SortColumn::Title => directed(
            first.title_lowercase.cmp(&second.title_lowercase),
            sort_order,
        ),
        SortColumn::Artist => compare_optional(
            first.artist_lowercase.as_ref(),
            second.artist_lowercase.as_ref(),
            sort_order,
            Ord::cmp,
        ),
        SortColumn::Format => directed(first.format.cmp(&second.format), sort_order),
        SortColumn::Folder => Ordering::Equal,
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
        SortColumn::Energy => {
            compare_optional(first.energy(), second.energy(), sort_order, Ord::cmp)
        }
        SortColumn::Quality => compare_optional(
            first.quality().map(|report| quality_rank(report.verdict)),
            second.quality().map(|report| quality_rank(report.verdict)),
            sort_order,
            Ord::cmp,
        ),
        SortColumn::Duration => compare_optional(
            first.duration_seconds(),
            second.duration_seconds(),
            sort_order,
            f32::total_cmp,
        ),
    }
}

/// Where a verdict sits from the most to the least worth checking.
fn quality_rank(verdict: QualityVerdict) -> u8 {
    match verdict {
        QualityVerdict::LikelyReencoded => 0,
        QualityVerdict::WeakOnABigSystem => 1,
        QualityVerdict::Acceptable => 2,
        QualityVerdict::Good => 3,
        QualityVerdict::Lossless => 4,
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
            &library.visible_tracks(
                LibraryFilter::AllTracks,
                None,
                query,
                sort_order,
                &FolderColumn::new(library.tracks(), library.folders(), None),
            ),
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
    fn the_title_comes_from_the_tags_with_the_file_name_as_fallback() {
        let mut library = sample_library();
        assert_eq!(library.tracks()[0].title(), "Low Tide.aiff");
        assert!(library.set_names(
            Path::new("/music/house/Low Tide.aiff"),
            Some("Low Tide (Extended Mix)".to_string()),
            Some("Marine Layer".to_string()),
        ));
        assert!(!library.set_names(Path::new("/nowhere.mp3"), None, None));
        let track = &library.tracks()[0];
        assert_eq!(track.title(), "Low Tide (Extended Mix)");
        assert_eq!(track.artist.as_deref(), Some("Marine Layer"));
        assert_eq!(track.file_name, "Low Tide.aiff");
    }

    #[test]
    fn search_finds_the_title_and_the_artist_as_well_as_the_file_name() {
        let mut library = sample_library();
        library.set_names(
            Path::new("/music/techno/Undertow.wav"),
            Some("Deep Water".to_string()),
            Some("Kaito Sato".to_string()),
        );
        assert_eq!(visible(&library, "kaito", None), ["Undertow.wav"]);
        assert_eq!(visible(&library, "deep water", None), ["Undertow.wav"]);
        assert_eq!(visible(&library, "undertow", None), ["Undertow.wav"]);
    }

    #[test]
    fn sorting_by_title_uses_the_tags_and_by_artist_puts_tracks_without_one_last() {
        let mut library = sample_library();
        library.set_names(
            Path::new("/music/techno/waiting.m4a"),
            Some("Aaa first by title".to_string()),
            Some("Zed".to_string()),
        );
        library.set_names(
            Path::new("/music/house/saffron.mp3"),
            None,
            Some("anna".to_string()),
        );
        assert_eq!(
            visible(&library, "", ascending(SortColumn::Title))[0],
            "waiting.m4a"
        );
        let by_artist = visible(&library, "", ascending(SortColumn::Artist));
        assert_eq!(by_artist[..2], ["saffron.mp3", "waiting.m4a"]);
        let by_artist_descending = visible(&library, "", descending(SortColumn::Artist));
        assert_eq!(by_artist_descending[..2], ["waiting.m4a", "saffron.mp3"]);
        assert_eq!(by_artist_descending.len(), 6);
    }

    /// An analysis rated by the core's confidence rules: a key margin of
    /// 0.02 is high and 0.001 low; a tempo margin of 0.6 is high and 0.01 low.
    fn rated(key_margin: f32, tempo_estimate: f32, tempo_margin: f32) -> TrackAnalysis {
        TrackAnalysis {
            key_runner_up: Some("E minor".parse().unwrap()),
            key_score_margin: Some(key_margin),
            tempo_estimate: Some(tempo_estimate),
            tempo_score_margin: Some(tempo_margin),
            ..analysis(Some("A minor"), Some(tempo_estimate), 300.0)
        }
    }

    #[test]
    fn a_correction_wins_over_the_analysis_and_resets_to_it() {
        let mut library = Library::default();
        let path = Path::new("/music/track.mp3");
        library.add_files(vec![path.to_path_buf()]);
        library.record_outcome(path, Ok(rated(0.02, 124.0, 0.6)));
        let e_minor: Key = "E minor".parse().unwrap();
        let revision = library.revision();

        let correction = Correction::of_key(e_minor).with_bpm(Some(62.0)).unwrap();
        assert!(library.set_correction(path, correction));
        let track = &library.tracks()[0];
        assert_eq!(track.key(), Some(e_minor));
        assert_eq!(track.bpm(), Some(62.0));
        assert_eq!(track.detected_key(), Some("A minor".parse().unwrap()));
        assert_eq!(track.detected_bpm(), Some(124.0));
        assert!(library.revision() > revision);
        let folder_column = FolderColumn::new(library.tracks(), library.folders(), None);
        let found =
            library.visible_tracks(LibraryFilter::AllTracks, None, "9a", None, &folder_column);
        assert_eq!(found.len(), 1, "search finds the key the user set");

        library.set_tempo_range(TempoRange::new(70.0, 140.0).unwrap());
        assert_eq!(
            library.tracks()[0].bpm(),
            Some(62.0),
            "a tempo set by hand is not folded"
        );

        assert!(library.set_correction(path, Correction::default()));
        let track = &library.tracks()[0];
        assert_eq!(track.key(), Some("A minor".parse().unwrap()));
        assert_eq!(track.bpm(), Some(124.0));
        assert!(!library.set_correction(Path::new("/music/other.mp3"), correction));
    }

    #[test]
    fn a_corrected_value_leaves_needs_review_and_the_other_value_still_counts() {
        let mut library = Library::default();
        let both = Path::new("/music/both doubtful.mp3");
        let failed = Path::new("/music/broken.mp3");
        library.add_files(vec![both.to_path_buf(), failed.to_path_buf()]);
        library.record_outcome(both, Ok(rated(0.001, 124.0, 0.01)));
        library.record_outcome(failed, Err("cannot decode".into()));
        assert_eq!(library.counts().needs_review, 2);

        let key_only = Correction::of_key("E minor".parse().unwrap());
        library.set_correction(both, key_only);
        assert_eq!(
            library.counts().needs_review,
            2,
            "the tempo is still doubtful"
        );
        library.set_correction(both, key_only.with_bpm(Some(124.0)).unwrap());
        assert_eq!(library.counts().needs_review, 1);
        library.set_correction(failed, key_only);
        assert_eq!(
            library.counts().needs_review,
            1,
            "a failed file still needs a look"
        );
        library.set_correction(both, Correction::default());
        assert_eq!(library.counts().needs_review, 2);
    }

    #[test]
    fn a_correction_given_before_the_analysis_counts_once_it_arrives() {
        let mut library = Library::default();
        let path = Path::new("/music/early.mp3");
        library.add_files(vec![path.to_path_buf()]);
        let correction = Correction::of_key("E minor".parse().unwrap())
            .with_bpm(Some(124.0))
            .unwrap();
        library.set_correction(path, correction);
        assert_eq!(library.tracks()[0].key(), Some("E minor".parse().unwrap()));
        library.record_outcome(path, Ok(rated(0.001, 124.0, 0.01)));
        assert_eq!(library.counts().needs_review, 0);
        assert_eq!(library.tracks()[0].key(), Some("E minor".parse().unwrap()));
    }

    #[test]
    fn needs_review_lists_low_keys_low_tempos_and_failed_files() {
        let mut library = Library::default();
        library.add_files(
            [
                "/music/sure.mp3",
                "/music/doubtful key.mp3",
                "/music/doubtful tempo.mp3",
                "/music/broken.mp3",
                "/music/waiting.mp3",
            ]
            .map(PathBuf::from)
            .to_vec(),
        );
        library.record_outcome(Path::new("/music/sure.mp3"), Ok(rated(0.02, 124.0, 0.6)));
        library.record_outcome(
            Path::new("/music/doubtful key.mp3"),
            Ok(rated(0.001, 124.0, 0.6)),
        );
        library.record_outcome(
            Path::new("/music/doubtful tempo.mp3"),
            Ok(rated(0.02, 124.0, 0.01)),
        );
        library.record_outcome(Path::new("/music/broken.mp3"), Err("cannot decode".into()));
        let needing_review = library.visible_tracks(
            LibraryFilter::NeedsReview,
            None,
            "",
            None,
            &FolderColumn::default(),
        );
        assert_eq!(
            names(&library, &needing_review),
            ["doubtful key.mp3", "doubtful tempo.mp3", "broken.mp3"]
        );
        assert_eq!(library.counts().needs_review, 3);

        library.record_outcome(
            Path::new("/music/doubtful key.mp3"),
            Ok(rated(0.02, 124.0, 0.6)),
        );
        assert_eq!(
            library.counts().needs_review,
            2,
            "a result that came in again"
        );
    }

    #[test]
    fn a_tempo_range_moves_every_tempo_without_analyzing_again() {
        let mut library = Library::default();
        library.add_files(
            ["/music/half time.mp3", "/music/house.mp3"]
                .map(PathBuf::from)
                .to_vec(),
        );
        library.record_outcome(
            Path::new("/music/half time.mp3"),
            Ok(rated(0.02, 87.0, 0.6)),
        );
        library.record_outcome(Path::new("/music/house.mp3"), Ok(rated(0.02, 124.0, 0.6)));
        let tempos = |library: &Library| -> Vec<Option<f32>> {
            library.tracks().iter().map(Track::bpm).collect()
        };
        assert_eq!(tempos(&library), [Some(174.0), Some(124.0)]);

        let hip_hop = TempoRange::new(70.0, 140.0).unwrap();
        let revision = library.revision();
        library.set_tempo_range(hip_hop);
        assert_eq!(library.tempo_range(), hip_hop);
        assert_eq!(tempos(&library), [Some(87.0), Some(124.0)]);
        assert!(library.revision() > revision);

        library.add_files(vec![PathBuf::from("/music/late.mp3")]);
        library.record_outcome(Path::new("/music/late.mp3"), Ok(rated(0.02, 150.0, 0.6)));
        assert_eq!(
            library.tracks()[2].bpm(),
            Some(75.0),
            "a result that comes in later"
        );
    }

    #[test]
    fn a_range_set_in_place_gives_the_tempos_a_new_analysis_would() {
        let mut library = Library::default();
        let paths = [
            "/music/a.mp3",
            "/music/b.mp3",
            "/music/c.mp3",
            "/music/d.mp3",
        ];
        library.add_files(paths.map(PathBuf::from).to_vec());
        let estimates = [87.0, 124.0, 150.0, 61.5];
        for (path, estimate) in paths.iter().zip(estimates) {
            let mut analysis = rated(0.02, estimate, 0.6);
            analysis.beat_grid = Some(BeatGrid {
                first_downbeat_seconds: 0.125,
                bpm: f64::from(estimate),
                salience: 5.0,
                drift_seconds: None,
            });
            library.record_outcome(Path::new(path), Ok(analysis));
        }
        let mut no_estimate = analysis(Some("A minor"), Some(95.0), 300.0);
        no_estimate.key_timeline = vec![euphoria_core::KeySection {
            start_seconds: 0.0,
            end_seconds: 300.0,
            key: "A minor".parse().unwrap(),
        }];
        library.add_files(vec![PathBuf::from("/music/e.mp3")]);
        library.record_outcome(Path::new("/music/e.mp3"), Ok(no_estimate.clone()));

        for range in [
            TempoRange::new(70.0, 140.0).unwrap(),
            TempoRange::new(120.0, 200.0).unwrap(),
            TempoRange::DEFAULT,
        ] {
            library.set_tempo_range(range);
            for track in library.tracks() {
                let analysis = track.analysis().unwrap();
                let expected = analysis.clone().with_tempo_range(range);
                assert_eq!(analysis.bpm, expected.bpm, "{range}");
                assert_eq!(analysis.beat_grid, expected.beat_grid, "{range}");
            }
        }
        let kept = library.tracks()[4].analysis().unwrap();
        assert_eq!(kept.key_timeline, no_estimate.key_timeline);
    }

    #[test]
    fn a_range_set_and_set_back_restores_the_downbeat_analysis_fitted() {
        let mut library = Library::default();
        let path = Path::new("/music/a.mp3");
        library.add_files(vec![PathBuf::from(path)]);
        let mut analysis = rated(0.02, 87.0, 0.6);
        analysis.beat_grid = Some(BeatGrid {
            first_downbeat_seconds: 2.0,
            bpm: 87.0,
            salience: 5.0,
            drift_seconds: None,
        });
        library.record_outcome(path, Ok(analysis));

        // 2.0 seconds is beyond the 1.379-second bar of the 174 BPM grid
        // this range reports, so its projection wraps the downbeat.
        library.set_tempo_range(TempoRange::new(120.0, 240.0).unwrap());
        let grid = library.tracks()[0].analysis().unwrap().beat_grid.unwrap();
        assert_eq!(grid.bpm, 174.0, "the reported tempo's octave");
        assert!((grid.first_downbeat_seconds - 2.0 % (4.0 * 60.0 / 174.0)).abs() < 1e-3);

        // Going back to the range the analysis fitted in restores the fitted
        // grid; projecting the wrapped one instead would keep it at 0.62.
        library.set_tempo_range(TempoRange::new(60.0, 120.0).unwrap());
        let restored = library.tracks()[0].analysis().unwrap();
        assert_eq!(restored.beat_grid.unwrap().bpm, 87.0);
        assert!((restored.beat_grid.unwrap().first_downbeat_seconds - 2.0).abs() < 1e-3);
    }
    #[test]
    fn search_finds_a_key_in_every_notation() {
        let mut library = sample_library();
        library.record_outcome(
            Path::new("/music/techno/waiting.m4a"),
            Ok(analysis(Some("C major"), Some(120.0), 300.0)),
        );
        for code in ["8a", "1m", "am"] {
            assert_eq!(visible(&library, code, None), ["Low Tide.aiff"], "{code}");
        }
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
    fn a_track_is_queued_then_analyzing_then_analyzed() {
        let mut library = Library::default();
        let path = PathBuf::from("/music/next.mp3");
        library.add_files(vec![path.clone()]);
        assert_eq!(library.tracks()[0].status, TrackStatus::Queued);
        assert!(library.tracks()[0].is_unfinished());

        assert!(library.mark_analyzing(&path));
        assert_eq!(library.tracks()[0].status, TrackStatus::Analyzing);
        assert!(library.tracks()[0].is_unfinished());

        library.record_outcome(&path, Ok(analysis(None, None, 300.0)));
        assert!(!library.tracks()[0].is_unfinished());
        assert!(
            !library.mark_analyzing(&path),
            "a finished track stays finished"
        );
        assert!(library.tracks()[0].analysis().is_some());
        assert!(!library.mark_analyzing(Path::new("/music/unknown.mp3")));
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

    fn with_quality(verdict: QualityVerdict) -> TrackAnalysis {
        TrackAnalysis {
            quality: Some(QualityReport {
                codec: "aac".to_string(),
                lossless: false,
                bitrate_kbps: Some(128),
                sample_rate: 44_100,
                bit_depth: None,
                channels: Some(2),
                cutoff_hz: Some(16_000.0),
                clipped_share: 0.0,
                integrated_loudness_lufs: None,
                true_peak_dbtp: None,
                verdict,
            }),
            ..analysis(Some("A minor"), Some(124.0), 300.0)
        }
    }

    #[test]
    fn the_quality_filter_shows_weak_and_likely_re_encoded_tracks() {
        let mut library = sample_library();
        library.record_outcome(
            Path::new("/music/house/Low Tide.aiff"),
            Ok(with_quality(QualityVerdict::WeakOnABigSystem)),
        );
        library.record_outcome(
            Path::new("/music/house/saffron.mp3"),
            Ok(with_quality(QualityVerdict::Good)),
        );
        library.record_outcome(
            Path::new("/music/techno/Glasshouse.flac"),
            Ok(with_quality(QualityVerdict::LikelyReencoded)),
        );
        assert_eq!(library.counts().quality_to_check, 2);
        let shown = library.visible_tracks(
            LibraryFilter::QualityToCheck,
            None,
            "",
            None,
            &FolderColumn::default(),
        );
        assert_eq!(
            library.quality_to_check_in(Some(Path::new("/music/techno"))),
            1
        );
        assert_eq!(
            names(&library, &shown),
            ["Low Tide.aiff", "Glasshouse.flac"]
        );
        assert!(library.tracks()[0].quality().is_some());
        assert!(library.tracks()[4].quality().is_none());

        let by_quality = visible(&library, "", ascending(SortColumn::Quality));
        assert_eq!(
            &by_quality[..3],
            ["Glasshouse.flac", "Low Tide.aiff", "saffron.mp3"]
        );
        let reversed = visible(&library, "", descending(SortColumn::Quality));
        assert_eq!(
            &reversed[..3],
            ["saffron.mp3", "Low Tide.aiff", "Glasshouse.flac"]
        );
    }

    #[test]
    fn counts_follow_outcomes() {
        let mut library = sample_library();
        assert_eq!(
            library.counts(),
            LibraryCounts {
                total: 6,
                needs_review: 1,
                quality_to_check: 0,
            }
        );
        library.record_outcome(
            Path::new("/music/techno/broken.mp3"),
            Ok(analysis(None, None, 1.0)),
        );
        assert_eq!(library.counts().needs_review, 0);
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
    fn sorts_energy_numerically_with_unrated_tracks_last() {
        let mut library = Library::default();
        let rated = [
            ("calm.mp3", Some(2)),
            ("peak.mp3", Some(10)),
            ("silent.mp3", None),
            ("warm.mp3", Some(6)),
        ];
        for (name, energy) in rated {
            let path = PathBuf::from(format!("/music/{name}"));
            library.add_files(vec![path.clone()]);
            library.record_outcome(
                &path,
                Ok(TrackAnalysis {
                    energy,
                    tuning_cents: None,
                    key_runner_up: None,
                    key_score_margin: None,
                    tempo_estimate: None,
                    tempo_score_margin: None,
                    beat_grid: None,
                    cues: Vec::new(),
                    key_timeline: Vec::new(),
                    quality: None,
                    ..analysis(None, None, 300.0)
                }),
            );
        }
        library.add_files(vec![PathBuf::from("/music/pending.mp3")]);

        assert_eq!(library.tracks()[1].energy(), Some(10));
        assert_eq!(library.tracks()[2].energy(), None);
        assert_eq!(
            visible(&library, "", ascending(SortColumn::Energy)),
            [
                "calm.mp3",
                "warm.mp3",
                "peak.mp3",
                "silent.mp3",
                "pending.mp3"
            ]
        );
        assert_eq!(
            visible(&library, "", descending(SortColumn::Energy)),
            [
                "peak.mp3",
                "warm.mp3",
                "calm.mp3",
                "silent.mp3",
                "pending.mp3"
            ]
        );
    }

    #[test]
    fn a_missing_value_is_a_dash_only_once_the_track_is_analyzed() {
        let mut library = sample_library();
        library.record_outcome(
            Path::new("/music/house/Low Tide.aiff"),
            Ok(TrackAnalysis {
                energy: Some(7),
                tuning_cents: None,
                key_runner_up: None,
                key_score_margin: None,
                tempo_estimate: None,
                tempo_score_margin: None,
                beat_grid: None,
                cues: Vec::new(),
                key_timeline: Vec::new(),
                quality: None,
                ..analysis(Some("A minor"), Some(124.0), 408.0)
            }),
        );
        let energy_cell = |name: &str| {
            let track = library
                .tracks()
                .iter()
                .find(|track| track.file_name == name)
                .unwrap();
            track.value_or_dash(track.energy().map(|energy| energy.to_string()))
        };
        assert_eq!(energy_cell("Low Tide.aiff").as_deref(), Some("7"));
        assert_eq!(energy_cell("saffron.mp3").as_deref(), Some("\u{2013}"));
        assert_eq!(energy_cell("broken.mp3"), None);
        assert_eq!(energy_cell("waiting.m4a"), None);
    }

    #[test]
    fn sorts_folders_without_regard_to_letter_case() {
        let mut library = Library::default();
        library.add_files(
            [
                "/music/techno/b.mp3",
                "/music/House/a.mp3",
                "/music/ambient/c.mp3",
            ]
            .map(PathBuf::from)
            .to_vec(),
        );
        assert_eq!(
            visible(&library, "", ascending(SortColumn::Folder)),
            ["c.mp3", "a.mp3", "b.mp3"]
        );
        assert_eq!(
            visible(&library, "", descending(SortColumn::Folder)),
            ["b.mp3", "a.mp3", "c.mp3"]
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
        let no_column = FolderColumn::default();
        let in_folder = library.visible_tracks(
            LibraryFilter::AllTracks,
            Some(Path::new("/music/house")),
            "",
            None,
            &no_column,
        );
        assert_eq!(
            names(&library, &in_folder),
            ["Low Tide.aiff", "saffron.mp3"]
        );
        let needing_review =
            library.visible_tracks(LibraryFilter::NeedsReview, None, "", None, &no_column);
        assert_eq!(names(&library, &needing_review), ["broken.mp3"]);
        let needing_review_in_house = library.visible_tracks(
            LibraryFilter::NeedsReview,
            Some(Path::new("/music/house")),
            "",
            None,
            &no_column,
        );
        assert!(needing_review_in_house.is_empty());
        let searched_in_techno = library.visible_tracks(
            LibraryFilter::AllTracks,
            Some(Path::new("/music/techno")),
            "glass",
            None,
            &no_column,
        );
        assert_eq!(names(&library, &searched_in_techno), ["Glasshouse.flac"]);
    }

    /// A library of `count` analyzed tracks in one folder.
    fn analyzed_library(count: u32) -> Library {
        let mut library = Library::default();
        library.add_files(
            (0..count)
                .map(|number| PathBuf::from(format!("/music/set/track {number:05}.flac")))
                .collect(),
        );
        for number in 0..count {
            let key = Key::new((number % 12) as u8, euphoria_core::Mode::Minor);
            library.record_outcome(
                Path::new(&format!("/music/set/track {number:05}.flac")),
                Ok(TrackAnalysis {
                    duration_seconds: 200.0 + (number % 240) as f32,
                    key: Some(key),
                    bpm: Some(90.0 + (number % 80) as f32),
                    energy: Some((number % 10) as u8 + 1),
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

    /// The shortest of three runs of filtering and sorting `library` by key,
    /// so that a moment the test machine spends on other work does not count.
    fn shortest_sort_time(library: &Library) -> std::time::Duration {
        (0..3)
            .map(|_| {
                let started = std::time::Instant::now();
                let shown = library.visible_tracks(
                    LibraryFilter::AllTracks,
                    None,
                    "track",
                    descending(SortColumn::Key),
                    &FolderColumn::default(),
                );
                let elapsed = started.elapsed();
                assert_eq!(shown.len(), library.tracks().len());
                elapsed
            })
            .min()
            .unwrap_or_default()
    }

    #[test]
    fn ten_thousand_tracks_filter_and_sort_quickly() {
        let library = analyzed_library(10_000);

        let elapsed = shortest_sort_time(&library);

        // On an M5 the shortest of three runs takes 20 to 300 ms with the
        // other tests running, the longest when every core is busy. The
        // bound is ten times that, so only a far slower filter or sort,
        // such as one that allocates in every comparison, fails it.
        assert!(
            elapsed < std::time::Duration::from_secs(3),
            "10,000 tracks took {elapsed:?}"
        );
    }

    #[test]
    fn a_user_cue_past_the_end_is_hidden_once_the_track_is_analyzed() {
        let mut library = Library::default();
        let path = Path::new("/music/techno/Undertow.wav");
        let cues = vec![
            Cue {
                seconds: 12.0,
                name: "Intro".to_string(),
                hot_cue: Some(euphoria_core::HotCue::from_letter('A').unwrap()),
            },
            Cue {
                seconds: 999.0,
                name: "Past the end".to_string(),
                hot_cue: None,
            },
        ];

        // Without an analysis the length is unknown and every cue of the
        // set is shown.
        library.add_files(vec![path.to_path_buf()]);
        library.set_edited_cues(path, Some(cues.clone()));
        let track = library.track_by_path(path).unwrap();
        assert_eq!(track.cues().cues, cues);

        // The analysis records the length, and the cue past the end of the
        // 370 s track is left out, the same rule as TrackCues::of.
        library.record_outcome(path, Ok(analysis(None, None, 370.0)));
        let shown = library.track_by_path(path).unwrap().cues().cues;
        assert_eq!(shown, cues[..1].to_vec());
    }
}
