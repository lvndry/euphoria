// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use rayon::prelude::*;

use crate::detect::StratumDetector;
#[cfg(feature = "libkeyfinder")]
use crate::detect::{KeyDetector, KeyFinderDetector};
use crate::error::EuphoriaError;
use crate::key::Key;

/// Identifies the detection behind a [`TrackAnalysis`]. The analysis cache
/// serves a stored result only when it was produced under the same version.
///
/// Raise the number whenever a change can alter the key, tempo or duration
/// reported for a file: a detector upgrade, a tuning change, a decoding
/// change. The suffix names the key detector, since builds with and without
/// the `libkeyfinder` feature report different keys.
pub const ANALYZER_VERSION: &str = if cfg!(feature = "libkeyfinder") {
    "1+libkeyfinder"
} else {
    "1+stratum"
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

/// Decodes one file and detects its key and tempo.
///
/// Key detection uses libkeyfinder when the `libkeyfinder` feature is enabled,
/// as it is the more accurate detector; tempo comes from `stratum-dsp`.
pub fn analyze_file(path: &Path) -> Result<TrackAnalysis, EuphoriaError> {
    let audio = crate::audio::decode_mono(path)?;
    let stratum_analysis = StratumDetector.analyze(&audio)?;

    #[cfg(feature = "libkeyfinder")]
    let key = KeyFinderDetector.detect_key(&audio)?;
    #[cfg(not(feature = "libkeyfinder"))]
    let key = stratum_analysis.key;

    Ok(TrackAnalysis {
        duration_seconds: audio.duration_seconds(),
        key,
        bpm: stratum_analysis.tempo,
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
