// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Replacing a file with a rewritten copy of itself without ever leaving a
//! half written file in its place. The copy is the original with a few byte
//! ranges exchanged, and every other byte carried over as it is.

use std::ffi::OsString;
use std::fs::{File, Metadata, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::ErrorSource;

const TEMPORARY_SUFFIX: &str = ".euphoria-tmp";
const TEMPORARY_NAME_ATTEMPTS: u32 = 64;

static TEMPORARY_COUNTER: AtomicU64 = AtomicU64::new(0);

const COMPARE_BUFFER_LENGTH: usize = 64 * 1024;

/// New bytes for the range `start..end` of a file. An empty range inserts,
/// and empty bytes remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Splice {
    pub(super) start: u64,
    pub(super) end: u64,
    pub(super) bytes: Vec<u8>,
}

/// Writes `original` to `output` with each splice applied. The splices must
/// not overlap.
pub(super) fn compose(
    original: &mut File,
    splices: &mut [Splice],
    output: &mut File,
) -> io::Result<()> {
    splices.sort_by_key(|splice| (splice.start, splice.end));
    let mut position = 0;
    for splice in splices.iter() {
        if splice.start < position || splice.end < splice.start {
            return Err(io::Error::other("the changes to the file overlap"));
        }
        copy_range(original, position, splice.start, output)?;
        output.write_all(&splice.bytes)?;
        position = splice.end;
    }
    let length = original.metadata()?.len();
    copy_range(original, position, length.max(position), output)
}

fn copy_range(original: &mut File, start: u64, end: u64, output: &mut File) -> io::Result<()> {
    original.seek(SeekFrom::Start(start))?;
    let wanted = end - start;
    let copied = io::copy(&mut original.take(wanted), output)?;
    if copied == wanted {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "the file is shorter than its structure says",
        ))
    }
}

/// Whether every byte of `original` outside the splices is found, in the
/// same order, at its new place in `output`. The splices must be sorted.
pub(super) fn carried_over(
    original: &mut File,
    splices: &[Splice],
    output: &mut File,
) -> io::Result<bool> {
    let length = original.metadata()?.len();
    let mut original_position = 0;
    let mut output_position = 0;
    let ends = splices
        .iter()
        .map(|splice| (splice.start, splice.end, splice.bytes.len() as u64));
    for (start, end, inserted) in ends.chain([(length, length, 0)]) {
        let kept = start - original_position;
        if !same_bytes(original, original_position, output, output_position, kept)? {
            return Ok(false);
        }
        original_position = end;
        output_position += kept + inserted;
    }
    Ok(output.metadata()?.len() == output_position)
}

/// Whether `length` bytes at the two positions are equal.
pub(super) fn same_bytes(
    first: &mut File,
    first_position: u64,
    second: &mut File,
    second_position: u64,
    length: u64,
) -> io::Result<bool> {
    first.seek(SeekFrom::Start(first_position))?;
    second.seek(SeekFrom::Start(second_position))?;
    let mut first_buffer = vec![0; COMPARE_BUFFER_LENGTH];
    let mut second_buffer = vec![0; COMPARE_BUFFER_LENGTH];
    let mut remaining = length;
    while remaining > 0 {
        let step = remaining.min(COMPARE_BUFFER_LENGTH as u64) as usize;
        let read = first
            .read_exact(&mut first_buffer[..step])
            .and_then(|()| second.read_exact(&mut second_buffer[..step]));
        match read {
            Ok(()) if first_buffer[..step] == second_buffer[..step] => remaining -= step as u64,
            Ok(()) => return Ok(false),
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(false),
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

/// A new file next to the original that takes the original's place once it
/// has been written and flushed to disk.
pub(super) struct Replacement {
    target: PathBuf,
    temporary_path: PathBuf,
    file: File,
}

impl Replacement {
    /// Creates an empty file of its own in the directory of `target`, with
    /// the permission bits of `target`. `target` must be the real file, not
    /// a symbolic link to it.
    pub(super) fn start(target: &Path) -> io::Result<Self> {
        let metadata = std::fs::metadata(target)?;
        if has_several_links(&metadata) {
            return Err(io::Error::other("the file has more than one hard link"));
        }
        let (temporary_path, file) = create_temporary(target)?;
        if let Err(error) = file.set_permissions(metadata.permissions()) {
            // The permission error is the one reported; the new file is
            // removed on a best effort.
            let _ = std::fs::remove_file(&temporary_path);
            return Err(error);
        }
        Ok(Self {
            target: target.to_path_buf(),
            temporary_path,
            file,
        })
    }

    /// The new file, open for reading and writing.
    pub(super) fn file(&mut self) -> &mut File {
        &mut self.file
    }

    /// Removes the copy and leaves the original as it is.
    pub(super) fn discard(self) -> io::Result<()> {
        drop(self.file);
        std::fs::remove_file(&self.temporary_path)
    }

    /// Puts the new file in place of the original by renaming it over the
    /// original, which is one step: readers see the old file or the new
    /// one, and the original is whole until that step.
    pub(super) fn commit(self) -> Result<(), ErrorSource> {
        if let Err(error) = self.file.sync_all() {
            // The sync error is the one reported.
            let _ = self.discard();
            return Err(error.into());
        }
        if let Err(error) = std::fs::rename(&self.temporary_path, &self.target) {
            // The rename error is the one reported.
            let _ = self.discard();
            return Err(error.into());
        }
        sync_directory(&self.target)?;
        Ok(())
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

/// Whether the file has other names besides the one it was opened by.
/// Renaming a new file over one name would leave the other names with the
/// old contents, and overwriting the shared contents in place cannot be
/// undone if it is interrupted, so such a file is not rewritten at all.
#[cfg(unix)]
pub(super) fn has_several_links(metadata: &Metadata) -> bool {
    std::os::unix::fs::MetadataExt::nlink(metadata) > 1
}

/// Whether the file has other names besides the one it was opened by. The
/// standard library cannot tell on this platform.
#[cfg(not(unix))]
pub(super) fn has_several_links(_metadata: &Metadata) -> bool {
    false
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
        let path = scratch_file("commit", b"original");
        let mut replacement = Replacement::start(&path).unwrap();
        replacement.file().write_all(b"rewritten").unwrap();
        replacement.commit().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"rewritten");
        assert_eq!(entries(&path), ["track.bin"]);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn splices_replace_insert_and_remove() {
        let path = scratch_file("splices", b"0123456789");
        let mut original = File::open(&path).unwrap();
        let mut splices = vec![
            Splice {
                start: 8,
                end: 9,
                bytes: Vec::new(),
            },
            Splice {
                start: 2,
                end: 4,
                bytes: b"abc".to_vec(),
            },
            Splice {
                start: 6,
                end: 6,
                bytes: b"+".to_vec(),
            },
        ];
        let mut replacement = Replacement::start(&path).unwrap();
        compose(&mut original, &mut splices, replacement.file()).unwrap();
        assert!(carried_over(&mut original, &splices, replacement.file()).unwrap());

        replacement.commit().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"01abc45+679");
        assert_eq!(entries(&path), ["track.bin"]);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn damage_to_the_new_file_is_detected() {
        let path = scratch_file("damage", b"0123456789");
        let mut original = File::open(&path).unwrap();
        let mut splices = vec![Splice {
            start: 2,
            end: 4,
            bytes: b"abc".to_vec(),
        }];
        let mut replacement = Replacement::start(&path).unwrap();
        compose(&mut original, &mut splices, replacement.file()).unwrap();
        assert!(carried_over(&mut original, &splices, replacement.file()).unwrap());

        replacement.file().seek(SeekFrom::Start(0)).unwrap();
        replacement.file().write_all(b"X").unwrap();
        assert!(!carried_over(&mut original, &splices, replacement.file()).unwrap());

        replacement.discard().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"0123456789");
        assert_eq!(entries(&path), ["track.bin"]);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn file_with_several_hard_links_is_not_replaced() {
        let path = scratch_file("links", b"original");
        let other_name = path.with_file_name("other-name.bin");
        std::fs::hard_link(&path, &other_name).unwrap();
        assert!(Replacement::start(&path).is_err());
        assert_eq!(entries(&path), ["other-name.bin", "track.bin"]);
        assert_eq!(std::fs::read(&other_name).unwrap(), b"original");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn overlapping_splices_are_refused() {
        let path = scratch_file("overlap", b"0123456789");
        let mut original = File::open(&path).unwrap();
        let splice = |start, end| Splice {
            start,
            end,
            bytes: Vec::new(),
        };
        let mut replacement = Replacement::start(&path).unwrap();
        let mut splices = [splice(2, 6), splice(4, 8)];
        assert!(compose(&mut original, &mut splices, replacement.file()).is_err());
        replacement.discard().unwrap();
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
