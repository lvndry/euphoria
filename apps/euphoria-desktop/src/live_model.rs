// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What Live knows: whether it listens, what it heard last, and the tracks
//! of the library to play after it. The window and the menu bar item both
//! show this and neither owns it.
//!
//! The library is read from the analysis cache the first time the list is
//! shown, and never analyzes anything, so Live alone stays light: no views,
//! no waveforms, no analysis workers.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use euphoria_core::fingerprint::{FingerprintIndex, Match};
use euphoria_core::harmony::TrackFeatures;
use euphoria_core::live::{
    AudioCapture, CaptureError, ListenSource, LiveEngine, LiveReading,
    LiveSettings as AnalysisSettings, MatcherHandle,
};
use gpui::{AppContext as _, Context};

use crate::favorites::Favorites;
use crate::library::Library;
use crate::live_settings::{LiveSettings, live_settings_path};
use crate::next_rail::{EnergyChoice, NextQuery, NextRow, rank_after};
use crate::settings::Settings;
use crate::tag_settings::NotationChoice;
use crate::theme::AppTheme;
use crate::track_filters::TrackFilters;

/// How many tracks the list shows.
pub(crate) const NEXT_LIMIT: usize = 10;

/// How often the model looks at what the engine heard, which sets how
/// smoothly the level meter moves.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// How long only silence must have arrived before the panel says macOS may
/// not allow the capture.
pub(crate) const SILENCE_NOTICE_SECONDS: f32 = 10.0;

/// How many sizes the listening dot takes between quiet and loud.
const LEVEL_STEPS: f32 = 10.0;

pub(crate) type CaptureFactory = Box<dyn Fn() -> Box<dyn AudioCapture>>;
pub(crate) type LibraryLoader = Box<dyn Fn() -> Library + Send + Sync>;
/// Makes the fingerprint index of the files given, telling how many are done
/// and stopping early when told to.
pub(crate) type FingerprintLoader =
    Box<dyn Fn(&[PathBuf], &AtomicUsize, &AtomicBool) -> FingerprintIndex + Send + Sync>;

/// Where the pass that learns the library's fingerprints is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Learning {
    NotStarted,
    Working { total: usize },
    Ready { tracks: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ListenState {
    /// Not started yet, or paused by the user.
    Paused,
    Listening,
    /// Listening could not start, with the reason to show.
    Unavailable(String),
}

/// What the ranking of the list was last made from, so it is made again only
/// when the sound changed.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RankedFor {
    key: Option<euphoria_core::Key>,
    tempo_tenths: Option<i32>,
    energy: Option<u8>,
    choice: EnergyChoice,
    matched: Option<usize>,
}

/// How long a key and tempo must stay the same before the panel calls the
/// reading steady.
const STEADY_AFTER: Duration = Duration::from_secs(6);

/// How many earlier readings the panel keeps.
const HISTORY_LIMIT: usize = 8;

/// Whether what is heard has stopped changing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Steadiness {
    Settling,
    Steady,
}

/// A reading that held, kept for the Earlier row. In memory only.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Earlier {
    pub(crate) key: euphoria_core::Key,
    pub(crate) bpm: Option<f32>,
    pub(crate) title: Option<String>,
}

/// Where the listening dot sits between quiet (0) and loud (1). It is an
/// entity of its own so that the level, which changes with every poll,
/// redraws the dot and nothing else.
pub(crate) struct LiveLevel {
    pub(crate) position: f32,
    moved_at: Option<Instant>,
    holds_still: bool,
}

impl LiveLevel {
    fn new() -> Self {
        Self {
            position: 0.0,
            moved_at: None,
            holds_still: crate::motion::system_reduces_motion(),
        }
    }

    /// Lets the dot move as it does when the system does not reduce motion.
    #[cfg(test)]
    pub(crate) fn allow_motion(&mut self) {
        self.holds_still = false;
    }

    /// Moves the dot towards `target` in whole steps, at most once per
    /// [`crate::motion::FAST`], and not at all under Reduce motion. True when
    /// it moved.
    pub(crate) fn follow(&mut self, target: f32, now: Instant) -> bool {
        let stepped = if self.holds_still {
            0.0
        } else {
            (target.clamp(0.0, 1.0) * LEVEL_STEPS).round() / LEVEL_STEPS
        };
        if stepped == self.position {
            return false;
        }
        let resting = stepped == 0.0;
        let too_soon = self
            .moved_at
            .is_some_and(|moved| now.saturating_duration_since(moved) < crate::motion::FAST);
        if too_soon && !resting {
            return false;
        }
        self.position = stepped;
        self.moved_at = Some(now);
        true
    }
}

/// What the panel shows of the model, besides the level dot, so that a poll
/// that changed only the level or the running counts of the engine redraws
/// nothing else.
#[derive(Debug, Clone, PartialEq)]
struct Shown {
    reading: LiveReading,
    silent_too_long: bool,
    match_position: Option<u32>,
    steadiness: Option<Steadiness>,
    listening_seconds: Option<u64>,
}

pub(crate) struct LiveModel {
    capture_factory: CaptureFactory,
    engine: Option<LiveEngine>,
    analysis: AnalysisSettings,
    engine_interval: Duration,
    pub(crate) source: ListenSource,
    pub(crate) sources: Vec<ListenSource>,
    pub(crate) state: ListenState,
    pub(crate) reading: LiveReading,
    pub(crate) level: gpui::Entity<LiveLevel>,
    /// Whether Euphoria opens at login, read from macOS when the panel opens
    /// and when it is changed, because asking is slow.
    pub(crate) opens_at_login: bool,
    pub(crate) library: Option<Library>,
    library_requested: bool,
    library_generation: u64,
    library_folders: Vec<PathBuf>,
    library_loader: Arc<LibraryLoader>,
    matcher: MatcherHandle,
    fingerprint_loader: Arc<FingerprintLoader>,
    fingerprint_paths: Arc<Vec<PathBuf>>,
    pub(crate) learning: Learning,
    learned: Arc<AtomicUsize>,
    stop_learning: Arc<AtomicBool>,
    pub(crate) next_rows: Vec<NextRow>,
    ranked_for: Option<RankedFor>,
    pub(crate) energy: EnergyChoice,
    started_this_session: bool,
    listening_since: Option<Instant>,
    signature: Option<(euphoria_core::Key, i32)>,
    signature_since: Instant,
    pub(crate) earlier: Vec<Earlier>,
    artwork: crate::artwork::ArtworkStore,
    pub(crate) notice: Option<String>,
    pub(crate) theme: AppTheme,
    pub(crate) notation: NotationChoice,
    track_filters: TrackFilters,
    suggestion_filter: crate::decks::SuggestionFilter,
    pub(crate) settings: LiveSettings,
    settings_file: Option<PathBuf>,
}

impl LiveModel {
    pub(crate) fn new(capture_factory: CaptureFactory, cx: &mut Context<'_, Self>) -> Self {
        Self::with_parts(
            capture_factory,
            Box::new(load_library_from_cache),
            Box::new(learn_fingerprints_from_cache),
            Settings::load_or_default(),
            live_settings_path(),
            euphoria_core::live::ANALYSIS_INTERVAL,
            cx,
        )
    }

    pub(crate) fn with_parts(
        capture_factory: CaptureFactory,
        library_loader: LibraryLoader,
        fingerprint_loader: FingerprintLoader,
        settings: Settings,
        settings_file: Option<PathBuf>,
        engine_interval: Duration,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let live_settings = settings_file
            .as_deref()
            .map(LiveSettings::load)
            .unwrap_or_default();
        let model = Self {
            capture_factory,
            engine: None,
            analysis: AnalysisSettings::default(),
            engine_interval,
            source: ListenSource::AllAudio,
            sources: Vec::new(),
            state: ListenState::Paused,
            reading: LiveReading::listening(),
            level: cx.new(|_| LiveLevel::new()),
            opens_at_login: false,
            library: None,
            library_requested: false,
            library_generation: 0,
            library_folders: settings.folders.clone(),
            library_loader: Arc::new(library_loader),
            matcher: MatcherHandle::default(),
            fingerprint_loader: Arc::new(fingerprint_loader),
            fingerprint_paths: Arc::new(Vec::new()),
            learning: Learning::NotStarted,
            learned: Arc::new(AtomicUsize::new(0)),
            stop_learning: Arc::new(AtomicBool::new(false)),
            next_rows: Vec::new(),
            ranked_for: None,
            energy: live_settings.energy,
            started_this_session: false,
            listening_since: None,
            signature: None,
            signature_since: Instant::now(),
            earlier: Vec::new(),
            artwork: crate::artwork::ArtworkStore::new(8),
            notice: None,
            theme: AppTheme::named(settings.theme),
            notation: settings.tags.key_notation,
            track_filters: settings.track_filters,
            suggestion_filter: settings.suggestion_filter,
            settings: live_settings,
            settings_file,
        };
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(POLL_INTERVAL).await;
                if this.update(cx, |model, cx| model.poll(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        model
    }

    /// True until listening was started once in this run of the app, so
    /// that opening the panel starts it and reopening it after a pause does not.
    pub(crate) fn needs_first_start(&self) -> bool {
        !self.started_this_session
    }

    pub(crate) fn is_listening(&self) -> bool {
        self.state == ListenState::Listening
    }

    /// The title of the menu bar item: the key heard, or what Live is doing.
    pub(crate) fn menu_bar_title(&self) -> String {
        match &self.state {
            ListenState::Listening => self
                .reading
                .key
                .filter(|_| !self.reading.listening)
                .map_or_else(|| "Live".to_string(), |live| self.notation.spell(&live.key)),
            ListenState::Paused => "Live paused".to_string(),
            ListenState::Unavailable(_) => "Live off".to_string(),
        }
    }

    /// Starts listening to the chosen source, which is what opening Live
    /// does. The first start also marks Live as used.
    pub(crate) fn start(&mut self, cx: &mut Context<'_, Self>) {
        self.started_this_session = true;
        self.stop_engine();
        self.reading = LiveReading::listening();
        let capture = (self.capture_factory)();
        match LiveEngine::start(
            capture,
            self.source.clone(),
            self.analysis,
            self.engine_interval,
            Arc::clone(&self.matcher),
        ) {
            Ok(engine) => {
                self.engine = Some(engine);
                self.state = ListenState::Listening;
                self.listening_since = Some(Instant::now());
                self.signature = None;
                if !self.settings.started_once {
                    self.settings.started_once = true;
                    self.save_settings();
                }
            }
            Err(error) => self.state = ListenState::Unavailable(describe(&error)),
        }
        self.clear_rows();
        self.rest_level(cx);
        cx.notify();
    }

    /// Stops listening and forgets the audio.
    pub(crate) fn pause(&mut self, cx: &mut Context<'_, Self>) {
        self.stop_engine();
        self.listening_since = None;
        self.state = ListenState::Paused;
        self.reading = LiveReading::listening();
        self.clear_rows();
        self.rest_level(cx);
        cx.notify();
    }

    fn rest_level(&mut self, cx: &mut Context<'_, Self>) {
        let now = Instant::now();
        self.level.update(cx, |level, cx| {
            if level.follow(0.0, now) {
                cx.notify();
            }
        });
    }

    pub(crate) fn toggle_listening(&mut self, cx: &mut Context<'_, Self>) {
        if self.is_listening() {
            self.pause(cx);
        } else {
            self.start(cx);
        }
    }

    pub(crate) fn choose_source(&mut self, source: ListenSource, cx: &mut Context<'_, Self>) {
        self.source = source;
        if self.is_listening() {
            self.start(cx);
        } else {
            cx.notify();
        }
    }

    /// Looks again for the applications that can be listened to alone.
    pub(crate) fn refresh_sources(&mut self, cx: &mut Context<'_, Self>) {
        self.sources = (self.capture_factory)().sources();
        cx.notify();
    }

    pub(crate) fn set_energy(&mut self, choice: EnergyChoice, cx: &mut Context<'_, Self>) {
        if self.energy != choice {
            self.energy = choice;
            self.settings.energy = choice;
            self.save_settings();
            self.rank(cx);
            cx.notify();
        }
    }

    /// Opens Euphoria at login with Live in the menu bar, or stops.
    pub(crate) fn toggle_open_at_login(&mut self, cx: &mut Context<'_, Self>) {
        let wanted = !crate::login_item::is_enabled();
        match crate::login_item::set_enabled(wanted) {
            Ok(()) => {
                self.opens_at_login = wanted;
                self.settings.open_at_login = wanted;
                self.save_settings();
                self.notice = None;
            }
            Err(problem) => self.notice = Some(problem),
        }
        cx.notify();
    }

    pub(crate) fn set_keep_running(&mut self, keep: bool, cx: &mut Context<'_, Self>) {
        self.settings.keep_running_when_closed = keep;
        self.save_settings();
        cx.notify();
    }

    /// Reads the app's settings again, for the filters and the theme, and
    /// forgets the library so it is read again when the list is shown.
    pub(crate) fn reload_inputs(&mut self, settings: Settings, cx: &mut Context<'_, Self>) {
        self.opens_at_login = crate::login_item::is_enabled();
        self.theme = AppTheme::named(settings.theme);
        self.notation = settings.tags.key_notation;
        self.track_filters = settings.track_filters;
        self.suggestion_filter = settings.suggestion_filter;
        if self.library_folders != settings.folders {
            self.library_folders = settings.folders;
            self.stop_learning.store(true, Ordering::Relaxed);
            self.stop_learning = Arc::new(AtomicBool::new(false));
            self.fingerprint_paths = Arc::new(Vec::new());
            if let Ok(mut handle) = self.matcher.write() {
                *handle = None;
            }
            self.learning = Learning::NotStarted;
        }
        self.library_generation = self.library_generation.wrapping_add(1);
        self.library = None;
        self.library_requested = false;
        self.ranked_for = None;
        cx.notify();
    }

    /// Reads the library from the cache in the background, once.
    pub(crate) fn ensure_library(&mut self, cx: &mut Context<'_, Self>) {
        if self.library.is_some() || self.library_requested {
            return;
        }
        self.library_requested = true;
        let generation = self.library_generation;
        let loader = Arc::clone(&self.library_loader);
        cx.spawn(async move |this, cx| {
            let library = cx
                .background_executor()
                .spawn(async move { loader() })
                .await;
            this.update(cx, |model, cx| {
                if model.library_generation != generation {
                    return;
                }
                model.library = Some(library);
                model.ranked_for = None;
                model.start_learning(cx);
                model.rank(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Learns the fingerprints of the library's tracks in the background,
    /// one at a time and at a third of a core, and then lets the engine
    /// recognise what it hears. Tracks the cache holds no analysis for are
    /// left out, as the list leaves them.
    fn start_learning(&mut self, cx: &mut Context<'_, Self>) {
        let Some(library) = &self.library else {
            return;
        };
        let paths: Vec<PathBuf> = library
            .tracks()
            .iter()
            .filter(|track| track.analysis().is_some() && !track.is_missing())
            .map(|track| track.path.clone())
            .collect();
        if self.fingerprint_paths.as_ref() == &paths && self.learning != Learning::NotStarted {
            return;
        }
        self.stop_learning.store(true, Ordering::Relaxed);
        self.stop_learning = Arc::new(AtomicBool::new(false));
        if let Ok(mut handle) = self.matcher.write() {
            *handle = None;
        }
        self.learning = Learning::NotStarted;
        self.fingerprint_paths = Arc::new(paths);
        self.learned = Arc::new(AtomicUsize::new(0));
        if self.fingerprint_paths.is_empty() {
            return;
        }
        self.learning = Learning::Working {
            total: self.fingerprint_paths.len(),
        };
        let loader = Arc::clone(&self.fingerprint_loader);
        let paths = Arc::clone(&self.fingerprint_paths);
        let done = Arc::clone(&self.learned);
        let stop = Arc::clone(&self.stop_learning);
        cx.spawn(async move |this, cx| {
            let loaded_paths = Arc::clone(&paths);
            let index = cx
                .background_executor()
                .spawn(async move { loader(&loaded_paths, &done, &stop) })
                .await;
            this.update(cx, |model, cx| {
                if !Arc::ptr_eq(&model.fingerprint_paths, &paths) {
                    return;
                }
                if let Ok(mut handle) = model.matcher.write() {
                    *handle = Some(Arc::new(index));
                }
                model.learning = Learning::Ready {
                    tracks: paths.len(),
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// How many tracks have been learned so far.
    pub(crate) fn learned_count(&self) -> usize {
        self.learned.load(Ordering::Relaxed)
    }

    /// The library track the audio was recognised as, with how it matched.
    pub(crate) fn matched_track(&self) -> Option<(&crate::library::Track, Match)> {
        let found = self.reading.matched?;
        let path = self.fingerprint_paths.get(found.track)?;
        Some((self.library.as_ref()?.track_by_path(path)?, found))
    }

    pub(crate) fn poll(&mut self, cx: &mut Context<'_, Self>) {
        let Some(engine) = &self.engine else {
            return;
        };
        let reading = engine.latest();
        self.take_reading(reading, cx);
    }

    /// Keeps `reading`, moves the level dot, and redraws the rest of the
    /// panel only when something it shows changed.
    fn take_reading(&mut self, reading: LiveReading, cx: &mut Context<'_, Self>) {
        let before = self.shown();
        if reading != self.reading {
            self.reading = reading;
            self.note_signature();
            self.rank(cx);
        }
        let target = self
            .reading
            .level
            .filter(|_| self.is_listening())
            .map_or(0.0, |level| level.meter_position());
        let now = Instant::now();
        self.level.update(cx, |level, cx| {
            if level.follow(target, now) {
                cx.notify();
            }
        });
        if self.shown() != before {
            cx.notify();
        }
    }

    fn shown(&self) -> Shown {
        Shown {
            reading: LiveReading {
                level: None,
                heard_seconds: 0.0,
                no_signal_seconds: 0.0,
                ..self.reading
            },
            silent_too_long: self.hears_only_silence(),
            match_position: self
                .match_position_seconds()
                .map(|seconds| seconds.max(0.0) as u32),
            steadiness: self.steadiness(),
            listening_seconds: self.listening_for().map(|elapsed| elapsed.as_secs()),
        }
    }

    /// True when only silence has arrived for a while, which is what a capture
    /// that macOS has not allowed delivers.
    pub(crate) fn hears_only_silence(&self) -> bool {
        self.is_listening() && self.reading.no_signal_seconds >= SILENCE_NOTICE_SECONDS
    }

    /// Where in the recognised track the audio heard now is, in seconds.
    pub(crate) fn match_position_seconds(&self) -> Option<f32> {
        let found = self.reading.matched?;
        let heard = self.reading.heard_seconds.min(15.0);
        Some(found.start_seconds + heard * found.playback.tempo_ratio)
    }

    /// Tracks how long the key and the tempo have stayed the same, and keeps
    /// a reading that held for the Earlier row.
    fn note_signature(&mut self) {
        let signature = self
            .reading
            .key
            .filter(|_| !self.reading.listening)
            .map(|key| {
                let bpm = self
                    .reading
                    .tempo
                    .map_or(0, |tempo| tempo.bpm.round() as i32);
                (key.key, bpm)
            });
        if signature != self.signature {
            self.signature = signature;
            self.signature_since = Instant::now();
        }
        self.keep_earlier_if_steady();
    }

    fn keep_earlier_if_steady(&mut self) {
        if self.steadiness() != Some(Steadiness::Steady) {
            return;
        }
        let Some((key, _)) = self.signature else {
            return;
        };
        let title = self.matched_track().map(|(track, _)| shown_title(track));
        let entry = Earlier {
            key,
            bpm: self.reading.tempo.map(|tempo| tempo.bpm),
            title,
        };
        let same_reading = |last: &Earlier| {
            last.key == entry.key && last.bpm.map(f32::round) == entry.bpm.map(f32::round)
        };
        if let Some(last) = self.earlier.last_mut()
            && same_reading(last)
            && last.title.is_none()
            && entry.title.is_some()
        {
            last.title = entry.title.clone();
        }
        let same_as_last = self
            .earlier
            .last()
            .is_some_and(|last| same_reading(last) && last.title == entry.title);
        if !same_as_last {
            self.earlier.push(entry);
            if self.earlier.len() > HISTORY_LIMIT + 1 {
                self.earlier.remove(0);
            }
        }
    }

    /// Whether the reading has held for a few seconds, or `None` when
    /// nothing is shown.
    pub(crate) fn steadiness(&self) -> Option<Steadiness> {
        self.signature?;
        Some(if self.signature_since.elapsed() >= STEADY_AFTER {
            Steadiness::Steady
        } else {
            Steadiness::Settling
        })
    }

    /// How long Live has been listening since it was started or resumed.
    pub(crate) fn listening_for(&self) -> Option<Duration> {
        self.listening_since.map(|since| since.elapsed())
    }

    /// The readings before the current one, oldest first, at most
    /// [`HISTORY_LIMIT`]. The current reading is left out.
    pub(crate) fn earlier_readings(&self) -> &[Earlier] {
        let held = self.earlier.len();
        let current_is_last = self
            .signature
            .is_some_and(|(key, _)| self.earlier.last().is_some_and(|last| last.key == key));
        let end = if current_is_last { held - 1 } else { held };
        &self.earlier[end.saturating_sub(HISTORY_LIMIT)..end]
    }

    pub(crate) fn artwork(&self) -> &crate::artwork::ArtworkStore {
        &self.artwork
    }

    /// The words above the Next list: what it follows.
    pub(crate) fn next_heading(&self) -> String {
        if let Some((track, _)) = self.matched_track() {
            return format!("Next after {}", shown_title(track));
        }
        match (self.reading.key, self.reading.tempo) {
            (Some(key), Some(tempo)) => format!(
                "Next for {} \u{b7} {:.0} BPM",
                self.notation.spell(&key.key),
                tempo.bpm
            ),
            (Some(key), None) => format!("Next for {}", self.notation.spell(&key.key)),
            _ => "Next".to_string(),
        }
    }

    /// Makes the sound of a test steady without waiting.
    #[cfg(test)]
    pub(crate) fn backdate_signature(&mut self, by: Duration) {
        self.signature_since = Instant::now() - by;
    }

    /// Replaces what was heard, for a test of the list that needs an energy
    /// the generated audio does not have.
    #[cfg(test)]
    pub(crate) fn pretend_heard(&mut self, reading: LiveReading, cx: &mut Context<'_, Self>) {
        self.reading = reading;
        self.note_signature();
        self.rank(cx);
        cx.notify();
    }

    /// Takes `reading` as a poll would, redrawing only what it changed.
    #[cfg(test)]
    pub(crate) fn pretend_polled(&mut self, reading: LiveReading, cx: &mut Context<'_, Self>) {
        self.take_reading(reading, cx);
    }

    /// Pretends the audio was recognised as the library track at `index`.
    #[cfg(test)]
    pub(crate) fn pretend_match(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        let paths: Vec<PathBuf> = self
            .library
            .as_ref()
            .map(|library| {
                library
                    .tracks()
                    .iter()
                    .map(|track| track.path.clone())
                    .collect()
            })
            .unwrap_or_default();
        self.fingerprint_paths = Arc::new(paths);
        let mut reading = self.reading;
        reading.matched = Some(Match {
            track: index,
            start_seconds: 20.0,
            votes: 50,
            runner_up_votes: 2,
            share: 0.3,
            playback: euphoria_core::fingerprint::Playback {
                tempo_ratio: 1.0,
                pitch_follows_tempo: false,
            },
        });
        self.pretend_heard(reading, cx);
    }

    /// The reference the list ranks from: the key and tempo heard.
    fn reference(&self) -> Option<TrackFeatures> {
        if self.reading.listening {
            return None;
        }
        let key = self.reading.key?.key;
        Some(TrackFeatures {
            key: Some(key),
            bpm: self.reading.tempo.map(|tempo| tempo.bpm),
            energy: self.reading.energy.map(f32::from),
        })
    }

    fn rank(&mut self, cx: &mut Context<'_, Self>) {
        let matched_id = self
            .matched_track()
            .map(|(_, found)| found.track)
            .filter(|_| !self.reading.listening);
        let live = self.reference();
        if matched_id.is_none() && live.is_none() {
            self.clear_rows();
            return;
        }
        let ranked_for = RankedFor {
            key: live.and_then(|reference| reference.key),
            tempo_tenths: live
                .and_then(|reference| reference.bpm)
                .map(|bpm| (bpm * 10.0).round() as i32),
            energy: self.reading.energy,
            choice: self.energy,
            matched: matched_id,
        };
        if self.ranked_for == Some(ranked_for) {
            return;
        }
        let Some(library) = &self.library else {
            self.ensure_library(cx);
            return;
        };
        self.ranked_for = Some(ranked_for);
        let pool: Vec<usize> = (0..library.tracks().len()).collect();
        let favorites = Favorites::default();
        let query = NextQuery {
            library,
            favorites: &favorites,
            playing_index: 0,
            energy: self.energy,
            track_filters: &self.track_filters,
            suggestion_filter: &self.suggestion_filter,
            pool: &pool,
            played: &[],
            limit: NEXT_LIMIT,
        };
        let matched = self.matched_track().map(|(track, _)| track);
        self.next_rows = match (matched, live) {
            (Some(track), _) => rank_after(track, Some(&track.path), |_| false, &query),
            (None, Some(reference)) => rank_after(&reference, None, |_| false, &query),
            (None, None) => Vec::new(),
        };
        self.read_names_of_shown_tracks();
    }

    /// Reads the title and artist from the tags of the few tracks the panel
    /// shows, once each, so it can name them as the library does.
    fn read_names_of_shown_tracks(&mut self) {
        let Some(library) = &mut self.library else {
            return;
        };
        let mut paths: Vec<PathBuf> = self
            .next_rows
            .iter()
            .filter_map(|row| {
                library
                    .track(row.track_index)
                    .map(|track| track.path.clone())
            })
            .collect();
        if let Some(found) = self.reading.matched
            && let Some(path) = self.fingerprint_paths.get(found.track)
        {
            paths.push(path.clone());
        }
        for path in paths {
            if library
                .track_by_path(&path)
                .is_some_and(|track| track.title() != track.file_name)
            {
                continue;
            }
            if let Ok(name) = euphoria_core::read_track_name(&path) {
                library.set_names(&path, name);
            }
        }
    }

    fn clear_rows(&mut self) {
        self.next_rows.clear();
        self.ranked_for = None;
    }

    fn stop_engine(&mut self) {
        if let Some(mut engine) = self.engine.take() {
            engine.stop();
        }
    }

    fn save_settings(&self) {
        if let Some(path) = &self.settings_file {
            let _ = self.settings.save(path);
        }
    }
}

impl Drop for LiveModel {
    fn drop(&mut self) {
        self.stop_engine();
    }
}

fn describe(error: &CaptureError) -> String {
    error.to_string()
}

/// The library as the analysis cache holds it, for the folders the app
/// knows. Nothing is analyzed: a track the cache has no analysis for is
/// left out.
pub(crate) fn load_library_from_cache() -> Library {
    if let Some(count) = crate::demo::requested_track_count() {
        let mut library = Library::default();
        let tracks = crate::demo::tracks(count);
        library.add_files(tracks.iter().map(|track| track.path.clone()).collect());
        for track in tracks {
            library.record_outcome(
                &track.path,
                track.outcome.map_err(crate::library::TrackFailure::Failed),
            );
        }
        return library;
    }
    let settings = Settings::load_or_default();
    let cache = euphoria_core::default_cache_path()
        .ok()
        .and_then(|path| euphoria_core::Cache::open(&path).ok())
        .and_then(|cache| euphoria_core::CachedAnalyzer::new(cache).ok());
    let mut scan = crate::state::scan(&settings.folders, cache.as_ref());
    let mut library = Library::default();
    for folder in std::mem::take(&mut scan.folders) {
        library.add_folder(folder);
    }
    let files = std::mem::take(&mut scan.files);
    for file in library.add_files(files) {
        if let Some(analysis) = scan.cached.remove(&file) {
            library.record_outcome(&file, Ok(analysis));
            if let Some(correction) = scan.corrections.remove(&file) {
                library.set_correction(&file, correction);
            }
        }
    }
    library
}

/// The title the panel shows: the one in the tags, or the file name without
/// its extension.
pub(crate) fn shown_title(track: &crate::library::Track) -> String {
    if track.title() != track.file_name {
        return track.title().to_string();
    }
    std::path::Path::new(&track.file_name)
        .file_stem()
        .map_or_else(
            || track.file_name.clone(),
            |stem| stem.to_string_lossy().into_owned(),
        )
}

/// Fingerprints the library's files from the analysis cache, one at a time
/// and at about a third of a core, so learning a large library in the
/// background does not slow the Mac.
pub(crate) fn learn_fingerprints_from_cache(
    paths: &[PathBuf],
    done: &AtomicUsize,
    stop: &AtomicBool,
) -> FingerprintIndex {
    let analyzer = euphoria_core::default_cache_path()
        .ok()
        .and_then(|path| euphoria_core::Cache::open(&path).ok())
        .and_then(|cache| euphoria_core::CachedAnalyzer::new(cache).ok());
    let Some(analyzer) = analyzer else {
        let mut empty = FingerprintIndex::new();
        empty.freeze();
        return empty;
    };
    let last = std::cell::Cell::new(Instant::now());
    analyzer
        .fingerprint_library(
            paths,
            &|finished| {
                done.store(finished, Ordering::Relaxed);
                let worked = last.get().elapsed();
                std::thread::sleep(worked.min(Duration::from_secs(2)) * 2);
                last.set(Instant::now());
            },
            &|| stop.load(Ordering::Relaxed),
        )
        .index
}
