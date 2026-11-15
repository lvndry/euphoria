// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Transitions placed by hand in the planner. Dragging B to a new mix point
//! or pressing the marker of another window locks the transition of A into
//! B: the planner opens on it again, and rendering a set keeps it.

use std::path::PathBuf;

use euphoria_core::{BlendBars, LockedTransition, PlaylistEntry, locked_blend};
use gpui::Context;

use crate::app::EuphoriaApp;

/// What the planner says of a transition placed by hand.
pub(crate) const LOCKED_LINE: &str = "Placed by hand and locked: rendering a set keeps it.";
/// The button that gives the transition back to the plan.
pub(crate) const UNLOCK_ACTION: &str = "Unlock";

impl EuphoriaApp {
    /// A and B of the planner.
    fn planner_pair(&self) -> Option<(PathBuf, PathBuf)> {
        let a = self.selected_track()?.path.clone();
        let b = self
            .session
            .library()
            .track(self.focus.selected_match?)?
            .path
            .clone();
        Some((a, b))
    }

    /// The transition of A into B the user locked, if any.
    pub(crate) fn planner_lock(&self) -> Option<BlendBars> {
        let (a, b) = self.planner_pair()?;
        locked_blend(self.session.locked_transitions(), (&a, None), (&b, None))
    }

    /// Shows the locked transition of A into B, when there is one, in
    /// place of the best window.
    pub(crate) fn show_planner_lock(&mut self) {
        let Some(lock) = self.planner_lock() else {
            return;
        };
        let Some(planned) = self.picked_plan().cloned() else {
            return;
        };
        let ranked = planned.plan.windows.iter().position(|window| {
            window.a_start_bar == lock.a_start_bar
                && window.b_start_bar == lock.b_start_bar
                && window.bars == lock.bars
        });
        match ranked {
            Some(index) => {
                self.planner.window = index;
                self.planner.custom = None;
            }
            None => {
                self.planner.custom =
                    planned.window_at(lock.a_start_bar, lock.b_start_bar, lock.bars);
            }
        }
    }

    /// Locks the window the planner shows as the transition of A into B.
    pub(crate) fn lock_planner_window(&mut self, cx: &mut Context<'_, Self>) {
        let Some((a, b)) = self.planner_pair() else {
            return;
        };
        let Some((_, window)) = self.planner_window() else {
            return;
        };
        let blend = BlendBars {
            a_start_bar: window.a_start_bar,
            b_start_bar: window.b_start_bar,
            bars: window.bars,
            bass_swap_bar: window.bass_swap_bar,
        };
        if self.planner_lock() == Some(blend) {
            return;
        }
        self.lock_transition(a, b, blend, cx);
    }

    /// Locks `blend` as the transition of the track at `a` into the one at
    /// `b`, in place of any lock of that pair.
    pub(crate) fn lock_transition(
        &mut self,
        a: PathBuf,
        b: PathBuf,
        blend: BlendBars,
        cx: &mut Context<'_, Self>,
    ) {
        let cache = self.session.cache();
        let entries = PlaylistEntry::for_file(&a, cache.as_deref())
            .and_then(|from| Ok((from, PlaylistEntry::for_file(&b, cache.as_deref())?)));
        let (from, to) = match entries {
            Ok(entries) => entries,
            Err(error) => {
                self.notice = Some(format!(
                    "The transition could not be locked: {}",
                    error.chain()
                ));
                cx.notify();
                return;
            }
        };
        let lock = LockedTransition { from, to, blend };
        let locks = self.session.locked_transitions_mut();
        locks.retain(|existing| !existing.applies_to((&a, None), (&b, None)));
        locks.push(lock.clone());
        if let Some(saver) = self.session.playlist_saver() {
            self.keep_in_background(move || saver.lock_transition(&lock), cx);
        }
        cx.notify();
    }

    /// Gives the transition of A into B back to the plan: the best window
    /// is shown, and rendering a set plans it again.
    pub(crate) fn unlock_planner_transition(&mut self, cx: &mut Context<'_, Self>) {
        let Some((a, b)) = self.planner_pair() else {
            return;
        };
        self.session
            .locked_transitions_mut()
            .retain(|existing| !existing.applies_to((&a, None), (&b, None)));
        if let Some(saver) = self.session.playlist_saver() {
            self.keep_in_background(move || saver.unlock_transition(&a, &b), cx);
        }
        self.select_plan_window(0, cx);
        cx.notify();
    }

    fn keep_in_background(
        &mut self,
        save: impl FnOnce() -> Result<(), euphoria_core::EuphoriaError> + Send + 'static,
        cx: &mut Context<'_, Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let saved = cx.background_executor().spawn(async move { save() }).await;
            if let Err(error) = saved {
                this.update(cx, |this, cx| {
                    this.notice = Some(format!(
                        "The locked transitions could not be kept: {}",
                        error.chain()
                    ));
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }
}
