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

/// The most characters of a folder's name the line shows before it ends the
/// name with an ellipsis, so the line fits the toolbar of the narrowest
/// window.
const FOLDER_NAME_LIMIT: usize = 24;

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

    /// "Looking for audio files in DJ tracks · 214 found", or `None` when no
    /// search runs.
    pub(crate) fn label(&self) -> Option<String> {
        self.is_running()
            .then(|| scan_label(&self.paths(), self.found_so_far()))
    }

    /// The full paths being searched, one a line, with the home folder
    /// written as ~. `None` when no search runs or it was given no paths.
    pub(crate) fn tooltip(&self) -> Option<String> {
        let paths = self.paths();
        (!paths.is_empty()).then(|| scan_tooltip(&paths, home_folder().as_deref()))
    }
}

/// The line shown while `paths` are searched, after `found` audio files.
pub(crate) fn scan_label(paths: &[&Path], found: usize) -> String {
    let mut label = String::from("Looking for audio files");
    match paths {
        [] => {}
        [path] => {
            label.push_str(" in ");
            label.push_str(&shortened(&folder_name(path)));
        }
        several => label.push_str(&format!(" in {} folders", several.len())),
    }
    if found > 0 {
        label.push_str(&format!(" \u{b7} {found} found"));
    }
    label
}

/// `paths` one a line, with `home` written as ~.
pub(crate) fn scan_tooltip(paths: &[&Path], home: Option<&Path>) -> String {
    paths
        .iter()
        .map(|path| with_home_shortened(path, home))
        .collect::<Vec<_>>()
        .join("\n")
}

fn folder_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

fn shortened(name: &str) -> String {
    if name.chars().count() <= FOLDER_NAME_LIMIT {
        return name.to_string();
    }
    let kept: String = name.chars().take(FOLDER_NAME_LIMIT - 1).collect();
    format!("{}\u{2026}", kept.trim_end())
}

fn with_home_shortened(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

fn home_folder() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_folder_is_named_and_several_are_counted() {
        let tracks = Path::new("/Users/dj/Music/DJ tracks");
        let edits = Path::new("/Volumes/USB/Edits");
        assert_eq!(scan_label(&[], 0), "Looking for audio files");
        assert_eq!(
            scan_label(&[tracks], 0),
            "Looking for audio files in DJ tracks"
        );
        assert_eq!(
            scan_label(&[tracks, edits], 214),
            "Looking for audio files in 2 folders \u{b7} 214 found"
        );
    }

    #[test]
    fn a_long_folder_name_ends_with_an_ellipsis() {
        let long = Path::new("/music/Extended mixes from the summer festival season");
        assert_eq!(
            scan_label(&[long], 0),
            "Looking for audio files in Extended mixes from the\u{2026}"
        );
    }

    #[test]
    fn the_tooltip_writes_the_home_folder_as_a_tilde() {
        let home = Path::new("/Users/dj");
        let paths = [
            Path::new("/Users/dj/Music/DJ tracks"),
            Path::new("/Volumes/USB/Edits"),
            Path::new("/Users/dj"),
        ];
        assert_eq!(
            scan_tooltip(&paths, Some(home)),
            "~/Music/DJ tracks\n/Volumes/USB/Edits\n~"
        );
        assert_eq!(scan_tooltip(&paths[..1], None), "/Users/dj/Music/DJ tracks");
    }

    #[test]
    fn the_same_folder_in_two_searches_is_named_once() {
        let mut scans = RunningScans::default();
        let (first, first_found) = scans.begin(vec![PathBuf::from("/music/House")]);
        let (_, second_found) = scans.begin(vec![PathBuf::from("/music/House")]);
        first_found.store(3, Ordering::Relaxed);
        second_found.store(4, Ordering::Relaxed);
        assert_eq!(
            scans.label().as_deref(),
            Some("Looking for audio files in House \u{b7} 7 found")
        );
        scans.finish(first);
        assert!(scans.is_running());
        assert!(!scans.is_still_running(first));
    }
}
