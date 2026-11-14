// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/// Building the library to export from analysis results.
pub mod assemble;
/// The checks run before an export writes.
pub mod checks;
/// The corrections the user made, as JSON Lines for training a model.
pub mod corrections;
/// Updating the tracks of an Engine DJ library database.
pub mod engine;
/// The blobs of performance data in an Engine DJ library.
pub mod engine_blobs;
/// The extended M3U playlist format.
pub mod m3u;
/// Mirroring folders on disk as a playlist tree.
pub mod playlists;
/// Where the DJ programs keep their libraries, and whether they run.
pub mod program_files;
/// The rekordbox XML collection format.
pub mod rekordbox;
/// Finding the tracks with cues in a rekordbox collection.
pub mod rekordbox_collection;
/// The Traktor collection format, NML.
pub mod traktor;
/// Finding the tracks with cues in a Traktor collection.
pub mod traktor_collection;
/// Updating the songs of a VirtualDJ database.xml.
pub mod virtualdj;

pub use assemble::{
    AssembledLibrary, ExportContents, PlaylistLayout, SkippedTrack, UnreadTags, assemble_library,
    assemble_library_with,
};
pub use corrections::{
    CORRECTIONS_SCHEMA_VERSION, PathInclusion, render_corrections_jsonl, write_corrections_file,
};
pub use m3u::{M3uTrack, render_m3u8, write_m3u8_file};
pub use playlists::{playlist_count, playlists_from_folders};
