// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The backups of tag writes, kept on disk so that a write can be undone
//! by a later run of the program.
//!
//! Each write has a log file of its own. The backup of a file is in the
//! log, flushed to disk, before the file is touched, so an interrupted
//! write can still be undone.

use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::native::{Purpose, TagFile};
use super::{TagBackup, WritePlan, apply_plan, undo};
use crate::cache::default_data_directory;
use crate::error::{ErrorSource, EuphoriaError};

const LOG_DIRECTORY_NAME: &str = "tag-writes";
const LOG_NAME_START: &str = "write-";
const LOG_NAME_END: &str = ".jsonl";

/// One line of a log.
#[derive(Debug, Serialize, Deserialize)]
enum Entry {
    /// A file is about to be written, and this undoes it.
    Planned(TagBackup),
    /// The write of this file finished.
    Applied(PathBuf),
}

/// The folder the logs of tag writes are kept in unless the caller chooses
/// another: `tag-writes` in Euphoria's data folder, next to the analysis
/// cache.
pub fn default_write_log_directory() -> Result<PathBuf, EuphoriaError> {
    Ok(default_data_directory()?.join(LOG_DIRECTORY_NAME))
}

fn log_error(path: &Path, source: impl Into<ErrorSource>) -> EuphoriaError {
    EuphoriaError::TagWriteLog {
        path: path.to_path_buf(),
        source: source.into(),
    }
}

/// The log of one write, which may cover many files.
#[derive(Debug)]
pub struct WriteLog {
    directory: PathBuf,
    file: Option<(PathBuf, File)>,
}

impl WriteLog {
    /// Starts the log of a write in `directory`. Nothing is created on
    /// disk until a file is written.
    pub fn new(directory: &Path) -> Self {
        Self {
            directory: directory.to_path_buf(),
            file: None,
        }
    }

    /// Applies `plan` like [`apply_plan`], with its backup saved in the
    /// log first. A file whose backup cannot be saved is not written. An
    /// empty plan does nothing.
    pub fn apply(&mut self, plan: &WritePlan) -> Result<(), EuphoriaError> {
        if plan.is_empty() {
            return Ok(());
        }
        self.record(&Entry::Planned(plan.backup()))?;
        apply_plan(plan)?;
        self.record(&Entry::Applied(plan.path().to_path_buf()))
    }

    fn record(&mut self, entry: &Entry) -> Result<(), EuphoriaError> {
        if self.file.is_none() {
            self.file = Some(create_log(&self.directory)?);
        }
        let Some((path, file)) = &mut self.file else {
            return Ok(());
        };
        let mut line = serde_json::to_vec(entry).map_err(|error| log_error(path, error))?;
        line.push(b'\n');
        file.write_all(&line)
            .and_then(|()| file.sync_data())
            .map_err(|error| log_error(path, error))
    }
}

/// Creates a log whose name sorts after those of earlier writes.
fn create_log(directory: &Path) -> Result<(PathBuf, File), EuphoriaError> {
    std::fs::create_dir_all(directory).map_err(|error| log_error(directory, error))?;
    let since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let name = format!(
        "{LOG_NAME_START}{:020}-{:09}-{}{LOG_NAME_END}",
        since_epoch.as_secs(),
        since_epoch.subsec_nanos(),
        std::process::id()
    );
    let path = directory.join(name);
    let file = OpenOptions::new()
        .append(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| log_error(&path, error))?;
    Ok((path, file))
}

/// What undoing a write did to its files.
#[derive(Debug, Default)]
pub struct UndoneWrite {
    /// The files that got their tags back.
    pub restored: Vec<PathBuf>,
    /// The files the write was interrupted before changing, which needed
    /// nothing.
    pub untouched: Vec<PathBuf>,
    /// The files that could not be restored, and why. Their backups stay
    /// in the log, so undoing again tries them again.
    pub failed: Vec<(PathBuf, EuphoriaError)>,
}

/// The newest log in `directory`, if there is one.
fn last_log(directory: &Path) -> Result<Option<PathBuf>, EuphoriaError> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(log_error(directory, error)),
    };
    let mut newest: Option<PathBuf> = None;
    for entry in entries {
        let path = entry.map_err(|error| log_error(directory, error))?.path();
        let is_log = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(LOG_NAME_START) && name.ends_with(LOG_NAME_END));
        if is_log && newest.as_ref().is_none_or(|newest| path > *newest) {
            newest = Some(path);
        }
    }
    Ok(newest)
}

/// The backups in a log, oldest first, each with whether its write
/// finished.
fn read_log(path: &Path) -> Result<Vec<(TagBackup, bool)>, EuphoriaError> {
    let text = std::fs::read_to_string(path).map_err(|error| log_error(path, error))?;
    let mut backups = Vec::new();
    let mut applied = HashSet::new();
    let line_count = text.lines().count();
    for (index, line) in text.lines().enumerate() {
        match serde_json::from_str(line) {
            Ok(Entry::Planned(backup)) => backups.push(backup),
            Ok(Entry::Applied(path)) => {
                applied.insert(path);
            }
            // A write that was cut off can leave half a line at the end.
            // The file that line was for had not been touched yet.
            Err(_) if index + 1 == line_count && !text.ends_with('\n') => {}
            Err(error) => return Err(log_error(path, error)),
        }
    }
    Ok(backups
        .into_iter()
        .map(|backup| {
            let is_applied = applied.contains(&backup.path);
            (backup, is_applied)
        })
        .collect())
}

/// Whether every field of the backup's file still holds its original
/// value, which is the case when a write was planned but never made.
fn is_untouched(backup: &TagBackup) -> Result<bool, EuphoriaError> {
    let file = TagFile::load(&backup.path, Purpose::Reading)?;
    for field in &backup.fields {
        if file.get(&field.field)? != field.original {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Undoes the most recent write logged in `directory` that has not been
/// undone, restoring each of its files with [`undo`]. Returns `None` when
/// there is no such write.
///
/// One file that cannot be restored does not stop the others. The log is
/// removed once every file is restored. Otherwise it keeps the backups of
/// the files that failed, so the call can be repeated.
pub fn undo_last_write(directory: &Path) -> Result<Option<UndoneWrite>, EuphoriaError> {
    let Some(log_path) = last_log(directory)? else {
        return Ok(None);
    };
    let mut undone = UndoneWrite::default();
    let mut remaining = Vec::new();
    // Newest first, so that a file written twice in one log goes back
    // through its states in order.
    for (backup, is_applied) in read_log(&log_path)?.into_iter().rev() {
        let restored = if is_applied {
            undo(&backup).map(|()| true)
        } else {
            // The write of this file was interrupted. It either never
            // started, or it finished without being recorded.
            is_untouched(&backup).and_then(|is_untouched| {
                if is_untouched {
                    Ok(false)
                } else {
                    undo(&backup).map(|()| true)
                }
            })
        };
        match restored {
            Ok(true) => undone.restored.push(backup.path),
            Ok(false) => undone.untouched.push(backup.path),
            Err(error) => {
                undone.failed.push((backup.path.clone(), error));
                remaining.push(backup);
            }
        }
    }
    undone.restored.reverse();
    undone.untouched.reverse();
    undone.failed.reverse();
    remaining.reverse();

    if remaining.is_empty() {
        std::fs::remove_file(&log_path).map_err(|error| log_error(&log_path, error))?;
    } else {
        rewrite_log(&log_path, remaining)?;
    }
    Ok(Some(undone))
}

/// Replaces the log with one that holds only `backups`, in one step.
fn rewrite_log(log_path: &Path, backups: Vec<TagBackup>) -> Result<(), EuphoriaError> {
    let mut contents = Vec::new();
    for backup in backups {
        let applied = Entry::Applied(backup.path.clone());
        for entry in [Entry::Planned(backup), applied] {
            serde_json::to_writer(&mut contents, &entry)
                .map_err(|error| log_error(log_path, error))?;
            contents.push(b'\n');
        }
    }
    let temporary_path = log_path.with_extension("rewriting");
    let written = File::create(&temporary_path)
        .and_then(|mut file| {
            file.write_all(&contents)?;
            file.sync_all()
        })
        .and_then(|()| std::fs::rename(&temporary_path, log_path));
    written.map_err(|error| log_error(log_path, error))
}
