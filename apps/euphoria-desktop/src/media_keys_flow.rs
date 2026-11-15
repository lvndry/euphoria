// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the remote commands do in the app, and what Now Playing shows.
//!
//! While the decks are open, the transport is the deck that is heard more:
//! the one the crossfader favours, deck A when it is centered. When that deck
//! has no track, the other one. Next and Previous do nothing then, as the
//! decks have no list. In the set builder with a set, they follow the set
//! order, as the set does by itself when a track ends; elsewhere they move
//! along the rows shown.

use gpui::Context;

use crate::app::EuphoriaApp;
use crate::deck_audio::DeckId;
use crate::media_keys::{
    Direction, NowPlaying, RESTART_AFTER_SECONDS, RemoteCommand, TransportAction, action_for,
    neighbour,
};

impl EuphoriaApp {
    /// The deck the media keys control while the decks are open: a deck that
    /// plays, the favoured one first, else a deck with a track.
    fn media_deck(&self) -> Option<DeckId> {
        let decks = if self.decks.crossfader > 0.5 {
            [DeckId::B, DeckId::A]
        } else {
            [DeckId::A, DeckId::B]
        };
        decks
            .into_iter()
            .find(|deck| self.decks.view(*deck).playing)
            .or_else(|| {
                decks.into_iter().find(|deck| {
                    let view = self.decks.view(*deck);
                    view.track_index.is_some() && view.problem.is_none()
                })
            })
    }

    fn transport_is_playing(&self) -> bool {
        if self.decks.is_open {
            self.media_deck()
                .is_some_and(|deck| self.decks.view(deck).playing)
        } else {
            self.player.offers_pause()
        }
    }

    /// What Now Playing shows: the track of the transport while it plays or
    /// is paused, and nothing while it is stopped.
    pub(crate) fn now_playing(&self) -> Option<NowPlaying> {
        let library = self.session.library();
        if self.decks.is_open {
            let deck = self.media_deck()?;
            let view = self.decks.view(deck);
            let track = library.track(view.track_index?)?;
            return Some(NowPlaying {
                title: track.title().to_string(),
                artist: track.shown_artist().map(std::borrow::Cow::into_owned),
                duration_seconds: view.duration_seconds,
                elapsed_seconds: self.decks.position_seconds(deck),
                rate: if view.playing { view.rate } else { 0.0 },
                playing: view.playing,
            });
        }
        let path = self.player.track()?;
        let holds_track = self.player.is_playing()
            || self.player.offers_pause()
            || matches!(self.player.status(), crate::player::PlayerStatus::Paused);
        if !holds_track {
            return None;
        }
        let track = library.track_by_path(path);
        let playing = self.player.is_playing();
        Some(NowPlaying {
            title: track.map_or_else(
                || {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_default()
                },
                |track| track.title().to_string(),
            ),
            artist: track.and_then(|track| track.shown_artist().map(std::borrow::Cow::into_owned)),
            duration_seconds: self.player.duration_seconds().map(f64::from),
            elapsed_seconds: f64::from(self.player.position_seconds()),
            rate: if playing { 1.0 } else { 0.0 },
            playing,
        })
    }

    /// Carries out a command from the keyboard, the headphones or the
    /// system's Now Playing widget.
    pub(crate) fn perform_remote(&mut self, command: RemoteCommand, cx: &mut Context<'_, Self>) {
        let Some(action) = action_for(command, self.transport_is_playing()) else {
            return;
        };
        match action {
            TransportAction::Play => self.remote_play(cx),
            TransportAction::Pause => self.remote_pause(cx),
            TransportAction::NextTrack => self.remote_step(Direction::Forward, cx),
            TransportAction::PreviousTrack => self.remote_step(Direction::Backward, cx),
            TransportAction::SeekTo(seconds) => self.remote_seek(seconds, cx),
        }
        cx.notify();
    }

    fn remote_play(&mut self, cx: &mut Context<'_, Self>) {
        if self.decks.is_open {
            if let Some(deck) = self.media_deck() {
                self.toggle_deck_play(deck, cx);
            }
        } else if self.player.track().is_some() && !self.set_row_selected_to_play() {
            self.toggle_playback(cx);
        } else {
            self.play_or_pause_selection(cx);
        }
    }

    fn set_row_selected_to_play(&self) -> bool {
        self.set_row_to_play().is_some()
    }

    fn remote_pause(&mut self, cx: &mut Context<'_, Self>) {
        if self.decks.is_open {
            if let Some(deck) = self.media_deck() {
                self.toggle_deck_play(deck, cx);
            }
        } else {
            let request = self.player.toggle();
            self.carry_out(request, cx);
        }
    }

    fn remote_seek(&mut self, seconds: f64, cx: &mut Context<'_, Self>) {
        if self.decks.is_open {
            let Some(deck) = self.media_deck() else {
                return;
            };
            let Some(duration) = self.decks.view(deck).duration_seconds.filter(|d| *d > 0.0) else {
                return;
            };
            self.seek_deck(deck, (seconds / duration) as f32, cx);
        } else {
            let Some(duration) = self.player.duration_seconds().filter(|d| *d > 0.0) else {
                return;
            };
            let request = self
                .player
                .seek_to_fraction((seconds as f32 / duration).clamp(0.0, 1.0));
            self.carry_out(request, cx);
        }
    }

    fn remote_step(&mut self, direction: Direction, cx: &mut Context<'_, Self>) {
        if self.decks.is_open {
            return;
        }
        let elapsed = f64::from(self.player.position_seconds());
        let restarts = direction == Direction::Backward
            && self.player.offers_pause()
            && elapsed > RESTART_AFTER_SECONDS;
        if let Some(position) = self.set_position_for_remote() {
            let length = self
                .set_builder
                .planned
                .as_ref()
                .map_or(0, |planned| planned.paths.len());
            let target = if restarts {
                Some(position)
            } else {
                neighbour(Some(position), length, direction)
            };
            if let Some(target) = target {
                self.play_set_track(target, 0.0, cx);
            }
            return;
        }
        if restarts {
            let request = self.player.seek_to_fraction(0.0);
            self.carry_out(request, cx);
            return;
        }
        let current = self
            .player
            .pending_track()
            .or(self.player.track())
            .and_then(|path| self.session.library().index_of(path))
            .or(self.selected);
        let current_row =
            current.and_then(|index| self.visible.iter().position(|visible| *visible == index));
        if let Some(row) = neighbour(current_row, self.visible.len(), direction) {
            let index = self.visible[row];
            self.select_and_reveal(index, cx);
            self.play_selection(cx);
        }
    }

    /// The place in the set Next and Previous start from: where the player
    /// is in it, or the selected row when the player is not sounding.
    fn set_position_for_remote(&self) -> Option<usize> {
        if !self.set_builder.is_open || self.set_builder.planned.is_none() {
            return None;
        }
        self.set_playing_position().or_else(|| {
            (!self.player.offers_pause())
                .then_some(self.set_builder.selected_row)
                .flatten()
        })
    }
}
