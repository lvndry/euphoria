// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Gives the sections of a track the names a DJ cues them by: where the
//! music starts, where it first plays at full energy, its breakdowns and
//! drops, and where the outro begins. The sections themselves come from
//! [`sections`](super::sections), which knows nothing of dance music; this
//! layer reads them against the track's own level. A track whose level does
//! not rise and fall that way gets numbered sections instead.

use std::borrow::Cow;

use crate::audio::AudioBuffer;
use crate::confidence::Confidence;
use crate::detect::beat_grid::{BEATS_PER_BAR, BeatGrid, OnsetEnvelope, grid_of_envelope};
use crate::detect::sections::{SectionBoundary, section_boundaries};

/// Bars are measured from this long before each downbeat.
const BAR_LEAD_SECONDS: f64 = 0.05;
/// The band of the kick and the bass ends about here.
const KICK_BAND_HZ: f32 = 150.0;
/// The grid has one tempo for the whole track, which is a little off the
/// tempo of the music, so the beats drift from it. The kick of each bar is
/// looked for this many steps of [`KICK_SEARCH_STEP_SECONDS`] either side of
/// where the kick of the bar before was found. A bar where that finds none
/// is searched again this many steps of [`KICK_WIDE_SEARCH_STEP_SECONDS`]
/// either side of the grid, which finds the kick again after a breakdown
/// that lasted long enough for the beats to drift out of the first search.
const KICK_SEARCH_STEPS: i32 = 4;
const KICK_SEARCH_STEP_SECONDS: f64 = 0.01;
const KICK_WIDE_SEARCH_STEPS: i32 = 11;
const KICK_WIDE_SEARCH_STEP_SECONDS: f64 = 0.02;
/// The band of the hats and the percussion starts about here.
const HIGH_BAND_HZ: f32 = 2_000.0;
/// The kick is measured over this long from just before each beat, and the
/// band between beats over as long halfway to the next beat.
const KICK_WINDOW_SECONDS: f64 = 0.1;

/// The name of a [`CueSection::Section`] cue before its number.
const SECTION_NAME_PREFIX: &str = "Section ";

/// The most cues a track gets, one for each hot cue pad from A to H.
pub const MAXIMUM_CUES: usize = 8;
/// Bars in a phrase. Sections of dance music start every 8, 16 or 32 bars
/// from the first bar.
const PHRASE_BARS: usize = 8;
/// A section change on a grid rated at least [`SNAPPING_CONFIDENCE`] moves
/// onto the nearest phrase when it is this close to it, and onto the
/// nearest bar otherwise. Music that is not played in phrases keeps its
/// changes where they are heard.
const PHRASE_SNAP_BARS: f64 = 1.0;
const SNAPPING_CONFIDENCE: Confidence = Confidence::Medium;
/// Shorter runs of bars than this are not a section of their own: a fill,
/// a short break before a drop or the last bar before silence belongs to
/// the section around it.
const MINIMUM_SECTION_BARS: usize = 4;
/// A track without an energy arc gets a numbered cue only where its
/// sections change this much, on the scale where the largest change of the
/// track is 1. Music that barely changes, such as a steady loop, has only
/// smaller changes and keeps the Start cue alone.
const NUMBERED_SECTION_STRENGTH: f32 = 0.4;
/// A fall from the body of a track is a DJ's breakdown when its low end is
/// this far below the body's, or when this much larger a share of its bars
/// lose the kick. On one DJ library almost every track with a fall has one
/// this deep, while half the live band recordings of SALAMI have none.
const BREAKDOWN_LOW_DROP_DB: f32 = 4.0;
const BREAKDOWN_KICK_DROP: f32 = 0.5;
/// A bar is silent when it is this far below the loudest bar of the track.
const SILENT_BELOW_LOUDEST_DB: f32 = 40.0;
/// A bar has a kick when the band below 150 Hz is at least this much louder
/// just after its beats than between them.
const KICK_PRESENT_DB: f32 = 3.0;
/// The level of the body of a track is the one this share of its bars reach.
const BODY_PERCENTILE: f32 = 0.75;
/// A section plays at the level of the body of the track when its loudness
/// is no more than this many decibels below it and its low end no more than
/// [`HIGH_LOW_DROP_DB`] below the low end of the body.
const HIGH_LOUDNESS_DROP_DB: f32 = 3.0;
const HIGH_LOW_DROP_DB: f32 = 6.0;

/// The body of a track has a kick when this share of the bars at its level
/// have one. Then a section needs the kick for as many of its bars to play
/// at the level of the body. A percussive groove whose kick the detector
/// does not find has sections told apart by level alone.
const BODY_KICK_SHARE: f32 = 0.5;

/// What a cue marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CueSection {
    /// The first bar with sound.
    Start,
    /// The first bar of the first section at full energy, when that is not
    /// the start.
    Main,
    /// The first bar of a section well below full energy that follows a
    /// section at full energy.
    Breakdown,
    /// The first bar at full energy after a breakdown.
    Drop,
    /// The first bar after the last section at full energy, when at least a
    /// phrase follows it.
    Outro,
    /// The start of a section of a track whose level does not rise and fall
    /// the way a DJ's breakdowns and drops do, such as a ballad, a jam or a
    /// classical piece: the number of the section, counting the start as 1.
    Section(u8),
}

impl CueSection {
    /// The name of the cue as a DJ program shows it.
    pub fn name(self) -> Cow<'static, str> {
        match self {
            Self::Start => Cow::Borrowed("Start"),
            Self::Main => Cow::Borrowed("Main"),
            Self::Breakdown => Cow::Borrowed("Breakdown"),
            Self::Drop => Cow::Borrowed("Drop"),
            Self::Outro => Cow::Borrowed("Outro"),
            Self::Section(number) => Cow::Owned(format!("{SECTION_NAME_PREFIX}{number}")),
        }
    }

    /// The section a name from [`CueSection::name`] stands for.
    pub fn from_name(name: &str) -> Option<Self> {
        if let Some(digits) = name.strip_prefix(SECTION_NAME_PREFIX) {
            return digits
                .parse::<u8>()
                .ok()
                .filter(|number| *number >= 2 && number.to_string() == digits)
                .map(Self::Section);
        }
        [
            Self::Start,
            Self::Main,
            Self::Breakdown,
            Self::Drop,
            Self::Outro,
        ]
        .into_iter()
        .find(|section| section.name() == name)
    }
}

/// A place to set a cue: the start of a bar of the beat grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionCue {
    /// Seconds from the start of the track.
    pub seconds: f64,
    /// What starts there.
    pub section: CueSection,
}

/// Fits a beat grid to `audio` near `tempo_bpm`, as
/// [`detect_beat_grid`](crate::detect::detect_beat_grid) does, and finds
/// the cues of the track on it, in order, at most [`MAXIMUM_CUES`]. A track
/// without a grid has no cues.
pub fn detect_beat_grid_and_cues(
    audio: &AudioBuffer,
    tempo_bpm: f64,
) -> (Option<BeatGrid>, Vec<SectionCue>) {
    let Some(envelope) = OnsetEnvelope::of_audio(audio) else {
        return (None, Vec::new());
    };
    let Some(grid) = grid_of_envelope(&envelope, tempo_bpm) else {
        return (None, Vec::new());
    };
    let cues = find_cues(&envelope, &grid);
    (Some(grid), cues)
}

/// The cues of `audio` on `grid`, which was not fitted to it, in order, at
/// most [`MAXIMUM_CUES`]: each falls on a bar of that grid. Empty when the
/// audio is too short or too silent to measure.
pub fn cues_on_grid(audio: &AudioBuffer, grid: &BeatGrid) -> Vec<SectionCue> {
    OnsetEnvelope::of_audio(audio).map_or_else(Vec::new, |envelope| find_cues(&envelope, grid))
}

/// The part of a track between two section changes.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Segment {
    /// The bar of the grid the segment starts in.
    start: usize,
    /// How many bars it lasts.
    bars: usize,
    /// Where its cue goes, in seconds from the start of the track.
    seconds: f64,
    /// How large the change that starts it is, from 0 to 1.
    strength: f32,
}

/// Whether a section plays at the energy of the body of the track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionEnergy {
    /// At the level the track plays at for most of its length.
    High,
    /// Below it: an intro, a breakdown or an outro.
    Low,
}

/// How one bar sounds, in decibels.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BarMeasure {
    /// The loudness of the whole bar.
    loudness: f32,
    /// The loudness below about 150 Hz, where the kick and the bass are.
    low: f32,
    /// The loudness above about 2 kHz, where the hats and the percussion are.
    high: f32,
    /// How much louder the band below 150 Hz is just after each beat than
    /// halfway between beats. A kick on the beat raises it; a sustained
    /// bass line or pad sounds as loud between the beats and does not.
    kick: f32,
}

/// `samples` without what a one pole low pass at [`HIGH_BAND_HZ`] keeps.
fn high_passed(samples: &[f32], sample_rate: f32) -> Vec<f32> {
    let coefficient = 1.0 - (-std::f32::consts::TAU * HIGH_BAND_HZ / sample_rate).exp();
    let mut stage = 0.0_f32;
    samples
        .iter()
        .map(|sample| {
            stage += coefficient * (sample - stage);
            sample - stage
        })
        .collect()
}

/// The [`BarMeasure`] of every bar of the grid that fits in the track.
///
/// Bars are measured from slightly before each downbeat, since the onset of
/// a beat comes before the moment the grid places it, and a bar measured
/// from the downbeat would end with the onset of the next.
fn bar_measures(envelope: &OnsetEnvelope, grid: &BeatGrid) -> Vec<BarMeasure> {
    let sample_rate = f64::from(envelope.sample_rate);
    let low_passed = low_passed(&envelope.samples, envelope.sample_rate);
    let high_passed = high_passed(&envelope.samples, envelope.sample_rate);
    let power = |samples: &[f32]| {
        samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len().max(1) as f32
    };
    let decibels = |power: f32| 10.0 * (power + 1e-12).log10();
    let window = |start: f64, length: f64| {
        let first = ((start.max(0.0)) * sample_rate) as usize;
        let last = (((start + length).max(0.0) * sample_rate) as usize).min(low_passed.len());
        &low_passed[first.min(last)..last]
    };
    let duration = envelope.samples.len() as f64 / sample_rate;
    let bar_seconds = grid.bar_seconds();
    let beat_seconds = grid.beat_seconds();
    let mut measures = Vec::new();
    let mut downbeat = grid.first_downbeat_seconds;
    let mut drift = 0.0_f64;
    while downbeat + bar_seconds <= duration {
        let first = ((downbeat - BAR_LEAD_SECONDS).max(0.0) * sample_rate) as usize;
        let last = (((downbeat + bar_seconds - BAR_LEAD_SECONDS) * sample_rate) as usize)
            .min(envelope.samples.len());
        let range = first.min(last)..last;
        let kick_contrast = |shift: f64| {
            let (mut on_beat, mut between_beats) = (0.0_f32, 0.0_f32);
            for beat in 0..BEATS_PER_BAR {
                let beat_time = downbeat + beat as f64 * beat_seconds - BAR_LEAD_SECONDS + shift;
                on_beat += power(window(beat_time, KICK_WINDOW_SECONDS));
                between_beats += power(window(beat_time + beat_seconds / 2.0, KICK_WINDOW_SECONDS));
            }
            decibels(on_beat) - decibels(between_beats)
        };
        let search = |center: f64, steps: i32, step_seconds: f64| {
            (-steps..=steps)
                .map(|step| {
                    let shift = center + f64::from(step) * step_seconds;
                    (shift, kick_contrast(shift))
                })
                .fold((center, f32::NEG_INFINITY), |best, candidate| {
                    if candidate.1 > best.1 {
                        candidate
                    } else {
                        best
                    }
                })
        };
        let (mut best_shift, mut best_contrast) =
            search(drift, KICK_SEARCH_STEPS, KICK_SEARCH_STEP_SECONDS);
        if best_contrast < KICK_PRESENT_DB {
            let (wide_shift, wide_contrast) =
                search(0.0, KICK_WIDE_SEARCH_STEPS, KICK_WIDE_SEARCH_STEP_SECONDS);
            if wide_contrast > best_contrast {
                (best_shift, best_contrast) = (wide_shift, wide_contrast);
            }
        }
        if best_contrast >= KICK_PRESENT_DB {
            drift = best_shift;
        }
        measures.push(BarMeasure {
            loudness: decibels(power(&envelope.samples[range.clone()])),
            low: decibels(power(&low_passed[range.clone()])),
            high: decibels(power(&high_passed[range])),
            kick: best_contrast,
        });
        downbeat += bar_seconds;
    }
    measures
}

/// `samples` through two one pole low passes at [`KICK_BAND_HZ`], which
/// keep the kick and the bass and take a 440 Hz tone down by about 20 dB.
fn low_passed(samples: &[f32], sample_rate: f32) -> Vec<f32> {
    let coefficient = 1.0 - (-std::f32::consts::TAU * KICK_BAND_HZ / sample_rate).exp();
    let mut first_stage = 0.0_f32;
    let mut second_stage = 0.0_f32;
    samples
        .iter()
        .map(|sample| {
            first_stage += coefficient * (sample - first_stage);
            second_stage += coefficient * (first_stage - second_stage);
            second_stage
        })
        .collect()
}

/// The first and the last bar with sound, or `None` when the track is
/// silent.
fn bars_with_sound(measures: &[BarMeasure]) -> Option<(usize, usize)> {
    let loudest = measures
        .iter()
        .map(|measure| measure.loudness)
        .fold(f32::NEG_INFINITY, f32::max);
    let has_sound = |measure: &BarMeasure| measure.loudness >= loudest - SILENT_BELOW_LOUDEST_DB;
    let first = measures.iter().position(has_sound)?;
    let last = measures.iter().rposition(has_sound)?;
    Some((first, last))
}

/// The bar a section change falls in, and the seconds its cue goes to. On
/// a grid rated at least [`SNAPPING_CONFIDENCE`], the cue goes to the
/// phrase line counted from `first` when one is within
/// [`PHRASE_SNAP_BARS`], and to the nearest bar otherwise. On a grid that
/// is not trusted, the cue stays where the change was heard.
fn placed(boundary: &SectionBoundary, grid: &BeatGrid, first: usize) -> (usize, f64) {
    let bar_seconds = grid.bar_seconds();
    let position = (boundary.seconds - grid.first_downbeat_seconds) / bar_seconds;
    if grid.confidence() < SNAPPING_CONFIDENCE {
        return (position.round().max(0.0) as usize, boundary.seconds);
    }
    let from_first = position - first as f64;
    let phrase = (from_first / PHRASE_BARS as f64).round() * PHRASE_BARS as f64;
    let bar_from_first = if (from_first - phrase).abs() <= PHRASE_SNAP_BARS {
        phrase
    } else {
        from_first.round()
    };
    let bar = (first as f64 + bar_from_first).max(0.0) as usize;
    (bar, grid.first_downbeat_seconds + bar as f64 * bar_seconds)
}

/// The segments of the bars from `first` to `bar_count`, split at
/// `boundaries`.
fn segments(
    boundaries: &[SectionBoundary],
    grid: &BeatGrid,
    first: usize,
    bar_count: usize,
) -> Vec<Segment> {
    let mut starts = vec![Segment {
        start: first,
        bars: 0,
        seconds: grid.first_downbeat_seconds + first as f64 * grid.bar_seconds(),
        strength: 1.0,
    }];
    for boundary in boundaries {
        let (bar, seconds) = placed(boundary, grid, first);
        let last = starts.last().map_or(first, |segment| segment.start);
        if bar >= last + MINIMUM_SECTION_BARS && bar + MINIMUM_SECTION_BARS <= bar_count {
            starts.push(Segment {
                start: bar,
                bars: 0,
                seconds,
                strength: boundary.strength,
            });
        }
    }
    let next_starts: Vec<usize> = starts
        .iter()
        .skip(1)
        .map(|segment| segment.start)
        .chain([bar_count])
        .collect();
    for (segment, next) in starts.iter_mut().zip(next_starts) {
        segment.bars = next - segment.start;
    }
    starts
}

/// The value that `share` of the bars of `segments` reach or stay below,
/// where every bar has the median value of its segment.
fn body_level(segments: &[(Segment, f32)], share: f32) -> f32 {
    let mut ordered: Vec<(Segment, f32)> = segments.to_vec();
    ordered.sort_by(|left, right| left.1.total_cmp(&right.1));
    let total: usize = ordered.iter().map(|(segment, _)| segment.bars).sum();
    let mut reached = 0;
    for (segment, level) in &ordered {
        reached += segment.bars;
        if reached as f32 >= total as f32 * share {
            return *level;
        }
    }
    ordered.last().map_or(0.0, |(_, level)| *level)
}

/// Whether each segment plays at the level of the body of the track: the
/// loudness and the low end that [`BODY_PERCENTILE`] of the bars reach, with
/// a kick when the track has one for most of its length. Neighbours at the
/// same energy are one section.
fn section_energies(
    segments: &[Segment],
    measures: &[BarMeasure],
) -> Vec<(Segment, SectionEnergy)> {
    let median = |segment: &Segment, value: fn(&BarMeasure) -> f32| {
        let mut values: Vec<f32> = measures[segment.start..segment.start + segment.bars]
            .iter()
            .map(value)
            .collect();
        values.sort_by(f32::total_cmp);
        values[values.len() / 2]
    };
    let loudness: Vec<(Segment, f32)> = segments
        .iter()
        .map(|segment| (*segment, median(segment, |measure| measure.loudness)))
        .collect();
    let low: Vec<(Segment, f32)> = segments
        .iter()
        .map(|segment| (*segment, median(segment, |measure| measure.low)))
        .collect();
    let body_loudness = body_level(&loudness, BODY_PERCENTILE);
    let body_low = body_level(&low, BODY_PERCENTILE);
    let at_body_level: Vec<bool> = segments
        .iter()
        .enumerate()
        .map(|(index, _)| {
            loudness[index].1 >= body_loudness - HIGH_LOUDNESS_DROP_DB
                && low[index].1 >= body_low - HIGH_LOW_DROP_DB
        })
        .collect();
    let kick_share = |segment: &Segment| {
        let bars = &measures[segment.start..segment.start + segment.bars];
        bars.iter()
            .filter(|measure| measure.kick >= KICK_PRESENT_DB)
            .count() as f32
            / bars.len().max(1) as f32
    };
    let (kick_bars, body_bars) = segments
        .iter()
        .zip(&at_body_level)
        .filter(|(_, at_body_level)| **at_body_level)
        .fold((0.0, 0.0), |(kick, total), (segment, _)| {
            (
                kick + kick_share(segment) * segment.bars as f32,
                total + segment.bars as f32,
            )
        });
    let body_has_kick = body_bars > 0.0 && kick_bars / body_bars >= BODY_KICK_SHARE;
    let mut energies: Vec<(Segment, SectionEnergy)> = Vec::new();
    for (segment, &at_body_level) in segments.iter().zip(&at_body_level) {
        let energy = if at_body_level && (!body_has_kick || kick_share(segment) >= BODY_KICK_SHARE)
        {
            SectionEnergy::High
        } else {
            SectionEnergy::Low
        };
        match energies.last_mut() {
            Some((last, last_energy)) if *last_energy == energy => last.bars += segment.bars,
            _ => energies.push((*segment, energy)),
        }
    }
    energies
}

/// The cues a DJ sets on a track whose sections rise to the level of its
/// body and fall from it, in order: the start, the main section, each
/// breakdown and drop, and the outro.
fn energy_cues(sections: &[(Segment, SectionEnergy)]) -> Vec<(f64, CueSection)> {
    let Some((first, _)) = sections.first() else {
        return Vec::new();
    };
    let mut cues = vec![(first.seconds, CueSection::Start)];
    let mut seen_high = false;
    let mut in_breakdown = false;
    for (index, &(segment, energy)) in sections.iter().enumerate() {
        match energy {
            SectionEnergy::High if !seen_high => {
                seen_high = true;
                cues.push((segment.seconds, CueSection::Main));
            }
            SectionEnergy::High if in_breakdown => {
                in_breakdown = false;
                cues.push((segment.seconds, CueSection::Drop));
            }
            SectionEnergy::Low if seen_high && !in_breakdown => {
                in_breakdown = true;
                let is_last = index + 1 == sections.len();
                let section = if is_last && segment.bars >= PHRASE_BARS {
                    CueSection::Outro
                } else {
                    CueSection::Breakdown
                };
                cues.push((segment.seconds, section));
            }
            _ => {}
        }
    }
    let mut placed: Vec<(f64, CueSection)> = Vec::new();
    for (seconds, section) in cues {
        if placed
            .iter()
            .all(|(other, _)| (other - seconds).abs() > 1e-6)
        {
            placed.push((seconds, section));
        }
    }
    placed
}

/// The cues of a track whose level does not follow an energy arc: the
/// start, then the start of each section that a change of at least
/// [`NUMBERED_SECTION_STRENGTH`] begins, numbered from 2, keeping the
/// largest changes when there are more than [`MAXIMUM_CUES`].
fn numbered_cues(segments: &[Segment]) -> Vec<(f64, CueSection)> {
    let Some((first, rest)) = segments.split_first() else {
        return Vec::new();
    };
    let mut kept: Vec<&Segment> = rest
        .iter()
        .filter(|segment| segment.strength >= NUMBERED_SECTION_STRENGTH)
        .collect();
    kept.sort_by(|left, right| right.strength.total_cmp(&left.strength));
    kept.truncate(MAXIMUM_CUES - 1);
    kept.sort_by(|left, right| left.seconds.total_cmp(&right.seconds));
    std::iter::once((first.seconds, CueSection::Start))
        .chain(
            kept.iter()
                .zip(2_u8..)
                .map(|(segment, number)| (segment.seconds, CueSection::Section(number))),
        )
        .collect()
}

/// How a stretch of bars sounds, from the medians of its bars.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SectionLevel {
    /// The median low end, in decibels.
    low: f32,
    /// The share of its bars with a kick.
    kick_share: f32,
}

fn section_level(segment: &Segment, measures: &[BarMeasure]) -> SectionLevel {
    let bars = &measures[segment.start..segment.start + segment.bars];
    let mut lows: Vec<f32> = bars.iter().map(|measure| measure.low).collect();
    lows.sort_by(f32::total_cmp);
    SectionLevel {
        low: lows
            .get(lows.len() / 2)
            .copied()
            .unwrap_or(f32::NEG_INFINITY),
        kick_share: bars
            .iter()
            .filter(|measure| measure.kick >= KICK_PRESENT_DB)
            .count() as f32
            / bars.len().max(1) as f32,
    }
}

/// Whether the energy cues of a track describe it. They must name more than
/// its start, and when the track falls below its body between two sections
/// at full energy, at least one of those falls must be a DJ's breakdown: the
/// low end at least [`BREAKDOWN_LOW_DROP_DB`] below the body's, or the kick
/// gone from at least [`BREAKDOWN_KICK_DROP`] more of its bars. A band that
/// plays a quieter verse or a solo dips less, and its sections are numbered
/// instead of being called breakdowns and drops.
fn follows_energy_arc(
    cues: &[(f64, CueSection)],
    sections: &[(Segment, SectionEnergy)],
    measures: &[BarMeasure],
) -> bool {
    if cues
        .iter()
        .all(|(_, section)| *section == CueSection::Start)
    {
        return false;
    }
    let high: Vec<usize> = sections
        .iter()
        .enumerate()
        .filter(|(_, (_, energy))| *energy == SectionEnergy::High)
        .map(|(index, _)| index)
        .collect();
    let (Some(&first_high), Some(&last_high)) = (high.first(), high.last()) else {
        return true;
    };
    let (mut body_bars, mut body_low, mut body_kick) = (0.0_f32, 0.0_f32, 0.0_f32);
    for &index in &high {
        let (segment, _) = &sections[index];
        let level = section_level(segment, measures);
        let bars = segment.bars as f32;
        body_bars += bars;
        body_low += level.low * bars;
        body_kick += level.kick_share * bars;
    }
    let body_low = body_low / body_bars.max(1.0);
    let body_kick = body_kick / body_bars.max(1.0);
    let mut breakdowns = sections[first_high..last_high]
        .iter()
        .filter(|(_, energy)| *energy == SectionEnergy::Low)
        .map(|(segment, _)| section_level(segment, measures))
        .peekable();
    if breakdowns.peek().is_none() {
        return true;
    }
    breakdowns.any(|level| {
        body_low - level.low >= BREAKDOWN_LOW_DROP_DB
            || body_kick - level.kick_share >= BREAKDOWN_KICK_DROP
    })
}

/// At most [`MAXIMUM_CUES`] of `cues`: the start, the main section and the
/// outro, then the breakdowns and drops in order.
fn keep_most_useful(cues: Vec<(f64, CueSection)>) -> Vec<(f64, CueSection)> {
    if cues.len() <= MAXIMUM_CUES {
        return cues;
    }
    let fixed = cues
        .iter()
        .filter(|(_, section)| {
            matches!(
                section,
                CueSection::Start | CueSection::Main | CueSection::Outro
            )
        })
        .count();
    let mut room = MAXIMUM_CUES - fixed;
    cues.into_iter()
        .filter(|(_, section)| match section {
            CueSection::Breakdown | CueSection::Drop if room > 0 => {
                room -= 1;
                true
            }
            CueSection::Breakdown | CueSection::Drop => false,
            _ => true,
        })
        .collect()
}

pub(crate) fn find_cues(envelope: &OnsetEnvelope, grid: &BeatGrid) -> Vec<SectionCue> {
    let measures = bar_measures(envelope, grid);
    let Some((first, last)) = bars_with_sound(&measures) else {
        return Vec::new();
    };
    let boundaries = section_boundaries(envelope, Some(grid));
    let segments = segments(&boundaries, grid, first, last + 1);
    let sections = section_energies(&segments, &measures);
    let energy = energy_cues(&sections);
    let cues = if follows_energy_arc(&energy, &sections, &measures) {
        keep_most_useful(energy)
    } else {
        numbered_cues(&segments)
    };
    cues.into_iter()
        .map(|(seconds, section)| SectionCue { seconds, section })
        .collect()
}
#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    const SAMPLE_RATE: u32 = 44_100;
    const TEMPO: f64 = 120.0;
    const BAR_SECONDS: f64 = 2.0;

    /// The parts that play in a stretch of bars.
    #[derive(Clone, Copy)]
    struct Parts {
        kick: bool,
        bass: bool,
        pad: bool,
    }

    /// A track at 120 BPM, its first downbeat at `offset` seconds, built
    /// from stretches of (bars, parts).
    fn arrangement(offset: f64, stretches: &[(usize, Parts)]) -> AudioBuffer {
        let bars: usize = stretches.iter().map(|(bars, _)| bars).sum();
        let length = ((offset + bars as f64 * BAR_SECONDS + 1.0) * f64::from(SAMPLE_RATE)) as usize;
        let mut samples = vec![0.0_f32; length];
        let mut bar_start = offset;
        let mut bar_index = 0_usize;
        let mut noise_state = 0x2545_f491_4f6c_dd1d_u64;
        for &(bar_count, parts) in stretches {
            for _ in 0..bar_count {
                let bass_frequency = if bar_index.is_multiple_of(2) {
                    110.0
                } else {
                    82.4
                };
                bar_index += 1;
                for beat in 0..4 {
                    let beat_start = bar_start + f64::from(beat) * 60.0 / TEMPO;
                    let first = (beat_start * f64::from(SAMPLE_RATE)) as usize;
                    let beat_length = (60.0 / TEMPO * f64::from(SAMPLE_RATE)) as usize;
                    for offset in 0..beat_length {
                        let time = offset as f64 / f64::from(SAMPLE_RATE);
                        let absolute = (first + offset) as f64 / f64::from(SAMPLE_RATE);
                        let mut value = 0.0;
                        if parts.kick {
                            value += 0.6
                                * (std::f64::consts::TAU * 55.0 * time).sin()
                                * (-time / 0.05).exp();
                        }
                        if parts.bass {
                            value +=
                                0.25 * (std::f64::consts::TAU * bass_frequency * absolute).sin();
                        }
                        if parts.bass && beat % 2 == 1 && time < 0.08 {
                            noise_state ^= noise_state << 13;
                            noise_state ^= noise_state >> 7;
                            noise_state ^= noise_state << 17;
                            let unit = (noise_state >> 40) as f64 / (1_u64 << 24) as f64;
                            value += 0.3 * (2.0 * unit - 1.0) * (-time / 0.02).exp();
                        }
                        if parts.pad {
                            value += 0.08 * (std::f64::consts::TAU * 440.0 * absolute).sin()
                                + 0.08 * (std::f64::consts::TAU * 554.4 * absolute).sin();
                        }
                        if let Some(sample) = samples.get_mut(first + offset) {
                            *sample = value as f32;
                        }
                    }
                }
                bar_start += BAR_SECONDS;
            }
        }
        AudioBuffer::new(samples, NonZeroU32::new(SAMPLE_RATE).unwrap())
    }

    const INTRO: Parts = Parts {
        kick: true,
        bass: false,
        pad: false,
    };
    const FULL: Parts = Parts {
        kick: true,
        bass: true,
        pad: true,
    };
    const BREAKDOWN: Parts = Parts {
        kick: false,
        bass: false,
        pad: true,
    };

    fn cues_of(audio: &AudioBuffer) -> Vec<(f64, CueSection)> {
        let (grid, cues) = detect_beat_grid_and_cues(audio, TEMPO);
        assert!(grid.is_some());
        cues.into_iter()
            .map(|cue| (cue.seconds, cue.section))
            .collect()
    }

    fn assert_cues(found: &[(f64, CueSection)], expected: &[(f64, CueSection)]) {
        assert_cues_within(found, expected, 0.05);
    }

    /// Without a kick the grid is not trusted, and the cues stay where the
    /// changes are heard, to within a unit of the section finder.
    fn assert_cues_within(
        found: &[(f64, CueSection)],
        expected: &[(f64, CueSection)],
        tolerance: f64,
    ) {
        assert_eq!(found.len(), expected.len(), "{found:?}");
        for ((seconds, section), (expected_seconds, expected_section)) in found.iter().zip(expected)
        {
            assert_eq!(section, expected_section, "{found:?}");
            assert!(
                (seconds - expected_seconds).abs() < tolerance,
                "{section:?} at {seconds}, expected {expected_seconds}"
            );
        }
    }

    #[test]
    fn a_dance_arrangement_gets_a_cue_at_every_section_on_its_phrases() {
        let audio = arrangement(
            0.5,
            &[
                (16, INTRO),
                (32, FULL),
                (16, BREAKDOWN),
                (32, FULL),
                (16, INTRO),
            ],
        );

        let cues = cues_of(&audio);

        assert_cues(
            &cues,
            &[
                (0.5, CueSection::Start),
                (32.5, CueSection::Main),
                (96.5, CueSection::Breakdown),
                (128.5, CueSection::Drop),
                (192.5, CueSection::Outro),
            ],
        );
    }

    #[test]
    fn a_track_that_starts_at_full_energy_has_no_separate_main_cue_or_outro_without_one() {
        let audio = arrangement(0.0, &[(24, FULL), (16, BREAKDOWN), (24, FULL)]);

        let cues = cues_of(&audio);

        assert_cues(
            &cues,
            &[
                (0.0, CueSection::Start),
                (48.0, CueSection::Breakdown),
                (80.0, CueSection::Drop),
            ],
        );
    }

    #[test]
    fn a_breakdown_that_keeps_a_steady_bass_line_is_still_a_breakdown() {
        let bass_without_kick = Parts {
            kick: false,
            bass: true,
            pad: true,
        };
        let audio = arrangement(0.0, &[(16, FULL), (16, bass_without_kick), (16, FULL)]);

        let cues = cues_of(&audio);

        assert_cues(
            &cues,
            &[
                (0.0, CueSection::Start),
                (32.0, CueSection::Breakdown),
                (64.0, CueSection::Drop),
            ],
        );
    }

    #[test]
    fn a_short_break_before_a_drop_is_not_a_breakdown() {
        let audio = arrangement(0.0, &[(16, INTRO), (30, FULL), (2, BREAKDOWN), (16, FULL)]);

        let cues = cues_of(&audio);

        assert_cues(&cues, &[(0.0, CueSection::Start), (32.0, CueSection::Main)]);
    }

    const GROOVE: Parts = Parts {
        kick: false,
        bass: true,
        pad: true,
    };
    const KICK_AND_BASS: Parts = Parts {
        kick: true,
        bass: true,
        pad: false,
    };

    #[test]
    fn a_groove_without_a_kick_after_a_kick_intro_is_the_main_section_not_an_outro() {
        let audio = arrangement(0.0, &[(16, INTRO), (48, GROOVE)]);

        let cues = cues_of(&audio);

        assert_cues(&cues, &[(0.0, CueSection::Start), (32.0, CueSection::Main)]);
    }

    #[test]
    fn a_loud_intro_before_a_quieter_steady_body_is_a_numbered_section_not_an_outro() {
        let audio = arrangement(0.0, &[(16, FULL), (48, KICK_AND_BASS)]);

        let cues = cues_of(&audio);

        assert_cues(
            &cues,
            &[(0.0, CueSection::Start), (32.0, CueSection::Section(2))],
        );
    }

    #[test]
    fn a_groove_without_a_kick_still_has_its_breakdown_drop_and_outro() {
        let audio = arrangement(
            0.0,
            &[
                (16, INTRO),
                (32, GROOVE),
                (16, BREAKDOWN),
                (32, GROOVE),
                (16, BREAKDOWN),
            ],
        );

        let cues = cues_of(&audio);

        assert_cues_within(
            &cues,
            &[
                (0.0, CueSection::Start),
                (32.0, CueSection::Main),
                (96.0, CueSection::Breakdown),
                (128.0, CueSection::Drop),
                (192.0, CueSection::Outro),
            ],
            0.55,
        );
    }

    type Scenario = (
        &'static str,
        f64,
        Vec<(usize, Parts)>,
        Vec<(usize, CueSection)>,
    );

    #[test]
    fn synthetic_arrangements_get_the_cues_of_their_sections() {
        use CueSection::*;
        let scenarios: Vec<Scenario> = vec![
            (
                "two breakdowns",
                0.0,
                vec![
                    (8, INTRO),
                    (24, FULL),
                    (8, BREAKDOWN),
                    (24, FULL),
                    (8, BREAKDOWN),
                    (32, FULL),
                    (16, INTRO),
                ],
                vec![
                    (0, Start),
                    (8, Main),
                    (32, Breakdown),
                    (40, Drop),
                    (64, Breakdown),
                    (72, Drop),
                    (104, Outro),
                ],
            ),
            (
                "long intro and outro",
                0.0,
                vec![(32, INTRO), (64, FULL), (32, INTRO)],
                vec![(0, Start), (32, Main), (96, Outro)],
            ),
            (
                "groove long intro and outro",
                0.0,
                vec![(32, INTRO), (64, GROOVE), (32, BREAKDOWN)],
                vec![(0, Start), (32, Main), (96, Outro)],
            ),
        ];
        for (name, offset, stretches, expected) in &scenarios {
            let audio = arrangement(*offset, stretches);
            let found = cues_of(&audio);
            let expected: Vec<(f64, CueSection)> = expected
                .iter()
                .map(|(bar, section)| (found[0].0 + *bar as f64 * BAR_SECONDS, *section))
                .collect();
            assert_eq!(found.len(), expected.len(), "{name}: {found:?}");
            for ((seconds, section), (expected_seconds, expected_section)) in
                found.iter().zip(&expected)
            {
                assert_eq!(section, expected_section, "{name}: {found:?}");
                assert!(
                    (seconds - expected_seconds).abs() < 0.05,
                    "{name}: {found:?}"
                );
            }
        }
    }

    #[test]
    fn a_few_silent_bars_at_the_end_do_not_turn_the_last_section_into_an_outro() {
        let silence = Parts {
            kick: false,
            bass: false,
            pad: false,
        };
        let audio = arrangement(
            0.0,
            &[
                (16, INTRO),
                (32, FULL),
                (16, BREAKDOWN),
                (32, FULL),
                (3, silence),
            ],
        );

        let cues = cues_of(&audio);

        assert_cues(
            &cues,
            &[
                (0.0, CueSection::Start),
                (32.0, CueSection::Main),
                (96.0, CueSection::Breakdown),
                (128.0, CueSection::Drop),
            ],
        );
    }

    #[test]
    fn many_breakdowns_keep_the_start_main_and_outro_and_the_first_breaks() {
        let mut cues = vec![(0.0, CueSection::Start), (8.0, CueSection::Main)];
        for index in 0..5 {
            cues.push((16.0 + 16.0 * f64::from(index), CueSection::Breakdown));
            cues.push((24.0 + 16.0 * f64::from(index), CueSection::Drop));
        }
        cues.push((200.0, CueSection::Outro));

        let kept = keep_most_useful(cues);

        assert_eq!(kept.len(), MAXIMUM_CUES);
        assert_eq!(kept[0], (0.0, CueSection::Start));
        assert_eq!(kept[1], (8.0, CueSection::Main));
        assert_eq!(
            kept[2..7]
                .iter()
                .map(|(seconds, _)| *seconds)
                .collect::<Vec<_>>(),
            [16.0, 24.0, 32.0, 40.0, 48.0]
        );
        assert_eq!(kept[7], (200.0, CueSection::Outro));
    }

    fn segment(seconds: f64, strength: f32) -> Segment {
        Segment {
            start: seconds as usize,
            bars: 1,
            seconds,
            strength,
        }
    }

    #[test]
    fn a_track_without_an_energy_arc_gets_numbered_sections_at_its_largest_changes() {
        let mut segments = vec![segment(0.0, 1.0)];
        for index in 1..=9 {
            segments.push(segment(f64::from(index) * 20.0, 0.3 + 0.1 * index as f32));
        }

        let cues = numbered_cues(&segments);

        assert_eq!(cues.len(), MAXIMUM_CUES);
        assert_eq!(cues[0], (0.0, CueSection::Start));
        assert_eq!(cues[1], (60.0, CueSection::Section(2)));
        assert_eq!(cues[7], (180.0, CueSection::Section(8)));
        assert!(!follows_energy_arc(&[(0.0, CueSection::Start)], &[], &[]));
        assert!(follows_energy_arc(
            &[(0.0, CueSection::Start), (30.0, CueSection::Main)],
            &[],
            &[]
        ));
    }

    #[test]
    fn silence_before_the_music_moves_the_start_and_no_beat_means_no_cues() {
        let silent_start = Parts {
            kick: false,
            bass: false,
            pad: false,
        };
        let audio = arrangement(0.0, &[(4, silent_start), (16, FULL)]);
        let cues = cues_of(&audio);
        assert_eq!(cues[0].1, CueSection::Start);
        assert!((cues[0].0 - 8.0).abs() < 0.05, "{cues:?}");

        let silence = AudioBuffer::new(
            vec![0.0; 20 * SAMPLE_RATE as usize],
            NonZeroU32::new(SAMPLE_RATE).unwrap(),
        );
        assert_eq!(
            detect_beat_grid_and_cues(&silence, TEMPO),
            (None, Vec::new())
        );
    }

    #[test]
    fn a_dip_that_keeps_the_low_end_and_the_kick_is_not_a_breakdown() {
        let bar = |low: f32, kick: f32| BarMeasure {
            loudness: -10.0,
            low,
            high: -20.0,
            kick,
        };
        let stretch = |start: usize, seconds: f64| Segment {
            start,
            bars: 8,
            seconds,
            strength: 1.0,
        };
        let sections = [
            (stretch(0, 0.0), SectionEnergy::High),
            (stretch(8, 16.0), SectionEnergy::Low),
            (stretch(16, 32.0), SectionEnergy::High),
        ];
        let cues = [
            (0.0, CueSection::Start),
            (16.0, CueSection::Breakdown),
            (32.0, CueSection::Drop),
        ];
        let with_dip = |low: f32, kick: f32| -> Vec<BarMeasure> {
            (0..24)
                .map(|index| {
                    if (8..16).contains(&index) {
                        bar(low, kick)
                    } else {
                        bar(-12.0, 6.0)
                    }
                })
                .collect()
        };

        assert!(!follows_energy_arc(&cues, &sections, &with_dip(-14.0, 6.0)));
        assert!(follows_energy_arc(&cues, &sections, &with_dip(-20.0, 6.0)));
        assert!(follows_energy_arc(&cues, &sections, &with_dip(-14.0, 0.0)));
    }

    #[test]
    fn cue_names_read_back() {
        for section in [
            CueSection::Start,
            CueSection::Main,
            CueSection::Breakdown,
            CueSection::Drop,
            CueSection::Outro,
            CueSection::Section(2),
            CueSection::Section(12),
        ] {
            assert_eq!(CueSection::from_name(&section.name()), Some(section));
        }
        assert_eq!(
            CueSection::from_name("Section 3")
                .map(CueSection::name)
                .as_deref(),
            Some("Section 3")
        );
        assert_eq!(CueSection::from_name("Chorus"), None);
        assert_eq!(CueSection::from_name("Section 1"), None);
        assert_eq!(CueSection::from_name("Section 03"), None);
        assert_eq!(CueSection::from_name("Section "), None);
    }
}
