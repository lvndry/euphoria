// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Playlists the user builds, kept in a small SQLite database of their own
//! beside the analysis cache.
//!
//! A playlist is a name, an optional emoji label and its tracks in order.
//! Each track is kept by the content of its file, as the cache keeps an
//! analysis, together with the path it was last seen at. A file that was
//! moved or renamed is found again where the cache last saw its content, so
//! a playlist outlives a tidy up of the music folders.
//!
//! The playlists live in their own file rather than in the cache: the cache
//! only saves time and may be cleared or deleted to recover from damage,
//! while a playlist is the user's own work.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, TransactionBehavior, params};

use crate::cache::{CachedAnalyzer, ContentHash};
use crate::error::{ErrorSource, EuphoriaError};

const DATABASE_FILE_NAME: &str = "playlists.sqlite3";

const BUSY_TIMEOUT: Duration = Duration::from_secs(10);

const SCHEMA_VERSION: u32 = 1;

const SCHEMA: &str = "
    CREATE TABLE playlists (
        position INTEGER NOT NULL PRIMARY KEY,
        name     TEXT    NOT NULL,
        label    TEXT
    );
    CREATE TABLE playlist_tracks (
        playlist_position INTEGER NOT NULL
            REFERENCES playlists (position) ON DELETE CASCADE,
        position          INTEGER NOT NULL,
        content_hash      BLOB    NOT NULL,
        path              TEXT    NOT NULL,
        PRIMARY KEY (playlist_position, position)
    ) WITHOUT ROWID;
";

/// One track of a playlist: the content of its file and where that file was
/// last seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistEntry {
    /// The hash of the file's content, which identifies the track wherever
    /// the file is.
    pub content: ContentHash,
    /// The absolute path the file was last seen at.
    pub path: PathBuf,
}

impl PlaylistEntry {
    /// The entry for the file at `path`. The content is taken from `cache`
    /// when it recorded the file at that path with the same size and
    /// modification time, and read from the file otherwise.
    ///
    /// # Errors
    ///
    /// Returns [`EuphoriaError::Open`] when the file has to be read and
    /// cannot be.
    pub fn for_file(path: &Path, cache: Option<&CachedAnalyzer>) -> Result<Self, EuphoriaError> {
        let open_error = |source| EuphoriaError::Open {
            path: path.to_path_buf(),
            source,
        };
        let absolute = std::path::absolute(path).map_err(open_error)?;
        let content = match cache.and_then(|cache| cache.known_content(&absolute)) {
            Some(content) => content,
            None => ContentHash::of_file(&absolute).map_err(open_error)?,
        };
        Ok(Self {
            content,
            path: absolute,
        })
    }
}

/// A playlist as it is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPlaylist {
    /// What the user called it.
    pub name: String,
    /// One emoji the user chose to tell it apart, if any.
    pub label: Option<String>,
    /// The tracks in play order.
    pub entries: Vec<PlaylistEntry>,
}

/// Where the playlists live unless the caller chooses another file: beside
/// the analysis cache, in the folder [`default_cache_path`] describes.
///
/// [`default_cache_path`]: crate::cache::default_cache_path
pub fn default_playlists_path() -> Result<PathBuf, EuphoriaError> {
    Ok(crate::cache::default_data_directory()?.join(DATABASE_FILE_NAME))
}

/// The playlists database.
#[derive(Debug)]
pub struct PlaylistStore {
    connection: Connection,
    path: PathBuf,
}

impl PlaylistStore {
    /// Opens the database at `path`, creating it and its folder when missing.
    ///
    /// # Errors
    ///
    /// Returns [`EuphoriaError::Playlists`] naming the file when it cannot
    /// be created, is not a SQLite database, or was written by a newer
    /// Euphoria.
    pub fn open(path: &Path) -> Result<Self, EuphoriaError> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(store_error(path))?;
        }
        let mut connection = Connection::open(path).map_err(store_error(path))?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(store_error(path))?;
        let found = prepare(&mut connection).map_err(store_error(path))?;
        if found > SCHEMA_VERSION {
            return Err(store_error(path)(format!(
                "the playlists have schema version {found} and this version of Euphoria reads up \
                 to {SCHEMA_VERSION}; update Euphoria"
            )));
        }
        Ok(Self {
            connection,
            path: path.to_path_buf(),
        })
    }

    /// The database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every playlist, in the order the user gave them.
    ///
    /// # Errors
    ///
    /// Returns [`EuphoriaError::Playlists`] when the database cannot be
    /// read or holds a damaged row.
    pub fn load(&self) -> Result<Vec<StoredPlaylist>, EuphoriaError> {
        self.read().map_err(store_error(&self.path))
    }

    /// Replaces every playlist with `playlists`, at once: a reader sees the
    /// playlists from before or after, never part of each.
    ///
    /// # Errors
    ///
    /// Returns [`EuphoriaError::Playlists`] when the database cannot be
    /// written.
    pub fn save(&mut self, playlists: &[StoredPlaylist]) -> Result<(), EuphoriaError> {
        let path = self.path.clone();
        self.write(playlists).map_err(store_error(&path))
    }

    /// The playlist called `name`, ignoring case, when there is one.
    ///
    /// # Errors
    ///
    /// The errors of [`PlaylistStore::load`].
    pub fn find(&self, name: &str) -> Result<Option<StoredPlaylist>, EuphoriaError> {
        let wanted = name.trim().to_lowercase();
        Ok(self
            .load()?
            .into_iter()
            .find(|playlist| playlist.name.trim().to_lowercase() == wanted))
    }

    fn read(&self) -> rusqlite::Result<Vec<StoredPlaylist>> {
        let mut playlists_statement = self
            .connection
            .prepare("SELECT position, name, label FROM playlists ORDER BY position")?;
        let mut tracks_statement = self.connection.prepare(
            "SELECT content_hash, path FROM playlist_tracks
             WHERE playlist_position = ?1 ORDER BY position",
        )?;
        let rows: Vec<(i64, String, Option<String>)> = playlists_statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<rusqlite::Result<_>>()?;
        rows.into_iter()
            .map(|(position, name, label)| {
                let entries = tracks_statement
                    .query_map([position], |row| {
                        let bytes: Vec<u8> = row.get(0)?;
                        let content = <[u8; 32]>::try_from(bytes.as_slice())
                            .map(ContentHash::from_bytes)
                            .map_err(|_| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    0,
                                    rusqlite::types::Type::Blob,
                                    "a content hash is 32 bytes".into(),
                                )
                            })?;
                        let path: String = row.get(1)?;
                        Ok(PlaylistEntry {
                            content,
                            path: PathBuf::from(path),
                        })
                    })?
                    .collect::<rusqlite::Result<_>>()?;
                Ok(StoredPlaylist {
                    name,
                    label,
                    entries,
                })
            })
            .collect()
    }

    fn write(&mut self, playlists: &[StoredPlaylist]) -> Result<(), ErrorSource> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("DELETE FROM playlist_tracks", [])?;
        transaction.execute("DELETE FROM playlists", [])?;
        for (position, playlist) in playlists.iter().enumerate() {
            transaction.execute(
                "INSERT INTO playlists (position, name, label) VALUES (?1, ?2, ?3)",
                params![position as i64, playlist.name, playlist.label],
            )?;
            for (track_position, entry) in playlist.entries.iter().enumerate() {
                let path = entry
                    .path
                    .to_str()
                    .ok_or_else(|| format!("{} is not valid UTF-8", entry.path.display()))?;
                transaction.execute(
                    "INSERT INTO playlist_tracks
                         (playlist_position, position, content_hash, path)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![
                        position as i64,
                        track_position as i64,
                        entry.content.as_bytes(),
                        path
                    ],
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }
}

/// How sure to be that a file is the track a playlist holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryCheck {
    /// Trust a file whose path, size and modification time the cache
    /// recorded with the track's content, and a file the cache knows
    /// nothing about at the entry's own path, as the analysis cache does:
    /// enough for showing a playlist.
    Quick,
    /// Read the whole file and compare its content, before the file goes
    /// into an export.
    ReadFile,
}

/// Where the track of a playlist entry is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryLocation {
    /// At the path the entry holds.
    InPlace,
    /// At another path, where the cache last saw its content.
    Moved(PathBuf),
    /// A file is at the entry's path, but it holds other audio, and the
    /// track was found nowhere else.
    Replaced,
    /// No file holds the track any more.
    Gone,
}

impl EntryLocation {
    /// The path of the track, when it was found.
    pub fn path<'entry>(&'entry self, entry: &'entry PlaylistEntry) -> Option<&'entry Path> {
        match self {
            EntryLocation::InPlace => Some(&entry.path),
            EntryLocation::Moved(path) => Some(path),
            EntryLocation::Replaced | EntryLocation::Gone => None,
        }
    }
}

/// Whether the file at `path` holds `content`, as far as `check` asks to
/// be sure. `None` when that cannot be told, which only a quick check with
/// no record of the file allows.
fn holds(
    path: &Path,
    content: &ContentHash,
    cache: Option<&CachedAnalyzer>,
    check: EntryCheck,
) -> Option<bool> {
    if !path.is_file() {
        return Some(false);
    }
    match check {
        EntryCheck::ReadFile => {
            Some(ContentHash::of_file(path).is_ok_and(|found| found == *content))
        }
        EntryCheck::Quick => cache
            .and_then(|cache| cache.known_content(path))
            .map(|known| known == *content),
    }
}

/// Finds the track of `entry`: at its own path when the file there holds
/// its content, or else where `cache` last saw that content, when the file
/// there holds it. With [`EntryCheck::Quick`] a file at the entry's own
/// path that the cache has no record of is taken for the track; with
/// [`EntryCheck::ReadFile`] every file is read and compared.
pub fn locate_entry(
    entry: &PlaylistEntry,
    cache: Option<&CachedAnalyzer>,
    check: EntryCheck,
) -> EntryLocation {
    let in_place = holds(&entry.path, &entry.content, cache, check);
    if in_place != Some(false) {
        return EntryLocation::InPlace;
    }
    let candidate = cache
        .and_then(|cache| cache.last_seen_path_of(&entry.content))
        .filter(|candidate| *candidate != entry.path);
    if let Some(candidate) = candidate
        && holds(&candidate, &entry.content, cache, check) == Some(true)
    {
        return EntryLocation::Moved(candidate);
    }
    if entry.path.is_file() {
        EntryLocation::Replaced
    } else {
        EntryLocation::Gone
    }
}

/// Points each entry whose track moved to where it is now, after checking
/// as `check` asks, and leaves the others as they are. Returns whether any
/// entry moved, which is when the playlists are worth saving again, and the
/// entries whose track was not found, with where it was looked for.
pub fn locate_moved_files(
    playlists: &mut [StoredPlaylist],
    cache: Option<&CachedAnalyzer>,
    check: EntryCheck,
) -> (bool, Vec<(PathBuf, EntryLocation)>) {
    let mut moved = false;
    let mut not_found = Vec::new();
    for entry in playlists
        .iter_mut()
        .flat_map(|playlist| playlist.entries.iter_mut())
    {
        match locate_entry(entry, cache, check) {
            EntryLocation::InPlace => {}
            EntryLocation::Moved(path) => {
                entry.path = path;
                moved = true;
            }
            missing => not_found.push((entry.path.clone(), missing)),
        }
    }
    (moved, not_found)
}

fn prepare(connection: &mut Connection) -> rusqlite::Result<u32> {
    connection.pragma_update(None, "foreign_keys", "ON")?;
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let applied: u32 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if applied == 0 {
        transaction.execute_batch(SCHEMA)?;
        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    transaction.commit()?;
    Ok(applied.max(SCHEMA_VERSION))
}

fn store_error<Cause: Into<ErrorSource>>(path: &Path) -> impl Fn(Cause) -> EuphoriaError {
    move |cause| EuphoriaError::Playlists {
        path: path.to_path_buf(),
        source: cause.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::Cache;
    use crate::cache::test_support::TemporaryFolder;

    fn entry(folder: &TemporaryFolder, name: &str, content: &str) -> PlaylistEntry {
        let path = folder.path().join(name);
        std::fs::write(&path, content).unwrap();
        PlaylistEntry::for_file(&path, None).unwrap()
    }

    fn store(folder: &TemporaryFolder) -> PlaylistStore {
        PlaylistStore::open(&folder.path().join("data").join(DATABASE_FILE_NAME)).unwrap()
    }

    #[test]
    fn playlists_come_back_in_order_with_their_labels_and_tracks() {
        let folder = TemporaryFolder::new();
        let first = entry(&folder, "first.mp3", "one");
        let second = entry(&folder, "second.mp3", "two");
        let playlists = vec![
            StoredPlaylist {
                name: "Peak time".to_string(),
                label: Some("\u{1f525}".to_string()),
                entries: vec![second.clone(), first.clone()],
            },
            StoredPlaylist {
                name: "Warmup".to_string(),
                label: None,
                entries: Vec::new(),
            },
        ];
        store(&folder).save(&playlists).unwrap();
        assert_eq!(store(&folder).load().unwrap(), playlists);

        let mut store = store(&folder);
        store.save(&playlists[1..]).unwrap();
        assert_eq!(store.load().unwrap(), playlists[1..]);
        assert_eq!(store.find(" warmup ").unwrap().unwrap().name, "Warmup");
        assert_eq!(store.find("peak time").unwrap(), None);
    }

    #[test]
    fn an_entry_is_the_content_of_its_file() {
        let folder = TemporaryFolder::new();
        let one = entry(&folder, "a.mp3", "same");
        let other = entry(&folder, "b.mp3", "same");
        let different = entry(&folder, "c.mp3", "different");
        assert_eq!(one.content, other.content);
        assert_ne!(one.content, different.content);
        assert!(one.path.is_absolute());
        assert!(PlaylistEntry::for_file(&folder.path().join("gone.mp3"), None).is_err());
    }

    #[test]
    fn a_moved_file_is_found_where_the_cache_last_saw_it() {
        let folder = TemporaryFolder::new();
        let mut cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        let original = folder.path().join("track.mp3");
        std::fs::write(&original, b"audio").unwrap();
        let analysis = crate::analysis::TrackAnalysis {
            duration_seconds: 60.0,
            key: None,
            bpm: None,
            energy: None,
            tuning_cents: None,
            key_timeline: Vec::new(),
            quality: None,
            key_runner_up: None,
            key_score_margin: None,
            tempo_estimate: None,
            tempo_score_margin: None,
            beat_grid: None,
            cues: Vec::new(),
        };
        cache.record_retagged_file(&original, &analysis).unwrap();
        let mut playlists = vec![StoredPlaylist {
            name: "Set".to_string(),
            label: None,
            entries: vec![PlaylistEntry::for_file(&original, None).unwrap()],
        }];

        let moved = folder.path().join("renamed.mp3");
        std::fs::rename(&original, &moved).unwrap();
        cache.recognise_file(&moved).unwrap();
        let analyzer = CachedAnalyzer::new(cache).unwrap();
        for check in [EntryCheck::Quick, EntryCheck::ReadFile] {
            let mut copy = playlists.clone();
            let (has_moved, not_found) = locate_moved_files(&mut copy, Some(&analyzer), check);
            assert!(has_moved && not_found.is_empty(), "{check:?}");
            assert_eq!(copy[0].entries[0].path, moved);
        }
        locate_moved_files(&mut playlists, Some(&analyzer), EntryCheck::ReadFile);
        assert_eq!(
            locate_moved_files(&mut playlists, Some(&analyzer), EntryCheck::ReadFile),
            (false, Vec::new())
        );
        assert_eq!(
            PlaylistEntry::for_file(&moved, Some(&analyzer))
                .unwrap()
                .content,
            playlists[0].entries[0].content
        );
    }

    #[test]
    fn a_file_replaced_at_the_same_path_is_not_taken_for_the_track() {
        let folder = TemporaryFolder::new();
        let mut cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        let path = folder.path().join("track.mp3");
        std::fs::write(&path, b"the track").unwrap();
        let entry = PlaylistEntry::for_file(&path, None).unwrap();
        cache
            .record_retagged_file(&path, &silent_analysis())
            .unwrap();

        std::fs::write(&path, b"other one").unwrap();
        let analyzer = CachedAnalyzer::new(cache).unwrap();
        assert_eq!(
            locate_entry(&entry, Some(&analyzer), EntryCheck::ReadFile),
            EntryLocation::Replaced
        );
        let mut playlists = vec![StoredPlaylist {
            name: "Set".to_string(),
            label: None,
            entries: vec![entry.clone()],
        }];
        let (moved, not_found) =
            locate_moved_files(&mut playlists, Some(&analyzer), EntryCheck::ReadFile);
        assert!(!moved);
        assert_eq!(not_found, [(path.clone(), EntryLocation::Replaced)]);
        assert_eq!(
            playlists[0].entries[0], entry,
            "the entry is left as it was"
        );

        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            locate_entry(&entry, Some(&analyzer), EntryCheck::Quick),
            EntryLocation::Gone
        );
        std::fs::write(&path, b"the track").unwrap();
        assert_eq!(
            locate_entry(&entry, None, EntryCheck::ReadFile),
            EntryLocation::InPlace
        );
    }

    #[test]
    fn a_reused_path_does_not_pass_for_a_moved_track() {
        let folder = TemporaryFolder::new();
        let mut cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        let original = folder.path().join("track.mp3");
        std::fs::write(&original, b"the track").unwrap();
        let entry = PlaylistEntry::for_file(&original, None).unwrap();
        let moved = folder.path().join("moved.mp3");
        std::fs::rename(&original, &moved).unwrap();
        cache.recognise_file(&moved).unwrap();
        cache
            .record_retagged_file(&moved, &silent_analysis())
            .unwrap();
        std::fs::write(&moved, b"something else").unwrap();
        let analyzer = CachedAnalyzer::new(cache).unwrap();
        for check in [EntryCheck::Quick, EntryCheck::ReadFile] {
            assert_eq!(
                locate_entry(&entry, Some(&analyzer), check),
                EntryLocation::Gone,
                "{check:?}: the file the cache last saw now holds other audio"
            );
        }
    }

    fn silent_analysis() -> crate::analysis::TrackAnalysis {
        crate::analysis::TrackAnalysis {
            duration_seconds: 60.0,
            key: None,
            bpm: None,
            energy: None,
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

    #[test]
    fn a_file_that_is_not_a_database_is_an_error_naming_it() {
        let folder = TemporaryFolder::new();
        let path = folder.path().join(DATABASE_FILE_NAME);
        std::fs::write(&path, b"not a database at all, just some text").unwrap();
        let error = PlaylistStore::open(&path).unwrap_err();
        assert!(error.to_string().contains(DATABASE_FILE_NAME), "{error}");
    }
}
