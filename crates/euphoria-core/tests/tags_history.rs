// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The log of tag writes: a write made through it can be undone later
//! from what is on disk, file by file, also after an interruption.

use std::path::{Path, PathBuf};

use euphoria_core::tags::{
    LoggedWrite, TagValues, TagWriteOptions, WriteLog, apply_plan, plan_write, read_tags,
    undo_last_write,
};
use euphoria_core::{EuphoriaError, Key, Mode};

const FIXTURES: [&str; 8] = [
    "sine-440hz-mono.mp3",
    "sine-440hz-mono.wav",
    "sine-440hz-mono.aiff",
    "sine-440hz-mono.m4a",
    "sine-440hz-mono.flac",
    "sine-440hz-mono.ogg",
    "sine-440hz-mono-alac.m4a",
    "sine-440hz-mono.mp4",
];

/// A folder of its own for one test, with a copy of every fixture in
/// `tracks` and an empty place for the logs.
struct Scratch {
    folder: PathBuf,
}

impl Scratch {
    fn new(test: &str) -> Self {
        let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("history-{test}"));
        if folder.exists() {
            std::fs::remove_dir_all(&folder).expect("the old scratch folder is removed");
        }
        std::fs::create_dir_all(folder.join("tracks")).expect("the scratch folder is created");
        for fixture in FIXTURES {
            let source = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join(fixture);
            std::fs::copy(source, folder.join("tracks").join(fixture))
                .expect("the fixture is copied");
        }
        Self { folder }
    }

    fn tracks(&self) -> Vec<PathBuf> {
        FIXTURES
            .iter()
            .map(|fixture| self.folder.join("tracks").join(fixture))
            .collect()
    }

    fn logs(&self) -> PathBuf {
        self.folder.join("logs")
    }

    fn log_files(&self) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(self.logs()) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = entries
            .map(|entry| entry.expect("the log folder is listed").path())
            .collect();
        files.sort();
        files
    }

    fn contents(&self) -> Vec<Vec<u8>> {
        self.tracks()
            .iter()
            .map(|track| std::fs::read(track).expect("the track is read"))
            .collect()
    }

    /// Writes `values` to every track through one log.
    fn write(&self, values: TagValues) {
        let mut log = WriteLog::new(&self.logs());
        for track in self.tracks() {
            let plan = plan_write(&track, values, &TagWriteOptions::default())
                .expect("the write is planned");
            let outcome = log.apply(&plan).expect("the write is applied");
            assert!(matches!(outcome, LoggedWrite::Recorded), "{outcome:?}");
        }
    }

    fn keys(&self) -> Vec<Option<String>> {
        self.tracks()
            .iter()
            .map(|track| read_tags(track).expect("the tags are read").key)
            .collect()
    }
}

fn values(tonic: u8, energy: u8) -> TagValues {
    TagValues {
        key: Some(Key::new(tonic, Mode::Minor)),
        bpm: Some(126.0),
        energy: Some(energy),
    }
}

fn all(key: Option<&str>) -> Vec<Option<String>> {
    vec![key.map(str::to_string); FIXTURES.len()]
}

#[test]
fn logged_write_is_undone_to_the_exact_bytes() {
    let scratch = Scratch::new("round-trip");
    let original = scratch.contents();
    scratch.write(values(9, 7));
    assert_eq!(scratch.keys(), all(Some("8A")));
    assert_eq!(scratch.log_files().len(), 1);

    let undone = undo_last_write(&scratch.logs()).unwrap().unwrap();
    assert_eq!(undone.restored, scratch.tracks());
    assert!(undone.untouched.is_empty() && undone.failed.is_empty());
    assert!(scratch.contents() == original);
    assert!(scratch.log_files().is_empty());
    assert!(undo_last_write(&scratch.logs()).unwrap().is_none());
}

#[test]
fn each_undo_takes_back_one_write_newest_first() {
    let scratch = Scratch::new("two-writes");
    let original = scratch.contents();
    scratch.write(values(9, 7));
    let after_first = scratch.contents();
    scratch.write(values(4, 3));
    assert_eq!(scratch.keys(), all(Some("9A")));
    assert_eq!(scratch.log_files().len(), 2);

    undo_last_write(&scratch.logs()).unwrap().unwrap();
    assert!(scratch.contents() == after_first);
    undo_last_write(&scratch.logs()).unwrap().unwrap();
    assert!(scratch.contents() == original);
    assert!(undo_last_write(&scratch.logs()).unwrap().is_none());
}

#[test]
fn nothing_to_undo_without_a_log_folder_or_a_change() {
    let scratch = Scratch::new("nothing");
    assert!(undo_last_write(&scratch.logs()).unwrap().is_none());

    scratch.write(values(9, 7));
    undo_last_write(&scratch.logs()).unwrap().unwrap();
    // A write that changes nothing leaves no log behind.
    scratch.write(TagValues::default());
    assert!(scratch.log_files().is_empty());
    assert!(undo_last_write(&scratch.logs()).unwrap().is_none());
}

#[test]
fn file_edited_after_the_write_is_reported_and_kept_for_a_later_undo() {
    let scratch = Scratch::new("conflict");
    let original = scratch.contents();
    scratch.write(values(9, 7));
    let edited = &scratch.tracks()[2];
    let written = std::fs::read(edited).unwrap();
    // Another program changes the key of one file.
    let other_plan = plan_write(edited, values(0, 7), &TagWriteOptions::default()).unwrap();
    apply_plan(&other_plan).unwrap();

    let undone = undo_last_write(&scratch.logs()).unwrap().unwrap();
    assert_eq!(undone.restored.len(), FIXTURES.len() - 1);
    assert_eq!(undone.failed.len(), 1);
    let (failed_path, error) = &undone.failed[0];
    assert_eq!(failed_path, edited);
    assert!(
        matches!(error, EuphoriaError::TagConflict { .. }),
        "{error:?}"
    );
    assert_eq!(read_tags(edited).unwrap().key.as_deref(), Some("5A"));
    assert_eq!(scratch.log_files().len(), 1);

    // Once the file is back to what the write left, undo finishes the job.
    std::fs::write(edited, written).unwrap();
    let undone = undo_last_write(&scratch.logs()).unwrap().unwrap();
    assert_eq!(undone.restored, std::slice::from_ref(edited));
    assert!(scratch.contents() == original);
    assert!(scratch.log_files().is_empty());
}

#[test]
fn write_given_up_after_planning_does_not_stand_in_the_way_of_undo() {
    let scratch = Scratch::new("stale-plan");
    let original = scratch.contents();
    scratch.write(values(9, 7));
    let after_first = scratch.contents();
    assert_eq!(scratch.log_files().len(), 1);

    // A second write is planned for every file. Before it is applied,
    // another program changes two of them, so their plans no longer hold.
    let options = TagWriteOptions::default();
    let plans: Vec<_> = scratch
        .tracks()
        .iter()
        .map(|track| plan_write(track, values(4, 3), &options).unwrap())
        .collect();
    let changed_meanwhile = [scratch.tracks()[1].clone(), scratch.tracks()[3].clone()];
    for track in &changed_meanwhile {
        apply_plan(&plan_write(track, values(0, 5), &options).unwrap()).unwrap();
    }
    let mut log = WriteLog::new(&scratch.logs());
    let mut conflicts = Vec::new();
    for plan in &plans {
        match log.apply(plan) {
            Ok(outcome) => assert!(matches!(outcome, LoggedWrite::Recorded), "{outcome:?}"),
            Err(error) => {
                assert!(
                    matches!(error, EuphoriaError::TagConflict { .. }),
                    "{error:?}"
                );
                conflicts.push(plan.path().to_path_buf());
            }
        }
    }
    assert_eq!(conflicts, changed_meanwhile);
    assert_eq!(scratch.log_files().len(), 2);

    // Undoing the second write restores the three files it changed and
    // has nothing to say about the two it never touched.
    let undone = undo_last_write(&scratch.logs()).unwrap().unwrap();
    assert_eq!(undone.restored.len(), FIXTURES.len() - 2);
    assert!(undone.failed.is_empty() && undone.untouched.is_empty());
    assert_eq!(scratch.log_files().len(), 1);
    for track in &changed_meanwhile {
        assert_eq!(read_tags(track).unwrap().key.as_deref(), Some("5A"));
    }

    // The first write is reachable. The two files the other program
    // changed are reported, the rest go back to the original.
    let undone = undo_last_write(&scratch.logs()).unwrap().unwrap();
    assert_eq!(undone.restored.len(), FIXTURES.len() - 2);
    assert_eq!(undone.failed.len(), 2);
    for (index, track) in scratch.tracks().iter().enumerate() {
        if changed_meanwhile.contains(track) {
            std::fs::write(track, &after_first[index]).unwrap();
        } else {
            assert!(std::fs::read(track).unwrap() == original[index]);
        }
    }
    undo_last_write(&scratch.logs()).unwrap().unwrap();
    assert!(scratch.contents() == original);
    assert!(undo_last_write(&scratch.logs()).unwrap().is_none());
}

#[test]
fn write_in_which_every_file_was_given_up_is_skipped_by_undo() {
    let scratch = Scratch::new("all-stale");
    let original = scratch.contents();
    scratch.write(values(9, 7));
    let after_first = scratch.contents();

    let options = TagWriteOptions::default();
    let track = scratch.tracks().remove(0);
    let stale_plan = plan_write(&track, values(4, 3), &options).unwrap();
    apply_plan(&plan_write(&track, values(0, 5), &options).unwrap()).unwrap();
    let mut log = WriteLog::new(&scratch.logs());
    log.apply(&stale_plan).unwrap_err();
    assert_eq!(scratch.log_files().len(), 2);
    std::fs::write(&track, &after_first[0]).unwrap();

    // The newest log changed no file, so undo goes on to the write before.
    let undone = undo_last_write(&scratch.logs()).unwrap().unwrap();
    assert_eq!(undone.restored, scratch.tracks());
    assert!(scratch.contents() == original);
    assert!(scratch.log_files().is_empty());
}

#[test]
fn interrupted_write_is_still_undone() {
    let scratch = Scratch::new("interrupted");
    let original = scratch.contents();
    scratch.write(values(9, 7));
    let after_write = scratch.contents();
    let log_file = scratch.log_files().remove(0);
    let log = std::fs::read_to_string(&log_file).unwrap();
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), FIXTURES.len() * 2);

    // The program stopped after changing the last file, before noting it.
    let unconfirmed = lines[..lines.len() - 1].join("\n") + "\n";
    std::fs::write(&log_file, &unconfirmed).unwrap();
    let undone = undo_last_write(&scratch.logs()).unwrap().unwrap();
    assert_eq!(undone.restored, scratch.tracks());
    assert!(scratch.contents() == original);

    // The program stopped after saving the last backup, before changing
    // the file, and in the middle of a line.
    scratch.write(values(9, 7));
    assert!(scratch.contents() == after_write);
    let last_track = scratch.tracks().pop().unwrap();
    std::fs::write(&last_track, original.last().unwrap()).unwrap();
    let log_file = scratch.log_files().remove(0);
    let log = std::fs::read_to_string(&log_file).unwrap();
    let lines: Vec<&str> = log.lines().collect();
    let cut_off = lines[..lines.len() - 1].join("\n") + "\n{\"Applied\":\"/half a li";
    std::fs::write(&log_file, cut_off).unwrap();
    let undone = undo_last_write(&scratch.logs()).unwrap().unwrap();
    assert_eq!(undone.restored.len(), FIXTURES.len() - 1);
    assert_eq!(undone.untouched, [last_track]);
    assert!(undone.failed.is_empty());
    assert!(scratch.contents() == original);
    assert!(scratch.log_files().is_empty());
}

#[test]
fn damaged_log_is_an_error_and_changes_nothing() {
    let scratch = Scratch::new("damaged");
    scratch.write(values(9, 7));
    let after_write = scratch.contents();
    let log_file = scratch.log_files().remove(0);
    let log = std::fs::read_to_string(&log_file).unwrap();
    std::fs::write(&log_file, format!("not a log line\n{log}")).unwrap();

    let error = undo_last_write(&scratch.logs()).unwrap_err();
    assert!(
        matches!(&error, EuphoriaError::TagWriteLog { path, .. } if *path == log_file),
        "{error:?}"
    );
    assert!(scratch.contents() == after_write);
}
