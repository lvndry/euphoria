// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The backups of tag writes, kept on disk so that a write can be undone
//! by a later run of the program.
//!
//! Each write has a log file of its own, with one entry per line. The
//! backup of a file is in the log, flushed to disk, before the file is
//! touched, so an interrupted write can still be undone.

use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::native::{Purpose, TagFile};
use super::{BackupField, TagBackup, WritePlan};
use crate::cache::default_data_directory;
use crate::error::{ErrorSource, EuphoriaError};

const LOG_DIRECTORY_NAME: &str = "tag-writes";
const LOG_NAME_START: &str = "write-";
const LOG_NAME_END: &str = ".jsonl";

/// The longest line a log may hold. The backup of one file is one line: a
/// few short fields and the comment, at about four characters per byte, so
/// a line is normally one or two kilobytes.
const LONGEST_LINE: u64 = 4 * 1024 * 1024;
/// The most lines a log may hold. A file takes two lines, so this is a
/// write of half a million files.
const MOST_ENTRIES: u64 = 1_000_000;
/// The largest a log may be. At two kilobytes per file, a write of fifty
/// thousand files makes a log of about a hundred megabytes.
const LARGEST_LOG: u64 = 2 * 1024 * 1024 * 1024;
/// Room kept free for the line that records the outcome of a write, which
/// holds little more than the file's path.
const OUTCOME_RESERVE: u64 = 64 * 1024;

/// One line of a log.
#[derive(Debug, Serialize, Deserialize)]
enum Entry {
    /// A file is about to be written, and this undoes it.
    Planned(TagBackup),
    /// The write of this file finished.
    Applied(PathBuf),
    /// The write of this file was given up and the file was not changed.
    Failed(PathBuf),
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

/// How a write made through a [`WriteLog`] ended, when the file was
/// changed.
#[derive(Debug)]
#[must_use]
pub enum LoggedWrite {
    /// The file was changed, and the log says so.
    Recorded,
    /// The file was changed, but noting that in the log failed, for example
    /// because the disk is full. The backup was saved before the change, so
    /// undoing the write still restores the file.
    NotRecorded(EuphoriaError),
}

/// A boundary of a successful metadata rewrite. Observers may keep other
/// explicit user stores in step with the file without affecting tag backups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewriteStage {
    /// Immediately before attempting the rewrite, while old bytes remain.
    Before,
    /// The new bytes are in place. A later directory sync or log write may
    /// still report an error, so this event can precede a failed result.
    After,
}

struct OpenLog {
    path: PathBuf,
    file: File,
    entry_count: u64,
    length: u64,
}

/// The log of one write, which may cover many files.
pub struct WriteLog {
    directory: PathBuf,
    log: Option<OpenLog>,
    #[cfg(test)]
    fail_on_entry: Option<u64>,
}

impl std::fmt::Debug for WriteLog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WriteLog")
            .field("directory", &self.directory)
            .finish_non_exhaustive()
    }
}

impl WriteLog {
    /// Starts the log of a write in `directory`. Nothing is created on
    /// disk until a file is written.
    pub fn new(directory: &Path) -> Self {
        Self {
            directory: directory.to_path_buf(),
            log: None,
            #[cfg(test)]
            fail_on_entry: None,
        }
    }

    /// Applies `plan` like [`super::apply_plan`], with its backup saved in the
    /// log first. An empty plan does nothing.
    ///
    /// A refused write or a backup failure leaves the file unchanged. A
    /// directory synchronization error can be reported after replacement;
    /// its backup remains available for undo. `Ok` means the file was
    /// changed, and says whether the log could note that.
    pub fn apply(&mut self, plan: &WritePlan) -> Result<LoggedWrite, EuphoriaError> {
        self.apply_with_observer(plan, |_, _| {})
    }

    /// Like [`Self::apply`], observing the file before and after a successful
    /// rewrite. No events are sent for empty plans. A refused write sends
    /// only `Before`; observers must retain and report their own errors.
    pub fn apply_with_observer(
        &mut self,
        plan: &WritePlan,
        mut observer: impl FnMut(&Path, RewriteStage),
    ) -> Result<LoggedWrite, EuphoriaError> {
        if plan.is_empty() {
            return Ok(LoggedWrite::Recorded);
        }
        let backup = plan.backup();
        let path = backup.path.clone();
        self.record(&Entry::Planned(backup), OUTCOME_RESERVE)?;
        observer(&path, RewriteStage::Before);
        match super::apply_plan_observing(plan, || observer(&path, RewriteStage::After)) {
            Ok(_) => Ok(match self.record(&Entry::Applied(path), 0) {
                Ok(()) => LoggedWrite::Recorded,
                Err(error) => LoggedWrite::NotRecorded(error),
            }),
            Err(error) => {
                // A failed write leaves the file as it was, and then its
                // backup must not stay behind as if the write were still
                // under way: the note that it was given up takes it out of
                // any later undo. The one exception is a failure after the
                // new file was already in place, which shows in the file
                // holding the new values. Then, and when the file cannot
                // be read or the note cannot be saved, the backup stays
                // and an undo sorts it out.
                let backup = plan.backup();
                let was_changed = holds(&backup, |field| &field.written).unwrap_or(true);
                if !was_changed {
                    let _ = self.record(&Entry::Failed(path), 0);
                }
                Err(error)
            }
        }
    }

    /// Appends `entry` and flushes it to disk. `reserve` is room that must
    /// stay free after it.
    fn record(&mut self, entry: &Entry, reserve: u64) -> Result<(), EuphoriaError> {
        if self.log.is_none() {
            self.log = Some(create_log(&self.directory)?);
        }
        let Some(log) = &mut self.log else {
            return Ok(());
        };
        let mut line = serde_json::to_vec(entry).map_err(|error| log_error(&log.path, error))?;
        line.push(b'\n');
        let line_length = line.len() as u64;
        // A log that undo would refuse to read must never be written.
        let entries_needed = if reserve > 0 { 2 } else { 1 };
        if line_length > LONGEST_LINE {
            return Err(log_error(&log.path, "the backup of this file is too large"));
        }
        if log.entry_count + entries_needed > MOST_ENTRIES
            || log.length + line_length + reserve > LARGEST_LOG
        {
            return Err(log_error(
                &log.path,
                "the log of this write is full; write the remaining files in another run",
            ));
        }
        #[cfg(test)]
        if self.fail_on_entry == Some(log.entry_count) {
            return Err(log_error(&log.path, "failure requested by a test"));
        }
        log.file
            .write_all(&line)
            .and_then(|()| log.file.sync_data())
            .map_err(|error| log_error(&log.path, error))?;
        log.entry_count += 1;
        log.length += line_length;
        Ok(())
    }
}

/// Creates a log whose name sorts after those of earlier writes.
fn create_log(directory: &Path) -> Result<OpenLog, EuphoriaError> {
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
    Ok(OpenLog {
        path,
        file,
        entry_count: 0,
        length: 0,
    })
}

/// What undoing a write did to its files.
#[derive(Debug, Default)]
pub struct UndoneWrite {
    /// The files that got their tags back.
    pub restored: Vec<PathBuf>,
    /// The files whose original tag fields are already in place: never
    /// changed, or already restored by an earlier undo attempt.
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

/// Where the backup of one file is in a log.
struct PlannedWrite {
    offset: u64,
}

/// Reads the line at the reader's position, without its line end. `None`
/// at the end of the log. A line longer than the limit is an error.
fn read_line(reader: &mut impl BufRead, path: &Path) -> Result<Option<Vec<u8>>, EuphoriaError> {
    let mut line = Vec::new();
    let read = reader
        .by_ref()
        .take(LONGEST_LINE + 1)
        .read_until(b'\n', &mut line)
        .map_err(|error| log_error(path, error))?;
    if read == 0 {
        return Ok(None);
    }
    if line.last() == Some(&b'\n') {
        line.pop();
    } else if line.len() as u64 > LONGEST_LINE {
        return Err(log_error(
            path,
            "a line of the log is longer than the limit",
        ));
    }
    Ok(Some(line))
}

/// Goes through a log one line at a time and returns where its pending
/// backups are, oldest first. Backups whose write was given up are left
/// out. Only one line is held in memory at a time.
fn index_log(log_path: &Path) -> Result<Vec<PlannedWrite>, EuphoriaError> {
    let file = File::open(log_path).map_err(|error| log_error(log_path, error))?;
    let length = file
        .metadata()
        .map_err(|error| log_error(log_path, error))?
        .len();
    if length > LARGEST_LOG {
        return Err(log_error(log_path, "the log is larger than the limit"));
    }
    let mut reader = BufReader::new(file);
    let mut planned: Vec<(PathBuf, PlannedWrite)> = Vec::new();
    let mut latest_for_path: HashMap<PathBuf, usize> = HashMap::new();
    let mut given_up: HashSet<usize> = HashSet::new();
    let mut offset = 0;
    let mut entry_count = 0;
    while let Some(line) = read_line(&mut reader, log_path)? {
        let line_offset = offset;
        offset += line.len() as u64 + 1;
        entry_count += 1;
        if entry_count > MOST_ENTRIES {
            return Err(log_error(
                log_path,
                "the log holds more entries than the limit",
            ));
        }
        match serde_json::from_slice(&line) {
            Ok(Entry::Planned(backup)) => {
                latest_for_path.insert(backup.path.clone(), planned.len());
                let write = PlannedWrite {
                    offset: line_offset,
                };
                planned.push((backup.path, write));
            }
            Ok(Entry::Applied(_)) => {}
            Ok(Entry::Failed(path)) => {
                given_up.extend(latest_for_path.get(&path));
            }
            // A write that was cut off can leave half a line at the end.
            // The file that line was for had not been touched yet.
            Err(_) if offset > length => {}
            Err(error) => return Err(log_error(log_path, error)),
        }
    }
    Ok(planned
        .into_iter()
        .enumerate()
        .filter(|(index, _)| !given_up.contains(index))
        .map(|(_, (_, write))| write)
        .collect())
}

fn read_backup(log: &mut File, log_path: &Path, offset: u64) -> Result<TagBackup, EuphoriaError> {
    log.seek(SeekFrom::Start(offset))
        .map_err(|error| log_error(log_path, error))?;
    let line = read_line(&mut BufReader::new(&mut *log), log_path)?.unwrap_or_default();
    match serde_json::from_slice(&line) {
        Ok(Entry::Planned(backup)) => Ok(backup),
        Ok(_) => Err(log_error(log_path, "the log changed while it was read")),
        Err(error) => Err(log_error(log_path, error)),
    }
}

/// Whether every field of the backup's file holds the value `expected`
/// picks for it.
fn holds(
    backup: &TagBackup,
    expected: impl Fn(&BackupField) -> &Option<String>,
) -> Result<bool, EuphoriaError> {
    let file = TagFile::load(&backup.path, Purpose::Reading)?;
    for field in &backup.fields {
        if file.get(&field.field)? != *expected(field) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether every field still holds its original value, which is the case
/// when a write was planned but never made.
fn is_untouched(backup: &TagBackup) -> Result<bool, EuphoriaError> {
    holds(backup, |field| &field.original)
}

/// Undoes the most recent write logged in `directory` that has not been
/// undone, restoring each of its files with [`super::undo`]. Returns `None` when
/// there is no such write.
///
/// One file that cannot be restored does not stop the others. The log is
/// removed once every file is restored. Otherwise it keeps the backups of
/// the files that failed, so the call can be repeated. A log in which
/// every write was given up changed no file, and is removed on the way to
/// the write before it.
///
/// A log larger than 2 GiB, with more than a million lines or with a line
/// longer than 4 MiB is refused with [`EuphoriaError::TagWriteLog`].
/// [`WriteLog`] never writes one.
pub fn undo_last_write(directory: &Path) -> Result<Option<UndoneWrite>, EuphoriaError> {
    undo_last_write_with_observer(directory, |_, _| {})
}

/// Like [`undo_last_write`], observing each attempted restoration. `After`
/// is sent only for files actually restored, not untouched or refused files.
/// The observer performs any extra I/O and reports its own errors.
pub fn undo_last_write_with_observer(
    directory: &Path,
    mut observer: impl FnMut(&Path, RewriteStage),
) -> Result<Option<UndoneWrite>, EuphoriaError> {
    loop {
        let Some(log_path) = last_log(directory)? else {
            return Ok(None);
        };
        let planned = index_log(&log_path)?;
        if planned.is_empty() {
            std::fs::remove_file(&log_path).map_err(|error| log_error(&log_path, error))?;
            continue;
        }
        return undo_log(&log_path, &planned, &mut observer).map(Some);
    }
}

fn undo_log(
    log_path: &Path,
    planned: &[PlannedWrite],
    observer: &mut impl FnMut(&Path, RewriteStage),
) -> Result<UndoneWrite, EuphoriaError> {
    let mut log = File::open(log_path).map_err(|error| log_error(log_path, error))?;
    let mut undone = UndoneWrite::default();
    let mut remaining = Vec::new();
    let mut blocked = HashSet::new();
    // Newest first, so that a file written twice in one log goes back
    // through its states in order. One backup is in memory at a time.
    for write in planned.iter().rev() {
        let backup = read_backup(&mut log, log_path, write.offset)?;
        let identity = std::fs::canonicalize(&backup.path)
            .or_else(|_| std::path::absolute(&backup.path))
            .unwrap_or_else(|_| backup.path.clone());
        if blocked.contains(&identity) {
            remaining.push(backup);
            continue;
        }
        let mut restore = || {
            observer(&backup.path, RewriteStage::Before);
            super::undo_observing(&backup, || observer(&backup.path, RewriteStage::After))
                .map(|()| true)
        };
        // A prior undo can have committed its rename before reporting a
        // directory-sync error. Its original fields already satisfy backup,
        // whether or not the original write had an Applied note.
        let restored = is_untouched(&backup)
            .and_then(|untouched| if untouched { Ok(false) } else { restore() });
        match restored {
            Ok(true) => undone.restored.push(backup.path),
            Ok(false) => undone.untouched.push(backup.path),
            Err(error) => {
                // Keep older states of this same file until the newest
                // failed restoration is settled; other files can proceed.
                blocked.insert(identity);
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
        std::fs::remove_file(log_path).map_err(|error| log_error(log_path, error))?;
    } else {
        rewrite_log(log_path, remaining)?;
    }
    Ok(undone)
}

/// Replaces the log with one that holds only `backups`, in one step.
fn rewrite_log(log_path: &Path, backups: Vec<TagBackup>) -> Result<(), EuphoriaError> {
    let temporary_path = log_path.with_extension("rewriting");
    let write = || -> Result<(), ErrorSource> {
        let mut file = File::create(&temporary_path)?;
        for backup in backups {
            let applied = Entry::Applied(backup.path.clone());
            for entry in [Entry::Planned(backup), applied] {
                serde_json::to_writer(&mut file, &entry)?;
                file.write_all(b"\n")?;
            }
        }
        file.sync_all()?;
        std::fs::rename(&temporary_path, log_path)?;
        Ok(())
    };
    write().map_err(|error| log_error(log_path, error))
}

#[cfg(test)]
mod tests {
    use super::super::{TagValues, TagWriteOptions, plan_write, read_tags};
    use super::*;
    use crate::key::{Key, Mode};

    /// A copy of the FLAC fixture in a folder of its own, with a place for
    /// logs next to it.
    fn scratch(test: &str) -> (PathBuf, PathBuf) {
        let folder =
            std::env::temp_dir().join(format!("euphoria-history-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let track = folder.join("track.flac");
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join("sine-440hz-mono.flac");
        std::fs::copy(fixture, &track).unwrap();
        (track, folder.join("logs"))
    }

    fn plan(track: &Path) -> WritePlan {
        let values = TagValues {
            key: Some(Key::new(9, Mode::Minor)),
            bpm: Some(126.0),
            energy: Some(7),
        };
        plan_write(track, values, &TagWriteOptions::default()).unwrap()
    }

    #[test]
    fn change_that_could_not_be_recorded_is_reported_as_a_change_and_can_be_undone() {
        let (track, logs) = scratch("not-recorded");
        let original = std::fs::read(&track).unwrap();
        let mut log = WriteLog::new(&logs);
        // The backup is entry 0. The note that the write finished is 1.
        log.fail_on_entry = Some(1);

        let outcome = log.apply(&plan(&track)).unwrap();
        assert!(
            matches!(
                &outcome,
                LoggedWrite::NotRecorded(EuphoriaError::TagWriteLog { .. })
            ),
            "{outcome:?}"
        );
        assert_eq!(read_tags(&track).unwrap().key.as_deref(), Some("8A"));

        let undone = undo_last_write(&logs).unwrap().unwrap();
        assert_eq!(undone.restored, std::slice::from_ref(&track));
        assert!(std::fs::read(&track).unwrap() == original);
        std::fs::remove_dir_all(track.parent().unwrap()).unwrap();
    }

    #[test]
    fn file_is_not_changed_when_its_backup_cannot_be_saved() {
        let (track, logs) = scratch("no-backup");
        let original = std::fs::read(&track).unwrap();
        let mut log = WriteLog::new(&logs);
        log.fail_on_entry = Some(0);

        let error = log.apply(&plan(&track)).unwrap_err();
        assert!(
            matches!(error, EuphoriaError::TagWriteLog { .. }),
            "{error:?}"
        );
        assert!(std::fs::read(&track).unwrap() == original);
        assert!(undo_last_write(&logs).unwrap().is_none());
        std::fs::remove_dir_all(track.parent().unwrap()).unwrap();
    }

    #[test]
    fn full_log_takes_no_further_files() {
        let (track, logs) = scratch("full");
        let original = std::fs::read(&track).unwrap();
        let mut log = WriteLog::new(&logs);
        log.record(&Entry::Failed(PathBuf::from("placeholder")), 0)
            .unwrap();
        if let Some(open) = &mut log.log {
            open.entry_count = MOST_ENTRIES - 1;
        }
        let error = log.apply(&plan(&track)).unwrap_err();
        assert!(error.chain().contains("is full"), "{}", error.chain());
        assert!(std::fs::read(&track).unwrap() == original);

        if let Some(open) = &mut log.log {
            open.entry_count = 1;
            open.length = LARGEST_LOG - OUTCOME_RESERVE;
        }
        let error = log.apply(&plan(&track)).unwrap_err();
        assert!(error.chain().contains("is full"), "{}", error.chain());
        assert!(std::fs::read(&track).unwrap() == original);
        std::fs::remove_dir_all(track.parent().unwrap()).unwrap();
    }

    #[test]
    fn log_beyond_the_limits_is_refused() {
        let (track, logs) = scratch("limits");
        let mut log = WriteLog::new(&logs);
        let _ = log.apply(&plan(&track)).unwrap();
        let after_write = std::fs::read(&track).unwrap();
        let log_path = last_log(&logs).unwrap().unwrap();
        let contents = std::fs::read(&log_path).unwrap();

        // One line longer than the limit, in front of the real entries.
        let mut long_line = vec![b' '; LONGEST_LINE as usize + 1];
        long_line.push(b'\n');
        std::fs::write(&log_path, [&long_line[..], &contents[..]].concat()).unwrap();
        let error = undo_last_write(&logs).unwrap_err();
        assert!(
            error.chain().contains("longer than the limit"),
            "{}",
            error.chain()
        );

        // A file larger than the limit, made without writing its bytes.
        std::fs::write(&log_path, &contents).unwrap();
        let file = OpenOptions::new().write(true).open(&log_path).unwrap();
        file.set_len(LARGEST_LOG + 1).unwrap();
        drop(file);
        let error = undo_last_write(&logs).unwrap_err();
        assert!(
            error.chain().contains("larger than the limit"),
            "{}",
            error.chain()
        );
        assert!(std::fs::read(&track).unwrap() == after_write);

        // More lines than the limit.
        let line = b"{\"Failed\":\"elsewhere\"}\n";
        let many_lines = line.repeat(MOST_ENTRIES as usize);
        std::fs::write(&log_path, [&contents[..], &many_lines[..]].concat()).unwrap();
        let error = undo_last_write(&logs).unwrap_err();
        assert!(
            error.chain().contains("more entries than the limit"),
            "{}",
            error.chain()
        );
        assert!(std::fs::read(&track).unwrap() == after_write);

        std::fs::write(&log_path, &contents).unwrap();
        undo_last_write(&logs).unwrap().unwrap();
        std::fs::remove_dir_all(track.parent().unwrap()).unwrap();
    }
}
