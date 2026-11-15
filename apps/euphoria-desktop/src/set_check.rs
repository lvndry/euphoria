// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Checking the transitions of the set builder's set before it is
//! rendered: where each track comes in and why, and where both tracks of a
//! blend would sing at once, with the fixes for it. The same plan as
//! `euphoria set render --dry-run`.

use std::path::PathBuf;

use euphoria_core::set_render::{MixPlan, VocalFix, VocalFixChoice};
use gpui::Context;

use crate::app::EuphoriaApp;

/// The button that checks the transitions.
pub(crate) const CHECK_ACTION: &str = "Check transitions";

/// The plan of the set as it was when checked.
pub(crate) struct CheckedSet {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) plan: MixPlan,
}

impl EuphoriaApp {
    /// Plans the mix of the set off the main thread and shows each
    /// transition with its reason and any vocal overlap.
    pub(crate) fn check_set_transitions(&mut self, cx: &mut Context<'_, Self>) {
        if self.set_builder.is_checking {
            return;
        }
        let Some(paths) = self
            .set_builder
            .planned
            .as_ref()
            .map(|planned| planned.paths.clone())
        else {
            return;
        };
        let Some(job) = self.mix_plan_job(cx) else {
            return;
        };
        self.set_builder.is_checking = true;
        let background_job = self.begin_background_job("Checking the transitions", cx);
        cx.spawn(async move |this, cx| {
            let plan = cx
                .background_executor()
                .spawn(async move { job.plan() })
                .await;
            this.update(cx, |this, cx| {
                this.background_jobs.finish(background_job);
                this.set_builder.is_checking = false;
                this.set_builder.checked = Some(CheckedSet { paths, plan });
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// The checked plan, while the set is still the one that was checked.
    pub(crate) fn checked_transitions(&self) -> Option<&MixPlan> {
        let checked = self.set_builder.checked.as_ref()?;
        let planned = self.set_builder.planned.as_ref()?;
        (checked.paths == planned.paths).then_some(&checked.plan)
    }

    /// Applies `fix` to the blend out of track `transition` of the checked
    /// set, then checks the set again. Moving the blend locks it there, as
    /// a drag in the planner does; the other fixes hold for the renders of
    /// this set while the app runs.
    pub(crate) fn apply_set_vocal_fix(
        &mut self,
        transition: usize,
        fix: VocalFix,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(plan) = self.checked_transitions() else {
            return;
        };
        let (Some(from), Some(to)) = (
            plan.tracks.get(transition).map(|track| track.path.clone()),
            plan.tracks
                .get(transition + 1)
                .map(|track| track.path.clone()),
        ) else {
            return;
        };
        match fix {
            VocalFix::MoveWindow { blend, .. } => self.lock_transition(from, to, blend, cx),
            VocalFix::CutOutgoingVocals => {
                self.set_builder
                    .vocal_fixes
                    .insert((from, to), VocalFixChoice::Cut);
            }
            VocalFix::FadeOutBefore { .. } => {
                self.set_builder
                    .vocal_fixes
                    .insert((from, to), VocalFixChoice::Fade);
            }
        }
        self.check_set_transitions(cx);
    }
}
