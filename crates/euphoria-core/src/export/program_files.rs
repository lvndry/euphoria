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

/// Whether rekordbox is running on this computer, or `None` when that
/// cannot be checked.
pub fn rekordbox_is_running() -> Option<bool> {
    process_is_running("rekordbox", "rekordbox.exe")
}

/// Whether Serato DJ is running on this computer, or `None` when that
/// cannot be checked.
pub fn serato_is_running() -> Option<bool> {
    process_is_running("Serato DJ Pro", "Serato DJ Pro.exe")
}

/// Whether Traktor is running on this computer, or `None` when that cannot
/// be checked.
pub fn traktor_is_running() -> Option<bool> {
    process_is_running("Traktor", "Traktor.exe")
}

/// The checks of whether each DJ program is running, which a test
/// replaces.
#[derive(Debug, Clone, Copy)]
pub struct DjProgramChecks {
    /// Whether rekordbox is running.
    pub rekordbox: fn() -> Option<bool>,
    /// Whether Serato DJ is running.
    pub serato: fn() -> Option<bool>,
    /// Whether Traktor is running.
    pub traktor: fn() -> Option<bool>,
    /// Whether Engine DJ is running.
    pub engine: fn() -> Option<bool>,
    /// Whether VirtualDJ is running.
    pub virtualdj: fn() -> Option<bool>,
}

impl DjProgramChecks {
    /// The checks of the processes on this computer.
    pub const REAL: DjProgramChecks = DjProgramChecks {
        rekordbox: rekordbox_is_running,
        serato: serato_is_running,
        traktor: traktor_is_running,
        engine: engine_is_running,
        virtualdj: virtualdj_is_running,
    };

    /// The names of the programs that are running. A program that cannot
    /// be checked is not listed.
    pub fn running(&self) -> Vec<&'static str> {
        [
            ("rekordbox", self.rekordbox),
            ("Serato DJ", self.serato),
            ("Traktor", self.traktor),
            ("Engine DJ", self.engine),
            ("VirtualDJ", self.virtualdj),
        ]
        .into_iter()
        .filter(|(_, check)| check() == Some(true))
        .map(|(name, _)| name)
        .collect()
    }
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
