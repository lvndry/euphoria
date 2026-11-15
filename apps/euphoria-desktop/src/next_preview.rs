// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The one row of the list of tracks to play next that is open in place,
//! showing the track's details and what can be done with it, while the
//! inspector stays on the playing track. Opening eases over a panel's time
//! and closing over a hover's; with Reduce motion both jump.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use crate::motion::{self, Transition};

/// How long a row opened by a click waits before the inspector scrolls it
/// into view, so the second click of a double click lands on the same row.
pub(crate) const REVEAL_DELAY: Duration = Duration::from_millis(400);

/// The open row, and the one folding away after another took its place.
pub(crate) struct NextPreview {
    expanded: Option<usize>,
    opening: Transition,
    folding: Option<usize>,
    closing: Transition,
    /// The height the details had when last drawn, so opening can ease to it.
    pub(crate) seen_height: Rc<Cell<Option<f32>>>,
    /// Set when a row opens, until the inspector has scrolled it into view.
    pub(crate) reveal: Rc<Cell<bool>>,
}

impl Default for NextPreview {
    fn default() -> Self {
        Self {
            expanded: None,
            opening: Transition::at_rest(false),
            folding: None,
            closing: Transition::at_rest(false),
            seen_height: Rc::new(Cell::new(None)),
            reveal: Rc::new(Cell::new(false)),
        }
    }
}

impl NextPreview {
    /// The library index of the open row's track.
    pub(crate) fn expanded(&self) -> Option<usize> {
        self.expanded
    }

    /// Opens the row of `track_index`, or closes it when it is the open one.
    pub(crate) fn toggle(&mut self, track_index: usize) {
        if self.expanded == Some(track_index) {
            self.collapse();
        } else {
            self.expand(track_index);
        }
    }

    /// Opens the row of `track_index`, folding away the one open before.
    pub(crate) fn expand(&mut self, track_index: usize) {
        if self.expanded == Some(track_index) {
            return;
        }
        self.fold_open_row();
        self.expanded = Some(track_index);
        self.opening.snap_to(false);
        self.opening.go_to(true, motion::PANEL);
    }

    /// Closes the open row. Returns whether one was open.
    pub(crate) fn collapse(&mut self) -> bool {
        if self.expanded.is_none() {
            return false;
        }
        self.fold_open_row();
        true
    }

    fn fold_open_row(&mut self) {
        let Some(open) = self.expanded.take() else {
            return;
        };
        std::mem::swap(&mut self.opening, &mut self.closing);
        self.folding = Some(open);
        self.closing.go_to(false, motion::FAST);
    }

    /// Closes everything at once: the list now follows another track.
    pub(crate) fn clear(&mut self) {
        self.expanded = None;
        self.folding = None;
        self.opening.snap_to(false);
        self.closing.snap_to(false);
    }

    /// Forgets rows whose tracks left the list.
    pub(crate) fn keep_only(&mut self, listed: &[usize]) {
        if self.expanded.is_some_and(|open| !listed.contains(&open)) {
            self.expanded = None;
            self.opening.snap_to(false);
        }
        if self
            .folding
            .is_some_and(|folding| !listed.contains(&folding))
        {
            self.folding = None;
            self.closing.snap_to(false);
        }
    }

    /// Moves the open row `step` places along `listed`, opening the first
    /// or the last when none is open.
    pub(crate) fn step(&mut self, listed: &[usize], step: isize) {
        let Some(last) = listed.len().checked_sub(1) else {
            return;
        };
        let current = self
            .expanded
            .and_then(|open| listed.iter().position(|track| *track == open));
        let target = match current {
            Some(position) => position.saturating_add_signed(step).min(last),
            None if step > 0 => 0,
            None => last,
        };
        self.expand(listed[target]);
    }

    /// How far open the row of `track_index` is, from 0.0 shut to 1.0 open.
    pub(crate) fn fraction(&self, track_index: usize) -> f32 {
        if self.expanded == Some(track_index) {
            self.opening.value(self.opening.now())
        } else if self.folding == Some(track_index) {
            self.closing.value(self.closing.now())
        } else {
            0.0
        }
    }

    /// Settles finished motions. True while either still moves.
    pub(crate) fn advance(&mut self) -> bool {
        let now = self.opening.now();
        self.opening.settle(now);
        self.closing.settle(now);
        let closing_moves = self.closing.is_moving(now);
        if !closing_moves {
            self.folding = None;
        }
        self.opening.is_moving(now) || closing_moves
    }

    #[cfg(test)]
    fn with_sources(mut self, clock: &motion::testing::FakeClock) -> Self {
        self.opening = Transition::at_rest(false).with_sources(clock.sources());
        self.closing = Transition::at_rest(false).with_sources(clock.sources());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::testing::FakeClock;

    #[test]
    fn one_row_is_open_at_a_time_and_a_second_click_closes_it() {
        let mut preview = NextPreview::default();
        preview.toggle(4);
        assert_eq!(preview.expanded(), Some(4));
        preview.toggle(7);
        assert_eq!(preview.expanded(), Some(7));
        preview.toggle(7);
        assert_eq!(preview.expanded(), None);
        assert!(!preview.collapse(), "nothing was open");
    }

    #[test]
    fn opening_eases_over_a_panel_and_the_old_row_folds_over_a_hover() {
        let clock = FakeClock::new();
        let mut preview = NextPreview::default().with_sources(&clock);
        clock.at(0.0);
        preview.expand(1);
        clock.at(0.09);
        let half_open = preview.fraction(1);
        assert!(half_open > 0.5 && half_open < 1.0);
        clock.at(0.2);
        assert_eq!(preview.fraction(1), 1.0);
        preview.expand(2);
        clock.at(0.26);
        assert!(preview.fraction(1) > 0.0, "still folding");
        assert!(preview.advance());
        clock.at(0.33);
        assert_eq!(preview.fraction(1), 0.0);
        clock.at(0.4);
        assert!(!preview.advance());
        assert_eq!(preview.fraction(2), 1.0);
    }

    #[test]
    fn reduce_motion_opens_and_closes_at_once() {
        let clock = FakeClock::new();
        clock.reduce.set(true);
        let mut preview = NextPreview::default().with_sources(&clock);
        clock.at(0.0);
        preview.expand(3);
        assert_eq!(preview.fraction(3), 1.0);
        preview.collapse();
        assert_eq!(preview.fraction(3), 0.0);
        assert!(!preview.advance());
    }

    #[test]
    fn arrows_walk_the_list_and_stop_at_its_ends() {
        let listed = [10, 20, 30];
        let mut preview = NextPreview::default();
        preview.step(&listed, 1);
        assert_eq!(preview.expanded(), Some(10));
        preview.step(&listed, 1);
        preview.step(&listed, 1);
        preview.step(&listed, 1);
        assert_eq!(preview.expanded(), Some(30));
        preview.step(&listed, -1);
        assert_eq!(preview.expanded(), Some(20));
        preview.collapse();
        preview.step(&listed, -1);
        assert_eq!(preview.expanded(), Some(30));
        preview.step(&[], 1);
        assert_eq!(preview.expanded(), Some(30));
    }

    #[test]
    fn a_row_that_leaves_the_list_is_forgotten() {
        let mut preview = NextPreview::default();
        preview.expand(5);
        preview.keep_only(&[5, 6]);
        assert_eq!(preview.expanded(), Some(5));
        preview.keep_only(&[6]);
        assert_eq!(preview.expanded(), None);
    }
}
