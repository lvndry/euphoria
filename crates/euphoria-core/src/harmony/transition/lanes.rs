// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The lanes of a track, one value per bar: its section, its energy, and
//! whether vocals, bass and drums sound. The clash of two lanes decides
//! which parts of a mix fight.

use serde::{Deserialize, Serialize};

use crate::analysis::TrackAnalysis;
use crate::detect::{BeatGrid, CueSection};
use crate::energy::EnergyCurvePoint;

/// The stretch of audio each stem level covers, in seconds.
pub const STEM_LEVEL_HOP_SECONDS: f64 = 0.25;

/// A stem is absent in a bar whose level is below this, 40 dB below full
/// scale. Separation leaves a little of the other instruments in every
/// stem, and this sits above that bleed: in an instrumental the vocal stem
/// measures around 50 to 60 dB below full scale.
pub const STEM_ABSENT_FLOOR: f32 = 0.01;

/// A stem sounds in a bar when its level is at least this share of the
/// body level of the same stem, 12 dB below it, the ratio the mashup
/// profile also uses for a stem that is active.
pub const STEM_PRESENT_RATIO: f32 = 0.25;

/// The body level of a stem is the level that this share of its bars with
/// sound stay at or below, so a few loud bars do not raise the bar.
pub const STEM_BODY_PERCENTILE: f32 = 0.9;

/// The root mean square level of three stems over short stretches of a
/// track, measured from its cached stems.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StemLevels {
    /// Seconds each value covers, from the start of the track.
    pub hop_seconds: f64,
    /// The level of the vocals stem, from 0 to 1.
    pub vocals: Vec<f32>,
    /// The level of the bass stem.
    pub bass: Vec<f32>,
    /// The level of the drums stem.
    pub drums: Vec<f32>,
}

/// One point of the energy of a track over time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EnergyPoint {
    /// Where the stretch starts, in seconds from the start of the track.
    pub start_seconds: f32,
    /// How long it is.
    pub duration_seconds: f32,
    /// Its energy from 1 (calm) to 10 (intense), or `None` when silent.
    pub energy: Option<f32>,
}

impl From<&EnergyCurvePoint> for EnergyPoint {
    fn from(point: &EnergyCurvePoint) -> Self {
        Self {
            start_seconds: point.start_seconds,
            duration_seconds: point.duration_seconds,
            energy: point.rating.map(|rating| 1.0 + 9.0 * rating.score),
        }
    }
}

/// Whether an instrument sounds in a bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Presence {
    /// It sounds.
    Present,
    /// It does not.
    Absent,
    /// There are no stems to tell by.
    Unknown,
}

/// What a section starting at a bar is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SectionLabel {
    /// The first bar with sound.
    Start,
    /// The first section at full energy.
    Main,
    /// A section without the kick after full energy.
    Breakdown,
    /// Full energy again after a breakdown.
    Drop,
    /// The closing section.
    Outro,
}

impl SectionLabel {
    /// The name shown to a DJ.
    pub fn name(self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Main => "Main",
            Self::Breakdown => "Breakdown",
            Self::Drop => "Drop",
            Self::Outro => "Outro",
        }
    }
}

impl From<CueSection> for SectionLabel {
    fn from(section: CueSection) -> Self {
        match section {
            CueSection::Start => Self::Start,
            CueSection::Main => Self::Main,
            CueSection::Breakdown => Self::Breakdown,
            CueSection::Drop => Self::Drop,
            CueSection::Outro => Self::Outro,
        }
    }
}

/// A section of a track: it runs from `start_bar` to the next section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanSection {
    /// The bar it starts on, counted from 0 at the first downbeat.
    pub start_bar: usize,
    /// What it is.
    pub label: SectionLabel,
}

/// Where the sections of a track come from. Section detection is being
/// rebuilt, so the planner reads sections only through this seam.
pub trait SectionSource {
    /// The sections of the track in order of bar.
    fn sections(&self, analysis: &TrackAnalysis) -> Vec<PlanSection>;
}

/// The sections of today's automatic cues.
#[derive(Debug, Clone, Copy, Default)]
pub struct AutomaticCues;

impl SectionSource for AutomaticCues {
    fn sections(&self, analysis: &TrackAnalysis) -> Vec<PlanSection> {
        let Some(grid) = analysis.beat_grid else {
            return Vec::new();
        };
        let bar_seconds = grid.bar_seconds();
        analysis
            .cues
            .iter()
            .filter_map(|cue| {
                let bars = (cue.seconds - grid.first_downbeat_seconds) / bar_seconds;
                (bars.is_finite() && bars > -0.5).then(|| PlanSection {
                    start_bar: bars.round().max(0.0) as usize,
                    label: cue.section.into(),
                })
            })
            .collect()
    }
}

/// Everything the planner reads of one track.
#[derive(Debug, Clone, Copy)]
pub struct PlanTrack<'track> {
    /// Its key, tempo and beat grid.
    pub analysis: &'track TrackAnalysis,
    /// Its sections, from a [`SectionSource`].
    pub sections: &'track [PlanSection],
    /// Its energy over time. Empty when not measured.
    pub energy: &'track [EnergyPoint],
    /// The levels of its stems, or `None` without cached stems.
    pub stems: Option<&'track StemLevels>,
}

/// One bar of a track.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BarLane {
    /// The bar, counted from 0 at the first downbeat.
    pub bar: usize,
    /// Seconds from the start of the track to the bar.
    pub start_seconds: f64,
    /// The section the bar is in, when sections are known.
    pub section: Option<SectionLabel>,
    /// The energy of the bar from 1 to 10, when measured.
    pub energy: Option<f32>,
    /// Whether vocals sound.
    pub vocals: Presence,
    /// Whether bass sounds.
    pub bass: Presence,
    /// Whether drums sound.
    pub drums: Presence,
}

/// The bars of a track from its first downbeat to its end.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackLanes {
    /// Seconds in a bar of the track's own grid.
    pub bar_seconds: f64,
    /// Every bar.
    pub bars: Vec<BarLane>,
    /// Whether the stems were measured.
    pub stems_known: bool,
}

impl TrackLanes {
    /// The lanes of `track` on `grid`.
    pub fn of(track: &PlanTrack<'_>, grid: &BeatGrid) -> Self {
        let bar_seconds = grid.bar_seconds();
        let usable = f64::from(track.analysis.duration_seconds) - grid.first_downbeat_seconds;
        let bar_count = if bar_seconds > 0.0 && usable > 0.0 {
            (usable / bar_seconds).floor() as usize
        } else {
            0
        };
        let starts: Vec<f64> = (0..bar_count)
            .map(|bar| grid.first_downbeat_seconds + bar as f64 * bar_seconds)
            .collect();
        let presence = |select: fn(&StemLevels) -> &Vec<f32>| match track.stems {
            Some(levels) => stem_presence(&bar_levels(
                select(levels),
                levels.hop_seconds,
                &starts,
                bar_seconds,
            )),
            None => vec![Presence::Unknown; bar_count],
        };
        let vocals = presence(|levels| &levels.vocals);
        let bass = presence(|levels| &levels.bass);
        let drums = presence(|levels| &levels.drums);
        let bars = starts
            .iter()
            .enumerate()
            .map(|(bar, start)| BarLane {
                bar,
                start_seconds: *start,
                section: track
                    .sections
                    .iter()
                    .rev()
                    .find(|section| section.start_bar <= bar)
                    .map(|section| section.label),
                energy: bar_energy(track.energy, *start, bar_seconds),
                vocals: vocals[bar],
                bass: bass[bar],
                drums: drums[bar],
            })
            .collect();
        Self {
            bar_seconds,
            bars,
            stems_known: track.stems.is_some(),
        }
    }
}

fn bar_levels(levels: &[f32], hop_seconds: f64, starts: &[f64], bar_seconds: f64) -> Vec<f32> {
    starts
        .iter()
        .map(|start| {
            let first = (start / hop_seconds).ceil().max(0.0) as usize;
            let last = (((start + bar_seconds) / hop_seconds).ceil() as usize).min(levels.len());
            if first >= last {
                return 0.0;
            }
            let squares: f32 = levels[first..last].iter().map(|level| level * level).sum();
            (squares / (last - first) as f32).sqrt()
        })
        .collect()
}

fn stem_presence(levels: &[f32]) -> Vec<Presence> {
    let mut sounding: Vec<f32> = levels
        .iter()
        .copied()
        .filter(|level| *level >= STEM_ABSENT_FLOOR)
        .collect();
    sounding.sort_by(f32::total_cmp);
    let body = if sounding.is_empty() {
        f32::INFINITY
    } else {
        let index = ((sounding.len() - 1) as f32 * STEM_BODY_PERCENTILE).round() as usize;
        sounding[index]
    };
    let threshold = (body * STEM_PRESENT_RATIO).max(STEM_ABSENT_FLOOR);
    levels
        .iter()
        .map(|level| {
            if *level >= threshold {
                Presence::Present
            } else {
                Presence::Absent
            }
        })
        .collect()
}

fn bar_energy(points: &[EnergyPoint], start: f64, bar_seconds: f64) -> Option<f32> {
    let end = start + bar_seconds;
    let center = |point: &EnergyPoint| {
        f64::from(point.start_seconds) + f64::from(point.duration_seconds) / 2.0
    };
    let inside: Vec<f32> = points
        .iter()
        .filter(|point| (start..end).contains(&center(point)))
        .filter_map(|point| point.energy)
        .collect();
    if !inside.is_empty() {
        return Some(inside.iter().sum::<f32>() / inside.len() as f32);
    }
    let middle = (start + end) / 2.0;
    points
        .iter()
        .filter(|point| point.energy.is_some())
        .min_by(|first, second| {
            (center(first) - middle)
                .abs()
                .total_cmp(&(center(second) - middle).abs())
        })
        .filter(|point| (center(point) - middle).abs() <= bar_seconds.max(1.0))
        .and_then(|point| point.energy)
}

/// What two tracks do in one bar of the overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Clash {
    /// Both sing.
    Vocals,
    /// Both play bass.
    Bass,
    /// Neither of those, as far as the stems say.
    Clear,
    /// A stem is missing, so it cannot be told.
    Unknown,
}

impl Clash {
    /// What `first` and `second` do together. A clash of vocals outranks one
    /// of bass.
    pub fn of(first: &BarLane, second: &BarLane) -> Self {
        let vocals = both(first.vocals, second.vocals);
        let bass = both(first.bass, second.bass);
        match (vocals, bass) {
            (Some(true), _) => Self::Vocals,
            (_, Some(true)) => Self::Bass,
            (Some(false), Some(false)) => Self::Clear,
            _ => Self::Unknown,
        }
    }
}

fn both(first: Presence, second: Presence) -> Option<bool> {
    match (first, second) {
        (Presence::Present, Presence::Present) => Some(true),
        (Presence::Absent, _) | (_, Presence::Absent) => Some(false),
        _ => None,
    }
}
