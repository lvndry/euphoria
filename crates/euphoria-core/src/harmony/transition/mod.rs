// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Planning a transition from track A, playing, to track B, next: B is
//! stretched to A's tempo and laid on A's bars, both tracks are read bar by
//! bar, and the windows where A can mix out and B in are ranked.
//!
//! Everything here is a pure function of the analyses, the sections, the
//! energy and the stem levels it is given, so the same input always gives
//! the same plan.

mod lanes;
mod windows;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

pub use lanes::{
    AutomaticCues, BarLane, Clash, EnergyPoint, PlanSection, PlanTrack, Presence,
    STEM_ABSENT_FLOOR, STEM_BODY_PERCENTILE, STEM_LEVEL_HOP_SECONDS, STEM_PRESENT_RATIO,
    SectionLabel, SectionSource, StemLevels, TrackLanes,
};
pub use windows::{
    BASS_CLASH_WEIGHT, CLASH_SCORE_WEIGHT, CLASH_TOLERANCE_SHARE, CandidateWindow, ClashCounts,
    ENERGY_SCORE_WEIGHT, KEY_SCORE_WEIGHT, LONG_PHRASE_BARS, NEXT_SECTION_BARS, OVERRUN_FACTOR,
    PHRASE_BARS, ROOM_SCORE_WEIGHT, ScoreBreakdown, Step, StepKind, VOCAL_CLASH_WEIGHT,
    WINDOW_LENGTHS,
};

use crate::detect::{BeatGrid, CueSection};
use crate::track_cues::Cue;

/// The slowest rate a deck plays a track at.
pub const MINIMUM_DECK_RATE: f64 = 0.5;
/// The fastest rate a deck plays a track at.
pub const MAXIMUM_DECK_RATE: f64 = 2.0;

/// The rate a deck really plays for `rate`, inside what it can do.
pub fn reachable_rate(rate: f64) -> f64 {
    if rate.is_finite() {
        rate.clamp(MINIMUM_DECK_RATE, MAXIMUM_DECK_RATE)
    } else {
        1.0
    }
}

/// The playback speed that brings a track at `own_bpm` closest to the
/// tempo `target_bpm`, reading the target at half or double when that is
/// closer, as a DJ beatmatches a track counted at half time. The speed is
/// one a deck can play, from [`MINIMUM_DECK_RATE`] to
/// [`MAXIMUM_DECK_RATE`], so a target out of reach gives the nearest speed
/// that is not; compare `own_bpm` times the speed with the target to tell.
pub fn matching_rate(own_bpm: f64, target_bpm: f64) -> f64 {
    if own_bpm <= 0.0 || target_bpm <= 0.0 || !own_bpm.is_finite() || !target_bpm.is_finite() {
        return 1.0;
    }
    [1.0, 2.0, 0.5]
        .into_iter()
        .map(|scale| {
            let wanted = target_bpm * scale / own_bpm;
            let reached = reachable_rate(wanted);
            // How far the tempo reached is from the one wanted, then how
            // far the speed is from the track's own.
            ((reached / wanted).ln().abs(), reached.ln().abs(), reached)
        })
        .min_by(|first, second| {
            first
                .0
                .total_cmp(&second.0)
                .then_with(|| first.1.total_cmp(&second.1))
        })
        .map_or(1.0, |(_, _, rate)| rate)
}

/// Windows returned by default.
pub const DEFAULT_WINDOW_COUNT: usize = 10;

/// What to plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanOptions {
    /// How many ranked windows to return.
    pub window_count: usize,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            window_count: DEFAULT_WINDOW_COUNT,
        }
    }
}

/// Which track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    /// The track that is playing.
    A,
    /// The track that is next.
    B,
}

/// Why no plan could be made.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum PlanError {
    /// A track has no beat grid to put on bars.
    #[error("track {0:?} has no beat grid")]
    NoBeatGrid(Side),
    /// B cannot be stretched to A's tempo within what a deck can do.
    #[error(
        "B would have to play at {rate:.2} times its speed to match A, and a deck plays between \
         {MINIMUM_DECK_RATE} and {MAXIMUM_DECK_RATE}"
    )]
    TempoOutOfReach {
        /// The rate that matching would take.
        rate: f64,
    },
}

/// How B lies on A's bars.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Alignment {
    /// The tempo of A.
    pub bpm_a: f64,
    /// The tempo B has before it is stretched.
    pub bpm_b: f64,
    /// The speed B plays at to match A.
    pub rate: f64,
    /// How much faster (positive) or slower B plays, in percent.
    pub tempo_change_percent: f64,
    /// Seconds in a bar of A, and of B once stretched.
    pub bar_seconds: f64,
}

/// A cue to suggest, which serializes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuggestedCue {
    /// Seconds from the start of the track's own audio.
    pub seconds: f64,
    /// The cue's name.
    pub name: String,
}

impl From<&SuggestedCue> for Cue {
    fn from(cue: &SuggestedCue) -> Self {
        Cue {
            seconds: cue.seconds,
            name: cue.name.clone(),
            hot_cue: None,
            energy: None,
        }
    }
}

/// The cues that carry out a window, for the user to accept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuggestedCues {
    /// For A, in order of time.
    pub a: Vec<SuggestedCue>,
    /// For B, in order of time.
    pub b: Vec<SuggestedCue>,
}

impl SuggestedCues {
    /// The cues for A as the cue model has them, on no hot cue pad.
    pub fn a_cues(&self) -> Vec<Cue> {
        self.a.iter().map(Cue::from).collect()
    }

    /// The cues for B as the cue model has them, on no hot cue pad.
    pub fn b_cues(&self) -> Vec<Cue> {
        self.b.iter().map(Cue::from).collect()
    }
}

/// A transition from A to B.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransitionPlan {
    /// How B lies on A.
    pub alignment: Alignment,
    /// The bars of A.
    pub lanes_a: TrackLanes,
    /// The bars of B.
    pub lanes_b: TrackLanes,
    /// The best windows, best first. Empty when no window fits the tracks.
    pub windows: Vec<CandidateWindow>,
    /// The cues for the best window, or `None` without one.
    pub cues: Option<SuggestedCues>,
}

/// Plans the transition from `a` to `b`.
pub fn plan_transition(
    a: &PlanTrack<'_>,
    b: &PlanTrack<'_>,
    options: &PlanOptions,
) -> Result<TransitionPlan, PlanError> {
    let grid_a = a.analysis.beat_grid.ok_or(PlanError::NoBeatGrid(Side::A))?;
    let grid_b = b.analysis.beat_grid.ok_or(PlanError::NoBeatGrid(Side::B))?;
    let wanted_rate = grid_a.bpm / grid_b.bpm;
    if !wanted_rate.is_finite() || reachable_rate(wanted_rate) != wanted_rate {
        return Err(PlanError::TempoOutOfReach { rate: wanted_rate });
    }
    let alignment = Alignment {
        bpm_a: grid_a.bpm,
        bpm_b: grid_b.bpm,
        rate: wanted_rate,
        tempo_change_percent: (wanted_rate - 1.0) * 100.0,
        bar_seconds: grid_a.bar_seconds(),
    };
    let lanes_a = TrackLanes::of(a, &grid_a);
    let lanes_b = TrackLanes::of(b, &grid_b);
    let found = windows::search(
        &windows::SearchInput {
            a: &lanes_a,
            b: &lanes_b,
            a_sections: a.sections,
            b_sections: b.sections,
            a_key: a.analysis.key,
            b_key: b.analysis.key,
        },
        options.window_count,
    );
    let cues = found.first().map(|window| cues_for_window(window, a, b));
    Ok(TransitionPlan {
        alignment,
        lanes_a,
        lanes_b,
        windows: found,
        cues: cues.flatten(),
    })
}

/// A window of `bars` bars that starts on bar `a_start` of A and bar
/// `b_start` of B, scored like the ranked ones, wherever it falls, or `None`
/// when B does not last that long or A has no bar `a_start`.
pub fn window_at(
    plan: &TransitionPlan,
    a: &PlanTrack<'_>,
    b: &PlanTrack<'_>,
    a_start: usize,
    b_start: usize,
    bars: usize,
) -> Option<CandidateWindow> {
    if bars == 0 || a_start >= plan.lanes_a.bars.len() || b_start + bars > plan.lanes_b.bars.len() {
        return None;
    }
    let input = windows::SearchInput {
        a: &plan.lanes_a,
        b: &plan.lanes_b,
        a_sections: a.sections,
        b_sections: b.sections,
        a_key: a.analysis.key,
        b_key: b.analysis.key,
    };
    Some(windows::evaluate(&input, a_start, b_start, bars))
}

/// The cues that carry out `window`, on the beat grids of `a` and `b`, or
/// `None` when either has no beat grid.
pub fn cues_for_window(
    window: &CandidateWindow,
    a: &PlanTrack<'_>,
    b: &PlanTrack<'_>,
) -> Option<SuggestedCues> {
    let grid_a = a.analysis.beat_grid?;
    let grid_b = b.analysis.beat_grid?;
    let a_at = |bar: usize| grid_a.first_downbeat_seconds + bar as f64 * grid_a.bar_seconds();
    let b_at = |bar: usize| grid_b.first_downbeat_seconds + bar as f64 * grid_b.bar_seconds();
    let cue = |seconds, name: &str| SuggestedCue {
        seconds,
        name: name.to_string(),
    };
    let mut a_cues = vec![cue(a_at(window.a_start_bar), "Mix out")];
    let end = a_at(window.a_start_bar + window.bars);
    if end <= f64::from(a.analysis.duration_seconds) {
        a_cues.push(cue(end, "Mix out end"));
    }
    let mut b_cues = vec![cue(b_at(window.b_start_bar), "Mix in")];
    for step in &window.steps {
        match step.kind {
            StepKind::SwapBass => b_cues.push(cue(b_at(step.b_bar), "Bass swap")),
            StepKind::BringInVocals => b_cues.push(cue(b_at(step.b_bar), "Vocals in")),
            StepKind::StartB | StepKind::AEnds => {}
        }
    }
    Some(SuggestedCues {
        a: a_cues,
        b: b_cues,
    })
}

/// The sections of a track from the cues it shows, as bars of `grid`: each
/// cue named after a section starts one. The planner reads the cues the
/// user edited this way, so a section moved by hand moves the plan.
pub fn sections_from_cues(cues: &[Cue], grid: &BeatGrid) -> Vec<PlanSection> {
    let bar_seconds = grid.bar_seconds();
    if bar_seconds <= 0.0 {
        return Vec::new();
    }
    cues.iter()
        .filter_map(|cue| {
            let section = CueSection::from_name(&cue.name)?;
            let bars = (cue.seconds - grid.first_downbeat_seconds) / bar_seconds;
            (bars.is_finite() && bars > -0.5).then(|| PlanSection {
                start_bar: bars.round().max(0.0) as usize,
                label: section.into(),
            })
        })
        .collect()
}
