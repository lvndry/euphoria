// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tags of the library's files, each read once and kept in memory, so
//! that counting what the tag settings would change re-plans from memory
//! when a setting changes instead of reading every file again. An entry is
//! kept while the file's modified time and length stay the same, and the
//! whole cache is emptied whenever Euphoria changes tags.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use euphoria_core::EuphoriaError;
use euphoria_core::tags::LoadedTags;

type Stamp = (Option<SystemTime>, u64);
type Entries = HashMap<PathBuf, (Stamp, Arc<LoadedTags>)>;

/// Tags read from files, shared between the main thread and the planning
/// threads.
#[derive(Clone, Default)]
pub(crate) struct TagReads {
    entries: Arc<Mutex<Entries>>,
    reads: Arc<AtomicUsize>,
}

impl TagReads {
    /// The tags of the file at `path`, read now unless they were read
    /// while the file was as it is.
    pub(crate) fn tags_of(&self, path: &Path) -> Result<Arc<LoadedTags>, EuphoriaError> {
        let stamp = std::fs::metadata(path)
            .ok()
            .map(|metadata| (metadata.modified().ok(), metadata.len()));
        if let Some(stamp) = stamp {
            let entries = self
                .entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some((kept, tags)) = entries.get(path)
                && *kept == stamp
            {
                return Ok(tags.clone());
            }
        }
        let tags = Arc::new(LoadedTags::load(path)?);
        self.reads.fetch_add(1, Ordering::SeqCst);
        if let Some(stamp) = stamp {
            self.entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(path.to_path_buf(), (stamp, tags.clone()));
        }
        Ok(tags)
    }

    /// Forgets every file's tags: some were just changed.
    pub(crate) fn invalidate(&self) {
        self.entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    /// How many times a file was read, for tests.
    #[cfg(test)]
    pub(crate) fn read_count(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}
