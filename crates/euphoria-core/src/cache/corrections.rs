// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The keys and tempos users set by hand, in a table of their own.
//!
//! A correction is stored under the content hash of the file, like an
//! analysis, but with no analyzer version: a new version of the analyzer
//! analyzes every track again and keeps every correction. A file found at
//! the path and with the size and modification time an analysis recorded
//! is matched to its correction without being read. When a tag write or an
//! undo changes a file's bytes, the correction follows it to the new
//! content.

use std::path::Path;

use rusqlite::{OptionalExtension, Row, params};

use super::identity::{ContentHash, FileStamp};
use super::{Cache, cache_error, key_in_column, seconds_since_epoch, tempo_in_column};
use crate::correction::Correction;
use crate::error::EuphoriaError;
use crate::key::Key;

/// A correction as the cache keeps it, with what was detected for the same
/// content when an analysis of it is stored.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredCorrection {
    /// The content the correction belongs to.
    pub content_hash: ContentHash,
    /// The values the user set.
    pub correction: Correction,
    /// When the user set them, in seconds since the Unix epoch.
    pub corrected_at_seconds: i64,
    /// The stored analysis of the same content, if any. A correction made
    /// before the track was analyzed, or kept after its analysis was
    /// cleared, has none.
    pub detected: Option<DetectedValues>,
}

/// The key and tempo an analysis found, as the cache stored them.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectedValues {
    /// The detected key, or `None` when the track had no detectable key.
    pub key: Option<Key>,
    /// The detected tempo in the default tempo range, or `None` when the
    /// track had no detectable tempo.
    pub bpm: Option<f32>,
    /// The version of the analyzer that detected them, which may be older
    /// than the running one.
    pub analyzer_version: String,
    /// When the analysis was stored, in seconds since the Unix epoch.
    pub analyzed_at_seconds: i64,
    /// Where the file was last seen, absolute, as the cache records it:
    /// only paths that are valid Unicode are recorded. The file may have
    /// moved since.
    pub last_known_path: Option<String>,
}

impl Cache {
    /// The correction the user set for the file at `path`, if any.
    ///
    /// A file at the place an analysis recorded is matched without being
    /// read. Another file is hashed, but only while the cache holds any
    /// correction at all, so a library nobody corrected is never read here.
    ///
    /// # Errors
    ///
    /// [`EuphoriaError::Scan`] when the file must be hashed and cannot be
    /// read, and [`EuphoriaError::Cache`] when the database cannot be read.
    pub fn correction_for(&self, path: &Path) -> Result<Option<Correction>, EuphoriaError> {
        if !self.has_corrections()? {
            return Ok(None);
        }
        let content_hash = self.content_at(path)?;
        self.correction_of(&content_hash)
    }

    /// Keeps `correction` for the file at `path` as it is now, replacing any
    /// earlier one; an empty correction removes it. The file is read once to
    /// hash it, unless an analysis recorded where it is.
    ///
    /// # Errors
    ///
    /// [`EuphoriaError::Scan`] when the file cannot be read, and
    /// [`EuphoriaError::Cache`] when the database cannot be written.
    pub fn set_correction(
        &mut self,
        path: &Path,
        correction: &Correction,
    ) -> Result<(), EuphoriaError> {
        // Public fields also allow callers to bypass the constructors.
        correction.with_bpm(correction.bpm)?;
        let content_hash = self.content_at(path)?;
        let result = if correction.is_empty() {
            self.connection.execute(
                "DELETE FROM corrections WHERE content_hash = ?1",
                params![content_hash.as_bytes()],
            )
        } else {
            self.connection.execute(
                "INSERT OR REPLACE INTO corrections (content_hash, key, bpm, corrected_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    content_hash.as_bytes(),
                    correction.key.map(|key| key.to_string()),
                    correction.bpm,
                    seconds_since_epoch(),
                ],
            )
        };
        result.map(|_| ()).map_err(cache_error(&self.path))
    }

    /// The correction the user set for the file at `path`, if any, after
    /// reading the whole file to be sure of its content. For tag writing
    /// and export, which change files and other programs' libraries: a file
    /// replaced by different audio of the same size and modification time
    /// would pass the quick check of [`Cache::correction_for`].
    ///
    /// # Errors
    ///
    /// As [`Cache::correction_for`].
    pub fn verified_correction_for(
        &self,
        path: &Path,
    ) -> Result<Option<Correction>, EuphoriaError> {
        if !self.has_corrections()? {
            return Ok(None);
        }
        let content_hash = ContentHash::of_file(path).map_err(unreadable(path))?;
        self.correction_of(&content_hash)
    }

    /// Every correction kept in the cache, oldest first, each with the
    /// stored analysis of the same content. Reads no audio file.
    ///
    /// # Errors
    ///
    /// [`EuphoriaError::Cache`] when the database cannot be read, and
    /// [`EuphoriaError::InvalidCorrection`] when it holds a tempo that is
    /// not a positive number no faster than 999.
    pub fn stored_corrections(&self) -> Result<Vec<StoredCorrection>, EuphoriaError> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT corrections.content_hash, corrections.key, corrections.bpm,
                        corrections.corrected_at, analyses.analyzer_version,
                        analyses.key, analyses.bpm, analyses.analyzed_at,
                        analyses.last_known_path
                 FROM corrections
                 LEFT JOIN analyses ON analyses.content_hash = corrections.content_hash
                 ORDER BY corrections.corrected_at, corrections.content_hash",
            )
            .map_err(cache_error(&self.path))?;
        let stored: Vec<StoredCorrection> = statement
            .query_map([], stored_correction_in_row)
            .and_then(Iterator::collect)
            .map_err(cache_error(&self.path))?;
        for entry in &stored {
            entry.correction.with_bpm(entry.correction.bpm)?;
        }
        Ok(stored)
    }

    /// The content an analysis last recorded at `path`, whatever its
    /// analyzer version, when the file there still has the size and
    /// modification time recorded with it. Does not read the file.
    ///
    /// # Errors
    ///
    /// [`EuphoriaError::Cache`] when the database cannot be read.
    pub fn recorded_content(&self, path: &Path) -> Result<Option<ContentHash>, EuphoriaError> {
        let (Ok(absolute_path), Ok(stamp)) = (std::path::absolute(path), FileStamp::of(path))
        else {
            return Ok(None);
        };
        let Some(last_known_path) = absolute_path.to_str() else {
            return Ok(None);
        };
        self.connection
            .query_row(
                "SELECT content_hash FROM analyses
                 WHERE last_known_path = ?1 AND file_size = ?2 AND modified_at = ?3",
                params![last_known_path, stamp.size, stamp.modified_nanoseconds],
                |row| row.get::<_, [u8; 32]>(0),
            )
            .optional()
            .map(|bytes| bytes.map(ContentHash::from_bytes))
            .map_err(cache_error(&self.path))
    }

    /// The content of the file at `path`: the one an analysis recorded
    /// there, without reading the file, or else the hash of the file.
    pub(super) fn content_at(&self, path: &Path) -> Result<ContentHash, EuphoriaError> {
        match self.recorded_content(path)? {
            Some(content_hash) => Ok(content_hash),
            None => ContentHash::of_file(path).map_err(unreadable(path)),
        }
    }

    /// The content an analysis last recorded at `path`, whatever the file
    /// there is now, as before a write replaced it.
    pub(super) fn previous_content(
        &self,
        path: &Path,
    ) -> Result<Option<ContentHash>, EuphoriaError> {
        let Some(last_known_path) = std::path::absolute(path)
            .ok()
            .and_then(|path| path.to_str().map(str::to_string))
        else {
            return Ok(None);
        };
        self.connection
            .query_row(
                "SELECT content_hash FROM analyses WHERE last_known_path = ?1",
                params![last_known_path],
                |row| row.get::<_, [u8; 32]>(0),
            )
            .optional()
            .map(|bytes| bytes.map(ContentHash::from_bytes))
            .map_err(cache_error(&self.path))
    }

    pub(super) fn has_corrections(&self) -> Result<bool, EuphoriaError> {
        self.connection
            .query_row("SELECT EXISTS (SELECT 1 FROM corrections)", [], |row| {
                row.get(0)
            })
            .map_err(cache_error(&self.path))
    }

    pub(super) fn correction_of(
        &self,
        content_hash: &ContentHash,
    ) -> Result<Option<Correction>, EuphoriaError> {
        let correction: Option<Correction> = self
            .connection
            .query_row(
                "SELECT key, bpm FROM corrections WHERE content_hash = ?1",
                params![content_hash.as_bytes()],
                |row| {
                    Ok(Correction {
                        key: key_in_column(row, 0)?,
                        bpm: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(cache_error(&self.path))?;
        correction
            .map(|value| value.with_bpm(value.bpm))
            .transpose()
    }
}

fn stored_correction_in_row(row: &Row<'_>) -> rusqlite::Result<StoredCorrection> {
    let analyzer_version: Option<String> = row.get(4)?;
    let detected = match analyzer_version {
        Some(analyzer_version) => Some(DetectedValues {
            key: key_in_column(row, 5)?,
            bpm: tempo_in_column(row, 6)?,
            analyzer_version,
            analyzed_at_seconds: row.get(7)?,
            last_known_path: row.get(8)?,
        }),
        None => None,
    };
    Ok(StoredCorrection {
        content_hash: ContentHash::from_bytes(row.get(0)?),
        correction: Correction {
            key: key_in_column(row, 1)?,
            bpm: row.get(2)?,
        },
        corrected_at_seconds: row.get(3)?,
        detected,
    })
}

pub(super) fn unreadable(path: &Path) -> impl Fn(std::io::Error) -> EuphoriaError {
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
    use crate::analysis::TrackAnalysis;
    use crate::key::{Key, Mode};

    fn analysis() -> TrackAnalysis {
        TrackAnalysis {
            duration_seconds: 120.0,
            key: Some(Key::new(9, Mode::Minor)),
            bpm: Some(124.0),
            energy: Some(7),
            tuning_cents: None,
            key_timeline: Vec::new(),
            quality: None,
            key_runner_up: None,
            key_score_margin: None,
            tempo_estimate: None,
            tempo_score_margin: None,
            beat_grid: None,
            cues: Vec::new(),
        }
    }

    fn open(folder: &TemporaryFolder) -> Cache {
        Cache::open(&folder.path().join("cache.sqlite3")).unwrap()
    }

    fn e_minor() -> Correction {
        Correction::of_key(Key::new(4, Mode::Minor))
            .with_bpm(Some(87.0))
            .unwrap()
    }

    #[test]
    fn invalid_public_tempos_cannot_be_saved_or_read_from_the_cache() {
        let folder = TemporaryFolder::new();
        let track = folder.path().join("invalid-bpm.mp3");
        std::fs::write(&track, b"audio").unwrap();
        let mut cache = open(&folder);
        cache.set_correction(&track, &e_minor()).unwrap();
        for bpm in [f32::NAN, f32::INFINITY, -1.0, 0.0, 1000.0] {
            let invalid = Correction {
                key: None,
                bpm: Some(bpm),
            };
            assert!(matches!(
                cache.set_correction(&track, &invalid),
                Err(EuphoriaError::InvalidCorrection { .. })
            ));
            assert_eq!(
                cache.verified_correction_for(&track).unwrap(),
                Some(e_minor())
            );
        }
        cache
            .connection
            .execute("UPDATE corrections SET bpm = -1", [])
            .unwrap();
        assert!(matches!(
            cache.verified_correction_for(&track),
            Err(EuphoriaError::InvalidCorrection { .. })
        ));
    }

    #[test]
    fn a_correction_is_found_again_by_path_and_by_content_after_a_move() {
        let folder = TemporaryFolder::new();
        let track = folder.path().join("track.mp3");
        std::fs::write(&track, b"audio").unwrap();
        let mut cache = open(&folder);
        cache.record_retagged_file(&track, &analysis()).unwrap();
        assert_eq!(cache.correction_for(&track).unwrap(), None);

        cache.set_correction(&track, &e_minor()).unwrap();
        assert_eq!(
            open(&folder).correction_for(&track).unwrap(),
            Some(e_minor())
        );

        let moved = folder.path().join("moved.mp3");
        std::fs::rename(&track, &moved).unwrap();
        assert_eq!(
            open(&folder).correction_for(&moved).unwrap(),
            Some(e_minor())
        );
    }

    #[test]
    fn an_empty_correction_removes_it() {
        let folder = TemporaryFolder::new();
        let track = folder.path().join("track.mp3");
        std::fs::write(&track, b"audio").unwrap();
        let mut cache = open(&folder);
        cache.set_correction(&track, &e_minor()).unwrap();
        cache
            .set_correction(&track, &Correction::default())
            .unwrap();
        assert_eq!(cache.correction_for(&track).unwrap(), None);
    }

    #[test]
    fn a_new_analyzer_version_keeps_every_correction() {
        let folder = TemporaryFolder::new();
        let track = folder.path().join("track.mp3");
        std::fs::write(&track, b"audio").unwrap();
        let mut cache = open(&folder);
        cache.record_retagged_file(&track, &analysis()).unwrap();
        cache.set_correction(&track, &e_minor()).unwrap();
        // A new analyzer version analyzes every track again, which replaces
        // or drops the analyses; the corrections are another table.
        cache
            .connection
            .execute("DELETE FROM analyses", [])
            .unwrap();
        let reopened = open(&folder);
        assert_eq!(reopened.entry_count().unwrap(), 0);
        assert_eq!(reopened.correction_for(&track).unwrap(), Some(e_minor()));
    }

    #[test]
    fn clearing_the_cache_keeps_the_corrections() {
        let folder = TemporaryFolder::new();
        let track = folder.path().join("track.mp3");
        std::fs::write(&track, b"audio").unwrap();
        let mut cache = open(&folder);
        cache.record_retagged_file(&track, &analysis()).unwrap();
        cache.set_correction(&track, &e_minor()).unwrap();
        drop(cache);
        assert!(super::super::clear_cache(&folder.path().join("cache.sqlite3")).unwrap());
        let reopened = open(&folder);
        assert_eq!(reopened.entry_count().unwrap(), 0);
        assert_eq!(reopened.correction_for(&track).unwrap(), Some(e_minor()));
    }

    #[test]
    fn a_same_size_same_time_replacement_passes_the_quick_check_but_not_the_verified_one() {
        let folder = TemporaryFolder::new();
        let track = folder.path().join("track.mp3");
        std::fs::write(&track, b"audio").unwrap();
        let mut cache = open(&folder);
        cache.record_retagged_file(&track, &analysis()).unwrap();
        cache.set_correction(&track, &e_minor()).unwrap();
        let modified = std::fs::metadata(&track).unwrap().modified().unwrap();

        std::fs::write(&track, b"other").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&track)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert_eq!(
            cache.correction_for(&track).unwrap(),
            Some(e_minor()),
            "the display trusts path, size and time, as the analysis cache does"
        );
        assert_eq!(cache.verified_correction_for(&track).unwrap(), None);
        std::fs::write(&track, b"audio").unwrap();
        assert_eq!(
            cache.verified_correction_for(&track).unwrap(),
            Some(e_minor())
        );
    }

    #[test]
    fn a_failed_carry_of_the_correction_keeps_nothing_of_the_write() {
        let folder = TemporaryFolder::new();
        let track = folder.path().join("track.mp3");
        std::fs::write(&track, b"audio").unwrap();
        let mut cache = open(&folder);
        cache.record_retagged_file(&track, &analysis()).unwrap();
        cache.set_correction(&track, &e_minor()).unwrap();
        let before = cache.recorded_content(&track).unwrap().unwrap();
        cache
            .connection
            .execute_batch(
                "CREATE TRIGGER refuse BEFORE INSERT ON corrections
                 BEGIN SELECT RAISE(ABORT, 'refused'); END;",
            )
            .unwrap();

        std::fs::write(&track, b"audio with a tag").unwrap();
        assert!(cache.record_retagged_file(&track, &analysis()).is_err());
        assert_eq!(cache.recorded_content(&track).unwrap(), None);
        assert_eq!(cache.previous_content(&track).unwrap(), Some(before));
        assert_eq!(cache.entry_count().unwrap(), 1);

        cache
            .connection
            .execute_batch("DROP TRIGGER refuse")
            .unwrap();
        cache.record_retagged_file(&track, &analysis()).unwrap();
        assert_eq!(cache.correction_for(&track).unwrap(), Some(e_minor()));
    }

    #[test]
    fn a_correction_follows_its_file_through_a_tag_write_and_an_undo() {
        let folder = TemporaryFolder::new();
        let track = folder.path().join("track.mp3");
        std::fs::write(&track, b"audio").unwrap();
        let original = std::fs::read(&track).unwrap();
        let mut cache = open(&folder);
        cache.record_retagged_file(&track, &analysis()).unwrap();
        cache.set_correction(&track, &e_minor()).unwrap();

        std::fs::write(&track, b"audio with a tag").unwrap();
        cache.record_retagged_file(&track, &analysis()).unwrap();
        assert_eq!(
            open(&folder).correction_for(&track).unwrap(),
            Some(e_minor())
        );

        std::fs::write(&track, &original).unwrap();
        assert!(cache.recognise_file(&track).unwrap());
        assert_eq!(
            open(&folder).correction_for(&track).unwrap(),
            Some(e_minor())
        );
    }

    #[test]
    fn the_analyzer_reads_and_writes_corrections_too() {
        let folder = TemporaryFolder::new();
        let track = folder.path().join("track.mp3");
        std::fs::write(&track, b"audio").unwrap();
        let analyzer = CachedAnalyzer::new(open(&folder)).unwrap();
        analyzer.set_correction(&track, &e_minor()).unwrap();
        assert_eq!(analyzer.correction_for(&track).unwrap(), Some(e_minor()));
    }

    #[test]
    fn stored_corrections_come_with_what_was_detected_when_an_analysis_is_kept() {
        let folder = TemporaryFolder::new();
        let analyzed = folder.path().join("analyzed.mp3");
        let unanalyzed = folder.path().join("unanalyzed.mp3");
        std::fs::write(&analyzed, b"audio").unwrap();
        std::fs::write(&unanalyzed, b"other audio").unwrap();
        let mut cache = open(&folder);
        assert!(cache.stored_corrections().unwrap().is_empty());
        cache.record_retagged_file(&analyzed, &analysis()).unwrap();
        cache.set_correction(&analyzed, &e_minor()).unwrap();
        cache
            .set_correction(&unanalyzed, &Correction::of_bpm(126.0).unwrap())
            .unwrap();
        cache
            .connection
            .execute(
                "UPDATE corrections SET corrected_at = 5 WHERE content_hash = ?1",
                params![ContentHash::of_file(&unanalyzed).unwrap().as_bytes()],
            )
            .unwrap();

        let stored = cache.stored_corrections().unwrap();
        assert_eq!(stored.len(), 2);
        assert_eq!(
            stored[0].content_hash,
            ContentHash::of_file(&unanalyzed).unwrap(),
            "oldest first"
        );
        assert_eq!(stored[0].correction, Correction::of_bpm(126.0).unwrap());
        assert_eq!(stored[0].corrected_at_seconds, 5);
        assert_eq!(stored[0].detected, None);

        assert_eq!(
            stored[1].content_hash,
            ContentHash::of_file(&analyzed).unwrap()
        );
        assert_eq!(stored[1].correction, e_minor());
        let detected = stored[1].detected.as_ref().unwrap();
        assert_eq!(detected.key, Some(Key::new(9, Mode::Minor)));
        assert_eq!(detected.bpm, Some(124.0));
        assert_eq!(detected.analyzer_version, crate::ANALYZER_VERSION);
        assert_eq!(
            detected.last_known_path.as_deref(),
            std::path::absolute(&analyzed).unwrap().to_str()
        );
    }

    #[test]
    fn a_stored_tempo_that_is_not_valid_fails_the_listing() {
        let folder = TemporaryFolder::new();
        let track = folder.path().join("track.mp3");
        std::fs::write(&track, b"audio").unwrap();
        let mut cache = open(&folder);
        cache.set_correction(&track, &e_minor()).unwrap();
        cache
            .connection
            .execute("UPDATE corrections SET bpm = 0", [])
            .unwrap();
        assert!(matches!(
            cache.stored_corrections(),
            Err(EuphoriaError::InvalidCorrection { .. })
        ));
    }

    #[test]
    fn a_library_without_corrections_never_reads_its_files_for_them() {
        let folder = TemporaryFolder::new();
        let cache = open(&folder);
        let missing = folder.path().join("not here.mp3");
        assert_eq!(cache.correction_for(&missing).unwrap(), None);
    }
}
