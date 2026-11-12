// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! How the window is divided: whether the sidebar is folded away and how
//! large the track detail is. The sizes the user chose are kept as chosen,
//! and fitted to the window each time they are used, so a size remembered
//! from a large window never squeezes the table out of a small one.

use serde::{Deserialize, Serialize};

/// Width of the sidebar when it is shown.
pub(crate) const SIDEBAR_WIDTH: f32 = 220.0;

/// Width of the inspector until the user drags its edge.
pub(crate) const DEFAULT_INSPECTOR_WIDTH: u32 = 320;

/// The narrowest the inspector gets: the three statistics boxes still show
/// their text in full.
pub(crate) const NARROWEST_INSPECTOR: f32 = 288.0;

/// The widest the inspector gets. Beyond this the waveform bars only spread
/// out and the lines of text become too long to read.
pub(crate) const WIDEST_INSPECTOR: f32 = 640.0;

/// The narrowest the track table is left: room for the title, the key, the
/// tempo and the energy.
pub(crate) const NARROWEST_TABLE: f32 = 400.0;

/// Height of the strip under the table, in the layout that has one, until
/// the user drags its edge.
pub(crate) const DEFAULT_STRIP_HEIGHT: u32 = 180;

/// The shortest the strip gets: the summary, the statistics and a waveform
/// of the usual height fit.
pub(crate) const SHORTEST_STRIP: f32 = 124.0;

/// The tallest the strip gets.
pub(crate) const TALLEST_STRIP: f32 = 420.0;

/// The fewest table rows left visible above the strip, as a height.
pub(crate) const SHORTEST_TABLE: f32 = 200.0;

/// An edge of the track detail that can be dragged to resize it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PanelEdge {
    /// The left edge of the inspector column.
    InspectorLeft,
    /// The top edge of the strip under the table.
    StripTop,
}

/// How the user has arranged the window. Kept in the settings file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Panels {
    /// True when the sidebar is folded away.
    pub(crate) sidebar_folded: bool,
    /// The inspector width the user chose, in pixels.
    pub(crate) inspector_width: u32,
    /// The height the user chose for the strip under the table, in pixels.
    pub(crate) strip_height: u32,
}

impl Default for Panels {
    fn default() -> Self {
        Self {
            sidebar_folded: false,
            inspector_width: DEFAULT_INSPECTOR_WIDTH,
            strip_height: DEFAULT_STRIP_HEIGHT,
        }
    }
}

impl Panels {
    /// The width the sidebar takes: nothing when it is folded.
    pub(crate) fn sidebar_width(&self) -> f32 {
        if self.sidebar_folded {
            0.0
        } else {
            SIDEBAR_WIDTH
        }
    }

    /// The inspector width to draw in a window `window_width` wide: the
    /// chosen width, kept between the narrowest and widest inspector and
    /// leaving the table its narrowest width. When the window is too small
    /// for both, the inspector keeps its narrowest width.
    pub(crate) fn inspector_width_in(&self, window_width: f32) -> f32 {
        let left_for_inspector = window_width - self.sidebar_width() - NARROWEST_TABLE;
        let widest = WIDEST_INSPECTOR
            .min(left_for_inspector)
            .max(NARROWEST_INSPECTOR);
        (self.inspector_width as f32).clamp(NARROWEST_INSPECTOR, widest)
    }

    /// The strip height to draw above a bottom edge `available_height` below
    /// the top of the table: the chosen height, kept between the shortest
    /// and tallest strip and leaving the table its shortest height.
    pub(crate) fn strip_height_in(&self, available_height: f32) -> f32 {
        let tallest = TALLEST_STRIP
            .min(available_height - SHORTEST_TABLE)
            .max(SHORTEST_STRIP);
        (self.strip_height as f32).clamp(SHORTEST_STRIP, tallest)
    }

    /// Folds the sidebar away, or brings it back.
    pub(crate) fn toggle_sidebar(&mut self) {
        self.sidebar_folded = !self.sidebar_folded;
    }

    /// The left edge of the inspector is dragged to `pointer_x` in a window
    /// `window_width` wide. The width chosen is the one that fits there.
    pub(crate) fn drag_inspector_edge_to(&mut self, pointer_x: f32, window_width: f32) {
        self.inspector_width = (window_width - pointer_x).max(0.0).round() as u32;
        self.inspector_width = self.inspector_width_in(window_width).round() as u32;
    }

    /// The top edge of the strip is dragged to `pointer_y`, where the strip
    /// ends at `strip_bottom` and the table starts `available_height` above
    /// that.
    pub(crate) fn drag_strip_edge_to(
        &mut self,
        pointer_y: f32,
        strip_bottom: f32,
        available_height: f32,
    ) {
        self.strip_height = (strip_bottom - pointer_y).max(0.0).round() as u32;
        self.strip_height = self.strip_height_in(available_height).round() as u32;
    }

    /// Returns the inspector to the width it has before any drag.
    pub(crate) fn reset_inspector_width(&mut self) {
        self.inspector_width = DEFAULT_INSPECTOR_WIDTH;
    }

    /// Returns the strip to the height it has before any drag.
    pub(crate) fn reset_strip_height(&mut self) {
        self.strip_height = DEFAULT_STRIP_HEIGHT;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The smallest window the app allows.
    const SMALLEST_WINDOW_WIDTH: f32 = 960.0;

    fn panels(inspector_width: u32, sidebar_folded: bool) -> Panels {
        Panels {
            sidebar_folded,
            inspector_width,
            ..Panels::default()
        }
    }

    #[test]
    fn a_new_window_shows_the_sidebar_and_the_usual_sizes() {
        let panels = Panels::default();
        assert!(!panels.sidebar_folded);
        assert_eq!(panels.sidebar_width(), 220.0);
        assert_eq!(panels.inspector_width_in(1440.0), 320.0);
        assert_eq!(panels.strip_height_in(800.0), 180.0);
    }

    #[test]
    fn folding_the_sidebar_frees_its_width_and_unfolding_takes_it_back() {
        let mut panels = Panels::default();
        panels.toggle_sidebar();
        assert_eq!(panels.sidebar_width(), 0.0);
        panels.toggle_sidebar();
        assert_eq!(panels.sidebar_width(), 220.0);
    }

    #[test]
    fn the_inspector_stays_between_its_narrowest_and_widest() {
        assert_eq!(panels(10, false).inspector_width_in(1440.0), 288.0);
        assert_eq!(panels(5_000, false).inspector_width_in(2_400.0), 640.0);
        assert_eq!(panels(480, false).inspector_width_in(1440.0), 480.0);
    }

    #[test]
    fn a_wide_inspector_gives_way_to_the_table_in_a_small_window() {
        let wide = panels(640, false);
        assert_eq!(wide.inspector_width_in(1440.0), 640.0);
        assert_eq!(wide.inspector_width_in(1100.0), 480.0);
        assert_eq!(wide.inspector_width_in(SMALLEST_WINDOW_WIDTH), 340.0);
        assert_eq!(wide.inspector_width, 640);
    }

    #[test]
    fn folding_the_sidebar_leaves_more_room_for_a_wide_inspector() {
        assert_eq!(
            panels(640, true).inspector_width_in(SMALLEST_WINDOW_WIDTH),
            560.0
        );
    }

    #[test]
    fn the_table_and_the_inspector_both_fit_the_smallest_window_whatever_is_remembered() {
        for inspector_width in [0, 100, 320, 640, 100_000, u32::MAX] {
            for sidebar_folded in [false, true] {
                let panels = panels(inspector_width, sidebar_folded);
                let inspector = panels.inspector_width_in(SMALLEST_WINDOW_WIDTH);
                let table = SMALLEST_WINDOW_WIDTH - panels.sidebar_width() - inspector;
                assert!(inspector >= NARROWEST_INSPECTOR, "{panels:?}");
                assert!(table >= NARROWEST_TABLE, "{panels:?}: table {table}");
            }
        }
    }

    #[test]
    fn a_window_too_small_for_both_keeps_the_narrowest_inspector() {
        assert_eq!(panels(640, false).inspector_width_in(700.0), 288.0);
        assert_eq!(panels(640, false).inspector_width_in(0.0), 288.0);
    }

    #[test]
    fn dragging_the_edge_sets_the_width_that_fits_under_the_pointer() {
        let mut panels = Panels::default();
        panels.drag_inspector_edge_to(1_000.0, 1_440.0);
        assert_eq!(panels.inspector_width, 440);

        panels.drag_inspector_edge_to(1_300.0, 1_440.0);
        assert_eq!(panels.inspector_width, 288);

        panels.drag_inspector_edge_to(100.0, 1_440.0);
        assert_eq!(panels.inspector_width, 640);

        panels.drag_inspector_edge_to(100.0, SMALLEST_WINDOW_WIDTH);
        assert_eq!(panels.inspector_width, 340);

        panels.drag_inspector_edge_to(5_000.0, 1_440.0);
        assert_eq!(panels.inspector_width, 288);
    }

    #[test]
    fn a_double_click_returns_to_the_usual_sizes() {
        let mut panels = Panels::default();
        panels.drag_inspector_edge_to(900.0, 1_440.0);
        panels.drag_strip_edge_to(400.0, 800.0, 700.0);
        panels.reset_inspector_width();
        panels.reset_strip_height();
        assert_eq!(panels, Panels::default());
    }

    #[test]
    fn the_strip_stays_between_its_shortest_and_tallest_and_leaves_table_rows() {
        let chosen = |strip_height| Panels {
            strip_height,
            ..Panels::default()
        };
        assert_eq!(chosen(10).strip_height_in(800.0), 124.0);
        assert_eq!(chosen(5_000).strip_height_in(1_200.0), 420.0);
        assert_eq!(chosen(300).strip_height_in(800.0), 300.0);
        assert_eq!(chosen(420).strip_height_in(500.0), 300.0);
        assert_eq!(chosen(420).strip_height_in(250.0), 124.0);
    }

    #[test]
    fn dragging_the_strip_edge_sets_the_height_that_fits_above_the_bottom() {
        let mut panels = Panels::default();
        panels.drag_strip_edge_to(600.0, 860.0, 760.0);
        assert_eq!(panels.strip_height, 260);
        panels.drag_strip_edge_to(850.0, 860.0, 760.0);
        assert_eq!(panels.strip_height, 124);
        panels.drag_strip_edge_to(0.0, 860.0, 760.0);
        assert_eq!(panels.strip_height, 420);
    }

    #[test]
    fn the_arrangement_is_stored_by_field_and_missing_fields_take_defaults() {
        let stored = serde_json::to_string(&panels(480, true)).unwrap();
        assert_eq!(
            stored,
            r#"{"sidebar_folded":true,"inspector_width":480,"strip_height":180}"#
        );
        let partial: Panels = serde_json::from_str(r#"{"sidebar_folded":true}"#).unwrap();
        assert_eq!(partial, panels(320, true));
        let empty: Panels = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, Panels::default());
    }
}
