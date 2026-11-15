// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The audio fingerprints of tracks, in a table of their own.
//!
//! Kept under the content hash of the file, like corrections, and apart from
//! the analyses, so making them never touches an analysis and no analyzer
//! version bump is needed. A row made by another [`FINGERPRINT_VERSION`] is
//! ignored and made again.

use std::path::{Path, PathBuf};
use std::sync::PoisonError;

use rusqlite::types::Type;
use rusqlite::{OptionalExtension, params};

use super::single::CachedAnalyzer;
use super::{Cache, cache_error, seconds_since_epoch};
use crate::audio::decode_mono;
use crate::error::EuphoriaError;
use crate::fingerprint::{
    FINGERPRINT_VERSION, Fingerprint, FingerprintIndex, STORED_THINNING, fingerprint,
};

/// The most hashes the in-memory index of a library holds, 12 bytes each.
/// A larger library is thinned further, which costs a little recall.
pub const MAXIMUM_INDEX_HASHES: usize = 12_000_000;

impl Cache {
    /// The stored fingerprint of the file at `path`, if one was made by this
    /// version of the fingerprint.
    ///
    /// # Errors
    ///
    /// [`EuphoriaError::Scan`] when the file must be hashed and cannot be
    /// read, and [`EuphoriaError::Cache`] when the database cannot be read.
    pub fn stored_fingerprint(&self, path: &Path) -> Result<Option<Fingerprint>, EuphoriaError> {
        let content_hash = self.content_at(path)?;
        self.connection
            .query_row(
                "SELECT hashes FROM fingerprints WHERE content_hash = ?1 AND version = ?2",
                params![content_hash.as_bytes(), FINGERPRINT_VERSION],
                |row| {
                    let bytes: Vec<u8> = row.get(0)?;
                    Fingerprint::from_bytes(&bytes).ok_or_else(|| {
                        rusqlite::Error::FromSqlConversionFailure(
                            0,
                            Type::Blob,
                            "a fingerprint that is not a whole number of hashes".into(),
                        )
                    })
                },
            )
            .optional()
            .map_err(cache_error(&self.path))
    }

    /// Keeps `fingerprint`, thinned as [`STORED_THINNING`] says, as the
    /// fingerprint of the file at `path`.
    ///
    /// # Errors
    ///
    /// As [`Cache::stored_fingerprint`], for a database that cannot be
    /// written.
    pub fn store_fingerprint(
        &mut self,
        path: &Path,
        fingerprint: &Fingerprint,
    ) -> Result<(), EuphoriaError> {
        let content_hash = self.content_at(path)?;
        self.connection
            .execute(
                "INSERT OR REPLACE INTO fingerprints (content_hash, version, hashes, fingerprinted_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    content_hash.as_bytes(),
                    FINGERPRINT_VERSION,
                    fingerprint.thinned(STORED_THINNING).to_bytes(),
                    seconds_since_epoch()
                ],
            )
            .map(|_| ())
            .map_err(cache_error(&self.path))
    }
}

/// What reading the fingerprints of a library gave.
pub struct LibraryFingerprints {
    /// The hashes of every track, by the track's position in the paths given.
    pub index: FingerprintIndex,
    /// How many tracks were fingerprinted now, the rest came from the cache.
    pub made: usize,
    /// How many tracks could not be read.
    pub failed: usize,
}

impl CachedAnalyzer {
    /// The fingerprint of the file at `path` as the cache stores it, made
    /// first when it has none: the file is decoded once.
    ///
    /// # Errors
    ///
    /// The file's decoding error, or [`EuphoriaError::Cache`].
    pub fn fingerprint_file(&self, path: &Path) -> Result<Fingerprint, EuphoriaError> {
        let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(stored) = cache.stored_fingerprint(path)? {
            return Ok(stored);
        }
        drop(cache);
        let made = fingerprint(&decode_mono(path)?);
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache.store_fingerprint(path, &made)?;
        Ok(made.thinned(STORED_THINNING))
    }

    /// The index of the fingerprints of `paths`, making the missing ones. The
    /// track id of a path is its position in `paths`. `progress` is told how
    /// many paths are done, and the work ends early when `should_stop` says
    /// so, with what was read until then.
    pub fn fingerprint_library(
        &self,
        paths: &[PathBuf],
        progress: &dyn Fn(usize),
        should_stop: &dyn Fn() -> bool,
    ) -> LibraryFingerprints {
        let mut prints: Vec<(usize, Fingerprint)> = Vec::with_capacity(paths.len());
        let (mut made, mut failed) = (0, 0);
        for (id, path) in paths.iter().enumerate() {
            if should_stop() {
                break;
            }
            let was_stored = self
                .cache
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .stored_fingerprint(path)
                .ok()
                .flatten();
            match was_stored {
                Some(stored) => prints.push((id, stored)),
                None => match self.fingerprint_file(path) {
                    Ok(print) => {
                        made += 1;
                        prints.push((id, print));
                    }
                    Err(_) => failed += 1,
                },
            }
            progress(id + 1);
        }
        let total: usize = prints.iter().map(|(_, print)| print.len()).sum();
        let extra_thinning = total.div_ceil(MAXIMUM_INDEX_HASHES).max(1) as u32;
        let mut index = FingerprintIndex::new();
        for (id, print) in &prints {
            if extra_thinning > 1 {
                index.add(*id, &print.thinned_again(extra_thinning));
            } else {
                index.add(*id, print);
            }
        }
        index.freeze();
        LibraryFingerprints {
            index,
            made,
            failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;
    use crate::cache::test_support::TemporaryFolder;

    fn write_tone(path: &Path, seconds: u32, base: f32) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 22_050,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for index in 0..22_050 * seconds {
            let time = index as f32 / 22_050.0;
            let note = base * 2.0_f32.powf(((time * 2.0) as u32 % 7) as f32 / 12.0);
            let value = (std::f32::consts::TAU * note * time).sin()
                + 0.5 * (std::f32::consts::TAU * note * 2.0 * time).sin();
            writer.write_sample((value * 8_000.0) as i16).unwrap();
        }
        writer.finalize().unwrap();
    }

    #[test]
    fn a_fingerprint_is_made_once_stored_and_found_by_its_audio() {
        let folder = TemporaryFolder::new();
        let first = folder.path().join("first.wav");
        let second = folder.path().join("second.wav");
        write_tone(&first, 20, 220.0);
        write_tone(&second, 20, 330.0);
        let cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        let analyzer = CachedAnalyzer::new(cache).unwrap();
        let paths = vec![first.clone(), second.clone()];
        let library = analyzer.fingerprint_library(&paths, &|_| {}, &|| false);
        assert_eq!((library.made, library.failed), (2, 0));
        assert!(library.index.hash_count() > 0);
        let again = analyzer.fingerprint_library(&paths, &|_| {}, &|| false);
        assert_eq!(again.made, 0, "the second pass reads the table");
        assert_eq!(again.index.hash_count(), library.index.hash_count());

        let live = fingerprint(&crate::audio::AudioBuffer::new(
            {
                let audio = decode_mono(&second).unwrap();
                audio.samples()[22_050 * 4..22_050 * 14].to_vec()
            },
            NonZeroU32::new(22_050).unwrap(),
        ));
        let found = again.index.find(&live).expect("the second track");
        assert_eq!(found.track, 1);
        assert!((found.start_seconds - 4.0).abs() < 0.6, "{found:?}");
    }

    #[test]
    fn stopping_ends_the_pass_with_what_was_read() {
        let folder = TemporaryFolder::new();
        let path = folder.path().join("only.wav");
        write_tone(&path, 12, 220.0);
        let cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        let analyzer = CachedAnalyzer::new(cache).unwrap();
        let library = analyzer.fingerprint_library(&[path], &|_| {}, &|| true);
        assert_eq!(library.index.hash_count(), 0);
        assert_eq!(library.made, 0);
    }

    #[test]
    fn a_file_that_cannot_be_decoded_is_counted_and_skipped() {
        let folder = TemporaryFolder::new();
        let broken = folder.path().join("broken.wav");
        std::fs::write(&broken, b"not audio").unwrap();
        let cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        let analyzer = CachedAnalyzer::new(cache).unwrap();
        let library = analyzer.fingerprint_library(&[broken], &|_| {}, &|| false);
        assert_eq!(library.failed, 1);
    }
}
