// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Playlist identity follows only explicit, successful metadata rewrites.

use std::path::{Path, PathBuf};

use euphoria_core::tags::{
    FieldEdit, LoggedWrite, RewriteStage, TagField, TextChange, WriteLog, apply_plan, plan_edit,
    undo_last_write_with_observer,
};
use euphoria_core::{ContentHash, PlaylistEntry, PlaylistStore, StoredPlaylist};

fn fixture(name: &str) -> (PathBuf, PathBuf) {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("scratch folder can be created");
    let path = folder.join("track.flac");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sine-440hz-mono.flac"),
        &path,
    )
    .expect("fixture can be copied");
    (folder, path)
}

fn genre(value: &str) -> [FieldEdit; 1] {
    [FieldEdit {
        field: TagField::Genre,
        change: TextChange::Set(value.to_string()),
    }]
}

#[test]
fn playlists_follow_explicit_writes_and_undo_but_not_untouched_copies() {
    let (folder, path) = fixture("playlist-explicit-rewrites");
    let original = std::fs::read(&path).unwrap();
    let copy = folder.join("copy.flac");
    std::fs::copy(&path, &copy).unwrap();
    let before = ContentHash::of_file(&path).unwrap();
    let mut store = PlaylistStore::open(&folder.join("playlists.sqlite3")).unwrap();
    store
        .save(&[StoredPlaylist {
            name: "Set".into(),
            label: Some("🔥".into()),
            entries: vec![
                PlaylistEntry {
                    content: before,
                    path: path.clone(),
                },
                PlaylistEntry {
                    content: before,
                    path: copy.clone(),
                },
                PlaylistEntry {
                    content: before,
                    path: folder.join("moved.flac"),
                },
            ],
        }])
        .unwrap();
    let mut old = None;
    let mut observer = |path: &Path, stage| match stage {
        RewriteStage::Before => old = Some(ContentHash::of_file(path).unwrap()),
        RewriteStage::After => {
            store
                .rebind_rewritten_file(
                    path,
                    old.take().unwrap(),
                    ContentHash::of_file(path).unwrap(),
                )
                .unwrap();
        }
    };
    let plan = plan_edit(&path, &genre("Deep House")).unwrap();
    let logs = folder.join("logs");
    assert!(matches!(
        WriteLog::new(&logs)
            .apply_with_observer(&plan, &mut observer)
            .unwrap(),
        LoggedWrite::Recorded
    ));
    let changed = ContentHash::of_file(&path).unwrap();
    let after = PlaylistStore::open(&folder.join("playlists.sqlite3"))
        .unwrap()
        .load()
        .unwrap();
    assert_eq!(after[0].entries[0].content, changed);
    assert_eq!(
        after[0].entries[1],
        PlaylistEntry {
            content: before,
            path: copy
        }
    );
    assert_eq!(
        after[0].entries[2],
        PlaylistEntry {
            content: changed,
            path: path.clone()
        }
    );
    undo_last_write_with_observer(&logs, &mut observer).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(
        store.load().unwrap()[0]
            .entries
            .iter()
            .all(|entry| entry.content == before)
    );
}

#[test]
fn empty_and_refused_plans_never_report_a_successful_rewrite() {
    let (folder, path) = fixture("playlist-rewrite-observer-refusal");
    let mut events = Vec::new();
    let mut observer = |_: &Path, stage| events.push(stage);
    let mut log = WriteLog::new(&folder.join("logs"));
    assert!(matches!(
        log.apply_with_observer(&plan_edit(&path, &[]).unwrap(), &mut observer)
            .unwrap(),
        LoggedWrite::Recorded
    ));
    let stale = plan_edit(&path, &genre("Deep House")).unwrap();
    apply_plan(&plan_edit(&path, &genre("Techno")).unwrap()).unwrap();
    assert!(log.apply_with_observer(&stale, &mut observer).is_err());
    assert_eq!(events, [RewriteStage::Before]);
}

#[cfg(unix)]
#[test]
fn a_symlink_write_rebinds_the_real_file_but_not_a_separate_copy() {
    let (folder, path) = fixture("playlist-rewrite-symlink");
    let alias = folder.join("alias.flac");
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    let before = ContentHash::of_file(&path).unwrap();
    let mut entry = PlaylistEntry {
        path: path.clone(),
        content: before,
    };
    apply_plan(&plan_edit(&alias, &genre("Deep House")).unwrap()).unwrap();
    let after = ContentHash::of_file(&path).unwrap();
    assert!(entry.rebind_rewritten_file(&alias, before, after));
    assert_eq!(entry.path, path);
    assert_eq!(entry.content, after);
}

#[cfg(unix)]
#[test]
fn observers_follow_renames_even_when_the_later_directory_sync_fails() {
    use std::os::unix::fs::PermissionsExt;
    let (folder, path) = fixture("playlist-rewrite-post-commit");
    let original = std::fs::read(&path).unwrap();
    let plan = plan_edit(&path, &genre("Deep House")).unwrap();
    let logs = folder.join("logs");
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o300)).unwrap();
    if std::fs::File::open(&folder).is_ok() {
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
        return; // Privileged users cannot reproduce this permission failure.
    }
    let mut events = Vec::new();
    let written = WriteLog::new(&logs).apply_with_observer(&plan, |_, stage| events.push(stage));
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(written.is_err());
    assert_eq!(events, [RewriteStage::Before, RewriteStage::After]);
    assert_ne!(std::fs::read(&path).unwrap(), original);
    events.clear();
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o300)).unwrap();
    let undone = undo_last_write_with_observer(&logs, |_, stage| events.push(stage));
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(undone.unwrap().unwrap().failed.len(), 1);
    assert_eq!(events, [RewriteStage::Before, RewriteStage::After]);
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[cfg(unix)]
fn logged_genre(log: &mut WriteLog, path: &Path, value: &str) {
    assert!(matches!(
        log.apply(&plan_edit(path, &genre(value)).expect("a planned genre change"))
            .expect("the genre is written"),
        LoggedWrite::Recorded
    ));
}

#[cfg(unix)]
#[test]
fn an_undo_committed_before_a_sync_error_can_be_retried_and_older_logs_remain_reachable() {
    use std::os::unix::fs::PermissionsExt;
    let (folder, path) = fixture("undo-sync-retry-older-logs");
    let original = std::fs::read(&path).unwrap();
    let logs = folder.join("logs");
    logged_genre(&mut WriteLog::new(&logs), &path, "House");
    let middle = std::fs::read(&path).unwrap();
    logged_genre(&mut WriteLog::new(&logs), &path, "Techno");

    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o300)).unwrap();
    if std::fs::File::open(&folder).is_ok() {
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }
    let failed = undo_last_write_with_observer(&logs, |_, _| {});
    std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(failed.unwrap().unwrap().failed.len(), 1);
    assert_eq!(std::fs::read(&path).unwrap(), middle);

    let mut events = Vec::new();
    let retry = undo_last_write_with_observer(&logs, |_, stage| events.push(stage))
        .unwrap()
        .unwrap();
    assert!(retry.failed.is_empty(), "{:?}", retry.failed);
    assert_eq!(retry.untouched.as_slice(), std::slice::from_ref(&path));
    assert!(retry.restored.is_empty());
    assert!(
        events.is_empty(),
        "already-restored files are not rewritten"
    );
    let older = undo_last_write_with_observer(&logs, |_, _| {})
        .unwrap()
        .unwrap();
    assert_eq!(older.restored.as_slice(), std::slice::from_ref(&path));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(
        undo_last_write_with_observer(&logs, |_, _| {})
            .unwrap()
            .is_none()
    );
}

#[cfg(unix)]
#[test]
fn a_failed_undo_defers_older_backups_of_the_same_file_even_through_an_alias() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let (folder, fixture_path) = fixture("undo-sync-retry-same-log");
    let blocked = folder.join("blocked");
    std::fs::create_dir(&blocked).unwrap();
    let path = blocked.join("track.flac");
    std::fs::rename(&fixture_path, &path).unwrap();
    let other = folder.join("other.flac");
    std::fs::copy(&path, &other).unwrap();
    let original = std::fs::read(&path).unwrap();
    let alias = folder.join("track-link.flac");
    symlink(&path, &alias).unwrap();
    let logs = folder.join("logs");
    let mut log = WriteLog::new(&logs);
    logged_genre(&mut log, &alias, "House");
    let middle = std::fs::read(&path).unwrap();
    logged_genre(&mut log, &other, "Ambient");
    logged_genre(&mut log, &path, "Techno");

    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o300)).unwrap();
    if std::fs::File::open(&blocked).is_ok() {
        std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o700)).unwrap();
        return;
    }
    let failed = undo_last_write_with_observer(&logs, |_, _| {});
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o700)).unwrap();
    let failed = failed.unwrap().unwrap();
    assert_eq!(failed.failed.len(), 1);
    assert_eq!(failed.restored.as_slice(), std::slice::from_ref(&other));
    assert_eq!(std::fs::read(&other).unwrap(), original);
    assert_eq!(std::fs::read(&path).unwrap(), middle);

    let mut events = Vec::new();
    let retry = undo_last_write_with_observer(&logs, |path, stage| {
        events.push((path.to_path_buf(), stage))
    })
    .unwrap()
    .unwrap();
    assert!(retry.failed.is_empty(), "{:?}", retry.failed);
    assert_eq!(retry.untouched.as_slice(), std::slice::from_ref(&path));
    assert_eq!(retry.restored.as_slice(), std::slice::from_ref(&alias));
    assert_eq!(
        events,
        [
            (alias.clone(), RewriteStage::Before),
            (alias, RewriteStage::After)
        ]
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert!(
        undo_last_write_with_observer(&logs, |_, _| {})
            .unwrap()
            .is_none()
    );
}
