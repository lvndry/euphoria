// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Turns the folders a library is sorted into into a playlist tree.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::rekordbox::{Folder, Playlist, PlaylistNode};

/// The tracks of one folder on disk and of the folders inside it.
#[derive(Default)]
struct FolderTree {
    tracks: Vec<PathBuf>,
    subfolders: BTreeMap<OsString, FolderTree>,
}

/// Builds a playlist tree that mirrors where `tracks` sit inside `folders`.
///
/// Each of `folders` gives one entry at the top level, in the order given and
/// named after the folder. Inside it, and all the way down:
///
/// - a folder holding tracks and no subfolders with tracks becomes a playlist,
/// - a folder holding subfolders with tracks becomes a playlist folder with
///   one entry per such subfolder, sorted by name. Tracks sitting directly in
///   it go into a playlist of the same name, placed first inside it,
/// - a folder with no tracks anywhere beneath it is left out.
///
/// Tracks inside none of `folders` go into one last playlist called
/// `loose_playlist_name`, which is left out when there are none.
///
/// Playlists keep the order of `tracks`. A folder listed twice is used once.
/// When one of `folders` is inside another, each gets its own entry, so its
/// tracks appear under both. Entries that would share a name with an earlier
/// entry beside them get " (2)", " (3)" and so on after their name.
///
/// Only the paths are compared: nothing is read from disk.
pub fn playlists_from_folders(
    folders: &[PathBuf],
    tracks: &[PathBuf],
    loose_playlist_name: &str,
) -> Vec<PlaylistNode> {
    let mut used_folders: Vec<&Path> = Vec::new();
    let mut tree_of_folder: HashMap<&Path, usize> = HashMap::new();
    for folder in folders {
        if !tree_of_folder.contains_key(folder.as_path()) {
            tree_of_folder.insert(folder, used_folders.len());
            used_folders.push(folder);
        }
    }

    // Each track is looked up through the folders above it, so the work
    // grows with the number of tracks and not with tracks times folders.
    let mut trees: Vec<FolderTree> = used_folders.iter().map(|_| FolderTree::default()).collect();
    let mut loose_tracks: Vec<PathBuf> = Vec::new();
    for track in tracks {
        let mut is_inside_a_folder = false;
        for folder_above in track.ancestors().skip(1) {
            if let Some(&tree_index) = tree_of_folder.get(folder_above)
                && let Ok(inside_folder) = track.strip_prefix(folder_above)
            {
                trees[tree_index].insert(inside_folder, track);
                is_inside_a_folder = true;
            }
        }
        if !is_inside_a_folder {
            loose_tracks.push(track.clone());
        }
    }

    let mut nodes: Vec<PlaylistNode> = used_folders
        .iter()
        .zip(trees)
        .filter_map(|(folder, tree)| tree.into_node(folder_name(folder)))
        .collect();
    if !loose_tracks.is_empty() {
        nodes.push(PlaylistNode::Playlist(Playlist {
            name: loose_playlist_name.to_string(),
            tracks: loose_tracks,
        }));
    }

    make_names_unique(&mut nodes);
    nodes
}

/// The number of playlists in a tree, at any depth. Folders are not counted.
pub fn playlist_count(nodes: &[PlaylistNode]) -> usize {
    nodes
        .iter()
        .map(|node| match node {
            PlaylistNode::Playlist(_) => 1,
            PlaylistNode::Folder(folder) => playlist_count(&folder.children),
        })
        .sum()
}

impl FolderTree {
    fn insert(&mut self, inside_folder: &Path, track: &Path) {
        let mut tree = self;
        if let Some(parent) = inside_folder.parent() {
            for component in parent.iter() {
                tree = tree.subfolders.entry(component.to_os_string()).or_default();
            }
        }
        tree.tracks.push(track.to_path_buf());
    }

    fn into_node(self, name: String) -> Option<PlaylistNode> {
        let mut children: Vec<PlaylistNode> = self
            .subfolders
            .into_iter()
            .filter_map(|(subfolder_name, subfolder)| {
                subfolder.into_node(subfolder_name.to_string_lossy().into_owned())
            })
            .collect();
        if children.is_empty() {
            if self.tracks.is_empty() {
                return None;
            }
            return Some(PlaylistNode::Playlist(Playlist {
                name,
                tracks: self.tracks,
            }));
        }
        if !self.tracks.is_empty() {
            children.insert(
                0,
                PlaylistNode::Playlist(Playlist {
                    name: name.clone(),
                    tracks: self.tracks,
                }),
            );
        }
        make_names_unique(&mut children);
        Some(PlaylistNode::Folder(Folder { name, children }))
    }
}

/// The last part of the folder's path, or the whole path for a drive or the
/// root of the file system, which have no last part.
fn folder_name(folder: &Path) -> String {
    match folder.file_name() {
        Some(name) => name.to_string_lossy().into_owned(),
        None => folder.display().to_string(),
    }
}

fn make_names_unique(nodes: &mut [PlaylistNode]) {
    let mut taken: HashSet<String> = HashSet::new();
    for node in nodes {
        let name = match node {
            PlaylistNode::Folder(folder) => &mut folder.name,
            PlaylistNode::Playlist(playlist) => &mut playlist.name,
        };
        let mut unique_name = name.clone();
        let mut copy_number = 2;
        while taken.contains(&unique_name) {
            unique_name = format!("{name} ({copy_number})");
            copy_number += 1;
        }
        taken.insert(unique_name.clone());
        *name = unique_name;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(texts: &[&str]) -> Vec<PathBuf> {
        texts.iter().map(PathBuf::from).collect()
    }

    fn playlist(name: &str, tracks: &[&str]) -> PlaylistNode {
        PlaylistNode::Playlist(Playlist {
            name: name.to_string(),
            tracks: paths(tracks),
        })
    }

    fn folder(name: &str, children: Vec<PlaylistNode>) -> PlaylistNode {
        PlaylistNode::Folder(Folder {
            name: name.to_string(),
            children,
        })
    }

    #[test]
    fn genre_folders_become_playlists_inside_a_folder() {
        let tracks = [
            "/music/dj/house/a.mp3",
            "/music/dj/house/b.flac",
            "/music/dj/techno/c.wav",
        ];
        assert_eq!(
            playlists_from_folders(&paths(&["/music/dj"]), &paths(&tracks), "export"),
            vec![folder(
                "dj",
                vec![
                    playlist("house", &tracks[..2]),
                    playlist("techno", &tracks[2..]),
                ]
            )]
        );
    }

    #[test]
    fn nested_folders_become_nested_folders() {
        let tracks = [
            "/music/house/deep/a.mp3",
            "/music/house/deep/late/b.mp3",
            "/music/house/tech/c.mp3",
            "/music/intro.mp3",
        ];
        assert_eq!(
            playlists_from_folders(&paths(&["/music"]), &paths(&tracks), "export"),
            vec![folder(
                "music",
                vec![
                    playlist("music", &tracks[3..]),
                    folder(
                        "house",
                        vec![
                            folder(
                                "deep",
                                vec![
                                    playlist("deep", &tracks[..1]),
                                    playlist("late", &tracks[1..2]),
                                ]
                            ),
                            playlist("tech", &tracks[2..3]),
                        ]
                    ),
                ]
            )]
        );
    }

    #[test]
    fn a_folder_holding_only_tracks_becomes_one_playlist() {
        let tracks = ["/music/house/a.mp3", "/music/house/b.mp3"];
        assert_eq!(
            playlists_from_folders(&paths(&["/music/house"]), &paths(&tracks), "export"),
            vec![playlist("house", &tracks)]
        );
    }

    #[test]
    fn a_folder_with_no_tracks_is_left_out() {
        let tracks = ["/music/house/a.mp3"];
        assert_eq!(
            playlists_from_folders(
                &paths(&["/music/empty", "/music/house"]),
                &paths(&tracks),
                "export"
            ),
            vec![playlist("house", &tracks)]
        );
        assert_eq!(
            playlists_from_folders(&paths(&["/music/empty"]), &[], "export"),
            vec![]
        );
    }

    #[test]
    fn files_named_directly_go_into_one_last_playlist() {
        let tracks = ["/downloads/new.mp3", "/music/house/a.mp3", "/other/b.mp3"];
        assert_eq!(
            playlists_from_folders(&paths(&["/music/house"]), &paths(&tracks), "friday set"),
            vec![
                playlist("house", &tracks[1..2]),
                playlist("friday set", &["/downloads/new.mp3", "/other/b.mp3"]),
            ]
        );
        assert_eq!(
            playlists_from_folders(&[], &paths(&tracks), "friday set"),
            vec![playlist("friday set", &tracks)]
        );
    }

    #[test]
    fn two_folders_keep_the_order_they_were_given_in() {
        let tracks = ["/a/house/x.mp3", "/b/techno/y.mp3", "/b/techno/z.mp3"];
        assert_eq!(
            playlists_from_folders(&paths(&["/b", "/a", "/b"]), &paths(&tracks), "export"),
            vec![
                folder("b", vec![playlist("techno", &tracks[1..])]),
                folder("a", vec![playlist("house", &tracks[..1])]),
            ]
        );
    }

    #[test]
    fn a_folder_inside_another_gets_its_own_entry() {
        let tracks = ["/music/house/a.mp3", "/music/techno/b.mp3"];
        assert_eq!(
            playlists_from_folders(
                &paths(&["/music", "/music/house"]),
                &paths(&tracks),
                "export"
            ),
            vec![
                folder(
                    "music",
                    vec![
                        playlist("house", &tracks[..1]),
                        playlist("techno", &tracks[1..]),
                    ]
                ),
                playlist("house", &tracks[..1]),
            ]
        );
    }

    #[test]
    fn entries_that_share_a_name_are_numbered() {
        let tracks = [
            "/a/house/x.mp3",
            "/b/house/y.mp3",
            "/c/house/house/z.mp3",
            "/c/house/w.mp3",
            "/loose.mp3",
        ];
        assert_eq!(
            playlists_from_folders(
                &paths(&["/a/house", "/b/house", "/c/house"]),
                &paths(&tracks),
                "house"
            ),
            vec![
                playlist("house", &tracks[..1]),
                playlist("house (2)", &tracks[1..2]),
                folder(
                    "house (3)",
                    vec![
                        playlist("house", &tracks[3..4]),
                        playlist("house (2)", &tracks[2..3]),
                    ]
                ),
                playlist("house (4)", &tracks[4..]),
            ]
        );
    }

    #[test]
    fn the_root_of_the_file_system_is_named_by_its_path() {
        let tracks = ["/a.mp3"];
        assert_eq!(
            playlists_from_folders(&paths(&["/"]), &paths(&tracks), "export"),
            vec![playlist("/", &tracks)]
        );
    }

    #[test]
    fn a_large_library_with_many_folders_is_mapped_in_one_pass() {
        const GENRES: usize = 3000;
        const TRACKS_PER_GENRE: usize = 8;
        let genre_folder = |genre: usize| format!("/music/genre {genre:04}");
        let mut folders = vec![PathBuf::from("/music")];
        folders.extend((0..GENRES).map(|genre| PathBuf::from(genre_folder(genre))));
        let mut tracks = Vec::new();
        for genre in 0..GENRES {
            for track in 0..TRACKS_PER_GENRE {
                tracks.push(PathBuf::from(format!(
                    "{}/track {track}.mp3",
                    genre_folder(genre)
                )));
            }
        }
        tracks.push(PathBuf::from("/downloads/new.mp3"));

        let nodes = playlists_from_folders(&folders, &tracks, "export");

        assert_eq!(nodes.len(), GENRES + 2);
        let PlaylistNode::Folder(music) = &nodes[0] else {
            panic!("the first entry is the music folder");
        };
        assert_eq!(music.children.len(), GENRES);
        for (genre, (inside_music, on_its_own)) in
            music.children.iter().zip(&nodes[1..]).enumerate()
        {
            let expected = PlaylistNode::Playlist(Playlist {
                name: format!("genre {genre:04}"),
                tracks: tracks[genre * TRACKS_PER_GENRE..(genre + 1) * TRACKS_PER_GENRE].to_vec(),
            });
            assert_eq!(inside_music, &expected);
            assert_eq!(on_its_own, &expected);
        }
        assert_eq!(
            nodes[GENRES + 1],
            playlist("export", &["/downloads/new.mp3"])
        );
        assert_eq!(playlist_count(&nodes), 2 * GENRES + 1);
    }

    #[test]
    fn counts_playlists_at_every_depth() {
        let tracks = [
            "/music/house/deep/a.mp3",
            "/music/house/b.mp3",
            "/music/techno/c.mp3",
            "/loose.mp3",
        ];
        let nodes = playlists_from_folders(&paths(&["/music"]), &paths(&tracks), "export");
        assert_eq!(playlist_count(&nodes), 4);
        assert_eq!(playlist_count(&[]), 0);
    }
}
