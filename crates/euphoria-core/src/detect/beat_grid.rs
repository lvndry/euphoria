// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fits a beat grid with one constant tempo to a track: where its beats and
//! its bars fall.

use std::f32::consts::TAU;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use crate::audio::AudioBuffer;
use crate::confidence::Confidence;
use crate::detect::decimate::decimate;
use crate::tempo_range::TempoRange;

/// The envelope is computed from the audio lowered to the smallest whole
/// fraction of its sample rate that stays at or above this rate. Onsets of
/// drums show well below 5.5 kHz, and the lower rate keeps long tracks cheap.
const ANALYSIS_RATE_FLOOR: u32 = 11_025;
/// Frames of the onset envelope per second, 5 ms apart.
const FRAMES_PER_SECOND: f32 = 200.0;
/// Samples in each spectrum, about 23 ms at the analysis rate.
const WINDOW_LENGTH: usize = 256;
/// Upper edge of the band that holds the kick drum and the bass.
const LOW_BAND_HZ: f32 = 150.0;
/// Lower edge of the band that holds snares, claps and hats.
const HIGH_BAND_HZ: f32 = 2_000.0;
/// Bands of the coarse spectrum each beat is described by.
const SPECTRUM_BANDS: usize = 16;
/// How far from the estimated tempo the grid's tempo is searched, as a
/// share of it. The tempo estimate is within half a percent for most
/// tracks; a grid further off is not the same tempo.
const TEMPO_SEARCH_SHARE: f64 = 0.02;
/// Length of the stretches in which drift is measured, and how far apart
/// they start.
const DRIFT_SEGMENT_SECONDS: f64 = 8.0;
const DRIFT_SEGMENT_HOP_SECONDS: f64 = 4.0;
/// A stretch whose beats stand out from the rest of the envelope by less
/// than this, as with a breakdown without drums, tells nothing about drift.
const SALIENT_SEGMENT: f32 = 1.3;
/// A beat has an onset when the onset strength near it reaches this share
/// of the strongest beat's.
const BEAT_HIT_SHARE: f32 = 0.1;
/// Fewer beats with an onset than this is no beat at all, such as one
/// transient in silence.
const MINIMUM_BEAT_HITS: usize = 4;
/// A grid can be rated medium or high only when at least this many of its
/// beats have an onset, and at least [`RECURRING_BEAT_SHARE`] of them, so
/// that a few hits in a long silence or ambience cannot make it confident.
const RECURRING_BEAT_HITS: usize = 16;
const RECURRING_BEAT_SHARE: f64 = 0.25;
/// The comb score is nearly flat within a few thousandths of a frame of
/// the best period, which over a long track still lets the last beats drift
/// by tens of milliseconds. So the period and phase are then fitted to where
/// the beats of each stretch of the track fall, a line through their
/// offsets from the grid, this many times.
const LINE_FIT_ROUNDS: usize = 2;
/// A line fit is kept when the comb score of the whole track stays at least
/// this share of the best found by the search, so that a stretch whose
/// strongest onsets are off the beat cannot pull a good grid away.
const LINE_FIT_SCORE_SHARE: f32 = 0.97;
/// Stretches whose beats sit further from the grid than this share of a
/// beat are left out of the line fit: they lock onto another onset of the
/// bar, not onto the beat.
const LINE_FIT_OFFSET_SHARE: f64 = 0.15;
/// Beats in a bar of 4/4, the metre of nearly all dance music.
pub(crate) const BEATS_PER_BAR: usize = 4;

/// The onset strength of a track over time, and its coarse spectrum.
pub(crate) struct OnsetEnvelope {
    /// Frames per second.
    pub(crate) frame_rate: f64,
    /// Seconds from the start of the track to the moment frame 0 stands for.
    pub(crate) first_frame_seconds: f64,
    /// Rise of the whole spectrum from one frame to the next.
    pub(crate) full: Vec<f32>,
    /// Rise below [`LOW_BAND_HZ`].
    pub(crate) low: Vec<f32>,
    /// Rise above [`HIGH_BAND_HZ`].
    pub(crate) high: Vec<f32>,
    /// Log magnitude of each frame in [`SPECTRUM_BANDS`] bands.
    pub(crate) spectrum: Vec<[f32; SPECTRUM_BANDS]>,
    /// The audio at the analysis rate, for the harmony at each beat.
    pub(crate) samples: Vec<f32>,
    /// Samples per second of `samples`.
    pub(crate) sample_rate: f32,
}

impl OnsetEnvelope {
    pub(crate) fn of_audio(audio: &AudioBuffer) -> Option<Self> {
        let sample_rate = audio.sample_rate().get();
        let factor = (sample_rate / ANALYSIS_RATE_FLOOR).max(1) as usize;
        let samples = decimate(audio.samples(), factor);
        let rate = sample_rate as f32 / factor as f32;
        let hop = ((rate / FRAMES_PER_SECOND).round() as usize).max(1);
        if samples.len() < WINDOW_LENGTH + hop {
            return None;
        }
        let transform = FftPlanner::<f32>::new().plan_fft_forward(WINDOW_LENGTH);
        let window: Vec<f32> = (0..WINDOW_LENGTH)
            .map(|index| 0.5 - 0.5 * (TAU * index as f32 / WINDOW_LENGTH as f32).cos())
            .collect();
        let bin_count = WINDOW_LENGTH / 2;
        let bin_width = rate / WINDOW_LENGTH as f32;
        let low_end = ((LOW_BAND_HZ / bin_width).ceil() as usize).clamp(2, bin_count);
        let high_start = ((HIGH_BAND_HZ / bin_width).floor() as usize).min(bin_count - 1);
        let band_edges: Vec<usize> = (0..=SPECTRUM_BANDS)
            .map(|band| {
                let share = band as f32 / SPECTRUM_BANDS as f32;
                ((bin_count as f32).powf(share).round() as usize).clamp(1, bin_count)
            })
            .collect();

        let frame_count = (samples.len() - WINDOW_LENGTH) / hop + 1;
        let mut frame = vec![Complex::default(); WINDOW_LENGTH];
        let mut scratch = vec![Complex::default(); transform.get_inplace_scratch_len()];
        let mut previous = vec![0.0_f32; bin_count];
        let mut current = vec![0.0_f32; bin_count];
        let mut envelope = Self {
            frame_rate: f64::from(rate) / hop as f64,
            first_frame_seconds: (WINDOW_LENGTH as f64 / 2.0) / f64::from(rate),
            full: Vec::with_capacity(frame_count),
            low: Vec::with_capacity(frame_count),
            high: Vec::with_capacity(frame_count),
            spectrum: Vec::with_capacity(frame_count),
            samples: Vec::new(),
            sample_rate: rate,
        };
        for frame_index in 0..frame_count {
            let start = frame_index * hop;
            for ((slot, sample), weight) in frame
                .iter_mut()
                .zip(&samples[start..start + WINDOW_LENGTH])
                .zip(&window)
            {
                *slot = Complex::new(sample * weight, 0.0);
            }
            transform.process_with_scratch(&mut frame, &mut scratch);
            for (value, bin) in current.iter_mut().zip(&frame) {
                *value = (1.0 + 100.0 * bin.norm()).ln();
            }
            let rise = |range: std::ops::Range<usize>| -> f32 {
                range
                    .map(|bin| (current[bin] - previous[bin]).max(0.0))
                    .sum()
            };
            let is_first = frame_index == 0;
            envelope
                .full
                .push(if is_first { 0.0 } else { rise(1..bin_count) });
            envelope
                .low
                .push(if is_first { 0.0 } else { rise(1..low_end) });
            envelope.high.push(if is_first {
                0.0
            } else {
                rise(high_start..bin_count)
            });
            let mut bands = [0.0_f32; SPECTRUM_BANDS];
            for (band, value) in bands.iter_mut().enumerate() {
                let (first, last) = (
                    band_edges[band],
                    band_edges[band + 1].max(band_edges[band] + 1),
                );
                let last = last.min(bin_count);
                if first < last {
                    *value = current[first..last].iter().sum::<f32>() / (last - first) as f32;
                }
            }
            envelope.spectrum.push(bands);
            std::mem::swap(&mut previous, &mut current);
        }
        envelope.samples = samples;
        Some(envelope)
    }
}

/// A smoothed copy of `values` with its slow changes removed, so that only
/// onsets stand above zero.
fn peaks_only(values: &[f32], frame_rate: f64) -> Vec<f32> {
    let kernel = [0.25_f32, 0.5, 1.0, 0.5, 0.25];
    let smoothed: Vec<f32> = (0..values.len())
        .map(|index| {
            kernel
                .iter()
                .enumerate()
                .filter_map(|(offset, weight)| {
                    let source = (index + offset).checked_sub(2)?;
                    values.get(source).map(|value| value * weight)
                })
                .sum()
        })
        .collect();
    let half_width = (frame_rate / 4.0).round() as usize;
    let mut sums = vec![0.0_f64; smoothed.len() + 1];
    for (index, value) in smoothed.iter().enumerate() {
        sums[index + 1] = sums[index] + f64::from(*value);
    }
    (0..smoothed.len())
        .map(|index| {
            let first = index.saturating_sub(half_width);
            let last = (index + half_width + 1).min(smoothed.len());
            let mean = (sums[last] - sums[first]) / (last - first) as f64;
            (smoothed[index] - mean as f32).max(0.0)
        })
        .collect()
}

/// The value of `values` at a fractional index, between its neighbours.
fn interpolated(values: &[f32], index: f64) -> f32 {
    if index < 0.0 {
        return 0.0;
    }
    let lower = index.floor() as usize;
    let Some(&below) = values.get(lower) else {
        return 0.0;
    };
    let above = values.get(lower + 1).copied().unwrap_or(below);
    let share = (index - lower as f64) as f32;
    below + (above - below) * share
}

/// Mean onset strength at the beats `phase + k * period` that fall within
/// `first..last` frames.
fn comb(values: &[f32], period: f64, phase: f64, first: f64, last: f64) -> f32 {
    let mut beat = phase + ((first - phase) / period).ceil().max(0.0) * period;
    let mut total = 0.0_f32;
    let mut count = 0_u32;
    while beat < last {
        total += interpolated(values, beat);
        count += 1;
        beat += period;
    }
    if count == 0 {
        0.0
    } else {
        total / count as f32
    }
}

/// Where the beats fall: one period and one phase, in frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BeatFit {
    pub(crate) period_frames: f64,
    pub(crate) phase_frames: f64,
    /// How much stronger the onsets are on the beats than the average of
    /// all phases of the same period.
    pub(crate) salience: f32,
}

/// Finds the period within [`TEMPO_SEARCH_SHARE`] of `estimate_bpm` and the
/// phase whose beats have the strongest onsets over the whole track.
pub(crate) fn fit_beats(envelope: &OnsetEnvelope, estimate_bpm: f64) -> Option<BeatFit> {
    let values = peaks_only(&envelope.full, envelope.frame_rate);
    let last = values.len() as f64;
    let estimate_period = 60.0 * envelope.frame_rate / estimate_bpm;
    if !(estimate_period.is_finite() && estimate_period >= 2.0) || last < 4.0 * estimate_period {
        return None;
    }
    let mut best = (0.0_f32, estimate_period, 0.0);
    let coarse_steps = 80;
    for step in 0..=coarse_steps {
        let share =
            -TEMPO_SEARCH_SHARE + 2.0 * TEMPO_SEARCH_SHARE * step as f64 / coarse_steps as f64;
        let period = estimate_period * (1.0 + share);
        let mut phase = 0.0;
        while phase < period {
            let score = comb(&values, period, phase, 0.0, last);
            if score > best.0 {
                best = (score, period, phase);
            }
            phase += 1.0;
        }
    }
    let coarse_period_step = estimate_period * 2.0 * TEMPO_SEARCH_SHARE / coarse_steps as f64;
    let (mut best_score, mut best_period, mut best_phase) = best;
    for period_step in -10..=10 {
        let period = best.1 + coarse_period_step * period_step as f64 / 10.0;
        for phase_step in -10..=10 {
            let phase = best.2 + phase_step as f64 / 10.0;
            let score = comb(&values, period, phase, 0.0, last);
            if score > best_score {
                (best_score, best_period, best_phase) = (score, period, phase);
            }
        }
    }
    // Keep the acceptance floor separate from the returned grid's score.
    let mut strongest_score = best_score;
    for _ in 0..LINE_FIT_ROUNDS {
        let Some((period, phase)) =
            fitted_to_stretches(&values, envelope.frame_rate, best_period, best_phase)
        else {
            break;
        };
        let score = comb(&values, period, phase, 0.0, last);
        if score < strongest_score * LINE_FIT_SCORE_SHARE {
            break;
        }
        strongest_score = strongest_score.max(score);
        (best_score, best_period, best_phase) = (score, period, phase);
    }
    let phase_count = best_period.ceil() as usize;
    let mean_over_phases = (0..phase_count)
        .map(|phase| comb(&values, best_period, phase as f64, 0.0, last))
        .sum::<f32>()
        / phase_count as f32;
    if mean_over_phases <= 0.0 || best_score <= 0.0 {
        return None;
    }
    let (hits, beats) = beat_hits(&values, best_period, best_phase);
    if hits < MINIMUM_BEAT_HITS {
        return None;
    }
    let recurs = hits >= RECURRING_BEAT_HITS && hits as f64 >= RECURRING_BEAT_SHARE * beats as f64;
    Some(BeatFit {
        period_frames: best_period,
        phase_frames: best_phase.rem_euclid(best_period),
        salience: if recurs {
            best_score / mean_over_phases
        } else {
            0.0
        },
    })
}

/// How many beats of the grid have an onset near them, and how many beats
/// the grid has. A beat counts when the strongest onset within two frames
/// of it reaches [`BEAT_HIT_SHARE`] of the strongest beat's.
fn beat_hits(values: &[f32], period: f64, phase: f64) -> (usize, usize) {
    let mut strengths = Vec::new();
    let mut beat = phase.rem_euclid(period);
    while beat < values.len() as f64 {
        let centre = beat.round() as usize;
        let first = centre.saturating_sub(2);
        let last = (centre + 3).min(values.len());
        strengths.push(
            values[first.min(last)..last]
                .iter()
                .copied()
                .fold(0.0, f32::max),
        );
        beat += period;
    }
    let strongest = strengths.iter().copied().fold(0.0, f32::max);
    if strongest <= 0.0 {
        return (0, strengths.len());
    }
    let hits = strengths
        .iter()
        .filter(|strength| **strength >= BEAT_HIT_SHARE * strongest)
        .count();
    (hits, strengths.len())
}

/// How far, in seconds, the beats of stretches of the track sit from the
/// grid: the 90th percentile over the stretches with clear beats, or `None`
/// when fewer than two have them.
pub(crate) fn drift_seconds(envelope: &OnsetEnvelope, fit: BeatFit) -> Option<f64> {
    let values = peaks_only(&envelope.full, envelope.frame_rate);
    let mut offsets: Vec<f64> = stretch_offsets(
        &values,
        envelope.frame_rate,
        fit.period_frames,
        fit.phase_frames,
    )
    .into_iter()
    .map(|(_, offset)| offset.abs() / envelope.frame_rate)
    .collect();
    if offsets.len() < 2 {
        return None;
    }
    offsets.sort_by(f64::total_cmp);
    Some(offsets[((offsets.len() - 1) as f64 * 0.9).round() as usize])
}

/// Where the beats of each stretch of the track with clear beats fall
/// against the grid of `period` and `phase`: the middle frame of the
/// stretch and the offset, in frames, that lines the grid up with them best.
fn stretch_offsets(values: &[f32], frame_rate: f64, period: f64, phase: f64) -> Vec<(f64, f64)> {
    let segment = DRIFT_SEGMENT_SECONDS * frame_rate;
    let hop = DRIFT_SEGMENT_HOP_SECONDS * frame_rate;
    let mut offsets = Vec::new();
    let mut start = 0.0;
    while start + segment <= values.len() as f64 {
        let last = start + segment;
        let mut best = (0.0_f32, 0.0_f64);
        let mut total = 0.0_f32;
        let mut count = 0_u32;
        let mut offset = -period / 2.0;
        while offset < period / 2.0 {
            let score = comb(values, period, phase + offset, start, last);
            total += score;
            count += 1;
            if score > best.0 {
                best = (score, offset);
            }
            offset += 0.25;
        }
        let mean = total / count.max(1) as f32;
        if mean > 0.0 && best.0 / mean >= SALIENT_SEGMENT {
            offsets.push((start + segment / 2.0, best.1));
        }
        start += hop;
    }
    offsets
}

/// The period and phase of a line through the offsets of the stretches
/// from the grid of `period` and `phase`, or `None` when fewer than three
/// stretches line up with the grid.
fn fitted_to_stretches(
    values: &[f32],
    frame_rate: f64,
    period: f64,
    phase: f64,
) -> Option<(f64, f64)> {
    let points: Vec<(f64, f64)> = stretch_offsets(values, frame_rate, period, phase)
        .into_iter()
        .filter(|(_, offset)| offset.abs() <= LINE_FIT_OFFSET_SHARE * period)
        .collect();
    if points.len() < 3 {
        return None;
    }
    let count = points.len() as f64;
    let mean_frame = points.iter().map(|(frame, _)| frame).sum::<f64>() / count;
    let mean_offset = points.iter().map(|(_, offset)| offset).sum::<f64>() / count;
    let spread = points
        .iter()
        .map(|(frame, _)| (frame - mean_frame).powi(2))
        .sum::<f64>();
    if spread <= 0.0 {
        return None;
    }
    let slope = points
        .iter()
        .map(|(frame, offset)| (frame - mean_frame) * (offset - mean_offset))
        .sum::<f64>()
        / spread;
    let offset_at_start = mean_offset - slope * mean_frame;
    // Beat k of the grid, at frame phase + k * period, is off by the change
    // of phase plus k times the change of period, so the offsets rise with
    // the frame by the change of period over the period.
    let corrected_period = period * (1.0 + slope);
    let corrected_phase = phase + offset_at_start + phase * slope;
    (corrected_period.is_finite() && corrected_period > 0.0).then_some((
        corrected_period,
        corrected_phase.rem_euclid(corrected_period),
    ))
}

/// What each of the four beats of a bar would make of the beats if it were
/// the downbeat: how strongly the kick lands on it, how much stronger snares
/// and claps are on the beats after it and two after, and how much the
/// sound changes at it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct DownbeatEvidence {
    pub(crate) accent: f32,
    pub(crate) backbeat: f32,
    pub(crate) change: f32,
    pub(crate) harmony: f32,
}

/// Samples of the long window that reads the bass and chords at a beat.
const HARMONY_WINDOW_LENGTH: usize = 2_048;
const HARMONY_LOWEST_HZ: f32 = 40.0;
const HARMONY_HIGHEST_HZ: f32 = 500.0;

/// How much the bass and chords at each beat differ from those at the beat
/// before, from 0 for the same notes to 1 for none in common.
fn harmonic_change(envelope: &OnsetEnvelope, beats: &[f64]) -> Vec<f32> {
    let transform = FftPlanner::<f32>::new().plan_fft_forward(HARMONY_WINDOW_LENGTH);
    let window: Vec<f32> = (0..HARMONY_WINDOW_LENGTH)
        .map(|index| 0.5 - 0.5 * (TAU * index as f32 / HARMONY_WINDOW_LENGTH as f32).cos())
        .collect();
    let bin_width = envelope.sample_rate / HARMONY_WINDOW_LENGTH as f32;
    let first_bin = (HARMONY_LOWEST_HZ / bin_width).floor() as usize;
    let last_bin =
        ((HARMONY_HIGHEST_HZ / bin_width).ceil() as usize).min(HARMONY_WINDOW_LENGTH / 2);
    let mut frame = vec![Complex::default(); HARMONY_WINDOW_LENGTH];
    let profiles: Vec<Vec<f32>> = beats
        .iter()
        .map(|beat| {
            let seconds = envelope.first_frame_seconds + beat / envelope.frame_rate;
            let start = (seconds * f64::from(envelope.sample_rate)).max(0.0) as usize;
            for (index, slot) in frame.iter_mut().enumerate() {
                let sample = envelope.samples.get(start + index).copied().unwrap_or(0.0);
                *slot = Complex::new(sample * window[index], 0.0);
            }
            transform.process(&mut frame);
            let profile: Vec<f32> = frame[first_bin..last_bin]
                .iter()
                .map(|bin| (1.0 + 100.0 * bin.norm()).ln())
                .collect();
            let norm = profile
                .iter()
                .map(|value| value * value)
                .sum::<f32>()
                .sqrt();
            if norm > 0.0 {
                profile.iter().map(|value| value / norm).collect()
            } else {
                profile
            }
        })
        .collect();
    (0..profiles.len())
        .map(|index| {
            let Some(previous) = index.checked_sub(1) else {
                return 0.0;
            };
            let similarity: f32 = profiles[index]
                .iter()
                .zip(&profiles[previous])
                .map(|(after, before)| after * before)
                .sum();
            1.0 - similarity
        })
        .collect()
}

/// The evidence for each beat of the bar being the downbeat, for the beats
/// counted from the first beat of the fit.
pub(crate) fn downbeat_evidence(
    envelope: &OnsetEnvelope,
    fit: BeatFit,
) -> [DownbeatEvidence; BEATS_PER_BAR] {
    let near_beat = |values: &[f32], beat: f64| -> f32 {
        (-2..=2)
            .map(|offset| interpolated(values, beat + f64::from(offset)))
            .fold(0.0_f32, f32::max)
    };
    let mut beats = Vec::new();
    let mut beat = fit.phase_frames;
    while beat + fit.period_frames < envelope.full.len() as f64 {
        beats.push(beat);
        beat += fit.period_frames;
    }
    let mean_spectrum = |from: f64, to: f64| -> [f32; SPECTRUM_BANDS] {
        let first = from.max(0.0) as usize;
        let last = (to.max(0.0) as usize).clamp(first + 1, envelope.spectrum.len());
        let mut mean = [0.0_f32; SPECTRUM_BANDS];
        for bands in &envelope.spectrum[first.min(last - 1)..last] {
            for (total, value) in mean.iter_mut().zip(bands) {
                *total += value;
            }
        }
        let count = (last - first.min(last - 1)) as f32;
        mean.map(|total| total / count)
    };
    let spectra: Vec<[f32; SPECTRUM_BANDS]> = beats
        .iter()
        .map(|beat| mean_spectrum(*beat, beat + fit.period_frames))
        .collect();
    let change: Vec<f32> = spectra
        .iter()
        .enumerate()
        .map(|(index, bands)| {
            let Some(before) = index.checked_sub(1).map(|previous| spectra[previous]) else {
                return 0.0;
            };
            bands
                .iter()
                .zip(before)
                .map(|(after, before)| (after - before) * (after - before))
                .sum::<f32>()
                .sqrt()
        })
        .collect();
    let harmony = harmonic_change(envelope, &beats);
    let low: Vec<f32> = beats
        .iter()
        .map(|beat| near_beat(&envelope.low, *beat))
        .collect();
    let high: Vec<f32> = beats
        .iter()
        .map(|beat| near_beat(&envelope.high, *beat))
        .collect();
    let mean_at = |values: &[f32], position: usize| -> f32 {
        let chosen: Vec<f32> = values
            .iter()
            .skip(position)
            .step_by(BEATS_PER_BAR)
            .copied()
            .collect();
        if chosen.is_empty() {
            0.0
        } else {
            chosen.iter().sum::<f32>() / chosen.len() as f32
        }
    };
    let mut evidence = [DownbeatEvidence::default(); BEATS_PER_BAR];
    for (position, item) in evidence.iter_mut().enumerate() {
        let backbeats = mean_at(&high, (position + 1) % BEATS_PER_BAR)
            + mean_at(&high, (position + 3) % BEATS_PER_BAR);
        let on_beats = mean_at(&high, position) + mean_at(&high, (position + 2) % BEATS_PER_BAR);
        *item = DownbeatEvidence {
            accent: mean_at(&low, position),
            backbeat: backbeats - on_beats,
            change: mean_at(&change, position),
            harmony: mean_at(&harmony, position),
        };
    }
    evidence
}

/// Onsets are found from a window's centre, but the beat a listener taps
/// falls slightly later. Measured on the GTZAN disco and hip hop excerpts
/// used to set the grid, the beats came out this much early before it was
/// added.
const BEAT_TIMING_OFFSET_SECONDS: f64 = 0.0187;

/// How much each cue counts in choosing the downbeat, after the cues are
/// put on one scale across the four beats of the bar. Chosen on half of the
/// GTZAN disco and hip hop excerpts: kick accent, snares and claps on the
/// beats after and two after, the change of the overall sound, and the
/// change of bass and chords.
const DOWNBEAT_WEIGHTS: DownbeatEvidence = DownbeatEvidence {
    accent: 0.5,
    backbeat: 1.0,
    change: 1.0,
    harmony: 1.0,
};

/// A grid whose beats stand out from the other phases of the same period
/// by less than this is rated low, and from this one up medium.
const MEDIUM_SALIENCE: f32 = 6.5;
/// A grid whose beats stand out by at least this is rated high.
const HIGH_SALIENCE: f32 = 9.5;
/// A grid whose beats drift further than this from the onsets of some
/// stretch of the track, half the window in which a beat counts as found,
/// is rated low whatever else it shows.
const DRIFT_LIMIT_SECONDS: f32 = 0.035;

/// Where the beats and bars of a track fall, with one constant tempo.
///
/// Dance music is produced at a constant tempo, so one tempo and one first
/// downbeat place every beat of a track. A track whose tempo drifts, such
/// as a live recording, still gets one grid, the one that fits it best, and
/// how far its beats wander from that grid is measured and lowers its
/// confidence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BeatGrid {
    /// Seconds from the start of the track to the first beat of the first
    /// bar, from 0 up to the length of one bar.
    pub first_downbeat_seconds: f64,
    /// Tempo of the grid in beats per minute, in the octave of the tempo it
    /// was fitted to.
    pub bpm: f64,
    /// How many times stronger the onsets on the beats are than the average
    /// over every phase of the same tempo. The basis of the confidence. It
    /// is 0, which rates the grid low, when fewer than 16 beats or fewer
    /// than a quarter of them have an onset, since a few hits in silence
    /// stand out from every other phase without being a beat.
    pub salience: f32,
    /// How far the beats of the stretches of the track wander from the
    /// grid, in seconds: the 90th percentile over stretches of 8 seconds
    /// with clear beats, or `None` when the track has fewer than two.
    pub drift_seconds: Option<f32>,
}

impl BeatGrid {
    /// A grid a DJ made, read from another program, rather than fitted to
    /// the audio. Nothing was measured, so it has the salience of the
    /// highest rating and no drift: the DJ checked it by ear.
    /// `first_downbeat_seconds` is moved into the first bar.
    pub fn imported(first_downbeat_seconds: f64, bpm: f64) -> Self {
        let grid = Self {
            first_downbeat_seconds: 0.0,
            bpm,
            salience: HIGH_SALIENCE,
            drift_seconds: None,
        };
        Self {
            first_downbeat_seconds: first_downbeat_seconds.rem_euclid(grid.bar_seconds()),
            ..grid
        }
    }

    /// Seconds from one beat to the next.
    pub fn beat_seconds(&self) -> f64 {
        60.0 / self.bpm
    }

    /// Seconds from one downbeat to the next, in 4/4.
    pub fn bar_seconds(&self) -> f64 {
        self.beat_seconds() * BEATS_PER_BAR as f64
    }

    /// How far to trust the beats of the grid. It does not rate the choice
    /// of downbeat, which is right less often.
    pub fn confidence(&self) -> Confidence {
        let drifts = self
            .drift_seconds
            .is_some_and(|drift| drift > DRIFT_LIMIT_SECONDS);
        if drifts || self.salience < MEDIUM_SALIENCE {
            Confidence::Low
        } else if self.salience < HIGH_SALIENCE {
            Confidence::Medium
        } else {
            Confidence::High
        }
    }

    /// The same grid with its tempo in `range`. Halving the tempo keeps
    /// every other beat and doubling it adds one between each two, and the
    /// first downbeat stays a downbeat either way. Doubling shortens the
    /// bar, so the first downbeat moves back by whole new bars to stay
    /// within the first bar.
    pub fn in_tempo_range(&self, range: TempoRange) -> Self {
        let bpm = f64::from(range.fold(self.bpm as f32));
        self.near_tempo(bpm)
    }

    /// Counts the grid in the octave closest to a reported tempo, retaining
    /// its refinement. Range boundaries must not separate the two counts.
    pub(crate) fn near_tempo(&self, bpm: f64) -> Self {
        let octaves = (bpm / self.bpm).log2().round() as i32;
        let moved = Self {
            bpm: self.bpm * 2.0_f64.powi(octaves),
            ..*self
        };
        Self {
            first_downbeat_seconds: self.first_downbeat_seconds.rem_euclid(moved.bar_seconds()),
            ..moved
        }
    }
}

/// Fits a beat grid to `audio` near `tempo_bpm`, which is searched within
/// 2% of it. `None` for audio shorter than four beats, and for audio whose
/// onsets show no beat, such as silence or a single transient: fewer than
/// four beats of the grid have an onset.
pub fn detect_beat_grid(audio: &AudioBuffer, tempo_bpm: f64) -> Option<BeatGrid> {
    grid_of_envelope(&OnsetEnvelope::of_audio(audio)?, tempo_bpm)
}

/// [`detect_beat_grid`] on an envelope that is already computed.
pub(crate) fn grid_of_envelope(envelope: &OnsetEnvelope, tempo_bpm: f64) -> Option<BeatGrid> {
    let fit = fit_beats(envelope, tempo_bpm)?;
    let evidence = downbeat_evidence(envelope, fit);
    let downbeat = choose_downbeat(&evidence);
    let beat_seconds = fit.period_frames / envelope.frame_rate;
    let first_beat_seconds = envelope.first_frame_seconds
        + fit.phase_frames / envelope.frame_rate
        + BEAT_TIMING_OFFSET_SECONDS;
    let bar_seconds = beat_seconds * BEATS_PER_BAR as f64;
    let downbeat_seconds = first_beat_seconds + downbeat as f64 * beat_seconds;
    Some(BeatGrid {
        first_downbeat_seconds: downbeat_seconds.rem_euclid(bar_seconds),
        bpm: 60.0 / beat_seconds,
        salience: fit.salience,
        drift_seconds: drift_seconds(envelope, fit).map(|drift| drift as f32),
    })
}

/// The beat of the bar, counted from the first beat of the fit, with the
/// most evidence of being the downbeat.
fn choose_downbeat(evidence: &[DownbeatEvidence; BEATS_PER_BAR]) -> usize {
    let cues: [fn(&DownbeatEvidence) -> f32; 4] = [
        |item| item.accent,
        |item| item.backbeat,
        |item| item.change,
        |item| item.harmony,
    ];
    let mut scores = [0.0_f32; BEATS_PER_BAR];
    for cue in cues {
        let values = evidence.map(|item| cue(&item));
        let mean = values.iter().sum::<f32>() / BEATS_PER_BAR as f32;
        let spread = (values
            .iter()
            .map(|value| (value - mean).powi(2))
            .sum::<f32>()
            / BEATS_PER_BAR as f32)
            .sqrt();
        let weight = cue(&DOWNBEAT_WEIGHTS);
        for (score, value) in scores.iter_mut().zip(values) {
            if spread > 0.0 {
                *score += weight * (value - mean) / spread;
            }
        }
    }
    let mut best = 0;
    for (position, score) in scores.iter().enumerate() {
        if *score > scores[best] {
            best = position;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    const SAMPLE_RATE: u32 = 44_100;

    /// A decaying tone of `frequency` starting at `start` seconds.
    fn add_hit(samples: &mut [f32], start: f64, frequency: f64, decay: f64, amplitude: f32) {
        let first = (start * f64::from(SAMPLE_RATE)) as usize;
        let length = (6.0 * decay * f64::from(SAMPLE_RATE)) as usize;
        for (offset, sample) in samples.iter_mut().skip(first).take(length).enumerate() {
            let time = offset as f64 / f64::from(SAMPLE_RATE);
            let tone = (std::f64::consts::TAU * frequency * time).sin() * (-time / decay).exp();
            *sample += amplitude * tone as f32;
        }
    }

    /// Noise of `amplitude` for a short time from `start`, the same on
    /// every run.
    fn add_noise_burst(samples: &mut [f32], start: f64, amplitude: f32, state: &mut u64) {
        let first = (start * f64::from(SAMPLE_RATE)) as usize;
        let length = SAMPLE_RATE as usize / 12;
        for (offset, sample) in samples.iter_mut().skip(first).take(length).enumerate() {
            *state ^= *state << 13;
            *state ^= *state >> 7;
            *state ^= *state << 17;
            let unit = (*state >> 40) as f32 / (1_u64 << 24) as f32;
            let fade = (-(offset as f32) / (length as f32 / 4.0)).exp();
            *sample += amplitude * fade * (2.0 * unit - 1.0);
        }
    }

    /// Four on the floor at `tempo`: a kick on every beat, a clap on the
    /// second and fourth beats of each bar, and a bass note held through
    /// each bar that changes at every downbeat. The first downbeat is at
    /// `first_downbeat` seconds.
    fn dance_loop(tempo: f64, seconds: f64, first_downbeat: f64) -> AudioBuffer {
        let mut samples = vec![0.0_f32; (seconds * f64::from(SAMPLE_RATE)) as usize];
        let beat = 60.0 / tempo;
        let bass_notes = [55.0, 73.4, 65.4, 49.0];
        let mut noise_state = 0x2545_F491_4F6C_DD1D_u64;
        let mut index = 0;
        loop {
            let time = first_downbeat + index as f64 * beat;
            if time + beat >= seconds {
                break;
            }
            add_hit(&mut samples, time, 60.0, 0.08, 0.6);
            if index % 4 == 1 || index % 4 == 3 {
                add_noise_burst(&mut samples, time, 0.4, &mut noise_state);
            }
            if index % 4 == 0 {
                let note = bass_notes[(index / 4) % bass_notes.len()];
                add_hit(&mut samples, time, note * 2.0, 4.0 * beat / 3.0, 0.3);
            }
            index += 1;
        }
        AudioBuffer::new(samples, NonZeroU32::new(SAMPLE_RATE).unwrap())
    }

    fn assert_close(actual: f64, expected: f64, tolerance: f64, what: &str) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{what}: {actual} is not within {tolerance} of {expected}"
        );
    }

    #[test]
    fn a_dance_loop_gets_its_tempo_beats_and_downbeat() {
        for (tempo, estimate, first_downbeat) in [(128.0, 127.0, 0.4), (124.5, 125.0, 1.1)] {
            let audio = dance_loop(tempo, 60.0, first_downbeat);

            let grid = detect_beat_grid(&audio, estimate).expect("a loop has a grid");

            assert_close(grid.bpm, tempo, 0.02, "tempo");
            let bar = grid.bar_seconds();
            let distance = (grid.first_downbeat_seconds - first_downbeat).rem_euclid(bar);
            let distance = distance.min(bar - distance);
            assert!(
                distance < 0.04,
                "{tempo} BPM: the first downbeat is at {} instead of {first_downbeat}",
                grid.first_downbeat_seconds
            );
            assert!((0.0..bar).contains(&grid.first_downbeat_seconds));
            assert_eq!(grid.confidence(), Confidence::High, "{grid:?}");
        }
    }

    #[test]
    fn a_grid_on_a_drifting_tempo_is_rated_low() {
        let mut samples = vec![0.0_f32; 60 * SAMPLE_RATE as usize];
        let mut time = 0.3;
        let mut index = 0_u32;
        while time < 59.0 {
            add_hit(&mut samples, time, 60.0, 0.08, 0.6);
            // The tempo rises from 120 to 126 BPM over the minute.
            let tempo = 120.0 + 6.0 * time / 60.0;
            time += 60.0 / tempo;
            index += 1;
        }
        let audio = AudioBuffer::new(samples, NonZeroU32::new(SAMPLE_RATE).unwrap());

        let grid = detect_beat_grid(&audio, 123.0).expect("clicks have a grid");

        assert!(index > 100);
        assert!(
            grid.drift_seconds
                .expect("a minute of clicks has stretches")
                > 0.035
        );
        assert_eq!(grid.confidence(), Confidence::Low, "{grid:?}");
    }

    #[test]
    fn silence_and_audio_shorter_than_four_beats_have_no_grid() {
        let silence = AudioBuffer::new(
            vec![0.0; 10 * SAMPLE_RATE as usize],
            NonZeroU32::new(SAMPLE_RATE).unwrap(),
        );
        assert_eq!(detect_beat_grid(&silence, 128.0), None);
        let short = dance_loop(128.0, 1.5, 0.1);
        assert_eq!(detect_beat_grid(&short, 128.0), None);
    }

    #[test]
    fn reported_salience_measures_the_returned_grid() {
        for tempo in [120.0, 127.537, 132.1, 95.0] {
            let audio = dance_loop(tempo, 40.0, 0.2);
            let envelope = OnsetEnvelope::of_audio(&audio).unwrap();
            let fit = fit_beats(&envelope, tempo).unwrap();
            let values = peaks_only(&envelope.full, envelope.frame_rate);
            let phases = fit.period_frames.ceil() as usize;
            let last = values.len() as f64;
            let mean = (0..phases)
                .map(|phase| comb(&values, fit.period_frames, phase as f64, 0.0, last))
                .sum::<f32>()
                / phases as f32;
            let actual = comb(&values, fit.period_frames, fit.phase_frames, 0.0, last) / mean;
            assert!(
                (fit.salience - actual).abs() < 0.0001,
                "tempo {tempo}: reported {} but returned grid measures {actual}",
                fit.salience
            );
        }
    }

    #[test]
    fn a_long_track_gets_a_tempo_precise_enough_to_keep_its_last_beats_on_the_grid() {
        let tempo = 127.537;
        let seconds = 240.0;
        let mut samples = vec![0.0_f32; (seconds * f64::from(SAMPLE_RATE)) as usize];
        let mut beat = 0.2;
        while beat < seconds - 1.0 {
            add_hit(&mut samples, beat, 60.0, 0.05, 0.8);
            beat += 60.0 / tempo;
        }
        let audio = AudioBuffer::new(samples, NonZeroU32::new(SAMPLE_RATE).unwrap());

        let grid = detect_beat_grid(&audio, 128.0).expect("clicks have a grid");

        let last_beat_error = (grid.bpm - tempo).abs() / tempo * seconds;
        assert!(
            last_beat_error < 0.005,
            "{} BPM puts the last beat {last_beat_error} s off",
            grid.bpm
        );
    }

    #[test]
    fn a_transient_in_silence_has_no_grid_and_a_few_bars_in_silence_a_low_one() {
        let mut samples = vec![0.0_f32; 60 * SAMPLE_RATE as usize];
        add_hit(&mut samples, 1.0, 55.0, 0.05, 0.8);
        let transient = AudioBuffer::new(samples, NonZeroU32::new(SAMPLE_RATE).unwrap());
        assert_eq!(detect_beat_grid(&transient, 120.0), None);

        let mut samples = vec![0.0_f32; 60 * SAMPLE_RATE as usize];
        for beat in 0..8 {
            add_hit(&mut samples, 1.0 + f64::from(beat) * 0.5, 55.0, 0.05, 0.8);
        }
        let few_beats = AudioBuffer::new(samples, NonZeroU32::new(SAMPLE_RATE).unwrap());
        let grid = detect_beat_grid(&few_beats, 120.0).expect("a grid");
        assert_eq!(grid.salience, 0.0);
        assert_eq!(grid.confidence(), Confidence::Low);
    }

    #[test]
    fn a_grid_moves_into_another_tempo_range_by_whole_octaves() {
        let grid = BeatGrid {
            first_downbeat_seconds: 0.25,
            bpm: 170.02,
            salience: 12.0,
            drift_seconds: Some(0.01),
        };
        let half_time: TempoRange = "70-140".parse().unwrap();
        let moved = grid.in_tempo_range(half_time);
        assert_close(moved.bpm, 85.01, 1e-9, "tempo");
        assert_eq!(moved.first_downbeat_seconds, grid.first_downbeat_seconds);
        assert_eq!(grid.in_tempo_range(TempoRange::DEFAULT), grid);

        let slow = BeatGrid {
            first_downbeat_seconds: 2.5,
            bpm: 85.0,
            ..grid
        };
        let doubled = slow.in_tempo_range(TempoRange::DEFAULT);
        assert_close(doubled.bpm, 170.0, 1e-9, "tempo");
        assert_close(
            doubled.first_downbeat_seconds,
            2.5 - doubled.bar_seconds(),
            1e-9,
            "downbeat",
        );
    }

    #[test]
    fn confidence_follows_salience_unless_the_beats_drift() {
        let grid = |salience: f32, drift_seconds: Option<f32>| BeatGrid {
            first_downbeat_seconds: 0.0,
            bpm: 128.0,
            salience,
            drift_seconds,
        };
        assert_eq!(grid(12.0, Some(0.01)).confidence(), Confidence::High);
        assert_eq!(grid(8.0, None).confidence(), Confidence::Medium);
        assert_eq!(grid(5.0, Some(0.01)).confidence(), Confidence::Low);
        assert_eq!(grid(12.0, Some(0.05)).confidence(), Confidence::Low);
    }

    #[test]
    fn the_downbeat_is_the_beat_with_the_most_weighted_evidence() {
        let mut evidence = [DownbeatEvidence::default(); BEATS_PER_BAR];
        evidence[2] = DownbeatEvidence {
            accent: 1.0,
            backbeat: 1.0,
            change: 1.0,
            harmony: 1.0,
        };
        assert_eq!(choose_downbeat(&evidence), 2);
        assert_eq!(
            choose_downbeat(&[DownbeatEvidence::default(); BEATS_PER_BAR]),
            0
        );
    }
}
