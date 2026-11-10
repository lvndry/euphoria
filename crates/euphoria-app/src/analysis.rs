// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Running the core's analysis on background workers, one result per file as
//! each finishes.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use euphoria_core::TrackAnalysis;

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
}

/// A pool of background workers that analyze queued files.
pub(crate) struct AnalysisWorkers {
    thread_pool: rayon::ThreadPool,
    finished_sender: Sender<FinishedFile>,
    stopping: Arc<AtomicBool>,
    files_in_progress: Arc<(Mutex<usize>, Condvar)>,
}

impl AnalysisWorkers {
    /// Starts the workers. Finished files arrive on the returned receiver in
    /// the order they finish.
    pub(crate) fn start() -> Result<(Self, Receiver<FinishedFile>), rayon::ThreadPoolBuildError> {
        let thread_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(worker_count())
            .thread_name(|worker_index| format!("euphoria-analysis-{worker_index}"))
            .build()?;
        let (finished_sender, finished_receiver) = channel();
        let workers = Self {
            thread_pool,
            finished_sender,
            stopping: Arc::new(AtomicBool::new(false)),
            files_in_progress: Arc::new((Mutex::new(0), Condvar::new())),
        };
        Ok((workers, finished_receiver))
    }

    /// Queues files for analysis. Returns at once; each file is analyzed by
    /// the core when a worker is free.
    pub(crate) fn enqueue(&self, files: Vec<PathBuf>) {
        for path in files {
            let finished_sender = self.finished_sender.clone();
            let stopping = Arc::clone(&self.stopping);
            let files_in_progress = Arc::clone(&self.files_in_progress);
            self.thread_pool.spawn(move || {
                {
                    // Checked under the lock so that `stop` either sees this
                    // file counted or this file sees the stop.
                    let (count, _) = &*files_in_progress;
                    let mut in_progress = count.lock().unwrap_or_else(PoisonError::into_inner);
                    if stopping.load(Ordering::SeqCst) {
                        return;
                    }
                    *in_progress += 1;
                }

                let outcome = euphoria_core::analyze_file(&path).map_err(|error| error.chain());
                // The receiver is gone once the app is closing, and then
                // nobody is left to show the result to.
                let _ = finished_sender.send(FinishedFile { path, outcome });

                let (count, all_finished) = &*files_in_progress;
                *count.lock().unwrap_or_else(PoisonError::into_inner) -= 1;
                all_finished.notify_all();
            });
        }
    }

    /// Skips every file still waiting in the queue, then waits up to
    /// `patience` for the files being analyzed to finish. Returns true when
    /// no worker is left in the middle of a file.
    pub(crate) fn stop(&self, patience: Duration) -> bool {
        self.stopping.store(true, Ordering::SeqCst);
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
        let (workers, finished) = AnalysisWorkers::start().unwrap();
        let missing = PathBuf::from("/definitely/not/here.mp3");
        workers.enqueue(vec![missing.clone()]);
        let file = finished.recv_timeout(Duration::from_secs(30)).unwrap();
        assert_eq!(file.path, missing);
        let reason = file.outcome.unwrap_err();
        assert!(reason.starts_with("cannot open"), "{reason}");
        assert!(workers.stop(Duration::from_secs(30)));
    }

    #[test]
    fn files_queued_after_stopping_are_skipped() {
        let (workers, finished) = AnalysisWorkers::start().unwrap();
        assert!(workers.stop(Duration::from_secs(1)));
        workers.enqueue(vec![PathBuf::from("/definitely/not/here.mp3")]);
        drop(workers);
        assert!(finished.recv_timeout(Duration::from_secs(30)).is_err());
    }
}
