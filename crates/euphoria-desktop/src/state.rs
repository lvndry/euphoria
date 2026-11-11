// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The library together with the analysis that fills it in. Nothing here
//! needs a window.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, PoisonError};

use euphoria_core::{
    CachedAnalyzer, EntryCheck, EuphoriaError, PlaylistStore, StoredPlaylist, TrackAnalysis,
    locate_moved_files,
};

use crate::analysis::{AnalysisEvent, AnalysisWorkers, StopSignal};
use crate::demo::DemoTrack;
use crate::library::Library;
use crate::playlists::Playlists;
use crate::progress::AnalysisProgress;

/// What was found under the files and folders the user added.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Scan {
    /// The paths that are folders.
    pub(crate) folders: Vec<PathBuf>,
    /// Every audio file found, sorted within each added path.
    pub(crate) files: Vec<PathBuf>,
    /// The analyses the cache already holds for those files.
    pub(crate) cached: HashMap<PathBuf, TrackAnalysis>,
    /// The keys and tempos the user set for those files.
    pub(crate) corrections: HashMap<PathBuf, euphoria_core::Correction>,
    /// One message for each path that could not be read.
    pub(crate) problems: Vec<String>,
}

/// Asks the core for the audio files under each path and, with a cache, for
/// the analyses it already holds for them. A path that cannot be read is
/// reported in [`Scan::problems`] and does not stop the others.
///
/// This walks folders on disk, so call it off the main thread.
pub(crate) fn scan(paths: &[PathBuf], cache: Option<&CachedAnalyzer>) -> Scan {
    let mut scan = Scan::default();
    for path in paths {
        match euphoria_core::find_audio_files(std::slice::from_ref(path)) {
            Ok(files) => {
                if path.is_dir() {
                    scan.folders.push(path.clone());
                }
                scan.files.extend(files);
            }
            Err(error) => scan.problems.push(error.chain()),
        }
    }
    if let Some(cache) = cache {
        for file in &scan.files {
            if let Some(analysis) = cache.stored_analysis(file) {
                scan.cached.insert(file.clone(), analysis);
            }
            if let Ok(Some(correction)) = cache.correction_for(file) {
                scan.corrections.insert(file.clone(), correction);
            }
        }
    }
    scan
}

/// The library and the background workers analyzing it.
pub(crate) struct Session {
    library: Library,
    progress: AnalysisProgress,
    workers: AnalysisWorkers,
    events: Receiver<AnalysisEvent>,
    cache: Option<Arc<CachedAnalyzer>>,
    cached_count: usize,
    cache_problem: Option<String>,
    playlists: Playlists,
    playlist_saver: Option<PlaylistSaver>,
}

/// The playlists database, shared with the tasks that save to it.
#[derive(Clone)]
pub(crate) struct PlaylistSaver {
    store: Arc<Mutex<PlaylistStore>>,
    /// The revision of the playlists last written, so a slow save of older
    /// playlists never overwrites a newer one.
    saved_revision: Arc<AtomicU64>,
}

impl PlaylistSaver {
    /// Keeps `playlists`, made at `revision`, unless newer ones were kept
    /// already.
    pub(crate) fn save(
        &self,
        playlists: &[StoredPlaylist],
        revision: u64,
    ) -> Result<(), EuphoriaError> {
        let mut store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
        if self.saved_revision.load(Ordering::Relaxed) >= revision {
            return Ok(());
        }
        store.save(playlists)?;
        self.saved_revision.store(revision, Ordering::Relaxed);
        Ok(())
    }
}

impl Session {
    /// Starts with an empty library and idle workers. With a cache, tracks
    /// analyzed before are filled in without analyzing them again and new
    /// analyses are stored.
    pub(crate) fn start(
        cache: Option<Arc<CachedAnalyzer>>,
    ) -> Result<Self, rayon::ThreadPoolBuildError> {
        let (workers, events) = AnalysisWorkers::start(cache.clone())?;
        Ok(Self {
            library: Library::default(),
            progress: AnalysisProgress::default(),
            workers,
            events,
            cache,
            cached_count: 0,
            cache_problem: None,
            playlists: Playlists::default(),
            playlist_saver: None,
        })
    }

    /// Keeps playlists in `store`, starting from those it holds. Files that
    /// moved are looked for where the cache last saw them.
    pub(crate) fn use_playlist_store(&mut self, store: PlaylistStore) -> Result<(), EuphoriaError> {
        let mut stored = store.load()?;
        let moved = self
            .cache
            .as_deref()
            .is_some_and(|cache| locate_moved_files(&mut stored, Some(cache), EntryCheck::Quick).0);
        self.playlists = Playlists::from_stored(stored);
        let mut store = store;
        if moved {
            store.save(&self.playlists.stored_with_contents(|_| None))?;
        }
        let saver = PlaylistSaver {
            store: Arc::new(Mutex::new(store)),
            saved_revision: Arc::new(AtomicU64::new(0)),
        };
        self.playlist_saver = Some(saver);
        Ok(())
    }

    /// The playlists.
    pub(crate) fn playlists(&self) -> &Playlists {
        &self.playlists
    }

    /// The playlists, to change. The caller saves them afterwards.
    pub(crate) fn playlists_mut(&mut self) -> &mut Playlists {
        &mut self.playlists
    }

    /// Where playlists are kept, when they are.
    pub(crate) fn playlist_saver(&self) -> Option<PlaylistSaver> {
        self.playlist_saver.clone()
    }

    /// The tracks added so far.
    pub(crate) fn library(&self) -> &Library {
        &self.library
    }

    /// How far the current run of analysis has got.
    pub(crate) fn progress(&self) -> AnalysisProgress {
        self.progress
    }

    /// The cache to scan with, when there is one.
    pub(crate) fn cache(&self) -> Option<Arc<CachedAnalyzer>> {
        self.cache.clone()
    }

    /// How many tracks were filled in from the cache.
    pub(crate) fn cached_count(&self) -> usize {
        self.cached_count
    }

    /// Why results are not being stored in the cache, the first time that is
    /// found out. Later calls return `None`.
    pub(crate) fn take_cache_problem(&mut self) -> Option<String> {
        self.cache_problem.take()
    }

    /// The signal that stops the workers.
    pub(crate) fn stop_signal(&self) -> StopSignal {
        self.workers.stop_signal()
    }

    /// Adds scanned folders and files to the library. New files the cache
    /// knows are filled in at once; the other new files are queued for
    /// analysis. Returns how many files were queued.
    pub(crate) fn add(&mut self, mut scan: Scan) -> usize {
        for folder in scan.folders {
            self.library.add_folder(folder);
        }
        let mut files_to_analyze = Vec::new();
        for file in self.library.add_files(scan.files) {
            match scan.cached.remove(&file) {
                Some(analysis) => {
                    self.library.record_outcome(&file, Ok(analysis));
                    self.cached_count += 1;
                }
                None => files_to_analyze.push(file),
            }
        }
        for (file, correction) in scan.corrections {
            self.library.set_correction(&file, correction);
        }
        let queued = files_to_analyze.len();
        self.progress.enqueue(queued);
        self.workers.enqueue(files_to_analyze);
        queued
    }

    /// Sets the key and tempo the user chose for the track at `path`, in the
    /// library at once. Keeping it in the cache is
    /// [`CachedAnalyzer::set_correction`], which reads the file and so runs
    /// off the main thread.
    pub(crate) fn set_correction(&mut self, path: &Path, correction: euphoria_core::Correction) {
        self.library.set_correction(path, correction);
    }

    /// Shows every tempo in `range`, without analyzing anything again.
    pub(crate) fn set_tempo_range(&mut self, range: euphoria_core::TempoRange) {
        self.library.set_tempo_range(range);
    }

    /// Takes the titles and artists read from the files' tags.
    pub(crate) fn set_track_names(&mut self, names: Vec<(PathBuf, euphoria_core::TrackName)>) {
        for (path, name) in names {
            self.library.set_names(&path, name.title, name.artist);
        }
    }

    /// Adds a made up folder for made up tracks to be in, without looking
    /// on disk.
    pub(crate) fn add_demo_folder(&mut self, folder: PathBuf) {
        self.library.add_folder(folder);
    }

    /// Adds made up tracks with their invented results, without analyzing
    /// anything.
    pub(crate) fn add_demo_tracks(&mut self, tracks: Vec<DemoTrack>) {
        self.library
            .add_files(tracks.iter().map(|track| track.path.clone()).collect());
        for track in tracks {
            self.library.record_outcome(&track.path, track.outcome);
        }
    }

    /// Applies everything the workers have reported to the library: which
    /// files they started and the results of the ones they finished. Returns
    /// how many reports arrived.
    pub(crate) fn collect_finished(&mut self) -> usize {
        let mut collected = 0;
        while let Ok(event) = self.events.try_recv() {
            self.apply(event);
            collected += 1;
        }
        collected
    }

    /// Applies one report from a worker. Only a finished file moves the
    /// progress on.
    fn apply(&mut self, event: AnalysisEvent) {
        let file = match event {
            AnalysisEvent::Started(path) => {
                self.library.mark_analyzing(&path);
                return;
            }
            AnalysisEvent::Finished(file) => *file,
        };
        self.library.record_outcome(&file.path, file.outcome);
        self.progress.finish_one();
        if file.from_cache {
            self.cached_count += 1;
        }
        if self.cache_problem.is_none() {
            self.cache_problem = file.cache_problem;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use euphoria_core::Cache;

    use super::*;
    use crate::analysis::FinishedFile;
    use crate::library::TrackStatus;

    fn fixtures_folder() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../euphoria-core/tests/fixtures")
    }

    /// A folder under the system's temporary folder, deleted when dropped.
    struct TemporaryFolder(PathBuf);

    impl TemporaryFolder {
        fn new(name: &str) -> Self {
            let folder =
                std::env::temp_dir().join(format!("euphoria-state-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&folder);
            std::fs::create_dir_all(&folder).unwrap();
            Self(folder)
        }

        fn open_cache(&self) -> Arc<CachedAnalyzer> {
            let cache = Cache::open(&self.0.join("analysis-cache.sqlite3")).unwrap();
            Arc::new(CachedAnalyzer::new(cache).unwrap())
        }
    }

    impl Drop for TemporaryFolder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn wait_until_idle(session: &mut Session) {
        let deadline = Instant::now() + Duration::from_secs(120);
        while session.progress().is_active() {
            assert!(Instant::now() < deadline, "analysis did not finish");
            session.collect_finished();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn assert_every_track_is_analyzed(session: &Session) {
        assert_eq!(session.library().counts().total, 10);
        for track in session.library().tracks() {
            match &track.status {
                TrackStatus::Analyzed(analysis) => {
                    assert!((analysis.duration_seconds - 1.0).abs() < 0.1, "{track:?}");
                }
                other => panic!("{} was not analyzed: {other:?}", track.file_name),
            }
        }
    }

    #[test]
    fn scanning_reports_unreadable_paths_and_keeps_the_rest() {
        let found = scan(
            &[PathBuf::from("/definitely/not/here"), fixtures_folder()],
            None,
        );
        assert_eq!(found.folders, vec![fixtures_folder()]);
        assert_eq!(found.files.len(), 10);
        assert!(found.cached.is_empty());
        assert_eq!(found.problems.len(), 1);
        assert!(found.problems[0].starts_with("cannot read /definitely/not/here"));
    }

    #[test]
    fn a_correction_kept_in_the_cache_comes_back_on_the_next_launch() {
        let folder = TemporaryFolder::new("corrections");
        let track = folder.0.join("track.wav");
        std::fs::copy(fixtures_folder().join("sine-440hz-mono.wav"), &track).unwrap();
        let e_minor = euphoria_core::Key::new(4, euphoria_core::Mode::Minor);
        let correction = euphoria_core::Correction::of_key(e_minor)
            .with_bpm(Some(87.0))
            .unwrap();
        {
            let cache = folder.open_cache();
            let mut session = Session::start(Some(cache.clone())).unwrap();
            session.add(scan(std::slice::from_ref(&folder.0), Some(&cache)));
            wait_until_idle(&mut session);
            session.set_correction(&track, correction);
            cache.set_correction(&track, &correction).unwrap();
        }

        let cache = folder.open_cache();
        let found = scan(std::slice::from_ref(&folder.0), Some(&cache));
        assert_eq!(found.corrections.get(&track), Some(&correction));
        let mut session = Session::start(Some(cache)).unwrap();
        session.add(found);
        let reopened = &session.library().tracks()[0];
        assert_eq!(reopened.key(), Some(e_minor));
        assert_eq!(reopened.bpm(), Some(87.0));
        assert!(
            reopened.analysis().is_some(),
            "the analysis came from the cache"
        );
    }

    #[test]
    fn analyzes_the_fixture_folder_and_fills_in_every_track() {
        let mut session = Session::start(None).unwrap();
        let found = scan(&[fixtures_folder()], None);

        assert_eq!(session.add(found), 10);
        assert_eq!(session.library().folders(), [fixtures_folder()]);
        assert_eq!(session.library().counts().total, 10);
        assert!(session.progress().is_active());

        wait_until_idle(&mut session);

        assert_every_track_is_analyzed(&session);
        assert_eq!(session.cached_count(), 0);
        assert!(
            session
                .stop_signal()
                .wait_for_files_in_progress(Duration::from_secs(30))
        );
    }

    #[test]
    fn a_started_file_shows_as_analyzing_and_only_a_finished_one_counts_as_done() {
        let mut session = Session::start(None).unwrap();
        session.stop_signal().skip_queued_files();
        let first = PathBuf::from("/music/first.mp3");
        let second = PathBuf::from("/music/second.mp3");
        let found = Scan {
            files: vec![first.clone(), second.clone()],
            ..Scan::default()
        };
        assert_eq!(session.add(found), 2);
        let status_of = |session: &Session, path: &Path| {
            let index = session.library().index_of(path).unwrap();
            session.library().track(index).unwrap().status.clone()
        };

        session.apply(AnalysisEvent::Started(first.clone()));
        assert_eq!(status_of(&session, &first), TrackStatus::Analyzing);
        assert_eq!(status_of(&session, &second), TrackStatus::Queued);
        assert_eq!(session.progress().label().as_deref(), Some("0 of 2 done"));

        session.apply(AnalysisEvent::Finished(Box::new(FinishedFile {
            path: first.clone(),
            outcome: Err("cannot open the file".to_string()),
            from_cache: false,
            cache_problem: None,
        })));
        assert_eq!(
            status_of(&session, &first),
            TrackStatus::Failed("cannot open the file".to_string())
        );
        assert_eq!(status_of(&session, &second), TrackStatus::Queued);
        assert_eq!(session.progress().label().as_deref(), Some("1 of 2 done"));
    }

    #[test]
    fn adding_the_same_folder_again_queues_nothing() {
        let mut session = Session::start(None).unwrap();
        session.add(scan(&[fixtures_folder()], None));
        assert_eq!(session.add(scan(&[fixtures_folder()], None)), 0);
        assert_eq!(session.library().folders().len(), 1);
        wait_until_idle(&mut session);
    }

    #[test]
    fn a_file_that_cannot_be_decoded_fails_with_a_reason() {
        let mut session = Session::start(None).unwrap();
        let found = scan(&[fixtures_folder().join("README.md")], None);
        assert_eq!(session.add(found), 1);
        wait_until_idle(&mut session);
        let reason = session.library().tracks()[0].failure().unwrap();
        assert!(reason.contains("README.md"), "{reason}");
        assert_eq!(session.library().counts().needs_review, 1);
    }

    #[test]
    fn a_second_session_fills_every_track_in_from_the_cache_without_analyzing() {
        let folder = TemporaryFolder::new("second-session");

        let mut first = Session::start(Some(folder.open_cache())).unwrap();
        let found = scan(&[fixtures_folder()], first.cache().as_deref());
        assert!(found.cached.is_empty());
        assert_eq!(first.add(found), 10);
        wait_until_idle(&mut first);
        assert_every_track_is_analyzed(&first);
        assert_eq!(first.cached_count(), 0);
        assert_eq!(first.take_cache_problem(), None);
        drop(first);

        let mut second = Session::start(Some(folder.open_cache())).unwrap();
        let found = scan(&[fixtures_folder()], second.cache().as_deref());
        assert_eq!(found.cached.len(), 10);
        assert_eq!(second.add(found), 0);
        assert!(!second.progress().is_active());
        assert_every_track_is_analyzed(&second);
        assert_eq!(second.cached_count(), 10);
    }

    #[test]
    fn a_copied_file_is_recognised_by_its_content_when_it_is_analyzed() {
        let folder = TemporaryFolder::new("copied-file");
        let original = fixtures_folder().join("sine-440hz-mono.wav");

        let mut first = Session::start(Some(folder.open_cache())).unwrap();
        first.add(scan(
            std::slice::from_ref(&original),
            first.cache().as_deref(),
        ));
        wait_until_idle(&mut first);
        drop(first);

        let copy = folder.0.join("copy.wav");
        std::fs::copy(&original, &copy).unwrap();
        let mut second = Session::start(Some(folder.open_cache())).unwrap();
        let found = scan(std::slice::from_ref(&copy), second.cache().as_deref());
        assert!(found.cached.is_empty());
        assert_eq!(second.add(found), 1);
        wait_until_idle(&mut second);
        assert_eq!(second.cached_count(), 1);
        assert!(second.library().tracks()[0].analysis().is_some());
    }
}
