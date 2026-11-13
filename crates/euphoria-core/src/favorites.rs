// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Favorites the user marks: tracks they starred, and transitions, a track
//! into another that they found mixes well, each with an optional note.
//!
//! Favorites are the user's own work, so they live in the playlists
//! database rather than in the analysis cache, which may be cleared. Each
//! track is kept by the content of its file and the path it was last seen
//! at, as a playlist entry is, so a favorite outlives a move or a rename
//! and follows a tag write.

use std::path::{Path, PathBuf};

use rusqlite::{Transaction, TransactionBehavior, params};

use crate::cache::{CachedAnalyzer, ContentHash};
use crate::error::{ErrorSource, EuphoriaError};
use crate::playlists::{
    EntryCheck, EntryLocation, PlaylistEntry, PlaylistStore, locate_entry, store_error,
};

/// The schema change that adds the favorites to the playlists database.
pub(crate) const FAVORITES_MIGRATION: &str = "
    CREATE TABLE favorite_tracks (
        position     INTEGER NOT NULL PRIMARY KEY,
        content_hash BLOB    NOT NULL,
        path         TEXT    NOT NULL
    );
    CREATE TABLE favorite_transitions (
        position          INTEGER NOT NULL PRIMARY KEY,
        from_content_hash BLOB    NOT NULL,
        from_path         TEXT    NOT NULL,
        to_content_hash   BLOB    NOT NULL,
        to_path           TEXT    NOT NULL,
        note              TEXT
    );
";

/// A transition the user marked: playing `to` after `from` mixes well.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FavoriteTransition {
    /// The track playing out.
    pub from: PlaylistEntry,
    /// The track mixed in after it.
    pub to: PlaylistEntry,
    /// What the user wrote about the mix, such as where to bring it in.
    pub note: Option<String>,
}

/// Every favorite, in the order the user marked them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredFavorites {
    /// The starred tracks.
    pub tracks: Vec<PlaylistEntry>,
    /// The marked transitions.
    pub transitions: Vec<FavoriteTransition>,
}

impl StoredFavorites {
    /// Every entry, the starred tracks first and then both sides of each
    /// transition.
    pub fn entries_mut(&mut self) -> impl Iterator<Item = &mut PlaylistEntry> {
        self.tracks.iter_mut().chain(
            self.transitions
                .iter_mut()
                .flat_map(|transition| [&mut transition.from, &mut transition.to]),
        )
    }

    /// Points each entry whose track moved to where it is now, as
    /// [`locate_moved_files`] does for playlists, and returns whether any
    /// entry moved.
    ///
    /// [`locate_moved_files`]: crate::playlists::locate_moved_files
    pub fn locate_moved_files(
        &mut self,
        cache: Option<&CachedAnalyzer>,
        check: EntryCheck,
    ) -> bool {
        let mut moved = false;
        for entry in self.entries_mut() {
            if let EntryLocation::Moved(path) = locate_entry(entry, cache, check) {
                entry.path = path;
                moved = true;
            }
        }
        moved
    }
}

impl PlaylistStore {
    /// Every favorite, in the order the user marked them.
    ///
    /// # Errors
    ///
    /// Returns [`EuphoriaError::Playlists`] when the database cannot be
    /// read or holds a damaged row.
    pub fn load_favorites(&self) -> Result<StoredFavorites, EuphoriaError> {
        read_favorites(&self.connection).map_err(store_error(&self.path))
    }

    /// Replaces every favorite with `favorites`, at once.
    ///
    /// # Errors
    ///
    /// Returns [`EuphoriaError::Playlists`] when the database cannot be
    /// written.
    pub fn save_favorites(&mut self, favorites: &StoredFavorites) -> Result<(), EuphoriaError> {
        let path = self.path.clone();
        write_favorites(&mut self.connection, favorites).map_err(store_error(&path))
    }
}

fn read_favorites(connection: &rusqlite::Connection) -> rusqlite::Result<StoredFavorites> {
    let tracks = connection
        .prepare("SELECT content_hash, path FROM favorite_tracks ORDER BY position")?
        .query_map([], |row| entry_from(row, 0, 1))?
        .collect::<rusqlite::Result<_>>()?;
    let transitions = connection
        .prepare(
            "SELECT from_content_hash, from_path, to_content_hash, to_path, note
             FROM favorite_transitions ORDER BY position",
        )?
        .query_map([], |row| {
            Ok(FavoriteTransition {
                from: entry_from(row, 0, 1)?,
                to: entry_from(row, 2, 3)?,
                note: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(StoredFavorites {
        tracks,
        transitions,
    })
}

fn entry_from(
    row: &rusqlite::Row<'_>,
    content_column: usize,
    path_column: usize,
) -> rusqlite::Result<PlaylistEntry> {
    let bytes: Vec<u8> = row.get(content_column)?;
    let content = <[u8; 32]>::try_from(bytes.as_slice())
        .map(ContentHash::from_bytes)
        .map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                content_column,
                rusqlite::types::Type::Blob,
                "a content hash is 32 bytes".into(),
            )
        })?;
    let path: String = row.get(path_column)?;
    Ok(PlaylistEntry {
        content,
        path: PathBuf::from(path),
    })
}

fn utf8_path(path: &Path) -> Result<&str, ErrorSource> {
    path.to_str()
        .ok_or_else(|| format!("{} is not valid UTF-8", path.display()).into())
}

fn write_favorites(
    connection: &mut rusqlite::Connection,
    favorites: &StoredFavorites,
) -> Result<(), ErrorSource> {
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute("DELETE FROM favorite_tracks", [])?;
    transaction.execute("DELETE FROM favorite_transitions", [])?;
    for (position, entry) in favorites.tracks.iter().enumerate() {
        transaction.execute(
            "INSERT INTO favorite_tracks (position, content_hash, path) VALUES (?1, ?2, ?3)",
            params![
                position as i64,
                entry.content.as_bytes(),
                utf8_path(&entry.path)?
            ],
        )?;
    }
    for (position, transition) in favorites.transitions.iter().enumerate() {
        transaction.execute(
            "INSERT INTO favorite_transitions
                 (position, from_content_hash, from_path, to_content_hash, to_path, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                position as i64,
                transition.from.content.as_bytes(),
                utf8_path(&transition.from.path)?,
                transition.to.content.as_bytes(),
                utf8_path(&transition.to.path)?,
                transition.note,
            ],
        )?;
    }
    transaction.commit()?;
    Ok(())
}

/// Follows a known safe metadata rewrite of the file at `path` in the
/// favorites, as [`PlaylistStore::rebind_rewritten_file`] does in the
/// playlists, inside its transaction. Returns each entry rebound, before
/// and after.
pub(crate) fn rebind_rewritten_favorites(
    transaction: &Transaction<'_>,
    path: &Path,
    before: ContentHash,
    after: ContentHash,
) -> Result<Vec<(PlaylistEntry, PlaylistEntry)>, ErrorSource> {
    let mut favorites = read_favorites(transaction)?;
    let mut changed = Vec::new();
    for entry in favorites.entries_mut() {
        let original = entry.clone();
        if entry.rebind_rewritten_file(path, before, after) {
            changed.push((original, entry.clone()));
        }
    }
    if changed.is_empty() {
        return Ok(changed);
    }
    for (position, entry) in favorites.tracks.iter().enumerate() {
        transaction.execute(
            "UPDATE favorite_tracks SET content_hash = ?1, path = ?2 WHERE position = ?3",
            params![
                entry.content.as_bytes(),
                utf8_path(&entry.path)?,
                position as i64
            ],
        )?;
    }
    for (position, transition) in favorites.transitions.iter().enumerate() {
        transaction.execute(
            "UPDATE favorite_transitions
             SET from_content_hash = ?1, from_path = ?2, to_content_hash = ?3, to_path = ?4
             WHERE position = ?5",
            params![
                transition.from.content.as_bytes(),
                utf8_path(&transition.from.path)?,
                transition.to.content.as_bytes(),
                utf8_path(&transition.to.path)?,
                position as i64
            ],
        )?;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoredPlaylist;
    use crate::cache::test_support::TemporaryFolder;

    fn entry(folder: &TemporaryFolder, name: &str, content: &str) -> PlaylistEntry {
        let path = folder.path().join(name);
        std::fs::write(&path, content).unwrap();
        PlaylistEntry::for_file(&path, None).unwrap()
    }

    fn store_path(folder: &TemporaryFolder) -> PathBuf {
        folder.path().join("data").join("playlists.sqlite3")
    }

    #[test]
    fn favorites_come_back_in_order_beside_the_playlists() {
        let folder = TemporaryFolder::new();
        let first = entry(&folder, "first.mp3", "one");
        let second = entry(&folder, "second.mp3", "two");
        let favorites = StoredFavorites {
            tracks: vec![second.clone(), first.clone()],
            transitions: vec![FavoriteTransition {
                from: first.clone(),
                to: second.clone(),
                note: Some("bring the bass in on the drop".to_string()),
            }],
        };
        let playlist = StoredPlaylist {
            name: "Set".to_string(),
            label: None,
            entries: vec![first.clone()],
        };
        let mut store = PlaylistStore::open(&store_path(&folder)).unwrap();
        store.save(std::slice::from_ref(&playlist)).unwrap();
        store.save_favorites(&favorites).unwrap();
        drop(store);

        let store = PlaylistStore::open(&store_path(&folder)).unwrap();
        assert_eq!(store.load_favorites().unwrap(), favorites);
        assert_eq!(store.load().unwrap(), [playlist]);
    }

    #[test]
    fn a_database_of_version_one_gains_empty_favorites_and_keeps_its_playlists() {
        let folder = TemporaryFolder::new();
        let path = store_path(&folder);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(crate::playlists::first_migration_for_tests())
            .unwrap();
        connection
            .execute(
                "INSERT INTO playlists (position, name, label) VALUES (0, 'Old', NULL)",
                [],
            )
            .unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();
        drop(connection);

        let mut store = PlaylistStore::open(&path).unwrap();
        assert_eq!(store.load().unwrap()[0].name, "Old");
        assert_eq!(store.load_favorites().unwrap(), StoredFavorites::default());
        let favorite = entry(&folder, "track.mp3", "audio");
        store
            .save_favorites(&StoredFavorites {
                tracks: vec![favorite.clone()],
                transitions: Vec::new(),
            })
            .unwrap();
        assert_eq!(store.load_favorites().unwrap().tracks, [favorite]);
        let version: u32 = store
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);
    }

    #[test]
    fn a_tag_write_moves_starred_tracks_and_both_sides_of_a_transition() {
        let folder = TemporaryFolder::new();
        let written = entry(&folder, "written.mp3", "before");
        let other = entry(&folder, "other.mp3", "other");
        let mut store = PlaylistStore::open(&store_path(&folder)).unwrap();
        store
            .save_favorites(&StoredFavorites {
                tracks: vec![written.clone(), other.clone()],
                transitions: vec![
                    FavoriteTransition {
                        from: written.clone(),
                        to: other.clone(),
                        note: None,
                    },
                    FavoriteTransition {
                        from: other.clone(),
                        to: written.clone(),
                        note: Some("kept".to_string()),
                    },
                ],
            })
            .unwrap();
        std::fs::write(&written.path, "after").unwrap();
        let after = ContentHash::of_file(&written.path).unwrap();

        let changed = store
            .rebind_rewritten_file(&written.path, written.content, after)
            .unwrap();
        assert_eq!(changed.len(), 3);
        let favorites = store.load_favorites().unwrap();
        assert_eq!(favorites.tracks[0].content, after);
        assert_eq!(favorites.tracks[1], other);
        assert_eq!(favorites.transitions[0].from.content, after);
        assert_eq!(favorites.transitions[0].to, other);
        assert_eq!(favorites.transitions[1].to.content, after);
        assert_eq!(favorites.transitions[1].note.as_deref(), Some("kept"));
    }
}
