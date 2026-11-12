// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Keeps playlists and manual corrections attached to our own tag rewrites.

use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use euphoria_core::tags::RewriteStage;
use euphoria_core::{
    Cache, ContentHash, EuphoriaError, PlaylistStore, default_cache_path, default_playlists_path,
};

use crate::output;
use crate::style::Theme;
use crate::terminal::Printable;

/// Opens a store only if the caller's explicit path already exists.
fn open_existing<T>(
    path: Result<PathBuf, EuphoriaError>,
    open: impl FnOnce(&Path) -> Result<T, EuphoriaError>,
) -> Result<Option<T>, String> {
    let path = path.map_err(|error| error.chain())?;
    if !path
        .try_exists()
        .map_err(|error| format!("{}: {error}", path.display()))?
    {
        return Ok(None);
    }
    open(&path).map(Some).map_err(|error| error.chain())
}

pub struct RewriteIdentities {
    playlists: Option<PlaylistStore>,
    cache: Option<Cache>,
    before: HashMap<PathBuf, ContentHash>,
    playlist_failures: Vec<String>,
    correction_failures: Vec<String>,
    changed: bool,
}

impl RewriteIdentities {
    /// Neither store is created for people who have never used it.
    /// `uses_cache` follows the command's explicit `--no-cache` choice.
    pub fn new(uses_cache: bool) -> Self {
        let mut playlist_failures = Vec::new();
        let playlists = match open_existing(default_playlists_path(), PlaylistStore::open) {
            Ok(store) => store,
            Err(error) => {
                playlist_failures.push(error);
                None
            }
        };
        let mut correction_failures = Vec::new();
        let cache = if uses_cache {
            match open_existing(default_cache_path(), Cache::open) {
                Ok(cache) => cache,
                Err(error) => {
                    correction_failures.push(error);
                    None
                }
            }
        } else {
            None
        };
        Self {
            playlists,
            cache,
            before: HashMap::new(),
            playlist_failures,
            correction_failures,
            changed: false,
        }
    }

    pub fn observe(&mut self, path: &Path, stage: RewriteStage) {
        match stage {
            RewriteStage::Before => {
                // A correction can be added while the replacement is prepared,
                // so retain the old identity even if no correction exists yet.
                if self.playlists.is_none() && self.cache.is_none() {
                    return;
                }
                match ContentHash::of_file(path) {
                    Ok(hash) => {
                        self.before.insert(path.to_path_buf(), hash);
                    }
                    Err(error) => {
                        let message = format!("{}: {error}", path.display());
                        if self.playlists.is_some() {
                            self.playlist_failures.push(message.clone());
                        }
                        if self.cache.is_some() {
                            self.correction_failures.push(message);
                        }
                    }
                }
            }
            RewriteStage::After => {
                self.changed = true;
                let Some(before) = self.before.remove(path) else {
                    return;
                };
                let after = match ContentHash::of_file(path) {
                    Ok(hash) => hash,
                    Err(error) => {
                        let message = format!("{}: {error}", path.display());
                        if self.playlists.is_some() {
                            self.playlist_failures.push(message.clone());
                        }
                        if self.cache.is_some() {
                            self.correction_failures.push(message);
                        }
                        return;
                    }
                };
                if let Some(store) = &mut self.playlists
                    && let Err(error) = store.rebind_rewritten_file(path, before, after)
                {
                    self.playlist_failures.push(error.chain());
                }
                if let Some(cache) = &mut self.cache
                    && let Err(error) = cache.carry_rewritten_correction(before, after)
                {
                    self.correction_failures.push(error.chain());
                }
            }
        }
    }

    pub fn report(&self, writer: &mut impl Write, theme: &Theme) -> io::Result<()> {
        if !self.changed {
            return Ok(());
        }
        for failure in &self.playlist_failures {
            output::write_warning(
                writer,
                theme,
                format_args!(
                    "could not keep saved playlists in step with tag changes: {}",
                    Printable(failure)
                ),
            )?;
        }
        if !self.playlist_failures.is_empty() {
            output::write_hint(
                writer,
                theme,
                "If affected tracks show as replaced in a playlist, add them to that playlist again.",
            )?;
        }
        for failure in &self.correction_failures {
            output::write_warning(
                writer,
                theme,
                format_args!(
                    "could not keep manual corrections in step with tag changes: {}",
                    Printable(failure)
                ),
            )?;
        }
        if !self.correction_failures.is_empty() {
            output::write_hint(
                writer,
                theme,
                "Check the affected tracks' key and tempo in the desktop app before exporting or writing their analysis.",
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_correction_created_during_the_rewrite_follows_the_committed_content() {
        struct Folder(PathBuf);
        impl Drop for Folder {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let folder = Folder(std::env::temp_dir().join(format!(
            "euphoria-cli-rewrite-{}-{unique}",
            std::process::id()
        )));
        std::fs::create_dir(&folder.0).unwrap();
        let track = folder.0.join("track.mp3");
        let database = folder.0.join("cache.sqlite3");
        std::fs::write(&track, b"audio with original tags").unwrap();
        let mut observer = RewriteIdentities {
            playlists: None,
            cache: Some(Cache::open(&database).unwrap()),
            before: HashMap::new(),
            playlist_failures: Vec::new(),
            correction_failures: Vec::new(),
            changed: false,
        };
        observer.observe(&track, RewriteStage::Before);
        let correction = euphoria_core::Correction::of_key(euphoria_core::Key::new(
            4,
            euphoria_core::Mode::Minor,
        ));
        Cache::open(&database)
            .unwrap()
            .set_correction(&track, &correction)
            .unwrap();
        std::fs::write(&track, b"audio with rewritten tags").unwrap();
        observer.observe(&track, RewriteStage::After);
        assert_eq!(
            Cache::open(&database)
                .unwrap()
                .verified_correction_for(&track)
                .unwrap(),
            Some(correction)
        );
        assert!(observer.correction_failures.is_empty());
    }
}
