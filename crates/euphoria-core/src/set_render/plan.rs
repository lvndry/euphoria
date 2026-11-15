// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Choosing the blend between each track and the next with the transition
//! planner.

use super::{Blend, MixMove, MixPlan, MixTransition, RenderTrack};
use crate::harmony::transition::{
    CandidateWindow, PlanError, PlanOptions, PlanTrack, Side, WINDOW_LENGTHS, plan_transition,
};

/// How many ranked windows of each transition are looked through for one
/// that starts after the blend into the outgoing track has ended.
pub const SEARCHED_WINDOWS: usize = 64;

/// A track of the set with what the planner reads of it.
pub struct MixInput<'track> {
    /// The track as the mix plays it.
    pub track: RenderTrack,
    /// Its analysis, sections, energy and stem levels.
    pub plan: PlanTrack<'track>,
}

/// Plans the mix of `inputs` in their order: each track blends into the
/// next over the best window the transition planner ranks that starts once
/// the blend into it has ended, so no more than two tracks ever play at
/// once. The outgoing track leaves on one of its phrase starts, and the
/// incoming one comes in at the start of one of its sections.
///
/// A pair the planner cannot blend, because a track has no beat grid, the
/// tempos are too far apart for a deck or no window fits, is played one
/// after the other, and its reason says why.
pub fn plan_mix(inputs: &[MixInput<'_>]) -> MixPlan {
    let mut transitions = Vec::with_capacity(inputs.len().saturating_sub(1));
    let mut earliest_exit_bar = 0;
    for pair in inputs.windows(2) {
        let [outgoing, incoming] = pair else {
            continue;
        };
        let transition = transition_between(outgoing, incoming, earliest_exit_bar);
        earliest_exit_bar = match transition.mix {
            MixMove::Blend(blend) => blend.b_start_bar + blend.bars,
            MixMove::Sequence => 0,
        };
        transitions.push(transition);
    }
    MixPlan {
        tracks: inputs.iter().map(|input| input.track.clone()).collect(),
        transitions,
    }
}

fn transition_between(
    outgoing: &MixInput<'_>,
    incoming: &MixInput<'_>,
    earliest_exit_bar: usize,
) -> MixTransition {
    let options = PlanOptions {
        window_count: SEARCHED_WINDOWS,
    };
    let plan = match plan_transition(&outgoing.plan, &incoming.plan, &options) {
        Ok(plan) => plan,
        Err(error) => return sequence(&plan_error_reason(&error)),
    };
    if plan.windows.is_empty() {
        return sequence(&format!(
            "no window of {} bars or more fits these two tracks",
            WINDOW_LENGTHS[0]
        ));
    }
    let Some(window) = plan
        .windows
        .iter()
        .find(|window| window.a_start_bar >= earliest_exit_bar)
    else {
        return sequence("every window would start before the mix into this track ends");
    };
    MixTransition {
        mix: MixMove::Blend(Blend {
            a_start_bar: window.a_start_bar,
            b_start_bar: window.b_start_bar,
            bars: window.bars,
            bass_swap_bar: window.bass_swap_bar,
        }),
        reason: blend_reason(window),
    }
}

fn sequence(why: &str) -> MixTransition {
    MixTransition {
        mix: MixMove::Sequence,
        reason: format!("Played one after the other: {why}"),
    }
}

fn plan_error_reason(error: &PlanError) -> String {
    match error {
        PlanError::NoBeatGrid(Side::A) => "the outgoing track has no beat grid".to_string(),
        PlanError::NoBeatGrid(Side::B) => "the incoming track has no beat grid".to_string(),
        PlanError::TempoOutOfReach { rate } => {
            format!("the incoming track would have to play at {rate:.2} times its speed to match")
        }
    }
}

fn blend_reason(window: &CandidateWindow) -> String {
    let place = |section: Option<crate::harmony::transition::SectionLabel>, bar: usize| {
        section.map_or_else(
            || format!("bar {}", bar + 1),
            |section| format!("{} (bar {})", section.name(), bar + 1),
        )
    };
    format!(
        "{} into {}, {} bars: {}",
        place(window.a_section, window.a_start_bar),
        place(window.b_section, window.b_start_bar),
        window.bars,
        window.reason
    )
}
