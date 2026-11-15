// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A light audio fingerprint to recognise which track of a library is
//! playing from a few seconds of audio, and where in the track.
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
const HOP: usize = 512;
const LOWEST_BIN: usize = 24;
const HIGHEST_BIN: usize = 480;
const PEAK_FREQUENCY_RADIUS: usize = 6;
const PEAK_TIME_RADIUS: usize = 3;
const PEAKS_PER_FRAME: usize = 3;
const FLOOR_BELOW_LOUDEST_DB: f32 = 45.0;
const SHORTEST_PAIR_FRAMES: usize = 2;
const LONGEST_PAIR_FRAMES: usize = 40;
const WIDEST_PAIR_BINS: i32 = 64;
const PAIRS_PER_ANCHOR: usize = 4;
/// Votes below this are chance, whatever the audio.
const MINIMUM_VOTES: u32 = 8;
/// At least this share of the query's hashes must agree.
const MINIMUM_SHARE: f32 = 0.05;
/// The best track must have this many times the votes of the next track.
const MINIMUM_LEAD: f32 = 2.0;

/// How long one frame of the fingerprint is, in seconds.
pub const FRAME_SECONDS: f32 = HOP as f32 / FINGERPRINT_SAMPLE_RATE as f32;

/// The hashes of a stretch of audio, each with the frame it starts at.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fingerprint {
    hashes: Vec<(u32, u32)>,
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

    /// A fingerprint rebuilt from stored hashes.
    pub fn from_hashes(hashes: Vec<(u32, u32)>) -> Self {
        Self { hashes }
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
        }
    }
}

/// The fingerprint of `audio`, at any sample rate.
pub fn fingerprint(audio: &AudioBuffer) -> Fingerprint {
    let samples = resample(audio.samples(), audio.sample_rate().get());
    let peaks = spectral_peaks(&samples);
    Fingerprint {
        hashes: pair_peaks(&peaks),
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

fn pair_peaks(peaks: &[Peak]) -> Vec<(u32, u32)> {
    let mut hashes = Vec::new();
    for (index, anchor) in peaks.iter().enumerate() {
        let mut paired = 0;
        for target in &peaks[index + 1..] {
            let gap = target.frame - anchor.frame;
            if gap > LONGEST_PAIR_FRAMES {
                break;
            }
            let difference = target.bin as i32 - anchor.bin as i32;
            if gap < SHORTEST_PAIR_FRAMES || difference.abs() > WIDEST_PAIR_BINS {
                continue;
            }
            let hash = ((anchor.bin as u32) << 13)
                | (((difference + WIDEST_PAIR_BINS) as u32) << 6)
                | gap as u32;
            hashes.push((hash, anchor.frame as u32));
            paired += 1;
            if paired == PAIRS_PER_ANCHOR {
                break;
            }
        }
    }
    hashes
}

/// Which track a stretch of audio is from and where.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Match {
    /// The id the track was added with.
    pub track: usize,
    /// Where in the track the audio starts, in seconds. The audio playing
    /// now is this plus its own length.
    pub start_seconds: f32,
    /// How many hashes agree on the track and the position.
    pub votes: u32,
    /// How many the best other track had.
    pub runner_up_votes: u32,
    /// The share of the query's hashes that voted for this track and position.
    pub share: f32,
}

/// The fingerprints of a library, searched by hash.
#[derive(Debug, Clone, Default)]
pub struct FingerprintIndex {
    entries: HashMap<u32, Vec<(u32, u32)>>,
    hash_count: usize,
}

impl FingerprintIndex {
    /// An index without tracks.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a track under `track`, an id the caller chooses.
    pub fn add(&mut self, track: usize, fingerprint: &Fingerprint) {
        for (hash, frame) in fingerprint.hashes() {
            self.entries
                .entry(*hash)
                .or_default()
                .push((track as u32, *frame));
            self.hash_count += 1;
        }
    }

    /// How many hashes the index holds.
    pub fn hash_count(&self) -> usize {
        self.hash_count
    }

    /// The track and position that most of `query`'s hashes agree on, or
    /// `None` when no track has clearly more votes than chance and than the
    /// others.
    pub fn find(&self, query: &Fingerprint) -> Option<Match> {
        let mut votes: HashMap<(u32, i64), u32> = HashMap::new();
        for (hash, query_frame) in query.hashes() {
            let Some(hits) = self.entries.get(hash) else {
                continue;
            };
            for (track, track_frame) in hits {
                let offset = i64::from(*track_frame) - i64::from(*query_frame);
                *votes.entry((*track, offset)).or_default() += 1;
            }
        }
        let combined = |track: u32, offset: i64| -> u32 {
            (offset - 1..=offset + 1)
                .map(|near| votes.get(&(track, near)).copied().unwrap_or(0))
                .sum()
        };
        let (best_track, best_offset, best_votes) = votes
            .keys()
            .map(|(track, offset)| (*track, *offset, combined(*track, *offset)))
            .max_by_key(|(track, offset, count)| (*count, std::cmp::Reverse((*track, *offset))))?;
        let runner_up = votes
            .keys()
            .filter(|(track, _)| *track != best_track)
            .map(|(track, offset)| combined(*track, *offset))
            .max()
            .unwrap_or(0);
        let share = best_votes as f32 / query.len().max(1) as f32;
        if best_votes < MINIMUM_VOTES
            || share < MINIMUM_SHARE
            || (best_votes as f32) < MINIMUM_LEAD * runner_up as f32
        {
            return None;
        }
        Some(Match {
            track: best_track as usize,
            start_seconds: best_offset.max(0) as f32 * FRAME_SECONDS,
            votes: best_votes,
            runner_up_votes: runner_up,
            share,
        })
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
        let mut random = Random(seed.wrapping_mul(7919) + 13);
        let total = TRACK_SECONDS * TRACK_RATE as usize;
        let mut samples = vec![0.0; total];
        let tempo = 100.0 + random.below(40) as f32;
        let beat = (60.0 / tempo * TRACK_RATE as f32) as usize;
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
        for seed in [100, 101, 102, 103] {
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
}
