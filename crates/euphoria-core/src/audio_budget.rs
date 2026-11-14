// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Limits how much audio is decoded at once, so a folder of long mixes cannot
//! exhaust memory the way a count of workers alone allows.

use std::path::Path;
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};

use crate::audio::declared_duration_seconds;

/// The audio, in minutes, analyzed at once unless told otherwise. A track
/// peaks near 0.6 GB per seven minutes, about 86 MB a minute, so 60 minutes
/// is about 5 GB: what 8 workers on full length tracks peaked at, between
/// 4.8 and 6.2 GB. A library of normal tracks never meets the limit; a few
/// long mixes in flight at once do.
pub const DEFAULT_AUDIO_BUDGET_MINUTES: f64 = 60.0;

/// The length assumed for a file that does not declare one, from its size:
/// the bytes one second takes at 320 kbit/s. A lossless or high bit rate
/// file is taken to be longer than it is, which errs towards less in flight.
const ASSUMED_BYTES_PER_SECOND: u64 = 40_000;

/// The shortest length a file counts for.
const MINIMUM_SECONDS: f64 = 1.0;

/// A budget of audio seconds shared by the analyses running at once.
///
/// [`AudioBudget::acquire`] waits until the audio in flight plus the new
/// file fits the capacity. A file longer than the whole capacity waits for
/// the audio in flight to finish and runs alone, so it is never stuck.
#[derive(Debug)]
pub struct AudioBudget {
    state: Mutex<BudgetState>,
    released: Condvar,
}

#[derive(Debug)]
struct BudgetState {
    capacity_seconds: f64,
    in_flight_seconds: f64,
}

/// Audio held from an [`AudioBudget`] until it is dropped.
#[derive(Debug)]
pub struct AudioPermit<'budget> {
    budget: &'budget AudioBudget,
    seconds: f64,
}

impl AudioBudget {
    /// A budget of `capacity_seconds` of audio in flight.
    pub fn new(capacity_seconds: f64) -> Self {
        Self {
            state: Mutex::new(BudgetState {
                capacity_seconds: capacity_seconds.max(MINIMUM_SECONDS),
                in_flight_seconds: 0.0,
            }),
            released: Condvar::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, BudgetState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Changes the capacity. Files already in flight keep what they hold.
    pub fn set_capacity_seconds(&self, capacity_seconds: f64) {
        self.lock().capacity_seconds = capacity_seconds.max(MINIMUM_SECONDS);
        self.released.notify_all();
    }

    /// The capacity in seconds of audio.
    pub fn capacity_seconds(&self) -> f64 {
        self.lock().capacity_seconds
    }

    /// The seconds of audio held by permits now.
    pub fn in_flight_seconds(&self) -> f64 {
        self.lock().in_flight_seconds
    }

    /// Waits until `seconds` of audio fits, then holds it until the permit
    /// is dropped.
    pub fn acquire(&self, seconds: f64) -> AudioPermit<'_> {
        let seconds = seconds.max(MINIMUM_SECONDS);
        let mut state = self.lock();
        while state.in_flight_seconds > 0.0
            && state.in_flight_seconds + seconds > state.capacity_seconds
        {
            state = self
                .released
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        state.in_flight_seconds += seconds;
        AudioPermit {
            budget: self,
            seconds,
        }
    }
}

impl Drop for AudioPermit<'_> {
    fn drop(&mut self) {
        let mut state = self.budget.lock();
        state.in_flight_seconds = (state.in_flight_seconds - self.seconds).max(0.0);
        drop(state);
        self.budget.released.notify_all();
    }
}

/// The budget every analysis in this process shares.
pub fn global_audio_budget() -> &'static AudioBudget {
    static BUDGET: OnceLock<AudioBudget> = OnceLock::new();
    BUDGET.get_or_init(|| AudioBudget::new(DEFAULT_AUDIO_BUDGET_MINUTES * 60.0))
}

/// Sets how many minutes of audio are analyzed at once in this process.
pub fn set_audio_budget_minutes(minutes: f64) {
    global_audio_budget().set_capacity_seconds(minutes * 60.0);
}

/// How long the file at `path` is for the budget: the length its header
/// declares, or one estimated from its size when it declares none.
pub fn budgeted_seconds(path: &Path) -> f64 {
    declared_duration_seconds(path).map_or_else(
        || {
            let bytes = std::fs::metadata(path).map_or(0, |metadata| metadata.len());
            (bytes / ASSUMED_BYTES_PER_SECOND) as f64
        },
        f64::from,
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;

    /// Runs one thread per fake duration, each holding its audio for a
    /// moment, and returns the most seconds ever in flight at once and the
    /// largest number of files in flight.
    fn run(capacity_seconds: f64, durations: &[f64]) -> (f64, usize) {
        let budget = AudioBudget::new(capacity_seconds);
        let running = AtomicUsize::new(0);
        let most_files = AtomicUsize::new(0);
        let most_seconds = Mutex::new(0.0_f64);
        let (budget, running, most_files, most_seconds) =
            (&budget, &running, &most_files, &most_seconds);
        std::thread::scope(|scope| {
            for &seconds in durations {
                scope.spawn(move || {
                    let permit = budget.acquire(seconds);
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    most_files.fetch_max(now, Ordering::SeqCst);
                    {
                        let mut most = most_seconds.lock().unwrap();
                        *most = most.max(budget.in_flight_seconds());
                    }
                    std::thread::sleep(Duration::from_millis(15));
                    running.fetch_sub(1, Ordering::SeqCst);
                    drop(permit);
                });
            }
        });
        assert_eq!(budget.in_flight_seconds(), 0.0);
        let most_seconds = *most_seconds.lock().unwrap();
        (most_seconds, most_files.load(Ordering::SeqCst))
    }

    #[test]
    fn short_files_run_together_up_to_the_capacity() {
        let (most_seconds, most_files) = run(1_800.0, &[420.0; 12]);
        assert!(most_seconds <= 1_800.0, "{most_seconds}");
        assert!(most_files >= 2, "{most_files}");
        assert!(most_files <= 4, "{most_files}");
    }

    #[test]
    fn long_mixes_are_held_back_where_a_worker_count_would_not() {
        let hour = 3_600.0;
        let (most_seconds, most_files) = run(2.0 * hour, &[hour; 8]);
        assert!(most_seconds <= 2.0 * hour, "{most_seconds}");
        assert!(most_files <= 2, "{most_files}");
    }

    #[test]
    fn a_file_longer_than_the_capacity_runs_alone_and_finishes() {
        let (most_seconds, most_files) = run(600.0, &[5_000.0, 60.0, 60.0, 5_000.0, 60.0]);
        assert!(most_files >= 1);
        assert!(most_seconds <= 5_000.0 + 1e-6, "{most_seconds}");
        let budget = AudioBudget::new(600.0);
        let alone = budget.acquire(5_000.0);
        assert_eq!(budget.in_flight_seconds(), 5_000.0);
        drop(alone);
        assert_eq!(budget.in_flight_seconds(), 0.0);
    }

    #[test]
    fn a_file_waits_until_enough_audio_is_released() {
        let budget = AudioBudget::new(100.0);
        let first = budget.acquire(80.0);
        let waited = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let _second = budget.acquire(50.0);
                waited.store(1, Ordering::SeqCst);
            });
            std::thread::sleep(Duration::from_millis(50));
            assert_eq!(waited.load(Ordering::SeqCst), 0);
            drop(first);
        });
        assert_eq!(waited.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_file_without_a_declared_length_is_sized_from_its_bytes() {
        let path = std::env::temp_dir().join(format!("euphoria-budget-{}.xyz", std::process::id()));
        std::fs::write(&path, vec![0_u8; 4_000_000]).unwrap();
        let seconds = budgeted_seconds(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(seconds, 100.0);
    }
}
