// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Analyzing tracks again on demand, so that tracks analyzed before a new
//! field existed can be refreshed without redoing the whole library.

use std::path::PathBuf;

use gpui::Context;

use crate::app::EuphoriaApp;
use crate::format;
use crate::library::TrackStatus;

/// The row of the menus that analyzes `count` tracks again, or `None` when
/// there are none to analyze.
pub(crate) fn menu_label(count: usize) -> Option<String> {
    (count > 0).then(|| analyze_again_label(count))
}

/// The menu item that analyzes `count` tracks again.
pub(crate) fn analyze_again_label(count: usize) -> String {
    if count == 1 {
        "Analyze again".to_string()
    } else {
        format!("Analyze {} tracks again", format::count(count))
    }
}

impl EuphoriaApp {
    /// The files a right-click on the row of the track at `track_index`
    /// analyzes again: the selection when the row is part of it, the row
    /// alone otherwise. Tracks that are waiting, being analyzed or missing
    /// are left out.
    pub(crate) fn rows_to_analyze_again(&self, track_index: usize) -> Vec<PathBuf> {
        let indexes = if self.is_marked(track_index) {
            self.selected_tracks()
        } else {
            vec![track_index]
        };
        self.reanalyzable_paths(indexes)
    }

    fn reanalyzable_paths(&self, indexes: Vec<usize>) -> Vec<PathBuf> {
        let library = self.session.library();
        indexes
            .into_iter()
            .filter_map(|index| library.track(index))
            .filter(|track| {
                matches!(
                    track.status,
                    TrackStatus::Analyzed(_) | TrackStatus::Failed(_)
                )
            })
            .map(|track| track.path.clone())
            .collect()
    }

    /// Analyzes the tracks of a right-clicked row again.
    pub(crate) fn analyze_row_again(&mut self, track_index: usize, cx: &mut Context<'_, Self>) {
        let paths = self.rows_to_analyze_again(track_index);
        self.analyze_paths_again(&paths, cx);
    }

    /// Analyzes the selected tracks again.
    pub(crate) fn analyze_selection_again(&mut self, cx: &mut Context<'_, Self>) {
        let indexes = if self.marked.is_empty() {
            self.selected.into_iter().collect()
        } else {
            self.selected_tracks()
        };
        let paths = self.reanalyzable_paths(indexes);
        self.analyze_paths_again(&paths, cx);
    }

    /// The "Analyze again" of the Needs review box: the selected track,
    /// through the same queue.
    pub(crate) fn analyze_selected_failed_again(&mut self, cx: &mut Context<'_, Self>) {
        let indexes = self.selected.into_iter().collect();
        let paths = self.reanalyzable_paths(indexes);
        self.analyze_paths_again(&paths, cx);
    }

    fn analyze_paths_again(&mut self, paths: &[PathBuf], cx: &mut Context<'_, Self>) {
        if self.session.analyze_again(paths) > 0 {
            self.refresh_visible();
            self.start_collecting(cx);
            cx.notify();
        }
    }

    /// Stops the analysis of the tracks still waiting. Tracks being
    /// analyzed finish.
    pub(crate) fn stop_analysis(&mut self, cx: &mut Context<'_, Self>) {
        if self.session.progress().is_active() {
            self.session.cancel_queued();
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_label_says_how_many_tracks() {
        assert_eq!(analyze_again_label(1), "Analyze again");
        assert_eq!(analyze_again_label(12), "Analyze 12 tracks again");
        assert_eq!(analyze_again_label(575), "Analyze 575 tracks again");
        assert_eq!(menu_label(0), None);
        assert_eq!(menu_label(2).as_deref(), Some("Analyze 2 tracks again"));
    }
}
