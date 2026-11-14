// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Builds the library to export from the results of an analysis batch.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rayon::prelude::*;

use super::playlists::playlists_from_folders;
use super::rekordbox::{CueKind, CuePoint, Library, Playlist, PlaylistNode, Track};
use crate::analysis::{FileResult, TrackAnalysis};
use crate::confidence::Confidence;
use crate::error::EuphoriaError;
use crate::track_cues::{Cue, TrackCues};
use crate::track_name::{TrackName, read_track_name};

/// Which playlists an exported library gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaylistLayout<'paths> {
    /// No playlists, only the collection of tracks.
    CollectionOnly,
    /// A playlist tree that mirrors the folders the tracks sit in, as built by
    /// [`playlists_from_folders`].
    MirrorFolders {
        /// The folders that were searched for tracks.
        folders: &'paths [PathBuf],
        /// Name of the playlist for tracks inside none of `folders`.
        loose_playlist_name: &'paths str,
    },
    /// One playlist with the tracks in the order given, such as a playlist
    /// the user built. A path whose analysis failed is left out of it.
    Ordered {
        /// The playlist's name.
        name: &'paths str,
        /// The tracks in play order.
        tracks: &'paths [PathBuf],
    },
}

/// A file that is not part of the exported library.
#[derive(Debug)]
pub struct SkippedTrack {
    /// The file that was left out.
    pub path: PathBuf,
    /// Why its analysis failed.
    pub reason: EuphoriaError,
}

/// A track that was exported under its file name because its tags could not
/// be read.
#[derive(Debug)]
pub struct UnreadTags {
    /// The file whose tags could not be read.
    pub path: PathBuf,
    /// Why they could not be read.
    pub reason: EuphoriaError,
}

/// A library ready to be written, and the files it does not contain.
#[derive(Debug)]
pub struct AssembledLibrary {
    /// The tracks that were analyzed, in the order of the results, and their
    /// playlists.
    pub library: Library,
    /// The files that were left out, in the order of the results.
    pub skipped: Vec<SkippedTrack>,
    /// The tracks of the library whose tags could not be read, in the order
    /// of the results.
    pub unread_tags: Vec<UnreadTags>,
}

/// What an export contains beyond each track's key, tempo and energy.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ExportContents {
    /// Write each track's beat grid, so the DJ program needs no grid of its
    /// own. A grid rated low confidence is left out, and the program
    /// analyzes that track itself.
    pub beat_grid: bool,
    /// Write each track's cues as memory cues, and those on a hot cue pad
    /// as hot cues too: the user's set from
    /// [`ExportContents::edited_cues`] when the track has one, and
    /// otherwise the cues analysis placed, on pads from A in order. The
    /// cues analysis placed are written only on tracks whose beat grid is
    /// written, since they sit on the bars of that grid; the user's are
    /// written whatever the grid, since the user placed them.
    pub cues: bool,
    /// The cues users set, by file, as [`Cache::edited_cues_for`] reads
    /// them. A file missing here has the cues analysis placed.
    ///
    /// [`Cache::edited_cues_for`]: crate::cache::Cache::edited_cues_for
    pub edited_cues: HashMap<PathBuf, Vec<Cue>>,
}

/// Builds the library to export from analysis results.
///
/// Each analyzed file becomes a track with its duration, key and tempo, and
/// with its energy rating as the comment "Energy 7" when it has one. Its
/// title and artist are read from the file's own tags with [`read_track_name`]; a
/// file with no title is given its file name without the extension. So is a
/// file whose tags cannot be read, which has no artist and is also returned
/// in [`AssembledLibrary::unread_tags`] with the reason. A file whose
/// analysis failed is not in the library and is returned in
/// [`AssembledLibrary::skipped`] with the reason.
///
/// Tags are read in parallel on the current Rayon thread pool.
pub fn assemble_library(results: Vec<FileResult>, layout: PlaylistLayout<'_>) -> AssembledLibrary {
    assemble_library_with(results, layout, ExportContents::default())
}

/// [`assemble_library`], with what [`ExportContents`] asks for on top.
///
/// With [`ExportContents::beat_grid`], a track whose beat grid is rated
/// medium or high gets the grid's first downbeat and the grid's tempo, which
/// is then also the tempo written for the track. The tempo is written with
/// two decimals, off by at most 0.005 BPM, which moves a beat T seconds
/// into the track by at most T x 0.005 / BPM seconds: 23 ms at the end of a
/// seven minute track at 90 BPM, 16 ms at 128 BPM and 12 ms at 180 BPM.
pub fn assemble_library_with(
    results: Vec<FileResult>,
    layout: PlaylistLayout<'_>,
    contents: ExportContents,
) -> AssembledLibrary {
    let outcomes: Vec<Result<(Track, Option<UnreadTags>), SkippedTrack>> = results
        .into_par_iter()
        .map(|result| {
            let FileResult { path, outcome } = result;
            match outcome {
                Ok(analysis) => Ok(track_from_analysis(&path, &analysis, &contents)),
                Err(reason) => Err(SkippedTrack { path, reason }),
            }
        })
        .collect();

    let mut tracks = Vec::new();
    let mut skipped = Vec::new();
    let mut unread_tags = Vec::new();
    for outcome in outcomes {
        match outcome {
            Ok((track, track_unread_tags)) => {
                tracks.push(track);
                unread_tags.extend(track_unread_tags);
            }
            Err(skipped_track) => skipped.push(skipped_track),
        }
    }

    let playlists = match layout {
        PlaylistLayout::CollectionOnly => Vec::new(),
        PlaylistLayout::MirrorFolders {
            folders,
            loose_playlist_name,
        } => {
            let track_paths: Vec<PathBuf> = tracks.iter().map(|track| track.path.clone()).collect();
            playlists_from_folders(folders, &track_paths, loose_playlist_name)
        }
        PlaylistLayout::Ordered {
            name,
            tracks: order,
        } => {
            let exported: std::collections::HashSet<&Path> =
                tracks.iter().map(|track| track.path.as_path()).collect();
            vec![PlaylistNode::Playlist(Playlist {
                name: name.to_string(),
                tracks: order
                    .iter()
                    .filter(|path| exported.contains(path.as_path()))
                    .cloned()
                    .collect(),
            })]
        }
    };
    AssembledLibrary {
        library: Library { tracks, playlists },
        skipped,
        unread_tags,
    }
}

fn track_from_analysis(
    path: &Path,
    analysis: &TrackAnalysis,
    contents: &ExportContents,
) -> (Track, Option<UnreadTags>) {
    let (tags, unread_tags) = match read_track_name(path) {
        Ok(tags) => (tags, None),
        Err(reason) => (
            TrackName::default(),
            Some(UnreadTags {
                path: path.to_path_buf(),
                reason,
            }),
        ),
    };
    let title = tags.title.or_else(|| {
        path.file_stem()
            .map(|file_stem| file_stem.to_string_lossy().into_owned())
    });
    // A duration that is not a positive number of seconds is written as zero.
    let duration = Duration::try_from_secs_f32(analysis.duration_seconds).unwrap_or_default();
    let beat_grid = analysis
        .beat_grid
        .filter(|grid| contents.beat_grid && grid.confidence() != Confidence::Low);
    let edited_cues = contents.edited_cues.get(path).cloned();
    let cue_points = if contents.cues && (edited_cues.is_some() || beat_grid.is_some()) {
        cue_points(&TrackCues::of(analysis, edited_cues).cues)
    } else {
        Vec::new()
    };
    let track = Track {
        title,
        artist: tags.artist,
        key: analysis.key,
        bpm: beat_grid
            .map(|grid| grid.bpm)
            .or(analysis.bpm.map(f64::from)),
        comment: analysis.energy.map(|energy| format!("Energy {energy}")),
        first_beat: beat_grid
            .and_then(|grid| Duration::try_from_secs_f64(grid.first_downbeat_seconds).ok()),
        cue_points,
        ..Track::new(path, duration)
    };
    (track, unread_tags)
}

/// Each cue as a memory cue, and as a hot cue on its pad when it has one.
fn cue_points(cues: &[Cue]) -> Vec<CuePoint> {
    let mut points = Vec::with_capacity(cues.len() * 2);
    for cue in cues {
        let Ok(time) = Duration::try_from_secs_f64(cue.seconds) else {
            continue;
        };
        let hot = cue.hot_cue.map(|hot_cue| CueKind::Hot(hot_cue.pad()));
        for kind in std::iter::once(CueKind::Memory).chain(hot) {
            points.push(CuePoint {
                name: cue.name.clone(),
                time,
                kind,
            });
        }
    }
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::{BeatGrid, CueSection, SectionCue};
    use crate::track_cues::HotCue;

    fn analysis(beat_grid: Option<BeatGrid>) -> TrackAnalysis {
        TrackAnalysis {
            duration_seconds: 300.0,
            key: None,
            bpm: Some(124.0),
            energy: None,
            tuning_cents: None,
            key_timeline: Vec::new(),
            quality: None,
            key_runner_up: None,
            key_score_margin: None,
            key_source: None,
            tempo_estimate: None,
            tempo_score_margin: None,
            beat_grid,
            cues: vec![
                SectionCue {
                    seconds: 0.5,
                    section: CueSection::Start,
                },
                SectionCue {
                    seconds: 31.5,
                    section: CueSection::Drop,
                },
            ],
        }
    }

    fn confident_grid() -> BeatGrid {
        BeatGrid {
            first_downbeat_seconds: 0.5,
            bpm: 124.0,
            salience: 10.0,
            drift_seconds: Some(0.001),
        }
    }

    fn track_path() -> PathBuf {
        PathBuf::from("/no/such/track.mp3")
    }

    fn points(analysis: TrackAnalysis, contents: ExportContents) -> Vec<(String, f64, CueKind)> {
        let assembled = assemble_library_with(
            vec![FileResult {
                path: track_path(),
                outcome: Ok(analysis),
            }],
            PlaylistLayout::CollectionOnly,
            contents,
        );
        assembled.library.tracks[0]
            .cue_points
            .iter()
            .map(|point| (point.name.clone(), point.time.as_secs_f64(), point.kind))
            .collect()
    }

    fn with_cues(edited_cues: HashMap<PathBuf, Vec<Cue>>) -> ExportContents {
        ExportContents {
            beat_grid: true,
            cues: true,
            edited_cues,
        }
    }

    #[test]
    fn automatic_cues_are_memory_cues_and_hot_cues_from_a_on_a_written_grid() {
        assert_eq!(
            points(analysis(Some(confident_grid())), with_cues(HashMap::new())),
            vec![
                ("Start".to_string(), 0.5, CueKind::Memory),
                ("Start".to_string(), 0.5, CueKind::Hot(0)),
                ("Drop".to_string(), 31.5, CueKind::Memory),
                ("Drop".to_string(), 31.5, CueKind::Hot(1)),
            ]
        );
        assert!(points(analysis(None), with_cues(HashMap::new())).is_empty());
    }

    #[test]
    fn the_users_cues_replace_the_automatic_ones_with_their_pads_and_need_no_grid() {
        let edited = vec![
            Cue {
                seconds: 64.0,
                name: "Vocal".to_string(),
                hot_cue: HotCue::from_letter('E'),
            },
            Cue {
                seconds: 12.0,
                name: "Warm up".to_string(),
                hot_cue: None,
            },
        ];
        let contents = with_cues(HashMap::from([(track_path(), edited)]));
        let expected = vec![
            ("Warm up".to_string(), 12.0, CueKind::Memory),
            ("Vocal".to_string(), 64.0, CueKind::Memory),
            ("Vocal".to_string(), 64.0, CueKind::Hot(4)),
        ];
        assert_eq!(
            points(analysis(Some(confident_grid())), contents.clone()),
            expected
        );
        assert_eq!(points(analysis(None), contents.clone()), expected);
        let without_cues = ExportContents {
            cues: false,
            ..contents
        };
        assert!(points(analysis(None), without_cues).is_empty());
    }

    #[test]
    fn a_user_who_deleted_every_cue_exports_none() {
        let contents = with_cues(HashMap::from([(track_path(), Vec::new())]));
        assert!(points(analysis(Some(confident_grid())), contents).is_empty());
    }
}
