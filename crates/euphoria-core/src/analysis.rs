// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;

use crate::confidence::{Confidence, key_confidence, tempo_confidence};
#[cfg(not(feature = "libkeyfinder"))]
use crate::detect::KeyDetector;
#[cfg(feature = "libkeyfinder")]
use crate::detect::KeyFinderDetector;
use crate::detect::{KeyReading, KeySection, StratumDetector};
use crate::energy::rate_energy;
use crate::error::EuphoriaError;
use crate::key::Key;
use crate::quality::{QualityReport, decode_measuring_quality};
use crate::tempo_range::TempoRange;

/// Identifies the detection behind a [`TrackAnalysis`]. The analysis cache
/// serves a stored result only when it was produced under the same version.
///
/// Raise the number whenever a change can alter the key, tempo or duration
/// reported for a file, or adds something to what is reported: a detector
/// upgrade, a tuning change, a decoding change. The suffix names the key
/// detector, since builds with and without the `libkeyfinder` feature report
/// different keys.
///
/// The energy rating has a version of its own,
/// [`ENERGY_MODEL_VERSION`](crate::ENERGY_MODEL_VERSION), which the cache
/// checks as well, so recalibrating energy needs no change here.
pub const ANALYZER_VERSION: &str = if cfg!(feature = "libkeyfinder") {
    "8+libkeyfinder"
} else {
    "8+stratum"
};

/// What Euphoria found in one track.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackAnalysis {
    /// Length of the track in seconds.
    pub duration_seconds: f32,
    /// The detected key, or `None` when the track has no detectable key.
    pub key: Option<Key>,
    /// The detected tempo in beats per minute, or `None` when the track has
    /// no detectable tempo. An analysis reports it in [`TempoRange::DEFAULT`],
    /// and [`TrackAnalysis::with_tempo_range`] in another range. The energy
    /// rating always uses the tempo in the default range.
    pub bpm: Option<f32>,
    /// The energy rating from 1 (calm) to 10 (intense), or `None` when there
    /// is nothing to rate, such as silence. It is Euphoria's own scale, set
    /// by [`ENERGY_MODEL_VERSION`](crate::ENERGY_MODEL_VERSION).
    pub energy: Option<u8>,
    /// How far the track is tuned from A = 440 Hz, in cents from -50 to 50
    /// with sharp as positive, or `None` when it could not be measured, as
    /// for silence, noise and tracks that are mostly drums. With the
    /// `libkeyfinder` feature, the key of a track at least 20 cents off is
    /// detected after bringing it back to concert pitch, so `key` names the
    /// nearest key at 440 Hz.
    pub tuning_cents: Option<f32>,
    /// The key over time: sections in order that together cover the track,
    /// except where it is silent. A track in which no change of key was
    /// found has one section in the key of the track, or one on either side
    /// of each silence. Empty when the track has no detectable key, and in
    /// builds without the `libkeyfinder` feature. It is reported only: `key`
    /// is decided from the whole track at once.
    pub key_timeline: Vec<KeySection>,
    /// How the file is encoded and how good it sounds for DJ use, measured
    /// while it was decoded for the other analyses. `None` only for an
    /// analysis made without it.
    pub quality: Option<QualityReport>,
    /// The key that matched the track second best, or `None` when there is
    /// no key or the key detector does not rank keys, as in builds without
    /// the `libkeyfinder` feature.
    pub key_runner_up: Option<Key>,
    /// How far the score of the key is above that of the runner up, the
    /// basis of [`TrackAnalysis::key_confidence`].
    pub key_score_margin: Option<f32>,
    /// The tempo as the detector estimated it, before it is reported in a
    /// range, so that the range can change without analyzing again.
    pub tempo_estimate: Option<f32>,
    /// How far the tempogram scored the tempo above the best tempo that is
    /// not an octave of it, the basis of [`TrackAnalysis::tempo_confidence`].
    /// `None` for a tempo from the fallback estimate, which is rated low.
    pub tempo_score_margin: Option<f32>,
}

impl TrackAnalysis {
    /// How far to trust the key, or `None` when there is no key or no
    /// margin to rate it by.
    pub fn key_confidence(&self) -> Option<Confidence> {
        self.key.and(self.key_score_margin).map(key_confidence)
    }

    /// How far to trust the beat period of the tempo, whatever octave it is
    /// reported in, or `None` when there is no tempo.
    pub fn tempo_confidence(&self) -> Option<Confidence> {
        self.tempo_estimate
            .map(|_| tempo_confidence(self.tempo_score_margin))
    }

    /// The tempo reported in `range`. Without a stored estimate, as in an
    /// analysis built by hand, it is [`TrackAnalysis::bpm`] placed in the
    /// range. `None` when there is no tempo, or when it is not a positive
    /// number and so has no place in a range.
    pub fn tempo_in(&self, range: TempoRange) -> Option<f32> {
        self.tempo_estimate
            .or(self.bpm)
            .filter(|tempo| tempo.is_finite() && *tempo > 0.0)
            .map(|tempo| range.fold(tempo))
    }

    /// The same analysis with [`TrackAnalysis::bpm`] reported in `range`.
    /// Nothing is analyzed again: the tempo comes from the stored estimate.
    pub fn with_tempo_range(mut self, range: TempoRange) -> Self {
        self.bpm = self.tempo_in(range);
        self
    }

    /// Half or double the tempo reported in `range`, when a DJ could
    /// plausibly count it that way. See [`TempoRange::alternative`].
    pub fn tempo_alternative_in(&self, range: TempoRange) -> Option<f32> {
        self.tempo_in(range)
            .and_then(|tempo| range.alternative(tempo))
    }

    /// Whether the key or the tempo is rated low, so the track is worth
    /// checking by ear.
    pub fn needs_review(&self) -> bool {
        self.key_confidence() == Some(Confidence::Low)
            || self.tempo_confidence() == Some(Confidence::Low)
    }
}

/// The result of analyzing one file.
#[derive(Debug)]
pub struct FileResult {
    /// The file that was analyzed.
    pub path: PathBuf,
    /// The analysis, or why it failed.
    pub outcome: Result<TrackAnalysis, EuphoriaError>,
}

/// Progress through a batch, reported after each file finishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress<'batch> {
    /// Files finished so far, including this one.
    pub completed: usize,
    /// Files in the batch.
    pub total: usize,
    /// The file that just finished.
    pub path: &'batch Path,
}

/// Decodes one file once and finds its key, its tempo, its energy and its
/// audio quality.
///
/// Key detection uses libkeyfinder when the `libkeyfinder` feature is enabled,
/// as it is the more accurate detector, and tempo comes from the tempo only
/// path of `stratum-dsp`, so its costly key analysis does not run. Without
/// the feature, `stratum-dsp` finds both, the tuning is measured without
/// being corrected for, and there is no key over time. The energy rating
/// takes the detected tempo into account when there is one.
///
/// A file whose sample rate is outside the range the energy rating supports
/// fails with [`EuphoriaError::UnsupportedSampleRate`].
pub fn analyze_file(path: &Path) -> Result<TrackAnalysis, EuphoriaError> {
    let (audio, pending_quality) = decode_measuring_quality(path)?;

    // Tempo runs first because it has the larger peak. libkeyfinder then
    // reuses the memory tempo released, where the other order keeps
    // libkeyfinder's released memory resident under the tempo peak.
    let tempo_reading = StratumDetector.read_tempo(&audio)?;
    #[cfg(feature = "libkeyfinder")]
    let reading = KeyFinderDetector.read_key(&audio)?;
    #[cfg(not(feature = "libkeyfinder"))]
    let reading = KeyReading {
        key: StratumDetector.detect_key(&audio)?,
        tuning_cents: crate::detect::measure_tuning(&audio).map(|tuning| tuning.cents),
        ..KeyReading::default()
    };
    let KeyReading {
        key,
        runner_up,
        score_margin,
        tuning_cents,
        timeline,
    } = reading;

    // The energy rating was set with tempos in the default range, so it uses
    // that tempo whatever range the tempo is reported in.
    let tempo = tempo_reading.map(|reading| reading.tempo_in(TempoRange::DEFAULT));
    let energy_rating = rate_energy(&audio, tempo)?;
    let energy = energy_rating.map(|rating| rating.rating);
    let loudness = energy_rating.map(|rating| rating.features.integrated_loudness_lufs);
    let quality = pending_quality.finish(&audio, loudness);

    Ok(TrackAnalysis {
        duration_seconds: audio.duration_seconds(),
        key,
        bpm: tempo,
        energy,
        tuning_cents,
        key_timeline: timeline,
        quality: Some(quality),
        key_runner_up: runner_up,
        key_score_margin: score_margin,
        tempo_estimate: tempo_reading.map(|reading| reading.estimate),
        tempo_score_margin: tempo_reading.and_then(|reading| reading.score_margin),
    })
}

/// Analyzes every file in parallel and returns one result per file, in the
/// order given. A file that fails does not stop the others.
///
/// `on_progress` is called from worker threads once per finished file. The
/// batch runs on the current Rayon thread pool, so callers control the number
/// of parallel workers with [`rayon::ThreadPool::install`].
pub fn analyze_files(
    paths: &[PathBuf],
    on_progress: impl Fn(Progress<'_>) + Sync,
) -> Vec<FileResult> {
    let completed = AtomicUsize::new(0);
    paths
        .par_iter()
        .map(|path| {
            let outcome = analyze_file(path);
            on_progress(Progress {
                completed: completed.fetch_add(1, Ordering::Relaxed) + 1,
                total: paths.len(),
                path,
            });
            FileResult {
                path: path.clone(),
                outcome,
            }
        })
        .collect()
}
