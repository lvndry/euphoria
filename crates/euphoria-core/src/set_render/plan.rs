// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Choosing the blend between each track and the next with the transition
//! planner, keeping the blends the user placed by hand.

use super::vocals::{VocalFix, VocalFixChoice, blend_of, overlap_of};
use super::{Blend, MixMove, MixPlan, MixTransition, RenderTrack};
use crate::harmony::transition::{
    CandidateWindow, PlanError, PlanOptions, PlanTrack, SectionLabel, Side, TransitionPlan,
    WINDOW_LENGTHS, plan_transition, window_at,
};

/// How many ranked windows of each transition are looked through for one
/// that fits between the blend into the outgoing track and the blend out
/// of the incoming one.
pub const SEARCHED_WINDOWS: usize = 64;

/// A track of the set with what the planner reads of it.
pub struct MixInput<'track> {
    /// The track as the mix plays it.
    pub track: RenderTrack,
    /// Its analysis, sections, energy and stem levels.
    pub plan: PlanTrack<'track>,
    /// The blend into the next track when the user placed it by hand.
    /// Planning keeps it as it is.
    pub locked_exit: Option<Blend>,
    /// How to fix the blend into the next track when both would sing, or
    /// `None` to only warn. A locked blend is never moved.
    pub exit_vocal_fix: Option<VocalFixChoice>,
}

/// Plans the mix of `inputs` in their order.
///
/// A blend the user locked is kept as it is. Every other pair blends over
/// the best window the transition planner ranks that starts once the blend
/// into the outgoing track has ended and ends before a locked blend out of
/// the incoming one starts, so no more than two tracks ever play at once.
/// The outgoing track leaves on one of its phrase starts, and the incoming
/// one comes in at the start of one of its sections.
///
/// A pair the planner cannot blend, because a track has no beat grid, the
/// tempos are too far apart for a deck or no window fits, is played one
/// after the other, and its reason says why.
pub fn plan_mix(inputs: &[MixInput<'_>]) -> MixPlan {
    let mut transitions = Vec::with_capacity(inputs.len().saturating_sub(1));
    let mut earliest_exit_bar = 0;
    for (index, pair) in inputs.windows(2).enumerate() {
        let [outgoing, incoming] = pair else {
            continue;
        };
        let latest_entry_end = inputs
            .get(index + 2)
            .and(incoming.locked_exit)
            .map(|next| next.a_start_bar);
        let room = Room {
            earliest_exit_bar,
            latest_entry_end,
        };
        let transition = match outgoing.locked_exit {
            Some(blend) => locked_transition(outgoing, incoming, blend, room),
            None => transition_between(outgoing, incoming, room),
        };
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

fn plan_of(outgoing: &MixInput<'_>, incoming: &MixInput<'_>) -> Result<TransitionPlan, PlanError> {
    plan_transition(
        &outgoing.plan,
        &incoming.plan,
        &PlanOptions {
            window_count: SEARCHED_WINDOWS,
        },
    )
}

/// Where a blend may lie: from the bar of the outgoing track where the
/// blend into it has ended, to the bar of the incoming track where a locked
/// blend out of it starts.
#[derive(Debug, Clone, Copy)]
struct Room {
    earliest_exit_bar: usize,
    latest_entry_end: Option<usize>,
}

impl Room {
    fn fits(self, window: &CandidateWindow) -> bool {
        window.a_start_bar >= self.earliest_exit_bar
            && self
                .latest_entry_end
                .is_none_or(|latest| window.b_start_bar + window.bars <= latest)
    }
}

/// The best window of `plan` that fits `room` with only one track singing
/// at a time, checked on every bar.
fn clean_alternative(plan: &TransitionPlan, room: Room) -> Option<&CandidateWindow> {
    plan.windows.iter().find(|window| {
        room.fits(window) && window.clash_counts.vocals == 0 && window.clash_counts.unknown == 0
    })
}

fn locked_transition(
    outgoing: &MixInput<'_>,
    incoming: &MixInput<'_>,
    blend: Blend,
    room: Room,
) -> MixTransition {
    let plan = plan_of(outgoing, incoming).ok();
    let window = plan.as_ref().and_then(|plan| {
        window_at(
            plan,
            &outgoing.plan,
            &incoming.plan,
            blend.a_start_bar,
            blend.b_start_bar,
            blend.bars,
        )
    });
    let reason = match &window {
        Some(window) => format!("Locked, placed by hand. {}", blend_reason(window)),
        None => format!(
            "Locked, placed by hand. Bar {} into bar {}, {} bars",
            blend.a_start_bar + 1,
            blend.b_start_bar + 1,
            blend.bars
        ),
    };
    let vocal_overlap = window.as_ref().and_then(|window| {
        let alternative = plan.as_ref().and_then(|plan| clean_alternative(plan, room));
        overlap_of(window, alternative, outgoing.track.has_instrumental())
    });
    let vocal_fix = vocal_overlap.as_ref().and_then(|overlap| {
        let kept: Vec<VocalFix> = overlap
            .fixes
            .iter()
            .filter(|fix| !matches!(fix, VocalFix::MoveWindow { .. }))
            .cloned()
            .collect();
        outgoing.exit_vocal_fix?.pick(&kept).cloned()
    });
    MixTransition {
        mix: MixMove::Blend(blend),
        reason,
        vocal_overlap,
        vocal_fix,
    }
}

fn transition_between(
    outgoing: &MixInput<'_>,
    incoming: &MixInput<'_>,
    room: Room,
) -> MixTransition {
    let plan = match plan_of(outgoing, incoming) {
        Ok(plan) => plan,
        Err(error) => return sequence(&plan_error_reason(&error)),
    };
    if plan.windows.is_empty() {
        return sequence(&format!(
            "no window of {} bars or more fits these two tracks",
            WINDOW_LENGTHS[0]
        ));
    }
    let Some(window) = plan.windows.iter().find(|window| room.fits(window)) else {
        return sequence(match room.latest_entry_end {
            Some(_) => "no window fits between the blends you locked around it",
            None => "every window would start before the mix into this track ends",
        });
    };
    let vocal_overlap = overlap_of(
        window,
        clean_alternative(&plan, room),
        outgoing.track.has_instrumental(),
    );
    let chosen = vocal_overlap
        .as_ref()
        .zip(outgoing.exit_vocal_fix)
        .and_then(|(overlap, choice)| choice.pick(&overlap.fixes).cloned());
    match chosen {
        Some(VocalFix::MoveWindow { blend, reason }) => MixTransition {
            mix: MixMove::Blend(blend),
            reason: format!("Moved so only one track sings. {reason}"),
            vocal_overlap: None,
            vocal_fix: None,
        },
        chosen => MixTransition {
            mix: MixMove::Blend(blend_of(window)),
            reason: blend_reason(window),
            vocal_overlap,
            vocal_fix: chosen,
        },
    }
}

fn sequence(why: &str) -> MixTransition {
    MixTransition {
        mix: MixMove::Sequence,
        reason: format!("Played one after the other: {why}"),
        vocal_overlap: None,
        vocal_fix: None,
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

pub(super) fn blend_reason(window: &CandidateWindow) -> String {
    let place = |section: Option<SectionLabel>, bar: usize| {
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
