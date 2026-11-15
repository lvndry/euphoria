// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The play and pause key, the headphone buttons and the Now Playing widget
//! of the system, as far as they do not depend on the system: what each
//! remote command means, what Now Playing is told, and when it is told.
//!
//! The macOS calls sit behind [`NowPlayingSurface`] and a channel of
//! [`RemoteCommand`]s, so the rest runs, and is tested, without them.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{App, Entity};

use crate::app::EuphoriaApp;

/// How often the app looks at what is playing. Now Playing is told only
/// when something changed, not at every look.
const LOOK_INTERVAL: Duration = Duration::from_millis(500);

/// How far the position may be from where Now Playing counts it, going on
/// at the rate it was told, before it is told again. Beyond this, the
/// position was moved: a seek.
const DRIFT_TOLERANCE_SECONDS: f64 = 1.0;

/// Past this many seconds into a track, Previous goes back to its start
/// instead of to the track before.
pub(crate) const RESTART_AFTER_SECONDS: f64 = 3.0;

/// A command from the keyboard, the headphones or the system's Now Playing
/// widget.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum RemoteCommand {
    Play,
    Pause,
    TogglePlayPause,
    NextTrack,
    PreviousTrack,
    /// Move to this many seconds from the beginning.
    SeekTo(f64),
}

/// What the app does for a command.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TransportAction {
    Play,
    Pause,
    NextTrack,
    PreviousTrack,
    SeekTo(f64),
}

/// The action for `command`, given whether sound is coming out. Play while
/// playing and Pause while paused change nothing.
pub(crate) fn action_for(command: RemoteCommand, is_playing: bool) -> Option<TransportAction> {
    match command {
        RemoteCommand::Play => (!is_playing).then_some(TransportAction::Play),
        RemoteCommand::Pause => is_playing.then_some(TransportAction::Pause),
        RemoteCommand::TogglePlayPause => Some(if is_playing {
            TransportAction::Pause
        } else {
            TransportAction::Play
        }),
        RemoteCommand::NextTrack => Some(TransportAction::NextTrack),
        RemoteCommand::PreviousTrack => Some(TransportAction::PreviousTrack),
        RemoteCommand::SeekTo(seconds) => Some(TransportAction::SeekTo(seconds)),
    }
}

/// Which way along a list a command moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    Forward,
    Backward,
}

/// The place next to `position` in a list of `length` rows, or `None` at
/// the end of the list in that direction. Without a position, forward
/// starts at the first row and backward at the last.
pub(crate) fn neighbour(
    position: Option<usize>,
    length: usize,
    direction: Direction,
) -> Option<usize> {
    match (position, direction) {
        (_, _) if length == 0 => None,
        (None, Direction::Forward) => Some(0),
        (None, Direction::Backward) => Some(length - 1),
        (Some(position), Direction::Forward) => (position + 1 < length).then_some(position + 1),
        (Some(position), Direction::Backward) => position.checked_sub(1),
    }
}

/// What Now Playing shows about the track that holds the transport.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NowPlaying {
    pub(crate) title: String,
    pub(crate) artist: Option<String>,
    pub(crate) duration_seconds: Option<f64>,
    pub(crate) elapsed_seconds: f64,
    /// Seconds of the track that go by in a second: 0 while paused.
    pub(crate) rate: f64,
    pub(crate) playing: bool,
}

/// Where Now Playing is told what plays.
pub(crate) trait NowPlayingSurface {
    fn publish(&mut self, info: &NowPlaying);
    fn clear(&mut self);
}

/// Tells a surface what plays, and only when it changed: the system moves
/// the position along by itself at the rate it was given.
pub(crate) struct NowPlayingPublisher {
    surface: Box<dyn NowPlayingSurface>,
    last: Option<(NowPlaying, Instant)>,
}

impl NowPlayingPublisher {
    pub(crate) fn new(surface: Box<dyn NowPlayingSurface>) -> Self {
        Self {
            surface,
            last: None,
        }
    }

    /// Brings the surface up to date with `now`, as seen at `at`.
    pub(crate) fn update(&mut self, now: Option<NowPlaying>, at: Instant) {
        let Some(info) = now else {
            self.clear();
            return;
        };
        if self
            .last
            .as_ref()
            .is_some_and(|(last, told_at)| !needs_telling(last, *told_at, &info, at))
        {
            return;
        }
        self.surface.publish(&info);
        self.last = Some((info, at));
    }

    /// Takes the track off Now Playing, when it shows one.
    pub(crate) fn clear(&mut self) {
        if self.last.take().is_some() {
            self.surface.clear();
        }
    }
}

fn needs_telling(last: &NowPlaying, told_at: Instant, info: &NowPlaying, at: Instant) -> bool {
    if last.title != info.title
        || last.artist != info.artist
        || last.duration_seconds != info.duration_seconds
        || last.rate != info.rate
        || last.playing != info.playing
    {
        return true;
    }
    let expected = last.elapsed_seconds + last.rate * at.duration_since(told_at).as_secs_f64();
    (info.elapsed_seconds - expected).abs() > DRIFT_TOLERANCE_SECONDS
}

/// Connects the system's remote commands and Now Playing to the app: the
/// commands from `commands` are carried out in the window's app on the main
/// thread, and what plays is told to `surface`, which is cleared on quitting.
pub(crate) fn install(
    view: &Entity<EuphoriaApp>,
    surface: Box<dyn NowPlayingSurface>,
    commands: async_channel::Receiver<RemoteCommand>,
    cx: &mut App,
) {
    let publisher = Rc::new(RefCell::new(NowPlayingPublisher::new(surface)));

    let quitting = Rc::clone(&publisher);
    cx.on_app_quit(move |_| {
        quitting.borrow_mut().clear();
        async {}
    })
    .detach();

    let weak = view.downgrade();
    let command_publisher = Rc::clone(&publisher);
    let command_view = weak.clone();
    cx.spawn(async move |cx| {
        while let Ok(command) = commands.recv().await {
            let seen = command_view.update(cx, |app, cx| {
                app.perform_remote(command, cx);
                app.now_playing()
            });
            match seen {
                Ok(now) => command_publisher.borrow_mut().update(now, Instant::now()),
                Err(_) => break,
            }
        }
    })
    .detach();

    cx.spawn(async move |cx| {
        loop {
            let Ok(now) = weak.read_with(cx, |app, _| app.now_playing()) else {
                break;
            };
            publisher.borrow_mut().update(now, Instant::now());
            cx.background_executor().timer(LOOK_INTERVAL).await;
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_pauses_what_plays_and_plays_what_is_paused() {
        assert_eq!(
            action_for(RemoteCommand::TogglePlayPause, true),
            Some(TransportAction::Pause)
        );
        assert_eq!(
            action_for(RemoteCommand::TogglePlayPause, false),
            Some(TransportAction::Play)
        );
    }

    #[test]
    fn play_and_pause_change_nothing_when_already_so() {
        assert_eq!(action_for(RemoteCommand::Play, true), None);
        assert_eq!(action_for(RemoteCommand::Pause, false), None);
        assert_eq!(
            action_for(RemoteCommand::Play, false),
            Some(TransportAction::Play)
        );
        assert_eq!(
            action_for(RemoteCommand::Pause, true),
            Some(TransportAction::Pause)
        );
    }

    #[test]
    fn navigation_and_seek_commands_map_one_to_one() {
        assert_eq!(
            action_for(RemoteCommand::NextTrack, false),
            Some(TransportAction::NextTrack)
        );
        assert_eq!(
            action_for(RemoteCommand::PreviousTrack, true),
            Some(TransportAction::PreviousTrack)
        );
        assert_eq!(
            action_for(RemoteCommand::SeekTo(42.5), true),
            Some(TransportAction::SeekTo(42.5))
        );
    }

    #[test]
    fn neighbours_stop_at_the_ends_of_a_list() {
        assert_eq!(neighbour(Some(1), 3, Direction::Forward), Some(2));
        assert_eq!(neighbour(Some(2), 3, Direction::Forward), None);
        assert_eq!(neighbour(Some(1), 3, Direction::Backward), Some(0));
        assert_eq!(neighbour(Some(0), 3, Direction::Backward), None);
        assert_eq!(neighbour(None, 3, Direction::Forward), Some(0));
        assert_eq!(neighbour(None, 3, Direction::Backward), Some(2));
        assert_eq!(neighbour(None, 0, Direction::Forward), None);
    }

    #[derive(Debug, PartialEq)]
    enum Told {
        Published(NowPlaying),
        Cleared,
    }

    struct Recording(Rc<RefCell<Vec<Told>>>);

    impl NowPlayingSurface for Recording {
        fn publish(&mut self, info: &NowPlaying) {
            self.0.borrow_mut().push(Told::Published(info.clone()));
        }
        fn clear(&mut self) {
            self.0.borrow_mut().push(Told::Cleared);
        }
    }

    fn publisher() -> (NowPlayingPublisher, Rc<RefCell<Vec<Told>>>) {
        let told = Rc::new(RefCell::new(Vec::new()));
        (
            NowPlayingPublisher::new(Box::new(Recording(Rc::clone(&told)))),
            told,
        )
    }

    fn playing(elapsed_seconds: f64, rate: f64) -> NowPlaying {
        NowPlaying {
            title: "Title".to_string(),
            artist: Some("Artist".to_string()),
            duration_seconds: Some(240.0),
            elapsed_seconds,
            rate,
            playing: rate > 0.0,
        }
    }

    #[test]
    fn the_title_artist_durations_and_rate_are_told() {
        let (mut publisher, told) = publisher();
        publisher.update(Some(playing(12.0, 1.0)), Instant::now());
        let told = told.borrow();
        let Told::Published(info) = &told[0] else {
            panic!("expected a publish");
        };
        assert_eq!(info.title, "Title");
        assert_eq!(info.artist.as_deref(), Some("Artist"));
        assert_eq!(info.duration_seconds, Some(240.0));
        assert_eq!(info.elapsed_seconds, 12.0);
        assert_eq!(info.rate, 1.0);
    }

    #[test]
    fn a_position_that_goes_on_at_the_rate_is_not_told_again() {
        let (mut publisher, told) = publisher();
        let start = Instant::now();
        publisher.update(Some(playing(10.0, 1.0)), start);
        for second in 1..=20u64 {
            let at = start + Duration::from_millis(second * 500);
            publisher.update(Some(playing(10.0 + second as f64 * 0.5, 1.0)), at);
        }
        assert_eq!(told.borrow().len(), 1);
    }

    #[test]
    fn a_seek_pause_resume_or_new_track_is_told() {
        let (mut publisher, told) = publisher();
        let start = Instant::now();
        publisher.update(Some(playing(10.0, 1.0)), start);
        publisher.update(
            Some(playing(120.0, 1.0)),
            start + Duration::from_millis(500),
        );
        publisher.update(Some(playing(120.0, 0.0)), start + Duration::from_secs(1));
        publisher.update(Some(playing(120.0, 1.0)), start + Duration::from_secs(2));
        let mut next = playing(0.0, 1.0);
        next.title = "Other".to_string();
        publisher.update(Some(next), start + Duration::from_secs(3));
        assert_eq!(told.borrow().len(), 5);
    }

    #[test]
    fn a_paused_position_that_stays_put_is_not_told_again() {
        let (mut publisher, told) = publisher();
        let start = Instant::now();
        publisher.update(Some(playing(30.0, 0.0)), start);
        publisher.update(Some(playing(30.0, 0.0)), start + Duration::from_secs(60));
        assert_eq!(told.borrow().len(), 1);
    }

    #[test]
    fn pausing_keeps_the_info_as_paused_and_only_nothing_held_clears() {
        let (mut publisher, told) = publisher();
        let start = Instant::now();
        publisher.update(Some(playing(30.0, 1.0)), start);
        publisher.update(Some(playing(31.0, 0.0)), start + Duration::from_secs(1));
        publisher.update(Some(playing(31.0, 0.0)), start + Duration::from_secs(30));
        let told_so_far = told.borrow();
        assert_eq!(told_so_far.len(), 2);
        let Told::Published(paused) = &told_so_far[1] else {
            panic!("pausing must publish, not clear");
        };
        assert!(!paused.playing);
        assert_eq!(paused.rate, 0.0);
        assert!(!told_so_far.contains(&Told::Cleared));
        drop(told_so_far);
        publisher.update(None, start + Duration::from_secs(31));
        assert_eq!(told.borrow().last(), Some(&Told::Cleared));
    }

    #[test]
    fn stopping_clears_once() {
        let (mut publisher, told) = publisher();
        let start = Instant::now();
        publisher.update(None, start);
        assert!(told.borrow().is_empty());
        publisher.update(Some(playing(1.0, 1.0)), start);
        publisher.update(None, start + Duration::from_secs(1));
        publisher.update(None, start + Duration::from_secs(2));
        assert_eq!(told.borrow().last(), Some(&Told::Cleared));
        assert_eq!(told.borrow().len(), 2);
    }
}
