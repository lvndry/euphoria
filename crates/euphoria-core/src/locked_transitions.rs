// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Transitions the user placed by hand in the planner: where one track
//! blends into another. Planning a mix again keeps them as they are.
//!
//! They are the user's own work, so they live in the playlists database
//! beside the favorites. Each side is kept by the content of its file and
//! the path it was last seen at, and a lock applies to a pair when either
//! matches, so it outlives a move and a tag write.

use std::path::Path;

use rusqlite::{TransactionBehavior, params};

use crate::cache::ContentHash;
use crate::error::{ErrorSource, EuphoriaError};
use crate::playlists::{PlaylistEntry, PlaylistStore, store_error};

/// The schema change that adds the locked transitions.
pub(crate) const LOCKED_TRANSITIONS_MIGRATION: &str = "
    CREATE TABLE locked_transitions (
        from_content_hash BLOB    NOT NULL,
        from_path         TEXT    NOT NULL,
        to_content_hash   BLOB    NOT NULL,
        to_path           TEXT    NOT NULL,
        a_start_bar       INTEGER NOT NULL,
        b_start_bar       INTEGER NOT NULL,
        bars              INTEGER NOT NULL,
        bass_swap_bar     INTEGER,
        PRIMARY KEY (from_content_hash, to_content_hash)
    ) WITHOUT ROWID;
";

/// Where a blend lies, in the bars of each track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlendBars {
    /// The bar of the outgoing track it starts on, counted from 0 at its
    /// first downbeat.
    pub a_start_bar: usize,
    /// The bar of the incoming track that starts then.
    pub b_start_bar: usize,
    /// How many bars the two play together.
    pub bars: usize,
    /// The bar of the blend, from 1, where the bass is swapped, or `None`
    /// for the middle.
    pub bass_swap_bar: Option<usize>,
}

impl BlendBars {
    /// The bar of the blend, from 1, where the bass is swapped.
    pub fn swap_bar(&self) -> usize {
        self.bass_swap_bar
            .filter(|bar| (1..=self.bars).contains(bar))
            .unwrap_or(self.bars / 2 + 1)
    }
}

/// A transition the user placed by hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedTransition {
    /// The track playing out.
    pub from: PlaylistEntry,
    /// The track coming in.
    pub to: PlaylistEntry,
    /// Where the blend lies.
    pub blend: BlendBars,
}

impl LockedTransition {
    /// Whether this lock is for `from` into `to`: each side matches by its
    /// content or by its path.
    pub fn applies_to(
        &self,
        from: (&Path, Option<&ContentHash>),
        to: (&Path, Option<&ContentHash>),
    ) -> bool {
        let matches = |entry: &PlaylistEntry, (path, content): (&Path, Option<&ContentHash>)| {
            entry.path == path || content.is_some_and(|content| *content == entry.content)
        };
        matches(&self.from, from) && matches(&self.to, to)
    }
}

/// The lock among `locks` for `from` into `to`, the last one kept when
/// several match.
pub fn locked_blend(
    locks: &[LockedTransition],
    from: (&Path, Option<&ContentHash>),
    to: (&Path, Option<&ContentHash>),
) -> Option<BlendBars> {
    locks
        .iter()
        .rev()
        .find(|lock| lock.applies_to(from, to))
        .map(|lock| lock.blend)
}

impl PlaylistStore {
    /// Every locked transition.
    ///
    /// # Errors
    ///
    /// Returns [`EuphoriaError::Playlists`] when the database cannot be
    /// read or holds a damaged row.
    pub fn load_locked_transitions(&self) -> Result<Vec<LockedTransition>, EuphoriaError> {
        read_locks(&self.connection).map_err(store_error(&self.path))
    }

    /// Keeps `lock`, replacing the lock of the same pair of contents.
    ///
    /// # Errors
    ///
    /// Returns [`EuphoriaError::Playlists`] when the database cannot be
    /// written or a path is not UTF-8.
    pub fn lock_transition(&mut self, lock: &LockedTransition) -> Result<(), EuphoriaError> {
        let path = self.path.clone();
        write_lock(&mut self.connection, lock).map_err(store_error(&path))
    }

    /// Forgets the lock of `from` into `to`, matched as
    /// [`LockedTransition::applies_to`] does. Returns how many were
    /// forgotten.
    ///
    /// # Errors
    ///
    /// Returns [`EuphoriaError::Playlists`] when the database cannot be
    /// written.
    pub fn unlock_transition(
        &mut self,
        from: (&Path, Option<&ContentHash>),
        to: (&Path, Option<&ContentHash>),
    ) -> Result<usize, EuphoriaError> {
        let path = self.path.clone();
        let locks = self.load_locked_transitions()?;
        let mut forgotten = 0;
        for lock in locks.iter().filter(|lock| lock.applies_to(from, to)) {
            forgotten += self
                .connection
                .execute(
                    "DELETE FROM locked_transitions
                     WHERE from_content_hash = ?1 AND to_content_hash = ?2",
                    params![lock.from.content.as_bytes(), lock.to.content.as_bytes()],
                )
                .map_err(store_error(&path))?;
        }
        Ok(forgotten)
    }
}

fn read_locks(connection: &rusqlite::Connection) -> Result<Vec<LockedTransition>, ErrorSource> {
    let mut statement = connection.prepare(
        "SELECT from_content_hash, from_path, to_content_hash, to_path,
                a_start_bar, b_start_bar, bars, bass_swap_bar
         FROM locked_transitions",
    )?;
    let rows = statement
        .query_map([], |row| {
            let bar = |index: usize| -> rusqlite::Result<usize> {
                let value: i64 = row.get(index)?;
                usize::try_from(value)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(index, value))
            };
            let swap: Option<i64> = row.get(7)?;
            Ok(LockedTransition {
                from: entry(row, 0, 1)?,
                to: entry(row, 2, 3)?,
                blend: BlendBars {
                    a_start_bar: bar(4)?,
                    b_start_bar: bar(5)?,
                    bars: bar(6)?,
                    bass_swap_bar: swap.and_then(|swap| usize::try_from(swap).ok()),
                },
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn entry(
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
        path: path.into(),
    })
}

fn write_lock(
    connection: &mut rusqlite::Connection,
    lock: &LockedTransition,
) -> Result<(), ErrorSource> {
    let utf8 = |path: &Path| -> Result<String, ErrorSource> {
        path.to_str()
            .map(str::to_string)
            .ok_or_else(|| format!("{} is not valid UTF-8", path.display()).into())
    };
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    transaction.execute(
        "INSERT OR REPLACE INTO locked_transitions
             (from_content_hash, from_path, to_content_hash, to_path,
              a_start_bar, b_start_bar, bars, bass_swap_bar)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            lock.from.content.as_bytes(),
            utf8(&lock.from.path)?,
            lock.to.content.as_bytes(),
            utf8(&lock.to.path)?,
            lock.blend.a_start_bar as i64,
            lock.blend.b_start_bar as i64,
            lock.blend.bars as i64,
            lock.blend.bass_swap_bar.map(|bar| bar as i64),
        ],
    )?;
    transaction.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry_at(path: &str, byte: u8) -> PlaylistEntry {
        PlaylistEntry {
            content: ContentHash::from_bytes([byte; 32]),
            path: PathBuf::from(path),
        }
    }

    fn lock(a_start_bar: usize) -> LockedTransition {
        LockedTransition {
            from: entry_at("/music/a.flac", 1),
            to: entry_at("/music/b.flac", 2),
            blend: BlendBars {
                a_start_bar,
                b_start_bar: 16,
                bars: 32,
                bass_swap_bar: Some(17),
            },
        }
    }

    #[test]
    fn a_lock_is_kept_replaced_and_forgotten() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("playlists.sqlite3");
        let mut store = PlaylistStore::open(&path).unwrap();
        assert!(store.load_locked_transitions().unwrap().is_empty());
        store.lock_transition(&lock(64)).unwrap();
        store.lock_transition(&lock(96)).unwrap();
        let reopened = PlaylistStore::open(&path).unwrap();
        assert_eq!(reopened.load_locked_transitions().unwrap(), vec![lock(96)]);

        let other_content = ContentHash::from_bytes([9; 32]);
        let forgotten = store
            .unlock_transition(
                (Path::new("/music/a.flac"), Some(&other_content)),
                (
                    Path::new("/moved/b.flac"),
                    Some(&ContentHash::from_bytes([2; 32])),
                ),
            )
            .unwrap();
        assert_eq!(forgotten, 1);
        assert!(store.load_locked_transitions().unwrap().is_empty());
    }

    #[test]
    fn a_lock_follows_its_tracks_by_path_or_by_content() {
        let locks = [lock(64)];
        let moved_b = PathBuf::from("/elsewhere/b.flac");
        let b_content = ContentHash::from_bytes([2; 32]);
        let found = locked_blend(
            &locks,
            (Path::new("/music/a.flac"), None),
            (&moved_b, Some(&b_content)),
        );
        assert_eq!(found.map(|blend| blend.a_start_bar), Some(64));
        assert_eq!(
            locked_blend(
                &locks,
                (Path::new("/music/b.flac"), None),
                (Path::new("/music/a.flac"), None),
            ),
            None
        );
    }
}
