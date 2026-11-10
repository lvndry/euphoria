// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Replacing a file with a rewritten copy of itself without ever leaving a
//! half written file in its place.

use std::ffi::OsString;
use std::fs::{File, Metadata, OpenOptions};
use std::io::{self, Seek};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::ErrorSource;

const TEMPORARY_SUFFIX: &str = ".euphoria-tmp";
const TEMPORARY_NAME_ATTEMPTS: u32 = 64;

static TEMPORARY_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A private copy of a file, made next to it, that takes the file's place
/// once it has been rewritten and flushed to disk.
pub(super) struct Replacement {
    target: PathBuf,
    temporary_path: PathBuf,
    file: File,
    target_link_count: u64,
}

impl Replacement {
    /// Copies `target` into a new file of its own in the same directory.
    /// `target` must be the real file, not a symbolic link to it.
    pub(super) fn start(target: &Path) -> io::Result<Self> {
        let mut original = File::open(target)?;
        let metadata = original.metadata()?;
        let (temporary_path, mut file) = create_temporary(target)?;
        let copied = io::copy(&mut original, &mut file)
            .and_then(|_| file.set_permissions(metadata.permissions()));
        if let Err(error) = copied {
            // The copy error is the one reported; the unfinished copy is
            // removed on a best effort.
            let _ = std::fs::remove_file(&temporary_path);
            return Err(error);
        }
        Ok(Self {
            target: target.to_path_buf(),
            temporary_path,
            file,
            target_link_count: link_count(&metadata),
        })
    }

    /// The copy, open for reading and writing.
    pub(super) fn file(&mut self) -> &mut File {
        &mut self.file
    }

    /// Removes the copy and leaves the original as it is.
    pub(super) fn discard(self) -> io::Result<()> {
        drop(self.file);
        std::fs::remove_file(&self.temporary_path)
    }

    /// Puts the copy in place of the original.
    ///
    /// A file with a single name is replaced by renaming the copy over it,
    /// which is one step: readers see the old file or the new one. A file
    /// with several hard links is overwritten where it is instead, so every
    /// link keeps pointing at the same contents.
    pub(super) fn commit(self) -> Result<(), ErrorSource> {
        if let Err(error) = self.file.sync_all() {
            // The sync error is the one reported.
            let _ = self.discard();
            return Err(error.into());
        }
        if self.target_link_count > 1 {
            return self.overwrite_target();
        }
        if let Err(error) = std::fs::rename(&self.temporary_path, &self.target) {
            // The rename error is the one reported.
            let _ = self.discard();
            return Err(error.into());
        }
        sync_directory(&self.target)?;
        Ok(())
    }

    fn overwrite_target(mut self) -> Result<(), ErrorSource> {
        let overwrite = |copy: &mut File| -> io::Result<()> {
            copy.rewind()?;
            let mut target = OpenOptions::new().write(true).open(&self.target)?;
            let length = io::copy(copy, &mut target)?;
            target.set_len(length)?;
            target.sync_all()
        };
        match overwrite(&mut self.file) {
            Ok(()) => Ok(self.discard()?),
            Err(error) => Err(format!(
                "{error}; the file may be incomplete, and a complete copy with the new tags is \
                 kept at {}",
                self.temporary_path.display()
            )
            .into()),
        }
    }
}

/// Creates a file that did not exist before, next to `target`. The name
/// holds the process and a counter, so concurrent writes, in this process
/// or another, each get their own file.
fn create_temporary(target: &Path) -> io::Result<(PathBuf, File)> {
    let file_name = target
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"))?;
    let mut last_error = None;
    for _ in 0..TEMPORARY_NAME_ATTEMPTS {
        let unique = TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut name = OsString::from(".");
        name.push(file_name);
        name.push(format!(
            ".{}-{unique}{TEMPORARY_SUFFIX}",
            std::process::id()
        ));
        let path = target.with_file_name(name);
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => last_error = Some(error),
            Err(error) => return Err(error),
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::other("no temporary file name was tried")))
}

#[cfg(unix)]
fn link_count(metadata: &Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::nlink(metadata)
}

#[cfg(not(unix))]
fn link_count(_metadata: &Metadata) -> u64 {
    1
}

/// Flushes the directory entry of `path`, so the rename survives a power
/// loss.
#[cfg(unix)]
fn sync_directory(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(directory) => File::open(directory)?.sync_all(),
        None => Ok(()),
    }
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_file(name: &str, contents: &[u8]) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("euphoria-replace-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("track.bin");
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn entries(path: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn concurrent_replacements_get_their_own_copies() {
        let path = scratch_file("concurrent", b"original");
        let first = Replacement::start(&path).unwrap();
        let second = Replacement::start(&path).unwrap();
        assert_ne!(first.temporary_path, second.temporary_path);
        assert_eq!(entries(&path).len(), 3);

        first.discard().unwrap();
        second.discard().unwrap();
        assert_eq!(entries(&path), ["track.bin"]);
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn an_existing_temporary_file_is_never_reused() {
        let path = scratch_file("existing", b"original");
        let next = TEMPORARY_COUNTER.load(Ordering::Relaxed);
        for unique in next..next + 8 {
            let name = format!(
                ".track.bin.{}-{unique}{TEMPORARY_SUFFIX}",
                std::process::id()
            );
            std::fs::write(path.with_file_name(name), b"someone else's").unwrap();
        }
        let before = entries(&path);

        let replacement = Replacement::start(&path).unwrap();
        assert!(
            !before.contains(
                &replacement
                    .temporary_path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            )
        );
        replacement.discard().unwrap();
        assert_eq!(entries(&path), before);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn commit_replaces_the_contents_and_removes_the_copy() {
        use std::io::Write;

        let path = scratch_file("commit", b"original");
        let mut replacement = Replacement::start(&path).unwrap();
        replacement.file().write_all(b" and more").unwrap();
        replacement.commit().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"original and more");
        assert_eq!(entries(&path), ["track.bin"]);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn commit_keeps_the_permission_bits() {
        use std::os::unix::fs::PermissionsExt;

        let path = scratch_file("permissions", b"original");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        Replacement::start(&path).unwrap().commit().unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
