// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The folders the user added, as a tree of the subfolders that hold their
//! tracks, and the folder of each track as the table shows it: relative to
//! the folder on show.
//!
//! Everything here works on the paths the library already holds. Nothing
//! reads the disk, so a folder that has since disappeared is simply absent.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::{MAIN_SEPARATOR_STR, Path, PathBuf};

use crate::library::Track;

/// One folder in the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FolderNode {
    /// Where the folder is.
    pub(crate) path: PathBuf,
    /// The folder's own name.
    pub(crate) name: String,
    /// 0 for a folder the user added, 1 for a folder inside it, and so on.
    pub(crate) depth: usize,
    /// The folder holding this one, unless the user added this one.
    pub(crate) parent: Option<usize>,
    /// The folders inside, in name order, each holding at least one track.
    pub(crate) children: Vec<usize>,
    /// How many tracks are in this folder and the folders inside it.
    pub(crate) track_count: usize,
    /// How many of those tracks need review: a key or tempo rated low, or
    /// a file that could not be analyzed.
    pub(crate) review_count: usize,
}

/// The folders the user added and the subfolders holding their tracks.
#[derive(Debug, Default)]
pub(crate) struct FolderTree {
    nodes: Vec<FolderNode>,
    roots: Vec<usize>,
    index_by_path: HashMap<PathBuf, usize>,
    /// For each track, by its index in the library, the folder holding it,
    /// or `None` when it is outside every added folder.
    folder_of_track: Vec<Option<usize>>,
}

/// A key pressed while the folder tree has the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TreeKey {
    /// The folder above.
    Up,
    /// The folder below.
    Down,
    /// Folds the folder, or goes to the folder holding it.
    Left,
    /// Unfolds the folder, or goes to the first folder inside it.
    Right,
    /// The first folder.
    First,
    /// The last folder shown.
    Last,
}

/// What a key press does to the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TreeMove {
    /// Shows the folder at this index.
    Select(usize),
    /// Unfolds the folder at this index.
    Expand(usize),
    /// Folds the folder at this index.
    Collapse(usize),
}

/// The folders the user added that are not inside another one, each once, in
/// the order they were added. A folder added inside another added folder is
/// shown as part of the outer folder's tree, so its tracks are not counted
/// twice.
pub(crate) fn outermost_folders(folders: &[PathBuf]) -> Vec<&Path> {
    let mut outermost: Vec<&Path> = Vec::new();
    for folder in folders {
        let is_inside_another = folders
            .iter()
            .any(|other| other != folder && folder.starts_with(other));
        if !is_inside_another && !outermost.contains(&folder.as_path()) {
            outermost.push(folder);
        }
    }
    outermost
}

/// The last part of `path`, or the whole path when it has no last part.
pub(crate) fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

impl FolderTree {
    /// Builds the tree of `folders` from the paths of `tracks`. A subfolder
    /// appears only when it holds a track somewhere under it; a folder the
    /// user added always appears, so it can be seen even while it is empty.
    pub(crate) fn build<'track>(
        folders: &[PathBuf],
        tracks: impl IntoIterator<Item = &'track Track>,
    ) -> Self {
        let mut tree = Self::default();
        let outermost = outermost_folders(folders);
        for root in &outermost {
            let index = tree.push(root.to_path_buf(), None);
            tree.roots.push(index);
        }
        for track in tracks {
            tree.folder_of_track.push(None);
            let Some(root) = outermost.iter().find(|root| track.path.starts_with(root)) else {
                continue;
            };
            let Some(relative) = track
                .path
                .parent()
                .and_then(|parent| parent.strip_prefix(root).ok())
            else {
                continue;
            };
            let is_to_review = track.needs_review();
            let mut current = tree.index_by_path[*root];
            tree.count(current, is_to_review);
            let mut path = root.to_path_buf();
            for component in relative.components() {
                path.push(component);
                current = match tree.index_by_path.get(&path) {
                    Some(index) => *index,
                    None => {
                        let index = tree.push(path.clone(), Some(current));
                        tree.nodes[current].children.push(index);
                        index
                    }
                };
                tree.count(current, is_to_review);
            }
            if let Some(folder) = tree.folder_of_track.last_mut() {
                *folder = Some(current);
            }
        }
        for index in 0..tree.nodes.len() {
            let mut children = std::mem::take(&mut tree.nodes[index].children);
            children.sort_by(|first, second| {
                compare_names(&tree.nodes[*first].name, &tree.nodes[*second].name)
            });
            tree.nodes[index].children = children;
        }
        tree
    }

    fn push(&mut self, path: PathBuf, parent: Option<usize>) -> usize {
        let index = self.nodes.len();
        let depth = parent.map_or(0, |parent| self.nodes[parent].depth + 1);
        self.index_by_path.insert(path.clone(), index);
        self.nodes.push(FolderNode {
            name: folder_name(&path),
            path,
            depth,
            parent,
            children: Vec::new(),
            track_count: 0,
            review_count: 0,
        });
        index
    }

    fn count(&mut self, index: usize, is_to_review: bool) {
        let node = &mut self.nodes[index];
        node.track_count += 1;
        if is_to_review {
            node.review_count += 1;
        }
    }

    /// Counts again how many tracks need review under each folder, after results
    /// came in for `tracks`, the same tracks the tree was built from. No
    /// paths are looked at: each track's folder is already known.
    pub(crate) fn recount_reviews(&mut self, tracks: &[Track]) {
        for node in &mut self.nodes {
            node.review_count = 0;
        }
        for (track, folder) in tracks.iter().zip(&self.folder_of_track) {
            if !track.needs_review() {
                continue;
            }
            let mut candidate = *folder;
            while let Some(index) = candidate {
                self.nodes[index].review_count += 1;
                candidate = self.nodes[index].parent;
            }
        }
    }

    /// The folder at `index`.
    pub(crate) fn node(&self, index: usize) -> Option<&FolderNode> {
        self.nodes.get(index)
    }

    /// The index of the folder at `path`, when it is in the tree.
    pub(crate) fn index_of(&self, path: &Path) -> Option<usize> {
        self.index_by_path.get(path).copied()
    }

    /// The folders to list, top to bottom: each added folder, followed by
    /// the folders inside it when it is in `expanded`, and so on down.
    pub(crate) fn visible_rows(&self, expanded: &HashSet<PathBuf>) -> Vec<usize> {
        let mut rows = Vec::new();
        let mut waiting: Vec<usize> = self.roots.iter().rev().copied().collect();
        while let Some(index) = waiting.pop() {
            rows.push(index);
            let node = &self.nodes[index];
            if expanded.contains(&node.path) {
                waiting.extend(node.children.iter().rev().copied());
            }
        }
        rows
    }

    /// What `key` does when the folder at `current` is shown and `rows` are
    /// listed. A shown folder hidden inside a folded one counts as the
    /// folded one.
    pub(crate) fn navigate(
        &self,
        rows: &[usize],
        expanded: &HashSet<PathBuf>,
        current: Option<usize>,
        key: TreeKey,
    ) -> Option<TreeMove> {
        let first = *rows.first()?;
        let last = *rows.last()?;
        let position = current.and_then(|current| self.visible_position(rows, current));
        let Some(position) = position else {
            return match key {
                TreeKey::Down | TreeKey::First | TreeKey::Right => Some(TreeMove::Select(first)),
                TreeKey::Up | TreeKey::Last => Some(TreeMove::Select(last)),
                TreeKey::Left => None,
            };
        };
        let index = rows[position];
        let node = &self.nodes[index];
        let is_expanded = expanded.contains(&node.path);
        match key {
            TreeKey::Up => position
                .checked_sub(1)
                .map(|above| TreeMove::Select(rows[above])),
            TreeKey::Down => rows.get(position + 1).map(|below| TreeMove::Select(*below)),
            TreeKey::First => (index != first).then_some(TreeMove::Select(first)),
            TreeKey::Last => (index != last).then_some(TreeMove::Select(last)),
            TreeKey::Left if is_expanded && !node.children.is_empty() => {
                Some(TreeMove::Collapse(index))
            }
            TreeKey::Left => node.parent.map(TreeMove::Select),
            TreeKey::Right if node.children.is_empty() => None,
            TreeKey::Right if is_expanded => Some(TreeMove::Select(node.children[0])),
            TreeKey::Right => Some(TreeMove::Expand(index)),
        }
    }

    /// Where `index`, or the closest folder holding it, is among `rows`.
    fn visible_position(&self, rows: &[usize], index: usize) -> Option<usize> {
        let mut candidate = Some(index);
        while let Some(index) = candidate {
            if let Some(position) = rows.iter().position(|row| *row == index) {
                return Some(position);
            }
            candidate = self.nodes[index].parent;
        }
        None
    }
}

/// Folder names in the order people expect: letter case does not matter,
/// and the exact spelling only breaks ties.
fn compare_names(first: &str, second: &str) -> Ordering {
    first
        .to_lowercase()
        .cmp(&second.to_lowercase())
        .then_with(|| first.cmp(second))
}

/// Where a track's folder is, as the Folder column shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct FolderLabel {
    /// The folders from the one on show down to the track's, joined by the
    /// path separator. Empty when the track is directly in the folder on
    /// show.
    pub(crate) text: String,
    /// The track's folder, in full.
    pub(crate) full_path: PathBuf,
    /// Where the added folder holding the track comes among the added
    /// folders, when the label starts from one; 0 when it starts from the
    /// folder on show, and after every added folder for a track outside
    /// them.
    root_rank: usize,
    /// The folder names below that start, which order tracks as the tree
    /// orders folders.
    names_below: Vec<String>,
}

/// The Folder column's text for the track at `track_path`.
///
/// With a folder on show, it is the path from that folder to the track's.
/// Otherwise it is the path from the added folder holding the track, led by
/// that folder's name when several folders were added. A track outside every
/// added folder shows the name of its own folder.
pub(crate) fn folder_label(
    track_path: &Path,
    shown_folder: Option<&Path>,
    outermost: &[&Path],
) -> FolderLabel {
    let parent = track_path.parent().unwrap_or(Path::new(""));
    let (root_rank, root_name, names_below) =
        match shown_folder.filter(|folder| parent.starts_with(folder)) {
            Some(folder) => (0, None, names_between(folder, parent)),
            None => match outermost.iter().position(|root| parent.starts_with(root)) {
                Some(rank) => {
                    let root = outermost[rank];
                    let root_name = (outermost.len() > 1).then(|| folder_name(root));
                    (rank, root_name, names_between(root, parent))
                }
                None if parent.as_os_str().is_empty() => (outermost.len(), None, Vec::new()),
                None => (outermost.len(), None, vec![folder_name(parent)]),
            },
        };
    let shown_names: Vec<&str> = root_name
        .iter()
        .chain(&names_below)
        .map(String::as_str)
        .collect();
    FolderLabel {
        text: shown_names.join(MAIN_SEPARATOR_STR),
        full_path: parent.to_path_buf(),
        root_rank,
        names_below,
    }
}

fn names_between(ancestor: &Path, path: &Path) -> Vec<String> {
    path.strip_prefix(ancestor)
        .map(|relative| {
            relative
                .components()
                .map(|component| component.as_os_str().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// The Folder column for every track in the library, worked out once for
/// the folder on show.
#[derive(Debug, Default)]
pub(crate) struct FolderColumn {
    labels: Vec<FolderLabel>,
}

impl FolderColumn {
    /// The labels of `tracks` with `shown_folder` on show, or every track
    /// when it is `None`.
    pub(crate) fn new(tracks: &[Track], folders: &[PathBuf], shown_folder: Option<&Path>) -> Self {
        let outermost = outermost_folders(folders);
        Self {
            labels: tracks
                .iter()
                .map(|track| folder_label(&track.path, shown_folder, &outermost))
                .collect(),
        }
    }

    /// The label of the track at `index` in the library.
    pub(crate) fn label(&self, index: usize) -> Option<&FolderLabel> {
        self.labels.get(index)
    }

    /// Orders two tracks by their folder path in the order of the sidebar
    /// tree: added folders in the order they were added, then a folder at a
    /// time by name, so that tracks directly in a folder come before those
    /// in the folders inside it.
    pub(crate) fn compare(&self, first: usize, second: usize) -> Ordering {
        match (self.labels.get(first), self.labels.get(second)) {
            (Some(first), Some(second)) => first.root_rank.cmp(&second.root_rank).then_with(|| {
                first
                    .names_below
                    .iter()
                    .zip(&second.names_below)
                    .map(|(first, second)| compare_names(first, second))
                    .find(|ordering| ordering.is_ne())
                    .unwrap_or_else(|| first.names_below.len().cmp(&second.names_below.len()))
            }),
            (first, second) => first.is_some().cmp(&second.is_some()).reverse(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::Library;

    fn library(folders: &[&str], files: &[&str]) -> Library {
        let mut library = Library::default();
        for folder in folders {
            library.add_folder(PathBuf::from(folder));
        }
        library.add_files(files.iter().map(PathBuf::from).collect());
        library
    }

    fn tree_of(library: &Library) -> FolderTree {
        FolderTree::build(library.folders(), library.tracks())
    }

    /// Each listed folder as "name count", indented two spaces a level.
    fn outline(tree: &FolderTree, expanded: &HashSet<PathBuf>) -> Vec<String> {
        tree.visible_rows(expanded)
            .into_iter()
            .map(|index| {
                let node = tree.node(index).unwrap();
                format!(
                    "{}{} {}",
                    "  ".repeat(node.depth),
                    node.name,
                    node.track_count
                )
            })
            .collect()
    }

    fn everything_expanded(tree: &FolderTree) -> HashSet<PathBuf> {
        tree.nodes.iter().map(|node| node.path.clone()).collect()
    }

    fn dj_library() -> Library {
        library(
            &["/music/DJ tracks"],
            &[
                "/music/DJ tracks/afro_house/warmup/one.mp3",
                "/music/DJ tracks/afro_house/warmup/two.mp3",
                "/music/DJ tracks/afro_house/peak/three.flac",
                "/music/DJ tracks/Deep/four.wav",
                "/music/DJ tracks/loose.mp3",
                "/music/DJ tracks/ambient/drones/long/five.aiff",
            ],
        )
    }

    #[test]
    fn groups_tracks_by_folder_and_counts_everything_under_each() {
        let library = dj_library();
        let tree = tree_of(&library);
        assert_eq!(
            outline(&tree, &everything_expanded(&tree)),
            [
                "DJ tracks 6",
                "  afro_house 3",
                "    peak 1",
                "    warmup 2",
                "  ambient 1",
                "    drones 1",
                "      long 1",
                "  Deep 1",
            ]
        );
    }

    #[test]
    fn a_file_directly_in_an_added_folder_counts_there_and_adds_no_subfolder() {
        let library = library(&["/promos"], &["/promos/a.mp3", "/promos/b.mp3"]);
        let tree = tree_of(&library);
        assert_eq!(outline(&tree, &everything_expanded(&tree)), ["promos 2"]);
        assert!(tree.node(tree.roots[0]).unwrap().children.is_empty());
    }

    #[test]
    fn folded_folders_hide_what_is_inside_them() {
        let tree = tree_of(&dj_library());
        assert_eq!(outline(&tree, &HashSet::new()), ["DJ tracks 6"]);
        let expanded: HashSet<PathBuf> = [
            PathBuf::from("/music/DJ tracks"),
            PathBuf::from("/music/DJ tracks/ambient/drones"),
        ]
        .into();
        assert_eq!(
            outline(&tree, &expanded),
            ["DJ tracks 6", "  afro_house 3", "  ambient 1", "  Deep 1"]
        );
    }

    #[test]
    fn folders_are_ordered_by_name_whatever_the_letter_case_and_added_folders_as_added() {
        let library = library(
            &["/zeta", "/alpha"],
            &[
                "/zeta/b/1.mp3",
                "/zeta/C/2.mp3",
                "/zeta/a/3.mp3",
                "/alpha/x/4.mp3",
            ],
        );
        let tree = tree_of(&library);
        assert_eq!(
            outline(&tree, &everything_expanded(&tree)),
            ["zeta 3", "  a 1", "  b 1", "  C 1", "alpha 1", "  x 1"]
        );
    }

    #[test]
    fn an_added_folder_inside_another_shows_as_part_of_it_and_counts_once() {
        let library = library(
            &["/music/DJ tracks/afro_house", "/music/DJ tracks"],
            &[
                "/music/DJ tracks/afro_house/warmup/one.mp3",
                "/music/DJ tracks/Deep/two.mp3",
            ],
        );
        let tree = tree_of(&library);
        assert_eq!(
            outline(&tree, &everything_expanded(&tree)),
            ["DJ tracks 2", "  afro_house 1", "    warmup 1", "  Deep 1"]
        );
    }

    #[test]
    fn the_same_folder_added_twice_shows_once() {
        let folders = [
            PathBuf::from("/music/set"),
            PathBuf::from("/music/set/"),
            PathBuf::from("/music/set"),
        ];
        assert_eq!(outermost_folders(&folders), [Path::new("/music/set")]);
    }

    #[test]
    fn a_folder_added_through_a_link_keeps_the_name_it_was_added_by() {
        let library = library(
            &["/Volumes/USB/Crate link", "/Users/dj/Crate"],
            &[
                "/Volumes/USB/Crate link/Deep/a.mp3",
                "/Users/dj/Crate/Deep/a.mp3",
            ],
        );
        let tree = tree_of(&library);
        assert_eq!(
            outline(&tree, &everything_expanded(&tree)),
            ["Crate link 1", "  Deep 1", "Crate 1", "  Deep 1"]
        );
    }

    #[test]
    fn an_added_folder_without_tracks_still_shows_and_tracks_outside_are_left_out() {
        let library = library(&["/empty"], &["/elsewhere/a.mp3"]);
        let tree = tree_of(&library);
        assert_eq!(outline(&tree, &everything_expanded(&tree)), ["empty 0"]);
    }

    #[test]
    fn counts_the_tracks_that_failed_under_each_folder() {
        let mut library = dj_library();
        library.record_outcome(
            Path::new("/music/DJ tracks/afro_house/peak/three.flac"),
            Err("cannot decode".into()),
        );
        let tree = tree_of(&library);
        let failed = |path: &str| {
            let index = tree.index_of(Path::new(path)).unwrap();
            tree.node(index).unwrap().review_count
        };
        assert_eq!(failed("/music/DJ tracks"), 1);
        assert_eq!(failed("/music/DJ tracks/afro_house"), 1);
        assert_eq!(failed("/music/DJ tracks/afro_house/warmup"), 0);
    }

    #[test]
    fn builds_thousands_of_tracks_in_hundreds_of_folders_quickly() {
        let files: Vec<String> = (0..20_000)
            .map(|number| {
                format!(
                    "/music/DJ/genre {}/label {}/year {}/track {number}.mp3",
                    number % 12,
                    number % 37,
                    number % 5
                )
            })
            .collect();
        let file_refs: Vec<&str> = files.iter().map(String::as_str).collect();
        let library = library(&["/music/DJ"], &file_refs);
        let started = std::time::Instant::now();
        let tree = tree_of(&library);
        let rows = tree.visible_rows(&everything_expanded(&tree));
        // On an M5 this takes 0.4 to 0.9 seconds with the other tests
        // running, and up to 2.5 with every core busy. The bound is ten times
        // that; a quadratic build takes minutes on this library.
        assert!(started.elapsed() < std::time::Duration::from_secs(25));
        assert_eq!(tree.node(tree.roots[0]).unwrap().track_count, 20_000);
        assert!(rows.len() > 400, "{}", rows.len());
    }

    #[test]
    fn sorting_by_folder_follows_the_tree_with_added_folders_in_the_order_added() {
        let library = library(
            &["/zeta", "/alpha"],
            &[
                "/alpha/x/1.mp3",
                "/zeta/b/2.mp3",
                "/zeta/B/3.mp3",
                "/zeta/a/4.mp3",
                "/zeta/5.mp3",
                "/outside/6.mp3",
            ],
        );
        let tree = tree_of(&library);
        assert_eq!(
            outline(&tree, &everything_expanded(&tree)),
            ["zeta 4", "  a 1", "  B 1", "  b 1", "alpha 1", "  x 1"]
        );
        let column = FolderColumn::new(library.tracks(), library.folders(), None);
        let mut order: Vec<usize> = (0..library.tracks().len()).collect();
        order.sort_by(|first, second| column.compare(*first, *second));
        let names: Vec<&str> = order
            .iter()
            .map(|index| library.tracks()[*index].file_name.as_str())
            .collect();
        assert_eq!(
            names,
            ["5.mp3", "4.mp3", "3.mp3", "2.mp3", "1.mp3", "6.mp3"]
        );
    }

    /// A library of 20,000 tracks in a few hundred folders, every tenth
    /// track failed.
    fn large_library() -> Library {
        let files: Vec<String> = (0..20_000)
            .map(|number| {
                format!(
                    "/music/DJ/genre {}/label {}/year {}/track {number}.mp3",
                    number % 12,
                    number % 37,
                    number % 5
                )
            })
            .collect();
        let file_refs: Vec<&str> = files.iter().map(String::as_str).collect();
        let mut library = library(&["/music/DJ"], &file_refs);
        for file in files.iter().step_by(10) {
            library.record_outcome(Path::new(file), Err("cannot decode".into()));
        }
        library
    }

    #[test]
    fn results_coming_in_recount_reviews_as_a_fresh_build_would_and_faster() {
        let mut library = large_library();
        let started = std::time::Instant::now();
        let mut tree = tree_of(&library);
        let build_time = started.elapsed();

        for file in (0..20_000).step_by(7).map(|number| {
            format!(
                "/music/DJ/genre {}/label {}/year {}/track {number}.mp3",
                number % 12,
                number % 37,
                number % 5
            )
        }) {
            library.record_outcome(Path::new(&file), Err("cannot decode".into()));
        }
        // The shortest of five recounts, so that a moment the test machine
        // spends on other work does not count. A recount takes about 2 ms
        // against 0.4 to 2.5 s for a build on an M5, so the margin below is
        // wide; a pause in the middle of a single recount was its only risk.
        let recount_time = (0..5)
            .map(|_| {
                let started = std::time::Instant::now();
                tree.recount_reviews(library.tracks());
                started.elapsed()
            })
            .min()
            .unwrap_or_default();

        let fresh = tree_of(&library);
        assert_eq!(tree.nodes, fresh.nodes);
        assert!(
            recount_time * 4 < build_time,
            "recount {recount_time:?}, build {build_time:?}"
        );
    }

    fn index(tree: &FolderTree, path: &str) -> usize {
        tree.index_of(Path::new(path)).unwrap()
    }

    #[test]
    fn up_and_down_move_through_the_listed_folders() {
        let tree = tree_of(&dj_library());
        let expanded: HashSet<PathBuf> = [PathBuf::from("/music/DJ tracks")].into();
        let rows = tree.visible_rows(&expanded);
        let root = index(&tree, "/music/DJ tracks");
        let afro = index(&tree, "/music/DJ tracks/afro_house");
        let deep = index(&tree, "/music/DJ tracks/Deep");
        let step = |current, key| tree.navigate(&rows, &expanded, current, key);

        assert_eq!(step(None, TreeKey::Down), Some(TreeMove::Select(root)));
        assert_eq!(
            step(Some(root), TreeKey::Down),
            Some(TreeMove::Select(afro))
        );
        assert_eq!(step(Some(afro), TreeKey::Up), Some(TreeMove::Select(root)));
        assert_eq!(step(Some(root), TreeKey::Up), None);
        assert_eq!(step(Some(deep), TreeKey::Down), None);
        assert_eq!(
            step(Some(afro), TreeKey::Last),
            Some(TreeMove::Select(deep))
        );
        assert_eq!(
            step(Some(deep), TreeKey::First),
            Some(TreeMove::Select(root))
        );
    }

    #[test]
    fn right_unfolds_then_enters_and_left_folds_then_leaves() {
        let tree = tree_of(&dj_library());
        let root = index(&tree, "/music/DJ tracks");
        let afro = index(&tree, "/music/DJ tracks/afro_house");
        let peak = index(&tree, "/music/DJ tracks/afro_house/peak");

        let folded = HashSet::new();
        let rows = tree.visible_rows(&folded);
        assert_eq!(
            tree.navigate(&rows, &folded, Some(root), TreeKey::Right),
            Some(TreeMove::Expand(root))
        );
        assert_eq!(
            tree.navigate(&rows, &folded, Some(root), TreeKey::Left),
            None
        );

        let expanded: HashSet<PathBuf> = [
            PathBuf::from("/music/DJ tracks"),
            PathBuf::from("/music/DJ tracks/afro_house"),
        ]
        .into();
        let rows = tree.visible_rows(&expanded);
        assert_eq!(
            tree.navigate(&rows, &expanded, Some(root), TreeKey::Right),
            Some(TreeMove::Select(afro))
        );
        assert_eq!(
            tree.navigate(&rows, &expanded, Some(afro), TreeKey::Right),
            Some(TreeMove::Select(peak))
        );
        assert_eq!(
            tree.navigate(&rows, &expanded, Some(peak), TreeKey::Right),
            None
        );
        assert_eq!(
            tree.navigate(&rows, &expanded, Some(peak), TreeKey::Left),
            Some(TreeMove::Select(afro))
        );
        assert_eq!(
            tree.navigate(&rows, &expanded, Some(afro), TreeKey::Left),
            Some(TreeMove::Collapse(afro))
        );
    }

    #[test]
    fn a_shown_folder_inside_a_folded_one_moves_from_the_folded_one() {
        let tree = tree_of(&dj_library());
        let root = index(&tree, "/music/DJ tracks");
        let warmup = index(&tree, "/music/DJ tracks/afro_house/warmup");
        let folded = HashSet::new();
        let rows = tree.visible_rows(&folded);
        assert_eq!(
            tree.navigate(&rows, &folded, Some(warmup), TreeKey::Right),
            Some(TreeMove::Expand(root))
        );
    }

    #[test]
    fn an_empty_tree_ignores_the_keys() {
        let tree = FolderTree::default();
        let rows = tree.visible_rows(&HashSet::new());
        assert_eq!(
            tree.navigate(&rows, &HashSet::new(), None, TreeKey::Down),
            None
        );
    }

    fn label(track: &str, shown: Option<&str>, folders: &[&str]) -> String {
        let folders: Vec<PathBuf> = folders.iter().map(PathBuf::from).collect();
        folder_label(
            Path::new(track),
            shown.map(Path::new),
            &outermost_folders(&folders),
        )
        .text
    }

    fn joined(names: &[&str]) -> String {
        names.join(MAIN_SEPARATOR_STR)
    }

    const TRACK: &str = "/music/DJ tracks/afro_house/warmup/one.mp3";

    #[test]
    fn the_folder_shows_relative_to_the_folder_on_show() {
        let folders = ["/music/DJ tracks"];
        assert_eq!(
            label(TRACK, Some("/music/DJ tracks"), &folders),
            joined(&["afro_house", "warmup"])
        );
        assert_eq!(
            label(TRACK, Some("/music/DJ tracks/afro_house"), &folders),
            "warmup"
        );
    }

    #[test]
    fn a_track_directly_in_the_folder_on_show_has_no_folder_text() {
        let folders = ["/music/DJ tracks"];
        assert_eq!(
            label(TRACK, Some("/music/DJ tracks/afro_house/warmup"), &folders),
            ""
        );
        assert_eq!(label("/music/DJ tracks/loose.mp3", None, &folders), "");
    }

    #[test]
    fn all_tracks_shows_the_path_inside_the_added_folder() {
        assert_eq!(
            label(TRACK, None, &["/music/DJ tracks"]),
            joined(&["afro_house", "warmup"])
        );
    }

    #[test]
    fn with_several_added_folders_all_tracks_leads_with_the_added_folder() {
        let folders = ["/music/DJ tracks", "/promos"];
        assert_eq!(
            label(TRACK, None, &folders),
            joined(&["DJ tracks", "afro_house", "warmup"])
        );
        assert_eq!(label("/promos/new.mp3", None, &folders), "promos");
    }

    #[test]
    fn a_folder_added_inside_another_is_relative_to_the_outer_one() {
        let folders = ["/music/DJ tracks/afro_house", "/music/DJ tracks"];
        assert_eq!(
            label(TRACK, None, &folders),
            joined(&["afro_house", "warmup"])
        );
    }

    #[test]
    fn a_folder_added_through_a_link_is_relative_to_the_link() {
        let folders = ["/Volumes/USB/Crate link"];
        assert_eq!(
            label("/Volumes/USB/Crate link/Deep/2024/a.mp3", None, &folders),
            joined(&["Deep", "2024"])
        );
        assert_eq!(
            label(
                "/Volumes/USB/Crate link/Deep/2024/a.mp3",
                Some("/Volumes/USB/Crate link/Deep"),
                &folders
            ),
            "2024"
        );
    }

    #[test]
    fn a_track_outside_every_added_folder_shows_its_own_folder_name() {
        assert_eq!(label("/downloads/edits/a.mp3", None, &["/music"]), "edits");
    }

    #[test]
    fn the_full_folder_is_kept_for_the_tooltip() {
        let folders = [PathBuf::from("/music/DJ tracks")];
        let label = folder_label(
            Path::new(TRACK),
            Some(Path::new("/music/DJ tracks")),
            &outermost_folders(&folders),
        );
        assert_eq!(
            label.full_path,
            PathBuf::from("/music/DJ tracks/afro_house/warmup")
        );
    }

    #[test]
    fn sorting_by_folder_puts_direct_tracks_first_then_subfolders_in_tree_order() {
        let library = library(
            &["/music/DJ tracks"],
            &[
                "/music/DJ tracks/b/z.mp3",
                "/music/DJ tracks/a b/y.mp3",
                "/music/DJ tracks/a/x.mp3",
                "/music/DJ tracks/top.mp3",
                "/music/DJ tracks/a/deeper/w.mp3",
                "/music/DJ tracks/B2/v.mp3",
            ],
        );
        let column = FolderColumn::new(library.tracks(), library.folders(), None);
        let mut order: Vec<usize> = (0..library.tracks().len()).collect();
        order.sort_by(|first, second| column.compare(*first, *second));
        let names: Vec<&str> = order
            .iter()
            .map(|index| library.tracks()[*index].file_name.as_str())
            .collect();
        assert_eq!(
            names,
            ["top.mp3", "x.mp3", "w.mp3", "y.mp3", "z.mp3", "v.mp3"]
        );
    }
}
