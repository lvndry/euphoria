// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Separating tracks into stems in the background, one track at a time,
//! and what the player hears of the separated ones.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex, PoisonError};

use euphoria_core::EuphoriaError;
use euphoria_core::stems::{CachedSeparator, SeparationObserver, SeparationStage, StemFiles};

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

/// Makes the separation the first time a track is separated.
pub(crate) type SeparationFactory = fn() -> Result<Box<dyn StemSeparation>, EuphoriaError>;

/// The separation of the app: the htdemucs model, with its stems cached in
/// Euphoria's data folder.
pub(crate) fn model_separation() -> Result<Box<dyn StemSeparation>, EuphoriaError> {
    Ok(Box::new(CachedSeparator::open_default()?))
}

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

    /// Whether it is played from the stems. The full track is played from
    /// the file itself, which is what the stems were separated from.
    pub(crate) fn plays_stems(self) -> bool {
        self != StemMode::Full
    }

    /// Which stems it is made of.
    pub(crate) fn audible_stems(self) -> [bool; 4] {
        match self {
            StemMode::Full => [true; 4],
            StemMode::Acapella => [false, false, false, true],
            StemMode::Instrumental => [true, true, true, false],
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
    separation: Arc<Mutex<Option<Box<dyn StemSeparation>>>>,
    factory: SeparationFactory,
    mode: StemMode,
    mix: Arc<StemMix>,
}

impl StemSession {
    pub(crate) fn new(factory: SeparationFactory) -> Self {
        Self {
            statuses: HashMap::new(),
            running: None,
            separation: Arc::new(Mutex::new(None)),
            factory,
            mode: StemMode::default(),
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
        self.ready_files(path).filter(|_| self.mode.plays_stems())
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
        Some(path)
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
        let thread_progress = progress.clone();
        let thread_path = path.to_path_buf();
        let spawned = std::thread::Builder::new()
            .name("euphoria-stems".to_string())
            .spawn(move || {
                let mut separation = separation.lock().unwrap_or_else(PoisonError::into_inner);
                let result = match &mut *separation {
                    Some(separation) => separation.stems(&thread_path, &thread_progress),
                    empty => factory()
                        .and_then(|made| empty.insert(made).stems(&thread_path, &thread_progress)),
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

    /// What the user hears of the separated tracks.
    pub(crate) fn mode(&self) -> StemMode {
        self.mode
    }

    /// What the user hears of the separated tracks, with the sound told to
    /// follow. Returns true when the mix changed.
    pub(crate) fn set_mode(&mut self, mode: StemMode) -> bool {
        if self.mode == mode {
            return false;
        }
        self.mode = mode;
        self.update_mix();
        true
    }

    /// Which stems the player lets through, shared with the sound.
    pub(crate) fn mix(&self) -> Arc<StemMix> {
        Arc::clone(&self.mix)
    }

    fn update_mix(&self) {
        self.mix
            .set_audible(|stem| self.mode.audible_stems()[stem as usize]);
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

    fn failing() -> Result<Box<dyn StemSeparation>, EuphoriaError> {
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
}
