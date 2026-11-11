// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Running the core's analysis on background workers, one result per file as
//! each finishes.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use euphoria_core::{CachedAnalyzer, TrackAnalysis};

/// Each worker can hold 2 to 3 GB while analyzing a full length track, so the
/// app analyzes at most this many files at once, as the command line tool
/// does by default.
const MAXIMUM_WORKERS: usize = 4;

/// The result of analyzing one file: the analysis, or why it failed.
#[derive(Debug)]
pub(crate) struct FinishedFile {
    /// The file that was analyzed.
    pub(crate) path: PathBuf,
    /// The analysis, or the reason it failed with every underlying cause.
    pub(crate) outcome: Result<TrackAnalysis, String>,
    /// True when the analysis was served from the cache.
    pub(crate) from_cache: bool,
    /// Why the analysis could not be stored in the cache, when storing
    /// failed.
    pub(crate) cache_problem: Option<String>,
}

/// What a worker reports about one file.
#[derive(Debug)]
pub(crate) enum AnalysisEvent {
    /// A worker took the file out of the queue and is analyzing it.
    Started(PathBuf),
    /// The file's analysis ended, with a result or a failure.
    Finished(FinishedFile),
}

/// Tells the workers to stop and reports when they have.
#[derive(Debug, Clone, Default)]
pub(crate) struct StopSignal {
    stopping: Arc<AtomicBool>,
    files_in_progress: Arc<(Mutex<usize>, Condvar)>,
}

impl StopSignal {
    /// Makes every file still waiting in the queue be skipped. Returns at
    /// once; files in the middle of analysis carry on.
    pub(crate) fn skip_queued_files(&self) {
        self.stopping.store(true, Ordering::SeqCst);
    }

    /// Waits up to `patience` for the files in the middle of analysis.
    /// Returns true when no worker is left in the middle of a file.
    pub(crate) fn wait_for_files_in_progress(&self, patience: Duration) -> bool {
        let deadline = Instant::now() + patience;
        let (count, all_finished) = &*self.files_in_progress;
        let mut in_progress = count.lock().unwrap_or_else(PoisonError::into_inner);
        while *in_progress > 0 {
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            in_progress = all_finished
                .wait_timeout(in_progress, remaining)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }

    /// Counts a file as in progress, unless the workers are stopping.
    fn begin_file(&self) -> bool {
        // Checked under the lock so that a stop either sees this file
        // counted or this file sees the stop.
        let (count, _) = &*self.files_in_progress;
        let mut in_progress = count.lock().unwrap_or_else(PoisonError::into_inner);
        if self.stopping.load(Ordering::SeqCst) {
            return false;
        }
        *in_progress += 1;
        true
    }

    fn finish_file(&self) {
        let (count, all_finished) = &*self.files_in_progress;
        *count.lock().unwrap_or_else(PoisonError::into_inner) -= 1;
        all_finished.notify_all();
    }
}

/// A pool of background workers that analyze queued files.
pub(crate) struct AnalysisWorkers {
    thread_pool: rayon::ThreadPool,
    event_sender: Sender<AnalysisEvent>,
    stop_signal: StopSignal,
    cache: Option<Arc<CachedAnalyzer>>,
}

impl AnalysisWorkers {
    /// Starts the workers. With a cache, each file is looked up there first
    /// and each new analysis is stored. Each file is reported on the returned
    /// receiver when a worker starts it and again when it finishes.
    pub(crate) fn start(
        cache: Option<Arc<CachedAnalyzer>>,
    ) -> Result<(Self, Receiver<AnalysisEvent>), rayon::ThreadPoolBuildError> {
        let thread_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(worker_count())
            .thread_name(|worker_index| format!("euphoria-analysis-{worker_index}"))
            .build()?;
        let (event_sender, event_receiver) = channel();
        let workers = Self {
            thread_pool,
            event_sender,
            stop_signal: StopSignal::default(),
            cache,
        };
        Ok((workers, event_receiver))
    }

    /// Queues files for analysis. Returns at once; each file is analyzed by
    /// the core when a worker is free.
    pub(crate) fn enqueue(&self, files: Vec<PathBuf>) {
        for path in files {
            let event_sender = self.event_sender.clone();
            let stop_signal = self.stop_signal.clone();
            let cache = self.cache.clone();
            self.thread_pool.spawn(move || {
                if !stop_signal.begin_file() {
                    return;
                }
                // The receiver is gone once the app is closing, and then
                // nobody is left to show the progress or the result to.
                let _ = event_sender.send(AnalysisEvent::Started(path.clone()));
                let finished_file = analyze(path, cache.as_deref());
                let _ = event_sender.send(AnalysisEvent::Finished(finished_file));
                stop_signal.finish_file();
            });
        }
    }

    /// The signal that stops these workers.
    pub(crate) fn stop_signal(&self) -> StopSignal {
        self.stop_signal.clone()
    }
}

/// Asks the core for the analysis of one file, through the cache when there
/// is one.
fn analyze(path: PathBuf, cache: Option<&CachedAnalyzer>) -> FinishedFile {
    let (outcome, from_cache, cache_problem) = match cache {
        Some(cache) => match cache.analyze_file(&path) {
            Ok(cached) => (
                Ok(cached.analysis),
                cached.from_cache,
                cached.store_error.map(|error| error.chain()),
            ),
            Err(error) => (Err(error.chain()), false, None),
        },
        None => (analyze_without_cache(&path), false, None),
    };
    FinishedFile {
        path,
        outcome,
        from_cache,
        cache_problem,
    }
}

fn analyze_without_cache(path: &Path) -> Result<TrackAnalysis, String> {
    euphoria_core::analyze_file(path).map_err(|error| error.chain())
}

fn worker_count() -> usize {
    std::thread::available_parallelism()
        .map(NonZeroUsize::get)
        .unwrap_or(1)
        .min(MAXIMUM_WORKERS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_at_most_four_workers() {
        assert!((1..=MAXIMUM_WORKERS).contains(&worker_count()));
    }

    #[test]
    fn reports_a_file_that_cannot_be_opened() {
        let (workers, events) = AnalysisWorkers::start(None).unwrap();
        let missing = PathBuf::from("/definitely/not/here.mp3");
        workers.enqueue(vec![missing.clone()]);
        let started = events.recv_timeout(Duration::from_secs(30)).unwrap();
        assert!(matches!(started, AnalysisEvent::Started(path) if path == missing));
        let AnalysisEvent::Finished(file) = events.recv_timeout(Duration::from_secs(30)).unwrap()
        else {
            panic!("the second report for a file is its result");
        };
        assert_eq!(file.path, missing);
        assert!(!file.from_cache);
        let reason = file.outcome.unwrap_err();
        assert!(reason.starts_with("cannot open"), "{reason}");
        assert!(
            workers
                .stop_signal()
                .wait_for_files_in_progress(Duration::from_secs(30))
        );
    }

    #[test]
    fn files_queued_after_stopping_are_skipped() {
        let (workers, finished) = AnalysisWorkers::start(None).unwrap();
        workers.stop_signal().skip_queued_files();
        workers.enqueue(vec![PathBuf::from("/definitely/not/here.mp3")]);
        drop(workers);
        assert!(finished.recv_timeout(Duration::from_secs(30)).is_err());
    }

    #[test]
    fn waiting_gives_up_when_a_file_takes_longer_than_the_patience() {
        let signal = StopSignal::default();
        assert!(signal.begin_file());
        let started = Instant::now();
        assert!(!signal.wait_for_files_in_progress(Duration::from_millis(50)));
        assert!(started.elapsed() < Duration::from_secs(5));
        signal.finish_file();
        assert!(signal.wait_for_files_in_progress(Duration::from_millis(50)));
    }
}
