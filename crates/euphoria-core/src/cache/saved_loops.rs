// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The loops users saved, in a table of their own.
//!
//! Kept like edited cues: under the content hash of the file, with no
//! analyzer version, so a new version of the analyzer keeps every loop, and
//! carried to the new content of a file whose tags Euphoria writes. A track
//! without a row has no saved loops.

use std::path::Path;

use rusqlite::types::Type;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::corrections::unreadable;
use super::identity::ContentHash;
use super::{Cache, cache_error, seconds_since_epoch};
use crate::error::EuphoriaError;
use crate::track_loops::{SavedLoop, check_loops, sorted_loops};

/// One loop as the table stores it, in a JSON array.
#[derive(Serialize, Deserialize)]
struct StoredLoop {
    start_seconds: f64,
    end_seconds: f64,
    name: String,
}

impl Cache {
    /// The loops the user saved for the file at `path`, in order of their
    /// start, or none.
    ///
    /// A file at the place an analysis recorded is matched without being
    /// read. Another file is hashed, but only while the cache holds any
    /// saved loops at all.
    ///
    /// # Errors
    ///
    /// [`EuphoriaError::Scan`] when the file must be hashed and cannot be
    /// read, and [`EuphoriaError::Cache`] when the database cannot be read
    /// or holds loops it cannot understand.
    pub fn saved_loops_for(&self, path: &Path) -> Result<Vec<SavedLoop>, EuphoriaError> {
        if !self.has_saved_loops()? {
            return Ok(Vec::new());
        }
        let content_hash = self.content_at(path)?;
        self.saved_loops_of(&content_hash)
    }

    /// The loops the user saved for the file at `path`, after reading the
    /// whole file to be sure of its content, for export.
    ///
    /// # Errors
    ///
    /// As [`Cache::saved_loops_for`].
    pub fn verified_saved_loops_for(&self, path: &Path) -> Result<Vec<SavedLoop>, EuphoriaError> {
        if !self.has_saved_loops()? {
            return Ok(Vec::new());
        }
        let content_hash = ContentHash::of_file(path).map_err(unreadable(path))?;
        self.saved_loops_of(&content_hash)
    }

    /// Keeps `loops` as the saved loops of the file at `path`, replacing
    /// any kept before; none forgets them. The file is read once to hash
    /// it, unless an analysis recorded where it is.
    ///
    /// # Errors
    ///
    /// [`EuphoriaError::InvalidLoops`] when [`check_loops`] refuses
    /// `loops`, [`EuphoriaError::Scan`] when the file cannot be read, and
    /// [`EuphoriaError::Cache`] when the database cannot be written.
    pub fn set_saved_loops(
        &mut self,
        path: &Path,
        loops: &[SavedLoop],
    ) -> Result<(), EuphoriaError> {
        check_loops(loops)?;
        let content_hash = self.content_at(path)?;
        let result = if loops.is_empty() {
            self.connection.execute(
                "DELETE FROM saved_loops WHERE content_hash = ?1",
                params![content_hash.as_bytes()],
            )
        } else {
            self.connection.execute(
                "INSERT OR REPLACE INTO saved_loops (content_hash, loops, saved_at)
                 VALUES (?1, ?2, ?3)",
                params![
                    content_hash.as_bytes(),
                    stored_text(loops).map_err(cache_error(&self.path))?,
                    seconds_since_epoch()
                ],
            )
        };
        result.map(|_| ()).map_err(cache_error(&self.path))
    }

    fn has_saved_loops(&self) -> Result<bool, EuphoriaError> {
        self.connection
            .query_row("SELECT EXISTS (SELECT 1 FROM saved_loops)", [], |row| {
                row.get(0)
            })
            .map_err(cache_error(&self.path))
    }

    fn saved_loops_of(&self, content_hash: &ContentHash) -> Result<Vec<SavedLoop>, EuphoriaError> {
        self.connection
            .query_row(
                "SELECT loops FROM saved_loops WHERE content_hash = ?1",
                params![content_hash.as_bytes()],
                |row| {
                    let text: String = row.get(0)?;
                    parse_stored(&text).ok_or_else(|| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            Type::Text,
                            format!("{text:?} is not a set of loops").into(),
                        )
                    })
                },
            )
            .optional()
            .map(Option::unwrap_or_default)
            .map_err(cache_error(&self.path))
    }
}

fn stored_text(loops: &[SavedLoop]) -> serde_json::Result<String> {
    let stored: Vec<StoredLoop> = sorted_loops(loops.to_vec())
        .into_iter()
        .map(|saved| StoredLoop {
            start_seconds: saved.start_seconds,
            end_seconds: saved.end_seconds,
            name: saved.name,
        })
        .collect();
    serde_json::to_string(&stored)
}

fn parse_stored(text: &str) -> Option<Vec<SavedLoop>> {
    let stored: Vec<StoredLoop> = serde_json::from_str(text).ok()?;
    let loops: Vec<SavedLoop> = stored
        .into_iter()
        .map(|stored| SavedLoop {
            start_seconds: stored.start_seconds,
            end_seconds: stored.end_seconds,
            name: stored.name,
        })
        .collect();
    check_loops(&loops).ok()?;
    Some(loops)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::TemporaryFolder;
    use super::*;

    fn open(folder: &TemporaryFolder) -> Cache {
        Cache::open(&folder.path().join("cache.sqlite3")).unwrap()
    }

    fn track(folder: &TemporaryFolder, name: &str, content: &[u8]) -> std::path::PathBuf {
        let path = folder.path().join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn saved(start_seconds: f64, end_seconds: f64, name: &str) -> SavedLoop {
        SavedLoop {
            start_seconds,
            end_seconds,
            name: name.to_string(),
        }
    }

    #[test]
    fn saved_loops_are_kept_in_order_under_the_content_and_forgotten_when_empty() {
        let folder = TemporaryFolder::new();
        let first = track(&folder, "a.mp3", b"some audio");
        let copy = track(&folder, "copy.mp3", b"some audio");
        let other = track(&folder, "b.mp3", b"other audio");
        let mut cache = open(&folder);
        assert!(cache.saved_loops_for(&first).unwrap().is_empty());

        let loops = vec![
            saved(64.0, 72.0, "Drop \"loop\"\n"),
            saved(8.0, 16.0, "Intro"),
        ];
        cache.set_saved_loops(&first, &loops).unwrap();
        let expected = vec![
            saved(8.0, 16.0, "Intro"),
            saved(64.0, 72.0, "Drop \"loop\"\n"),
        ];
        assert_eq!(cache.saved_loops_for(&first).unwrap(), expected);
        assert_eq!(
            cache.saved_loops_for(&copy).unwrap(),
            expected,
            "kept under the content, so a copy of the file has them too"
        );
        assert_eq!(cache.verified_saved_loops_for(&copy).unwrap(), expected);
        assert!(cache.saved_loops_for(&other).unwrap().is_empty());

        drop(cache);
        let mut reopened = open(&folder);
        assert_eq!(reopened.saved_loops_for(&first).unwrap(), expected);
        reopened.set_saved_loops(&first, &[]).unwrap();
        assert!(reopened.saved_loops_for(&first).unwrap().is_empty());
    }

    #[test]
    fn a_loop_that_cannot_be_is_refused_and_nothing_is_kept() {
        let folder = TemporaryFolder::new();
        let path = track(&folder, "a.mp3", b"some audio");
        let mut cache = open(&folder);
        let refused = cache.set_saved_loops(&path, &[saved(8.0, 4.0, "")]);
        assert!(matches!(refused, Err(EuphoriaError::InvalidLoops { .. })));
        assert!(cache.saved_loops_for(&path).unwrap().is_empty());
    }
}
