// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use rayon::prelude::*;

use super::identity::{ContentHash, FileStamp};
use super::{Cache, CacheWrite, FileLocation, Snapshot};
use crate::analysis::{
    AnalysisObserver, FileResult, Progress, TrackAnalysis, Unobserved, analyze_file_observed,
};
use crate::error::EuphoriaError;

/// Whether a batch may serve analyses that are already stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CachePolicy {
    /// Serve stored analyses and analyze only the files that have none.
    UseCached,
    /// Analyze every file again and overwrite what is stored.
    Reanalyze,
}

/// What a cached batch produced.
#[derive(Debug)]
pub struct CachedBatch {
    /// One result per file, in the order the files were given.
    pub results: Vec<FileResult>,
    /// How many of the results were served from the cache.
    pub cached_count: usize,
    /// Why storing stopped, when it did. The results are complete either way,
    /// but analyses finished after the failure were not stored.
    pub store_error: Option<EuphoriaError>,
}

/// Where the analysis of one file came from.
pub(super) enum Origin {
    Cache,
    Analyzer,
}

/// Like [`analyze_files`](crate::analysis::analyze_files), but serves files
/// the cache already knows and stores each new analysis as it completes.
///
/// A file that fails is reported in its result and is not stored, so it is
/// tried again on the next run. Nor is the analysis of a file that changed
/// while it was being analyzed, since it may not belong to the content that
/// was hashed. Lookups use the analyses stored when the
/// batch starts, so two identical files met for the first time in one batch
/// are both analyzed.
///
/// Fails before analyzing anything when the cache cannot be read. A failure
/// to write is reported in [`CachedBatch::store_error`] and does not discard
/// the results.
///
/// `on_progress` is called from worker threads once per finished file. The
/// batch runs on the current Rayon thread pool, and all database writes
/// happen on one further thread.
pub fn analyze_files_cached(
    cache: &mut Cache,
    paths: &[PathBuf],
    policy: CachePolicy,
    on_progress: impl Fn(Progress<'_>) + Sync,
) -> Result<CachedBatch, EuphoriaError> {
    analyze_files_cached_observed(cache, paths, policy, &Unobserved, on_progress)
}

/// Like [`analyze_files_cached`], telling `observer` the stages of each file
/// that is analyzed. Files served from the cache report no stage.
pub fn analyze_files_cached_observed(
    cache: &mut Cache,
    paths: &[PathBuf],
    policy: CachePolicy,
    observer: &impl AnalysisObserver,
    on_progress: impl Fn(Progress<'_>) + Sync,
) -> Result<CachedBatch, EuphoriaError> {
    let analyze = |path: &Path| analyze_file_observed(path, observer);
    analyze_files_cached_with(cache, paths, policy, analyze, on_progress)
}

fn analyze_files_cached_with(
    cache: &mut Cache,
    paths: &[PathBuf],
    policy: CachePolicy,
    analyze: impl Fn(&Path) -> Result<TrackAnalysis, EuphoriaError> + Sync,
    on_progress: impl Fn(Progress<'_>) + Sync,
) -> Result<CachedBatch, EuphoriaError> {
    let snapshot = match policy {
        CachePolicy::UseCached => cache.snapshot()?,
        CachePolicy::Reanalyze => Snapshot::default(),
    };
    let completed = AtomicUsize::new(0);
    let cached_count = AtomicUsize::new(0);

    let (results, store_outcome) = std::thread::scope(|scope| {
        let (writes, pending_writes) = mpsc::channel();
        let writer = scope.spawn(move || cache.apply_writes(pending_writes));
        let results: Vec<FileResult> = paths
            .par_iter()
            .map(|path| {
                let outcome = cached_or_analyzed(path, &snapshot, &analyze).map(
                    |(analysis, origin, write)| {
                        if matches!(origin, Origin::Cache) {
                            cached_count.fetch_add(1, Ordering::Relaxed);
                        }
                        if let Some(write) = write {
                            // Sending fails only after the writer has stopped
                            // on an error, and that error is the one the
                            // batch reports.
                            let _ = writes.send(write);
                        }
                        analysis
                    },
                );
                on_progress(Progress {
                    completed: completed.fetch_add(1, Ordering::Relaxed) + 1,
                    total: paths.len(),
                    path,
                });
                FileResult {
                    path: path.clone(),
                    outcome,
                }
            })
            .collect();
        drop(writes);
        let store_outcome = writer
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        (results, store_outcome)
    });

    Ok(CachedBatch {
        results,
        cached_count: cached_count.into_inner(),
        store_error: store_outcome.err(),
    })
}

/// The analysis of one file, where it came from, and the change to make to
/// the database for it, if any.
pub(super) fn cached_or_analyzed(
    path: &Path,
    snapshot: &Snapshot,
    analyze: impl Fn(&Path) -> Result<TrackAnalysis, EuphoriaError>,
) -> Result<(TrackAnalysis, Origin, Option<CacheWrite>), EuphoriaError> {
    let unreadable = |source| EuphoriaError::Scan {
        path: path.to_path_buf(),
        source,
    };
    let location = FileLocation {
        absolute_path: std::path::absolute(path).map_err(unreadable)?,
        stamp: FileStamp::of(path).map_err(missing_or_scan(path))?,
    };
    if let Some(analysis) = snapshot.at_location(&location) {
        return Ok((analysis.clone(), Origin::Cache, None));
    }

    let content_hash = ContentHash::of_file(path).map_err(missing_or_scan(path))?;
    let (analysis, origin, write) = match snapshot.of_content(&content_hash) {
        Some(analysis) => (
            analysis.clone(),
            Origin::Cache,
            CacheWrite::Relocate {
                content_hash,
                location,
            },
        ),
        None => {
            let analysis = analyze(path)?;
            let stamp_after_analysis = FileStamp::of(path).ok();
            if stamp_after_analysis != Some(location.stamp) {
                return Ok((analysis, Origin::Analyzer, None));
            }
            let write = CacheWrite::Store {
                content_hash,
                analysis: Box::new(analysis.clone()),
                location,
            };
            (analysis, Origin::Analyzer, write)
        }
    };
    Ok((analysis, origin, Some(write)))
}

/// The error for reading a file that is not there: a missing file when it
/// is gone, an unreadable one otherwise. A file the cache has relocated to
/// another path is read where it moved to, so this only says the file is
/// missing when nothing is left of it.
fn missing_or_scan(path: &Path) -> impl Fn(std::io::Error) -> EuphoriaError + '_ {
    move |source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            EuphoriaError::Missing {
                path: path.to_path_buf(),
            }
        } else {
            EuphoriaError::Scan {
                path: path.to_path_buf(),
                source,
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::time::{Duration, SystemTime};

    use super::super::test_support::TemporaryFolder;
    use super::*;

    /// A stand in for the analyzer that counts how often it runs and derives
    /// its answer from the file, so different contents give different results.
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
            let checksum: u32 = content.iter().map(|byte| u32::from(*byte)).sum();
            Ok(TrackAnalysis {
                duration_seconds: content.len() as f32,
                key: Some(crate::Key::new((checksum % 12) as u8, crate::Mode::Minor)),
                bpm: Some(60.0 + (checksum % 120) as f32 + 0.37),
                energy: Some((checksum % 10) as u8 + 1),
                tuning_cents: Some((checksum % 100) as f32 - 49.63),
                key_runner_up: None,
                key_score_margin: None,
                tempo_estimate: None,
                tempo_score_margin: None,
                beat_grid: None,
                cues: Vec::new(),
                key_timeline: vec![
                    crate::KeySection {
                        start_seconds: 0.0,
                        end_seconds: content.len() as f32 / 3.0,
                        key: crate::Key::new((checksum % 12) as u8, crate::Mode::Minor),
                    },
                    crate::KeySection {
                        start_seconds: content.len() as f32 / 3.0,
                        end_seconds: content.len() as f32,
                        key: crate::Key::new((checksum % 12) as u8, crate::Mode::Major),
                    },
                ],
                quality: Some(crate::quality::QualityReport {
                    codec: "pcm_s16le".to_string(),
                    lossless: true,
                    bitrate_kbps: Some(1_411),
                    sample_rate: 44_100,
                    bit_depth: Some(16),
                    channels: Some(2),
                    cutoff_hz: None,
                    clipped_share: 0.0,
                    integrated_loudness_lufs: Some(-9.0 - (checksum % 5) as f32),
                    true_peak_dbtp: Some(-0.5),
                    verdict: crate::quality::QualityVerdict::Lossless,
                }),
            })
        }

        fn runs(&self) -> usize {
            self.runs.load(Ordering::Relaxed)
        }
    }

    struct Library {
        folder: TemporaryFolder,
        cache: Cache,
        analyzer: CountingAnalyzer,
    }

    impl Library {
        fn new() -> Self {
            let folder = TemporaryFolder::new();
            let cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
            Self {
                folder,
                cache,
                analyzer: CountingAnalyzer::new(),
            }
        }

        fn add_track(&self, name: &str, content: &str) -> PathBuf {
            let path = self.folder.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, content).unwrap();
            path
        }

        fn analyze_with(&mut self, paths: &[PathBuf], policy: CachePolicy) -> CachedBatch {
            let batch = analyze_files_cached_with(
                &mut self.cache,
                paths,
                policy,
                |path| self.analyzer.analyze(path),
                |_| {},
            )
            .unwrap();
            assert!(batch.store_error.is_none(), "{:?}", batch.store_error);
            assert_eq!(batch.results.len(), paths.len());
            batch
        }

        fn analyze(&mut self, paths: &[PathBuf]) -> CachedBatch {
            self.analyze_with(paths, CachePolicy::UseCached)
        }
    }

    fn analyses(batch: &CachedBatch) -> Vec<&TrackAnalysis> {
        batch
            .results
            .iter()
            .map(|result| result.outcome.as_ref().unwrap())
            .collect()
    }

    fn set_modified_later(path: &Path) {
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(3600))
            .unwrap();
    }

    #[test]
    fn a_second_run_is_served_from_the_cache() {
        let mut library = Library::new();
        let tracks = [
            library.add_track("first.wav", "first track"),
            library.add_track("second.wav", "second track, longer"),
        ];

        let first_run = library.analyze(&tracks);
        let second_run = library.analyze(&tracks);

        assert_eq!(first_run.cached_count, 0);
        assert_eq!(second_run.cached_count, 2);
        assert_eq!(library.analyzer.runs(), 2);
        assert_eq!(analyses(&first_run), analyses(&second_run));
        assert_eq!(second_run.results[1].path, tracks[1]);
    }

    #[test]
    fn a_renamed_file_is_served_from_the_cache() {
        let mut library = Library::new();
        let original = library.add_track("original.wav", "a track");
        let first_run = library.analyze(std::slice::from_ref(&original));

        let renamed = original.with_file_name("renamed.wav");
        std::fs::rename(&original, &renamed).unwrap();
        let second_run = library.analyze(std::slice::from_ref(&renamed));

        assert_eq!(second_run.cached_count, 1);
        assert_eq!(library.analyzer.runs(), 1);
        assert_eq!(analyses(&first_run), analyses(&second_run));
    }

    #[test]
    fn a_moved_file_is_served_from_the_cache_and_its_new_path_is_recorded() {
        let mut library = Library::new();
        let original = library.add_track("inbox/track.wav", "a track");
        library.analyze(std::slice::from_ref(&original));

        let moved = library.add_track("sorted/house/placeholder", "");
        let moved = moved.with_file_name("track.wav");
        std::fs::rename(&original, &moved).unwrap();
        let second_run = library.analyze(std::slice::from_ref(&moved));

        assert_eq!(second_run.cached_count, 1);
        assert_eq!(library.analyzer.runs(), 1);
        let snapshot = library.cache.snapshot().unwrap();
        assert!(snapshot.locations.contains_key(&moved));
        assert!(!snapshot.locations.contains_key(&original));
        assert_eq!(library.cache.entry_count().unwrap(), 1);
    }

    #[test]
    fn a_touched_file_with_the_same_content_is_served_from_the_cache() {
        let mut library = Library::new();
        let track = library.add_track("track.wav", "a track");
        library.analyze(std::slice::from_ref(&track));

        set_modified_later(&track);
        let second_run = library.analyze(std::slice::from_ref(&track));

        assert_eq!(second_run.cached_count, 1);
        assert_eq!(library.analyzer.runs(), 1);
        let snapshot = library.cache.snapshot().unwrap();
        let (recorded_stamp, _) = snapshot.locations[&track];
        assert_eq!(recorded_stamp, FileStamp::of(&track).unwrap());
    }

    #[test]
    fn a_file_that_changes_during_its_analysis_is_not_stored() {
        let mut library = Library::new();
        let track = library.add_track("track.wav", "version one");
        let tracks = std::slice::from_ref(&track);

        let interrupted_run = analyze_files_cached_with(
            &mut library.cache,
            tracks,
            CachePolicy::UseCached,
            |path| {
                let analysis = library.analyzer.analyze(path);
                std::fs::write(path, "version two").unwrap();
                set_modified_later(path);
                analysis
            },
            |_| {},
        )
        .unwrap();

        assert!(interrupted_run.results[0].outcome.is_ok());
        assert!(interrupted_run.store_error.is_none());
        assert_eq!(library.cache.entry_count().unwrap(), 0);

        let next_run = library.analyze(tracks);
        let last_run = library.analyze(tracks);

        assert_eq!(next_run.cached_count, 0);
        assert_eq!(last_run.cached_count, 1);
        assert_eq!(library.analyzer.runs(), 2);
        assert_ne!(analyses(&interrupted_run), analyses(&next_run));
    }

    #[test]
    fn changed_content_at_the_same_path_is_analyzed_again() {
        let mut library = Library::new();
        let track = library.add_track("track.wav", "version one");
        let first_run = library.analyze(std::slice::from_ref(&track));

        std::fs::write(&track, "version two").unwrap();
        set_modified_later(&track);
        let second_run = library.analyze(std::slice::from_ref(&track));
        let third_run = library.analyze(std::slice::from_ref(&track));

        assert_eq!(second_run.cached_count, 0);
        assert_eq!(library.analyzer.runs(), 2);
        assert_ne!(analyses(&first_run), analyses(&second_run));
        assert_eq!(third_run.cached_count, 1);
        assert_eq!(analyses(&second_run), analyses(&third_run));
    }

    #[test]
    fn a_removed_file_is_reported_as_missing_and_not_reanalyzed() {
        let mut library = Library::new();
        let track = library.add_track("track.wav", "a track");
        let first_run = library.analyze(std::slice::from_ref(&track));
        assert!(first_run.results[0].outcome.is_ok());

        std::fs::remove_file(&track).unwrap();
        let second_run = library.analyze(std::slice::from_ref(&track));

        let missing = second_run.results[0]
            .outcome
            .as_ref()
            .err()
            .and_then(EuphoriaError::missing_path);
        assert_eq!(missing, Some(track.as_path()));
        // The file is gone, so the analyzer is not run for it.
        assert_eq!(library.analyzer.runs(), 1);
        // The stored analysis is kept, so the track is found again if the
        // file comes back.
        let restored = library.add_track("track.wav", "a track");
        let third_run = library.analyze(std::slice::from_ref(&restored));
        assert!(third_run.results[0].outcome.is_ok());
        assert_eq!(third_run.cached_count, 1);
        assert_eq!(library.analyzer.runs(), 1);
    }

    #[test]
    fn an_analysis_from_another_analyzer_version_is_ignored_and_replaced() {
        let mut library = Library::new();
        let track = library.add_track("track.wav", "a track");
        library.analyze(std::slice::from_ref(&track));

        library.cache.analyzer_version = "a later analyzer".to_string();
        let after_upgrade = library.analyze(std::slice::from_ref(&track));
        let next_run = library.analyze(std::slice::from_ref(&track));

        assert_eq!(after_upgrade.cached_count, 0);
        assert_eq!(next_run.cached_count, 1);
        assert_eq!(library.analyzer.runs(), 2);
        assert_eq!(library.cache.entry_count().unwrap(), 1);
    }

    #[test]
    fn earlier_timelines_are_recomputed_after_the_timeline_rule_changes() {
        for previous_version in [6, 7, 8] {
            let mut library = Library::new();
            let track = library.add_track("track.wav", "a track");
            let current_version = library.cache.analyzer_version.clone();
            let detector = if cfg!(feature = "libkeyfinder") {
                "libkeyfinder"
            } else {
                "stratum"
            };
            library.cache.analyzer_version = format!("{previous_version}+{detector}");
            library.analyze(std::slice::from_ref(&track));

            library.cache.analyzer_version = current_version;
            let after_upgrade = library.analyze(std::slice::from_ref(&track));
            let next_run = library.analyze(std::slice::from_ref(&track));

            assert_eq!(after_upgrade.cached_count, 0, "version {previous_version}");
            assert_eq!(next_run.cached_count, 1);
            assert_eq!(library.analyzer.runs(), 2);
        }
    }

    #[test]
    fn an_analysis_from_another_energy_model_is_ignored_and_replaced() {
        let mut library = Library::new();
        let track = library.add_track("track.wav", "a track");
        library.analyze(std::slice::from_ref(&track));

        library.cache.energy_model_version += 1;
        let after_recalibration = library.analyze(std::slice::from_ref(&track));
        let next_run = library.analyze(std::slice::from_ref(&track));

        assert_eq!(after_recalibration.cached_count, 0);
        assert_eq!(next_run.cached_count, 1);
        assert_eq!(library.analyzer.runs(), 2);
        assert_eq!(library.cache.entry_count().unwrap(), 1);
    }

    #[test]
    fn a_track_stored_by_a_version_1_database_is_analyzed_again_and_replaced() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        let track = folder.path().join("track.wav");
        std::fs::write(&track, "a track").unwrap();
        crate::cache::test_support::create_version_1_database(&database, &track);
        let mut library = Library {
            cache: Cache::open(&database).unwrap(),
            folder,
            analyzer: CountingAnalyzer::new(),
        };

        let after_upgrade = library.analyze(std::slice::from_ref(&track));
        let next_run = library.analyze(std::slice::from_ref(&track));

        assert_eq!(after_upgrade.cached_count, 0);
        assert_eq!(library.analyzer.runs(), 1);
        assert!(analyses(&after_upgrade)[0].energy.is_some());
        assert_eq!(next_run.cached_count, 1);
        assert_eq!(analyses(&after_upgrade), analyses(&next_run));
        assert_eq!(library.cache.entry_count().unwrap(), 1);
    }

    #[test]
    fn a_track_stored_by_a_version_4_database_is_analyzed_again_and_replaced() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        let track = folder.path().join("track.wav");
        std::fs::write(&track, "a track").unwrap();
        crate::cache::test_support::create_version_4_database(&database, &track);
        let mut library = Library {
            cache: Cache::open(&database).unwrap(),
            folder,
            analyzer: CountingAnalyzer::new(),
        };

        let after_upgrade = library.analyze(std::slice::from_ref(&track));
        let next_run = library.analyze(std::slice::from_ref(&track));

        assert_eq!(after_upgrade.cached_count, 0);
        assert_eq!(library.analyzer.runs(), 1);
        assert!(analyses(&after_upgrade)[0].quality.is_some());
        assert_eq!(next_run.cached_count, 1);
        assert_eq!(analyses(&after_upgrade), analyses(&next_run));
        assert_eq!(library.cache.entry_count().unwrap(), 1);
        let stored_version: String = library
            .cache
            .connection
            .query_row("SELECT analyzer_version FROM analyses", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(stored_version, crate::ANALYZER_VERSION);
    }

    #[test]
    fn failures_are_reported_and_not_cached() {
        let mut library = Library::new();
        let tracks = [
            library.add_track("broken.wav", "broken file"),
            library.add_track("fine.wav", "a track"),
            library.folder.path().join("missing.wav"),
        ];

        let first_run = library.analyze(&tracks);
        let second_run = library.analyze(&tracks);

        for run in [&first_run, &second_run] {
            assert!(matches!(
                run.results[0].outcome,
                Err(EuphoriaError::EmptyAudio { .. })
            ));
            assert!(run.results[1].outcome.is_ok());
            assert!(matches!(
                run.results[2].outcome,
                Err(EuphoriaError::Missing { .. })
            ));
        }
        assert_eq!(second_run.cached_count, 1);
        assert_eq!(library.analyzer.runs(), 3);
        assert_eq!(library.cache.entry_count().unwrap(), 1);
    }

    #[test]
    fn reanalyzing_ignores_and_overwrites_what_is_stored() {
        let mut library = Library::new();
        let track = library.add_track("track.wav", "a track");
        library.analyze(std::slice::from_ref(&track));

        let reanalyzed = library.analyze_with(std::slice::from_ref(&track), CachePolicy::Reanalyze);
        let next_run = library.analyze(std::slice::from_ref(&track));

        assert_eq!(reanalyzed.cached_count, 0);
        assert_eq!(library.analyzer.runs(), 2);
        assert_eq!(next_run.cached_count, 1);
        assert_eq!(library.cache.entry_count().unwrap(), 1);
    }

    #[test]
    fn a_parallel_batch_stores_every_result_and_reports_progress_once_per_file() {
        let mut library = Library::new();
        let tracks: Vec<PathBuf> = (0..300)
            .map(|number| {
                library.add_track(&format!("track-{number}.wav"), &format!("track {number}"))
            })
            .collect();
        let thread_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(8)
            .build()
            .unwrap();
        let progress_reports = AtomicUsize::new(0);

        let first_run = thread_pool.install(|| {
            analyze_files_cached_with(
                &mut library.cache,
                &tracks,
                CachePolicy::UseCached,
                |path| library.analyzer.analyze(path),
                |progress| {
                    assert_eq!(progress.total, tracks.len());
                    progress_reports.fetch_add(1, Ordering::Relaxed);
                },
            )
            .unwrap()
        });
        let second_run = thread_pool.install(|| library.analyze(&tracks));

        assert!(first_run.store_error.is_none());
        assert_eq!(progress_reports.into_inner(), tracks.len());
        assert_eq!(library.cache.entry_count().unwrap(), 300);
        assert_eq!(second_run.cached_count, 300);
        assert_eq!(library.analyzer.runs(), 300);
        assert_eq!(analyses(&first_run), analyses(&second_run));
        for (result, track) in second_run.results.iter().zip(&tracks) {
            assert_eq!(&result.path, track);
        }
    }

    #[test]
    fn a_failed_write_is_reported_without_losing_the_results() {
        let mut library = Library::new();
        let track = library.add_track("track.wav", "a track");
        library
            .cache
            .connection
            .execute_batch("DROP TABLE analyses")
            .unwrap();

        let batch = analyze_files_cached_with(
            &mut library.cache,
            std::slice::from_ref(&track),
            CachePolicy::Reanalyze,
            |path| library.analyzer.analyze(path),
            |_| {},
        )
        .unwrap();

        assert!(batch.results[0].outcome.is_ok());
        assert!(matches!(
            batch.store_error,
            Some(EuphoriaError::Cache { .. })
        ));
    }

    #[test]
    fn real_analyses_survive_the_round_trip_through_the_database() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let tracks = crate::find_audio_files(&[fixtures]).unwrap();
        let folder = TemporaryFolder::new();
        let mut cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();

        let first_run =
            analyze_files_cached(&mut cache, &tracks, CachePolicy::UseCached, |_| {}).unwrap();
        let second_run =
            analyze_files_cached(&mut cache, &tracks, CachePolicy::UseCached, |_| {}).unwrap();

        assert_eq!(first_run.cached_count, 0);
        assert_eq!(second_run.cached_count, tracks.len());
        assert_eq!(analyses(&first_run), analyses(&second_run));
    }
}
