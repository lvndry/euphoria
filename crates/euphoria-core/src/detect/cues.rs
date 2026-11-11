// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Finds the places in a track where a DJ sets cues: where the music
//! starts, where it first plays at full energy, its breakdowns and drops,
//! and where the outro begins. Every cue falls on the start of a bar of the
//! beat grid, at a phrase boundary.

use crate::audio::AudioBuffer;
use crate::detect::beat_grid::{BeatGrid, OnsetEnvelope, grid_of_envelope};

/// Bars are measured from this long before each downbeat.
const BAR_LEAD_SECONDS: f64 = 0.05;
/// The band of the kick and the bass ends about here.
const KICK_BAND_HZ: f32 = 150.0;

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
/// A bar plays at full energy when its kick and bass are no more than
/// [`FULL_KICK_DROP_DB`] below their usual level and the whole bar no more
/// than [`FULL_LOUDNESS_DROP_DB`] below its usual loudness.
const FULL_KICK_DROP_DB: f32 = 6.0;
const FULL_LOUDNESS_DROP_DB: f32 = 4.0;
/// A bar has no kick when its kick and bass are this far below their usual
/// level: the breakdown of a dance track.
const NO_KICK_DROP_DB: f32 = 12.0;
/// The usual level of a track is the level this share of its bars with
/// sound reach.
const USUAL_LEVEL_PERCENTILE: f32 = 0.9;

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

/// How a bar sounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarLevel {
    /// The kick and the bass at their usual level, and the track near its
    /// usual loudness.
    Full,
    /// Kick or bass, but quieter or thinner than at full energy.
    Partial,
    /// No kick and bass to speak of, or silence.
    NoKick,
}

/// The loudness and the loudness below about 150 Hz, where the kick and the
/// bass are, of every bar of the grid that fits in the track, in decibels.
///
/// Bars are measured from slightly before each downbeat, since the onset of
/// a beat comes before the moment the grid places it, and a bar measured
/// from the downbeat would end with the onset of the next.
fn bar_measures(envelope: &OnsetEnvelope, grid: &BeatGrid) -> Vec<(f32, f32)> {
    let sample_rate = f64::from(envelope.sample_rate);
    let low_passed = low_passed(&envelope.samples, envelope.sample_rate);
    let decibels = |samples: &[f32]| {
        let power =
            samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len().max(1) as f32;
        10.0 * (power + 1e-12).log10()
    };
    let duration = envelope.samples.len() as f64 / sample_rate;
    let bar_seconds = grid.bar_seconds();
    let mut measures = Vec::new();
    let mut downbeat = grid.first_downbeat_seconds;
    while downbeat + bar_seconds <= duration {
        let first = ((downbeat - BAR_LEAD_SECONDS).max(0.0) * sample_rate) as usize;
        let last = (((downbeat + bar_seconds - BAR_LEAD_SECONDS) * sample_rate) as usize)
            .min(envelope.samples.len());
        let range = first.min(last)..last;
        measures.push((
            decibels(&envelope.samples[range.clone()]),
            decibels(&low_passed[range]),
        ));
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

/// The value that `share` of `values` reach or stay below.
fn percentile(values: &[f32], share: f32) -> f32 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    let index = ((sorted.len() - 1) as f32 * share).round() as usize;
    sorted[index]
}

/// The level of every bar, and the index of the first bar with sound.
fn bar_levels(measures: &[(f32, f32)]) -> Option<(Vec<BarLevel>, usize)> {
    let loudest = measures
        .iter()
        .map(|(loudness, _)| *loudness)
        .fold(f32::NEG_INFINITY, f32::max);
    let silent = |loudness: f32| loudness < loudest - SILENT_BELOW_LOUDEST_DB;
    let first_with_sound = measures.iter().position(|(loudness, _)| !silent(*loudness))?;
    let sounding: Vec<(f32, f32)> = measures
        .iter()
        .copied()
        .filter(|(loudness, _)| !silent(*loudness))
        .collect();
    let usual_loudness = percentile(
        &sounding.iter().map(|(loudness, _)| *loudness).collect::<Vec<_>>(),
        USUAL_LEVEL_PERCENTILE,
    );
    let usual_kick = percentile(
        &sounding.iter().map(|(_, kick)| *kick).collect::<Vec<_>>(),
        USUAL_LEVEL_PERCENTILE,
    );
    let levels = measures
        .iter()
        .map(|(loudness, kick)| {
            if silent(*loudness) || *kick < usual_kick - NO_KICK_DROP_DB {
                BarLevel::NoKick
            } else if *kick >= usual_kick - FULL_KICK_DROP_DB
                && *loudness >= usual_loudness - FULL_LOUDNESS_DROP_DB
            {
                BarLevel::Full
            } else {
                BarLevel::Partial
            }
        })
        .collect();
    Some((levels, first_with_sound))
}

/// Runs of bars at one level, as (first bar, level), from `first` on. A run
/// shorter than [`MINIMUM_SECTION_BARS`] joins the run before it.
fn sections(levels: &[BarLevel], first: usize) -> Vec<(usize, BarLevel)> {
    let mut runs: Vec<(usize, usize, BarLevel)> = Vec::new();
    for (index, level) in levels.iter().enumerate().skip(first) {
        match runs.last_mut() {
            Some((_, length, last_level)) if last_level == level => *length += 1,
            _ => runs.push((index, 1, *level)),
        }
    }
    let mut merged: Vec<(usize, usize, BarLevel)> = Vec::new();
    for (start, length, level) in runs {
        match merged.last_mut() {
            Some((_, last_length, last_level)) if *last_level == level => *last_length += length,
            Some((_, last_length, _)) if length < MINIMUM_SECTION_BARS => *last_length += length,
            _ => merged.push((start, length, level)),
        }
    }
    // A short first run is kept above, since nothing comes before it; it
    // joins the run after it instead.
    if merged.len() > 1 && merged[0].1 < MINIMUM_SECTION_BARS {
        let (start, length, _) = merged.remove(0);
        merged[0].0 = start;
        merged[0].1 += length;
    }
    merged
        .into_iter()
        .map(|(start, _, level)| (start, level))
        .collect()
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
fn cue_bars(levels: &[BarLevel], first: usize) -> Vec<(usize, CueSection)> {
    let runs = sections(levels, first);
    let mut cues = vec![(first, CueSection::Start)];
    let mut seen_full = false;
    let mut in_breakdown = false;
    for &(start, level) in &runs {
        match level {
            BarLevel::Full if !seen_full => {
                seen_full = true;
                cues.push((start, CueSection::Main));
            }
            BarLevel::Full if in_breakdown => {
                in_breakdown = false;
                cues.push((start, CueSection::Drop));
            }
            BarLevel::NoKick if seen_full && !in_breakdown => {
                in_breakdown = true;
                cues.push((start, CueSection::Breakdown));
            }
            _ => {}
        }
    }
    let last_full_end = runs
        .iter()
        .enumerate()
        .filter(|(_, (_, level))| *level == BarLevel::Full)
        .next_back()
        .map(|(index, _)| runs.get(index + 1).map_or(levels.len(), |(start, _)| *start));
    if let Some(end) = last_full_end
        && end + PHRASE_BARS <= levels.len()
    {
        // A breakdown cue at the same bar is the outro instead.
        cues.retain(|(bar, _)| *bar != end);
        cues.push((end, CueSection::Outro));
    }

    let mut snapped: Vec<(usize, CueSection)> = Vec::new();
    for (bar, section) in cues {
        let bar = snapped_to_phrase(bar, first).min(levels.len().saturating_sub(1));
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
    let Some((levels, first)) = bar_levels(&measures) else {
        return Vec::new();
    };
    cue_bars(&levels, first)
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
                let bass_frequency = if bar_index % 2 == 0 { 110.0 } else { 82.4 };
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
    fn a_short_break_before_a_drop_is_not_a_breakdown() {
        let audio = arrangement(0.0, &[(16, INTRO), (30, FULL), (2, BREAKDOWN), (16, FULL)]);

        let cues = cues_of(&audio);

        assert_cues(&cues, &[(0.0, CueSection::Start), (32.0, CueSection::Main)]);
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
        assert_eq!(kept[2..7].iter().map(|(bar, _)| *bar).collect::<Vec<_>>(), [16, 24, 32, 40, 48]);
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
        assert_eq!(detect_beat_grid_and_cues(&silence, TEMPO), (None, Vec::new()));
    }

    #[test]
    fn debug_measures() {
        let audio = arrangement(0.5, &[(16, INTRO), (32, FULL), (16, BREAKDOWN), (32, FULL), (16, INTRO)]);
        let envelope = OnsetEnvelope::of_audio(&audio).unwrap();
        let grid = grid_of_envelope(&envelope, TEMPO).unwrap();
        for (index, measure) in bar_measures(&envelope, &grid).iter().enumerate() {
            eprintln!("{index} {:.1} {:.3}", measure.0, measure.1);
        }
        panic!();
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
