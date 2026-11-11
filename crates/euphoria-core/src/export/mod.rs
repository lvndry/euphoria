// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/// Building the library to export from analysis results.
pub mod assemble;
/// Mirroring folders on disk as a playlist tree.
pub mod playlists;
/// The rekordbox XML collection format.
pub mod rekordbox;

pub use assemble::{
    AssembledLibrary, ExportContents, PlaylistLayout, SkippedTrack, UnreadTags, assemble_library,
    assemble_library_with,
};
pub use playlists::{playlist_count, playlists_from_folders};
