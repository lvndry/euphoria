// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Chooses among the tempogram's best tempos by how strongly the onsets of
//! the track repeat at each one.
//!
//! The tempogram often ranks a tempo that is a dotted or syncopated
//! subdivision of the beat first, such as 173 BPM for a 130 BPM track. The
//! beat itself is the tempo at which the drum pattern repeats: after one beat,
//! after a bar of four beats, and, between those, after half a beat. The
//! snare, claps and chords of the mid band carry that pattern, so each
//! candidate gets one weighted sum of its tempogram score and the three repeat
//! strengths of the mid band onsets.

use stratum_dsp::features::period::tempogram::TempogramCandidateDebug;

use crate::tempo_range::TempoRange;

/// Tempos closer than this share of each other count as the same tempo.
const SAME_TEMPO_TOLERANCE: f32 = 0.04;

/// How many distinct tempos compete.
const CANDIDATE_COUNT: usize = 4;

/// The mid band, where snares, claps and chords sound, in hertz.
const MID_BAND_HERTZ: std::ops::Range<f32> = 150.0..1_500.0;

/// Repeat distances measured, in beats.
const REPEAT_BEATS: [f32; 3] = [1.0, 4.0, 0.5];

const FEATURE_COUNT: usize = 1 + REPEAT_BEATS.len();

/// Weights of the tempogram score relative to the best candidate's, then of
/// the repeat strengths at [`REPEAT_BEATS`]. Fitted on the development half
/// of GiantSteps Tempo.
const WEIGHTS: [f32; FEATURE_COUNT] = [2.94, 1.58, 1.70, 2.94];

/// A tempo the tempogram scored, placed in the default range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Candidate {
    pub tempo: f32,
    pub score: f32,
}

/// The best scoring tempos of the tempogram that differ from each other,
/// each placed in the default range, best first.
pub(super) fn leading_candidates(candidates: &[TempogramCandidateDebug]) -> Vec<Candidate> {
    let mut ranked: Vec<&TempogramCandidateDebug> = candidates
        .iter()
        .filter(|candidate| candidate.bpm.is_finite() && candidate.bpm > 0.0)
        .collect();
    ranked.sort_by(|first, second| second.score.total_cmp(&first.score));
    let mut leading: Vec<Candidate> = Vec::with_capacity(CANDIDATE_COUNT);
    for candidate in ranked {
        let tempo = TempoRange::DEFAULT.fold(candidate.bpm);
        let repeated = leading
            .iter()
            .any(|kept| (tempo - kept.tempo).abs() <= SAME_TEMPO_TOLERANCE * kept.tempo);
        if !repeated {
            leading.push(Candidate {
                tempo,
                score: candidate.score,
            });
        }
        if leading.len() == CANDIDATE_COUNT {
            break;
        }
    }
    leading
}

/// How the mid band onsets of a track change from frame to frame.
pub(super) struct OnsetPulse {
    frames_per_second: f32,
    onsets: Vec<f32>,
    energy: f32,
}

impl OnsetPulse {
    pub(super) fn from_spectrogram(
        spectrogram: &[Vec<f32>],
        sample_rate: u32,
        frame_size: usize,
        hop_size: usize,
    ) -> Self {
        let hertz_per_bin = sample_rate as f32 / frame_size as f32;
        let loudness: Vec<f32> = spectrogram
            .iter()
            .map(|frame| {
                frame
                    .iter()
                    .enumerate()
                    .filter(|(bin, _)| MID_BAND_HERTZ.contains(&(*bin as f32 * hertz_per_bin)))
                    .map(|(_, magnitude)| magnitude)
                    .sum()
            })
            .collect();
        let onsets = onset_strength(&loudness);
        let energy = onsets.iter().map(|onset| onset * onset).sum();
        Self {
            frames_per_second: sample_rate as f32 / hop_size as f32,
            onsets,
            energy,
        }
    }

    /// How strongly the onsets repeat after `lag_frames`, as a share of
    /// their total energy: the best of the three whole lags around it.
    fn repeat_strength(&self, lag_frames: f32) -> f32 {
        let centre = lag_frames.round() as usize;
        if self.energy <= 0.0 || centre < 1 || centre + 1 >= self.onsets.len() {
            return 0.0;
        }
        (centre - 1..=centre + 1)
            .map(|lag| {
                let sum: f32 = self.onsets[..self.onsets.len() - lag]
                    .iter()
                    .zip(&self.onsets[lag..])
                    .map(|(first, second)| first * second)
                    .sum();
                sum / self.energy
            })
            .fold(f32::MIN, f32::max)
    }

    fn features(&self, candidate: Candidate, best_score: f32) -> [f32; FEATURE_COUNT] {
        let frames_per_beat = 60.0 / candidate.tempo * self.frames_per_second;
        let mut features = [candidate.score / best_score, 0.0, 0.0, 0.0];
        for (slot, beats) in REPEAT_BEATS.iter().enumerate() {
            features[1 + slot] = self.repeat_strength(frames_per_beat * beats);
        }
        features
    }
}

/// The rises of the log loudness above the median loudness, which mark where
/// a sound starts, with their mean removed.
fn onset_strength(loudness: &[f32]) -> Vec<f32> {
    if loudness.is_empty() {
        return Vec::new();
    }
    let mut sorted = loudness.to_vec();
    sorted.sort_by(f32::total_cmp);
    let median = sorted[sorted.len() / 2];
    let compressed: Vec<f32> = loudness
        .iter()
        .map(|value| (10.0 * value / (median + 1e-9)).ln_1p())
        .collect();
    let mut rises: Vec<f32> = std::iter::once(0.0)
        .chain(
            compressed
                .windows(2)
                .map(|pair| (pair[1] - pair[0]).max(0.0)),
        )
        .collect();
    let mean = rises.iter().sum::<f32>() / rises.len() as f32;
    for rise in &mut rises {
        *rise -= mean;
    }
    rises
}

/// The tempo among `candidates` whose score and onset pattern together
/// are strongest, or `None` without candidates.
pub(super) fn choose(pulse: &OnsetPulse, candidates: &[Candidate]) -> Option<Candidate> {
    let best_score = candidates.first()?.score;
    candidates
        .iter()
        .map(|candidate| {
            (
                *candidate,
                weighted(&pulse.features(*candidate, best_score)),
            )
        })
        .max_by(|(_, first), (_, second)| first.total_cmp(second))
        .map(|(candidate, _)| candidate)
}

fn weighted(features: &[f32; FEATURE_COUNT]) -> f32 {
    features
        .iter()
        .zip(WEIGHTS)
        .map(|(feature, weight)| feature * weight)
        .sum()
}
