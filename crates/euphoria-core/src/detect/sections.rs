// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Finds where the sections of any piece of music change, whatever its
//! genre: where its harmony, its timbre or its level changes and stays
//! changed. Nothing here assumes a beat, a kick, 4/4 or phrases of 8 bars;
//! [`cues`](super::cues) gives the sections their meaning for a DJ.

use std::f32::consts::TAU;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use crate::confidence::Confidence;
use crate::detect::beat_grid::{BeatGrid, OnsetEnvelope};

/// Samples of the window each frame is measured over: about 190 ms at the
/// analysis rate, long enough to tell apart the semitones above 100 Hz.
const FRAME_WINDOW_LENGTH: usize = 2_048;
/// Samples from one frame to the next: about 93 ms.
const FRAME_HOP: usize = 1_024;
/// The pitches the harmony is read from.
const CHROMA_LOWEST_HZ: f32 = 100.0;
const CHROMA_HIGHEST_HZ: f32 = 2_500.0;
/// The bands of the timbre, spaced evenly on the mel scale.
pub(crate) const MEL_BANDS: usize = 40;
const MEL_LOWEST_HZ: f32 = 40.0;
const MEL_HIGHEST_HZ: f32 = 5_000.0;
/// The band of the kick and the bass ends about here, and the band of the
/// hats and the cymbals starts about here.
const LOW_BAND_HZ: f32 = 150.0;
const HIGH_BAND_HZ: f32 = 2_000.0;

/// What one frame of a track sounds like.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Frame {
    /// The power of each of the twelve pitch classes, from C.
    pub(crate) chroma: [f32; 12],
    /// The power in each band of the mel scale.
    pub(crate) mel: [f32; MEL_BANDS],
    /// The power of the whole frame, below [`LOW_BAND_HZ`] and above
    /// [`HIGH_BAND_HZ`].
    pub(crate) power: f32,
    pub(crate) low_power: f32,
    pub(crate) high_power: f32,
    /// How much the band below [`LOW_BAND_HZ`] rises over the frame: high
    /// when a kick or a bass note strikes, whatever the style.
    pub(crate) low_onsets: f32,
}

/// The frames of a track, [`FRAME_HOP`] samples apart.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Frames {
    /// Seconds from the start of the track to the middle of frame 0.
    pub(crate) first_frame_seconds: f64,
    /// Seconds from one frame to the next.
    pub(crate) frame_seconds: f64,
    pub(crate) frames: Vec<Frame>,
}

fn hz_to_mel(hz: f32) -> f32 {
    2_595.0 * (1.0 + hz / 700.0).log10()
}

fn mel_to_hz(mel: f32) -> f32 {
    700.0 * (10.0_f32.powf(mel / 2_595.0) - 1.0)
}

/// The weight of each bin of the spectrum in each mel band: triangles that
/// overlap by half.
fn mel_weights(bin_count: usize, bin_width: f32) -> Vec<[f32; MEL_BANDS]> {
    let highest = MEL_HIGHEST_HZ.min(bin_width * bin_count as f32);
    let lowest_mel = hz_to_mel(MEL_LOWEST_HZ);
    let step = (hz_to_mel(highest) - lowest_mel) / (MEL_BANDS + 1) as f32;
    let edges: Vec<f32> = (0..MEL_BANDS + 2)
        .map(|index| mel_to_hz(lowest_mel + step * index as f32))
        .collect();
    (0..bin_count)
        .map(|bin| {
            let hz = bin as f32 * bin_width;
            let mut weights = [0.0_f32; MEL_BANDS];
            for (band, weight) in weights.iter_mut().enumerate() {
                let (left, center, right) = (edges[band], edges[band + 1], edges[band + 2]);
                *weight = if hz > left && hz <= center {
                    (hz - left) / (center - left)
                } else if hz > center && hz < right {
                    (right - hz) / (right - center)
                } else {
                    0.0
                };
            }
            weights
        })
        .collect()
}

/// The pitch class of each bin of the spectrum, for the bins between
/// [`CHROMA_LOWEST_HZ`] and [`CHROMA_HIGHEST_HZ`].
fn pitch_classes(bin_count: usize, bin_width: f32) -> Vec<Option<usize>> {
    (0..bin_count)
        .map(|bin| {
            let hz = bin as f32 * bin_width;
            (CHROMA_LOWEST_HZ..=CHROMA_HIGHEST_HZ)
                .contains(&hz)
                .then(|| {
                    let midi_note = 69.0 + 12.0 * (hz / 440.0).log2();
                    (midi_note.round() as i64).rem_euclid(12) as usize
                })
        })
        .collect()
}

impl Frames {
    /// The frames of the audio the envelope was computed from. Empty when
    /// the track is shorter than one window.
    pub(crate) fn of_envelope(envelope: &OnsetEnvelope) -> Self {
        let rate = envelope.sample_rate;
        let samples = &envelope.samples;
        let frame_seconds = FRAME_HOP as f64 / f64::from(rate);
        let mut frames = Self {
            first_frame_seconds: FRAME_WINDOW_LENGTH as f64 / 2.0 / f64::from(rate),
            frame_seconds,
            frames: Vec::new(),
        };
        if samples.len() < FRAME_WINDOW_LENGTH {
            return frames;
        }
        let transform = FftPlanner::<f32>::new().plan_fft_forward(FRAME_WINDOW_LENGTH);
        let window: Vec<f32> = (0..FRAME_WINDOW_LENGTH)
            .map(|index| 0.5 - 0.5 * (TAU * index as f32 / FRAME_WINDOW_LENGTH as f32).cos())
            .collect();
        let bin_count = FRAME_WINDOW_LENGTH / 2;
        let bin_width = rate / FRAME_WINDOW_LENGTH as f32;
        let mel_weights = mel_weights(bin_count, bin_width);
        let pitch_classes = pitch_classes(bin_count, bin_width);
        let low_end = ((LOW_BAND_HZ / bin_width).ceil() as usize).min(bin_count);
        let high_start = ((HIGH_BAND_HZ / bin_width).floor() as usize).min(bin_count);
        let mut buffer = vec![Complex::default(); FRAME_WINDOW_LENGTH];
        let mut scratch = vec![Complex::default(); transform.get_inplace_scratch_len()];
        let frame_count = (samples.len() - FRAME_WINDOW_LENGTH) / FRAME_HOP + 1;
        frames.frames.reserve(frame_count);
        for frame_index in 0..frame_count {
            let start = frame_index * FRAME_HOP;
            for ((slot, sample), weight) in buffer
                .iter_mut()
                .zip(&samples[start..start + FRAME_WINDOW_LENGTH])
                .zip(&window)
            {
                *slot = Complex::new(sample * weight, 0.0);
            }
            transform.process_with_scratch(&mut buffer, &mut scratch);
            let mut frame = Frame {
                chroma: [0.0; 12],
                mel: [0.0; MEL_BANDS],
                power: 0.0,
                low_power: 0.0,
                high_power: 0.0,
                low_onsets: 0.0,
            };
            for (bin, value) in buffer[..bin_count].iter().enumerate().skip(1) {
                let power = value.norm_sqr();
                frame.power += power;
                if bin < low_end {
                    frame.low_power += power;
                }
                if bin >= high_start {
                    frame.high_power += power;
                }
                if let Some(pitch_class) = pitch_classes[bin] {
                    frame.chroma[pitch_class] += power;
                }
                for (band, weight) in frame.mel.iter_mut().zip(&mel_weights[bin]) {
                    *band += power * weight;
                }
            }
            let middle_seconds = (start + FRAME_WINDOW_LENGTH / 2) as f64 / f64::from(rate);
            frame.low_onsets = onsets_around(envelope, middle_seconds, frame_seconds);
            frames.frames.push(frame);
        }
        frames
    }
}

/// The mean rise of the band below 150 Hz of `envelope` over the
/// `length_seconds` around `middle_seconds`.
fn onsets_around(envelope: &OnsetEnvelope, middle_seconds: f64, length_seconds: f64) -> f32 {
    let to_frame = |seconds: f64| {
        (((seconds - envelope.first_frame_seconds) * envelope.frame_rate).max(0.0) as usize)
            .min(envelope.low.len())
    };
    let first = to_frame(middle_seconds - length_seconds / 2.0);
    let last = to_frame(middle_seconds + length_seconds / 2.0);
    if last <= first {
        return 0.0;
    }
    envelope.low[first..last].iter().sum::<f32>() / (last - first) as f32
}

/// The grid is trusted to place the beats when it is rated at least
/// this. Below it, a track is measured in units of [`FIXED_UNIT_SECONDS`].
const SYNCHRONISED_CONFIDENCE: Confidence = Confidence::Medium;
/// The length of a unit of a track without a trusted grid.
const FIXED_UNIT_SECONDS: f64 = 0.5;
/// Mel bands further below the loudest band of the track than this count as
/// this far below it.
const MEL_FLOOR_DB: f32 = 80.0;
/// The timbre is the shape of the mel spectrum: its cepstral coefficients
/// from the first to this one, without the zeroth, which is the level.
const TIMBRE_COEFFICIENTS: usize = 13;
/// Each cepstral coefficient varies at least this much over a track, in
/// decibels, so that a steady track does not turn small differences into
/// section changes. Every SALAMI recording varies more than twice as much.
const TIMBRE_MINIMUM_SPREAD_DB: f32 = 1.0;
/// The level, the low end and the high end further below their loudest unit
/// than this count as this far below it.
const LEVEL_FLOOR_DB: f32 = 30.0;
/// The level features vary at least this much over a track, so that a
/// steady track does not turn small differences into section changes.
const LEVEL_MINIMUM_SPREAD_DB: f32 = 2.0;
const ONSET_MINIMUM_SPREAD: f32 = 0.05;
/// The kernel compares this many seconds of music before a moment with as
/// many after it.
const KERNEL_SECONDS: f64 = 32.0;
/// How much each kind of change counts: harmony, timbre and level.
const NOVELTY_WEIGHTS: [f32; 3] = [1.0, 2.0, 2.0];
/// Each novelty curve is scaled to its largest value away from the ends of
/// the track, where a fade or silence would set the scale, or to this when that
/// is smaller, so that a track that hardly changes keeps small novelty
/// instead of having its smallest wobble scaled up into a section change.
/// They sit near the smallest largest values of the SALAMI recordings, and
/// leave their scores unchanged.
const NOVELTY_FLOORS: [f32; 3] = [0.02, 0.25, 0.25];
/// A change counts when its novelty reaches this share of the largest
/// change of the track.
const BOUNDARY_SHARE: f32 = 0.05;
/// Section changes are at least this far apart.
const MINIMUM_SECTION_SECONDS: f64 = 10.0;
/// No section changes this close to the start or the end of the track.
const EDGE_SECONDS: f64 = 4.0;

/// Where a section of a track starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SectionBoundary {
    /// Seconds from the start of the track.
    pub(crate) seconds: f64,
    /// How large the change is, from 0 to 1 for the largest of the track.
    pub(crate) strength: f32,
}

/// The places where the sections of the track change, in order.
///
/// The track is cut into units, its beats when `grid` is trusted and short
/// fixed stretches otherwise, and each unit is described by its harmony
/// (a chroma vector), its timbre (cepstral coefficients of its mel
/// spectrum) and its level (loudness, low end, high end and how much the
/// low end strikes). For each of the three, a checkerboard kernel slid
/// along the matrix of distances between units finds where what comes
/// before differs from what comes after, after Foote; the three novelty
/// curves, each scaled to its largest value, are averaged, and its tallest
/// peaks are the section changes.
pub(crate) fn section_boundaries(
    envelope: &OnsetEnvelope,
    grid: Option<&BeatGrid>,
) -> Vec<SectionBoundary> {
    let frames = Frames::of_envelope(envelope);
    let duration = envelope.samples.len() as f64 / f64::from(envelope.sample_rate);
    let starts = unit_starts(grid, duration);
    if starts.len() < 2 || frames.frames.is_empty() {
        return Vec::new();
    }
    let units = aggregate(&frames, &starts);
    let unit_seconds = median_step(&starts);
    let half_width = ((KERNEL_SECONDS / unit_seconds / 2.0).round() as usize).max(2);
    let (harmony, timbre, level) = unit_features(&units);
    let curves = [
        foote_novelty(&harmony, half_width, chroma_distance),
        foote_novelty(&timbre, half_width, euclidean_distance),
        foote_novelty(&level, half_width, euclidean_distance),
    ];
    let weight_sum: f32 = NOVELTY_WEIGHTS.iter().sum();
    let mut novelty = vec![0.0_f32; starts.len()];
    let inside = |unit: &usize| (EDGE_SECONDS..=duration - EDGE_SECONDS).contains(&starts[*unit]);
    for ((curve, weight), floor) in curves.iter().zip(NOVELTY_WEIGHTS).zip(NOVELTY_FLOORS) {
        let largest = (0..curve.len())
            .filter(inside)
            .map(|unit| curve[unit])
            .fold(0.0_f32, f32::max)
            .max(floor);
        for (total, value) in novelty.iter_mut().zip(curve) {
            *total += weight * value.max(0.0) / largest / weight_sum;
        }
    }
    peaks(&starts, &novelty, unit_seconds, duration)
}

/// The start of every unit of the track, from 0: its beats when `grid` is
/// trusted, every [`FIXED_UNIT_SECONDS`] otherwise.
fn unit_starts(grid: Option<&BeatGrid>, duration: f64) -> Vec<f64> {
    if let Some(grid) = grid.filter(|grid| grid.confidence() >= SYNCHRONISED_CONFIDENCE) {
        let beat = grid.beat_seconds();
        let first =
            grid.first_downbeat_seconds - (grid.first_downbeat_seconds / beat).floor() * beat;
        let mut starts: Vec<f64> = (0..)
            .map(|index| first + index as f64 * beat)
            .take_while(|start| *start < duration)
            .collect();
        if starts.first().is_some_and(|start| *start > 1e-6) {
            starts.insert(0, 0.0);
        }
        starts
    } else {
        (0..)
            .map(|index| index as f64 * FIXED_UNIT_SECONDS)
            .take_while(|start| *start < duration)
            .collect()
    }
}

fn median_step(starts: &[f64]) -> f64 {
    let mut steps: Vec<f64> = starts.windows(2).map(|pair| pair[1] - pair[0]).collect();
    steps.sort_by(f64::total_cmp);
    steps
        .get(steps.len() / 2)
        .copied()
        .unwrap_or(FIXED_UNIT_SECONDS)
}

/// The mean of the frames whose middle falls in each unit. A unit without a
/// frame takes the values of the one before.
fn aggregate(frames: &Frames, starts: &[f64]) -> Vec<Frame> {
    let empty = Frame {
        chroma: [0.0; 12],
        mel: [0.0; MEL_BANDS],
        power: 0.0,
        low_power: 0.0,
        high_power: 0.0,
        low_onsets: 0.0,
    };
    let mut sums = vec![empty; starts.len()];
    let mut counts = vec![0_usize; starts.len()];
    for (index, frame) in frames.frames.iter().enumerate() {
        let middle = frames.first_frame_seconds + index as f64 * frames.frame_seconds;
        let Some(unit) = starts
            .partition_point(|start| *start <= middle)
            .checked_sub(1)
        else {
            continue;
        };
        let sum = &mut sums[unit];
        for (total, value) in sum.chroma.iter_mut().zip(&frame.chroma) {
            *total += value;
        }
        for (total, value) in sum.mel.iter_mut().zip(&frame.mel) {
            *total += value;
        }
        sum.power += frame.power;
        sum.low_power += frame.low_power;
        sum.high_power += frame.high_power;
        sum.low_onsets += frame.low_onsets;
        counts[unit] += 1;
    }
    let mut units = Vec::with_capacity(starts.len());
    let mut previous = empty;
    for (sum, count) in sums.into_iter().zip(counts) {
        let unit = if count == 0 {
            previous
        } else {
            let scale = 1.0 / count as f32;
            Frame {
                chroma: sum.chroma.map(|value| value * scale),
                mel: sum.mel.map(|value| value * scale),
                power: sum.power * scale,
                low_power: sum.low_power * scale,
                high_power: sum.high_power * scale,
                low_onsets: sum.low_onsets * scale,
            }
        };
        units.push(unit);
        previous = unit;
    }
    units
}

type Harmony = [f32; 12];
type Timbre = [f32; TIMBRE_COEFFICIENTS];
type Level = [f32; 4];

fn decibels(power: f32) -> f32 {
    10.0 * (power + 1e-10).log10()
}

/// Each column of `rows` less its mean, divided by its spread or by
/// `minimum_spread` when that is larger.
fn standardise<const WIDTH: usize>(rows: &mut [[f32; WIDTH]], minimum_spread: [f32; WIDTH]) {
    let count = rows.len().max(1) as f32;
    for column in 0..WIDTH {
        let mean = rows.iter().map(|row| row[column]).sum::<f32>() / count;
        let variance = rows
            .iter()
            .map(|row| (row[column] - mean).powi(2))
            .sum::<f32>()
            / count;
        let spread = variance.sqrt().max(minimum_spread[column]);
        for row in rows.iter_mut() {
            row[column] = (row[column] - mean) / spread;
        }
    }
}

/// The harmony, the timbre and the level of every unit.
fn unit_features(units: &[Frame]) -> (Vec<Harmony>, Vec<Timbre>, Vec<Level>) {
    let loudest_chroma = units
        .iter()
        .flat_map(|unit| unit.chroma)
        .fold(0.0_f32, f32::max)
        .max(1e-12);
    let harmony = units
        .iter()
        .map(|unit| {
            let compressed = unit
                .chroma
                .map(|value| (100.0 * value / loudest_chroma).ln_1p());
            let norm = compressed
                .iter()
                .map(|value| value * value)
                .sum::<f32>()
                .sqrt();
            compressed.map(|value| value / norm.max(1e-9))
        })
        .collect();

    let log_mel: Vec<[f32; MEL_BANDS]> = units.iter().map(|unit| unit.mel.map(decibels)).collect();
    let loudest_band = log_mel
        .iter()
        .flatten()
        .copied()
        .fold(f32::NEG_INFINITY, f32::max);
    let cosines: Vec<[f32; MEL_BANDS]> = (1..=TIMBRE_COEFFICIENTS)
        .map(|coefficient| {
            std::array::from_fn(|band| {
                (std::f32::consts::PI * coefficient as f32 * (2 * band + 1) as f32
                    / (2 * MEL_BANDS) as f32)
                    .cos()
                    * (2.0 / MEL_BANDS as f32).sqrt()
            })
        })
        .collect();
    let mut timbre: Vec<Timbre> = log_mel
        .iter()
        .map(|bands| {
            let floored = bands.map(|band| band.max(loudest_band - MEL_FLOOR_DB));
            std::array::from_fn(|coefficient| {
                cosines[coefficient]
                    .iter()
                    .zip(&floored)
                    .map(|(cosine, band)| cosine * band)
                    .sum()
            })
        })
        .collect();
    standardise(&mut timbre, [TIMBRE_MINIMUM_SPREAD_DB; TIMBRE_COEFFICIENTS]);

    let mut level: Vec<Level> = units
        .iter()
        .map(|unit| {
            [
                decibels(unit.power),
                decibels(unit.low_power),
                decibels(unit.high_power),
                unit.low_onsets.ln_1p(),
            ]
        })
        .collect();
    for column in 0..3 {
        let loudest = level
            .iter()
            .map(|row| row[column])
            .fold(f32::NEG_INFINITY, f32::max);
        for row in &mut level {
            row[column] = row[column].max(loudest - LEVEL_FLOOR_DB);
        }
    }
    standardise(
        &mut level,
        [
            LEVEL_MINIMUM_SPREAD_DB,
            LEVEL_MINIMUM_SPREAD_DB,
            LEVEL_MINIMUM_SPREAD_DB,
            ONSET_MINIMUM_SPREAD,
        ],
    );
    (harmony, timbre, level)
}

/// One minus the cosine of the angle between two chroma vectors of length 1.
fn chroma_distance(first: &Harmony, second: &Harmony) -> f32 {
    let cosine: f32 = first
        .iter()
        .zip(second)
        .map(|(left, right)| left * right)
        .sum();
    1.0 - cosine.clamp(-1.0, 1.0)
}

/// The root mean square of the differences between two feature vectors.
fn euclidean_distance<const WIDTH: usize>(first: &[f32; WIDTH], second: &[f32; WIDTH]) -> f32 {
    let squares: f32 = first
        .iter()
        .zip(second)
        .map(|(left, right)| (left - right).powi(2))
        .sum();
    (squares / WIDTH as f32).sqrt()
}

/// Foote's novelty: at each unit, how much the units of the `half_width`
/// before it resemble each other and the units after it resemble each
/// other, more than the two sides resemble one another, weighted by a
/// Gaussian that favours the units nearest the middle. Units beyond the
/// ends of the track count as the first or the last.
fn foote_novelty<T>(
    features: &[T],
    half_width: usize,
    distance: impl Fn(&T, &T) -> f32,
) -> Vec<f32> {
    let count = features.len();
    if count < 4 {
        return vec![0.0; count];
    }
    let width = 2 * half_width;
    let offsets: Vec<f32> = (0..width)
        .map(|index| index as f32 - half_width as f32 + 0.5)
        .collect();
    let taper: Vec<f32> = offsets
        .iter()
        .map(|offset| (-0.5 * (offset / (half_width as f32 / 2.0)).powi(2)).exp())
        .collect();
    let positive_cells = (half_width * half_width * 2) as f32;
    let band: Vec<Vec<f32>> = (0..count)
        .map(|first| {
            (0..width.min(count - first))
                .map(|step| distance(&features[first], &features[first + step]))
                .collect()
        })
        .collect();
    let between = |first: usize, second: usize| {
        let (low, high) = if first <= second {
            (first, second)
        } else {
            (second, first)
        };
        band[low][high - low]
    };
    let clamp = |index: isize| index.clamp(0, count as isize - 1) as usize;
    (0..count)
        .map(|center| {
            let mut total = 0.0_f32;
            for row in 0..width {
                let first = clamp(center as isize + row as isize - half_width as isize);
                for column in 0..width {
                    let second = clamp(center as isize + column as isize - half_width as isize);
                    let same_side = (offsets[row] > 0.0) == (offsets[column] > 0.0);
                    let weight = taper[row] * taper[column];
                    let similarity = -between(first, second);
                    total += if same_side { weight } else { -weight } * similarity;
                }
            }
            total / positive_cells
        })
        .collect()
}

/// The section changes: the units whose novelty is the largest within
/// [`MINIMUM_SECTION_SECONDS`] around them and at least
/// [`BOUNDARY_SHARE`], the largest first, at least
/// [`MINIMUM_SECTION_SECONDS`] apart and [`EDGE_SECONDS`] from the ends.
fn peaks(
    starts: &[f64],
    novelty: &[f32],
    unit_seconds: f64,
    duration: f64,
) -> Vec<SectionBoundary> {
    let reach = ((MINIMUM_SECTION_SECONDS / unit_seconds / 2.0).round() as usize).max(1);
    let mut candidates: Vec<usize> = (0..novelty.len())
        .filter(|&unit| {
            let around =
                &novelty[unit.saturating_sub(reach)..(unit + reach + 1).min(novelty.len())];
            novelty[unit] >= BOUNDARY_SHARE && around.iter().all(|other| *other <= novelty[unit])
        })
        .collect();
    candidates.sort_by(|left, right| novelty[*right].total_cmp(&novelty[*left]));
    let mut accepted: Vec<usize> = Vec::new();
    for unit in candidates {
        let seconds = starts[unit];
        if seconds < EDGE_SECONDS || seconds > duration - EDGE_SECONDS {
            continue;
        }
        if accepted
            .iter()
            .all(|other| (starts[*other] - seconds).abs() >= MINIMUM_SECTION_SECONDS)
        {
            accepted.push(unit);
        }
    }
    accepted.sort_unstable();
    accepted
        .into_iter()
        .map(|unit| SectionBoundary {
            seconds: starts[unit],
            strength: novelty[unit],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;
    use crate::audio::AudioBuffer;

    const SAMPLE_RATE: u32 = 22_050;

    /// Stretches of (seconds, partials as (frequency, amplitude)), with a
    /// little noise so no unit is exactly silent.
    fn piece(stretches: &[(f64, &[(f64, f64)])]) -> AudioBuffer {
        let mut samples = Vec::new();
        let mut noise_state = 0x9e37_79b9_7f4a_7c15_u64;
        for (seconds, partials) in stretches {
            let count = (seconds * f64::from(SAMPLE_RATE)) as usize;
            for _ in 0..count {
                let time = samples.len() as f64 / f64::from(SAMPLE_RATE);
                noise_state ^= noise_state << 13;
                noise_state ^= noise_state >> 7;
                noise_state ^= noise_state << 17;
                let noise = ((noise_state >> 40) as f64 / (1_u64 << 24) as f64 - 0.5) * 0.002;
                let value: f64 = partials
                    .iter()
                    .map(|(frequency, amplitude)| {
                        amplitude * (std::f64::consts::TAU * frequency * time).sin()
                    })
                    .sum();
                samples.push((value + noise) as f32);
            }
        }
        AudioBuffer::new(samples, NonZeroU32::new(SAMPLE_RATE).unwrap())
    }

    fn boundaries_of(audio: &AudioBuffer) -> Vec<f64> {
        let envelope = OnsetEnvelope::of_audio(audio).unwrap();
        section_boundaries(&envelope, None)
            .into_iter()
            .map(|boundary| boundary.seconds)
            .collect()
    }

    const C_MAJOR: &[(f64, f64)] = &[(261.6, 0.2), (329.6, 0.2), (392.0, 0.2)];
    const F_SHARP_MAJOR: &[(f64, f64)] = &[(370.0, 0.2), (466.2, 0.2), (277.2, 0.2)];
    const BRIGHT_C: &[(f64, f64)] = &[
        (261.6, 0.2),
        (523.2, 0.15),
        (784.8, 0.12),
        (1_046.4, 0.1),
        (1_569.6, 0.08),
        (2_092.8, 0.06),
    ];

    #[test]
    fn a_change_of_chord_at_the_same_level_starts_a_section() {
        let audio = piece(&[(40.0, C_MAJOR), (40.0, F_SHARP_MAJOR)]);

        let found = boundaries_of(&audio);

        assert!(
            found.iter().any(|seconds| (seconds - 40.0).abs() <= 1.0),
            "{found:?}"
        );
    }

    #[test]
    fn a_change_of_timbre_on_the_same_note_starts_a_section() {
        let audio = piece(&[(40.0, &[(261.6, 0.3)]), (40.0, BRIGHT_C)]);

        let found = boundaries_of(&audio);

        assert!(
            found.iter().any(|seconds| (seconds - 40.0).abs() <= 1.0),
            "{found:?}"
        );
    }

    fn strengths_of(audio: &AudioBuffer) -> Vec<f32> {
        let envelope = OnsetEnvelope::of_audio(audio).unwrap();
        section_boundaries(&envelope, None)
            .into_iter()
            .map(|boundary| boundary.strength)
            .collect()
    }

    #[test]
    fn music_that_does_not_change_has_only_weak_changes() {
        let steady = strengths_of(&piece(&[(90.0, C_MAJOR)]));
        let changing = strengths_of(&piece(&[(45.0, C_MAJOR), (45.0, F_SHARP_MAJOR)]));

        let strongest = |strengths: &[f32]| strengths.iter().copied().fold(0.0_f32, f32::max);
        assert!(strongest(&steady) < 0.4, "{steady:?}");
        assert!(strongest(&changing) > 0.8, "{changing:?}");
    }

    #[test]
    fn a_trusted_grid_measures_beats_and_an_untrusted_one_fixed_units() {
        let trusted = BeatGrid::imported(0.25, 120.0);
        let starts = unit_starts(Some(&trusted), 4.0);
        assert_eq!(
            starts,
            [0.0, 0.25, 0.75, 1.25, 1.75, 2.25, 2.75, 3.25, 3.75]
        );

        let untrusted = BeatGrid {
            salience: 0.0,
            ..trusted
        };
        let starts = unit_starts(Some(&untrusted), 2.0);
        assert_eq!(starts, [0.0, 0.5, 1.0, 1.5]);
        assert_eq!(unit_starts(None, 1.0), [0.0, 0.5]);
    }
}
