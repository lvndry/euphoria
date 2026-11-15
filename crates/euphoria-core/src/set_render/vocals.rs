// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Blends where both tracks sing at once, and what would fix them: another
//! window where only one sings, the outgoing track without its vocals from
//! its stems, or the outgoing track gone before the incoming one sings.

use super::Blend;
use crate::harmony::transition::{CandidateWindow, Clash};

/// Bars the outgoing track needs to fade out before both would sing, for
/// [`VocalFix::FadeOutBefore`] to be offered.
pub const FADE_OUT_MINIMUM_BARS: usize = 2;

/// Both tracks singing during a blend.
#[derive(Debug, Clone, PartialEq)]
pub struct VocalOverlap {
    /// Bars of the blend where both sing.
    pub bars: usize,
    /// The first of them, counted from 1 at the start of the blend.
    pub first_bar: usize,
    /// What would fix it, the one that changes the sound least first.
    pub fixes: Vec<VocalFix>,
}

impl VocalOverlap {
    /// The warning in one line.
    pub fn warning(&self) -> String {
        let bars = if self.bars == 1 { "bar" } else { "bars" };
        format!(
            "Both tracks sing for {} {bars}, from bar {} of the blend",
            self.bars, self.first_bar
        )
    }
}

/// A way to keep two voices from singing over each other in a blend.
#[derive(Debug, Clone, PartialEq)]
pub enum VocalFix {
    /// Blend over another window the planner ranks, where only one sings.
    MoveWindow {
        /// The window.
        blend: Blend,
        /// Why the planner places it there.
        reason: String,
    },
    /// Play the outgoing track from its stems without the vocals from the
    /// start of the blend.
    CutOutgoingVocals,
    /// Swap the bass early and have the outgoing track faded out by this
    /// bar of the blend, counted from 1, before the incoming one sings.
    FadeOutBefore {
        /// The bar.
        bar: usize,
    },
}

impl VocalFix {
    /// What the fix does, in a few words, for a button or a list.
    pub fn label(&self) -> String {
        match self {
            VocalFix::MoveWindow { blend, .. } => format!(
                "Move the blend to bar {} into bar {}",
                blend.a_start_bar + 1,
                blend.b_start_bar + 1
            ),
            VocalFix::CutOutgoingVocals => "Cut the outgoing vocals with its stems".to_string(),
            VocalFix::FadeOutBefore { bar } => {
                format!("Swap the bass early and fade out by bar {bar}")
            }
        }
    }

    /// A short name for the command line, such as `move`.
    pub fn code(&self) -> &'static str {
        match self {
            VocalFix::MoveWindow { .. } => "move",
            VocalFix::CutOutgoingVocals => "cut",
            VocalFix::FadeOutBefore { .. } => "fade",
        }
    }
}

/// The overlap in `window`, with the fixes that apply, or `None` when the
/// two never sing together or their vocals were not checked.
/// `alternative` is the best other window that fits and has no vocals
/// together; `has_instrumental` says whether the outgoing track has stems.
pub(super) fn overlap_of(
    window: &CandidateWindow,
    alternative: Option<&CandidateWindow>,
    has_instrumental: bool,
) -> Option<VocalOverlap> {
    let first = window
        .clash_strip
        .iter()
        .position(|clash| *clash == Clash::Vocals)?;
    let mut fixes = Vec::new();
    if let Some(alternative) = alternative {
        fixes.push(VocalFix::MoveWindow {
            blend: blend_of(alternative),
            reason: super::plan::blend_reason(alternative),
        });
    }
    if has_instrumental {
        fixes.push(VocalFix::CutOutgoingVocals);
    }
    if first >= FADE_OUT_MINIMUM_BARS {
        fixes.push(VocalFix::FadeOutBefore { bar: first + 1 });
    }
    Some(VocalOverlap {
        bars: window.clash_counts.vocals,
        first_bar: first + 1,
        fixes,
    })
}

pub(super) fn blend_of(window: &CandidateWindow) -> Blend {
    Blend {
        a_start_bar: window.a_start_bar,
        b_start_bar: window.b_start_bar,
        bars: window.bars,
        bass_swap_bar: window.bass_swap_bar,
    }
}

/// Which fix to use when both tracks would sing in a blend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VocalFixChoice {
    /// The first fix that applies, the one that changes the sound least.
    First,
    /// [`VocalFix::MoveWindow`].
    Move,
    /// [`VocalFix::CutOutgoingVocals`].
    Cut,
    /// [`VocalFix::FadeOutBefore`].
    Fade,
}

impl VocalFixChoice {
    /// The fix among `fixes` this choice picks, if it applies.
    pub fn pick(self, fixes: &[VocalFix]) -> Option<&VocalFix> {
        fixes.iter().find(|fix| match self {
            VocalFixChoice::First => true,
            VocalFixChoice::Move => matches!(fix, VocalFix::MoveWindow { .. }),
            VocalFixChoice::Cut => matches!(fix, VocalFix::CutOutgoingVocals),
            VocalFixChoice::Fade => matches!(fix, VocalFix::FadeOutBefore { .. }),
        })
    }
}
