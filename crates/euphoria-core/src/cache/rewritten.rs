// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Keeping the cache in step with files whose tags were rewritten.
//!
//! The cache knows a file by the hash of all its bytes, tags included, so a
//! tag write gives every file it touches content the cache has never seen.
//! The audio is the same, though, so the analysis of the file before the
//! write still holds. A writer records it for the new content right after
//! writing, and the next run serves it without analyzing again. An undo
//! returns a file to content the cache already knows, and only where that
//! content now is needs recording.

use std::io;
use std::path::Path;

use rusqlite::{OptionalExtension, params};

use super::identity::{ContentHash, FileStamp};
use super::{Cache, CacheWrite, FileLocation, cache_error};
use crate::analysis::TrackAnalysis;
use crate::error::EuphoriaError;

impl Cache {
    /// Records `analysis` for the file at `path` as it is now, after a tag
    /// write changed its tags and nothing else.
    ///
    /// `analysis` must be what this version of Euphoria found for the file
    /// before the write, such as an analysis this cache served or stored: it
    /// is recorded under the current analyzer and energy model versions,
    /// like any new analysis. The file is read once to hash it. When it
    /// changes while being read, nothing is recorded, and the file is
    /// analyzed again on the next run.
    ///
    /// # Errors
    ///
    /// [`EuphoriaError::Scan`] when the file cannot be read, and
    /// [`EuphoriaError::Cache`] when the database cannot be written.
    pub fn record_retagged_file(
        &mut self,
        path: &Path,
        analysis: &TrackAnalysis,
    ) -> Result<(), EuphoriaError> {
        let Some((content_hash, location)) = steady_content(path).map_err(unreadable(path))? else {
            return Ok(());
        };
        self.apply_write(&CacheWrite::Store {
            content_hash,
            analysis: analysis.clone(),
            location,
        })
        .map_err(cache_error(&self.path))
    }

    /// Records where the file at `path` is when the cache already holds a
    /// current analysis of its content, as after an undo puts back the bytes
    /// a file had before a write. Returns false, recording nothing, when the
    /// content is unknown or the file changed while it was read.
    ///
    /// # Errors
    ///
    /// [`EuphoriaError::Scan`] when the file cannot be read, and
    /// [`EuphoriaError::Cache`] when the database cannot be read or written.
    pub fn recognise_file(&mut self, path: &Path) -> Result<bool, EuphoriaError> {
        let Some((content_hash, location)) = steady_content(path).map_err(unreadable(path))? else {
            return Ok(false);
        };
        let is_known = self
            .connection
            .query_row(
                "SELECT 1 FROM analyses
                 WHERE content_hash = ?1 AND analyzer_version = ?2
                   AND energy_model_version = ?3",
                params![
                    content_hash.as_bytes(),
                    self.analyzer_version,
                    self.energy_model_version
                ],
                |_| Ok(()),
            )
            .optional()
            .map_err(cache_error(&self.path))?
            .is_some();
        if is_known {
            self.apply_write(&CacheWrite::Relocate {
                content_hash,
                location,
            })
            .map_err(cache_error(&self.path))?;
        }
        Ok(is_known)
    }
}

/// The hash and location of the file at `path`, or `None` when its size or
/// modification time changed while it was being hashed.
fn steady_content(path: &Path) -> io::Result<Option<(ContentHash, FileLocation)>> {
    let location = FileLocation {
        absolute_path: std::path::absolute(path)?,
        stamp: FileStamp::of(path)?,
    };
    let content_hash = ContentHash::of_file(path)?;
    if FileStamp::of(path)? != location.stamp {
        return Ok(None);
    }
    Ok(Some((content_hash, location)))
}

fn unreadable(path: &Path) -> impl Fn(io::Error) -> EuphoriaError {
    let path = path.to_path_buf();
    move |source| EuphoriaError::Scan {
        path: path.clone(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::super::CachedAnalyzer;
    use super::super::test_support::TemporaryFolder;
    use super::*;
    use crate::key::{Key, Mode};

    fn analysis() -> TrackAnalysis {
        TrackAnalysis {
            duration_seconds: 300.0,
            key: Some(Key::new(9, Mode::Minor)),
            bpm: Some(124.0),
            energy: Some(7),
            tuning_cents: Some(-3.0),
            key_timeline: Vec::new(),
        }
    }

    /// Stores the analysis for the file as it is, as a first analysis would.
    fn analyzed(folder: &TemporaryFolder, path: &Path) {
        let mut cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        cache.record_retagged_file(path, &analysis()).unwrap();
    }

    fn fresh_analyzer(folder: &TemporaryFolder) -> CachedAnalyzer {
        CachedAnalyzer::new(Cache::open(&folder.path().join("cache.sqlite3")).unwrap()).unwrap()
    }

    /// Rewrites the file the way a tag write would: new bytes, same audio.
    fn retag(path: &Path, tag: &[u8]) {
        let mut bytes = std::fs::read(path).unwrap();
        bytes.extend_from_slice(tag);
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn a_retagged_file_is_served_from_the_cache_without_being_read() {
        let folder = TemporaryFolder::new();
        let track = folder.path().join("track.mp3");
        std::fs::write(&track, b"audio").unwrap();
        analyzed(&folder, &track);

        retag(&track, b" with a tag");
        assert_eq!(fresh_analyzer(&folder).stored_analysis(&track), None);
        let mut cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        cache.record_retagged_file(&track, &analysis()).unwrap();

        let analyzer = fresh_analyzer(&folder);
        assert_eq!(analyzer.stored_analysis(&track), Some(analysis()));
        let served = analyzer.analyze_file(&track).unwrap();
        assert!(served.from_cache);
        assert_eq!(served.analysis, analysis());
    }

    #[test]
    fn a_file_put_back_by_an_undo_is_found_at_its_path_again() {
        let folder = TemporaryFolder::new();
        let track = folder.path().join("track.flac");
        std::fs::write(&track, b"audio").unwrap();
        analyzed(&folder, &track);
        let original = std::fs::read(&track).unwrap();

        retag(&track, b" with a tag");
        let mut cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        cache.record_retagged_file(&track, &analysis()).unwrap();
        std::fs::write(&track, &original).unwrap();
        assert_eq!(fresh_analyzer(&folder).stored_analysis(&track), None);

        assert!(cache.recognise_file(&track).unwrap());
        let analyzer = fresh_analyzer(&folder);
        assert_eq!(analyzer.stored_analysis(&track), Some(analysis()));
        assert!(analyzer.analyze_file(&track).unwrap().from_cache);
    }

    #[test]
    fn unknown_content_is_not_recognised_and_nothing_is_recorded() {
        let folder = TemporaryFolder::new();
        let track = folder.path().join("track.wav");
        std::fs::write(&track, b"never analyzed").unwrap();
        let mut cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        assert!(!cache.recognise_file(&track).unwrap());
        assert_eq!(cache.entry_count().unwrap(), 0);
    }

    #[test]
    fn a_missing_file_is_an_error_naming_it() {
        let folder = TemporaryFolder::new();
        let gone = folder.path().join("gone.mp3");
        let mut cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        for error in [
            cache.record_retagged_file(&gone, &analysis()).unwrap_err(),
            cache.recognise_file(&gone).unwrap_err(),
        ] {
            assert!(
                matches!(&error, EuphoriaError::Scan { path, .. } if *path == gone),
                "{error:?}"
            );
        }
    }
}
