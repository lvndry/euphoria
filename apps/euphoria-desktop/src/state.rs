// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The library together with the analysis that fills it in. Nothing here
//! needs a window.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex, PoisonError};

use euphoria_core::{
    CachedAnalyzer, EntryCheck, EuphoriaError, LockedTransition, PlaylistStore, StoredFavorites,
    StoredPlaylist, TrackAnalysis, locate_moved_files,
};

use crate::analysis::{AnalysisEvent, AnalysisWorkers, StopSignal};
use crate::demo::DemoTrack;
use crate::favorites::Favorites;
use crate::library::{Library, TrackFailure};
use crate::playlists::{PlaylistRewrite, Playlists};
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
    /// The cues the user set for those files.
    pub(crate) edited_cues: HashMap<PathBuf, Vec<euphoria_core::Cue>>,
    /// One message for each path that could not be read.
    pub(crate) problems: Vec<String>,
    /// The paths that are no longer there, such as a folder on a drive that
    /// is not connected, kept so they are looked for again next time.
    pub(crate) unavailable_folders: Vec<PathBuf>,
}

/// Asks the core for the audio files under each path and, with a cache, for
/// the analyses it already holds for them. A path that cannot be read is
/// reported in [`Scan::problems`] and does not stop the others.
///
/// This walks folders on disk, so call it off the main thread.
pub(crate) fn scan(paths: &[PathBuf], cache: Option<&CachedAnalyzer>) -> Scan {
    scan_counting(paths, cache, &std::sync::atomic::AtomicUsize::new(0))
}

/// [`scan`], adding one to `found_so_far` for each audio file as the walk
/// finds it.
pub(crate) fn scan_counting(
    paths: &[PathBuf],
    cache: Option<&CachedAnalyzer>,
    found_so_far: &std::sync::atomic::AtomicUsize,
) -> Scan {
    let mut scan = Scan::default();
    for path in paths {
        match euphoria_core::scan_audio_files_counting(std::slice::from_ref(path), found_so_far) {
            Ok(found) => {
                if path.is_dir() {
                    scan.folders.push(path.clone());
                }
                scan.problems.extend(
                    [found.unreadable_summary(), found.skipped_summary()]
                        .into_iter()
                        .flatten()
                        .map(|summary| format!("{summary}.")),
                );
                scan.files.extend(found.files);
            }
            Err(error) if error.missing_path().is_some() => {
                scan.unavailable_folders.push(path.clone());
                scan.problems.push(format!(
                    "{} is missing: moved, deleted, or on a drive that is not connected.",
                    path.display()
                ));
            }
            Err(error) => scan.problems.push(error.chain()),
        }
    }
    if let Some(cache) = cache {
        scan.files.retain(|file| {
            match cache.edited_cues_for(file) {
                Ok(Some(cues)) => {
                    scan.edited_cues.insert(file.clone(), cues);
                }
                Ok(None) => {}
                Err(error) => {
                    // An unread saved set must not become an editable automatic set.
                    scan.problems.push(format!(
                        "cannot read saved cues for {}: {}",
                        file.display(),
                        error.chain()
                    ));
                    return false;
                }
            }
            if let Some(analysis) = cache.stored_analysis(file) {
                scan.cached.insert(file.clone(), analysis);
            }
            if let Ok(Some(correction)) = cache.correction_for(file) {
                scan.corrections.insert(file.clone(), correction);
            }
            true
        });
    }
    scan
}

/// What [`Session::relink`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Relinked {
    /// The track and everything that held it moved. The playlists or the
    /// favorites are worth saving when they changed.
    Done { saved_lists_changed: bool },
    /// A playlist or a favorite recorded other audio for the old path, so
    /// nothing moved.
    OtherAudioRecorded,
}

/// The library and the background workers analyzing it.
pub(crate) struct Session {
    library: Library,
    progress: AnalysisProgress,
    workers: AnalysisWorkers,
    events: Receiver<AnalysisEvent>,
    cache: Option<Arc<CachedAnalyzer>>,
    cached_count: usize,
    fresh_analyses: Vec<PathBuf>,
    cache_problem: Option<String>,
    /// Files queued to be analyzed again, until their result or skip arrives.
    reanalyzing: HashSet<PathBuf>,
    playlists: Playlists,
    favorites: Favorites,
    /// The transitions the user placed by hand in the planner.
    locked_transitions: Vec<LockedTransition>,
    playlist_saver: Option<PlaylistSaver>,
}

/// The playlists database, shared with the tasks that save to it.
#[derive(Clone)]
pub(crate) struct PlaylistSaver {
    store: Arc<Mutex<PlaylistStore>>,
    /// The revision of the playlists last written, so a slow save of older
    /// playlists never overwrites a newer one.
    saved_revision: Arc<AtomicU64>,
    /// The revision of the favorites last written, as for the playlists.
    saved_favorites_revision: Arc<AtomicU64>,
    rewrites: Arc<Mutex<Vec<PlaylistRewrite>>>,
    registered: Arc<Mutex<HashMap<PathBuf, Option<euphoria_core::ContentHash>>>>,
}

impl PlaylistSaver {
    /// Registers the live paths before hashing or saving them off-thread,
    /// including new entries not yet present in the database.
    pub(crate) fn register(&self, playlists: &Playlists, favorites: &Favorites) {
        *self
            .registered
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = playlists
            .all()
            .iter()
            .flat_map(|playlist| &playlist.tracks)
            .map(|path| (path.clone(), playlists.content_of(path)))
            .chain(
                favorites
                    .paths()
                    .map(|path| (path.clone(), favorites.content_of(path))),
            )
            .collect();
    }

    /// Keeps `favorites`, made at `revision`, unless newer ones were kept
    /// already.
    pub(crate) fn save_favorites(
        &self,
        favorites: &StoredFavorites,
        revision: u64,
        rewrite_epoch: usize,
    ) -> Result<(), EuphoriaError> {
        let mut store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
        if self.saved_favorites_revision.load(Ordering::Relaxed) >= revision {
            return Ok(());
        }
        let mut favorites = favorites.clone();
        let rewrites = self.rewrites.lock().unwrap_or_else(PoisonError::into_inner);
        for entry in favorites.entries_mut() {
            for rewrite in rewrites.iter().skip(rewrite_epoch) {
                rewrite.apply(entry);
            }
        }
        store.save_favorites(&favorites)?;
        self.saved_favorites_revision
            .store(revision, Ordering::Relaxed);
        Ok(())
    }

    /// Keeps `lock`, replacing the lock of the same pair.
    pub(crate) fn lock_transition(&self, lock: &LockedTransition) -> Result<(), EuphoriaError> {
        self.store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .lock_transition(lock)
    }

    /// Forgets the lock of `from` into `to`.
    pub(crate) fn unlock_transition(&self, from: &Path, to: &Path) -> Result<(), EuphoriaError> {
        self.store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .unlock_transition((from, None), (to, None))
            .map(|_| ())
    }

    pub(crate) fn rewrites(&self) -> Vec<PlaylistRewrite> {
        self.rewrites
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Keeps `playlists`, made at `revision`, unless newer ones were kept
    /// already.
    pub(crate) fn save(
        &self,
        playlists: &[StoredPlaylist],
        revision: u64,
        rewrite_epoch: usize,
    ) -> Result<(), EuphoriaError> {
        let mut store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
        if self.saved_revision.load(Ordering::Relaxed) >= revision {
            return Ok(());
        }
        let mut playlists = playlists.to_vec();
        let rewrites = self.rewrites.lock().unwrap_or_else(PoisonError::into_inner);
        for entry in playlists
            .iter_mut()
            .flat_map(|playlist| &mut playlist.entries)
        {
            for rewrite in rewrites.iter().skip(rewrite_epoch) {
                rewrite.apply(entry);
            }
        }
        store.save(&playlists)?;
        self.saved_revision.store(revision, Ordering::Relaxed);
        Ok(())
    }

    pub(crate) fn rebind(&self, rewrite: &mut PlaylistRewrite) -> Result<(), EuphoriaError> {
        let mut store = self.store.lock().unwrap_or_else(PoisonError::into_inner);
        let rebound = store.rebind_rewritten_file(&rewrite.path, rewrite.before, rewrite.after);
        if let Ok(entries) = &rebound {
            for (before, after) in entries {
                rewrite
                    .paths
                    .insert(before.path.clone(), after.path.clone());
            }
        }
        let registered = self
            .registered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        for (path, content) in registered {
            if content.is_some_and(|content| content != rewrite.before) {
                continue;
            }
            if content.is_none() && !path.try_exists().unwrap_or(false) && path != rewrite.path {
                continue;
            }
            let mut entry = euphoria_core::PlaylistEntry {
                path: path.clone(),
                content: rewrite.before,
            };
            if entry.rebind_rewritten_file(&rewrite.path, rewrite.before, rewrite.after) {
                rewrite.paths.insert(path, entry.path);
            }
        }
        {
            let mut registered = self
                .registered
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            for (before, after) in &rewrite.paths {
                if registered
                    .get(before)
                    .is_some_and(|content| content.is_none_or(|value| value == rewrite.before))
                {
                    registered.remove(before);
                    registered.insert(after.clone(), Some(rewrite.after));
                }
            }
        }
        self.rewrites
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(rewrite.clone());
        rebound.map(|_| ())
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
            fresh_analyses: Vec::new(),
            cache_problem: None,
            reanalyzing: HashSet::new(),
            playlists: Playlists::default(),
            favorites: Favorites::default(),
            locked_transitions: Vec::new(),
            playlist_saver: None,
        })
    }

    /// Starts with workers that answer with `stub` instead of reading audio.
    #[cfg(test)]
    pub(crate) fn start_stubbed(
        stub: crate::analysis::StubAnalysis,
    ) -> Result<Self, rayon::ThreadPoolBuildError> {
        let mut session = Self::start(None)?;
        let (workers, events) = AnalysisWorkers::start_stubbed(stub)?;
        session.workers = workers;
        session.events = events;
        Ok(session)
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
            store.save(&self.playlists.stored_with_contents(|_| Ok(None))?)?;
        }
        let mut favorites = store.load_favorites()?;
        let favorites_moved = self
            .cache
            .as_deref()
            .is_some_and(|cache| favorites.locate_moved_files(Some(cache), EntryCheck::Quick));
        if favorites_moved {
            store.save_favorites(&favorites)?;
        }
        self.favorites = Favorites::from_stored(favorites);
        self.locked_transitions = store.load_locked_transitions()?;
        self.show_missing_entries();
        let saver = PlaylistSaver {
            store: Arc::new(Mutex::new(store)),
            saved_revision: Arc::new(AtomicU64::new(0)),
            saved_favorites_revision: Arc::new(AtomicU64::new(0)),
            rewrites: Arc::default(),
            registered: Arc::default(),
        };
        self.playlist_saver = Some(saver);
        Ok(())
    }

    /// Adds a missing track for each file of a playlist or a favorite that
    /// is no longer there, even where the cache last saw it, so the user
    /// can locate it or remove it.
    fn show_missing_entries(&mut self) {
        let gone: Vec<PathBuf> = self
            .playlists
            .all()
            .iter()
            .flat_map(|playlist| playlist.tracks.iter())
            .chain(self.favorites.paths())
            .filter(|path| matches!(path.try_exists(), Ok(false)))
            .cloned()
            .collect();
        self.library.add_missing(gone);
    }

    /// The content a missing track held, as far as the playlists and
    /// favorites know it without the cache.
    pub(crate) fn known_content_of(&self, path: &Path) -> Option<euphoria_core::ContentHash> {
        self.playlists
            .content_of(path)
            .or_else(|| self.favorites.content_of(path))
    }

    /// Points the missing track of `old`, with its playlists and favorites,
    /// at `new`, where the same audio, `content`, is now. When `new` is
    /// already in the library the missing track is removed instead, its
    /// playlists and favorites pointing at the track that is there.
    ///
    /// Nothing changes when the playlists or the favorites recorded other
    /// audio for `old`, so the track, its playlists and its favorites never
    /// end up in different places.
    pub(crate) fn relink(
        &mut self,
        old: &Path,
        new: &Path,
        content: euphoria_core::ContentHash,
    ) -> Relinked {
        let recorded = [
            self.playlists.content_of(old),
            self.favorites.content_of(old),
        ];
        if recorded
            .iter()
            .flatten()
            .any(|recorded| *recorded != content)
        {
            return Relinked::OtherAudioRecorded;
        }
        if !self.library.relink(old, new.to_path_buf()) {
            self.library.remove(old);
        }
        let playlists_changed = self.playlists.relink(old, new, content);
        let favorites_changed = self.favorites.relink(old, new, content);
        Relinked::Done {
            saved_lists_changed: playlists_changed || favorites_changed,
        }
    }

    /// Takes the missing tracks among `paths` out of the library, the
    /// playlists and the favorites. Returns the paths taken out.
    pub(crate) fn remove_missing(&mut self, paths: &[PathBuf]) -> Vec<PathBuf> {
        let missing: Vec<PathBuf> = paths
            .iter()
            .filter(|path| {
                self.library
                    .track_by_path(path)
                    .is_some_and(crate::library::Track::is_missing)
            })
            .cloned()
            .collect();
        for path in &missing {
            self.library.remove(path);
        }
        self.playlists.forget(&missing);
        self.favorites.forget(&missing);
        missing
    }

    /// The playlists.
    pub(crate) fn playlists(&self) -> &Playlists {
        &self.playlists
    }

    /// The playlists, to change. The caller saves them afterwards.
    pub(crate) fn playlists_mut(&mut self) -> &mut Playlists {
        &mut self.playlists
    }

    /// The starred tracks and marked transitions.
    pub(crate) fn favorites(&self) -> &Favorites {
        &self.favorites
    }

    /// The favorites, to change. The caller saves them afterwards.
    pub(crate) fn favorites_mut(&mut self) -> &mut Favorites {
        &mut self.favorites
    }

    /// The transitions the user placed by hand in the planner.
    pub(crate) fn locked_transitions(&self) -> &[LockedTransition] {
        &self.locked_transitions
    }

    /// The locked transitions, to change. The caller saves the change.
    pub(crate) fn locked_transitions_mut(&mut self) -> &mut Vec<LockedTransition> {
        &mut self.locked_transitions
    }

    /// Where playlists are kept, when they are.
    pub(crate) fn playlist_saver(&self) -> Option<PlaylistSaver> {
        self.playlist_saver.clone()
    }

    /// The tracks added so far.
    pub(crate) fn library(&self) -> &Library {
        &self.library
    }

    #[cfg(test)]
    pub(crate) fn library_mut(&mut self) -> &mut Library {
        &mut self.library
    }

    #[cfg(test)]
    pub(crate) fn pretend_finished(&mut self) {
        self.progress.finish_one();
    }

    #[cfg(test)]
    pub(crate) fn pretend_queued(&mut self, file_count: usize) {
        self.progress.enqueue(file_count);
    }

    #[cfg(test)]
    pub(crate) fn pretend_started_seconds_ago(&mut self, seconds: u64) {
        self.progress.backdate(seconds);
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

    /// The files analyzed from scratch since the last call: not served from
    /// the cache, and not failed.
    pub(crate) fn take_fresh_analyses(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.fresh_analyses)
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
        let revived: Vec<PathBuf> = scan
            .files
            .iter()
            .filter(|file| self.library.revive(file))
            .cloned()
            .collect();
        let mut new_files = self.library.add_files(scan.files);
        new_files.extend(revived);
        for file in new_files {
            match scan.cached.remove(&file) {
                Some(analysis) => {
                    self.library.record_outcome(&file, Ok(analysis));
                    self.cached_count += 1;
                }
                None => files_to_analyze.push(file.clone()),
            }
            // A scan can finish after an edit in the window. Only new
            // tracks need its snapshot; existing tracks already hold the
            // current edits, including resets to the automatic values.
            if let Some(correction) = scan.corrections.remove(&file) {
                self.library.set_correction(&file, correction);
            }
            if let Some(cues) = scan.edited_cues.remove(&file) {
                self.library.set_edited_cues(&file, Some(cues));
            }
        }
        let queued = files_to_analyze.len();
        self.progress.enqueue(queued);
        self.workers.enqueue(files_to_analyze);
        queued
    }

    /// Queues the tracks at `paths` for analysis again, ignoring what the
    /// cache holds. Only the analysis is replaced: corrections, edited
    /// cues, stars and playlists live apart from it. Tracks already waiting
    /// or being analyzed, missing tracks and files not in the library are
    /// left out. Returns how many were queued.
    pub(crate) fn analyze_again(&mut self, paths: &[PathBuf]) -> usize {
        let mut queued = Vec::new();
        for path in paths {
            if !self.reanalyzing.contains(path) && self.library.requeue_for_analysis(path) {
                self.reanalyzing.insert(path.clone());
                queued.push(path.clone());
            }
        }
        let count = queued.len();
        self.progress.enqueue(count);
        self.workers.enqueue_fresh(queued);
        count
    }

    /// Stops the files still waiting for a worker. Files being analyzed
    /// finish, and tracks that were skipped can be analyzed again.
    pub(crate) fn cancel_queued(&mut self) {
        self.workers.cancel_queued();
    }

    /// Sets the key and tempo the user chose for the track at `path`, in the
    /// library at once. Keeping it in the cache is
    /// [`CachedAnalyzer::set_correction`], which reads the file and so runs
    /// off the main thread.
    pub(crate) fn set_correction(&mut self, path: &Path, correction: euphoria_core::Correction) {
        self.library.set_correction(path, correction);
    }

    /// Sets the cues the user chose for the track at `path`, or goes back to
    /// the automatic ones with `None`, in the library at once. Keeping them
    /// in the cache is [`CachedAnalyzer::set_edited_cues`], off the main
    /// thread.
    pub(crate) fn set_edited_cues(&mut self, path: &Path, cues: Option<Vec<euphoria_core::Cue>>) {
        self.library.set_edited_cues(path, cues);
    }

    /// Shows every tempo in `range`, without analyzing anything again.
    pub(crate) fn set_tempo_range(&mut self, range: euphoria_core::TempoRange) {
        self.library.set_tempo_range(range);
    }

    /// Takes the titles, artists and key fields read from the files' tags.
    pub(crate) fn set_track_names(&mut self, names: Vec<(PathBuf, euphoria_core::TrackName)>) {
        for (path, name) in names {
            self.library.set_names(&path, name);
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
            self.library
                .record_outcome(&track.path, track.outcome.map_err(TrackFailure::Failed));
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
            AnalysisEvent::Skipped(path) => {
                self.reanalyzing.remove(&path);
                self.library.mark_stopped(&path);
                self.progress.finish_one();
                return;
            }
            AnalysisEvent::Finished(file) => *file,
        };
        self.reanalyzing.remove(&file.path);
        if !file.from_cache && file.outcome.is_ok() {
            self.fresh_analyses.push(file.path.clone());
        }
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
    #[test]
    fn queued_playlist_snapshots_follow_frozen_rewrites_without_rebinding_later_missing_copies() {
        use crate::tag_writing::testing::ScratchFolder;
        use euphoria_core::{ContentHash, PlaylistEntry};
        let folder = ScratchFolder::new("playlist-queued-rewrite");
        let path = folder.0.join("written.mp3");
        let untouched = folder.0.join("copy.mp3");
        let missing = folder.0.join("moved.mp3");
        std::fs::write(&path, b"before").unwrap();
        std::fs::copy(&path, &untouched).unwrap();
        let before = ContentHash::of_file(&path).unwrap();
        let stored = vec![StoredPlaylist {
            name: "Queued".into(),
            label: None,
            entries: [path.clone(), missing.clone(), untouched.clone()]
                .into_iter()
                .map(|path| PlaylistEntry {
                    path,
                    content: before,
                })
                .collect(),
        }];
        let mut session = Session::start(None).unwrap();
        session
            .use_playlist_store(PlaylistStore::open(&folder.0.join("playlists.sqlite3")).unwrap())
            .unwrap();
        session.playlists = Playlists::from_stored(stored.clone());
        let saver = session.playlist_saver().unwrap();
        saver.register(&session.playlists, &session.favorites);
        let stale_contents = session.playlists.known_contents();
        std::fs::write(&path, b"after metadata edit").unwrap();
        let after = ContentHash::of_file(&path).unwrap();
        let mut rewrite = PlaylistRewrite {
            path: path.clone(),
            before,
            after,
            paths: HashMap::from([(path.clone(), path.clone())]),
        };
        saver.rebind(&mut rewrite).unwrap();
        // This copy existed at the rewrite boundary. Its later disappearance
        // must not reinterpret the already-observed transition.
        std::fs::remove_file(&untouched).unwrap();
        saver.save(&stored, 1, 0).unwrap();
        let kept = PlaylistStore::open(&folder.0.join("playlists.sqlite3"))
            .unwrap()
            .load()
            .unwrap();
        assert_eq!(kept[0].entries[0].content, after);
        assert_eq!(kept[0].entries[1].path, path);
        assert_eq!(kept[0].entries[1].content, after);
        assert_eq!(kept[0].entries[2].content, before);
        session.playlists.follow_rewrite_history(&saver.rewrites());
        session.playlists.learn_contents(stale_contents);
        assert_eq!(session.playlists.content_of(&path), Some(after));
        assert_eq!(session.playlists.content_of(&missing), None);
        // A second transition and late old completion cannot roll back the
        // current in-memory identity or an already-queued snapshot.
        std::fs::write(&path, b"another metadata edit").unwrap();
        let newest = ContentHash::of_file(&path).unwrap();
        let mut next = PlaylistRewrite {
            path: path.clone(),
            before: after,
            after: newest,
            paths: HashMap::from([(path.clone(), path.clone())]),
        };
        saver.rebind(&mut next).unwrap();
        saver.save(&stored, 2, 0).unwrap();
        session.playlists.follow_rewrite_history(&saver.rewrites());
        session
            .playlists
            .learn_contents(HashMap::from([(path.clone(), before)]));
        assert_eq!(session.playlists.content_of(&path), Some(newest));
        let kept = PlaylistStore::open(&folder.0.join("playlists.sqlite3"))
            .unwrap()
            .load()
            .unwrap();
        assert_eq!(kept[0].entries[0].content, newest);
        // A genuinely new playlist created after those rewrites must not
        // replay them onto freshly hashed content restored by another tool.
        for id in session
            .playlists
            .all()
            .iter()
            .map(|playlist| playlist.id)
            .collect::<Vec<_>>()
        {
            session.playlists.delete(id);
        }
        assert_eq!(session.playlists.content_of(&path), None);
        std::fs::write(&path, b"before").unwrap();
        let id = session.playlists.create();
        session
            .playlists
            .add_tracks(id, std::slice::from_ref(&path));
        let epoch = saver.rewrites().len();
        saver.register(&session.playlists, &session.favorites);
        let fresh = session
            .playlists
            .stored_with_contents(|path| Ok(Some(ContentHash::of_file(path).unwrap())))
            .unwrap();
        saver.save(&fresh, 3, epoch).unwrap();
        session.playlists.follow_rewrite_history(&saver.rewrites());
        assert_eq!(session.playlists.content_of(&path), Some(before));
        let kept = PlaylistStore::open(&folder.0.join("playlists.sqlite3"))
            .unwrap()
            .load()
            .unwrap();
        assert_eq!(kept[0].entries[0].content, before);
    }
    use std::path::Path;
    use std::time::{Duration, Instant};

    use euphoria_core::Cache;

    use super::*;
    use crate::analysis::FinishedFile;
    use crate::library::TrackStatus;

    fn fixtures_folder() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/euphoria-core/tests/fixtures")
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
    fn scanning_names_the_audio_files_it_cannot_decode_in_a_notice() {
        let folder =
            std::env::temp_dir().join(format!("euphoria-desktop-skipped-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        for name in ["a.opus", "b.opus", "c.wma", "kept.mp3"] {
            std::fs::write(folder.join(name), b"").unwrap();
        }

        let found = scan(std::slice::from_ref(&folder), None);
        std::fs::remove_dir_all(&folder).unwrap();

        assert_eq!(found.files, vec![folder.join("kept.mp3")]);
        assert_eq!(
            found.problems,
            vec!["3 files skipped: 2 .opus, 1 .wma, which Euphoria can't decode yet."]
        );
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
        assert!(found.problems[0].starts_with("/definitely/not/here is missing"));
        assert_eq!(
            found.unavailable_folders,
            vec![PathBuf::from("/definitely/not/here")]
        );
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
    fn a_confirmation_kept_in_the_cache_comes_back_and_changes_no_value() {
        let folder = TemporaryFolder::new("confirmations");
        let track = folder.0.join("track.wav");
        std::fs::copy(fixtures_folder().join("sine-440hz-mono.wav"), &track).unwrap();
        let confirmed = euphoria_core::Correction::default()
            .confirming_key()
            .confirming_bpm();
        let (detected_key, detected_bpm) = {
            let cache = folder.open_cache();
            let mut session = Session::start(Some(cache.clone())).unwrap();
            session.add(scan(std::slice::from_ref(&folder.0), Some(&cache)));
            wait_until_idle(&mut session);
            session.set_correction(&track, confirmed);
            cache.set_correction(&track, &confirmed).unwrap();
            let analyzed = &session.library().tracks()[0];
            (analyzed.key(), analyzed.bpm())
        };

        let cache = folder.open_cache();
        let found = scan(std::slice::from_ref(&folder.0), Some(&cache));
        assert_eq!(found.corrections.get(&track), Some(&confirmed));
        let mut session = Session::start(Some(cache)).unwrap();
        session.add(found);
        let reopened = &session.library().tracks()[0];
        assert_eq!(reopened.correction, confirmed);
        assert_eq!(
            (reopened.key(), reopened.bpm()),
            (detected_key, detected_bpm)
        );
        assert!(reopened.review_reasons().is_empty());
    }

    #[test]
    fn a_late_scan_does_not_replace_cues_edited_or_reset_since_it_started() {
        let path = PathBuf::from("/tracks/edited.wav");
        let cue = |name: &str| euphoria_core::Cue {
            seconds: 0.5,
            name: name.to_string(),
            hot_cue: None,
            energy: None,
        };
        for current in [Some(vec![cue("New edit")]), None] {
            let mut session = Session::start(None).unwrap();
            let mut track = crate::demo::tracks(1).remove(0);
            track.path = path.clone();
            session.add_demo_tracks(vec![track]);
            let mut pending = Scan {
                files: vec![path.clone()],
                ..Scan::default()
            };
            pending
                .edited_cues
                .insert(path.clone(), vec![cue("Before")]);

            session.set_edited_cues(&path, current.clone());
            session.add(pending);

            assert_eq!(
                session.library().track_by_path(&path).unwrap().edited_cues,
                current
            );
        }
    }

    #[test]
    fn a_late_scan_does_not_replace_a_correction_edited_or_reset_since_it_started() {
        let path = PathBuf::from("/tracks/corrected.wav");
        for current in [
            euphoria_core::Correction::of_bpm(90.0).unwrap(),
            euphoria_core::Correction::default(),
        ] {
            let mut session = Session::start(None).unwrap();
            let mut track = crate::demo::tracks(1).remove(0);
            track.path = path.clone();
            session.add_demo_tracks(vec![track]);
            let mut pending = Scan {
                files: vec![path.clone()],
                ..Scan::default()
            };
            pending.corrections.insert(
                path.clone(),
                euphoria_core::Correction::of_bpm(120.0).unwrap(),
            );

            session.set_correction(&path, current);
            session.add(pending);

            assert_eq!(
                session.library().track_by_path(&path).unwrap().correction,
                current
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_saved_cues_are_reported_and_retried_without_replacing_edits() {
        use std::os::unix::fs::PermissionsExt;

        let folder = TemporaryFolder::new("unreadable-cues");
        let track = folder.0.join("track.wav");
        std::fs::copy(fixtures_folder().join("sine-440hz-mono.wav"), &track).unwrap();
        let mine = vec![euphoria_core::Cue {
            seconds: 0.5,
            name: "Mine".to_string(),
            hot_cue: None,
            energy: None,
        }];
        {
            let cache = folder.open_cache();
            cache.analyze_file(&track).unwrap();
            cache.set_edited_cues(&track, Some(&mine)).unwrap();
        }
        let cache_path = folder.0.join("analysis-cache.sqlite3");
        euphoria_core::clear_cache(&cache_path).unwrap();
        let cache = folder.open_cache();
        assert!(cache.stored_analysis(&track).is_none());
        assert_eq!(cache.edited_cues_for(&track).unwrap(), Some(mine.clone()));
        let unreadable_scan = || {
            let permissions = std::fs::metadata(&track).unwrap().permissions();
            std::fs::set_permissions(&track, std::fs::Permissions::from_mode(0o000)).unwrap();
            let found = scan(std::slice::from_ref(&track), Some(&cache));
            std::fs::set_permissions(&track, permissions).unwrap();
            found
        };

        let failed = unreadable_scan();
        assert_eq!(failed.problems.len(), 1, "{failed:?}");
        assert!(failed.problems[0].contains(&track.display().to_string()));
        assert!(failed.files.is_empty());
        let mut session = Session::start(Some(cache.clone())).unwrap();
        assert_eq!(session.add(failed), 0);
        assert!(session.library().track_by_path(&track).is_none());

        let retried = scan(std::slice::from_ref(&track), Some(&cache));
        assert!(retried.problems.is_empty());
        assert_eq!(retried.edited_cues.get(&track), Some(&mine));
        assert_eq!(session.add(retried), 1);
        wait_until_idle(&mut session);
        assert_eq!(
            session.library().track_by_path(&track).unwrap().cues().cues,
            mine
        );
        assert_eq!(cache.edited_cues_for(&track).unwrap(), Some(mine));

        let current = vec![euphoria_core::Cue {
            seconds: 0.75,
            name: "Current edit".to_string(),
            hot_cue: None,
            energy: None,
        }];
        session.set_edited_cues(&track, Some(current.clone()));
        euphoria_core::clear_cache(&cache_path).unwrap();
        let failed = unreadable_scan();
        assert_eq!(failed.problems.len(), 1);
        assert_eq!(session.add(failed), 0);
        assert_eq!(
            session.library().track_by_path(&track).unwrap().cues().cues,
            current
        );
    }

    #[test]
    fn cues_kept_in_the_cache_come_back_on_the_next_launch() {
        let folder = TemporaryFolder::new("edited-cues");
        let track = folder.0.join("track.wav");
        std::fs::copy(fixtures_folder().join("sine-440hz-mono.wav"), &track).unwrap();
        let cues = vec![euphoria_core::Cue {
            seconds: 0.5,
            name: "Mine".to_string(),
            hot_cue: euphoria_core::HotCue::from_letter('D'),
            energy: None,
        }];
        {
            let cache = folder.open_cache();
            let mut session = Session::start(Some(cache.clone())).unwrap();
            session.add(scan(std::slice::from_ref(&folder.0), Some(&cache)));
            wait_until_idle(&mut session);
            cache.set_edited_cues(&track, Some(&cues)).unwrap();
        }

        let cache = folder.open_cache();
        let found = scan(std::slice::from_ref(&folder.0), Some(&cache));
        assert_eq!(found.edited_cues.get(&track), Some(&cues));
        let mut session = Session::start(Some(cache)).unwrap();
        session.add(found);
        let reopened = session.library().tracks()[0].cues();
        assert!(reopened.is_edited());
        assert_eq!(reopened.cues, cues);
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
            outcome: Err("cannot open the file".into()),
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
    fn only_new_analysis_is_reported_as_fresh_not_cached_or_failed_results() {
        let mut session = Session::start(None).unwrap();
        session.stop_signal().skip_queued_files();
        let paths: Vec<PathBuf> = ["cached", "fresh", "failed"]
            .iter()
            .map(|name| PathBuf::from(format!("/music/{name}.mp3")))
            .collect();
        session.add(Scan {
            files: paths.clone(),
            ..Scan::default()
        });
        let finish = |session: &mut Session, path: &Path, outcome, from_cache| {
            session.apply(AnalysisEvent::Finished(Box::new(FinishedFile {
                path: path.to_path_buf(),
                outcome,
                from_cache,
                cache_problem: None,
            })));
        };
        let analysis = crate::tag_writing::testing::analysis();
        finish(&mut session, &paths[0], Ok(analysis.clone()), true);
        finish(&mut session, &paths[2], Err("broken".into()), false);
        assert!(session.take_fresh_analyses().is_empty());
        finish(&mut session, &paths[1], Ok(analysis), false);
        assert_eq!(session.take_fresh_analyses(), vec![paths[1].clone()]);
        assert!(session.take_fresh_analyses().is_empty());
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
