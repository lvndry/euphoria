// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Missing tracks in the window: showing them, pointing one at the file it
//! is now, checked by content, and taking them out of the library.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use euphoria_core::{CachedAnalyzer, ContentHash, RelinkMatch, check_relink, find_in_folder};
use gpui::{Context, PathPromptOptions};

use crate::app::EuphoriaApp;
use crate::library::LibraryFilter;

/// What locating a missing track found, worked out off the main thread.
#[derive(Debug)]
enum Located {
    /// The picked file holds the track's audio, or nothing was known to
    /// compare it with. Other missing tracks found beside it come along.
    Found {
        content: ContentHash,
        moved_with_it: Vec<(PathBuf, PathBuf, ContentHash)>,
    },
    /// The picked file holds other audio.
    OtherAudio,
    /// The picked file could not be read; the text says why.
    Unreadable(String),
}

/// The content each missing track held: the one its playlists or
/// favorites recorded, or else the one the cache last saw at its path.
fn content_of(
    path: &Path,
    known: Option<ContentHash>,
    cache: Option<&CachedAnalyzer>,
) -> Option<ContentHash> {
    known.or_else(|| cache.and_then(|cache| cache.last_content_at(path)))
}

/// Compares `picked` with the missing track at `old`, and looks beside it
/// for the other missing tracks, by name and content.
fn locate(
    old: &Path,
    old_content: Option<ContentHash>,
    picked: &Path,
    others: &[(PathBuf, Option<ContentHash>)],
    cache: Option<&CachedAnalyzer>,
) -> Located {
    let expected = content_of(old, old_content, cache);
    let content = match check_relink(expected.as_ref(), picked) {
        Ok(RelinkMatch::SameAudio(found) | RelinkMatch::NothingToCompare(found)) => found,
        Ok(RelinkMatch::OtherAudio) => return Located::OtherAudio,
        Err(error) => return Located::Unreadable(error.chain()),
    };
    let others: Vec<(PathBuf, ContentHash)> = others
        .iter()
        .filter(|(path, _)| path != old)
        .filter_map(|(path, known)| Some((path.clone(), content_of(path, *known, cache)?)))
        .collect();
    let moved_with_it = picked
        .parent()
        .map(|folder| find_in_folder(folder, &others))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(old_path, new_path)| {
            let content = others
                .iter()
                .find(|(path, _)| *path == old_path)
                .map(|(_, content)| *content)?;
            Some((old_path, new_path, content))
        })
        .collect();
    Located::Found {
        content,
        moved_with_it,
    }
}

impl EuphoriaApp {
    /// Shows only the tracks whose file is missing, within the folder on
    /// show.
    pub(crate) fn show_missing_tracks(&mut self, cx: &mut Context<'_, Self>) {
        self.close_set_builder();
        self.playlist = None;
        self.showing_favorites = false;
        self.filter = LibraryFilter::Missing;
        self.refresh_visible();
        cx.notify();
    }

    /// Asks for the file the selected missing track is now, then points the
    /// track at it when it holds the same audio.
    pub(crate) fn locate_selected_track(&mut self, cx: &mut Context<'_, Self>) {
        let Some(old) = self
            .selected_track()
            .filter(|track| track.is_missing())
            .map(|track| track.path.clone())
        else {
            return;
        };
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Locate".into()),
        });
        cx.spawn(async move |this, cx| {
            let picked = match chosen.await {
                Ok(Ok(Some(paths))) => paths.into_iter().next(),
                Ok(Ok(None)) | Err(_) => None,
                Ok(Err(error)) => {
                    this.update(cx, |this, cx| {
                        this.report(format!("Could not open the file picker: {error:#}"));
                        cx.notify();
                    })
                    .ok();
                    None
                }
            };
            if let Some(picked) = picked {
                this.update(cx, |this, cx| this.locate_track(old, picked, cx))
                    .ok();
            }
        })
        .detach();
    }

    /// Points the missing track of `old` at `picked` when `picked` holds the
    /// same audio, with every other missing track found beside it under its
    /// own name. Reading the files happens off the main thread.
    pub(crate) fn locate_track(
        &mut self,
        old: PathBuf,
        picked: PathBuf,
        cx: &mut Context<'_, Self>,
    ) {
        let old_content = self.session.known_content_of(&old);
        let others: Vec<(PathBuf, Option<ContentHash>)> = self
            .session
            .library()
            .tracks()
            .iter()
            .filter(|track| track.is_missing())
            .map(|track| {
                (
                    track.path.clone(),
                    self.session.known_content_of(&track.path),
                )
            })
            .collect();
        let cache: Option<Arc<CachedAnalyzer>> = self.session.cache();
        cx.spawn(async move |this, cx| {
            let background_picked = picked.clone();
            let background_old = old.clone();
            let located = cx
                .background_executor()
                .spawn(async move {
                    locate(
                        &background_old,
                        old_content,
                        &background_picked,
                        &others,
                        cache.as_deref(),
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                this.finish_locating(old, picked, located, cx)
            })
            .ok();
        })
        .detach();
    }

    fn finish_locating(
        &mut self,
        old: PathBuf,
        picked: PathBuf,
        located: Located,
        cx: &mut Context<'_, Self>,
    ) {
        let (content, moved_with_it) = match located {
            Located::Found {
                content,
                moved_with_it,
            } => (content, moved_with_it),
            Located::OtherAudio => {
                self.report(format!(
                    "{} holds other audio than the missing track, so the track was not \
                     linked to it.",
                    picked.display()
                ));
                cx.notify();
                return;
            }
            Located::Unreadable(reason) => {
                self.report(format!("Could not read {}: {reason}", picked.display()));
                cx.notify();
                return;
            }
        };
        let mut relinked = vec![picked.clone()];
        let mut saved_lists_changed = self.session.relink(&old, &picked, content);
        for (other_old, other_new, other_content) in &moved_with_it {
            saved_lists_changed |= self.session.relink(other_old, other_new, *other_content);
            relinked.push(other_new.clone());
        }
        self.selected = self.session.library().index_of(&picked);
        self.marked.clear();
        if saved_lists_changed {
            self.save_playlists(cx);
            self.save_favorites(cx);
        }
        self.report(match moved_with_it.len() {
            0 => format!("Linked the missing track to {}.", picked.display()),
            others => format!(
                "Linked the missing track to {}, and {} other {} found beside it.",
                picked.display(),
                others,
                if others == 1 { "track" } else { "tracks" }
            ),
        });
        self.add_paths(relinked, false, cx);
        self.refresh_visible();
        cx.notify();
    }

    /// Takes the missing tracks of the selection out of the library, their
    /// playlists and the favorites. Tracks whose file is there are left
    /// alone.
    pub(crate) fn remove_missing_tracks_of_selection(&mut self, cx: &mut Context<'_, Self>) {
        let paths = self.selected_paths();
        let removed = self.session.remove_missing(&paths);
        if removed.is_empty() {
            return;
        }
        self.selected = None;
        self.marked.clear();
        self.refresh_visible();
        self.save_playlists(cx);
        self.save_favorites(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_with_other_audio_is_refused_and_a_match_brings_its_neighbours() {
        let folder = crate::tag_writing::testing::ScratchFolder::new("locate-missing");
        let old_folder = folder.0.join("old");
        let new_folder = folder.0.join("new");
        std::fs::create_dir_all(&old_folder).unwrap();
        std::fs::create_dir_all(&new_folder).unwrap();
        let picked = new_folder.join("first.mp3");
        let neighbour = new_folder.join("second.mp3");
        let impostor = new_folder.join("impostor.mp3");
        std::fs::write(&picked, b"first track").unwrap();
        std::fs::write(&neighbour, b"second track").unwrap();
        std::fs::write(&impostor, b"other audio").unwrap();
        let first_content = ContentHash::of_file(&picked).unwrap();
        let second_content = ContentHash::of_file(&neighbour).unwrap();
        let old_first = old_folder.join("first.mp3");
        let old_second = old_folder.join("second.mp3");
        let others = vec![
            (old_first.clone(), Some(first_content)),
            (old_second.clone(), Some(second_content)),
        ];

        assert!(matches!(
            locate(&old_first, Some(first_content), &impostor, &others, None),
            Located::OtherAudio
        ));
        let Located::Found {
            content,
            moved_with_it,
        } = locate(&old_first, Some(first_content), &picked, &others, None)
        else {
            panic!("the picked file holds the track");
        };
        assert_eq!(content, first_content);
        assert_eq!(moved_with_it, vec![(old_second, neighbour, second_content)]);
        assert!(matches!(
            locate(&old_first, None, &impostor, &others, None),
            Located::Found { .. }
        ));
        assert!(matches!(
            locate(
                &old_first,
                None,
                &new_folder.join("gone.mp3"),
                &others,
                None
            ),
            Located::Unreadable(_)
        ));
    }
}
