// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A light audio fingerprint to recognise which track of a library is
//! playing from a few seconds of audio, and where in the track, also when
//! the track is played faster or slower.
//!
//! The audio is brought to 8 kHz mono and its spectrogram is searched for
//! peaks: points louder than everything around them in time and frequency.
//! Pairs of nearby peaks are hashed as (frequency of the first, frequency of
//! the second, time between them), so a hash does not depend on the level of
//! the audio, on where in the track the pair is, or on much of what else
//! plays at the same time. Matching a few seconds of audio against an index
//! of tracks counts, for every track and offset, how many hashes agree: the
//! right track and offset collect many votes, a wrong one a few by chance.
//!
//! A DJ plays tracks faster or slower, with the pitch following the tempo
//! or, with key lock, staying where it was. Both change the time between
//! two peaks, and the first changes their frequencies too. The index holds
//! the hashes of the track as it is; the query's peaks are moved, before
//! they are hashed, as each of a grid of tempo changes with and without the
//! pitch following would move them, and the grid step that agrees with the
//! most hashes wins. Frequencies are hashed on a log scale, so a small error
//! of the grid is a small error of a hash.
//!
//! The hashes of a track are kept in memory or stored by the caller; the
//! audio is not.

use std::collections::HashMap;
use std::f32::consts::TAU;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;

use crate::audio::AudioBuffer;

/// The sample rate the spectrogram is made at, in hertz.
pub const FINGERPRINT_SAMPLE_RATE: u32 = 8_000;
const WINDOW: usize = 1_024;
const HOP: usize = 256;
const LOWEST_BIN: usize = 24;
const HIGHEST_BIN: usize = 480;
const PEAK_FREQUENCY_RADIUS: usize = 6;
const PEAK_TIME_RADIUS: usize = 6;
const PEAKS_PER_FRAME: usize = 3;
const FLOOR_BELOW_LOUDEST_DB: f32 = 45.0;
const SHORTEST_PAIR_FRAMES: i64 = 4;
const LONGEST_PAIR_FRAMES: i64 = 80;
const WIDEST_PAIR_STEPS: i32 = 24;
const PAIRS_PER_ANCHOR: usize = 4;
/// Frequencies are hashed in steps of this ratio.
const FREQUENCY_STEP: f32 = 1.03;
const FREQUENCY_STEPS: i32 = 127;
/// Votes below this are chance, whatever the audio.
const MINIMUM_VOTES: u32 = 8;
/// At least this share of the query's hashes must agree.
const MINIMUM_SHARE: f32 = 0.05;
/// A query moved to another tempo is one of 20 tries, so by chance one of
/// them agrees more: it needs a larger share.
const MINIMUM_SCALED_SHARE: f32 = 0.07;
/// The best track must have this many times the votes of the next track.
const MINIMUM_LEAD: f32 = 2.0;
/// The tempo changes searched, as the fastest and slowest the query may be
/// played compared with the track, and the step between them.
const SLOWEST_RATIO: f32 = 0.90;
const FASTEST_RATIO: f32 = 1.10;
const RATIO_STEP: f32 = 0.02;
/// Hashes of a query at the track's own speed must agree on the offset to
/// this many frames; moved queries drift over their length, so they may
/// differ by more.
const OFFSET_TOLERANCE_FRAMES: i64 = 1;
const SCALED_OFFSET_TOLERANCE_FRAMES: i64 = 5;

/// Bumped when hashes change meaning, so stored ones are made again.
pub const FINGERPRINT_VERSION: u32 = 1;
/// A track is stored with one hash in this many, picked by value. A query
/// that keeps all of its hashes still matches.
pub const STORED_THINNING: u32 = 4;

/// How long one frame of the fingerprint is, in seconds.
pub const FRAME_SECONDS: f32 = HOP as f32 / FINGERPRINT_SAMPLE_RATE as f32;

/// The hashes of a stretch of audio, each with the frame it starts at, and
/// the peaks they were made from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fingerprint {
    hashes: Vec<(u32, u32)>,
    peaks: Vec<(u32, u16)>,
}

impl Fingerprint {
    /// How many hashes there are.
    pub fn len(&self) -> usize {
        self.hashes.len()
    }

    /// True when the audio gave no hash, as silence does.
    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }

    /// The hashes with their frames, in order of frame.
    pub fn hashes(&self) -> &[(u32, u32)] {
        &self.hashes
    }

    /// A fingerprint rebuilt from stored hashes. It matches at the speed of
    /// the track only: the peaks are not stored.
    pub fn from_hashes(hashes: Vec<(u32, u32)>) -> Self {
        Self {
            hashes,
            peaks: Vec::new(),
        }
    }

    /// Like [`Fingerprint::thinned`], by another function of the hash, so
    /// thinning what was already thinned keeps a fair share.
    pub fn thinned_again(&self, every: u32) -> Self {
        Self {
            hashes: self
                .hashes
                .iter()
                .copied()
                .filter(|(hash, _)| (hash.wrapping_mul(2_246_822_519) >> 11) % every == 0)
                .collect(),
            peaks: Vec::new(),
        }
    }

    /// The hashes as bytes, eight to a hash, for storing.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.hashes.len() * 8);
        for (hash, frame) in &self.hashes {
            bytes.extend_from_slice(&hash.to_le_bytes());
            bytes.extend_from_slice(&frame.to_le_bytes());
        }
        bytes
    }

    /// Hashes stored by [`Fingerprint::to_bytes`], or `None` when the bytes
    /// are not a whole number of hashes.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if !bytes.len().is_multiple_of(8) {
            return None;
        }
        Some(Self::from_hashes(
            bytes
                .chunks_exact(8)
                .map(|chunk| {
                    (
                        u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
                        u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]),
                    )
                })
                .collect(),
        ))
    }

    /// Keeps one hash in `every` by its value, for an index that stores
    /// less. A query that keeps all of its hashes still matches it.
    pub fn thinned(&self, every: u32) -> Self {
        Self {
            hashes: self
                .hashes
                .iter()
                .copied()
                .filter(|(hash, _)| (hash.wrapping_mul(2_654_435_761) >> 16) % every == 0)
                .collect(),
            peaks: Vec::new(),
        }
    }
}

/// The fingerprint of `audio`, at any sample rate.
pub fn fingerprint(audio: &AudioBuffer) -> Fingerprint {
    let samples = resample(audio.samples(), audio.sample_rate().get());
    let peaks = spectral_peaks(&samples);
    let moved: Vec<MovedPeak> = peaks
        .iter()
        .map(|peak| MovedPeak {
            frame: peak.frame as i64,
            bin: peak.bin as f32,
        })
        .collect();
    Fingerprint {
        hashes: pair_peaks(&moved),
        peaks: peaks
            .iter()
            .map(|peak| (peak.frame as u32, peak.bin as u16))
            .collect(),
    }
}

fn resample(samples: &[f32], from_rate: u32) -> Vec<f32> {
    if from_rate == FINGERPRINT_SAMPLE_RATE {
        return samples.to_vec();
    }
    let ratio = from_rate as f64 / f64::from(FINGERPRINT_SAMPLE_RATE);
    let width = ratio.max(1.0);
    let length = (samples.len() as f64 / ratio) as usize;
    (0..length)
        .map(|index| {
            let centre = index as f64 * ratio;
            let start = ((centre - width / 2.0).max(0.0)) as usize;
            let end = (((centre + width / 2.0) as usize) + 1).min(samples.len());
            let window = &samples[start.min(end.saturating_sub(1))..end];
            window.iter().sum::<f32>() / window.len().max(1) as f32
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
struct Peak {
    frame: usize,
    bin: usize,
}

#[derive(Debug, Clone, Copy)]
struct MovedPeak {
    frame: i64,
    bin: f32,
}

fn spectral_peaks(samples: &[f32]) -> Vec<Peak> {
    if samples.len() < WINDOW {
        return Vec::new();
    }
    let frame_count = (samples.len() - WINDOW) / HOP + 1;
    let fft = FftPlanner::new().plan_fft_forward(WINDOW);
    let window: Vec<f32> = (0..WINDOW)
        .map(|index| 0.5 - 0.5 * (TAU * index as f32 / WINDOW as f32).cos())
        .collect();
    let width = HIGHEST_BIN - LOWEST_BIN;
    let mut levels = vec![0.0_f32; frame_count * width];
    let mut buffer = vec![Complex::new(0.0, 0.0); WINDOW];
    for frame in 0..frame_count {
        let start = frame * HOP;
        for (index, slot) in buffer.iter_mut().enumerate() {
            *slot = Complex::new(samples[start + index] * window[index], 0.0);
        }
        fft.process(&mut buffer);
        for bin in 0..width {
            let magnitude = buffer[LOWEST_BIN + bin].norm();
            levels[frame * width + bin] = 20.0 * (magnitude + 1.0e-9).log10();
        }
    }
    let loudest = levels.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let floor = loudest - FLOOR_BELOW_LOUDEST_DB;
    let mut peaks = Vec::new();
    for frame in 0..frame_count {
        let mut found: Vec<(f32, usize)> = Vec::new();
        for bin in 0..width {
            let level = levels[frame * width + bin];
            if level < floor {
                continue;
            }
            let is_peak = (frame.saturating_sub(PEAK_TIME_RADIUS)
                ..=(frame + PEAK_TIME_RADIUS).min(frame_count - 1))
                .all(|other_frame| {
                    (bin.saturating_sub(PEAK_FREQUENCY_RADIUS)
                        ..=(bin + PEAK_FREQUENCY_RADIUS).min(width - 1))
                        .all(|other_bin| {
                            (other_frame == frame && other_bin == bin)
                                || levels[other_frame * width + other_bin] < level
                        })
                });
            if is_peak {
                found.push((level, bin + LOWEST_BIN));
            }
        }
        found.sort_by(|first, second| second.0.total_cmp(&first.0));
        peaks.extend(
            found
                .into_iter()
                .take(PEAKS_PER_FRAME)
                .map(|(_, bin)| Peak { frame, bin }),
        );
    }
    peaks
}

fn frequency_step(bin: f32) -> Option<i32> {
    if bin < LOWEST_BIN as f32 * 0.8 || bin > HIGHEST_BIN as f32 * 1.25 {
        return None;
    }
    let step = (bin / LOWEST_BIN as f32).ln() / FREQUENCY_STEP.ln();
    let step = step.round() as i32;
    (0..FREQUENCY_STEPS).contains(&step).then_some(step)
}

fn pair_peaks(peaks: &[MovedPeak]) -> Vec<(u32, u32)> {
    let mut hashes = Vec::new();
    for (index, anchor) in peaks.iter().enumerate() {
        let Some(anchor_step) = frequency_step(anchor.bin) else {
            continue;
        };
        let mut paired = 0;
        for target in &peaks[index + 1..] {
            let gap = target.frame - anchor.frame;
            if gap > LONGEST_PAIR_FRAMES {
                break;
            }
            if gap < SHORTEST_PAIR_FRAMES {
                continue;
            }
            let Some(target_step) = frequency_step(target.bin) else {
                continue;
            };
            if (target_step - anchor_step).abs() > WIDEST_PAIR_STEPS {
                continue;
            }
            let hash = ((anchor_step as u32) << 14) | ((target_step as u32) << 7) | gap as u32;
            hashes.push((hash, anchor.frame.max(0) as u32));
            paired += 1;
            if paired == PAIRS_PER_ANCHOR {
                break;
            }
        }
    }
    hashes
}

/// How a query was played compared with the track it matched.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playback {
    /// How much faster the query was played than the track: 1.05 is 5 percent
    /// faster.
    pub tempo_ratio: f32,
    /// True when the pitch followed the tempo, as with a turntable or a pitch
    /// fader without key lock.
    pub pitch_follows_tempo: bool,
}

/// Which track a stretch of audio is from and where.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Match {
    /// The id the track was added with.
    pub track: usize,
    /// Where in the track the audio starts, in seconds of the track. The
    /// audio playing now is this plus its own length times the tempo ratio.
    pub start_seconds: f32,
    /// How many hashes agree on the track and the position.
    pub votes: u32,
    /// How many the best other track had.
    pub runner_up_votes: u32,
    /// The share of the query's hashes that voted for this track and position.
    pub share: f32,
    /// How the track was played.
    pub playback: Playback,
}

#[derive(Debug, Clone, Copy)]
struct Candidate {
    track: u32,
    offset: i64,
    votes: u32,
    share: f32,
    playback: Playback,
}

/// The fingerprints of a library, searched by hash. Hashes are kept in one
/// list sorted by hash, which costs 12 bytes a hash and nothing else.
#[derive(Debug, Clone, Default)]
pub struct FingerprintIndex {
    entries: Vec<(u32, u32, u32)>,
    is_sorted: bool,
}

impl FingerprintIndex {
    /// An index without tracks.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            is_sorted: true,
        }
    }

    /// Adds a track under `track`, an id the caller chooses. Call
    /// [`FingerprintIndex::freeze`] after the last one and before searching.
    pub fn add(&mut self, track: usize, fingerprint: &Fingerprint) {
        self.entries.extend(
            fingerprint
                .hashes()
                .iter()
                .map(|(hash, frame)| (*hash, track as u32, *frame)),
        );
        self.is_sorted = false;
    }

    /// Sorts the hashes for searching.
    pub fn freeze(&mut self) {
        if !self.is_sorted {
            self.entries.sort_unstable();
            self.entries.shrink_to_fit();
            self.is_sorted = true;
        }
    }

    /// How many hashes the index holds.
    pub fn hash_count(&self) -> usize {
        self.entries.len()
    }

    fn hits(&self, hash: u32) -> &[(u32, u32, u32)] {
        let start = self.entries.partition_point(|entry| entry.0 < hash);
        let end = start + self.entries[start..].partition_point(|entry| entry.0 == hash);
        &self.entries[start..end]
    }

    /// The track and position that most of `query`'s hashes agree on, or
    /// `None` when no track has clearly more votes than chance and than the
    /// others. A query made from audio is searched at every tempo of the
    /// grid, with the pitch following it and without; one rebuilt from
    /// hashes only at the track's own speed.
    pub fn find(&self, query: &Fingerprint) -> Option<Match> {
        debug_assert!(self.is_sorted, "freeze the index before searching it");
        let mut passes: Vec<(Vec<(u32, u32)>, Playback)> = vec![(
            query.hashes.clone(),
            Playback {
                tempo_ratio: 1.0,
                pitch_follows_tempo: false,
            },
        )];
        if !query.peaks.is_empty() {
            let steps = ((FASTEST_RATIO - SLOWEST_RATIO) / RATIO_STEP).round() as i32;
            for step in 0..=steps {
                let ratio = SLOWEST_RATIO + step as f32 * RATIO_STEP;
                if (ratio - 1.0).abs() < RATIO_STEP / 2.0 {
                    continue;
                }
                for pitch_follows_tempo in [false, true] {
                    let moved: Vec<MovedPeak> = query
                        .peaks
                        .iter()
                        .map(|(frame, bin)| MovedPeak {
                            frame: (*frame as f32 * ratio).round() as i64,
                            bin: if pitch_follows_tempo {
                                f32::from(*bin) / ratio
                            } else {
                                f32::from(*bin)
                            },
                        })
                        .collect();
                    passes.push((
                        pair_peaks(&moved),
                        Playback {
                            tempo_ratio: ratio,
                            pitch_follows_tempo,
                        },
                    ));
                }
            }
        }
        let mut best: Option<Candidate> = None;
        let mut runner_ups: Vec<(u32, u32)> = Vec::new();
        for (hashes, playback) in &passes {
            let tolerance = if playback.tempo_ratio == 1.0 {
                OFFSET_TOLERANCE_FRAMES
            } else {
                SCALED_OFFSET_TOLERANCE_FRAMES
            };
            for (track, offset, votes) in self.tally(hashes, tolerance) {
                runner_ups.push((track, votes));
                if best.is_none_or(|current| votes > current.votes) {
                    best = Some(Candidate {
                        track,
                        offset,
                        votes,
                        share: votes as f32 / hashes.len().max(1) as f32,
                        playback: *playback,
                    });
                }
            }
        }
        let best = best?;
        let runner_up = runner_ups
            .iter()
            .filter(|(track, _)| *track != best.track)
            .map(|(_, votes)| *votes)
            .max()
            .unwrap_or(0);
        let minimum_share = if best.playback.tempo_ratio == 1.0 {
            MINIMUM_SHARE
        } else {
            MINIMUM_SCALED_SHARE
        };
        if best.votes < MINIMUM_VOTES
            || best.share < minimum_share
            || (best.votes as f32) < MINIMUM_LEAD * runner_up as f32
        {
            return None;
        }
        Some(Match {
            track: best.track as usize,
            start_seconds: best.offset.max(0) as f32 * FRAME_SECONDS,
            votes: best.votes,
            runner_up_votes: runner_up,
            share: best.share,
            playback: best.playback,
        })
    }

    /// The best offset of each track with votes, with its votes.
    fn tally(&self, hashes: &[(u32, u32)], tolerance: i64) -> Vec<(u32, i64, u32)> {
        let mut votes: HashMap<(u32, i64), u32> = HashMap::new();
        for (hash, query_frame) in hashes {
            for (_, track, track_frame) in self.hits(*hash) {
                let offset = i64::from(*track_frame) - i64::from(*query_frame);
                *votes.entry((*track, offset)).or_default() += 1;
            }
        }
        let mut best_per_track: HashMap<u32, (i64, u32)> = HashMap::new();
        for (track, offset) in votes.keys() {
            let combined: u32 = (offset - tolerance..=offset + tolerance)
                .map(|near| votes.get(&(*track, near)).copied().unwrap_or(0))
                .sum();
            let entry = best_per_track.entry(*track).or_insert((*offset, 0));
            if combined > entry.1 || (combined == entry.1 && *offset < entry.0) {
                *entry = (*offset, combined);
            }
        }
        let mut top: Vec<(u32, i64, u32)> = best_per_track
            .into_iter()
            .map(|(track, (offset, combined))| (track, offset, combined))
            .collect();
        top.sort_by_key(|(track, _, combined)| (std::cmp::Reverse(*combined), *track));
        top.truncate(2);
        top
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    const TRACK_RATE: u32 = 11_025;
    const TRACK_SECONDS: usize = 60;

    struct Random(u64);

    impl Random {
        fn next(&mut self) -> f32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((self.0 >> 40) as f32) / (1u64 << 24) as f32
        }

        fn below(&mut self, bound: usize) -> usize {
            (self.next() * bound as f32) as usize % bound
        }
    }

    fn add_note(samples: &mut [f32], start: usize, frequency: f32, seconds: f32, gain: f32) {
        let length = (seconds * TRACK_RATE as f32) as usize;
        for offset in 0..length.min(samples.len().saturating_sub(start)) {
            let time = offset as f32 / TRACK_RATE as f32;
            let envelope = (-time * 3.0 / seconds).exp() * (1.0 - (-time * 200.0).exp());
            let mut value = 0.0;
            for harmonic in 1..=3 {
                value += (TAU * frequency * harmonic as f32 * time).sin() / harmonic as f32;
            }
            samples[start + offset] += gain * envelope * value;
        }
    }

    fn synthetic_track(seed: u64) -> Vec<f32> {
        synthetic_track_at(seed, 1.0)
    }

    /// The same track played `tempo_ratio` times as fast with key lock: the
    /// notes keep their pitch and length, and come that much sooner.
    fn synthetic_track_at(seed: u64, tempo_ratio: f32) -> Vec<f32> {
        let mut random = Random(seed.wrapping_mul(7919) + 13);
        let total = TRACK_SECONDS * TRACK_RATE as usize;
        let mut samples = vec![0.0; total];
        let tempo = 100.0 + random.below(40) as f32;
        let beat = (60.0 / tempo / tempo_ratio * TRACK_RATE as f32) as usize;
        let root = 110.0 * 2.0_f32.powf(random.below(12) as f32 / 12.0);
        let scale = [0, 2, 3, 5, 7, 8, 10];
        let note = |degree: usize, octave: u32| {
            root * 2.0_f32.powf(scale[degree % 7] as f32 / 12.0) * 2.0_f32.powi(octave as i32)
        };
        let mut position = 0;
        while position + beat < total {
            let kick_length = beat / 3;
            for offset in 0..kick_length.min(total - position) {
                let time = offset as f32 / TRACK_RATE as f32;
                let frequency = 45.0 + 90.0 * (-time * 30.0).exp();
                samples[position + offset] +=
                    0.5 * (-time * 12.0).exp() * (TAU * frequency * time).sin();
            }
            position += beat;
        }
        let mut position = 0;
        while position + beat < total {
            let bar_degree = random.below(7);
            for step in 0..4 {
                let start = position + step * beat / 2;
                let degree = bar_degree + [0, 2, 4, 7][random.below(4)];
                add_note(&mut samples, start, note(degree, 1), 0.45, 0.25);
            }
            if (position / beat).is_multiple_of(4) {
                add_note(&mut samples, position, note(bar_degree, 0), 1.6, 0.3);
                add_note(&mut samples, position, note(bar_degree + 2, 0), 1.6, 0.2);
            }
            position += 2 * beat;
        }
        samples
    }

    fn excerpt(track: &[f32], start_seconds: f32, seconds: f32) -> Vec<f32> {
        let start = (start_seconds * TRACK_RATE as f32) as usize;
        track[start..start + (seconds * TRACK_RATE as f32) as usize].to_vec()
    }

    fn audio(samples: Vec<f32>, rate: u32) -> AudioBuffer {
        AudioBuffer::new(samples, NonZeroU32::new(rate).unwrap())
    }

    fn rms(samples: &[f32]) -> f32 {
        (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt()
    }

    fn mixed(main: &[f32], other: &[f32], other_db: f32) -> Vec<f32> {
        let gain = rms(main) / rms(other) * 10.0_f32.powf(other_db / 20.0);
        main.iter()
            .zip(other)
            .map(|(left, right)| left + right * gain)
            .collect()
    }

    fn upsampled(samples: &[f32], factor: f32) -> Vec<f32> {
        let length = (samples.len() as f32 * factor) as usize;
        (0..length)
            .map(|index| {
                let position = index as f32 / factor;
                let whole = position as usize;
                let fraction = position - whole as f32;
                let next = samples.get(whole + 1).copied().unwrap_or(0.0);
                samples[whole] * (1.0 - fraction) + next * fraction
            })
            .collect()
    }

    struct Library {
        tracks: Vec<Vec<f32>>,
        index: FingerprintIndex,
    }

    fn library(count: usize) -> Library {
        let tracks: Vec<Vec<f32>> = (0..count as u64).map(synthetic_track).collect();
        let mut index = FingerprintIndex::new();
        for (id, track) in tracks.iter().enumerate() {
            index.add(id, &fingerprint(&audio(track.clone(), TRACK_RATE)));
        }
        index.freeze();
        Library { tracks, index }
    }

    const EXCERPT_SECONDS: f32 = 15.0;

    struct Outcome {
        correct: usize,
        total: usize,
        wrong_track: usize,
        no_match: usize,
        wrong_position: usize,
        shares: Vec<f32>,
    }

    fn run(library: &Library, condition: impl Fn(&[f32], &[f32]) -> AudioBuffer) -> Outcome {
        run_accepting(library, false, condition)
    }

    fn run_accepting(
        library: &Library,
        other_track_is_playing_too: bool,
        condition: impl Fn(&[f32], &[f32]) -> AudioBuffer,
    ) -> Outcome {
        let mut outcome = Outcome {
            correct: 0,
            total: 0,
            wrong_track: 0,
            no_match: 0,
            wrong_position: 0,
            shares: Vec::new(),
        };
        let count = library.tracks.len();
        for (id, track) in library.tracks.iter().enumerate() {
            for start in [3.0, 21.0, 38.0] {
                let wanted = excerpt(track, start, EXCERPT_SECONDS);
                let other = excerpt(
                    &library.tracks[(id + 1) % count],
                    start + 2.0,
                    EXCERPT_SECONDS,
                );
                let query = fingerprint(&condition(&wanted, &other));
                outcome.total += 1;
                let result = library.index.find(&query);
                if let Some(found) = result {
                    outcome.shares.push(found.share);
                }
                match result {
                    None => outcome.no_match += 1,
                    Some(found)
                        if other_track_is_playing_too && found.track == (id + 1) % count =>
                    {
                        outcome.correct += 1;
                    }
                    Some(found) if found.track != id => outcome.wrong_track += 1,
                    Some(found) if (found.start_seconds - start).abs() > 0.5 => {
                        outcome.wrong_position += 1;
                    }
                    Some(_) => outcome.correct += 1,
                }
            }
        }
        outcome
    }

    fn describe(name: &str, outcome: &Outcome) -> String {
        format!(
            "{name}: {}/{} right (wrong track {}, no match {}, wrong position {}), lowest share of hashes {:.2}",
            outcome.correct,
            outcome.total,
            outcome.wrong_track,
            outcome.no_match,
            outcome.wrong_position,
            outcome.shares.iter().copied().fold(1.0, f32::min)
        )
    }

    #[test]
    fn it_recognises_tracks_and_positions_under_gain_changes_and_a_second_track() {
        let library = library(10);
        let conditions: Vec<(&str, Outcome)> = vec![
            (
                "clean",
                run(&library, |wanted, _| audio(wanted.to_vec(), TRACK_RATE)),
            ),
            (
                "gain -18 dB",
                run(&library, |wanted, _| {
                    audio(
                        wanted.iter().map(|sample| sample * 0.125).collect(),
                        TRACK_RATE,
                    )
                }),
            ),
            (
                "gain +12 dB",
                run(&library, |wanted, _| {
                    audio(
                        wanted.iter().map(|sample| sample * 4.0).collect(),
                        TRACK_RATE,
                    )
                }),
            ),
            (
                "second track at -10 dB",
                run(&library, |wanted, other| {
                    audio(mixed(wanted, other, -10.0), TRACK_RATE)
                }),
            ),
            (
                "second track at -3 dB, either track counts",
                run_accepting(&library, true, |wanted, other| {
                    audio(mixed(wanted, other, -3.0), TRACK_RATE)
                }),
            ),
            (
                "captured at 48 kHz",
                run(&library, |wanted, _| {
                    audio(upsampled(wanted, 48_000.0 / TRACK_RATE as f32), 48_000)
                }),
            ),
        ];
        for (name, outcome) in &conditions {
            println!("{}", describe(name, outcome));
        }
        let accuracy = |name: &str| {
            let outcome = &conditions
                .iter()
                .find(|(label, _)| *label == name)
                .unwrap()
                .1;
            outcome.correct as f32 / outcome.total as f32
        };
        assert!(accuracy("clean") >= 0.95);
        assert!(accuracy("gain -18 dB") >= 0.95);
        assert!(accuracy("gain +12 dB") >= 0.95);
        assert!(accuracy("second track at -10 dB") >= 0.9);
        assert!(accuracy("captured at 48 kHz") >= 0.9);
        for (name, outcome) in &conditions {
            assert_eq!(outcome.wrong_track, 0, "{}", describe(name, outcome));
        }
    }

    #[test]
    fn audio_of_a_track_that_is_not_in_the_library_matches_nothing() {
        let library = library(10);
        for seed in 100..112 {
            let stranger = synthetic_track(seed);
            let query = fingerprint(&audio(
                excerpt(&stranger, 10.0, EXCERPT_SECONDS),
                TRACK_RATE,
            ));
            let found = library.index.find(&query);
            println!("stranger {seed}: {found:?}");
            assert_eq!(found, None, "seed {seed}");
        }
    }

    #[test]
    fn silence_and_noise_give_no_match() {
        let library = library(4);
        let silence = fingerprint(&audio(vec![0.0; TRACK_RATE as usize * 15], TRACK_RATE));
        assert!(silence.is_empty());
        assert_eq!(library.index.find(&silence), None);
        let mut random = Random(5);
        let noise: Vec<f32> = (0..TRACK_RATE as usize * 15)
            .map(|_| random.next() - 0.5)
            .collect();
        assert_eq!(
            library.index.find(&fingerprint(&audio(noise, TRACK_RATE))),
            None
        );
    }

    #[test]
    fn a_thinned_index_still_matches_and_costs_a_fraction() {
        let tracks: Vec<Vec<f32>> = (0..6).map(synthetic_track).collect();
        let mut full = FingerprintIndex::new();
        let mut thin = FingerprintIndex::new();
        for (id, track) in tracks.iter().enumerate() {
            let print = fingerprint(&audio(track.clone(), TRACK_RATE));
            full.add(id, &print);
            thin.add(id, &print.thinned(4));
        }
        full.freeze();
        thin.freeze();
        assert!(thin.hash_count() * 3 < full.hash_count());
        let query = fingerprint(&audio(
            excerpt(&tracks[2], 20.0, EXCERPT_SECONDS),
            TRACK_RATE,
        ));
        let found = thin.find(&query).expect("a match in the thinned index");
        assert_eq!(found.track, 2);
        assert!((found.start_seconds - 20.0).abs() <= 0.5);
        println!(
            "{} hashes for {} s of audio, {:.1} per second, {} bytes per minute at 8 bytes a hash, thinned by 4: {:.1} per second",
            full.hash_count() / 6,
            TRACK_SECONDS,
            full.hash_count() as f32 / 6.0 / TRACK_SECONDS as f32,
            (full.hash_count() as f32 / 6.0 / TRACK_SECONDS as f32 * 60.0 * 8.0) as usize,
            thin.hash_count() as f32 / 6.0 / TRACK_SECONDS as f32
        );
    }

    #[test]
    fn it_is_the_same_for_the_same_audio_and_empty_for_too_little() {
        let track = synthetic_track(1);
        let first = fingerprint(&audio(excerpt(&track, 0.0, 5.0), TRACK_RATE));
        let second = fingerprint(&audio(excerpt(&track, 0.0, 5.0), TRACK_RATE));
        assert_eq!(first, second);
        assert!(fingerprint(&audio(vec![0.1; 100], TRACK_RATE)).is_empty());
    }

    struct Scaled {
        tried: usize,
        right: usize,
        wrong_track: usize,
        no_match: usize,
        wrong_position: usize,
        playbacks_named: usize,
        lowest_share: f32,
    }

    fn run_scaled(
        library: &Library,
        tracks_tried: usize,
        ratio: f32,
        pitch_follows_tempo: bool,
        key_lock_tracks: &[Vec<f32>],
    ) -> Scaled {
        let mut outcome = Scaled {
            tried: 0,
            right: 0,
            wrong_track: 0,
            no_match: 0,
            wrong_position: 0,
            playbacks_named: 0,
            lowest_share: 1.0,
        };
        for id in 0..tracks_tried {
            for query_start in [3.0, 21.0, 36.0] {
                let (samples, expected) = if pitch_follows_tempo {
                    let start = query_start * ratio;
                    let wanted = excerpt(&library.tracks[id], start, EXCERPT_SECONDS * ratio);
                    (upsampled(&wanted, 1.0 / ratio), start)
                } else {
                    (
                        excerpt(&key_lock_tracks[id], query_start, EXCERPT_SECONDS),
                        query_start * ratio,
                    )
                };
                let query = fingerprint(&audio(samples, TRACK_RATE));
                outcome.tried += 1;
                match library.index.find(&query) {
                    None => outcome.no_match += 1,
                    Some(found) if found.track != id => outcome.wrong_track += 1,
                    Some(found) if (found.start_seconds - expected).abs() > 0.6 => {
                        outcome.wrong_position += 1;
                    }
                    Some(found) => {
                        outcome.right += 1;
                        outcome.lowest_share = outcome.lowest_share.min(found.share);
                        if (found.playback.tempo_ratio - ratio).abs() <= 0.021
                            && found.playback.pitch_follows_tempo == pitch_follows_tempo
                        {
                            outcome.playbacks_named += 1;
                        }
                    }
                }
            }
        }
        outcome
    }

    #[test]
    fn it_recognises_tracks_played_faster_or_slower_with_and_without_key_lock() {
        let library = library(10);
        let tried = 5;
        let mut report = Vec::new();
        for ratio in [0.92, 0.95, 0.97, 1.03, 1.05, 1.07, 1.08] {
            let locked: Vec<Vec<f32>> = (0..tried as u64)
                .map(|seed| synthetic_track_at(seed, ratio))
                .collect();
            for (label, pitch_follows_tempo) in [("key lock", false), ("pitch follows", true)] {
                let outcome = run_scaled(&library, tried, ratio, pitch_follows_tempo, &locked);
                let line = format!(
                    "{:+.0}% {label}: {}/{} right (wrong track {}, no match {}, wrong position {}), named the playback {}, lowest share {:.2}",
                    (ratio - 1.0) * 100.0,
                    outcome.right * 1,
                    outcome.tried * 3 / 3,
                    outcome.wrong_track,
                    outcome.no_match,
                    outcome.wrong_position,
                    outcome.playbacks_named,
                    outcome.lowest_share
                );
                println!("{line}");
                report.push((line, outcome));
            }
        }
        for (line, outcome) in &report {
            assert_eq!(outcome.wrong_track, 0, "{line}");
            assert!(outcome.right * 10 >= outcome.tried * 8, "{line}");
        }
    }
}
