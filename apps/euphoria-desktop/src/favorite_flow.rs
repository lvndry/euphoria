// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Favorites from the window: starring tracks from the table, the track
//! detail, the decks and the keyboard, marking the transition between the
//! decks or into a track that mixes well, writing a note on it, showing the
//! starred tracks, and exporting them to rekordbox or as M3U8.
//!
//! Every change shows at once and is kept in the background, with the
//! playlists.

use std::path::{Path, PathBuf};

use gpui::{Context, Window};

use crate::app::EuphoriaApp;
use crate::deck_audio::DeckId;
use crate::favorites::FAVORITES_NAME;
use crate::library::LibraryFilter;
use crate::playlist_flow::{PlaylistFormat, content_for_saving};

impl EuphoriaApp {
    /// Shows the starred tracks, in the order they were starred.
    pub(crate) fn show_favorites(&mut self, cx: &mut Context<'_, Self>) {
        self.playlist = None;
        self.showing_favorites = true;
        self.filter = LibraryFilter::AllTracks;
        self.folder = None;
        self.sort_order = None;
        self.refresh_visible();
        cx.notify();
    }

    /// Stars the selected tracks, or takes their stars off when all of them
    /// are starred: what the keyboard shortcut does.
    pub(crate) fn toggle_star_on_selection(&mut self, cx: &mut Context<'_, Self>) {
        let mut paths = self.selected_paths();
        if paths.is_empty()
            && let Some(track) = self.selected_track()
        {
            paths.push(track.path.clone());
        }
        self.toggle_stars(&paths, cx);
    }

    /// Stars the track at `index` in the library, or takes its star off.
    pub(crate) fn toggle_star_on_track(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        if let Some(path) = self
            .session
            .library()
            .track(index)
            .map(|track| track.path.clone())
        {
            self.toggle_stars(&[path], cx);
        }
    }

    /// Stars the track on `deck`, or takes its star off.
    pub(crate) fn toggle_star_on_deck(&mut self, deck: DeckId, cx: &mut Context<'_, Self>) {
        if let Some(path) = self.decks.view(deck).path.clone() {
            self.toggle_stars(&[path], cx);
        }
    }

    fn toggle_stars(&mut self, paths: &[PathBuf], cx: &mut Context<'_, Self>) {
        if paths.is_empty() {
            return;
        }
        self.session.favorites_mut().toggle_stars(paths);
        if self.showing_favorites {
            self.refresh_visible();
        }
        self.save_favorites(cx);
        cx.notify();
    }

    /// The tracks on deck A and deck B, when both are loaded.
    pub(crate) fn deck_pair(&self) -> Option<(PathBuf, PathBuf)> {
        let from = self.decks.view(DeckId::A).path.clone()?;
        let to = self.decks.view(DeckId::B).path.clone()?;
        (from != to).then_some((from, to))
    }

    /// Marks the transition from deck A into deck B, or takes the mark off.
    pub(crate) fn toggle_deck_transition(&mut self, cx: &mut Context<'_, Self>) {
        if let Some((from, to)) = self.deck_pair() {
            self.toggle_transition(&from, &to, cx);
        }
    }

    /// Marks the transition from the selected track into the track at
    /// `index`, or takes the mark off.
    pub(crate) fn toggle_transition_from_selected(
        &mut self,
        index: usize,
        cx: &mut Context<'_, Self>,
    ) {
        let library = self.session.library();
        let Some(from) = self.selected_track().map(|track| track.path.clone()) else {
            return;
        };
        let Some(to) = library.track(index).map(|track| track.path.clone()) else {
            return;
        };
        self.toggle_transition(&from, &to, cx);
    }

    fn toggle_transition(&mut self, from: &Path, to: &Path, cx: &mut Context<'_, Self>) {
        let is_marked = self.session.favorites_mut().toggle_transition(from, to);
        if !is_marked
            && self
                .editing_transition_note
                .as_ref()
                .is_some_and(|(editing_from, editing_to)| editing_from == from && editing_to == to)
        {
            self.editing_transition_note = None;
        }
        self.refresh_mixes();
        self.save_favorites(cx);
        cx.notify();
    }

    /// Puts the note of the transition from `from` into `to` in the field,
    /// to be typed.
    pub(crate) fn start_editing_transition_note(
        &mut self,
        from: PathBuf,
        to: PathBuf,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(transition) = self.session.favorites().transition(&from, &to) else {
            return;
        };
        let note = transition.note.clone().unwrap_or_default();
        self.editing_transition_note = Some((from, to));
        self.transition_note_input.update(cx, |input, cx| {
            input.set_value(note, window, cx);
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        cx.notify();
    }

    /// Takes the note typed for the transition being noted. An empty note
    /// takes the note away.
    pub(crate) fn finish_editing_transition_note(&mut self, cx: &mut Context<'_, Self>) {
        let Some((from, to)) = self.editing_transition_note.take() else {
            return;
        };
        let typed = self.transition_note_input.read(cx).value().to_string();
        let before = self.session.favorites().revision();
        self.session.favorites_mut().set_note(&from, &to, &typed);
        if self.session.favorites().revision() != before {
            self.refresh_mixes();
            self.save_favorites(cx);
        }
        cx.notify();
    }

    /// Keeps the favorites in the background. Files are read there when
    /// their content is not known yet.
    pub(crate) fn save_favorites(&mut self, cx: &mut Context<'_, Self>) {
        self.refresh_suggestions();
        self.refresh_mixes();
        let Some(saver) = self.session.playlist_saver() else {
            return;
        };
        let history = saver.rewrites();
        self.session
            .favorites_mut()
            .follow_rewrite_history(&history);
        let rewrite_epoch = history.len();
        saver.register(self.session.playlists(), self.session.favorites());
        let favorites = self.session.favorites();
        let revision = favorites.revision();
        let mut snapshot = favorites.clone();
        let cache = self.session.cache();
        let completion = saver.clone();
        cx.spawn(async move |this, cx| {
            let saved = cx
                .background_executor()
                .spawn(async move {
                    let stored = snapshot
                        .stored_with_contents(|path| content_for_saving(path, cache.as_deref()))?;
                    saver
                        .save_favorites(&stored, revision, rewrite_epoch)
                        .map(|()| snapshot.known_contents())
                })
                .await;
            this.update(cx, |this, cx| match saved {
                Ok(contents) => {
                    if this.session.favorites().revision() == revision {
                        this.session
                            .favorites_mut()
                            .follow_rewrite_history(&completion.rewrites());
                        this.session.favorites_mut().learn_contents(contents);
                    }
                }
                Err(error) => {
                    this.notice = Some(format!(
                        "The favorites could not be kept: {}",
                        error.chain()
                    ));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Asks where to save the starred tracks as a playlist, then exports
    /// them there.
    pub(crate) fn export_favorites(&mut self, format: PlaylistFormat, cx: &mut Context<'_, Self>) {
        self.export_tracks(FAVORITES_NAME, format, cx, move |this| {
            Some(this.favorites_export_job(format))
        });
    }

    /// What exporting the starred tracks in `format` writes.
    pub(crate) fn favorites_export_job(
        &self,
        format: PlaylistFormat,
    ) -> crate::playlist_flow::PlaylistExport {
        let favorites = self.session.favorites();
        self.export_job(FAVORITES_NAME, favorites.tracks(), format, |path| {
            favorites.content_of(path)
        })
    }
}
