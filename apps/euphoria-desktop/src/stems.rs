// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Separating tracks into stems in the background, one track at a time,
//! and what the player hears of the separated ones.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex, PoisonError};

use euphoria_core::EuphoriaError;
use euphoria_core::stems::{
    CachedSeparator, SeparationMode, SeparationObserver, SeparationStage, Stem, StemFiles,
};

use crate::playback::StemMix;

/// Whatever separates a track into stem files: the model in the app, a
/// stand in in tests.
pub(crate) trait StemSeparation: Send {
    fn stems(
        &mut self,
        path: &Path,
        observer: &dyn SeparationObserver,
    ) -> Result<StemFiles, EuphoriaError>;
}

impl StemSeparation for CachedSeparator {
    fn stems(
        &mut self,
        path: &Path,
        observer: &dyn SeparationObserver,
    ) -> Result<StemFiles, EuphoriaError> {
        CachedSeparator::stems(self, path, observer)
    }
}

/// Makes the separation of a mode the first time a track is separated in
/// it.
pub(crate) type SeparationFactory =
    fn(SeparationMode) -> Result<Box<dyn StemSeparation>, EuphoriaError>;

/// The separation of the app in `mode`, with its stems cached in Euphoria's
/// data folder.
pub(crate) fn model_separation(
    mode: SeparationMode,
) -> Result<Box<dyn StemSeparation>, EuphoriaError> {
    Ok(Box::new(CachedSeparator::open(mode)?))
}

type LoadedSeparation = Option<(SeparationMode, Box<dyn StemSeparation>)>;

/// Where a separation has got to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum SeparationPhase {
    Starting,
    Waiting,
    Downloading { fraction: f32 },
    LoadingModel,
    Decoding,
    Separating { done: usize, total: usize },
    Saving,
}

impl SeparationPhase {
    /// What the track detail says while the separation runs.
    pub(crate) fn description(self) -> String {
        match self {
            SeparationPhase::Starting => "Starting".to_string(),
            SeparationPhase::Downloading { fraction } => format!(
                "Downloading the model, {}%",
                (fraction * 100.0).round() as u32
            ),
            SeparationPhase::Waiting => "Waiting for another separation to finish".to_string(),
            SeparationPhase::LoadingModel => "Loading the model".to_string(),
            SeparationPhase::Decoding => "Decoding the track".to_string(),
            SeparationPhase::Separating { done, total } => {
                format!("Separating, {done} of {total} segments")
            }
            SeparationPhase::Saving => "Saving the stems".to_string(),
        }
    }
}

/// What the user hears of a separated track. The mix of the model's four
/// stems that the player lets through.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StemMode {
    /// The track as it is.
    #[default]
    Full,
    /// Only the vocals.
    Acapella,
    /// Everything but the vocals.
    Instrumental,
}

impl StemMode {
    /// The choices as the track detail shows them.
    pub(crate) const ALL: [StemMode; 3] =
        [StemMode::Full, StemMode::Acapella, StemMode::Instrumental];

    /// What it is called in the interface.
    pub(crate) fn label(self) -> &'static str {
        match self {
            StemMode::Full => "Full track",
            StemMode::Acapella => "Acapella",
            StemMode::Instrumental => "Instrumental",
        }
    }

    /// Which stems it is made of.
    pub(crate) fn stems(self) -> StemSet {
        match self {
            StemMode::Full => StemSet::ALL,
            StemMode::Acapella => StemSet::only(Stem::Vocals),
            StemMode::Instrumental => StemSet::ALL.toggled(Stem::Vocals),
        }
    }
}

/// The stems that play, out of the four. Never empty: a choice that would
/// leave nothing plays every stem again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StemSet {
    audible: [bool; 4],
}

impl StemSet {
    pub(crate) const ALL: StemSet = StemSet { audible: [true; 4] };

    pub(crate) fn only(stem: Stem) -> StemSet {
        let mut audible = [false; 4];
        audible[stem as usize] = true;
        StemSet { audible }
    }

    fn from_audible(audible: [bool; 4]) -> StemSet {
        if audible.contains(&true) {
            StemSet { audible }
        } else {
            StemSet::ALL
        }
    }

    pub(crate) fn contains(self, stem: Stem) -> bool {
        self.audible[stem as usize]
    }

    pub(crate) fn is_all(self) -> bool {
        self == StemSet::ALL
    }

    /// The set with `stem` added, or taken out.
    pub(crate) fn toggled(self, stem: Stem) -> StemSet {
        let mut audible = self.audible;
        audible[stem as usize] = !audible[stem as usize];
        StemSet::from_audible(audible)
    }

    /// The set a click on the name of `stem` leads to. A plain click plays
    /// only `stem`, or every stem when only `stem` plays already. With
    /// `toggles`, `stem` is added or taken out.
    pub(crate) fn after_click(self, stem: Stem, toggles: bool) -> StemSet {
        if toggles {
            self.toggled(stem)
        } else if self == StemSet::only(stem) {
            StemSet::ALL
        } else {
            StemSet::only(stem)
        }
    }

    /// What the control near the lanes says, naming the stems in `order`.
    pub(crate) fn summary(self, order: &[Stem]) -> Option<String> {
        let names = |audible: bool| {
            order
                .iter()
                .filter(|stem| self.contains(**stem) == audible)
                .map(|stem| stem.label())
                .collect::<Vec<_>>()
                .join(" + ")
        };
        match self.audible.iter().filter(|audible| **audible).count() {
            4 => None,
            3 => Some(format!("Muted: {}", names(false))),
            _ => Some(format!("Solo: {}", names(true))),
        }
    }
}

/// The stems of one track, or where getting them stands.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StemStatus {
    Separating,
    Ready(StemFiles),
    Failed(String),
}

/// Hears a separation on its thread and keeps what it heard for the window.
#[derive(Debug, Default, Clone)]
struct SharedProgress {
    phase: Arc<Mutex<Option<SeparationPhase>>>,
    received: Arc<AtomicU64>,
    download_total: Arc<AtomicU64>,
    done: Arc<AtomicUsize>,
}

impl SharedProgress {
    fn set(&self, phase: SeparationPhase) {
        *self.phase.lock().unwrap_or_else(PoisonError::into_inner) = Some(phase);
    }

    /// Where the separation stands, now.
    fn phase(&self) -> SeparationPhase {
        let phase = *self.phase.lock().unwrap_or_else(PoisonError::into_inner);
        match phase {
            Some(SeparationPhase::Downloading { .. }) => {
                let total = self.download_total.load(Ordering::Relaxed).max(1);
                SeparationPhase::Downloading {
                    fraction: self.received.load(Ordering::Relaxed) as f32 / total as f32,
                }
            }
            Some(SeparationPhase::Separating { total, .. }) => SeparationPhase::Separating {
                done: self.done.load(Ordering::Relaxed),
                total,
            },
            Some(other) => other,
            None => SeparationPhase::Starting,
        }
    }

    fn stage(&self, stage: SeparationStage) {
        self.set(match stage {
            SeparationStage::Waiting => SeparationPhase::Waiting,
            SeparationStage::LoadingModel => SeparationPhase::LoadingModel,
            SeparationStage::Decoding => SeparationPhase::Decoding,
            SeparationStage::Separating => SeparationPhase::Separating { done: 0, total: 0 },
            SeparationStage::Saving => SeparationPhase::Saving,
        });
    }

    fn downloading(&self, received: u64, total: u64) {
        self.download_total.store(total, Ordering::Relaxed);
        self.received.store(received, Ordering::Relaxed);
        self.set(SeparationPhase::Downloading { fraction: 0.0 });
    }

    fn separated(&self, done: usize, total: usize) {
        self.done.store(done, Ordering::Relaxed);
        self.set(SeparationPhase::Separating { done, total });
    }
}

impl SeparationObserver for SharedProgress {
    fn stage(&self, stage: SeparationStage) {
        self.stage(stage);
    }

    fn downloading(&self, received: u64, total: u64) {
        self.downloading(received, total);
    }

    fn separated(&self, done: usize, total: usize) {
        self.separated(done, total);
    }
}

struct Running {
    path: PathBuf,
    progress: SharedProgress,
    finished: Receiver<Result<StemFiles, String>>,
}

impl Running {
    fn phase(&self) -> SeparationPhase {
        self.progress.phase()
    }
}

/// The stems of the tracks separated since the app opened, the one being
/// separated, and what the player hears of the separated ones.
pub(crate) struct StemSession {
    statuses: HashMap<PathBuf, StemStatus>,
    running: Option<Running>,
    queued: VecDeque<PathBuf>,
    separation: Arc<Mutex<LoadedSeparation>>,
    factory: SeparationFactory,
    separation_mode: SeparationMode,
    mode: StemMode,
    picked: Option<StemSet>,
    mix: Arc<StemMix>,
}

impl StemSession {
    pub(crate) fn new(factory: SeparationFactory) -> Self {
        Self {
            statuses: HashMap::new(),
            running: None,
            queued: VecDeque::new(),
            separation: Arc::new(Mutex::new(None)),
            factory,
            separation_mode: SeparationMode::default(),
            mode: StemMode::default(),
            picked: None,
            mix: Arc::new(StemMix::default()),
        }
    }

    /// The stems of the track at `path`, or where getting them stands.
    pub(crate) fn status(&self, path: &Path) -> Option<&StemStatus> {
        self.statuses.get(path)
    }

    /// The stem files of the track at `path`, once they are separated.
    pub(crate) fn ready_files(&self, path: &Path) -> Option<StemFiles> {
        match self.statuses.get(path) {
            Some(StemStatus::Ready(files)) => Some(files.clone()),
            _ => None,
        }
    }

    /// The stem files to play in place of the track at `path`: its stems
    /// once they are separated, unless the full track is to be heard.
    pub(crate) fn files_to_play(&self, path: &Path) -> Option<StemFiles> {
        self.ready_files(path).filter(|_| !self.audible().is_all())
    }

    /// Deletes the separated stems of the track at `path`, so that it can
    /// be separated again. Returns why when they could not be deleted.
    pub(crate) fn remove(&mut self, path: &Path) -> Result<(), String> {
        if let Some(files) = self.ready_files(path) {
            files.remove().map_err(|error| error.chain())?;
        }
        if !matches!(self.statuses.get(path), Some(StemStatus::Separating)) {
            self.statuses.remove(path);
        }
        Ok(())
    }

    /// Marks the track at `path` as separated into `files`.
    #[cfg(test)]
    pub(crate) fn mark_ready(&mut self, path: &Path, files: StemFiles) {
        self.statuses
            .insert(path.to_path_buf(), StemStatus::Ready(files));
    }

    /// Where the separation has got to, when one is running.
    pub(crate) fn progress(&self) -> Option<SeparationPhase> {
        self.running.as_ref().map(Running::phase)
    }

    /// A finished separation, when one has finished and is not heard of yet.
    pub(crate) fn poll(&mut self) -> Option<PathBuf> {
        let path = self.running.as_ref()?.path.clone();
        let result = self.running.as_ref()?.finished.try_recv();
        if matches!(result, Err(TryRecvError::Empty)) {
            return None;
        }
        self.running = None;
        self.statuses.insert(
            path.clone(),
            match result {
                Ok(Ok(files)) => StemStatus::Ready(files),
                Ok(Err(problem)) => StemStatus::Failed(problem),
                Err(_) => {
                    StemStatus::Failed("The separation stopped before it finished".to_string())
                }
            },
        );
        self.start_next_queued();
        Some(path)
    }

    /// True while a separation runs or waits its turn.
    pub(crate) fn is_busy(&self) -> bool {
        self.running.is_some() || !self.queued.is_empty()
    }

    /// Where the separation of the track at `path` has got to: the phase it
    /// is in, or waiting for the one running, or `None` when it is neither.
    pub(crate) fn phase_of(&self, path: &Path) -> Option<SeparationPhase> {
        match &self.running {
            Some(running) if running.path == path => Some(running.phase()),
            _ if self.queued.iter().any(|queued| queued == path) => Some(SeparationPhase::Waiting),
            _ => None,
        }
    }

    /// Puts the track at `path` in line behind the separation that runs,
    /// which it takes over from when that finishes. Returns false when it
    /// is already running or in line.
    pub(crate) fn enqueue(&mut self, path: &Path) -> bool {
        if self.phase_of(path).is_some() {
            return false;
        }
        self.queued.push_back(path.to_path_buf());
        self.statuses
            .insert(path.to_path_buf(), StemStatus::Separating);
        true
    }

    /// Stops waiting for the separation of the track at `path`. A track in
    /// line is simply taken out. One that runs is let go of: the model
    /// cannot be interrupted, so it finishes in the background and keeps
    /// the stems in the cache, where asking again finds them at once.
    pub(crate) fn cancel(&mut self, path: &Path) -> bool {
        let was_queued = self.queued.iter().any(|queued| queued == path);
        self.queued.retain(|queued| queued != path);
        let was_running = self.running.as_ref().is_some_and(|run| run.path == path);
        if was_running {
            self.running = None;
        }
        if !(was_queued || was_running) {
            return false;
        }
        self.statuses.remove(path);
        if was_running {
            self.start_next_queued();
        }
        true
    }

    fn start_next_queued(&mut self) {
        while self.running.is_none() {
            let Some(next) = self.queued.pop_front() else {
                break;
            };
            self.start(&next);
        }
    }

    /// Starts separating the track at `path` on a thread of its own.
    /// Returns false when another track is being separated, which has to
    /// finish first.
    pub(crate) fn start(&mut self, path: &Path) -> bool {
        if self.running.is_some() {
            return false;
        }
        let progress = SharedProgress::default();
        let (sender, finished) = channel();
        let separation = Arc::clone(&self.separation);
        let factory = self.factory;
        let separation_mode = self.separation_mode;
        let thread_progress = progress.clone();
        let thread_path = path.to_path_buf();
        let spawned = std::thread::Builder::new()
            .name("euphoria-stems".to_string())
            .spawn(move || {
                let mut loaded = separation.lock().unwrap_or_else(PoisonError::into_inner);
                if loaded
                    .as_ref()
                    .is_some_and(|(loaded_mode, _)| *loaded_mode != separation_mode)
                {
                    *loaded = None;
                }
                let result = match &mut *loaded {
                    Some((_, separation)) => separation.stems(&thread_path, &thread_progress),
                    empty => factory(separation_mode).and_then(|made| {
                        empty
                            .insert((separation_mode, made))
                            .1
                            .stems(&thread_path, &thread_progress)
                    }),
                };
                let _ = sender.send(result.map_err(|error| failure_message(&error)));
            });
        if spawned.is_err() {
            self.statuses.insert(
                path.to_path_buf(),
                StemStatus::Failed("Could not start the separation".to_string()),
            );
            return false;
        }
        self.statuses
            .insert(path.to_path_buf(), StemStatus::Separating);
        self.running = Some(Running {
            path: path.to_path_buf(),
            progress,
            finished,
        });
        true
    }

    /// The mode new separations are made in.
    #[cfg(test)]
    pub(crate) fn separation_mode(&self) -> SeparationMode {
        self.separation_mode
    }

    /// Makes the separations that start from now on use `mode`. The network
    /// that is loaded is let go when it is not the one of `mode`, before the
    /// next one loads, since the Advanced network needs several gigabytes. A
    /// separation that is running keeps the mode it started with, and the
    /// network it uses is let go by the next one to start. Returns true when
    /// the mode changed.
    pub(crate) fn set_separation_mode(&mut self, mode: SeparationMode) -> bool {
        if self.separation_mode == mode {
            return false;
        }
        self.separation_mode = mode;
        let mut loaded = match self.separation.try_lock() {
            Ok(loaded) => loaded,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return true,
        };
        *loaded = None;
        true
    }

    /// What the user hears of the separated tracks.
    #[cfg(test)]
    pub(crate) fn mode(&self) -> StemMode {
        self.mode
    }

    /// What the user hears of the separated tracks, with the sound told to
    /// follow. Returns true when the mix changed.
    pub(crate) fn set_mode(&mut self, mode: StemMode) -> bool {
        if self.mode == mode && self.picked.is_none() {
            return false;
        }
        self.mode = mode;
        self.picked = None;
        self.update_mix();
        true
    }

    /// The stems picked from the lanes, if any are.
    #[cfg(test)]
    pub(crate) fn picked(&self) -> Option<StemSet> {
        self.picked
    }

    /// The stems that play: the ones picked from the lanes, or else the
    /// ones of the mode.
    pub(crate) fn audible(&self) -> StemSet {
        self.picked.unwrap_or(self.mode.stems())
    }

    /// Plays `stems`, picked from the lanes. Every stem is the full track,
    /// played from its file. Returns true when the mix changed.
    pub(crate) fn pick(&mut self, stems: StemSet) -> bool {
        let picked = (!stems.is_all()).then_some(stems);
        if self.picked == picked && self.mode == StemMode::Full {
            return false;
        }
        let was_audible = self.audible();
        self.picked = picked;
        self.mode = StemMode::Full;
        self.update_mix();
        was_audible != self.audible()
    }

    /// Returns to what the mode plays, forgetting the stems picked from the
    /// lanes. Returns true when the mix changed.
    pub(crate) fn clear_picked(&mut self) -> bool {
        if self.picked.take().is_none() {
            return false;
        }
        self.update_mix();
        true
    }

    /// Which stems the player lets through, shared with the sound.
    pub(crate) fn mix(&self) -> Arc<StemMix> {
        Arc::clone(&self.mix)
    }

    fn update_mix(&self) {
        let audible = self.audible();
        self.mix.set_audible(|stem| audible.contains(stem));
    }
}

/// A short message for the track detail, with the cause the core gave.
fn failure_message(error: &EuphoriaError) -> String {
    match error {
        EuphoriaError::ModelDownload { .. } => {
            "Could not download the model. Check that this computer is online, then try again."
                .to_string()
        }
        other => other.chain(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use euphoria_core::stems::Stem;

    use super::*;

    fn failing(_mode: SeparationMode) -> Result<Box<dyn StemSeparation>, EuphoriaError> {
        Err(EuphoriaError::NoDataDirectory)
    }

    fn wait_for_finish(session: &mut StemSession) -> PathBuf {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(path) = session.poll() {
                return path;
            }
            assert!(Instant::now() < deadline, "the separation never finished");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    static LOADED_NETWORKS: AtomicUsize = AtomicUsize::new(0);
    static MOST_LOADED_NETWORKS: AtomicUsize = AtomicUsize::new(0);
    static OPENED_MODES: Mutex<Vec<SeparationMode>> = Mutex::new(Vec::new());

    struct CountedNetwork;

    impl CountedNetwork {
        fn load(mode: SeparationMode) -> Result<Box<dyn StemSeparation>, EuphoriaError> {
            OPENED_MODES.lock().unwrap().push(mode);
            let loaded = LOADED_NETWORKS.fetch_add(1, Ordering::SeqCst) + 1;
            MOST_LOADED_NETWORKS.fetch_max(loaded, Ordering::SeqCst);
            Ok(Box::new(CountedNetwork))
        }
    }

    impl Drop for CountedNetwork {
        fn drop(&mut self) {
            LOADED_NETWORKS.fetch_sub(1, Ordering::SeqCst);
        }
    }

    impl StemSeparation for CountedNetwork {
        fn stems(
            &mut self,
            _path: &Path,
            _observer: &dyn SeparationObserver,
        ) -> Result<StemFiles, EuphoriaError> {
            Ok(StemFiles::in_directory(PathBuf::from("/stems/track")))
        }
    }

    #[test]
    fn changing_the_separation_mode_lets_go_of_the_network_before_the_next_one_loads() {
        let mut session = StemSession::new(CountedNetwork::load);
        let track = Path::new("/music/track.mp3");
        assert_eq!(session.separation_mode(), SeparationMode::Default);
        assert!(!session.set_separation_mode(SeparationMode::Default));

        assert!(session.start(track));
        wait_for_finish(&mut session);
        assert_eq!(LOADED_NETWORKS.load(Ordering::SeqCst), 1);

        assert!(session.start(track));
        wait_for_finish(&mut session);
        assert_eq!(
            OPENED_MODES.lock().unwrap().len(),
            1,
            "the same mode keeps its network"
        );

        assert!(session.set_separation_mode(SeparationMode::Advanced));
        assert_eq!(LOADED_NETWORKS.load(Ordering::SeqCst), 0);

        assert!(session.start(track));
        wait_for_finish(&mut session);
        assert_eq!(
            *OPENED_MODES.lock().unwrap(),
            [SeparationMode::Default, SeparationMode::Advanced]
        );
        assert_eq!(MOST_LOADED_NETWORKS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_mode_names_its_stems_and_only_a_change_is_a_change() {
        let mut session = StemSession::new(failing);
        assert_eq!(session.mode(), StemMode::Full);
        let mix = session.mix();
        assert!(
            Stem::ALL
                .iter()
                .enumerate()
                .all(|(index, _)| mix.gain(index) > 0.99)
        );

        assert!(!session.set_mode(StemMode::Full));
        assert!(session.set_mode(StemMode::Acapella));
        assert!(mix.gain(0) < 0.01);
        assert!(mix.gain(3) > 0.99);
        assert!(!session.set_mode(StemMode::Acapella));

        assert!(session.set_mode(StemMode::Instrumental));
        assert!(mix.gain(0) > 0.99);
        assert!(mix.gain(3) < 0.01);

        assert!(session.set_mode(StemMode::Full));
        assert!(
            Stem::ALL
                .iter()
                .enumerate()
                .all(|(index, _)| mix.gain(index) > 0.99)
        );
    }

    fn audible_of(mix: &StemMix) -> [bool; 4] {
        Stem::ALL.map(|stem| mix.gain(stem as usize) > 0.99)
    }

    #[test]
    fn a_click_solos_a_stem_and_a_second_click_restores_the_full_track() {
        let mut session = StemSession::new(failing);
        let mix = session.mix();

        let solo = session.audible().after_click(Stem::Bass, false);
        assert_eq!(solo, StemSet::only(Stem::Bass));
        assert!(session.pick(solo));
        for stem in Stem::ALL {
            assert_eq!(
                mix.gain(stem as usize) > 0.99,
                stem == Stem::Bass,
                "{stem:?}"
            );
        }
        assert_eq!(
            session.audible().after_click(Stem::Drums, false),
            StemSet::only(Stem::Drums)
        );

        let restored = session.audible().after_click(Stem::Bass, false);
        assert_eq!(restored, StemSet::ALL);
        assert!(session.pick(restored));
        assert_eq!(session.picked(), None);
        assert_eq!(audible_of(&mix), [true; 4]);
        assert!(!session.pick(StemSet::ALL));
    }

    #[test]
    fn a_toggle_adds_and_removes_stems_and_never_leaves_none() {
        let mut session = StemSession::new(failing);
        let mix = session.mix();

        let without_drums = session.audible().after_click(Stem::Drums, true);
        assert!(session.pick(without_drums));
        assert_eq!(
            session.audible().summary(&Stem::ALL).as_deref(),
            Some("Muted: Drums")
        );
        assert_eq!(audible_of(&mix), Stem::ALL.map(|stem| stem != Stem::Drums));

        assert!(session.pick(StemSet::only(Stem::Vocals)));
        let pair = session.audible().after_click(Stem::Bass, true);
        assert!(session.pick(pair));
        assert_eq!(
            audible_of(&mix),
            Stem::ALL.map(|stem| matches!(stem, Stem::Vocals | Stem::Bass))
        );
        assert_eq!(
            pair.summary(&[Stem::Vocals, Stem::Drums, Stem::Bass, Stem::Other])
                .as_deref(),
            Some("Solo: Vocals + Bass")
        );

        let vocals = session.audible().after_click(Stem::Bass, true);
        assert_eq!(vocals, StemSet::only(Stem::Vocals));
        assert!(session.pick(vocals));

        let emptied = session.audible().after_click(Stem::Vocals, true);
        assert_eq!(emptied, StemSet::ALL);
        assert!(session.pick(emptied));
        assert_eq!(audible_of(&mix), [true; 4]);
        assert_eq!(StemSet::ALL.summary(&Stem::ALL), None);
    }

    #[test]
    fn picked_stems_match_the_quick_choices_and_choosing_one_clears_them() {
        let mut session = StemSession::new(failing);
        let mix = session.mix();
        let track = Path::new("/music/track.mp3");
        session.mark_ready(
            track,
            StemFiles::in_directory(PathBuf::from("/stems/track")),
        );
        assert_eq!(session.files_to_play(track), None);

        assert!(session.set_mode(StemMode::Acapella));
        assert_eq!(session.audible(), StemSet::only(Stem::Vocals));
        let with_drums = session.audible().after_click(Stem::Drums, true);
        assert!(session.pick(with_drums));
        assert_eq!(
            audible_of(&mix),
            Stem::ALL.map(|stem| matches!(stem, Stem::Vocals | Stem::Drums))
        );
        assert!(session.files_to_play(track).is_some());

        let instrumental = StemSet::ALL.toggled(Stem::Vocals);
        assert!(session.pick(instrumental));
        assert_eq!(session.audible(), StemMode::Instrumental.stems());

        let full = session.audible().after_click(Stem::Vocals, true);
        assert!(session.pick(full));
        assert_eq!(session.mode(), StemMode::Full);
        assert_eq!(session.files_to_play(track), None);

        assert!(session.pick(StemSet::only(Stem::Other)));
        assert!(session.set_mode(StemMode::Full));
        assert_eq!(session.picked(), None);
        assert_eq!(audible_of(&mix), [true; 4]);
        assert!(!session.clear_picked());
    }

    #[test]
    fn only_acapella_and_instrumental_play_the_stems() {
        let mut session = StemSession::new(failing);
        let track = Path::new("/music/track.mp3");
        let files = StemFiles::in_directory(PathBuf::from("/stems/track"));
        assert_eq!(session.files_to_play(track), None);
        session.mark_ready(track, files.clone());
        assert_eq!(session.files_to_play(track), None);
        session.set_mode(StemMode::Acapella);
        assert_eq!(session.files_to_play(track), Some(files.clone()));
        session.set_mode(StemMode::Instrumental);
        assert_eq!(session.files_to_play(track), Some(files));
        assert_eq!(session.files_to_play(Path::new("/music/other.mp3")), None);
    }

    #[test]
    fn removed_stems_are_deleted_and_forgotten() {
        let directory = std::env::temp_dir().join(format!(
            "euphoria-desktop-remove-stems-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("vocals.wav"), b"stem").unwrap();
        let mut session = StemSession::new(failing);
        let track = Path::new("/music/track.mp3");
        session.mark_ready(track, StemFiles::in_directory(directory.clone()));

        session.remove(track).unwrap();
        assert!(!directory.exists());
        assert_eq!(session.status(track), None);
        session.remove(track).unwrap();
    }

    #[test]
    fn a_failed_separation_says_why_and_frees_the_session() {
        let mut session = StemSession::new(failing);
        let track = Path::new("/music/track.mp3");
        assert!(session.start(track));
        assert!(!session.start(Path::new("/music/other.mp3")));
        assert_eq!(wait_for_finish(&mut session), track);
        match session.status(track) {
            Some(StemStatus::Failed(message)) => {
                assert!(message.contains("EUPHORIA_DATA_DIR"), "{message}")
            }
            other => panic!("{other:?}"),
        }
        assert!(session.running.is_none());
        assert_eq!(session.ready_files(track), None);
    }

    #[test]
    fn a_track_asked_for_while_another_runs_waits_its_turn_and_can_be_cancelled() {
        let mut session = StemSession::new(failing);
        let first = Path::new("/music/first.mp3");
        let second = Path::new("/music/second.mp3");
        let third = Path::new("/music/third.mp3");
        assert!(session.start(first));
        assert!(session.enqueue(second));
        assert!(session.enqueue(third));
        assert!(!session.enqueue(second));
        assert_eq!(session.phase_of(second), Some(SeparationPhase::Waiting));
        assert!(session.is_busy());

        assert!(session.cancel(third));
        assert_eq!(session.phase_of(third), None);
        assert_eq!(session.status(third), None);

        assert_eq!(wait_for_finish(&mut session), first);
        assert!(
            session.phase_of(second).is_some(),
            "the next in line started"
        );
        assert!(
            session
                .running
                .as_ref()
                .is_some_and(|run| run.path == second)
        );
        assert_eq!(wait_for_finish(&mut session), second);
        assert!(!session.is_busy());
    }

    #[test]
    fn cancelling_the_running_separation_frees_the_session_for_the_next() {
        let mut session = StemSession::new(failing);
        let first = Path::new("/music/first.mp3");
        let second = Path::new("/music/second.mp3");
        assert!(session.start(first));
        assert!(session.enqueue(second));
        assert!(session.cancel(first));
        assert_eq!(session.status(first), None);
        assert!(
            session
                .running
                .as_ref()
                .is_some_and(|run| run.path == second)
        );
        assert!(!session.cancel(Path::new("/music/unknown.mp3")));
    }
}
