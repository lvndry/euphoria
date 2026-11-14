// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Finds the audio files under the paths a command was given, tells the user
//! about folders that could not be read and files left out for their format,
//! and remembers that the scan was incomplete so the exit status says so.

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use euphoria_core::{AudioScan, scan_audio_files};

use crate::output;
use crate::style::Theme;
use crate::terminal::Printable;
use crate::with_causes;

/// The most unreadable folders listed with their reasons before the rest are
/// only counted.
const MAXIMUM_LISTED_UNREADABLE: usize = 10;

static SCAN_WAS_INCOMPLETE: AtomicBool = AtomicBool::new(false);

/// Scans `paths`, writes what it could not read and what it left out to
/// standard error, and returns the scan. An unreadable folder does not stop
/// the command: the command goes on with the files found, and ends with
/// [`MISSING_FILES_EXIT_STATUS`](output::MISSING_FILES_EXIT_STATUS) unless
/// something failed.
pub fn scan(paths: &[PathBuf]) -> Result<AudioScan> {
    let scan = scan_quietly(paths)?;
    write_notices(&mut io::stderr().lock(), &Theme::stderr(), &scan)?;
    Ok(scan)
}

/// The audio files under `paths`, reporting as [`scan`] does.
pub fn find_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    scan(paths).map(|scan| scan.files)
}

/// Scans `paths` and remembers an incomplete scan, but writes nothing, for a
/// command that writes the notices with its other messages.
pub fn scan_quietly(paths: &[PathBuf]) -> Result<AudioScan> {
    let scan = scan_audio_files(paths).map_err(with_causes)?;
    if scan.has_unreadable() {
        SCAN_WAS_INCOMPLETE.store(true, Ordering::Relaxed);
    }
    Ok(scan)
}

/// True when a scan of this run could not read some folder.
pub fn was_incomplete() -> bool {
    SCAN_WAS_INCOMPLETE.load(Ordering::Relaxed)
}

/// Writes a warning for the folders that could not be read, each with the
/// system's reason, and one for the files left out for their format.
pub fn write_notices(writer: &mut impl Write, theme: &Theme, scan: &AudioScan) -> io::Result<()> {
    if let Some(summary) = scan.unreadable_summary() {
        output::write_warning(writer, theme, Printable(summary))?;
        for warning in scan.unreadable.iter().take(MAXIMUM_LISTED_UNREADABLE) {
            writeln!(
                writer,
                "  {}: {}",
                Printable(warning.path.display()),
                Printable(&warning.reason)
            )?;
        }
        if scan.unreadable.len() > MAXIMUM_LISTED_UNREADABLE {
            writeln!(
                writer,
                "  and {} more",
                scan.unreadable.len() - MAXIMUM_LISTED_UNREADABLE
            )?;
        }
        output::write_hint(
            writer,
            theme,
            "The files in them were not analyzed. Fix the permissions or reconnect the drive, then run the command again.",
        )?;
    }
    if let Some(summary) = scan.skipped_summary() {
        output::write_warning(writer, theme, Printable(summary))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use euphoria_core::{ScanWarning, SkippedExtension};

    use super::*;

    fn notices(scan: &AudioScan) -> String {
        let mut buffer = Vec::new();
        write_notices(&mut buffer, &Theme::PLAIN, scan).unwrap();
        String::from_utf8(buffer).unwrap()
    }

    #[test]
    fn a_complete_scan_writes_nothing() {
        assert_eq!(notices(&AudioScan::default()), "");
    }

    #[test]
    fn unreadable_folders_and_skipped_files_are_each_summarised() {
        let scan = AudioScan {
            unreadable: vec![
                ScanWarning {
                    path: "/music/a".into(),
                    reason: "Permission denied (os error 13)".into(),
                    is_folder: true,
                },
                ScanWarning {
                    path: "/music/b".into(),
                    reason: "Input/output error (os error 5)".into(),
                    is_folder: true,
                },
            ],
            skipped: vec![
                SkippedExtension {
                    extension: "opus".into(),
                    count: 9,
                },
                SkippedExtension {
                    extension: "wma".into(),
                    count: 5,
                },
            ],
            ..AudioScan::default()
        };
        let text = notices(&scan);
        assert!(text.contains("warning: 2 folders could not be read: /music/a, /music/b"));
        assert!(text.contains("/music/a: Permission denied (os error 13)"));
        assert!(text.contains(
            "warning: 14 files skipped: 9 .opus, 5 .wma, which Euphoria can't decode yet"
        ));
    }
}
