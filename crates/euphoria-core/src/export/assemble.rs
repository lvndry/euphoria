// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Builds the library to export from the results of an analysis batch.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rayon::prelude::*;

use super::playlists::playlists_from_folders;
use super::rekordbox::{Library, Playlist, PlaylistNode, Track};
use crate::analysis::{FileResult, TrackAnalysis};
use crate::error::EuphoriaError;
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
    let outcomes: Vec<Result<(Track, Option<UnreadTags>), SkippedTrack>> = results
        .into_par_iter()
        .map(|result| {
            let FileResult { path, outcome } = result;
            match outcome {
                Ok(analysis) => Ok(track_from_analysis(&path, &analysis)),
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

fn track_from_analysis(path: &Path, analysis: &TrackAnalysis) -> (Track, Option<UnreadTags>) {
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
    let track = Track {
        title,
        artist: tags.artist,
        key: analysis.key,
        bpm: analysis.bpm.map(f64::from),
        comment: analysis.energy.map(|energy| format!("Energy {energy}")),
        ..Track::new(path, duration)
    };
    (track, unread_tags)
}
