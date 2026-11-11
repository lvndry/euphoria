// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/// Building the library to export from analysis results.
pub mod assemble;
/// The corrections the user made, as JSON Lines for training a model.
pub mod corrections;
/// The extended M3U playlist format.
pub mod m3u;
/// Mirroring folders on disk as a playlist tree.
pub mod playlists;
/// The rekordbox XML collection format.
pub mod rekordbox;

pub use assemble::{
    AssembledLibrary, ExportContents, PlaylistLayout, SkippedTrack, UnreadTags, assemble_library,
    assemble_library_with,
};
pub use corrections::{
    CORRECTIONS_SCHEMA_VERSION, PathInclusion, render_corrections_jsonl, write_corrections_file,
};
pub use m3u::{M3uTrack, render_m3u8, write_m3u8_file};
pub use playlists::{playlist_count, playlists_from_folders};
