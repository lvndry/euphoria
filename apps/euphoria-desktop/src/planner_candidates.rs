// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Auditioning B in the planner: the tracks that fit after A, a click that
//! plays the transition into one at once, Up and Down through them, and B
//! dragged along the timeline to move the mix point.

use gpui::Context;

use crate::app::EuphoriaApp;
use crate::planner_playback::{LEAD_IN_BARS, PlanPlay};
use crate::transition_plan::{BARS_AFTER, BARS_BEFORE, BlockDrag, PlanState};

/// How many candidates the list holds.
const CANDIDATE_LIMIT: usize = 40;

impl EuphoriaApp {
    /// Ranks the tracks that fit after the selected track, with the same
    /// ranking, the same Keep, Lift and Drop and the same filters as the
    /// Next after list.
    pub(crate) fn refresh_plan_candidates(&mut self) {
        let Some(anchor) = self.selected else {
            self.planner.candidates.clear();
            return;
        };
        let pool = self.suggestion_indexes();
        self.planner.candidates = crate::next_rail::next_after(&crate::next_rail::NextQuery {
            library: self.session.library(),
            favorites: self.session.favorites(),
            playing_index: anchor,
            energy: self.next_energy,
            track_filters: &self.track_filters,
            suggestion_filter: &self.decks.filter,
            pool: &pool,
            played: &[],
            limit: CANDIDATE_LIMIT,
        });
    }

    /// Makes the track at `track_index` B, works out its best window and
    /// plays the transition into it, without another step.
    pub(crate) fn audition_candidate(&mut self, track_index: usize, cx: &mut Context<'_, Self>) {
        self.stop_plan_playback(cx);
        self.planner.custom = None;
        self.planner.start_bar = None;
        self.planner.drag = None;
        self.pick_match(track_index, cx);
        self.planner.play_when_ready = true;
        self.play_when_plan_ready(cx);
    }

    /// Starts the transition once the plan of the picked candidate has an
    /// answer, when a click asked for it.
    pub(crate) fn play_when_plan_ready(&mut self, cx: &mut Context<'_, Self>) {
        if !self.planner.play_when_ready {
            return;
        }
        let state = self
            .focus
            .selected_match
            .and_then(|index| self.plan_state(index));
        match state {
            Some(PlanState::Computing) => {}
            Some(PlanState::Ready(_)) => {
                self.planner.play_when_ready = false;
                self.play_plan(PlanPlay::Transition, Some(-LEAD_IN_BARS), cx);
            }
            Some(PlanState::Failed(_)) | None => {
                self.planner.play_when_ready = false;
                cx.notify();
            }
        }
    }

    /// Up and Down in the candidates: the next one is picked and plays.
    pub(crate) fn step_candidate(&mut self, step: isize, cx: &mut Context<'_, Self>) {
        let candidates = &self.planner.candidates;
        if candidates.is_empty() {
            return;
        }
        let current = self
            .focus
            .selected_match
            .and_then(|picked| candidates.iter().position(|row| row.track_index == picked));
        let target = match current {
            Some(position) => position
                .saturating_add_signed(step)
                .min(candidates.len() - 1),
            None if step > 0 => 0,
            None => candidates.len() - 1,
        };
        let track_index = candidates[target].track_index;
        if Some(track_index) != self.focus.selected_match {
            self.audition_candidate(track_index, cx);
        }
    }

    /// A press on B's rows: the mix point follows the pointer from here.
    pub(crate) fn begin_b_drag(&mut self, pointer_x: f32) {
        let Some((_, window)) = self.planner_window() else {
            return;
        };
        let drag = BlockDrag {
            start_x: pointer_x,
            a_start_bar: window.a_start_bar,
            b_start_bar: window.b_start_bar,
            bars: window.bars,
            moved: false,
        };
        self.planner.drag = Some(drag);
    }

    /// The pointer moved with the button down: B moves to the nearest bar,
    /// and onto a suggested window when it reaches one.
    pub(crate) fn drag_b_to(&mut self, pointer_x: f32, cx: &mut Context<'_, Self>) {
        let Some(drag) = self.planner.drag else {
            return;
        };
        let Some(bounds) = self.planner.timeline_bounds.get() else {
            return;
        };
        let Some(planned) = self.picked_plan().cloned() else {
            return;
        };
        let total = (BARS_BEFORE + drag.bars + BARS_AFTER) as f32;
        let bar_width = bounds.size.width / gpui::px(1.0) / total;
        if bar_width <= 0.0 {
            return;
        }
        let moved = ((pointer_x - drag.start_x) / bar_width).round() as i64;
        let last = planned.plan.lanes_a.bars.len().saturating_sub(1) as i64;
        let a_start = (drag.a_start_bar as i64 + moved).clamp(0, last) as usize;
        let current = self.planner_window().map(|(_, window)| window.a_start_bar);
        if current == Some(a_start) {
            return;
        }
        self.stop_plan_playback(cx);
        if let Some(drag) = &mut self.planner.drag {
            drag.moved = true;
        }
        let ranked = planned.plan.windows.iter().position(|window| {
            window.a_start_bar == a_start
                && window.b_start_bar == drag.b_start_bar
                && window.bars == drag.bars
        });
        match ranked {
            Some(index) => {
                self.planner.window = index;
                self.planner.custom = None;
            }
            None => {
                if let Some(window) = planned.window_at(a_start, drag.b_start_bar, drag.bars) {
                    self.planner.custom = Some(window);
                }
            }
        }
        cx.notify();
    }

    /// The button came up: the transition plays again from 8 bars before
    /// the new mix point.
    pub(crate) fn end_b_drag(&mut self, cx: &mut Context<'_, Self>) {
        if self.planner.drag.take().is_some_and(|drag| drag.moved) {
            self.planner.start_bar = None;
            self.play_plan(PlanPlay::Transition, Some(-LEAD_IN_BARS), cx);
        }
    }

    /// A press on the marker of ranked window `index`: the mix point jumps
    /// there, and the transition replays when it was playing.
    pub(crate) fn jump_to_window(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        let was_playing = self.plan_is_playing();
        self.select_plan_window(index, cx);
        if was_playing {
            self.play_plan(PlanPlay::Transition, Some(-LEAD_IN_BARS), cx);
        }
    }
}
