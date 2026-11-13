// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where VirtualDJ and Engine DJ keep their libraries, whether they are
//! running, and where the copy of a library is saved before it is changed.

use std::path::{Path, PathBuf};

use super::traktor::ModifiedTime;

fn home_folder() -> Option<PathBuf> {
    let variable = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(variable).map(PathBuf::from)
}

/// The VirtualDJ `database.xml` in the user's home folder, when there is
/// one.
pub fn default_virtualdj_database() -> Option<PathBuf> {
    let home = home_folder()?;
    [
        home.join("Library/Application Support/VirtualDJ/database.xml"),
        home.join("Documents/VirtualDJ/database.xml"),
        home.join("OneDrive/Documents/VirtualDJ/database.xml"),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
}

/// The Engine Library folder in the user's Music folder, when there is one.
pub fn default_engine_library() -> Option<PathBuf> {
    let library = home_folder()?.join("Music").join("Engine Library");
    library.is_dir().then_some(library)
}

fn process_is_running(unix_name: &str, windows_image: &str) -> Option<bool> {
    if cfg!(windows) {
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("IMAGENAME eq {windows_image}"), "/NH"])
            .output()
            .ok()?;
        Some(
            String::from_utf8_lossy(&output.stdout)
                .to_lowercase()
                .contains(&windows_image.to_lowercase()),
        )
    } else {
        let output = std::process::Command::new("pgrep")
            .args(["-x", unix_name])
            .output()
            .ok()?;
        Some(output.status.success())
    }
}

/// Whether VirtualDJ is running on this computer, or `None` when that
/// cannot be checked.
pub fn virtualdj_is_running() -> Option<bool> {
    process_is_running("VirtualDJ", "virtualdj.exe")
}

/// Whether Engine DJ is running on this computer, or `None` when that
/// cannot be checked.
pub fn engine_is_running() -> Option<bool> {
    process_is_running("Engine DJ", "Engine DJ.exe")
}

/// Where a copy of the VirtualDJ `database` is saved before it is changed,
/// named for the moment `now`.
pub fn virtualdj_backup_path(database: &Path, now: ModifiedTime) -> PathBuf {
    database
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(
            "database.xml.euphoria-backup-{:04}{:02}{:02}-{:06}",
            now.year, now.month, now.day, now.seconds_of_day
        ))
}

/// Where a copy of the Engine DJ database of `library` is saved before it is
/// changed, named for the moment `now`. The copy sits beside the library.
pub fn engine_backup_path(library: &Path, now: ModifiedTime) -> PathBuf {
    library
        .parent()
        .unwrap_or(library)
        .join(format!(
            "Engine Library backup by Euphoria {:04}-{:02}-{:02} {:05}",
            now.year, now.month, now.day, now.seconds_of_day
        ))
        .join("Database2")
        .join("m.db")
}
