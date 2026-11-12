// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Building playlists from the window: making, naming, labelling, ordering
//! and deleting them, adding tracks by drag or from a menu, reordering the
//! tracks of one by drag, and exporting one to rekordbox or as M3U8.
//!
//! Every change shows at once and is kept in the background.

use std::path::{Path, PathBuf};

use euphoria_core::export::rekordbox::{ExistingFile, ExportOptions, write_xml_file};
use euphoria_core::{
    AssembledLibrary, FileResult, M3uTrack, PlaylistEntry, PlaylistLayout, assemble_library,
    write_m3u8_file,
};
use gpui::{Context, Window};

use crate::app::EuphoriaApp;
use crate::library::LibraryFilter;
use crate::playlists::PlaylistId;

/// Tracks being dragged from the table: the row the drag started on, and
/// with it the rest of the selection when that row is selected.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DraggedTracks {
    pub(crate) from_track: usize,
}

/// A playlist being dragged in the sidebar.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DraggedPlaylist(pub(crate) PlaylistId);

/// The formats a playlist is exported in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaylistFormat {
    /// A rekordbox XML collection holding the playlist.
    Rekordbox,
    /// An M3U8 file, for any other program.
    M3u8,
}

impl PlaylistFormat {
    fn extension(self) -> &'static str {
        match self {
            PlaylistFormat::Rekordbox => "xml",
            PlaylistFormat::M3u8 => "m3u8",
        }
    }
}

/// What the export of a playlist wrote, or why it could not.
pub(crate) type ExportOutcome = Result<String, String>;

impl EuphoriaApp {
    /// Shows the tracks of the playlist in its order.
    pub(crate) fn show_playlist(&mut self, id: PlaylistId, cx: &mut Context<'_, Self>) {
        if self.session.playlists().get(id).is_none() {
            return;
        }
        self.playlist = Some(id);
        self.showing_favorites = false;
        self.filter = LibraryFilter::AllTracks;
        self.folder = None;
        self.sort_order = None;
        self.refresh_visible();
        cx.notify();
    }

    /// Goes back to the playlist's own order after sorting it.
    pub(crate) fn show_playlist_order(&mut self, cx: &mut Context<'_, Self>) {
        self.sort_order = None;
        self.refresh_visible();
        cx.notify();
    }

    /// Makes an empty playlist, shows it and starts naming it.
    pub(crate) fn create_playlist(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> PlaylistId {
        let id = self.session.playlists_mut().create();
        self.show_playlist(id, cx);
        self.start_renaming_playlist(id, window, cx);
        self.save_playlists(cx);
        id
    }

    /// Makes a playlist of the selected tracks, in table order, and starts
    /// naming it.
    pub(crate) fn create_playlist_from_selection(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let paths = self.selected_paths();
        let id = self.session.playlists_mut().create();
        self.session.playlists_mut().add_tracks(id, &paths);
        self.show_playlist(id, cx);
        self.start_renaming_playlist(id, window, cx);
        self.save_playlists(cx);
    }

    /// Puts the playlist's name in the field over its row.
    pub(crate) fn start_renaming_playlist(
        &mut self,
        id: PlaylistId,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(name) = self
            .session
            .playlists()
            .get(id)
            .map(|playlist| playlist.name.clone())
        else {
            return;
        };
        self.renaming_playlist = Some(id);
        self.playlist_name_input.update(cx, |input, cx| {
            input.set_value(name, window, cx);
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        cx.notify();
    }

    /// Takes the name typed for the playlist being renamed. An empty name
    /// keeps the old one.
    pub(crate) fn finish_renaming_playlist(&mut self, cx: &mut Context<'_, Self>) {
        let Some(id) = self.renaming_playlist.take() else {
            return;
        };
        let typed = self.playlist_name_input.read(cx).value().to_string();
        if self.session.playlists_mut().rename(id, &typed) {
            self.save_playlists(cx);
        }
        cx.notify();
    }

    /// Gives the playlist an emoji label, or takes it away.
    pub(crate) fn set_playlist_label(
        &mut self,
        id: PlaylistId,
        label: Option<&str>,
        cx: &mut Context<'_, Self>,
    ) {
        self.session.playlists_mut().set_label(id, label);
        self.save_playlists(cx);
        cx.notify();
    }

    /// Deletes the playlist. Its tracks stay in the library.
    pub(crate) fn delete_playlist(&mut self, id: PlaylistId, cx: &mut Context<'_, Self>) {
        self.session.playlists_mut().delete(id);
        if self.playlist == Some(id) {
            self.show_all_tracks(cx);
        }
        if self.renaming_playlist == Some(id) {
            self.renaming_playlist = None;
        }
        self.save_playlists(cx);
        cx.notify();
    }

    /// Moves the dragged playlist to just before `before`, or to the end.
    pub(crate) fn drop_playlist(
        &mut self,
        dragged: PlaylistId,
        before: Option<PlaylistId>,
        cx: &mut Context<'_, Self>,
    ) {
        let playlists = self.session.playlists();
        let position = before
            .and_then(|before| {
                playlists
                    .all()
                    .iter()
                    .position(|playlist| playlist.id == before)
            })
            .unwrap_or(playlists.all().len());
        self.session
            .playlists_mut()
            .move_playlist(dragged, position);
        self.save_playlists(cx);
        cx.notify();
    }

    /// The files of the selected tracks, in the order the table shows them.
    pub(crate) fn selected_paths(&self) -> Vec<PathBuf> {
        let library = self.session.library();
        self.selected_tracks()
            .into_iter()
            .filter_map(|index| library.track(index))
            .map(|track| track.path.clone())
            .collect()
    }

    /// The files dragged: the selection when the drag started on a selected
    /// row, that row alone otherwise.
    pub(crate) fn dragged_paths(&self, dragged: DraggedTracks) -> Vec<PathBuf> {
        if self.is_marked(dragged.from_track) {
            self.selected_paths()
        } else {
            self.session
                .library()
                .track(dragged.from_track)
                .map(|track| vec![track.path.clone()])
                .unwrap_or_default()
        }
    }

    /// Tracks dropped on a playlist in the sidebar go to its end.
    pub(crate) fn drop_tracks_on_playlist(
        &mut self,
        id: PlaylistId,
        dragged: DraggedTracks,
        cx: &mut Context<'_, Self>,
    ) {
        let paths = self.dragged_paths(dragged);
        self.add_to_playlist(id, &paths, cx);
    }

    /// Adds the selected tracks to the end of the playlist.
    pub(crate) fn add_selection_to_playlist(&mut self, id: PlaylistId, cx: &mut Context<'_, Self>) {
        let paths = self.selected_paths();
        self.add_to_playlist(id, &paths, cx);
    }

    /// Adds tracks to the end of the playlist.
    pub(crate) fn add_to_playlist(
        &mut self,
        id: PlaylistId,
        paths: &[PathBuf],
        cx: &mut Context<'_, Self>,
    ) {
        if self.session.playlists_mut().add_tracks(id, paths) > 0 {
            if self.playlist == Some(id) {
                self.refresh_visible();
            }
            self.save_playlists(cx);
        }
        cx.notify();
    }

    /// Takes the selected tracks out of the playlist on show.
    pub(crate) fn remove_selection_from_playlist(&mut self, cx: &mut Context<'_, Self>) {
        let Some(id) = self.playlist else {
            return;
        };
        let paths = self.selected_paths();
        self.session.playlists_mut().remove_tracks(id, &paths);
        self.marked.clear();
        self.refresh_visible();
        self.save_playlists(cx);
        cx.notify();
    }

    /// True when rows can be dragged to a new place: in a playlist shown in
    /// its own order, with nothing hidden by the search.
    pub(crate) fn can_reorder_playlist(&self) -> bool {
        self.playlist.is_some() && self.sort_order.is_none() && self.query.trim().is_empty()
    }

    /// Tracks dropped on the row of `target_index` in the playlist on show
    /// move there; from elsewhere they are added there.
    pub(crate) fn drop_tracks_on_row(
        &mut self,
        dragged: DraggedTracks,
        target_index: usize,
        cx: &mut Context<'_, Self>,
    ) {
        let dragged_paths = self.dragged_paths(dragged);
        if !self.can_reorder_playlist() {
            return;
        }
        let Some(id) = self.playlist else {
            return;
        };
        let Some(target) = self
            .session
            .library()
            .track(target_index)
            .map(|track| track.path.clone())
        else {
            return;
        };
        let playlists = self.session.playlists_mut();
        playlists.add_tracks(id, &dragged_paths);
        let position = playlists
            .get(id)
            .and_then(|playlist| playlist.tracks.iter().position(|path| *path == target))
            .unwrap_or(usize::MAX);
        playlists.move_tracks(id, &dragged_paths, position);
        self.refresh_visible();
        self.save_playlists(cx);
        cx.notify();
    }

    /// Keeps the playlists in the background. Files are read there when
    /// their content is not known yet.
    pub(crate) fn save_playlists(&mut self, cx: &mut Context<'_, Self>) {
        self.refresh_suggestions();
        let Some(saver) = self.session.playlist_saver() else {
            return;
        };
        let history = saver.rewrites();
        self.session
            .playlists_mut()
            .follow_rewrite_history(&history);
        let rewrite_epoch = history.len();
        let playlists = self.session.playlists();
        saver.register(playlists, self.session.favorites());
        let revision = playlists.revision();
        let mut snapshot = playlists.clone();
        let cache = self.session.cache();
        let completion = saver.clone();
        cx.spawn(async move |this, cx| {
            let saved = cx
                .background_executor()
                .spawn(async move {
                    let stored = snapshot.stored_with_contents(|path| {
                        PlaylistEntry::for_file(path, cache.as_deref())
                            .ok()
                            .map(|entry| entry.content)
                    });
                    saver
                        .save(&stored, revision, rewrite_epoch)
                        .map(|()| snapshot.known_contents())
                })
                .await;
            this.update(cx, |this, cx| match saved {
                Ok(contents) => {
                    if this.session.playlists().revision() == revision {
                        this.session
                            .playlists_mut()
                            .follow_rewrite_history(&completion.rewrites());
                        this.session.playlists_mut().learn_contents(contents);
                    }
                }
                Err(error) => {
                    this.notice = Some(format!(
                        "The playlists could not be kept: {}",
                        error.chain()
                    ));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Asks where to save the playlist, then exports it there.
    pub(crate) fn export_playlist(
        &mut self,
        id: PlaylistId,
        format: PlaylistFormat,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(playlist) = self.session.playlists().get(id) else {
            return;
        };
        let name = playlist.name.clone();
        self.export_tracks(&name, format, cx, move |this| {
            this.playlist_export_job(id, format)
        });
    }

    /// Asks where to save an export named `name`, then writes there what
    /// `job` works out from the library as it is by then.
    pub(crate) fn export_tracks(
        &mut self,
        name: &str,
        format: PlaylistFormat,
        cx: &mut Context<'_, Self>,
        job: impl FnOnce(&Self) -> Option<PlaylistExport> + 'static,
    ) {
        let suggested = format!("{}.{}", file_name_for(name), format.extension());
        let directory = dirs_home().unwrap_or_else(|| PathBuf::from("/"));
        let chosen = cx.prompt_for_new_path(&directory, Some(&suggested));
        cx.spawn(async move |this, cx| {
            let destination = match chosen.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    this.update(cx, |this, cx| {
                        this.notice = Some(format!("Could not open the save dialog: {error:#}"));
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            let Ok(job) = this.update(cx, |this, _| job(this)) else {
                return;
            };
            let Some(job) = job else {
                return;
            };
            let outcome = cx
                .background_executor()
                .spawn(async move { job.write(&destination) })
                .await;
            this.update(cx, |this, cx| {
                this.notice = Some(match outcome {
                    Ok(message) | Err(message) => message,
                });
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// What exporting the playlist in `format` writes, worked out from the
    /// library as it is now.
    pub(crate) fn playlist_export_job(
        &self,
        id: PlaylistId,
        format: PlaylistFormat,
    ) -> Option<PlaylistExport> {
        let playlists = self.session.playlists();
        let playlist = playlists.get(id)?;
        Some(
            self.export_job(&playlist.name, &playlist.tracks, format, |path| {
                playlists.content_of(path)
            }),
        )
    }

    /// What exporting the tracks at `paths`, in order, as a playlist named
    /// `name` writes, worked out from the library as it is now.
    /// `content_of` gives the content each track had when it was added.
    pub(crate) fn export_job(
        &self,
        name: &str,
        paths: &[PathBuf],
        format: PlaylistFormat,
        content_of: impl Fn(&Path) -> Option<euphoria_core::ContentHash>,
    ) -> PlaylistExport {
        let library = self.session.library();
        let tracks = paths
            .iter()
            .filter_map(|path| {
                library
                    .index_of(path)
                    .and_then(|index| library.track(index))
            })
            .map(|track| ExportedTrack {
                path: track.path.clone(),
                content: content_of(&track.path),
                analysis: track
                    .analysis()
                    .map(|analysis| track.correction.apply(analysis.clone())),
                display_name: match &track.artist {
                    Some(artist) => format!("{artist} - {}", track.title()),
                    None => track.title().to_string(),
                },
            })
            .collect();
        PlaylistExport {
            name: name.to_string(),
            format,
            tracks,
            not_in_library: paths
                .iter()
                .filter(|path| library.index_of(path).is_none())
                .count(),
        }
    }
}

/// One track of a playlist export.
#[derive(Debug, Clone)]
pub(crate) struct ExportedTrack {
    path: PathBuf,
    /// The content the track had when it was added, when it is known. The
    /// file is compared with it before it is exported.
    content: Option<euphoria_core::ContentHash>,
    analysis: Option<euphoria_core::TrackAnalysis>,
    display_name: String,
}

/// A playlist ready to export off the main thread.
pub(crate) struct PlaylistExport {
    name: String,
    format: PlaylistFormat,
    tracks: Vec<ExportedTrack>,
    not_in_library: usize,
}

impl PlaylistExport {
    /// Writes the export to `destination`, replacing what is there: the
    /// save dialog already asked. Returns what to tell the user.
    pub(crate) fn write(self, destination: &Path) -> ExportOutcome {
        let failed = |error: euphoria_core::EuphoriaError| {
            format!("{} could not be exported: {}", self.name, error.chain())
        };
        // The files go into another program's library, so each one is read
        // and compared with the track that was added: a file replaced at
        // the same path by other audio is left out.
        let (tracks, replaced): (Vec<&ExportedTrack>, Vec<&ExportedTrack>) =
            self.tracks.iter().partition(|track| {
                track.content.is_none_or(|content| {
                    euphoria_core::ContentHash::of_file(&track.path)
                        .is_ok_and(|found| found == content)
                })
            });
        let left_out_note = |left_out: usize| {
            let missing = match left_out {
                0 => String::new(),
                1 => " 1 track was left out: it is not analyzed or not in the library.".to_string(),
                count => format!(
                    " {count} tracks were left out: they are not analyzed or not in the library."
                ),
            };
            let other_audio = match replaced.len() {
                0 => String::new(),
                1 => " 1 track was left out: its file now holds other audio than the track \
                       that was added."
                    .to_string(),
                count => format!(
                    " {count} tracks were left out: their files now hold other audio than the \
                     tracks that were added."
                ),
            };
            missing + &other_audio
        };
        match self.format {
            PlaylistFormat::Rekordbox => {
                let order: Vec<PathBuf> = tracks.iter().map(|track| track.path.clone()).collect();
                let results: Vec<FileResult> = tracks
                    .iter()
                    .filter_map(|track| {
                        track.analysis.clone().map(|analysis| FileResult {
                            path: track.path.clone(),
                            outcome: Ok(analysis),
                        })
                    })
                    .collect();
                let AssembledLibrary { library, .. } = assemble_library(
                    results,
                    PlaylistLayout::Ordered {
                        name: &self.name,
                        tracks: &order,
                    },
                );
                let exported = library.tracks.len();
                write_xml_file(
                    destination,
                    &library,
                    &ExportOptions::default(),
                    ExistingFile::Replace,
                )
                .map_err(failed)?;
                Ok(format!(
                    "Exported {} to {} for rekordbox: in rekordbox, open the XML view to import it.{}",
                    self.name,
                    destination.display(),
                    left_out_note(tracks.len() - exported + self.not_in_library)
                ))
            }
            PlaylistFormat::M3u8 => {
                let tracks: Vec<M3uTrack> = tracks
                    .iter()
                    .map(|track| M3uTrack {
                        path: track.path.clone(),
                        duration_seconds: track
                            .analysis
                            .as_ref()
                            .map(|analysis| analysis.duration_seconds),
                        display_name: track.display_name.clone(),
                    })
                    .collect();
                write_m3u8_file(destination, &self.name, &tracks, ExistingFile::Replace)
                    .map_err(failed)?;
                Ok(format!(
                    "Exported {} to {}.{}",
                    self.name,
                    destination.display(),
                    left_out_note(self.not_in_library)
                ))
            }
        }
    }
}

/// The playlist's name with the characters a file name cannot hold left
/// out.
fn file_name_for(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|character| !matches!(character, '/' | '\\' | ':' | '\0'))
        .collect();
    let cleaned = cleaned.trim().trim_start_matches('.').to_string();
    if cleaned.is_empty() {
        "Playlist".to_string()
    } else {
        cleaned
    }
}

/// The user's home folder, where the save dialog starts.
fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_name_keeps_what_a_file_name_can_hold() {
        assert_eq!(file_name_for("Peak time \u{1f525}"), "Peak time \u{1f525}");
        assert_eq!(file_name_for("A/B: mix"), "AB mix");
        assert_eq!(file_name_for("../"), "Playlist");
        assert_eq!(file_name_for("  "), "Playlist");
    }
}
