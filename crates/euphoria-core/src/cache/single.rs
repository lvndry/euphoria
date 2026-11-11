// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::Path;
use std::sync::{Mutex, PoisonError};

use super::batch::{Origin, cached_or_analyzed};
use super::identity::FileStamp;
use super::{Cache, FileLocation, Snapshot};
use crate::analysis::{TrackAnalysis, analyze_file};
use crate::error::EuphoriaError;

/// The analysis of one file through the cache.
#[derive(Debug)]
pub struct CachedAnalysis {
    /// What was found in the track.
    pub analysis: TrackAnalysis,
    /// True when the analysis was served from the cache.
    pub from_cache: bool,
    /// Why the analysis could not be stored, when storing failed. The
    /// analysis itself is complete either way.
    pub store_error: Option<EuphoriaError>,
}

/// Analyzes files one at a time through the cache, for callers that want
/// each result as soon as it is ready, such as an interface filling in a
/// list. It can be shared between threads.
///
/// Lookups use the analyses that were stored when the analyzer was created,
/// as a batch uses the ones stored when it starts. Each new analysis is
/// stored before [`CachedAnalyzer::analyze_file`] returns.
#[derive(Debug)]
pub struct CachedAnalyzer {
    snapshot: Snapshot,
    cache: Mutex<Cache>,
}

impl CachedAnalyzer {
    /// Reads the stored analyses from `cache`. Fails when the cache cannot be
    /// read.
    pub fn new(cache: Cache) -> Result<Self, EuphoriaError> {
        Ok(Self {
            snapshot: cache.snapshot()?,
            cache: Mutex::new(cache),
        })
    }

    /// The stored analysis of the file at `path`, when the file is at the
    /// path it was last seen at with the same size and modification time.
    ///
    /// This asks the file system about the file but does not read it, so it
    /// is quick enough to run over a whole library before any analysis
    /// starts. A file that moved or was touched gets `None` here and is still
    /// recognised by its content in [`CachedAnalyzer::analyze_file`].
    pub fn stored_analysis(&self, path: &Path) -> Option<TrackAnalysis> {
        let location = FileLocation {
            absolute_path: std::path::absolute(path).ok()?,
            stamp: FileStamp::of(path).ok()?,
        };
        self.snapshot.at_location(&location).cloned()
    }

    /// Returns the stored analysis of the file, or analyzes it and stores the
    /// result.
    ///
    /// A file that fails is not stored, so it is tried again next time. Nor
    /// is the analysis of a file that changed while it was being analyzed.
    pub fn analyze_file(&self, path: &Path) -> Result<CachedAnalysis, EuphoriaError> {
        self.analyze_file_with(path, analyze_file)
    }

    fn analyze_file_with(
        &self,
        path: &Path,
        analyze: impl Fn(&Path) -> Result<TrackAnalysis, EuphoriaError>,
    ) -> Result<CachedAnalysis, EuphoriaError> {
        let (analysis, origin, write) = cached_or_analyzed(path, &self.snapshot, analyze)?;
        let store_error = write.and_then(|write| {
            let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
            let database_path = cache.path().to_path_buf();
            cache
                .apply_write(&write)
                .map_err(super::cache_error(&database_path))
                .err()
        });
        Ok(CachedAnalysis {
            analysis,
            from_cache: matches!(origin, Origin::Cache),
            store_error,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::super::test_support::TemporaryFolder;
    use super::*;

    /// A stand in for the analyzer that counts how often it runs.
    struct CountingAnalyzer {
        runs: AtomicUsize,
    }

    impl CountingAnalyzer {
        fn new() -> Self {
            Self {
                runs: AtomicUsize::new(0),
            }
        }

        fn analyze(&self, path: &Path) -> Result<TrackAnalysis, EuphoriaError> {
            self.runs.fetch_add(1, Ordering::Relaxed);
            let content = std::fs::read(path).map_err(|source| EuphoriaError::Open {
                path: path.to_path_buf(),
                source,
            })?;
            if content.starts_with(b"broken") {
                return Err(EuphoriaError::EmptyAudio {
                    path: path.to_path_buf(),
                });
            }
            Ok(TrackAnalysis {
                duration_seconds: content.len() as f32,
                key: Some(crate::Key::new(content.len() as u8, crate::Mode::Minor)),
                bpm: Some(120.5),
                energy: Some((content.len() % 10) as u8 + 1),
                tuning_cents: None,
                key_runner_up: None,
                key_score_margin: None,
                tempo_estimate: None,
                tempo_score_margin: None,
                key_timeline: Vec::new(),
            })
        }

        fn runs(&self) -> usize {
            self.runs.load(Ordering::Relaxed)
        }
    }

    fn write_track(folder: &TemporaryFolder, name: &str, content: &str) -> PathBuf {
        let path = folder.path().join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn open_analyzer(folder: &TemporaryFolder) -> CachedAnalyzer {
        let cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        CachedAnalyzer::new(cache).unwrap()
    }

    #[test]
    fn a_new_file_is_analyzed_and_stored_for_the_next_analyzer() {
        let folder = TemporaryFolder::new();
        let track = write_track(&folder, "track.mp3", "some audio");
        let counter = CountingAnalyzer::new();

        let first = open_analyzer(&folder);
        assert_eq!(first.stored_analysis(&track), None);
        let analyzed = first
            .analyze_file_with(&track, |path| counter.analyze(path))
            .unwrap();
        assert!(!analyzed.from_cache);
        assert!(analyzed.store_error.is_none(), "{:?}", analyzed.store_error);
        drop(first);

        let second = open_analyzer(&folder);
        assert_eq!(
            second.stored_analysis(&track),
            Some(analyzed.analysis.clone())
        );
        let served = second
            .analyze_file_with(&track, |path| counter.analyze(path))
            .unwrap();
        assert!(served.from_cache);
        assert_eq!(served.analysis, analyzed.analysis);
        assert_eq!(counter.runs(), 1);
    }

    #[test]
    fn a_moved_file_is_not_found_by_path_but_is_served_by_content() {
        let folder = TemporaryFolder::new();
        let track = write_track(&folder, "before.mp3", "some audio");
        let counter = CountingAnalyzer::new();
        open_analyzer(&folder)
            .analyze_file_with(&track, |path| counter.analyze(path))
            .unwrap();
        let moved = folder.path().join("after.mp3");
        std::fs::rename(&track, &moved).unwrap();

        let analyzer = open_analyzer(&folder);
        assert_eq!(analyzer.stored_analysis(&moved), None);
        let served = analyzer
            .analyze_file_with(&moved, |path| counter.analyze(path))
            .unwrap();
        assert!(served.from_cache);
        assert!(served.store_error.is_none());
        assert_eq!(counter.runs(), 1);

        assert!(open_analyzer(&folder).stored_analysis(&moved).is_some());
    }

    #[test]
    fn a_changed_file_is_not_served_from_its_old_analysis() {
        let folder = TemporaryFolder::new();
        let track = write_track(&folder, "track.mp3", "some audio");
        let counter = CountingAnalyzer::new();
        open_analyzer(&folder)
            .analyze_file_with(&track, |path| counter.analyze(path))
            .unwrap();
        std::fs::write(&track, "different and longer audio").unwrap();

        let analyzer = open_analyzer(&folder);
        assert_eq!(analyzer.stored_analysis(&track), None);
        let analyzed = analyzer
            .analyze_file_with(&track, |path| counter.analyze(path))
            .unwrap();
        assert!(!analyzed.from_cache);
        assert_eq!(counter.runs(), 2);
    }

    #[test]
    fn a_failure_is_returned_and_not_stored() {
        let folder = TemporaryFolder::new();
        let track = write_track(&folder, "broken.mp3", "broken audio");
        let counter = CountingAnalyzer::new();

        let failed = open_analyzer(&folder).analyze_file_with(&track, |path| counter.analyze(path));
        assert!(matches!(failed, Err(EuphoriaError::EmptyAudio { .. })));

        let analyzer = open_analyzer(&folder);
        assert_eq!(analyzer.stored_analysis(&track), None);
        assert!(
            analyzer
                .analyze_file_with(&track, |path| counter.analyze(path))
                .is_err()
        );
        assert_eq!(counter.runs(), 2);
    }

    #[test]
    fn the_energy_rating_is_stored_and_served() {
        let folder = TemporaryFolder::new();
        let track = write_track(&folder, "track.mp3", "some audio");
        let counter = CountingAnalyzer::new();
        let analyzed = open_analyzer(&folder)
            .analyze_file_with(&track, |path| counter.analyze(path))
            .unwrap();
        assert_eq!(analyzed.analysis.energy, Some(1));

        let stored = open_analyzer(&folder).stored_analysis(&track).unwrap();
        assert_eq!(stored.energy, Some(1));
    }

    #[test]
    fn an_analysis_from_another_energy_model_is_ignored_and_replaced() {
        let folder = TemporaryFolder::new();
        let track = write_track(&folder, "track.mp3", "some audio");
        let counter = CountingAnalyzer::new();
        open_analyzer(&folder)
            .analyze_file_with(&track, |path| counter.analyze(path))
            .unwrap();
        let open_recalibrated = || {
            let mut cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
            cache.energy_model_version += 1;
            CachedAnalyzer::new(cache).unwrap()
        };

        let after_recalibration = open_recalibrated();
        assert_eq!(after_recalibration.stored_analysis(&track), None);
        let analyzed = after_recalibration
            .analyze_file_with(&track, |path| counter.analyze(path))
            .unwrap();
        assert!(!analyzed.from_cache);
        assert!(analyzed.store_error.is_none(), "{:?}", analyzed.store_error);
        drop(after_recalibration);

        assert!(open_recalibrated().stored_analysis(&track).is_some());
        assert_eq!(open_analyzer(&folder).stored_analysis(&track), None);
        assert_eq!(counter.runs(), 2);
    }

    #[test]
    fn a_missing_file_has_no_stored_analysis_and_fails_to_analyze() {
        let folder = TemporaryFolder::new();
        let analyzer = open_analyzer(&folder);
        let missing = folder.path().join("missing.mp3");
        assert_eq!(analyzer.stored_analysis(&missing), None);
        assert!(matches!(
            analyzer.analyze_file(&missing),
            Err(EuphoriaError::Scan { .. })
        ));
    }

    #[test]
    fn workers_on_several_threads_share_one_analyzer() {
        let folder = TemporaryFolder::new();
        let tracks: Vec<PathBuf> = (0..24)
            .map(|number| {
                write_track(
                    &folder,
                    &format!("track-{number}.mp3"),
                    &"audio ".repeat(number + 1),
                )
            })
            .collect();
        let counter = CountingAnalyzer::new();
        let analyzer = open_analyzer(&folder);

        std::thread::scope(|scope| {
            for worker_tracks in tracks.chunks(6) {
                let analyzer = &analyzer;
                let counter = &counter;
                scope.spawn(move || {
                    for track in worker_tracks {
                        let analyzed = analyzer
                            .analyze_file_with(track, |path| counter.analyze(path))
                            .unwrap();
                        assert!(analyzed.store_error.is_none());
                    }
                });
            }
        });
        drop(analyzer);

        let reopened = open_analyzer(&folder);
        assert!(
            tracks
                .iter()
                .all(|track| reopened.stored_analysis(track).is_some())
        );
        assert_eq!(counter.runs(), 24);
    }
}
