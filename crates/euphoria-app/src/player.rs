// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the player is doing and what it should do next. Nothing here makes
//! sound or needs a window: each change of state returns the request to pass
//! on to the audio output.

use std::path::{Path, PathBuf};

use crate::format;

/// What the player is doing.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PlayerStatus {
    /// Nothing is playing. The position is where playing would start.
    Stopped,
    /// A track was asked for and its file is being opened.
    Starting,
    /// Sound is coming out.
    Playing,
    /// Held at the current position.
    Paused,
    /// The track could not be played; the text says why, briefly.
    Failed(String),
}

/// What the audio output should do after a change of state.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PlayerRequest {
    /// Open the file and play it from this position, replacing whatever is
    /// playing.
    Start {
        /// The file to play.
        path: PathBuf,
        /// Where to start, in seconds from the beginning.
        from_seconds: f32,
    },
    /// Hold the sound at the current position.
    Pause,
    /// Carry on from where the sound was held.
    Resume,
    /// Jump to this position, in seconds from the beginning.
    SeekTo(f32),
    /// Let go of whatever is playing.
    Stop,
}

/// The player: one track, a position in it, and whether it is sounding.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Player {
    track: Option<PathBuf>,
    status: PlayerStatus,
    position_seconds: f32,
    duration_seconds: Option<f32>,
}

impl Default for Player {
    fn default() -> Self {
        Self {
            track: None,
            status: PlayerStatus::Stopped,
            position_seconds: 0.0,
            duration_seconds: None,
        }
    }
}

impl Player {
    /// The track the player holds, when it holds one.
    pub(crate) fn track(&self) -> Option<&Path> {
        self.track.as_deref()
    }

    /// What the player is doing.
    #[cfg(test)]
    pub(crate) fn status(&self) -> &PlayerStatus {
        &self.status
    }

    /// True while sound is coming out.
    pub(crate) fn is_playing(&self) -> bool {
        self.status == PlayerStatus::Playing
    }

    /// True while the control should offer to pause: sound is coming out or
    /// is about to.
    pub(crate) fn offers_pause(&self) -> bool {
        matches!(self.status, PlayerStatus::Playing | PlayerStatus::Starting)
    }

    /// The position in seconds from the beginning of the track.
    pub(crate) fn position_seconds(&self) -> f32 {
        self.position_seconds
    }

    /// The length of the track in seconds, when it is known.
    pub(crate) fn duration_seconds(&self) -> Option<f32> {
        self.duration_seconds
    }

    /// How far through the track the position is, from 0.0 to 1.0, when the
    /// length is known.
    pub(crate) fn progress(&self) -> Option<f32> {
        let duration = self.duration_seconds.filter(|duration| *duration > 0.0)?;
        Some((self.position_seconds / duration).clamp(0.0, 1.0))
    }

    /// Why the track could not be played, when it could not.
    pub(crate) fn failure(&self) -> Option<&str> {
        match &self.status {
            PlayerStatus::Failed(reason) => Some(reason),
            _ => None,
        }
    }

    /// The position and the length, for example "2:23 / 6:48", or only the
    /// position when the length is not known.
    pub(crate) fn time_label(&self) -> String {
        let position = format::duration(self.position_seconds);
        match self.duration_seconds {
            Some(duration) => format!("{position} / {}", format::duration(duration)),
            None => position,
        }
    }

    /// Follows the selection in the table. While a track is playing, the
    /// newly selected one starts playing in its place. Otherwise the player
    /// takes the new track and waits at its beginning.
    pub(crate) fn select(
        &mut self,
        path: Option<&Path>,
        duration_seconds: Option<f32>,
    ) -> Option<PlayerRequest> {
        if self.track.as_deref() == path {
            if self.duration_seconds.is_none() {
                self.duration_seconds = duration_seconds;
            }
            return None;
        }
        let was_sounding = self.offers_pause();
        let held_a_voice = self.status == PlayerStatus::Paused || was_sounding;
        self.track = path.map(Path::to_path_buf);
        self.position_seconds = 0.0;
        self.duration_seconds = duration_seconds;
        match path {
            Some(path) if was_sounding => {
                self.status = PlayerStatus::Starting;
                Some(PlayerRequest::Start {
                    path: path.to_path_buf(),
                    from_seconds: 0.0,
                })
            }
            _ => {
                self.status = PlayerStatus::Stopped;
                held_a_voice.then_some(PlayerRequest::Stop)
            }
        }
    }

    /// The play and pause control, or the space bar.
    pub(crate) fn toggle(&mut self) -> Option<PlayerRequest> {
        match self.status {
            PlayerStatus::Playing => {
                self.status = PlayerStatus::Paused;
                Some(PlayerRequest::Pause)
            }
            PlayerStatus::Paused => {
                self.status = PlayerStatus::Playing;
                Some(PlayerRequest::Resume)
            }
            PlayerStatus::Starting => {
                self.status = PlayerStatus::Stopped;
                Some(PlayerRequest::Stop)
            }
            PlayerStatus::Stopped | PlayerStatus::Failed(_) => {
                let path = self.track.clone()?;
                if self.is_at_the_end() {
                    self.position_seconds = 0.0;
                }
                self.status = PlayerStatus::Starting;
                Some(PlayerRequest::Start {
                    path,
                    from_seconds: self.position_seconds,
                })
            }
        }
    }

    /// A click or drag on the waveform, `fraction` of the way along it.
    /// While the track sounds or is paused the sound jumps there. Otherwise
    /// the position is kept for when playing starts.
    pub(crate) fn seek_to_fraction(&mut self, fraction: f32) -> Option<PlayerRequest> {
        self.track.as_ref()?;
        let duration = self.duration_seconds.filter(|duration| *duration > 0.0)?;
        self.position_seconds = fraction.clamp(0.0, 1.0) * duration;
        match self.status {
            PlayerStatus::Playing | PlayerStatus::Paused => {
                Some(PlayerRequest::SeekTo(self.position_seconds))
            }
            PlayerStatus::Starting | PlayerStatus::Stopped | PlayerStatus::Failed(_) => None,
        }
    }

    /// The output opened `path` and sound is starting. Returns false when
    /// the player has moved on since it asked, in which case the output
    /// should let go of the sound.
    pub(crate) fn started(&mut self, path: &Path, duration_seconds: Option<f32>) -> bool {
        if self.status != PlayerStatus::Starting || self.track.as_deref() != Some(path) {
            return false;
        }
        self.status = PlayerStatus::Playing;
        if self.duration_seconds.is_none() {
            self.duration_seconds = duration_seconds;
        }
        true
    }

    /// The output could not play `path`. Ignored when the player has moved
    /// on to another track since it asked.
    pub(crate) fn failed(&mut self, path: &Path, reason: String) {
        if self.track.as_deref() == Some(path) {
            self.status = PlayerStatus::Failed(reason);
        }
    }

    /// The output reports how far the sound has got.
    pub(crate) fn advance_to(&mut self, position_seconds: f32) {
        if self.status == PlayerStatus::Playing {
            self.position_seconds = match self.duration_seconds {
                Some(duration) => position_seconds.clamp(0.0, duration),
                None => position_seconds.max(0.0),
            };
        }
    }

    /// The sound reached the end of the track.
    pub(crate) fn finished(&mut self) {
        if self.status == PlayerStatus::Playing {
            self.status = PlayerStatus::Stopped;
            if let Some(duration) = self.duration_seconds {
                self.position_seconds = duration;
            }
        }
    }

    fn is_at_the_end(&self) -> bool {
        self.duration_seconds
            .is_some_and(|duration| self.position_seconds >= duration - END_TOLERANCE_SECONDS)
    }
}

/// How close to the end counts as the end, so that play after a finished
/// track starts it again instead of playing its last instant.
const END_TOLERANCE_SECONDS: f32 = 0.25;

/// How far along a strip a pointer is, from 0.0 at its left edge to 1.0 at
/// its right edge. A pointer outside the strip counts as its nearest edge.
pub(crate) fn fraction_along(pointer_x: f32, strip_left: f32, strip_width: f32) -> f32 {
    if strip_width <= 0.0 {
        return 0.0;
    }
    ((pointer_x - strip_left) / strip_width).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first() -> &'static Path {
        Path::new("/music/first.flac")
    }

    fn second() -> &'static Path {
        Path::new("/music/second.flac")
    }

    fn start(path: &Path, from_seconds: f32) -> Option<PlayerRequest> {
        Some(PlayerRequest::Start {
            path: path.to_path_buf(),
            from_seconds,
        })
    }

    /// A player holding the first track, seven minutes long, and playing it.
    fn playing() -> Player {
        let mut player = Player::default();
        player.select(Some(first()), Some(420.0));
        assert_eq!(player.toggle(), start(first(), 0.0));
        assert!(player.started(first(), Some(420.0)));
        player
    }

    #[test]
    fn a_new_player_holds_nothing_and_does_nothing() {
        let mut player = Player::default();
        assert_eq!(player.status(), &PlayerStatus::Stopped);
        assert_eq!(player.track(), None);
        assert_eq!(player.toggle(), None);
        assert_eq!(player.seek_to_fraction(0.5), None);
        assert_eq!(player.progress(), None);
        assert_eq!(player.time_label(), "0:00");
    }

    #[test]
    fn selecting_a_track_while_stopped_does_not_play_it() {
        let mut player = Player::default();
        assert_eq!(player.select(Some(first()), Some(420.0)), None);
        assert_eq!(player.track(), Some(first()));
        assert_eq!(player.status(), &PlayerStatus::Stopped);
        assert_eq!(player.time_label(), "0:00 / 7:00");
    }

    #[test]
    fn play_starts_the_selected_track_and_sounds_once_the_output_has_it() {
        let mut player = Player::default();
        player.select(Some(first()), Some(420.0));
        assert_eq!(player.toggle(), start(first(), 0.0));
        assert_eq!(player.status(), &PlayerStatus::Starting);
        assert!(player.offers_pause());
        assert!(!player.is_playing());

        assert!(player.started(first(), Some(420.0)));
        assert!(player.is_playing());
    }

    #[test]
    fn toggling_pauses_and_resumes_at_the_same_position() {
        let mut player = playing();
        player.advance_to(143.0);
        assert_eq!(player.toggle(), Some(PlayerRequest::Pause));
        assert_eq!(player.status(), &PlayerStatus::Paused);
        player.advance_to(150.0);
        assert_eq!(player.position_seconds(), 143.0);
        assert_eq!(player.time_label(), "2:23 / 7:00");

        assert_eq!(player.toggle(), Some(PlayerRequest::Resume));
        assert!(player.is_playing());
        assert_eq!(player.position_seconds(), 143.0);
    }

    #[test]
    fn selecting_another_track_while_playing_switches_to_it() {
        let mut player = playing();
        player.advance_to(60.0);
        assert_eq!(
            player.select(Some(second()), Some(300.0)),
            start(second(), 0.0)
        );
        assert_eq!(player.track(), Some(second()));
        assert_eq!(player.status(), &PlayerStatus::Starting);
        assert_eq!(player.position_seconds(), 0.0);
        assert_eq!(player.duration_seconds(), Some(300.0));
    }

    #[test]
    fn selecting_another_track_while_paused_stops_and_waits() {
        let mut player = playing();
        player.toggle();
        assert_eq!(
            player.select(Some(second()), Some(300.0)),
            Some(PlayerRequest::Stop)
        );
        assert_eq!(player.status(), &PlayerStatus::Stopped);
        assert_eq!(player.position_seconds(), 0.0);
    }

    #[test]
    fn selecting_the_same_track_again_changes_nothing() {
        let mut player = playing();
        player.advance_to(60.0);
        assert_eq!(player.select(Some(first()), Some(420.0)), None);
        assert!(player.is_playing());
        assert_eq!(player.position_seconds(), 60.0);
    }

    #[test]
    fn clearing_the_selection_stops_the_sound() {
        let mut player = playing();
        assert_eq!(player.select(None, None), Some(PlayerRequest::Stop));
        assert_eq!(player.track(), None);
        assert_eq!(player.status(), &PlayerStatus::Stopped);
    }

    #[test]
    fn a_late_answer_for_a_track_the_player_left_is_turned_away() {
        let mut player = Player::default();
        player.select(Some(first()), Some(420.0));
        player.toggle();
        player.select(Some(second()), Some(300.0));
        assert!(!player.started(first(), Some(420.0)));
        assert_eq!(player.status(), &PlayerStatus::Starting);

        player.failed(first(), "File not found".to_string());
        assert_eq!(player.failure(), None);
        assert!(player.started(second(), Some(300.0)));
    }

    #[test]
    fn stopping_while_the_file_is_still_opening_cancels_the_start() {
        let mut player = Player::default();
        player.select(Some(first()), Some(420.0));
        player.toggle();
        assert_eq!(player.toggle(), Some(PlayerRequest::Stop));
        assert_eq!(player.status(), &PlayerStatus::Stopped);
        assert!(!player.started(first(), Some(420.0)));
    }

    #[test]
    fn a_failure_is_kept_as_a_short_message_and_play_tries_again() {
        let mut player = Player::default();
        player.select(Some(first()), Some(420.0));
        player.toggle();
        player.failed(first(), "File not found".to_string());
        assert_eq!(player.failure(), Some("File not found"));
        assert!(!player.offers_pause());

        assert_eq!(player.toggle(), start(first(), 0.0));
        assert_eq!(player.failure(), None);
    }

    #[test]
    fn a_failure_is_forgotten_when_another_track_is_selected() {
        let mut player = Player::default();
        player.select(Some(first()), Some(420.0));
        player.toggle();
        player.failed(first(), "No audio output".to_string());
        assert_eq!(player.select(Some(second()), None), None);
        assert_eq!(player.failure(), None);
    }

    #[test]
    fn seeking_while_playing_jumps_the_sound() {
        let mut player = playing();
        assert_eq!(
            player.seek_to_fraction(0.5),
            Some(PlayerRequest::SeekTo(210.0))
        );
        assert_eq!(player.position_seconds(), 210.0);
        assert_eq!(player.progress(), Some(0.5));
    }

    #[test]
    fn seeking_while_paused_jumps_and_stays_paused() {
        let mut player = playing();
        player.toggle();
        assert_eq!(
            player.seek_to_fraction(0.25),
            Some(PlayerRequest::SeekTo(105.0))
        );
        assert_eq!(player.status(), &PlayerStatus::Paused);
    }

    #[test]
    fn seeking_while_stopped_sets_where_playing_will_start() {
        let mut player = Player::default();
        player.select(Some(first()), Some(420.0));
        assert_eq!(player.seek_to_fraction(0.5), None);
        assert_eq!(player.position_seconds(), 210.0);
        assert_eq!(player.toggle(), start(first(), 210.0));
    }

    #[test]
    fn seeking_is_kept_inside_the_track_and_needs_a_known_length() {
        let mut player = playing();
        assert_eq!(
            player.seek_to_fraction(1.7),
            Some(PlayerRequest::SeekTo(420.0))
        );
        assert_eq!(
            player.seek_to_fraction(-3.0),
            Some(PlayerRequest::SeekTo(0.0))
        );

        let mut unknown_length = Player::default();
        unknown_length.select(Some(first()), None);
        assert_eq!(unknown_length.seek_to_fraction(0.5), None);
        assert_eq!(unknown_length.position_seconds(), 0.0);
    }

    #[test]
    fn the_length_is_learned_from_the_output_when_analysis_did_not_give_one() {
        let mut player = Player::default();
        player.select(Some(first()), None);
        player.toggle();
        assert!(player.started(first(), Some(200.0)));
        assert_eq!(player.duration_seconds(), Some(200.0));
        player.advance_to(100.0);
        assert_eq!(player.progress(), Some(0.5));
    }

    #[test]
    fn the_position_never_runs_past_the_end() {
        let mut player = playing();
        player.advance_to(500.0);
        assert_eq!(player.position_seconds(), 420.0);
        assert_eq!(player.progress(), Some(1.0));
    }

    #[test]
    fn a_finished_track_stops_at_its_end_and_play_starts_it_over() {
        let mut player = playing();
        player.advance_to(419.0);
        player.finished();
        assert_eq!(player.status(), &PlayerStatus::Stopped);
        assert_eq!(player.time_label(), "7:00 / 7:00");
        assert_eq!(player.toggle(), start(first(), 0.0));
    }

    #[test]
    fn a_pointer_maps_to_a_fraction_of_the_strip() {
        assert_eq!(fraction_along(150.0, 100.0, 200.0), 0.25);
        assert_eq!(fraction_along(100.0, 100.0, 200.0), 0.0);
        assert_eq!(fraction_along(300.0, 100.0, 200.0), 1.0);
        assert_eq!(fraction_along(20.0, 100.0, 200.0), 0.0);
        assert_eq!(fraction_along(900.0, 100.0, 200.0), 1.0);
        assert_eq!(fraction_along(150.0, 100.0, 0.0), 0.0);
    }
}
