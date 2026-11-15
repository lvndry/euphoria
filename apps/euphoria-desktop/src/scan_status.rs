// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The folders being searched for audio files, and the one line that says
//! so: the same words in the toolbar, the empty table and the logo's
//! tooltip.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// How often the count of files found is redrawn while a search runs.
pub(crate) const COUNT_INTERVAL: Duration = Duration::from_millis(250);

/// The searches running now, each with the paths it was given and the
/// count of audio files its walk has found so far.
#[derive(Debug, Default)]
pub(crate) struct RunningScans {
    next_id: u64,
    running: Vec<RunningScan>,
}

#[derive(Debug)]
struct RunningScan {
    id: u64,
    paths: Vec<PathBuf>,
    found: Arc<AtomicUsize>,
}

impl RunningScans {
    /// Records a search of `paths` and returns its id with the counter its
    /// walk adds to.
    pub(crate) fn begin(&mut self, paths: Vec<PathBuf>) -> (u64, Arc<AtomicUsize>) {
        self.next_id += 1;
        let found = Arc::new(AtomicUsize::new(0));
        self.running.push(RunningScan {
            id: self.next_id,
            paths,
            found: found.clone(),
        });
        (self.next_id, found)
    }

    /// Records that the search `id` is over.
    pub(crate) fn finish(&mut self, id: u64) {
        self.running.retain(|scan| scan.id != id);
    }

    pub(crate) fn is_running(&self) -> bool {
        !self.running.is_empty()
    }

    pub(crate) fn is_still_running(&self, id: u64) -> bool {
        self.running.iter().any(|scan| scan.id == id)
    }

    /// Audio files found so far over every search running.
    pub(crate) fn found_so_far(&self) -> usize {
        self.running
            .iter()
            .map(|scan| scan.found.load(Ordering::Relaxed))
            .sum()
    }

    fn paths(&self) -> Vec<&Path> {
        let mut paths: Vec<&Path> = Vec::new();
        for path in self.running.iter().flat_map(|scan| &scan.paths) {
            if !paths.contains(&path.as_path()) {
                paths.push(path);
            }
        }
        paths
    }

    /// "Looking for audio files in ~/Music/DJ tracks · 214 found", or
    /// `None` when no search runs.
    pub(crate) fn label(&self) -> Option<String> {
        self.is_running().then(|| {
            scan_label(
                &self.paths(),
                euphoria_core::home_folder().as_deref(),
                self.found_so_far(),
            )
        })
    }

    /// [`Self::label`] as the words before the path, the path, and the
    /// words after it with the count, for the toolbar, which cuts the path
    /// further when it has little room and keeps the rest whole.
    pub(crate) fn label_parts(&self) -> Option<(&'static str, Option<String>, String)> {
        self.is_running().then(|| {
            let (lead, path, rest) = euphoria_core::search_description_parts(
                &self.paths(),
                euphoria_core::home_folder().as_deref(),
            );
            (lead, path, rest + &count_suffix(self.found_so_far()))
        })
    }

    /// The full paths being searched, one a line, with the home folder
    /// written as ~. `None` when no search runs or it was given no paths.
    pub(crate) fn tooltip(&self) -> Option<String> {
        let paths = self.paths();
        (!paths.is_empty()).then(|| scan_tooltip(&paths, euphoria_core::home_folder().as_deref()))
    }
}

/// The line shown while `paths` are searched, after `found` audio files.
pub(crate) fn scan_label(paths: &[&Path], home: Option<&Path>, found: usize) -> String {
    euphoria_core::search_description(paths, home) + &count_suffix(found)
}

fn count_suffix(found: usize) -> String {
    if found > 0 {
        format!(" \u{b7} {found} found")
    } else {
        String::new()
    }
}

/// `paths` one a line, whole, with `home` written as ~.
pub(crate) fn scan_tooltip(paths: &[&Path], home: Option<&Path>) -> String {
    paths
        .iter()
        .map(|path| euphoria_core::with_home_as_tilde(path, home))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_is_shown_and_the_count_follows_it() {
        let home = Path::new("/Users/dj");
        let tracks = Path::new("/Users/dj/Music/DJ tracks");
        let edits = Path::new("/Volumes/USB/Edits");
        assert_eq!(scan_label(&[], Some(home), 0), "Looking for audio files");
        assert_eq!(
            scan_label(&[tracks], Some(home), 0),
            "Looking for audio files in ~/Music/DJ tracks"
        );
        assert_eq!(
            scan_label(&[tracks, edits], Some(home), 214),
            "Looking for audio files in ~/Music/DJ tracks and 1 more \u{b7} 214 found"
        );
    }

    #[test]
    fn the_tooltip_keeps_every_path_whole() {
        let home = Path::new("/Users/dj");
        let paths = [
            Path::new("/Users/dj/Music/Collections/2026/Summer/Festival/DJ tracks"),
            Path::new("/Volumes/USB/Edits"),
        ];
        assert_eq!(
            scan_tooltip(&paths, Some(home)),
            "~/Music/Collections/2026/Summer/Festival/DJ tracks\n/Volumes/USB/Edits"
        );
    }

    #[test]
    fn the_same_folder_in_two_searches_is_listed_once() {
        let mut scans = RunningScans::default();
        let (first, first_found) = scans.begin(vec![PathBuf::from("/music/House")]);
        let (_, second_found) = scans.begin(vec![PathBuf::from("/music/House")]);
        first_found.store(3, Ordering::Relaxed);
        second_found.store(4, Ordering::Relaxed);
        assert_eq!(
            scans.label().as_deref(),
            Some("Looking for audio files in /music/House \u{b7} 7 found")
        );
        scans.finish(first);
        assert!(scans.is_running());
        assert!(!scans.is_still_running(first));
    }
}
