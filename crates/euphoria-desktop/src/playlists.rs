// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The playlists the user builds in the window: their names, labels and
//! tracks in order, changed here and kept in the playlists database in the
//! background.
//!
//! A track is a path here, as in the library. The content hash that keeps
//! it across moves and renames is worked out when the playlists are saved,
//! and remembered so each file is read at most once.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use euphoria_core::{ContentHash, Key, PlaylistEntry, StoredPlaylist};

use crate::mixes::compatible_keys;

/// The emoji a playlist can be labelled with, offered in its menu.
pub(crate) const LABELS: [&str; 16] = [
    "\u{1f525}",
    "\u{1f319}",
    "\u{2600}\u{fe0f}",
    "\u{1f30a}",
    "\u{26a1}",
    "\u{1f48e}",
    "\u{1f3b9}",
    "\u{1f941}",
    "\u{2764}\u{fe0f}",
    "\u{1f31f}",
    "\u{1f680}",
    "\u{1f3c1}",
    "\u{1f9ca}",
    "\u{1f33f}",
    "\u{1f377}",
    "\u{1f47e}",
];

/// The name a new playlist starts with.
const NEW_PLAYLIST_NAME: &str = "New playlist";

/// Tells playlists apart while the app runs, whatever their names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PlaylistId(u64);

impl PlaylistId {
    /// A number for element ids, unique among the playlists of this run.
    pub(crate) fn number(self) -> usize {
        self.0 as usize
    }
}

/// One playlist.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Playlist {
    pub(crate) id: PlaylistId,
    pub(crate) name: String,
    /// One emoji the user chose, if any.
    pub(crate) label: Option<String>,
    /// The tracks in play order, each once.
    pub(crate) tracks: Vec<PathBuf>,
}

/// Every playlist, in the order the sidebar lists them.
#[derive(Debug, Clone, Default)]
pub(crate) struct Playlists {
    items: Vec<Playlist>,
    next_id: u64,
    /// The content of each file met so far, so saving does not read a file
    /// twice, and a file that has gone keeps the content it had.
    contents: HashMap<PathBuf, ContentHash>,
    revision: u64,
}

impl Playlists {
    /// The playlists as they were kept.
    pub(crate) fn from_stored(stored: Vec<StoredPlaylist>) -> Self {
        let mut playlists = Self::default();
        for playlist in stored {
            let id = playlists.new_id();
            let mut seen = HashSet::new();
            let mut tracks = Vec::new();
            for entry in playlist.entries {
                if seen.insert(entry.path.clone()) {
                    playlists.contents.insert(entry.path.clone(), entry.content);
                    tracks.push(entry.path);
                }
            }
            playlists.items.push(Playlist {
                id,
                name: playlist.name,
                label: playlist.label,
                tracks,
            });
        }
        playlists
    }

    /// The playlists to keep, with the content of every track that has one:
    /// those already known and those `content_of` works out. A track whose
    /// content cannot be worked out, such as a file that has gone before it
    /// was ever read, is left out of what is kept.
    pub(crate) fn stored_with_contents(
        &mut self,
        mut content_of: impl FnMut(&Path) -> Option<ContentHash>,
    ) -> Vec<StoredPlaylist> {
        for path in self.items.iter().flat_map(|playlist| &playlist.tracks) {
            if !self.contents.contains_key(path)
                && let Some(content) = content_of(path)
            {
                self.contents.insert(path.clone(), content);
            }
        }
        self.items
            .iter()
            .map(|playlist| StoredPlaylist {
                name: playlist.name.clone(),
                label: playlist.label.clone(),
                entries: playlist
                    .tracks
                    .iter()
                    .filter_map(|path| {
                        self.contents.get(path).map(|content| PlaylistEntry {
                            content: *content,
                            path: path.clone(),
                        })
                    })
                    .collect(),
            })
            .collect()
    }

    /// The contents already known, for saving off the main thread.
    pub(crate) fn known_contents(&self) -> HashMap<PathBuf, ContentHash> {
        self.contents.clone()
    }

    /// Remembers contents worked out off the main thread.
    pub(crate) fn learn_contents(&mut self, contents: HashMap<PathBuf, ContentHash>) {
        self.contents.extend(contents);
    }

    /// A number that changes with every change, to know when to save.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    /// Every playlist, in order.
    pub(crate) fn all(&self) -> &[Playlist] {
        &self.items
    }

    /// The playlist `id`, while it exists.
    pub(crate) fn get(&self, id: PlaylistId) -> Option<&Playlist> {
        self.items.iter().find(|playlist| playlist.id == id)
    }

    fn get_mut(&mut self, id: PlaylistId) -> Option<&mut Playlist> {
        self.revision += 1;
        self.items.iter_mut().find(|playlist| playlist.id == id)
    }

    fn new_id(&mut self) -> PlaylistId {
        self.next_id += 1;
        PlaylistId(self.next_id)
    }

    /// Adds an empty playlist at the end, named "New playlist", or "New
    /// playlist 2" and so on when that name is taken.
    pub(crate) fn create(&mut self) -> PlaylistId {
        let taken: HashSet<String> = self
            .items
            .iter()
            .map(|playlist| playlist.name.to_lowercase())
            .collect();
        let name = std::iter::once(NEW_PLAYLIST_NAME.to_string())
            .chain((2..).map(|number| format!("{NEW_PLAYLIST_NAME} {number}")))
            .find(|name| !taken.contains(&name.to_lowercase()))
            .unwrap_or_else(|| NEW_PLAYLIST_NAME.to_string());
        let id = self.new_id();
        self.items.push(Playlist {
            id,
            name,
            label: None,
            tracks: Vec::new(),
        });
        self.revision += 1;
        id
    }

    /// Renames the playlist. A name that is empty once trimmed is refused,
    /// returning false.
    pub(crate) fn rename(&mut self, id: PlaylistId, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        match self.get_mut(id) {
            Some(playlist) => {
                playlist.name = name.to_string();
                true
            }
            None => false,
        }
    }

    /// Gives the playlist an emoji, or takes it away.
    pub(crate) fn set_label(&mut self, id: PlaylistId, label: Option<&str>) {
        if let Some(playlist) = self.get_mut(id) {
            playlist.label = label.map(str::to_string);
        }
    }

    /// Deletes the playlist. The tracks stay in the library.
    pub(crate) fn delete(&mut self, id: PlaylistId) {
        self.items.retain(|playlist| playlist.id != id);
        self.revision += 1;
    }

    /// Moves the playlist to `position` in the list, counted before the
    /// move.
    pub(crate) fn move_playlist(&mut self, id: PlaylistId, position: usize) {
        let Some(from) = self.items.iter().position(|playlist| playlist.id == id) else {
            return;
        };
        let playlist = self.items.remove(from);
        let to = if position > from {
            position - 1
        } else {
            position
        };
        self.items.insert(to.min(self.items.len()), playlist);
        self.revision += 1;
    }

    /// Adds `paths` at the end of the playlist, leaving out the ones it
    /// already holds. Returns how many were added.
    pub(crate) fn add_tracks(&mut self, id: PlaylistId, paths: &[PathBuf]) -> usize {
        let Some(playlist) = self.get_mut(id) else {
            return 0;
        };
        let mut held: HashSet<PathBuf> = playlist.tracks.iter().cloned().collect();
        let before = playlist.tracks.len();
        for path in paths {
            if held.insert(path.clone()) {
                playlist.tracks.push(path.clone());
            }
        }
        playlist.tracks.len() - before
    }

    /// Takes `paths` out of the playlist.
    pub(crate) fn remove_tracks(&mut self, id: PlaylistId, paths: &[PathBuf]) {
        let removed: HashSet<&PathBuf> = paths.iter().collect();
        if let Some(playlist) = self.get_mut(id) {
            playlist.tracks.retain(|path| !removed.contains(path));
        }
    }

    /// Moves `paths`, keeping their order, so they come just before the
    /// track at `position`, counted before the move, or at the end when
    /// `position` is past the last track.
    pub(crate) fn move_tracks(&mut self, id: PlaylistId, paths: &[PathBuf], position: usize) {
        let moved: HashSet<&PathBuf> = paths.iter().collect();
        let Some(playlist) = self.get_mut(id) else {
            return;
        };
        let anchor = playlist
            .tracks
            .iter()
            .skip(position)
            .find(|path| !moved.contains(path))
            .cloned();
        let in_order: Vec<PathBuf> = playlist
            .tracks
            .iter()
            .filter(|path| moved.contains(path))
            .cloned()
            .collect();
        playlist.tracks.retain(|path| !moved.contains(path));
        let at = anchor
            .and_then(|anchor| playlist.tracks.iter().position(|path| *path == anchor))
            .unwrap_or(playlist.tracks.len());
        playlist.tracks.splice(at..at, in_order);
    }
}

/// A rough picture of a playlist as a set, for its heading: the keys in
/// order with the steps a DJ cannot mix in key, the tempo range, and the
/// energy of each track.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SetShape {
    /// The key of each track that has one, in order, and whether the step
    /// into it from the key before is a compatible move.
    pub(crate) keys: Vec<(Key, bool)>,
    /// The slowest and fastest tempo.
    pub(crate) tempo_range: Option<(f32, f32)>,
    /// The energy of each track that has one, in order.
    pub(crate) energies: Vec<u8>,
}

impl SetShape {
    /// The shape of tracks with these keys, tempos and energies, in order.
    pub(crate) fn of(
        tracks: impl IntoIterator<Item = (Option<Key>, Option<f32>, Option<u8>)>,
    ) -> Self {
        let mut keys: Vec<(Key, bool)> = Vec::new();
        let mut tempo_range: Option<(f32, f32)> = None;
        let mut energies = Vec::new();
        for (key, bpm, energy) in tracks {
            if let Some(key) = key {
                let is_smooth = keys.last().is_none_or(|(previous, _)| {
                    *previous == key || compatible_keys(previous).contains(&key)
                });
                keys.push((key, is_smooth));
            }
            if let Some(bpm) = bpm {
                tempo_range = Some(match tempo_range {
                    Some((slowest, fastest)) => (slowest.min(bpm), fastest.max(bpm)),
                    None => (bpm, bpm),
                });
            }
            energies.extend(energy);
        }
        Self {
            keys,
            tempo_range,
            energies,
        }
    }

    /// How many steps between keys are not compatible moves.
    pub(crate) fn clashes(&self) -> usize {
        self.keys.iter().filter(|(_, is_smooth)| !is_smooth).count()
    }
}

#[cfg(test)]
mod tests {
    use euphoria_core::Mode;

    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names
            .iter()
            .map(|name| PathBuf::from(format!("/music/{name}.mp3")))
            .collect()
    }

    fn names(playlists: &Playlists, id: PlaylistId) -> Vec<String> {
        playlists
            .get(id)
            .unwrap()
            .tracks
            .iter()
            .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn new_playlists_get_names_that_are_free() {
        let mut playlists = Playlists::default();
        let first = playlists.create();
        let second = playlists.create();
        assert_eq!(playlists.get(first).unwrap().name, "New playlist");
        assert_eq!(playlists.get(second).unwrap().name, "New playlist 2");
        assert!(playlists.rename(first, "  Warmup "));
        assert!(!playlists.rename(first, "   "));
        assert_eq!(playlists.get(first).unwrap().name, "Warmup");
        let third = playlists.create();
        assert_eq!(playlists.get(third).unwrap().name, "New playlist");
    }

    #[test]
    fn tracks_are_added_once_and_moved_where_they_are_dropped() {
        let mut playlists = Playlists::default();
        let id = playlists.create();
        assert_eq!(playlists.add_tracks(id, &paths(&["a", "b", "c", "d"])), 4);
        assert_eq!(playlists.add_tracks(id, &paths(&["b", "e"])), 1);
        assert_eq!(names(&playlists, id), ["a", "b", "c", "d", "e"]);

        playlists.move_tracks(id, &paths(&["d", "b"]), 0);
        assert_eq!(names(&playlists, id), ["b", "d", "a", "c", "e"]);
        playlists.move_tracks(id, &paths(&["b"]), 4);
        assert_eq!(names(&playlists, id), ["d", "a", "c", "b", "e"]);
        playlists.move_tracks(id, &paths(&["d"]), 99);
        assert_eq!(names(&playlists, id), ["a", "c", "b", "e", "d"]);
        playlists.move_tracks(id, &paths(&["c"]), 2);
        assert_eq!(names(&playlists, id), ["a", "c", "b", "e", "d"]);

        playlists.remove_tracks(id, &paths(&["a", "e"]));
        assert_eq!(names(&playlists, id), ["c", "b", "d"]);
    }

    #[test]
    fn playlists_move_delete_and_carry_a_label() {
        let mut playlists = Playlists::default();
        let ids: Vec<PlaylistId> = (0..3).map(|_| playlists.create()).collect();
        let order = |playlists: &Playlists| -> Vec<PlaylistId> {
            playlists.all().iter().map(|playlist| playlist.id).collect()
        };
        playlists.move_playlist(ids[2], 0);
        assert_eq!(order(&playlists), [ids[2], ids[0], ids[1]]);
        playlists.move_playlist(ids[2], 3);
        assert_eq!(order(&playlists), ids);
        playlists.set_label(ids[1], Some(LABELS[0]));
        assert_eq!(
            playlists.get(ids[1]).unwrap().label.as_deref(),
            Some(LABELS[0])
        );
        playlists.set_label(ids[1], None);
        assert_eq!(playlists.get(ids[1]).unwrap().label, None);
        let revision = playlists.revision();
        playlists.delete(ids[0]);
        assert!(playlists.revision() > revision);
        assert_eq!(order(&playlists), [ids[1], ids[2]]);
    }

    #[test]
    fn what_is_kept_comes_back_and_a_file_is_read_once() {
        let mut playlists = Playlists::default();
        let id = playlists.create();
        playlists.rename(id, "Peak time");
        playlists.set_label(id, Some(LABELS[0]));
        playlists.add_tracks(id, &paths(&["a", "gone", "b"]));
        let mut reads = Vec::new();
        let stored = playlists.stored_with_contents(|path| {
            reads.push(path.to_path_buf());
            (!path.ends_with("gone.mp3")).then(|| fake_content(path))
        });
        assert_eq!(reads.len(), 3);
        assert_eq!(
            stored[0].entries.len(),
            2,
            "the file that has gone is left out"
        );

        let mut asked_again = Vec::new();
        let again = playlists.stored_with_contents(|path| {
            asked_again.push(path.to_path_buf());
            None
        });
        assert_eq!(
            asked_again,
            paths(&["gone"]),
            "only the file never read is asked for again"
        );
        assert_eq!(again[0].entries.len(), 2);

        let reloaded = Playlists::from_stored(stored);
        let playlist = &reloaded.all()[0];
        assert_eq!(playlist.name, "Peak time");
        assert_eq!(playlist.label.as_deref(), Some(LABELS[0]));
        assert_eq!(playlist.tracks, paths(&["a", "b"]));
    }

    fn fake_content(path: &Path) -> ContentHash {
        let folder = std::env::temp_dir().join(format!("euphoria-playlist-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join(path.file_name().unwrap());
        std::fs::write(&file, path.to_string_lossy().as_bytes()).unwrap();
        let content = ContentHash::of_file(&file).unwrap();
        let _ = std::fs::remove_file(&file);
        content
    }

    #[test]
    fn the_shape_marks_steps_out_of_key_and_spans_the_tempos() {
        let a_minor = Key::new(9, Mode::Minor);
        let e_minor = Key::new(4, Mode::Minor);
        let c_major = Key::new(0, Mode::Major);
        let f_sharp_major = Key::new(6, Mode::Major);
        let shape = SetShape::of([
            (Some(a_minor), Some(122.0), Some(4)),
            (Some(a_minor), None, None),
            (Some(e_minor), Some(124.0), None),
            (None, None, Some(6)),
            (Some(c_major), Some(126.5), Some(7)),
            (Some(f_sharp_major), Some(121.0), Some(8)),
        ]);
        let steps: Vec<bool> = shape.keys.iter().map(|(_, is_smooth)| *is_smooth).collect();
        assert_eq!(steps, [true, true, true, false, false]);
        assert_eq!(shape.clashes(), 2);
        assert_eq!(shape.tempo_range, Some((121.0, 126.5)));
        assert_eq!(shape.energies, [4, 6, 7, 8]);
        assert_eq!(SetShape::of([]).tempo_range, None);
    }
}
