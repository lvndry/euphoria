// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Counting how far the current run of analysis has got.

use crate::format;

/// Progress through the files queued since analysis was last idle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct AnalysisProgress {
    queued: usize,
    finished: usize,
}

impl AnalysisProgress {
    /// Adds files to the current run.
    pub(crate) fn enqueue(&mut self, file_count: usize) {
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

    /// Toolbar text such as "12 of 340 analyzed", or `None` when idle.
    pub(crate) fn label(&self) -> Option<String> {
        self.is_active().then(|| {
            format!(
                "{} of {} analyzed",
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
        assert_eq!(progress.label().as_deref(), Some("12 of 340 analyzed"));
        assert!((progress.fraction() - 12.0 / 340.0).abs() < f32::EPSILON);
    }

    #[test]
    fn files_added_during_a_run_extend_it() {
        let mut progress = AnalysisProgress::default();
        progress.enqueue(2);
        progress.finish_one();
        progress.enqueue(1200);
        assert_eq!(progress.label().as_deref(), Some("1 of 1,202 analyzed"));
    }

    #[test]
    fn the_run_resets_when_the_last_file_finishes() {
        let mut progress = AnalysisProgress::default();
        progress.enqueue(2);
        progress.finish_one();
        progress.finish_one();
        assert!(!progress.is_active());
        progress.enqueue(1);
        assert_eq!(progress.label().as_deref(), Some("0 of 1 analyzed"));
    }
}
