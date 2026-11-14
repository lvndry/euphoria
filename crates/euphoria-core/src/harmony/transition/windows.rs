// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Candidate windows for a transition: where in A to start mixing out and
//! where in B to start mixing in, for 16, 32 or 64 bars.

use serde::{Deserialize, Serialize};

use super::lanes::{Clash, PlanSection, SectionLabel, TrackLanes};
use crate::harmony::Relation;
use crate::key::Key;

/// The lengths of a window in bars.
pub const WINDOW_LENGTHS: [usize; 3] = [16, 32, 64];

/// A window of 16 bars starts on a multiple of this many bars of A, the
/// phrase of dance music.
pub const PHRASE_BARS: usize = 8;

/// A window of 32 or 64 bars starts on a multiple of this many bars of A.
pub const LONG_PHRASE_BARS: usize = 16;

/// A bar where both tracks sing costs this much, twice a clash of bass:
/// two voices make both unintelligible, while two basslines only blur the
/// low end, which an EQ cut on one of them repairs.
pub const VOCAL_CLASH_WEIGHT: f32 = 2.0;
/// A bar where both tracks play bass costs this much.
pub const BASS_CLASH_WEIGHT: f32 = 1.0;
/// The weighted clash that reduces the clash score to zero, as a share of
/// the bars of the window counted at the cost of a bass clash: a quarter of
/// the window clashing on bass, or an eighth on vocals.
pub const CLASH_TOLERANCE_SHARE: f32 = 0.25;

/// Weight of the clash score in a window's score: what a DJ hears first.
pub const CLASH_SCORE_WEIGHT: f32 = 0.45;
/// Weight of the energy continuity.
pub const ENERGY_SCORE_WEIGHT: f32 = 0.20;
/// Weight of the key compatibility.
pub const KEY_SCORE_WEIGHT: f32 = 0.20;
/// Weight of A having room to finish the window.
pub const ROOM_SCORE_WEIGHT: f32 = 0.15;
/// The score of a window that outlasts A is multiplied by this: A ends in
/// silence in the middle of the mix.
pub const OVERRUN_FACTOR: f32 = 0.5;
/// The bars of B after the window that set its next energy.
pub const NEXT_SECTION_BARS: usize = 8;

/// A step of the transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StepKind {
    /// B starts playing.
    StartB,
    /// B's bass comes in and A's goes out.
    SwapBass,
    /// B's vocals come in.
    BringInVocals,
    /// A ends.
    AEnds,
}

/// One step of a window, at a bar of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// What happens.
    pub kind: StepKind,
    /// The bar of the window, counted from 1.
    pub window_bar: usize,
    /// The bar of A, counted from 0 at its first downbeat.
    pub a_bar: usize,
    /// The bar of B.
    pub b_bar: usize,
    /// The step in words.
    pub text: String,
}

/// How many bars of a window fall in each kind of [`Clash`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClashCounts {
    /// Bars where both sing.
    pub vocals: usize,
    /// Bars where both play bass.
    pub bass: usize,
    /// Bars with no clash.
    pub clear: usize,
    /// Bars that cannot be told.
    pub unknown: usize,
}

/// The parts of a window's score, each from 0 to 1, or `None` when it could
/// not be measured.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    /// How little the stems clash.
    pub clash: Option<f32>,
    /// How close A's energy and B's next energy are.
    pub energy: Option<f32>,
    /// How well the keys go together.
    pub key: Option<f32>,
    /// 1 when A lasts through the window, 0 when it ends in it.
    pub room: f32,
}

/// A place to mix A out and B in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateWindow {
    /// The position in the ranking from 1.
    pub rank: usize,
    /// Length in bars.
    pub bars: usize,
    /// The bar of A it starts on.
    pub a_start_bar: usize,
    /// The bar of B it starts on.
    pub b_start_bar: usize,
    /// The section of A at the start.
    pub a_section: Option<SectionLabel>,
    /// The section of B at the start.
    pub b_section: Option<SectionLabel>,
    /// The score from 0 to 100.
    pub score: f32,
    /// What the score is made of.
    pub breakdown: ScoreBreakdown,
    /// The clash in every bar.
    pub clash_strip: Vec<Clash>,
    /// The bars in each kind of clash.
    pub clash_counts: ClashCounts,
    /// True when every bar is known and none clashes.
    pub clean: bool,
    /// How B's key relates to A's.
    pub key_relation: Option<Relation>,
    /// Whether A lasts through the whole window.
    pub a_has_room: bool,
    /// The bar of the window, from 1, where B's bass can come in and stay
    /// without both playing bass, or `None` when there is nothing to swap
    /// or no such bar.
    pub bass_swap_bar: Option<usize>,
    /// The bar, from 1, where B's vocals can come in without both singing.
    pub vocals_in_bar: Option<usize>,
    /// Why the window ranks where it does.
    pub reason: String,
    /// What to do, in order.
    pub steps: Vec<Step>,
}

pub(super) struct SearchInput<'search> {
    pub a: &'search TrackLanes,
    pub b: &'search TrackLanes,
    pub a_sections: &'search [PlanSection],
    pub b_sections: &'search [PlanSection],
    pub a_key: Option<Key>,
    pub b_key: Option<Key>,
}

pub(super) fn search(input: &SearchInput<'_>, keep: usize) -> Vec<CandidateWindow> {
    let a_bars = input.a.bars.len();
    let b_bars = input.b.bars.len();
    let mut windows = Vec::new();
    for bars in WINDOW_LENGTHS {
        let step = if bars == WINDOW_LENGTHS[0] {
            PHRASE_BARS
        } else {
            LONG_PHRASE_BARS
        };
        for a_start in (0..a_bars).step_by(step) {
            if !is_mix_out(input.a_sections, a_start) {
                continue;
            }
            for b_start in mix_in_starts(input.b_sections, b_bars) {
                if b_start + bars > b_bars {
                    continue;
                }
                windows.push(evaluate(input, a_start, b_start, bars));
            }
        }
    }
    windows.sort_by(|first, second| {
        second
            .score
            .total_cmp(&first.score)
            .then(first.clash_counts.vocals.cmp(&second.clash_counts.vocals))
            .then(first.a_start_bar.cmp(&second.a_start_bar))
            .then(first.b_start_bar.cmp(&second.b_start_bar))
            .then(first.bars.cmp(&second.bars))
    });
    let mut kept: Vec<CandidateWindow> = Vec::new();
    for window in windows {
        let repeats = kept.iter().any(|existing| {
            existing.a_start_bar == window.a_start_bar && existing.b_start_bar == window.b_start_bar
        });
        if !repeats {
            kept.push(window);
        }
        if kept.len() == keep {
            break;
        }
    }
    for (index, window) in kept.iter_mut().enumerate() {
        window.rank = index + 1;
    }
    kept
}

fn is_mix_out(sections: &[PlanSection], bar: usize) -> bool {
    match section_at(sections, bar) {
        Some(label) => label != SectionLabel::Start,
        None => true,
    }
}

fn mix_in_starts(sections: &[PlanSection], bar_count: usize) -> Vec<usize> {
    let mut starts: Vec<usize> = if sections.is_empty() {
        (0..bar_count).step_by(PHRASE_BARS).collect()
    } else {
        sections
            .iter()
            .filter(|section| section.label != SectionLabel::Outro)
            .filter(|section| {
                section.label == SectionLabel::Start || section.start_bar % PHRASE_BARS == 0
            })
            .map(|section| section.start_bar)
            .collect()
    };
    starts.sort_unstable();
    starts.dedup();
    starts
}

fn section_at(sections: &[PlanSection], bar: usize) -> Option<SectionLabel> {
    sections
        .iter()
        .rev()
        .find(|section| section.start_bar <= bar)
        .map(|section| section.label)
}

fn evaluate(
    input: &SearchInput<'_>,
    a_start: usize,
    b_start: usize,
    bars: usize,
) -> CandidateWindow {
    let a_bars = input.a.bars.len();
    let strip: Vec<Clash> = (0..bars)
        .map(|offset| match input.a.bars.get(a_start + offset) {
            Some(a_bar) => Clash::of(a_bar, &input.b.bars[b_start + offset]),
            None => Clash::Clear,
        })
        .collect();
    let mut counts = ClashCounts::default();
    for clash in &strip {
        match clash {
            Clash::Vocals => counts.vocals += 1,
            Clash::Bass => counts.bass += 1,
            Clash::Clear => counts.clear += 1,
            Clash::Unknown => counts.unknown += 1,
        }
    }
    let known = bars - counts.unknown;
    let clash_score = (known > 0).then(|| {
        let cost =
            counts.vocals as f32 * VOCAL_CLASH_WEIGHT + counts.bass as f32 * BASS_CLASH_WEIGHT;
        (1.0 - cost / (CLASH_TOLERANCE_SHARE * known as f32)).clamp(0.0, 1.0)
    });
    let a_has_room = a_start + bars <= a_bars;
    let key_relation = match (input.a_key, input.b_key) {
        (Some(a_key), Some(b_key)) => a_key.relation_to(&b_key),
        _ => None,
    };
    let key_score = match (input.a_key, input.b_key) {
        (Some(_), Some(_)) => Some(key_relation.map_or(0.0, key_compatibility)),
        _ => None,
    };
    let breakdown = ScoreBreakdown {
        clash: clash_score,
        energy: energy_continuity(input, a_start, b_start, bars),
        key: key_score,
        room: if a_has_room { 1.0 } else { 0.0 },
    };
    let parts = [
        (breakdown.clash, CLASH_SCORE_WEIGHT),
        (breakdown.energy, ENERGY_SCORE_WEIGHT),
        (breakdown.key, KEY_SCORE_WEIGHT),
        (Some(breakdown.room), ROOM_SCORE_WEIGHT),
    ];
    let total_weight: f32 = parts
        .iter()
        .filter(|(part, _)| part.is_some())
        .map(|(_, weight)| weight)
        .sum();
    let weighted: f32 = parts
        .iter()
        .filter_map(|(part, weight)| part.map(|part| part * weight))
        .sum();
    let mut score = 100.0 * weighted / total_weight;
    if !a_has_room {
        score *= OVERRUN_FACTOR;
    }

    let bass_swap_bar = clash_free_from(&strip, Clash::Bass)
        .filter(|bar| *bar > 0)
        .map(|bar| bar + 1);
    let vocals_in_bar = clash_free_from(&strip, Clash::Vocals).and_then(|from| {
        (from..bars)
            .find(|offset| input.b.bars[b_start + offset].vocals == super::lanes::Presence::Present)
            .map(|offset| offset + 1)
    });
    let mut window = CandidateWindow {
        rank: 0,
        bars,
        a_start_bar: a_start,
        b_start_bar: b_start,
        a_section: section_at(input.a_sections, a_start),
        b_section: section_at(input.b_sections, b_start),
        score,
        breakdown,
        clean: counts.unknown == 0 && counts.vocals == 0 && counts.bass == 0,
        clash_strip: strip,
        clash_counts: counts,
        key_relation,
        a_has_room,
        bass_swap_bar,
        vocals_in_bar,
        reason: String::new(),
        steps: Vec::new(),
    };
    window.steps = steps_of(&window);
    window.reason = reason_of(&window);
    window
}

fn clash_free_from(strip: &[Clash], kind: Clash) -> Option<usize> {
    let last_clash = strip.iter().rposition(|clash| *clash == kind);
    match last_clash {
        None => Some(0),
        Some(last) if last + 1 < strip.len() => Some(last + 1),
        Some(_) => None,
    }
}

fn key_compatibility(relation: Relation) -> f32 {
    match relation {
        Relation::Same | Relation::Adjacent | Relation::Relative => 1.0,
        Relation::Diagonal => 0.8,
        Relation::EnergyBoost => 0.7,
        Relation::MoodChange | Relation::SemitoneBoost => 0.5,
    }
}

fn mean_energy(lanes: &TrackLanes, from: usize, to: usize) -> Option<f32> {
    let values: Vec<f32> = lanes
        .bars
        .get(from..to.min(lanes.bars.len()))?
        .iter()
        .filter_map(|bar| bar.energy)
        .collect();
    (!values.is_empty()).then(|| values.iter().sum::<f32>() / values.len() as f32)
}

fn energy_continuity(
    input: &SearchInput<'_>,
    a_start: usize,
    b_start: usize,
    bars: usize,
) -> Option<f32> {
    let a_energy = mean_energy(input.a, a_start, a_start + bars)?;
    let after = b_start + bars;
    let b_energy = mean_energy(input.b, after, after + NEXT_SECTION_BARS)
        .or_else(|| mean_energy(input.b, b_start, after))?;
    Some((1.0 - (a_energy - b_energy).abs() / 9.0).clamp(0.0, 1.0))
}

fn steps_of(window: &CandidateWindow) -> Vec<Step> {
    let step = |kind, offset: usize, text: String| Step {
        kind,
        window_bar: offset + 1,
        a_bar: window.a_start_bar + offset,
        b_bar: window.b_start_bar + offset,
        text,
    };
    let mut steps = vec![step(StepKind::StartB, 0, "Start B".to_string())];
    if let Some(bar) = window.bass_swap_bar {
        steps.push(step(
            StepKind::SwapBass,
            bar - 1,
            format!("Swap the bass at bar {bar}: B's bass in, A's out"),
        ));
    }
    if let Some(bar) = window.vocals_in_bar {
        steps.push(step(
            StepKind::BringInVocals,
            bar - 1,
            format!("Bring in B's vocals at bar {bar}"),
        ));
    }
    steps.push(step(
        StepKind::AEnds,
        window.bars - 1,
        format!("A ends at bar {}", window.bars),
    ));
    steps.sort_by_key(|step| step.window_bar);
    steps
}

fn reason_of(window: &CandidateWindow) -> String {
    let counts = window.clash_counts;
    let mut parts: Vec<String> = Vec::new();
    if counts.unknown == window.bars {
        parts.push("stems missing, clashes not checked".to_string());
    } else {
        if counts.vocals == 0 {
            parts.push("no vocal overlap".to_string());
        } else {
            parts.push(format!("{} bars of both vocals", counts.vocals));
        }
        match (counts.bass, window.bass_swap_bar) {
            (0, _) => parts.push("no bass clash".to_string()),
            (_, Some(bar)) => parts.push(format!("bass swap at bar {bar}")),
            (bars, None) => parts.push(format!("{bars} bars of both bass, no clean swap")),
        }
        if counts.unknown > 0 {
            parts.push(format!("{} bars not checked", counts.unknown));
        }
    }
    if let Some(relation) = window.key_relation {
        parts.push(format!("key: {}", relation.label().to_lowercase()));
    }
    if !window.a_has_room {
        parts.push("A ends before the window does".to_string());
    }
    parts.join(", ")
}
