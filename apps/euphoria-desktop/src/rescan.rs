// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Looking through the library's folders again: new files join, tracks of
//! files that are gone leave, a moved or renamed file keeps its track, and
//! a file that changed is analyzed again. The cache keeps everything else,
//! so a rescan of an unchanged library analyzes nothing.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;

use euphoria_core::{CachedAnalyzer, ContentHash};
use gpui::Context;

use crate::app::EuphoriaApp;
use crate::state::{Scan, scan_counting};

/// What the folder tree's refresh button says when pointed at.
pub(crate) const RESCAN_TOOLTIP: &str = "Look for new and removed files";

/// The item of a folder's menu that rescans only that folder.
pub(crate) const RESCAN_FOLDER_ITEM: &str = "Rescan this folder";

/// A track under the folders being rescanned, as it was when the rescan
/// started.
#[derive(Debug, Clone)]
pub(crate) struct KnownTrack {
    pub(crate) path: PathBuf,
    pub(crate) is_missing: bool,
    pub(crate) is_analyzed: bool,
    /// The content a playlist or a favorite recorded for the file.
    pub(crate) recorded_content: Option<ContentHash>,
}

/// What a rescan found on disk, worked out off the main thread.
#[derive(Debug, Default)]
pub(crate) struct Rescan {
    /// The new files, and missing files that are back, with what the cache
    /// knows of them.
    pub(crate) found: Scan,
    /// Tracks whose file is gone.
    pub(crate) gone: Vec<PathBuf>,
    /// Tracks whose file moved or was renamed: the old path, the new one
    /// and the content both hold.
    pub(crate) moved: Vec<(PathBuf, PathBuf, ContentHash)>,
    /// Analyzed tracks whose file changed since it was analyzed.
    pub(crate) changed: Vec<PathBuf>,
}

/// Walks `roots` again and compares what is there with `known`, the tracks
/// under them. A track is gone only when its file is not there and the
/// library folder holding it is, so the tracks of a drive that is not
/// connected stay. A gone file is taken as moved when any new file holds
/// the same audio. With a cache, an analyzed
/// file whose size or modification time no longer matches is changed.
///
/// This walks folders and reads files, so call it off the main thread.
pub(crate) fn find_changes(
    roots: &[PathBuf],
    library_folders: &[PathBuf],
    known: &[KnownTrack],
    cache: Option<&CachedAnalyzer>,
    found_so_far: &AtomicUsize,
) -> Rescan {
    let mut found = scan_counting(roots, cache, found_so_far);
    found.folders.clear();
    let on_disk: HashSet<PathBuf> = found.files.iter().cloned().collect();
    let known_by_path: HashMap<&Path, &KnownTrack> = known
        .iter()
        .map(|track| (track.path.as_path(), track))
        .collect();
    let reachable_folders: Vec<&Path> = library_folders
        .iter()
        .filter(|folder| matches!(folder.try_exists(), Ok(true)))
        .map(PathBuf::as_path)
        .collect();
    let gone: Vec<&KnownTrack> = known
        .iter()
        .filter(|track| {
            !track.is_missing
                && !on_disk.contains(&track.path)
                && reachable_folders
                    .iter()
                    .any(|folder| track.path.starts_with(folder))
                && matches!(track.path.try_exists(), Ok(false))
        })
        .collect();
    found.files.retain(|file| {
        known_by_path
            .get(file.as_path())
            .is_none_or(|track| track.is_missing)
    });

    let unknown_files: Vec<PathBuf> = found
        .files
        .iter()
        .filter(|file| !known_by_path.contains_key(file.as_path()))
        .cloned()
        .collect();
    let mut contents: HashMap<PathBuf, Option<ContentHash>> = HashMap::new();
    let mut taken: HashSet<PathBuf> = HashSet::new();
    let mut moved = Vec::new();
    let mut still_gone = Vec::new();
    for track in gone {
        let content = track
            .recorded_content
            .or_else(|| cache.and_then(|cache| cache.last_content_at(&track.path)));
        let destination = content.and_then(|content| {
            move_candidates(&track.path, &unknown_files, &taken)
                .into_iter()
                .find(|candidate| {
                    *contents
                        .entry((*candidate).clone())
                        .or_insert_with(|| ContentHash::of_file(candidate).ok())
                        == Some(content)
                })
                .map(|destination| (destination.clone(), content))
        });
        match destination {
            Some((destination, content)) => {
                taken.insert(destination.clone());
                moved.push((track.path.clone(), destination, content));
            }
            None => still_gone.push(track.path.clone()),
        }
    }
    found.files.retain(|file| !taken.contains(file));
    if let Some(cache) = cache {
        for (_, destination, _) in &moved {
            if let Err(error) = cache.recognise_file(destination) {
                found.problems.push(format!(
                    "Could not record where {} moved: {}.",
                    destination.display(),
                    error.chain()
                ));
            }
        }
    }
    let kept: HashSet<&PathBuf> = found.files.iter().collect();
    found.cached.retain(|file, _| kept.contains(file));
    found.corrections.retain(|file, _| kept.contains(file));
    found.edited_cues.retain(|file, _| kept.contains(file));

    let changed = cache
        .map(|cache| {
            known
                .iter()
                .filter(|track| {
                    track.is_analyzed
                        && on_disk.contains(&track.path)
                        && cache.stored_analysis(&track.path).is_none()
                        && cache.known_content(&track.path).is_none()
                })
                .map(|track| track.path.clone())
                .collect()
        })
        .unwrap_or_default();

    Rescan {
        found,
        gone: still_gone,
        moved,
        changed,
    }
}

/// The new files the gone file at `old` may have become: those with its
/// name first, then the others in its folder, then every other new file.
/// A move and a rename can happen before one rescan.
fn move_candidates<'files>(
    old: &Path,
    new_files: &'files [PathBuf],
    taken: &HashSet<PathBuf>,
) -> Vec<&'files PathBuf> {
    let free = new_files.iter().filter(|file| !taken.contains(*file));
    let same_name = free
        .clone()
        .filter(|file| file.file_name() == old.file_name());
    let same_folder = free
        .clone()
        .filter(|file| file.file_name() != old.file_name() && file.parent() == old.parent());
    let elsewhere =
        free.filter(|file| file.file_name() != old.file_name() && file.parent() != old.parent());
    same_name.chain(same_folder).chain(elsewhere).collect()
}

/// How many tracks a rescan changed, which its toast tells.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RescanCounts {
    pub(crate) added: usize,
    pub(crate) removed: usize,
    pub(crate) moved: usize,
    pub(crate) changed: usize,
}

impl RescanCounts {
    /// "3 new tracks, 1 removed", "1 track moved", or "No changes".
    pub(crate) fn message(self) -> String {
        let parts: Vec<(usize, &str)> = [
            (self.added, "new"),
            (self.removed, "removed"),
            (self.moved, "moved"),
            (self.changed, "changed"),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .collect();
        let Some(((first_count, first_word), rest)) = parts.split_first() else {
            return "No changes".to_string();
        };
        let noun = if *first_count == 1 { "track" } else { "tracks" };
        let first = if *first_word == "new" {
            format!("{first_count} new {noun}")
        } else {
            format!("{first_count} {noun} {first_word}")
        };
        std::iter::once(first)
            .chain(rest.iter().map(|(count, word)| format!("{count} {word}")))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The second line of the toast when the selected track left the library.
pub(crate) const SELECTION_REMOVED: &str =
    "The selected track was removed, so nothing is selected.";

impl EuphoriaApp {
    /// Looks through every library folder again for new, moved, changed
    /// and removed files.
    pub(crate) fn rescan_folders(&mut self, cx: &mut Context<'_, Self>) {
        let roots = self.session.library().folders().to_vec();
        self.rescan(roots, cx);
    }

    /// Looks through `folder` and its subfolders again, leaving the rest of
    /// the library alone.
    pub(crate) fn rescan_folder(&mut self, folder: PathBuf, cx: &mut Context<'_, Self>) {
        self.rescan(vec![folder], cx);
    }

    fn rescan(&mut self, roots: Vec<PathBuf>, cx: &mut Context<'_, Self>) {
        if roots.is_empty() || self.scans.is_running() {
            return;
        }
        let library = self.session.library();
        let library_folders = library.folders().to_vec();
        let known: Vec<KnownTrack> = library
            .tracks()
            .iter()
            .filter(|track| roots.iter().any(|root| track.path.starts_with(root)))
            .map(|track| KnownTrack {
                path: track.path.clone(),
                is_missing: track.is_missing(),
                is_analyzed: track.analysis().is_some(),
                recorded_content: self.session.known_content_of(&track.path),
            })
            .collect();
        let (scan_id, found_so_far) = self.scans.begin(roots.clone());
        cx.notify();
        self.redraw_scan_count(scan_id, cx);
        let cache = self.session.cache();
        cx.spawn(async move |this, cx| {
            let rescan = cx
                .background_executor()
                .spawn(async move {
                    find_changes(
                        &roots,
                        &library_folders,
                        &known,
                        cache.as_deref(),
                        &found_so_far,
                    )
                })
                .await;
            this.update(cx, |this, cx| this.finish_rescan(scan_id, rescan, cx))
                .ok();
        })
        .detach();
    }

    fn finish_rescan(&mut self, scan_id: u64, rescan: Rescan, cx: &mut Context<'_, Self>) {
        self.scans.finish(scan_id);
        let Rescan {
            mut found,
            gone,
            moved,
            changed,
        } = rescan;
        let moved_to: HashMap<PathBuf, PathBuf> = moved
            .iter()
            .map(|(old, new, _)| (old.clone(), new.clone()))
            .collect();
        let follow = |path: PathBuf| moved_to.get(&path).cloned().unwrap_or(path);
        let library = self.session.library();
        let path_at = |index: usize| library.track(index).map(|track| track.path.clone());
        let selected_path = self.selected.and_then(path_at).map(follow);
        let marked_paths: Vec<PathBuf> = self
            .marked
            .iter()
            .filter_map(|index| path_at(*index))
            .map(follow)
            .collect();
        let anchor_path = self.rail_anchor.and_then(path_at).map(follow);

        let mut lists_changed = false;
        let mut moved_count = 0;
        for (old, new, content) in &moved {
            if let Some(changed_lists) = self.session.follow_moved_file(old, new, *content) {
                lists_changed |= changed_lists;
                moved_count += 1;
            }
        }
        let removed = self.session.drop_gone_files(&gone);
        let changed_count = self.session.analyze_changed(&changed);
        let added_files = found.files.clone();
        let problems = std::mem::take(&mut found.problems);
        self.session.add(found);

        let library = self.session.library();
        self.selected = selected_path
            .as_ref()
            .and_then(|path| library.index_of(path));
        let selection_removed = selected_path.is_some() && self.selected.is_none();
        self.marked = marked_paths
            .iter()
            .filter_map(|path| library.index_of(path))
            .collect();
        self.rail_anchor = anchor_path.and_then(|path| library.index_of(&path));
        self.hovered_track = None;
        self.follow_library_indexes();
        if lists_changed {
            self.save_playlists(cx);
            self.save_favorites(cx);
        }
        let names_to_read: Vec<PathBuf> = added_files
            .iter()
            .chain(&changed)
            .chain(moved.iter().map(|(_, new, _)| new))
            .cloned()
            .collect();
        self.read_track_names(names_to_read, cx);
        if !problems.is_empty() {
            self.report(problems.join(" "));
        }

        let message = RescanCounts {
            added: added_files.len(),
            removed,
            moved: moved_count,
            changed: changed_count,
        }
        .message();
        if selection_removed {
            self.show_toast_with_detail(message, SELECTION_REMOVED.to_string(), None, cx);
        } else {
            self.show_toast(message, None, cx);
        }
        self.start_collecting(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toast_counts_each_kind_of_change_and_says_when_there_is_none() {
        let counts = |added, removed, moved, changed| RescanCounts {
            added,
            removed,
            moved,
            changed,
        };
        assert_eq!(counts(0, 0, 0, 0).message(), "No changes");
        assert_eq!(counts(3, 1, 0, 0).message(), "3 new tracks, 1 removed");
        assert_eq!(counts(1, 0, 0, 0).message(), "1 new track");
        assert_eq!(counts(0, 1, 0, 0).message(), "1 track removed");
        assert_eq!(counts(0, 0, 2, 1).message(), "2 tracks moved, 1 changed");
    }

    fn known(path: &Path) -> KnownTrack {
        KnownTrack {
            path: path.to_path_buf(),
            is_missing: false,
            is_analyzed: true,
            recorded_content: None,
        }
    }

    #[test]
    fn a_moved_file_with_a_recorded_content_keeps_its_track() {
        let folder = crate::tag_writing::testing::ScratchFolder::new("rescan-moved");
        let old = folder.0.join("Old name.mp3");
        let new = folder.0.join("sub").join("Old name.mp3");
        let deleted = folder.0.join("Deleted.mp3");
        std::fs::create_dir_all(new.parent().unwrap()).unwrap();
        std::fs::write(&new, b"the moved track").unwrap();
        let content = ContentHash::of_file(&new).unwrap();
        let mut moved_track = known(&old);
        moved_track.recorded_content = Some(content);

        let rescan = find_changes(
            std::slice::from_ref(&folder.0),
            std::slice::from_ref(&folder.0),
            &[moved_track, known(&deleted)],
            None,
            &AtomicUsize::new(0),
        );

        assert_eq!(rescan.moved, vec![(old, new, content)]);
        assert_eq!(rescan.gone, vec![deleted]);
        assert!(rescan.found.files.is_empty());
        assert!(rescan.changed.is_empty());
    }

    #[test]
    fn a_file_moved_and_renamed_across_folders_keeps_its_track() {
        let folder = crate::tag_writing::testing::ScratchFolder::new("rescan-moved-renamed");
        let old = folder.0.join("house").join("Old.mp3");
        let new = folder.0.join("techno").join("New.mp3");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::create_dir_all(new.parent().unwrap()).unwrap();
        std::fs::write(&new, b"the same track").unwrap();
        let content = ContentHash::of_file(&new).unwrap();
        let mut moved_track = known(&old);
        moved_track.recorded_content = Some(content);

        let rescan = find_changes(
            std::slice::from_ref(&folder.0),
            std::slice::from_ref(&folder.0),
            &[moved_track],
            None,
            &AtomicUsize::new(0),
        );

        assert_eq!(rescan.moved, vec![(old, new, content)]);
        assert!(rescan.gone.is_empty());
        assert!(rescan.found.files.is_empty());
    }

    #[test]
    fn the_tracks_of_a_folder_that_is_not_there_are_not_gone() {
        let unplugged =
            std::env::temp_dir().join(format!("euphoria-rescan-unplugged-{}", std::process::id()));
        let track = known(&unplugged.join("track.mp3"));
        let rescan = find_changes(
            std::slice::from_ref(&unplugged),
            std::slice::from_ref(&unplugged),
            &[track],
            None,
            &AtomicUsize::new(0),
        );
        assert!(rescan.gone.is_empty());
        assert!(rescan.found.files.is_empty());
    }
}
