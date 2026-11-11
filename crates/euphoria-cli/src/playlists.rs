// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The playlists built in the desktop app, as the command line reads them:
//! listed with `euphoria playlists`, and exported by name.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Result, bail};
use euphoria_core::{
    CachedAnalyzer, PlaylistStore, StoredPlaylist, default_cache_path, default_playlists_path,
    locate_moved_files,
};

use crate::batch::open_cache;
use crate::terminal::Printable;
use crate::with_causes;

/// A playlist ready to export: its name as the user wrote it, the files
/// found in order, and the ones that are no longer anywhere.
pub struct FoundPlaylist {
    pub name: String,
    pub files: Vec<PathBuf>,
    pub missing: Vec<PathBuf>,
}

/// Every playlist, oldest first, or none when the app never kept any.
fn load_playlists() -> Result<Vec<StoredPlaylist>> {
    let path = default_playlists_path().map_err(with_causes)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    PlaylistStore::open(&path)
        .and_then(|store| store.load())
        .map_err(with_causes)
}

/// The playlist called `name`, ignoring case, with each file looked for
/// where the cache last saw it when it moved, unless `uses_cache` is false.
pub fn find_playlist(name: &str, uses_cache: bool) -> Result<FoundPlaylist> {
    let mut playlists = load_playlists()?;
    let wanted = name.trim().to_lowercase();
    let Some(position) = playlists
        .iter()
        .position(|playlist| playlist.name.trim().to_lowercase() == wanted)
    else {
        if playlists.is_empty() {
            bail!(
                "there is no playlist called {name:?}: no playlists have been made yet. Build one in the desktop app."
            );
        }
        let names: Vec<String> = playlists
            .iter()
            .map(|playlist| format!("{:?}", playlist.name))
            .collect();
        bail!(
            "there is no playlist called {name:?}. The playlists are {}.",
            names.join(", ")
        );
    };
    if uses_cache
        && default_cache_path().is_ok_and(|path| path.exists())
        && let Ok(cache) = open_cache().and_then(CachedAnalyzer::new)
    {
        locate_moved_files(&mut playlists[position..=position], &cache);
    }
    let playlist = playlists.swap_remove(position);
    let (files, missing) = playlist
        .entries
        .into_iter()
        .map(|entry| entry.path)
        .partition(|path| path.exists());
    Ok(FoundPlaylist {
        name: playlist.name,
        files,
        missing,
    })
}

/// Lists the playlists with their labels and track counts.
pub fn list() -> Result<ExitCode> {
    let playlists = load_playlists()?;
    let mut standard_output = std::io::stdout().lock();
    if playlists.is_empty() {
        writeln!(
            standard_output,
            "No playlists yet. Build them in the desktop app."
        )?;
        return Ok(ExitCode::SUCCESS);
    }
    for playlist in &playlists {
        let count = playlist.entries.len();
        let label = playlist
            .label
            .as_deref()
            .map(|label| format!("{} ", Printable(label)))
            .unwrap_or_default();
        writeln!(
            standard_output,
            "{label}{}  ({count} {})",
            Printable(&playlist.name),
            if count == 1 { "track" } else { "tracks" }
        )?;
    }
    Ok(ExitCode::SUCCESS)
}
