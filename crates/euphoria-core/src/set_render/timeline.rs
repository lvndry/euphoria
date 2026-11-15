// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where each track of a mix sits on the time of the mix, how fast it plays
//! at every moment, and how loud.

use super::{Blend, MixMove, MixPlan};
use crate::harmony::transition::reachable_rate;

/// After blending in at the tempo of the track before, a track glides back
/// to its own tempo over this many seconds of the mix, or over the time it
/// plays alone when that is shorter.
pub const TEMPO_RAMP_SECONDS: f64 = 16.0;

/// The bass is handed over in this share of a bar: one beat.
const BASS_SWAP_BARS: f64 = 0.25;

/// A blend as heard, in seconds of the mix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossfade {
    /// When the incoming track starts.
    pub start_seconds: f64,
    /// How long both play, by the bars of the blend.
    pub length_seconds: f64,
    /// When the bass starts to move from one track to the other.
    pub swap_seconds: f64,
    /// How long the bass takes to move.
    pub swap_length_seconds: f64,
}

/// One track laid on the time of the mix.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    /// When it is first heard, in seconds of the mix.
    pub start_seconds: f64,
    /// When it is last heard.
    pub end_seconds: f64,
    /// Where in the track it starts, in seconds of its own audio.
    pub source_start_seconds: f64,
    /// Where in the track it stops.
    pub source_end_seconds: f64,
    /// The speed it blends in at, as a share of its own: the tempo of the
    /// track before divided by its own. 1 when it does not blend in.
    pub entry_rate: f64,
    /// The blend in, when it has one.
    pub entry: Option<Crossfade>,
    /// The blend out, when it has one.
    pub exit: Option<Crossfade>,
    /// Seconds of the track played at the entry rate.
    entry_source_seconds: f64,
    /// Seconds of the mix over which the speed glides back to 1.
    ramp_seconds: f64,
}

impl Placement {
    /// Whether the track is played at any other speed than its own.
    pub fn is_stretched(&self) -> bool {
        (self.entry_rate - 1.0).abs() > 1e-9
    }

    /// The speed at `seconds` into the placement, in seconds of the mix.
    pub fn rate_at(&self, seconds: f64) -> f64 {
        let entry_seconds = self.entry_source_seconds / self.entry_rate;
        if seconds < entry_seconds {
            self.entry_rate
        } else if seconds < entry_seconds + self.ramp_seconds {
            let progress = (seconds - entry_seconds) / self.ramp_seconds;
            self.entry_rate + (1.0 - self.entry_rate) * progress
        } else {
            1.0
        }
    }

    /// Seconds of the track's own audio played by `seconds` into the
    /// placement, in seconds of the mix, from where it starts.
    pub fn source_offset_at(&self, seconds: f64) -> f64 {
        let entry_seconds = self.entry_source_seconds / self.entry_rate;
        if seconds < entry_seconds {
            return seconds * self.entry_rate;
        }
        let into_ramp = (seconds - entry_seconds).min(self.ramp_seconds);
        let ramped = if self.ramp_seconds > 0.0 {
            self.entry_rate * into_ramp
                + (1.0 - self.entry_rate) * into_ramp * into_ramp / (2.0 * self.ramp_seconds)
        } else {
            0.0
        };
        let after_ramp = (seconds - entry_seconds - self.ramp_seconds).max(0.0);
        self.entry_source_seconds + ramped + after_ramp
    }

    /// Seconds of the mix, from the start of the placement, at which the
    /// track reaches `offset` seconds past where it starts. Exact once the
    /// speed is back to 1, which is where every blend out lies.
    fn mix_offset_of(&self, offset: f64) -> f64 {
        let entry_seconds = self.entry_source_seconds / self.entry_rate;
        if offset <= self.entry_source_seconds {
            return offset / self.entry_rate;
        }
        let ramp_source = self.ramp_seconds * (self.entry_rate + 1.0) / 2.0;
        let past_ramp = offset - self.entry_source_seconds - ramp_source;
        entry_seconds + self.ramp_seconds + past_ramp.max(0.0)
    }

    /// The level of the track at `seconds` of the mix, from 0 to 1, as a
    /// whole and for its bass.
    pub fn gains_at(&self, seconds: f64) -> (f32, f32) {
        let mut volume = 1.0;
        let mut bass = 1.0;
        if let Some(entry) = &self.entry {
            let progress = (seconds - entry.start_seconds) / (entry.length_seconds / 2.0);
            volume *= (progress.clamp(0.0, 1.0) * std::f64::consts::FRAC_PI_2).sin();
            bass *= swap_progress(entry, seconds);
        }
        if let Some(exit) = &self.exit {
            let fade_from = exit.start_seconds + exit.length_seconds / 2.0;
            let fade_to = (exit.start_seconds + exit.length_seconds).min(self.end_seconds);
            let fade_from = fade_from.min(fade_to - 1e-3);
            let progress = (seconds - fade_from) / (fade_to - fade_from);
            volume *= (progress.clamp(0.0, 1.0) * std::f64::consts::FRAC_PI_2).cos();
            bass *= 1.0 - swap_progress(exit, seconds);
        }
        (volume as f32, bass as f32)
    }
}

fn swap_progress(crossfade: &Crossfade, seconds: f64) -> f64 {
    let progress = (seconds - crossfade.swap_seconds) / crossfade.swap_length_seconds;
    progress.clamp(0.0, 1.0)
}

/// Every track of a mix on the time of the mix.
#[derive(Debug, Clone, PartialEq)]
pub struct MixTimeline {
    /// One per track, in play order.
    pub placements: Vec<Placement>,
    /// How long the mix lasts, in seconds.
    pub length_seconds: f64,
}

/// Lays the tracks of `plan` on the time of the mix.
///
/// The first track plays from its start at its own speed. A track that
/// blends in starts on the bar of the blend, stretched to the tempo of the
/// track before, which plays at its own speed by then, and once the blend
/// is over glides back to its own tempo while it plays alone. A track that
/// does not blend in starts from its beginning when the one before ends.
///
/// Fails with the reason when a blend names a track without a beat grid,
/// asks for a speed a deck cannot play, or starts its blend out before its
/// blend in has ended.
pub fn mix_timeline(plan: &MixPlan) -> Result<MixTimeline, String> {
    if plan.transitions.len() + 1 != plan.tracks.len() && !plan.tracks.is_empty() {
        return Err(format!(
            "{} tracks need {} transitions, not {}",
            plan.tracks.len(),
            plan.tracks.len() - 1,
            plan.transitions.len()
        ));
    }
    let mut placements: Vec<Placement> = Vec::with_capacity(plan.tracks.len());
    let mut start_seconds = 0.0;
    for (index, track) in plan.tracks.iter().enumerate() {
        let incoming = index
            .checked_sub(1)
            .and_then(|previous| plan.transitions.get(previous));
        let outgoing = plan.transitions.get(index);
        let number = index + 1;
        let duration = track.duration_seconds.max(0.0);
        let grid_of = |what: &str| {
            track
                .grid
                .ok_or_else(|| format!("track {number} has no beat grid to {what}"))
        };
        let downbeat = |grid: &crate::detect::BeatGrid, bar: usize| {
            grid.first_downbeat_seconds + bar as f64 * grid.bar_seconds()
        };

        let mut source_start = 0.0;
        let mut entry_rate = 1.0;
        let mut entry_source_seconds = 0.0;
        let mut entry_blend = None;
        if let Some(MixMove::Blend(blend)) = incoming.map(|transition| transition.mix) {
            let grid = grid_of("blend into")?;
            let previous_grid = plan.tracks[index - 1]
                .grid
                .ok_or_else(|| format!("track {index} has no beat grid to blend out of"))?;
            entry_rate = previous_grid.bpm / grid.bpm;
            if !entry_rate.is_finite() || reachable_rate(entry_rate) != entry_rate {
                return Err(format!(
                    "track {number} would have to play at {entry_rate:.2} times its speed"
                ));
            }
            source_start = downbeat(&grid, blend.b_start_bar).max(0.0);
            entry_source_seconds = blend.bars as f64 * grid.bar_seconds();
            entry_blend = Some((blend, previous_grid.bar_seconds()));
        }
        let entry_end = source_start + entry_source_seconds;

        let mut source_end = duration;
        let mut exit_start_source = None;
        if let Some(MixMove::Blend(blend)) = outgoing.map(|transition| transition.mix) {
            let grid = grid_of("blend out of")?;
            let exit_start = downbeat(&grid, blend.a_start_bar);
            if exit_start < entry_end - 1e-6 {
                return Err(format!(
                    "track {number} would blend out at bar {} before its blend in ends",
                    blend.a_start_bar + 1
                ));
            }
            if exit_start >= duration {
                return Err(format!(
                    "track {number} ends before bar {}, where it would blend out",
                    blend.a_start_bar + 1
                ));
            }
            source_end = downbeat(&grid, blend.a_start_bar + blend.bars).min(duration);
            exit_start_source = Some(exit_start);
        }
        if source_end < entry_end {
            source_end = entry_end.min(duration.max(entry_end));
        }

        let room = exit_start_source.unwrap_or(source_end) - entry_end;
        let ramp_seconds = if (entry_rate - 1.0).abs() > 1e-9 {
            TEMPO_RAMP_SECONDS.min(2.0 * room.max(0.0) / (entry_rate + 1.0))
        } else {
            0.0
        };
        let mut placement = Placement {
            start_seconds,
            end_seconds: start_seconds,
            source_start_seconds: source_start,
            source_end_seconds: source_end,
            entry_rate,
            entry: None,
            exit: None,
            entry_source_seconds,
            ramp_seconds,
        };
        placement.end_seconds = start_seconds + placement.mix_offset_of(source_end - source_start);
        if let Some((blend, outgoing_bar_seconds)) = entry_blend {
            placement.entry = Some(crossfade(&blend, start_seconds, outgoing_bar_seconds));
        }
        if let (Some(previous), Some(entry)) = (placements.last_mut(), placement.entry) {
            previous.exit = Some(entry);
        }

        start_seconds = match (outgoing.map(|transition| transition.mix), exit_start_source) {
            (Some(MixMove::Blend(_)), Some(exit_start)) => {
                placement.start_seconds + placement.mix_offset_of(exit_start - source_start)
            }
            _ => placement.end_seconds,
        };
        placements.push(placement);
    }
    let length_seconds = placements
        .iter()
        .map(|placement| placement.end_seconds)
        .fold(0.0, f64::max);
    Ok(MixTimeline {
        placements,
        length_seconds,
    })
}

fn crossfade(blend: &Blend, start_seconds: f64, bar_seconds: f64) -> Crossfade {
    Crossfade {
        start_seconds,
        length_seconds: blend.bars as f64 * bar_seconds,
        swap_seconds: start_seconds + (blend.swap_bar() - 1) as f64 * bar_seconds,
        swap_length_seconds: BASS_SWAP_BARS * bar_seconds,
    }
}
