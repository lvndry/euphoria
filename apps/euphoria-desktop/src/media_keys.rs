// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The play and pause key, the headphone buttons and the Now Playing widget
//! of the system, as far as they do not depend on the system: what each
//! remote command means, what Now Playing is told, and when it is told.
//!
//! The macOS calls sit behind [`NowPlayingSurface`] and a channel of
//! [`RemoteCommand`]s, so the rest runs, and is tested, without them.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{App, Entity};

use crate::app::EuphoriaApp;
use crate::artwork::read_now_playing_cover;

/// How often the app looks at what is playing, besides each time the window
/// changes. Now Playing is told only when something changed, not at every
/// look.
const LOOK_INTERVAL: Duration = Duration::from_millis(500);

/// How far the position may be from where Now Playing counts it, going on
/// at the rate it was told, before it is told again. Beyond this, the
/// position was moved: a seek.
const DRIFT_TOLERANCE_SECONDS: f64 = 0.5;

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

/// The cover given to Now Playing: a square PNG. Cloning shares the bytes,
/// and two covers are the same only when they share them.
#[derive(Clone)]
pub(crate) struct NowPlayingArtwork {
    png: Arc<[u8]>,
    side_pixels: u32,
}

impl NowPlayingArtwork {
    pub(crate) fn new(png: Vec<u8>, side_pixels: u32) -> Self {
        Self {
            png: png.into(),
            side_pixels,
        }
    }

    pub(crate) fn png(&self) -> &[u8] {
        &self.png
    }

    pub(crate) fn side_pixels(&self) -> u32 {
        self.side_pixels
    }
}

impl PartialEq for NowPlayingArtwork {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.png, &other.png)
    }
}

impl std::fmt::Debug for NowPlayingArtwork {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "NowPlayingArtwork({}px)", self.side_pixels)
    }
}

/// What Now Playing shows about the track that holds the transport.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NowPlaying {
    /// The audio file, whose cover Now Playing shows.
    pub(crate) path: Option<PathBuf>,
    pub(crate) title: String,
    pub(crate) artist: Option<String>,
    pub(crate) album: Option<String>,
    /// The length of the track as recorded, whatever the tempo.
    pub(crate) duration_seconds: Option<f64>,
    /// Seconds of the track as recorded from its beginning.
    pub(crate) elapsed_seconds: f64,
    /// Seconds of the track that go by in a second: the playback tempo while
    /// playing, 0 while paused.
    pub(crate) rate: f64,
    pub(crate) playing: bool,
    pub(crate) artwork: Option<NowPlayingArtwork>,
}

/// The rate Now Playing counts as normal speed: the track's own tempo.
pub(crate) const DEFAULT_PLAYBACK_RATE: f64 = 1.0;

/// A key of the Now Playing info, named after the framework's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InfoKey {
    Title,
    Artist,
    AlbumTitle,
    PlaybackDuration,
    ElapsedPlaybackTime,
    PlaybackRate,
    DefaultPlaybackRate,
    Artwork,
}

/// A value of the Now Playing info.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum InfoValue {
    Text(String),
    Number(f64),
    Artwork(NowPlayingArtwork),
}

/// The entries of the Now Playing info for `info`. Durations and positions
/// are in seconds of the track as recorded, and the rate is how many of them
/// go by in a second, so the system's count follows a changed tempo.
pub(crate) fn info_entries(info: &NowPlaying) -> Vec<(InfoKey, InfoValue)> {
    let mut entries = vec![
        (InfoKey::Title, InfoValue::Text(info.title.clone())),
        (
            InfoKey::ElapsedPlaybackTime,
            InfoValue::Number(info.elapsed_seconds.max(0.0)),
        ),
        (
            InfoKey::PlaybackRate,
            InfoValue::Number(if info.playing { info.rate } else { 0.0 }),
        ),
        (
            InfoKey::DefaultPlaybackRate,
            InfoValue::Number(DEFAULT_PLAYBACK_RATE),
        ),
    ];
    if let Some(artist) = &info.artist {
        entries.push((InfoKey::Artist, InfoValue::Text(artist.clone())));
    }
    if let Some(album) = &info.album {
        entries.push((InfoKey::AlbumTitle, InfoValue::Text(album.clone())));
    }
    if let Some(duration) = info.duration_seconds {
        entries.push((InfoKey::PlaybackDuration, InfoValue::Number(duration)));
    }
    if let Some(artwork) = &info.artwork {
        entries.push((InfoKey::Artwork, InfoValue::Artwork(artwork.clone())));
    }
    entries
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
    if last.path != info.path
        || last.title != info.title
        || last.artist != info.artist
        || last.album != info.album
        || last.duration_seconds != info.duration_seconds
        || last.rate != info.rate
        || last.playing != info.playing
        || last.artwork != info.artwork
    {
        return true;
    }
    let expected = last.elapsed_seconds + last.rate * at.duration_since(told_at).as_secs_f64();
    (info.elapsed_seconds - expected).abs() > DRIFT_TOLERANCE_SECONDS
}

/// The cover of the track Now Playing shows. It is read off the UI thread
/// when the track changes, and a cover read for a track that is no longer
/// shown is dropped.
#[derive(Default)]
pub(crate) struct ArtworkFollower {
    path: Option<PathBuf>,
    artwork: Option<NowPlayingArtwork>,
}

impl ArtworkFollower {
    /// Follows the track at `path`. Returns the file whose cover is to be
    /// read when the track changed; until it is read, there is no cover.
    pub(crate) fn follow(&mut self, path: Option<&Path>) -> Option<PathBuf> {
        if self.path.as_deref() == path {
            return None;
        }
        self.path = path.map(Path::to_path_buf);
        self.artwork = None;
        self.path.clone()
    }

    /// True while the cover of `path` is still wanted.
    pub(crate) fn wants(&self, path: &Path) -> bool {
        self.path.as_deref() == Some(path)
    }

    /// Keeps the cover read for `path`, unless the track changed meanwhile.
    /// Returns whether it was kept.
    pub(crate) fn finish(&mut self, path: &Path, artwork: Option<NowPlayingArtwork>) -> bool {
        if !self.wants(path) {
            return false;
        }
        self.artwork = artwork;
        true
    }

    pub(crate) fn artwork(&self) -> Option<NowPlayingArtwork> {
        self.artwork.clone()
    }
}

/// What is told to Now Playing: the track and its cover.
pub(crate) struct NowPlayingSync {
    publisher: NowPlayingPublisher,
    artwork: ArtworkFollower,
}

impl NowPlayingSync {
    pub(crate) fn new(surface: Box<dyn NowPlayingSurface>) -> Self {
        Self {
            publisher: NowPlayingPublisher::new(surface),
            artwork: ArtworkFollower::default(),
        }
    }

    /// Brings Now Playing up to date with `now`, as seen at `at`, with the
    /// cover read so far. Returns the file whose cover is to be read, when
    /// the track changed.
    pub(crate) fn update(&mut self, now: Option<NowPlaying>, at: Instant) -> Option<PathBuf> {
        let to_read = self
            .artwork
            .follow(now.as_ref().and_then(|info| info.path.as_deref()));
        let now = now.map(|info| NowPlaying {
            artwork: self.artwork.artwork(),
            ..info
        });
        self.publisher.update(now, at);
        to_read
    }

    /// True while the cover of `path` is still wanted.
    pub(crate) fn wants_artwork(&self, path: &Path) -> bool {
        self.artwork.wants(path)
    }

    /// Keeps the cover read for `path`. Returns whether it is still wanted,
    /// in which case Now Playing is to be told again.
    pub(crate) fn artwork_read(&mut self, path: &Path, artwork: Option<NowPlayingArtwork>) -> bool {
        self.artwork.finish(path, artwork)
    }

    pub(crate) fn clear(&mut self) {
        self.publisher.clear();
    }
}

/// Connects the system's remote commands and Now Playing to the app: the
/// commands from `commands` are carried out in the window's app on the main
/// thread. What plays is looked at each time the window changes, and told
/// to `surface` only when what Now Playing shows changed (see
/// [`NowPlayingPublisher`]), never for ordinary progress. It is cleared on
/// quitting. Covers are read on the background executor.
pub(crate) fn install(
    view: &Entity<EuphoriaApp>,
    surface: Box<dyn NowPlayingSurface>,
    commands: async_channel::Receiver<RemoteCommand>,
    cx: &mut App,
) {
    let sync = Rc::new(RefCell::new(NowPlayingSync::new(surface)));
    let (cover_requests, cover_queue) = async_channel::unbounded::<PathBuf>();
    let tell = {
        let sync = Rc::clone(&sync);
        move |now: Option<NowPlaying>| {
            if let Some(path) = sync.borrow_mut().update(now, Instant::now()) {
                let _ = cover_requests.try_send(path);
            }
        }
    };

    let quitting = Rc::clone(&sync);
    cx.on_app_quit(move |_| {
        quitting.borrow_mut().clear();
        async {}
    })
    .detach();

    let observed = tell.clone();
    cx.observe(view, move |view, cx| observed(view.read(cx).now_playing()))
        .detach();

    let weak = view.downgrade();
    let command_view = weak.clone();
    let command_tell = tell.clone();
    cx.spawn(async move |cx| {
        while let Ok(command) = commands.recv().await {
            let seen = command_view.update(cx, |app, cx| {
                app.perform_remote(command, cx);
                app.now_playing()
            });
            match seen {
                Ok(now) => command_tell(now),
                Err(_) => break,
            }
        }
    })
    .detach();

    let cover_view = weak.clone();
    let cover_sync = Rc::clone(&sync);
    let cover_tell = tell.clone();
    cx.spawn(async move |cx| {
        while let Ok(path) = cover_queue.recv().await {
            if !cover_sync.borrow().wants_artwork(&path) {
                continue;
            }
            let reading = path.clone();
            let artwork = cx
                .background_executor()
                .spawn(async move { read_now_playing_cover(&reading) })
                .await;
            if !cover_sync.borrow_mut().artwork_read(&path, artwork) {
                continue;
            }
            match cover_view.read_with(cx, |app, _| app.now_playing()) {
                Ok(now) => cover_tell(now),
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
            tell(now);
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
            path: Some(PathBuf::from("/music/title.mp3")),
            title: "Title".to_string(),
            artist: Some("Artist".to_string()),
            album: Some("Album".to_string()),
            duration_seconds: Some(240.0),
            elapsed_seconds,
            rate,
            playing: rate > 0.0,
            artwork: None,
        }
    }

    fn cover() -> NowPlayingArtwork {
        NowPlayingArtwork::new(vec![1, 2, 3], 600)
    }

    fn entry(entries: &[(InfoKey, InfoValue)], key: InfoKey) -> Option<&InfoValue> {
        entries
            .iter()
            .find(|(each, _)| *each == key)
            .map(|(_, value)| value)
    }

    fn number(entries: &[(InfoKey, InfoValue)], key: InfoKey) -> f64 {
        match entry(entries, key) {
            Some(InfoValue::Number(value)) => *value,
            other => panic!("expected a number for {key:?}, got {other:?}"),
        }
    }

    fn text(entries: &[(InfoKey, InfoValue)], key: InfoKey) -> Option<&str> {
        match entry(entries, key) {
            Some(InfoValue::Text(value)) => Some(value),
            None => None,
            other => panic!("expected text for {key:?}, got {other:?}"),
        }
    }

    #[test]
    fn the_info_names_the_track_and_counts_in_seconds_of_the_recording() {
        let entries = info_entries(&playing(42.5, 1.0));
        assert_eq!(text(&entries, InfoKey::Title), Some("Title"));
        assert_eq!(text(&entries, InfoKey::Artist), Some("Artist"));
        assert_eq!(text(&entries, InfoKey::AlbumTitle), Some("Album"));
        assert_eq!(number(&entries, InfoKey::PlaybackDuration), 240.0);
        assert_eq!(number(&entries, InfoKey::ElapsedPlaybackTime), 42.5);
        assert_eq!(number(&entries, InfoKey::PlaybackRate), 1.0);
        assert_eq!(number(&entries, InfoKey::DefaultPlaybackRate), 1.0);
    }

    #[test]
    fn the_rate_is_the_playback_tempo_and_zero_while_paused() {
        let faster = info_entries(&playing(30.0, 1.08));
        assert_eq!(number(&faster, InfoKey::PlaybackRate), 1.08);
        assert_eq!(number(&faster, InfoKey::DefaultPlaybackRate), 1.0);
        assert_eq!(
            number(&faster, InfoKey::PlaybackDuration),
            240.0,
            "the length stays that of the recording, as the rate counts it"
        );
        assert_eq!(number(&faster, InfoKey::ElapsedPlaybackTime), 30.0);

        let mut paused = playing(30.0, 0.0);
        paused.playing = false;
        let paused = info_entries(&paused);
        assert_eq!(number(&paused, InfoKey::PlaybackRate), 0.0);
        assert_eq!(number(&paused, InfoKey::DefaultPlaybackRate), 1.0);

        let mut held = playing(30.0, 1.08);
        held.playing = false;
        assert_eq!(
            number(&info_entries(&held), InfoKey::PlaybackRate),
            0.0,
            "a paused track never counts on"
        );
    }

    #[test]
    fn missing_tags_length_and_cover_are_left_out() {
        let mut bare = playing(0.0, 1.0);
        bare.artist = None;
        bare.album = None;
        bare.duration_seconds = None;
        let entries = info_entries(&bare);
        let keys: Vec<InfoKey> = entries.iter().map(|(key, _)| *key).collect();
        assert_eq!(
            keys,
            [
                InfoKey::Title,
                InfoKey::ElapsedPlaybackTime,
                InfoKey::PlaybackRate,
                InfoKey::DefaultPlaybackRate,
            ]
        );
    }

    #[test]
    fn the_cover_is_in_the_info_when_there_is_one() {
        let mut with_cover = playing(0.0, 1.0);
        with_cover.artwork = Some(cover());
        let entries = info_entries(&with_cover);
        let Some(InfoValue::Artwork(artwork)) = entry(&entries, InfoKey::Artwork) else {
            panic!("expected the cover");
        };
        assert_eq!(artwork.side_pixels(), 600);
        assert_eq!(artwork.png(), &[1, 2, 3]);
        assert!(entry(&info_entries(&playing(0.0, 1.0)), InfoKey::Artwork).is_none());
    }

    #[test]
    fn covers_are_the_same_only_when_they_share_their_bytes() {
        let first = cover();
        assert_eq!(first, first.clone());
        assert_ne!(first, cover());
    }

    #[test]
    fn a_tempo_change_is_told_with_the_position_at_that_moment() {
        let (mut publisher, told) = publisher();
        let start = Instant::now();
        publisher.update(Some(playing(10.0, 1.0)), start);
        publisher.update(Some(playing(10.4, 1.0)), start + Duration::from_millis(400));
        publisher.update(
            Some(playing(10.5, 1.06)),
            start + Duration::from_millis(500),
        );
        let told = told.borrow();
        assert_eq!(told.len(), 2);
        let Told::Published(info) = &told[1] else {
            panic!("expected a publish");
        };
        assert_eq!(info.rate, 1.06);
        assert_eq!(info.elapsed_seconds, 10.5);
    }

    #[test]
    fn a_faster_tempo_counted_at_its_rate_is_not_told_again() {
        let (mut publisher, told) = publisher();
        let start = Instant::now();
        publisher.update(Some(playing(10.0, 1.1)), start);
        for step in 1..=20u64 {
            let at = start + Duration::from_millis(step * 500);
            publisher.update(Some(playing(10.0 + step as f64 * 0.55, 1.1)), at);
        }
        assert_eq!(told.borrow().len(), 1);
    }

    #[test]
    fn a_short_seek_is_told() {
        let (mut publisher, told) = publisher();
        let start = Instant::now();
        publisher.update(Some(playing(10.0, 1.0)), start);
        publisher.update(Some(playing(9.4, 1.0)), start + Duration::from_millis(100));
        assert_eq!(told.borrow().len(), 2);
    }

    #[test]
    fn a_cover_read_for_a_track_no_longer_shown_is_dropped() {
        let mut follower = ArtworkFollower::default();
        let first = Path::new("/music/first.mp3");
        let second = Path::new("/music/second.mp3");
        assert_eq!(follower.follow(Some(first)), Some(first.to_path_buf()));
        assert_eq!(follower.follow(Some(first)), None, "read once per track");
        assert_eq!(follower.follow(Some(second)), Some(second.to_path_buf()));
        assert!(!follower.finish(first, Some(cover())));
        assert!(follower.artwork().is_none());
        assert!(follower.finish(second, Some(cover())));
        assert!(follower.artwork().is_some());
        assert_eq!(follower.follow(None), None);
        assert!(follower.artwork().is_none());
    }

    fn sync() -> (NowPlayingSync, Rc<RefCell<Vec<Told>>>) {
        let told = Rc::new(RefCell::new(Vec::new()));
        (
            NowPlayingSync::new(Box::new(Recording(Rc::clone(&told)))),
            told,
        )
    }

    fn last_published(told: &Rc<RefCell<Vec<Told>>>) -> NowPlaying {
        match told.borrow().last() {
            Some(Told::Published(info)) => info.clone(),
            other => panic!("expected a publish, got {other:?}"),
        }
    }

    #[test]
    fn the_cover_follows_the_track_and_a_new_track_starts_without_the_old_cover() {
        let (mut sync, told) = sync();
        let start = Instant::now();
        let first = playing(0.0, 1.0);
        let first_path = first.path.clone().expect("a path");
        assert_eq!(
            sync.update(Some(first.clone()), start),
            Some(first_path.clone())
        );
        assert!(last_published(&told).artwork.is_none());

        assert!(sync.artwork_read(&first_path, Some(cover())));
        assert_eq!(sync.update(Some(first.clone()), start), None);
        assert!(last_published(&told).artwork.is_some());
        assert_eq!(told.borrow().len(), 2);

        let mut second = playing(0.0, 1.0);
        second.path = Some(PathBuf::from("/music/second.mp3"));
        second.title = "Second".to_string();
        let second_path = second.path.clone().expect("a path");
        assert_eq!(
            sync.update(Some(second), start + Duration::from_secs(1)),
            Some(second_path.clone())
        );
        let shown = last_published(&told);
        assert_eq!(shown.title, "Second");
        assert!(shown.artwork.is_none(), "the old cover is not kept");
        assert!(!sync.artwork_read(&first_path, Some(cover())));
        assert!(sync.wants_artwork(&second_path));
    }

    /// What a redraw at `frame` of a 120 Hz window sees while the track
    /// plays on steadily at `rate` from `start_seconds`.
    fn redraw(start_seconds: f64, rate: f64, frame: u64) -> (NowPlaying, Duration) {
        let since_start = Duration::from_micros(frame * 8_333);
        let elapsed = start_seconds + rate * since_start.as_secs_f64();
        (playing(elapsed, rate), since_start)
    }

    #[test]
    fn redraws_during_steady_playback_tell_nothing_and_a_change_tells_once() {
        let (mut sync, told) = sync();
        let start = Instant::now();
        let path = playing(0.0, 1.0).path.expect("a path");
        sync.update(Some(playing(10.0, 1.0)), start);
        sync.artwork_read(&path, Some(cover()));
        sync.update(Some(playing(10.0, 1.0)), start);
        let after_setup = told.borrow().len();

        for frame in 1..=100 {
            let (info, since_start) = redraw(10.0, 1.0, frame);
            sync.update(Some(info), start + since_start);
        }
        assert_eq!(
            told.borrow().len(),
            after_setup,
            "steady playback pushes nothing"
        );

        let (at_change, since_start) = redraw(10.0, 1.0, 101);
        let changed_at = start + since_start;
        let faster = NowPlaying {
            rate: 1.08,
            ..at_change.clone()
        };
        sync.update(Some(faster.clone()), changed_at);
        for frame in 1..=100 {
            let (info, since_change) = redraw(faster.elapsed_seconds, 1.08, frame);
            sync.update(Some(info), changed_at + since_change);
        }
        assert_eq!(
            told.borrow().len(),
            after_setup + 1,
            "a rate change pushes once"
        );

        let (before_seek, since_change) = redraw(faster.elapsed_seconds, 1.08, 101);
        let seeked_at = changed_at + since_change;
        let seeked = before_seek.elapsed_seconds + 30.0;
        for frame in 0..=100 {
            let (info, since_seek) = redraw(seeked, 1.08, frame);
            sync.update(Some(info), seeked_at + since_seek);
        }
        assert_eq!(told.borrow().len(), after_setup + 2, "a seek pushes once");
        let last = last_published(&told);
        assert_eq!(last.rate, 1.08);
        assert!(last.artwork.is_some());
    }

    #[test]
    fn a_track_without_a_cover_is_told_once_its_absence_is_known() {
        let (mut sync, told) = sync();
        let start = Instant::now();
        let first = playing(0.0, 1.0);
        let path = first.path.clone().expect("a path");
        sync.update(Some(first.clone()), start);
        assert!(sync.artwork_read(&path, None));
        sync.update(Some(first), start);
        assert_eq!(told.borrow().len(), 1, "nothing changed to tell");
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
