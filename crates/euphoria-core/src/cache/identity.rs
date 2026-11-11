// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::File;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// The identity of a file's content: the BLAKE3 hash of every byte in it.
///
/// Two files with the same hash are treated as the same track wherever they
/// are and whatever they are called. The hash is 256 bits wide, so two
/// different files sharing one is not a practical concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    /// Reads the whole file and hashes it.
    pub fn of_file(path: &Path) -> io::Result<Self> {
        let mut hasher = blake3::Hasher::new();
        hasher.update_reader(File::open(path)?)?;
        Ok(Self(*hasher.finalize().as_bytes()))
    }

    /// The 32 bytes of the hash.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub(crate) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

/// What the file system says about a file without reading it. When the size
/// and modification time at a path are the ones recorded with an analysis,
/// the content is taken to be unchanged and is not hashed again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FileStamp {
    pub(super) size: i64,
    pub(super) modified_nanoseconds: i64,
}

impl FileStamp {
    pub(super) fn of(path: &Path) -> io::Result<Self> {
        let metadata = std::fs::metadata(path)?;
        Ok(Self {
            size: i64::try_from(metadata.len()).unwrap_or(i64::MAX),
            modified_nanoseconds: nanoseconds_since_epoch(metadata.modified()?),
        })
    }
}

fn nanoseconds_since_epoch(time: SystemTime) -> i64 {
    match time.duration_since(UNIX_EPOCH) {
        Ok(after_epoch) => i64::try_from(after_epoch.as_nanos()).unwrap_or(i64::MAX),
        Err(before_epoch) => i64::try_from(before_epoch.duration().as_nanos())
            .map(|nanoseconds| -nanoseconds)
            .unwrap_or(i64::MIN),
    }
}
