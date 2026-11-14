// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Finds the places in a track where a DJ sets cues: where the music
//! starts, where it first plays at full energy, its breakdowns and drops,
//! and where the outro begins. Every cue falls on the start of a bar of the
//! beat grid, at a phrase boundary.

use crate::audio::AudioBuffer;
use crate::detect::beat_grid::{BEATS_PER_BAR, BeatGrid, OnsetEnvelope, grid_of_envelope};

/// Bars are measured from this long before each downbeat.
const BAR_LEAD_SECONDS: f64 = 0.05;
/// The band of the kick and the bass ends about here.
const KICK_BAND_HZ: f32 = 150.0;
/// The grid has one tempo for the whole track, which is a little off the
/// tempo of the music, so the beats drift from it. The kick of each bar is
/// looked for this many steps of [`KICK_SEARCH_STEP_SECONDS`] either side of
/// where the kick of the bar before was found, up to
/// [`MAXIMUM_KICK_DRIFT_SECONDS`] from the grid.
const KICK_SEARCH_STEPS: i32 = 4;
const KICK_SEARCH_STEP_SECONDS: f64 = 0.01;
const MAXIMUM_KICK_DRIFT_SECONDS: f64 = 0.3;
/// The band of the hats and the percussion starts about here.
const HIGH_BAND_HZ: f32 = 2_000.0;
/// The kick is measured over this long from just before each beat, and the
/// band between beats over as long halfway to the next beat.
const KICK_WINDOW_SECONDS: f64 = 0.1;

/// The most cues a track gets, one for each hot cue pad from A to H.
pub const MAXIMUM_CUES: usize = 8;
/// Bars in a phrase. Sections of dance music start every 8, 16 or 32 bars
/// from the first bar.
const PHRASE_BARS: usize = 8;
/// Half a phrase. A section change found more than [`PHRASE_SNAP_BARS`]
/// from a phrase boundary is placed on the nearest half phrase instead,
/// since sections sometimes start four bars off the count.
const HALF_PHRASE_BARS: usize = 4;
/// How far from a phrase boundary a change found from the bars is moved
/// onto it.
const PHRASE_SNAP_BARS: usize = 2;
/// Shorter runs of bars than this are not a section of their own: a fill or
/// a one bar break before a drop belongs to the section around it.
const MINIMUM_SECTION_BARS: usize = 8;
/// A bar is silent when it is this far below the loudest bar of the track.
const SILENT_BELOW_LOUDEST_DB: f32 = 40.0;
/// A bar has a kick when the band below 150 Hz is at least this much louder
/// just after its beats than between them.
const KICK_PRESENT_DB: f32 = 3.0;
/// Features further below the loudest bar of the track than this count as
/// this far below it.
const FEATURE_FLOOR_DB: f32 = 30.0;
/// The features of a bar vary at least this much, in decibels, over a track,
/// so that a steady track does not turn small differences into section
/// changes.
const MINIMUM_SPREAD_DB: f32 = 2.0;
/// A bar with a kick counts as this many decibels in the kick feature.
const KICK_FEATURE_DB: f32 = 3.0;
/// Bars of each window the novelty compares around a bar.
const NOVELTY_HALF_WIDTH_BARS: usize = 8;
/// The windows of the novelty have at least this many bars, so that the
/// last and the first bars have no novelty.
const MINIMUM_NOVELTY_WIDTH: usize = 4;
/// A peak of the novelty at least this large is a change of section.
const BOUNDARY_NOVELTY: f32 = 1.0;
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
    /// The first bar of a section without the kick that follows a section
    /// at full energy.
    Breakdown,
    /// The first bar at full energy after a breakdown.
    Drop,
    /// The first bar after the last section at full energy, when at least a
    /// phrase follows it.
    Outro,
}

impl CueSection {
    /// The name of the cue as a DJ program shows it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Start => "Start",
            Self::Main => "Main",
            Self::Breakdown => "Breakdown",
            Self::Drop => "Drop",
            Self::Outro => "Outro",
        }
    }

    /// The section a name from [`CueSection::name`] stands for.
    pub fn from_name(name: &str) -> Option<Self> {
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
    /// The first bar of the segment.
    start: usize,
    /// How many bars it lasts.
    bars: usize,
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
        let (best_shift, best_contrast) = (-KICK_SEARCH_STEPS..=KICK_SEARCH_STEPS)
            .map(|step| {
                let shift = drift + f64::from(step) * KICK_SEARCH_STEP_SECONDS;
                (shift, kick_contrast(shift))
            })
            .fold((drift, f32::NEG_INFINITY), |best, candidate| {
                if candidate.1 > best.1 {
                    candidate
                } else {
                    best
                }
            });
        if best_contrast >= KICK_PRESENT_DB {
            drift = best_shift.clamp(-MAXIMUM_KICK_DRIFT_SECONDS, MAXIMUM_KICK_DRIFT_SECONDS);
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

/// How many numbers describe a bar for the segmentation: its loudness, its
/// low end, its high end and whether it has a kick.
const FEATURE_COUNT: usize = 4;

/// Each of the four numbers of a bar in units of how much it varies over the
/// track, so that a change in any of them shows as a distance.
type BarFeatures = [f32; FEATURE_COUNT];

/// The first bar with sound, or `None` when the track is silent.
fn first_bar_with_sound(measures: &[BarMeasure]) -> Option<usize> {
    let loudest = measures
        .iter()
        .map(|measure| measure.loudness)
        .fold(f32::NEG_INFINITY, f32::max);
    measures
        .iter()
        .position(|measure| measure.loudness >= loudest - SILENT_BELOW_LOUDEST_DB)
}

/// The features of every bar of `measures`, which start at the first bar with
/// sound. A bar far below the loudest bars counts as that far below, so a
/// silent tail does not stretch the scale the other bars are measured on.
fn bar_features(measures: &[BarMeasure]) -> Vec<BarFeatures> {
    let clamped = |value: fn(&BarMeasure) -> f32| {
        let loudest = measures.iter().map(value).fold(f32::NEG_INFINITY, f32::max);
        move |measure: &BarMeasure| value(measure).max(loudest - FEATURE_FLOOR_DB)
    };
    let loudness = clamped(|measure| measure.loudness);
    let low = clamped(|measure| measure.low);
    let high = clamped(|measure| measure.high);
    let mut features: Vec<BarFeatures> = measures
        .iter()
        .map(|measure| {
            [
                loudness(measure),
                low(measure),
                high(measure),
                if measure.kick >= KICK_PRESENT_DB {
                    KICK_FEATURE_DB
                } else {
                    0.0
                },
            ]
        })
        .collect();
    let count = features.len().max(1) as f32;
    for column in 0..FEATURE_COUNT {
        let mean = features.iter().map(|bar| bar[column]).sum::<f32>() / count;
        let variance = features
            .iter()
            .map(|bar| (bar[column] - mean).powi(2))
            .sum::<f32>()
            / count;
        let spread = variance.sqrt().max(MINIMUM_SPREAD_DB);
        for bar in &mut features {
            bar[column] = (bar[column] - mean) / spread;
        }
    }
    features
}

/// How much the music changes at each bar: the checkerboard kernel novelty
/// of Foote on the distances between bars. Bars of a window before the bar
/// are compared with bars of a window after it; the more they differ from
/// each other, and the less they differ among themselves, the larger it is.
fn novelty(features: &[BarFeatures]) -> Vec<f32> {
    let distance = |first: &BarFeatures, second: &BarFeatures| -> f32 {
        first
            .iter()
            .zip(second)
            .map(|(left, right)| (left - right).powi(2))
            .sum()
    };
    let mean_distance = |first: &[BarFeatures], second: &[BarFeatures]| -> f32 {
        let total: f32 = first
            .iter()
            .flat_map(|left| second.iter().map(move |right| distance(left, right)))
            .sum();
        total / (first.len() * second.len()) as f32
    };
    (0..features.len())
        .map(|bar| {
            let width = NOVELTY_HALF_WIDTH_BARS.min(bar).min(features.len() - bar);
            if width < MINIMUM_NOVELTY_WIDTH {
                return 0.0;
            }
            let before = &features[bar - width..bar];
            let after = &features[bar..bar + width];
            mean_distance(before, after)
                - (mean_distance(before, before) + mean_distance(after, after)) / 2.0
        })
        .collect()
}

/// The bars, counted from the first bar with sound, where a new section
/// starts: the peaks of the novelty above [`BOUNDARY_NOVELTY`], the highest
/// first, at least [`MINIMUM_SECTION_BARS`] from each other and from the
/// ends.
fn boundaries(novelty: &[f32]) -> Vec<usize> {
    let reach = MINIMUM_SECTION_BARS / 2;
    let mut peaks: Vec<usize> = (MINIMUM_SECTION_BARS
        ..novelty.len().saturating_sub(MINIMUM_SECTION_BARS - 1))
        .filter(|&bar| {
            novelty[bar] >= BOUNDARY_NOVELTY
                && (bar.saturating_sub(reach)..(bar + reach + 1).min(novelty.len()))
                    .all(|other| novelty[other] <= novelty[bar])
        })
        .collect();
    peaks.sort_by(|left, right| novelty[*right].total_cmp(&novelty[*left]));
    let mut accepted: Vec<usize> = Vec::new();
    for bar in peaks {
        if accepted
            .iter()
            .all(|other| other.abs_diff(bar) >= MINIMUM_SECTION_BARS)
        {
            accepted.push(bar);
        }
    }
    accepted.sort_unstable();
    accepted
}

/// The segments of the bars from `first` to `bar_count`, split at
/// `boundaries` (counted from `first`) moved onto the phrases.
fn segments(boundaries: &[usize], first: usize, bar_count: usize) -> Vec<Segment> {
    let mut starts = vec![first];
    for &boundary in boundaries {
        let bar = snapped_to_phrase(first + boundary, first);
        let last = *starts.last().unwrap_or(&first);
        if bar >= last + MINIMUM_SECTION_BARS && bar + MINIMUM_SECTION_BARS <= bar_count {
            starts.push(bar);
        }
    }
    starts
        .iter()
        .enumerate()
        .map(|(index, &start)| Segment {
            start,
            bars: starts.get(index + 1).copied().unwrap_or(bar_count) - start,
        })
        .collect()
}

/// The value that `share` of the bars of `segments` reach or stay below,
/// where every bar has the value of the mean of its segment.
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
    let mean = |segment: &Segment, value: fn(&BarMeasure) -> f32| {
        let bars = &measures[segment.start..segment.start + segment.bars];
        bars.iter().map(value).sum::<f32>() / bars.len().max(1) as f32
    };
    let loudness: Vec<(Segment, f32)> = segments
        .iter()
        .map(|segment| (*segment, mean(segment, |measure| measure.loudness)))
        .collect();
    let low: Vec<(Segment, f32)> = segments
        .iter()
        .map(|segment| (*segment, mean(segment, |measure| measure.low)))
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

/// `bar` moved onto the phrase boundary counted from `first`, or onto the
/// nearest half phrase when it is further than [`PHRASE_SNAP_BARS`] from
/// the nearest phrase boundary.
fn snapped_to_phrase(bar: usize, first: usize) -> usize {
    let offset = bar - first;
    let nearest = |unit: usize| (offset + unit / 2) / unit * unit;
    let phrase = nearest(PHRASE_BARS);
    if phrase.abs_diff(offset) <= PHRASE_SNAP_BARS {
        first + phrase
    } else {
        first + nearest(HALF_PHRASE_BARS)
    }
}

/// The cues of a track, as (bar, section), in order.
fn cue_bars(measures: &[BarMeasure], first: usize) -> Vec<(usize, CueSection)> {
    let features = bar_features(&measures[first..]);
    let starts = boundaries(&novelty(&features));
    let sections = section_energies(&segments(&starts, first, measures.len()), measures);
    let mut cues = vec![(first, CueSection::Start)];
    let mut seen_high = false;
    let mut in_breakdown = false;
    for (index, &(segment, energy)) in sections.iter().enumerate() {
        match energy {
            SectionEnergy::High if !seen_high => {
                seen_high = true;
                cues.push((segment.start, CueSection::Main));
            }
            SectionEnergy::High if in_breakdown => {
                in_breakdown = false;
                cues.push((segment.start, CueSection::Drop));
            }
            SectionEnergy::Low if seen_high && !in_breakdown => {
                in_breakdown = true;
                let is_last = index + 1 == sections.len();
                let section = if is_last && segment.bars >= PHRASE_BARS {
                    CueSection::Outro
                } else {
                    CueSection::Breakdown
                };
                cues.push((segment.start, section));
            }
            _ => {}
        }
    }
    let mut snapped: Vec<(usize, CueSection)> = Vec::new();
    for (bar, section) in cues {
        let bar = snapped_to_phrase(bar, first).min(measures.len().saturating_sub(1));
        if snapped.iter().all(|(placed, _)| *placed != bar) {
            snapped.push((bar, section));
        }
    }
    snapped.sort_by_key(|(bar, _)| *bar);
    keep_most_useful(snapped)
}

/// At most [`MAXIMUM_CUES`] of `cues`: the start, the main section and the
/// outro, then the breakdowns and drops in order.
fn keep_most_useful(cues: Vec<(usize, CueSection)>) -> Vec<(usize, CueSection)> {
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
    let Some(first) = first_bar_with_sound(&measures) else {
        return Vec::new();
    };
    cue_bars(&measures, first)
        .into_iter()
        .map(|(bar, section)| SectionCue {
            seconds: grid.first_downbeat_seconds + bar as f64 * grid.bar_seconds(),
            section,
        })
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
        assert_eq!(found.len(), expected.len(), "{found:?}");
        for ((seconds, section), (expected_seconds, expected_section)) in found.iter().zip(expected)
        {
            assert_eq!(section, expected_section, "{found:?}");
            assert!(
                (seconds - expected_seconds).abs() < 0.05,
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
    fn a_loud_intro_before_a_quieter_steady_body_does_not_start_an_outro() {
        let audio = arrangement(0.0, &[(16, FULL), (48, KICK_AND_BASS)]);

        let cues = cues_of(&audio);

        assert_cues(&cues, &[(0.0, CueSection::Start)]);
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

        assert_cues(
            &cues,
            &[
                (0.0, CueSection::Start),
                (32.0, CueSection::Main),
                (96.0, CueSection::Breakdown),
                (128.0, CueSection::Drop),
                (192.0, CueSection::Outro),
            ],
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
    fn changes_off_the_phrase_land_on_the_nearest_phrase_or_half_phrase() {
        assert_eq!(snapped_to_phrase(17, 1), 17);
        assert_eq!(snapped_to_phrase(15, 1), 17);
        assert_eq!(snapped_to_phrase(19, 1), 17);
        assert_eq!(snapped_to_phrase(20, 1), 21);
        assert_eq!(snapped_to_phrase(22, 1), 21);
        assert_eq!(snapped_to_phrase(23, 1), 25);
        assert_eq!(snapped_to_phrase(0, 0), 0);
    }

    #[test]
    fn many_breakdowns_keep_the_start_main_and_outro_and_the_first_breaks() {
        let full = |bar| (bar, CueSection::Drop);
        let mut cues = vec![(0, CueSection::Start), (8, CueSection::Main)];
        for index in 0..5 {
            cues.push((16 + 16 * index, CueSection::Breakdown));
            cues.push(full(24 + 16 * index));
        }
        cues.push((200, CueSection::Outro));

        let kept = keep_most_useful(cues);

        assert_eq!(kept.len(), MAXIMUM_CUES);
        assert_eq!(kept[0], (0, CueSection::Start));
        assert_eq!(kept[1], (8, CueSection::Main));
        assert_eq!(
            kept[2..7].iter().map(|(bar, _)| *bar).collect::<Vec<_>>(),
            [16, 24, 32, 40, 48]
        );
        assert_eq!(kept[7], (200, CueSection::Outro));
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
    fn cue_names_read_back() {
        for section in [
            CueSection::Start,
            CueSection::Main,
            CueSection::Breakdown,
            CueSection::Drop,
            CueSection::Outro,
        ] {
            assert_eq!(CueSection::from_name(section.name()), Some(section));
        }
        assert_eq!(CueSection::from_name("Chorus"), None);
    }
}
