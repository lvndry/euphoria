// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tracks the user starred and the transitions they marked as mixing
//! well, changed here and kept in the playlists database in the background.
//!
//! As in the playlists, a track is a path here. The content hash that keeps
//! a favorite across moves, renames and tag writes is worked out when the
//! favorites are saved, and remembered so each file is read at most once.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use euphoria_core::{ContentHash, FavoriteTransition, PlaylistEntry, StoredFavorites};

use crate::playlists::PlaylistRewrite;

/// The name favorites are exported under.
pub(crate) const FAVORITES_NAME: &str = "Favorites";

/// A transition the user marked: `to` mixes well after `from`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Transition {
    pub(crate) from: PathBuf,
    pub(crate) to: PathBuf,
    /// What the user wrote about the mix, never empty.
    pub(crate) note: Option<String>,
}

/// Every favorite, in the order the user marked them.
#[derive(Debug, Clone, Default)]
pub(crate) struct Favorites {
    tracks: Vec<PathBuf>,
    starred: HashSet<PathBuf>,
    transitions: Vec<Transition>,
    /// The content of each file met so far, so saving does not read a file
    /// twice, and a file that has gone keeps the content it had.
    contents: HashMap<PathBuf, ContentHash>,
    revision: u64,
    applied_rewrites: usize,
}

impl Favorites {
    /// The favorites as they were kept.
    pub(crate) fn from_stored(stored: StoredFavorites) -> Self {
        let mut favorites = Self::default();
        for entry in stored.tracks {
            favorites.contents.insert(entry.path.clone(), entry.content);
            if favorites.starred.insert(entry.path.clone()) {
                favorites.tracks.push(entry.path);
            }
        }
        for transition in stored.transitions {
            favorites
                .contents
                .insert(transition.from.path.clone(), transition.from.content);
            favorites
                .contents
                .insert(transition.to.path.clone(), transition.to.content);
            let from = transition.from.path;
            let to = transition.to.path;
            if favorites.transition(&from, &to).is_none() {
                favorites.transitions.push(Transition {
                    from,
                    to,
                    note: transition.note,
                });
            }
        }
        favorites
    }

    /// The favorites to keep, with the content of every track that has one:
    /// those already known and those `content_of` works out. A favorite
    /// whose content cannot be worked out, such as a file that has gone
    /// before it was ever read, is left out of what is kept.
    pub(crate) fn stored_with_contents(
        &mut self,
        mut content_of: impl FnMut(&Path) -> Option<ContentHash>,
    ) -> StoredFavorites {
        let unknown: HashSet<PathBuf> = self
            .paths()
            .filter(|path| !self.contents.contains_key(*path))
            .cloned()
            .collect();
        let mut unknown: Vec<PathBuf> = unknown.into_iter().collect();
        unknown.sort();
        for path in unknown {
            if let Some(content) = content_of(&path) {
                self.contents.insert(path, content);
            }
        }
        let entry = |path: &PathBuf| {
            self.contents.get(path).map(|content| PlaylistEntry {
                content: *content,
                path: path.clone(),
            })
        };
        StoredFavorites {
            tracks: self.tracks.iter().filter_map(entry).collect(),
            transitions: self
                .transitions
                .iter()
                .filter_map(|transition| {
                    Some(FavoriteTransition {
                        from: entry(&transition.from)?,
                        to: entry(&transition.to)?,
                        note: transition.note.clone(),
                    })
                })
                .collect(),
        }
    }

    /// Every path a favorite holds, a path once for each place it is in.
    pub(crate) fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.tracks.iter().chain(
            self.transitions
                .iter()
                .flat_map(|transition| [&transition.from, &transition.to]),
        )
    }

    /// The content of the file at `path` when it was marked, when known.
    pub(crate) fn content_of(&self, path: &Path) -> Option<ContentHash> {
        self.contents.get(path).copied()
    }

    /// The contents already known, for saving off the main thread.
    pub(crate) fn known_contents(&self) -> HashMap<PathBuf, ContentHash> {
        self.contents.clone()
    }

    /// Remembers contents worked out off the main thread.
    pub(crate) fn learn_contents(&mut self, contents: HashMap<PathBuf, ContentHash>) {
        let used: HashSet<PathBuf> = self.paths().cloned().collect();
        for (path, content) in contents {
            if used.contains(&path) {
                self.contents.entry(path).or_insert(content);
            }
        }
    }

    /// Follows the rewrites not followed yet, in order.
    pub(crate) fn follow_rewrite_history(&mut self, rewrites: &[PlaylistRewrite]) {
        let from = self.applied_rewrites.min(rewrites.len());
        for rewrite in &rewrites[from..] {
            self.follow_rewrite(rewrite);
        }
        self.applied_rewrites = rewrites.len();
    }

    fn follow_rewrite(&mut self, rewrite: &PlaylistRewrite) {
        for (old, new) in &rewrite.paths {
            let is_other_content = self
                .contents
                .get(old)
                .is_some_and(|content| *content != rewrite.before);
            if is_other_content || !self.paths().any(|path| path == old) {
                continue;
            }
            self.contents.remove(old);
            self.contents.insert(new.clone(), rewrite.after);
            let renamed = |path: &mut PathBuf| {
                if path == old {
                    *path = new.clone();
                }
            };
            self.tracks.iter_mut().for_each(renamed);
            for transition in &mut self.transitions {
                renamed(&mut transition.from);
                renamed(&mut transition.to);
            }
            let mut seen = HashSet::new();
            self.tracks.retain(|path| seen.insert(path.clone()));
            self.starred = seen;
            let mut seen_pairs = HashSet::new();
            self.transitions.retain(|transition| {
                seen_pairs.insert((transition.from.clone(), transition.to.clone()))
            });
            self.revision += 1;
        }
    }

    fn forget_unreferenced_contents(&mut self) {
        let used: HashSet<PathBuf> = self.paths().cloned().collect();
        self.contents.retain(|path, _| used.contains(path));
    }

    /// A number that changes with every change, to know when to save.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// The starred tracks, in the order they were starred.
    pub(crate) fn tracks(&self) -> &[PathBuf] {
        &self.tracks
    }

    /// True when the track at `path` is starred.
    pub(crate) fn is_starred(&self, path: &Path) -> bool {
        self.starred.contains(path)
    }

    /// Stars every track of `paths`, or, when all of them are starred
    /// already, takes the star off each. Returns whether they are starred
    /// now.
    pub(crate) fn toggle_stars(&mut self, paths: &[PathBuf]) -> bool {
        let star = !paths.iter().all(|path| self.is_starred(path));
        for path in paths {
            self.set_starred(path, star);
        }
        star
    }

    /// Stars the track at `path`, or takes its star off.
    pub(crate) fn set_starred(&mut self, path: &Path, starred: bool) {
        if starred == self.is_starred(path) {
            return;
        }
        if starred {
            self.starred.insert(path.to_path_buf());
            self.tracks.push(path.to_path_buf());
        } else {
            self.starred.remove(path);
            self.tracks.retain(|track| track != path);
            self.forget_unreferenced_contents();
        }
        self.revision += 1;
    }

    /// Every marked transition, in the order they were marked.
    pub(crate) fn transitions(&self) -> &[Transition] {
        &self.transitions
    }

    /// The transition from `from` into `to`, when it is marked.
    pub(crate) fn transition(&self, from: &Path, to: &Path) -> Option<&Transition> {
        self.transitions
            .iter()
            .find(|transition| transition.from == from && transition.to == to)
    }

    /// The marked transitions out of the track at `from`.
    pub(crate) fn transitions_from<'favorites>(
        &'favorites self,
        from: &'favorites Path,
    ) -> impl Iterator<Item = &'favorites Transition> {
        self.transitions
            .iter()
            .filter(move |transition| transition.from == from)
    }

    /// Marks the transition from `from` into `to`, or takes the mark off.
    /// A track does not transition into itself. Returns whether it is
    /// marked now.
    pub(crate) fn toggle_transition(&mut self, from: &Path, to: &Path) -> bool {
        if from == to {
            return false;
        }
        if self.transition(from, to).is_some() {
            self.transitions
                .retain(|transition| !(transition.from == from && transition.to == to));
            self.forget_unreferenced_contents();
            self.revision += 1;
            return false;
        }
        self.transitions.push(Transition {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
            note: None,
        });
        self.revision += 1;
        true
    }

    /// Sets the note of the marked transition from `from` into `to`. A note
    /// that is empty once trimmed takes the note away.
    pub(crate) fn set_note(&mut self, from: &Path, to: &Path, note: &str) {
        let note = Some(note.trim()).filter(|note| !note.is_empty());
        let Some(transition) = self
            .transitions
            .iter_mut()
            .find(|transition| transition.from == from && transition.to == to)
        else {
            return;
        };
        if transition.note.as_deref() != note {
            transition.note = note.map(str::to_string);
            self.revision += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(name: &str) -> PathBuf {
        PathBuf::from(format!("/music/{name}.mp3"))
    }

    fn fake_content(byte: u8) -> ContentHash {
        let folder =
            std::env::temp_dir().join(format!("euphoria-favorites-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join(format!("{byte}.bin"));
        std::fs::write(&file, [byte]).unwrap();
        ContentHash::of_file(&file).unwrap()
    }

    #[test]
    fn stars_toggle_together_and_keep_their_order() {
        let mut favorites = Favorites::default();
        assert!(favorites.toggle_stars(&[path("a"), path("b")]));
        assert!(favorites.is_starred(&path("a")));
        assert!(
            favorites.toggle_stars(&[path("b"), path("c")]),
            "a selection that is not all starred is starred"
        );
        assert_eq!(favorites.tracks(), [path("a"), path("b"), path("c")]);
        assert!(!favorites.toggle_stars(&[path("a"), path("c")]));
        assert_eq!(favorites.tracks(), [path("b")]);
        let revision = favorites.revision();
        favorites.set_starred(&path("b"), true);
        assert_eq!(favorites.revision(), revision, "nothing changed");
    }

    #[test]
    fn a_transition_is_one_way_and_carries_a_trimmed_note() {
        let mut favorites = Favorites::default();
        assert!(!favorites.toggle_transition(&path("a"), &path("a")));
        assert!(favorites.toggle_transition(&path("a"), &path("b")));
        assert!(favorites.transition(&path("b"), &path("a")).is_none());
        favorites.set_note(&path("a"), &path("b"), "  loop the outro  ");
        assert_eq!(
            favorites
                .transition(&path("a"), &path("b"))
                .unwrap()
                .note
                .as_deref(),
            Some("loop the outro")
        );
        favorites.set_note(&path("a"), &path("b"), "   ");
        assert_eq!(
            favorites.transition(&path("a"), &path("b")).unwrap().note,
            None
        );
        assert_eq!(favorites.transitions_from(&path("a")).count(), 1);
        assert_eq!(favorites.transitions_from(&path("b")).count(), 0);
        assert!(!favorites.toggle_transition(&path("a"), &path("b")));
        assert!(favorites.transitions().is_empty());
    }

    #[test]
    fn what_is_kept_comes_back_and_a_file_is_read_once() {
        let mut favorites = Favorites::default();
        favorites.toggle_stars(&[path("a"), path("gone")]);
        favorites.toggle_transition(&path("a"), &path("b"));
        favorites.set_note(&path("a"), &path("b"), "on the break");
        favorites.toggle_transition(&path("gone"), &path("a"));
        let mut reads = Vec::new();
        let stored = favorites.stored_with_contents(|asked| {
            reads.push(asked.to_path_buf());
            (!asked.ends_with("gone.mp3")).then(|| fake_content(reads.len() as u8))
        });
        reads.sort();
        assert_eq!(reads, [path("a"), path("b"), path("gone")]);
        assert_eq!(stored.tracks.len(), 1, "the file that has gone is left out");
        assert_eq!(stored.transitions.len(), 1);

        let mut asked_again = Vec::new();
        favorites.stored_with_contents(|asked| {
            asked_again.push(asked.to_path_buf());
            None
        });
        assert_eq!(
            asked_again,
            [path("gone")],
            "only the file never read is asked again"
        );

        let reloaded = Favorites::from_stored(stored);
        assert_eq!(reloaded.tracks(), [path("a")]);
        let transition = reloaded.transition(&path("a"), &path("b")).unwrap();
        assert_eq!(transition.note.as_deref(), Some("on the break"));
    }

    #[test]
    fn a_rewrite_moves_the_star_and_the_transitions_to_the_new_path() {
        let mut favorites = Favorites::default();
        favorites.toggle_stars(&[path("old")]);
        favorites.toggle_transition(&path("old"), &path("b"));
        favorites.learn_contents(HashMap::from([(path("old"), fake_content(1))]));
        let rewrite = PlaylistRewrite {
            path: path("new"),
            before: fake_content(1),
            after: fake_content(2),
            paths: HashMap::from([(path("old"), path("new"))]),
        };
        favorites.follow_rewrite_history(std::slice::from_ref(&rewrite));
        assert!(favorites.is_starred(&path("new")));
        assert!(!favorites.is_starred(&path("old")));
        assert!(favorites.transition(&path("new"), &path("b")).is_some());
        assert_eq!(favorites.content_of(&path("new")), Some(fake_content(2)));
        let revision = favorites.revision();
        favorites.follow_rewrite_history(std::slice::from_ref(&rewrite));
        assert_eq!(favorites.revision(), revision, "a rewrite is followed once");
    }
}
