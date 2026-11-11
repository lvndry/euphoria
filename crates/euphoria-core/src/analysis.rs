// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;

#[cfg(feature = "libkeyfinder")]
use crate::detect::{KeyFinderDetector, TempoDetector};
use crate::detect::{KeyReading, KeySection, StratumDetector};
use crate::energy::rate_energy;
use crate::error::EuphoriaError;
use crate::key::Key;

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
    "5+libkeyfinder"
} else {
    "5+stratum"
};

/// What Euphoria found in one track.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackAnalysis {
    /// Length of the track in seconds.
    pub duration_seconds: f32,
    /// The detected key, or `None` when the track has no detectable key.
    pub key: Option<Key>,
    /// The detected tempo in beats per minute, or `None` when the track has
    /// no detectable tempo.
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

/// Decodes one file once and finds its key, its tempo and its energy.
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
    let audio = crate::audio::decode_mono(path)?;

    // Tempo runs first because it has the larger peak. libkeyfinder then
    // reuses the memory tempo released, where the other order keeps
    // libkeyfinder's released memory resident under the tempo peak.
    #[cfg(feature = "libkeyfinder")]
    let (tempo, reading) = (
        StratumDetector.detect_tempo(&audio)?,
        KeyFinderDetector.read_key(&audio)?,
    );
    #[cfg(not(feature = "libkeyfinder"))]
    let (reading, tempo) = {
        let analysis = StratumDetector.analyze(&audio)?;
        let reading = KeyReading {
            key: analysis.key,
            tuning_cents: crate::detect::measure_tuning(&audio).map(|tuning| tuning.cents),
            ..KeyReading::default()
        };
        (reading, analysis.tempo)
    };
    let KeyReading {
        key,
        tuning_cents,
        timeline,
        ..
    } = reading;

    let energy = rate_energy(&audio, tempo)?.map(|rating| rating.rating);

    Ok(TrackAnalysis {
        duration_seconds: audio.duration_seconds(),
        key,
        bpm: tempo,
        energy,
        tuning_cents,
        key_timeline: timeline,
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
