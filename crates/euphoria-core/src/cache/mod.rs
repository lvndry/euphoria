// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A local SQLite database of finished analyses, so a track is analyzed once.
//!
//! # How a file is recognised
//!
//! An analysis is stored under the BLAKE3 hash of the whole file, which makes
//! a moved or renamed file the same track as before. Hashing reads every
//! byte, so each analysis also records the path, size and modification time
//! the file had. A file found at its recorded path with the recorded size and
//! modification time is served without being read at all. Anything else is
//! hashed, and analyzed only when the hash is unknown.
//!
//! Trade offs of this scheme:
//!
//! - The whole file is hashed, not a sample such as its first and last
//!   megabyte. Sampling would make a moved library faster to recognise, but
//!   two different files that agree on the sampled parts and on size would
//!   share one analysis. Stems bounced from one project do exactly that:
//!   equal length, silence at both ends. Hashing costs a small fraction of
//!   analyzing, and only runs for files that are new, moved or modified.
//! - The hash covers tags and artwork as well as audio. Finding the audio
//!   inside every supported container is a parser per format, and a mistake
//!   in one would serve a wrong result. The consequence is that a file whose
//!   tags were edited is analyzed once more.
//! - The quick check trusts the file system. A file rewritten in place with
//!   the same size and the same modification time keeps its old analysis.
//! - One analysis remembers one path. Byte for byte copies of a track share
//!   an analysis, and all but one of them are hashed again on every run.
//! - A path that is not valid Unicode is not recorded, so such a file is
//!   hashed on every run.
//!
//! # Analyzer versions
//!
//! Each analysis records the [`ANALYZER_VERSION`] that produced it. Analyses
//! from any other version are never served, and are replaced when their file
//! is analyzed again.

mod batch;
mod identity;
mod location;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::types::Type;
use rusqlite::{Connection, Row, params};

pub use batch::{CachePolicy, CachedBatch, analyze_files_cached};
pub use identity::ContentHash;
pub use location::{DATA_DIRECTORY_VARIABLE, default_cache_path};

use crate::analysis::{ANALYZER_VERSION, TrackAnalysis};
use crate::error::{ErrorSource, EuphoriaError};
use crate::key::Key;
use identity::FileStamp;

/// One entry per schema version, each bringing a database from the version
/// before it. `PRAGMA user_version` holds how many have been applied, so a
/// schema change is a new entry at the end and never an edit to an old one.
const MIGRATIONS: [&str; 1] = ["
    CREATE TABLE analyses (
        content_hash     BLOB    NOT NULL PRIMARY KEY,
        duration_seconds REAL    NOT NULL,
        key              TEXT,
        bpm              REAL,
        analyzer_version TEXT    NOT NULL,
        analyzed_at      INTEGER NOT NULL,
        last_known_path  TEXT,
        file_size        INTEGER NOT NULL,
        modified_at      INTEGER NOT NULL
    ) WITHOUT ROWID;
    CREATE UNIQUE INDEX analyses_last_known_path ON analyses (last_known_path);
"];

const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// The analysis cache: an open SQLite database.
///
/// In its `analyses` table, `key` is a key name such as "A minor",
/// `analyzed_at` is in seconds and `modified_at` in nanoseconds since the Unix
/// epoch, and `last_known_path` is absolute and belongs to at most one row.
#[derive(Debug)]
pub struct Cache {
    connection: Connection,
    path: PathBuf,
    analyzer_version: String,
}

/// A change to the database, sent by analysis workers to the one thread that
/// writes.
#[derive(Debug)]
enum CacheWrite {
    /// Record a new analysis, replacing any stored for the same content.
    Store {
        content_hash: ContentHash,
        analysis: TrackAnalysis,
        location: FileLocation,
    },
    /// Record where known content is now.
    Relocate {
        content_hash: ContentHash,
        location: FileLocation,
    },
}

/// Where a file was seen and what the file system said about it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileLocation {
    absolute_path: PathBuf,
    stamp: FileStamp,
}

/// Every analysis of the current analyzer version, read once so that workers
/// can look files up without touching the database.
#[derive(Debug, Default)]
struct Snapshot {
    analyses: HashMap<ContentHash, TrackAnalysis>,
    locations: HashMap<PathBuf, (FileStamp, ContentHash)>,
}

impl Snapshot {
    /// The analysis of the file at `location`, when the path, size and
    /// modification time are the ones recorded with it.
    fn at_location(&self, location: &FileLocation) -> Option<&TrackAnalysis> {
        let (stamp, content_hash) = self.locations.get(&location.absolute_path)?;
        if *stamp != location.stamp {
            return None;
        }
        self.analyses.get(content_hash)
    }

    fn of_content(&self, content_hash: &ContentHash) -> Option<&TrackAnalysis> {
        self.analyses.get(content_hash)
    }
}

impl Cache {
    /// Opens the database at `path`, creating it and its folder when missing
    /// and bringing an older schema up to date.
    ///
    /// Fails with [`EuphoriaError::Cache`] naming the file when it cannot be
    /// created, is not a SQLite database, or was written by a newer Euphoria.
    pub fn open(path: &Path) -> Result<Self, EuphoriaError> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(cache_error(path))?;
        }
        let mut connection = Connection::open(path).map_err(cache_error(path))?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .and_then(|()| connection.pragma_update(None, "synchronous", "NORMAL"))
            .map_err(cache_error(path))?;
        migrate(&mut connection, path)?;
        Ok(Self {
            connection,
            path: path.to_path_buf(),
            analyzer_version: ANALYZER_VERSION.to_string(),
        })
    }

    /// The database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Number of analyses stored, of every analyzer version.
    pub fn entry_count(&self) -> Result<u64, EuphoriaError> {
        self.connection
            .query_row("SELECT COUNT(*) FROM analyses", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(i64::unsigned_abs)
            .map_err(cache_error(&self.path))
    }

    fn snapshot(&self) -> Result<Snapshot, EuphoriaError> {
        self.read_snapshot().map_err(cache_error(&self.path))
    }

    fn read_snapshot(&self) -> rusqlite::Result<Snapshot> {
        let mut statement = self.connection.prepare(
            "SELECT content_hash, duration_seconds, key, bpm,
                    last_known_path, file_size, modified_at
             FROM analyses WHERE analyzer_version = ?1",
        )?;
        let mut rows = statement.query([&self.analyzer_version])?;
        let mut snapshot = Snapshot::default();
        while let Some(row) = rows.next()? {
            let content_hash = ContentHash::from_bytes(row.get(0)?);
            snapshot.analyses.insert(
                content_hash,
                TrackAnalysis {
                    duration_seconds: row.get(1)?,
                    key: key_in_column(row, 2)?,
                    bpm: row.get(3)?,
                },
            );
            if let Some(last_known_path) = row.get::<_, Option<String>>(4)? {
                let stamp = FileStamp {
                    size: row.get(5)?,
                    modified_nanoseconds: row.get(6)?,
                };
                snapshot
                    .locations
                    .insert(PathBuf::from(last_known_path), (stamp, content_hash));
            }
        }
        Ok(snapshot)
    }

    /// Applies writes as they arrive, until every sender is gone or a write
    /// fails. Returning drops the receiver, which tells senders to stop.
    fn apply_writes(&mut self, writes: Receiver<CacheWrite>) -> Result<(), EuphoriaError> {
        for write in writes {
            self.apply_write(&write).map_err(cache_error(&self.path))?;
        }
        Ok(())
    }

    fn apply_write(&mut self, write: &CacheWrite) -> rusqlite::Result<()> {
        let (content_hash, location) = match write {
            CacheWrite::Store {
                content_hash,
                location,
                ..
            }
            | CacheWrite::Relocate {
                content_hash,
                location,
            } => (content_hash, location),
        };
        let last_known_path = location.absolute_path.to_str();
        let transaction = self.connection.transaction()?;
        if let Some(last_known_path) = last_known_path {
            transaction.execute(
                "UPDATE analyses SET last_known_path = NULL
                 WHERE last_known_path = ?1 AND content_hash <> ?2",
                params![last_known_path, content_hash.as_bytes()],
            )?;
        }
        match write {
            CacheWrite::Store { analysis, .. } => transaction.execute(
                "INSERT OR REPLACE INTO analyses (
                     content_hash, duration_seconds, key, bpm, analyzer_version,
                     analyzed_at, last_known_path, file_size, modified_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    content_hash.as_bytes(),
                    analysis.duration_seconds,
                    analysis.key.map(|key| key.to_string()),
                    analysis.bpm,
                    self.analyzer_version,
                    seconds_since_epoch(),
                    last_known_path,
                    location.stamp.size,
                    location.stamp.modified_nanoseconds,
                ],
            )?,
            CacheWrite::Relocate { .. } => transaction.execute(
                "UPDATE analyses
                 SET last_known_path = ?2, file_size = ?3, modified_at = ?4
                 WHERE content_hash = ?1",
                params![
                    content_hash.as_bytes(),
                    last_known_path,
                    location.stamp.size,
                    location.stamp.modified_nanoseconds,
                ],
            )?,
        };
        transaction.commit()
    }
}

/// Deletes the database at `path` so the next run starts empty. Returns
/// whether there was a database to delete.
///
/// The file is removed, not emptied, so this also recovers from a database
/// that can no longer be opened.
pub fn clear_cache(path: &Path) -> Result<bool, EuphoriaError> {
    let database_existed = remove_if_present(path).map_err(cache_error(path))?;
    for suffix in ["-wal", "-shm"] {
        let mut companion = path.as_os_str().to_owned();
        companion.push(suffix);
        remove_if_present(Path::new(&companion)).map_err(cache_error(path))?;
    }
    Ok(database_existed)
}

fn remove_if_present(path: &Path) -> std::io::Result<bool> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn migrate(connection: &mut Connection, path: &Path) -> Result<(), EuphoriaError> {
    let applied: u32 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(cache_error(path))?;
    if applied > SCHEMA_VERSION {
        return Err(cache_error(path)(format!(
            "the cache has schema version {applied} and this version of Euphoria reads up to \
             {SCHEMA_VERSION}; update Euphoria or clear the cache"
        )));
    }
    for (version, migration) in (1..=SCHEMA_VERSION).zip(MIGRATIONS).skip(applied as usize) {
        let transaction = connection.transaction().map_err(cache_error(path))?;
        transaction
            .execute_batch(migration)
            .and_then(|()| transaction.pragma_update(None, "user_version", version))
            .and_then(|()| transaction.commit())
            .map_err(cache_error(path))?;
    }
    Ok(())
}

fn key_in_column(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<Key>> {
    row.get::<_, Option<String>>(column)?
        .map(|name| name.parse())
        .transpose()
        .map_err(|error: EuphoriaError| {
            rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(error))
        })
}

fn seconds_since_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn cache_error<Cause: Into<ErrorSource>>(path: &Path) -> impl Fn(Cause) -> EuphoriaError {
    move |cause| EuphoriaError::Cache {
        path: path.to_path_buf(),
        source: cause.into(),
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A folder under the system's temporary folder, deleted when dropped.
    pub(crate) struct TemporaryFolder(PathBuf);

    impl TemporaryFolder {
        pub(crate) fn new() -> Self {
            static CREATED: AtomicUsize = AtomicUsize::new(0);
            let folder = std::env::temp_dir().join(format!(
                "euphoria-cache-test-{}-{}",
                std::process::id(),
                CREATED.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&folder).unwrap();
            Self(folder.canonicalize().unwrap())
        }

        pub(crate) fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TemporaryFolder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::TemporaryFolder;
    use super::*;

    #[test]
    fn opening_creates_the_folder_and_an_empty_database() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("nested").join("cache.sqlite3");

        let cache = Cache::open(&database).unwrap();

        assert_eq!(cache.path(), database);
        assert_eq!(cache.entry_count().unwrap(), 0);
        let schema_version: u32 = cache
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn reopening_keeps_what_was_stored() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        let track = folder.path().join("track.wav");
        std::fs::write(&track, b"audio").unwrap();
        let location = FileLocation {
            absolute_path: track.clone(),
            stamp: FileStamp::of(&track).unwrap(),
        };
        let analysis = TrackAnalysis {
            duration_seconds: 301.25,
            key: Some("F# minor".parse().unwrap()),
            bpm: Some(127.98),
        };

        let mut cache = Cache::open(&database).unwrap();
        cache
            .apply_write(&CacheWrite::Store {
                content_hash: ContentHash::of_file(&track).unwrap(),
                analysis: analysis.clone(),
                location: location.clone(),
            })
            .unwrap();
        drop(cache);

        let snapshot = Cache::open(&database).unwrap().snapshot().unwrap();
        assert_eq!(snapshot.at_location(&location), Some(&analysis));
    }

    #[test]
    fn a_file_that_is_not_a_database_is_an_error_naming_it() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        std::fs::write(&database, "this is not a database, ".repeat(400)).unwrap();

        let error = Cache::open(&database).unwrap_err();

        assert!(matches!(&error, EuphoriaError::Cache { path, .. } if *path == database));
        let description = error.chain();
        assert!(
            description.contains(database.to_str().unwrap()),
            "{description}"
        );
        assert!(description.contains("not a database"), "{description}");
    }

    #[test]
    fn a_database_that_cannot_be_opened_is_an_error_naming_it() {
        let folder = TemporaryFolder::new();

        let error = Cache::open(folder.path()).unwrap_err();

        assert!(matches!(&error, EuphoriaError::Cache { path, .. } if path == folder.path()));
    }

    #[test]
    fn a_damaged_row_is_an_error_and_not_a_panic() {
        let folder = TemporaryFolder::new();
        let cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        cache
            .connection
            .execute(
                "INSERT INTO analyses VALUES (?1, 1.0, 'H dorian', NULL, ?2, 0, NULL, 1, 1)",
                params![[7u8; 32], ANALYZER_VERSION],
            )
            .unwrap();

        let description = cache.snapshot().unwrap_err().chain();

        assert!(description.contains("H dorian"), "{description}");
    }

    #[test]
    fn a_database_from_a_newer_euphoria_is_refused() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        Cache::open(&database)
            .unwrap()
            .connection
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();

        let description = Cache::open(&database).unwrap_err().chain();

        assert!(description.contains("schema version"), "{description}");
    }

    #[test]
    fn clearing_deletes_the_database_and_reports_whether_one_existed() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        drop(Cache::open(&database).unwrap());

        assert!(clear_cache(&database).unwrap());
        assert!(!database.exists());
        assert!(!clear_cache(&database).unwrap());
    }
}
