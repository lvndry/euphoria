// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Counting how far the current run of analysis has got.

use std::time::{Duration, Instant};

use crate::format;

/// How much of a run has to be seen before a time left is worth saying.
const MIN_SECONDS_FOR_TIME_LEFT: u64 = 10;

/// Progress through the files queued since analysis was last idle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AnalysisProgress {
    queued: usize,
    finished: usize,
    started: Option<Instant>,
}

impl AnalysisProgress {
    /// Adds files to the current run.
    pub(crate) fn enqueue(&mut self, file_count: usize) {
        self.enqueue_at(file_count, Instant::now());
    }

    fn enqueue_at(&mut self, file_count: usize, now: Instant) {
        if file_count > 0 {
            self.started.get_or_insert(now);
        }
        self.queued += file_count;
    }

    /// Records one finished file. When the last queued file finishes the run
    /// is over and the counts start again from zero.
    pub(crate) fn finish_one(&mut self) {
        self.finished = (self.finished + 1).min(self.queued);
        if self.finished == self.queued {
            *self = Self::default();
        }
    }

    /// True while queued files are still waiting or being analyzed.
    pub(crate) fn is_active(&self) -> bool {
        self.queued > 0
    }

    /// The share of the run that is finished, from 0.0 to 1.0.
    pub(crate) fn fraction(&self) -> f32 {
        if self.queued == 0 {
            return 0.0;
        }
        self.finished as f32 / self.queued as f32
    }

    /// Files finished and files queued in the current run, or `None` when
    /// idle.
    pub(crate) fn counts(&self) -> Option<(usize, usize)> {
        self.is_active().then_some((self.finished, self.queued))
    }

    /// What the logo says while the run goes on, such as "Analyzing 12 of 340
    /// tracks", or `None` when idle.
    pub(crate) fn counts_label(&self) -> Option<String> {
        self.is_active().then(|| {
            format!(
                "Analyzing {} of {} tracks",
                format::count(self.finished),
                format::count(self.queued)
            )
        })
    }

    /// How long the run has left at the rate so far, in words such as
    /// "about 4 min left". `None` until [`MIN_SECONDS_FOR_TIME_LEFT`] seconds
    /// of the run have been seen and a file has finished.
    pub(crate) fn time_left_label(&self) -> Option<String> {
        self.time_left_label_at(Instant::now())
    }

    fn time_left_label_at(&self, now: Instant) -> Option<String> {
        let elapsed = now.checked_duration_since(self.started?)?;
        if !self.is_active()
            || self.finished == 0
            || elapsed < Duration::from_secs(MIN_SECONDS_FOR_TIME_LEFT)
        {
            return None;
        }
        let remaining = (self.queued - self.finished) as f64;
        let seconds_left = elapsed.as_secs_f64() * remaining / self.finished as f64;
        Some(if seconds_left < 45.0 {
            "under a minute left".to_string()
        } else if seconds_left < 90.0 * 60.0 {
            format!("about {} min left", (seconds_left / 60.0).round().max(1.0))
        } else {
            format!("about {} h left", (seconds_left / 3600.0).round())
        })
    }

    /// Toolbar text such as "12 of 340 done", counting failed files too, or `None` when idle.
    pub(crate) fn label(&self) -> Option<String> {
        self.is_active().then(|| {
            format!(
                "{} of {} done",
                format::count(self.finished),
                format::count(self.queued)
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_progress_has_no_label() {
        let progress = AnalysisProgress::default();
        assert!(!progress.is_active());
        assert_eq!(progress.label(), None);
        assert_eq!(progress.fraction(), 0.0);
    }

    #[test]
    fn counts_finished_files_against_the_queue() {
        let mut progress = AnalysisProgress::default();
        progress.enqueue(340);
        for _ in 0..12 {
            progress.finish_one();
        }
        assert_eq!(progress.label().as_deref(), Some("12 of 340 done"));
        assert!((progress.fraction() - 12.0 / 340.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_logo_label_counts_tracks() {
        let mut progress = AnalysisProgress::default();
        assert_eq!(progress.counts_label(), None);
        progress.enqueue(575);
        for _ in 0..120 {
            progress.finish_one();
        }
        assert_eq!(
            progress.counts_label().as_deref(),
            Some("Analyzing 120 of 575 tracks")
        );
    }

    #[test]
    fn files_added_during_a_run_extend_it() {
        let mut progress = AnalysisProgress::default();
        progress.enqueue(2);
        progress.finish_one();
        progress.enqueue(1200);
        assert_eq!(progress.label().as_deref(), Some("1 of 1,202 done"));
    }

    #[test]
    fn the_run_resets_when_the_last_file_finishes() {
        let mut progress = AnalysisProgress::default();
        progress.enqueue(2);
        progress.finish_one();
        progress.finish_one();
        assert!(!progress.is_active());
        progress.enqueue(1);
        assert_eq!(progress.label().as_deref(), Some("0 of 1 done"));
    }

    #[test]
    fn the_time_left_waits_for_ten_seconds_of_data() {
        let start = Instant::now();
        let mut progress = AnalysisProgress::default();
        progress.enqueue_at(575, start);
        assert_eq!(
            progress.time_left_label_at(start + Duration::from_secs(20)),
            None
        );
        for _ in 0..40 {
            progress.finish_one();
        }
        assert_eq!(
            progress.time_left_label_at(start + Duration::from_secs(9)),
            None
        );
        assert!(
            progress
                .time_left_label_at(start + Duration::from_secs(10))
                .is_some()
        );
    }

    #[test]
    fn the_time_left_follows_the_rate_so_far() {
        let start = Instant::now();
        let mut progress = AnalysisProgress::default();
        progress.enqueue_at(575, start);
        for _ in 0..312 {
            progress.finish_one();
        }
        assert_eq!(
            progress
                .time_left_label_at(start + Duration::from_secs(300))
                .as_deref(),
            Some("about 4 min left")
        );
        let mut nearly = AnalysisProgress::default();
        nearly.enqueue_at(100, start);
        for _ in 0..90 {
            nearly.finish_one();
        }
        assert_eq!(
            nearly
                .time_left_label_at(start + Duration::from_secs(60))
                .as_deref(),
            Some("under a minute left")
        );
    }

    #[test]
    fn there_is_no_time_left_when_idle() {
        let progress = AnalysisProgress::default();
        assert_eq!(
            progress.time_left_label_at(Instant::now() + Duration::from_secs(60)),
            None
        );
    }
}
