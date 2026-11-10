// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs detectors over dataset tracks and collects the results.
//!
//! Each track is decoded, run through every detector and dropped before the
//! worker takes the next one, so memory stays bounded by the number of worker
//! threads.

use std::time::Duration;

use cpu_time::ThreadTime;
use euphoria_core::detect::{KeyDetector, TempoDetector};
use euphoria_core::{AudioBuffer, EuphoriaError, Key, decode_mono};
use rayon::prelude::*;

use crate::dataset::Track;
use crate::scoring::{KeyTally, TempoTally};

/// Results for the benchmarks that were run.
#[derive(Debug)]
pub struct Report {
    /// Key detection on GiantSteps Key, when it was run.
    pub key: Option<Evaluation<KeyTally>>,
    /// Tempo detection on GiantSteps Tempo, when it was run.
    pub tempo: Option<Evaluation<TempoTally>>,
}

/// Results of running a set of detectors over one dataset.
#[derive(Debug)]
pub struct Evaluation<Scores> {
    /// Tracks that had audio, whether or not they decoded.
    pub track_count: usize,
    /// Tracks whose audio could not be decoded, in track order.
    pub decode_failures: Vec<Failure>,
    /// One entry per detector, in the order the detectors were given.
    pub detectors: Vec<DetectorResult<Scores>>,
}

/// One detector's results over every decoded track.
#[derive(Debug)]
pub struct DetectorResult<Scores> {
    /// The detector's report name.
    pub name: &'static str,
    /// How its estimates compare with the annotations.
    pub tally: Scores,
    /// CPU time spent inside the detector, summed over tracks.
    pub total_cpu_time: Duration,
    /// Number of tracks the detector was run on.
    pub measured_tracks: usize,
    /// Tracks where the detector returned an error, in track order.
    pub failures: Vec<Failure>,
}

impl<Scores> DetectorResult<Scores> {
    /// Average CPU seconds per measured track, or `None` when no track was
    /// measured.
    pub fn cpu_seconds_per_track(&self) -> Option<f64> {
        (self.measured_tracks > 0)
            .then(|| self.total_cpu_time.as_secs_f64() / self.measured_tracks as f64)
    }
}

/// A track that could not be decoded or analysed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// Name of the track.
    pub track: String,
    /// The full error chain.
    pub reason: String,
}

/// A score that accumulates one detector's estimates against references.
pub trait Tally: Default {
    /// The annotated value for a track.
    type Reference: Sync;
    /// What the detector returns for a track.
    type Estimate: Send;

    /// Scores one estimate against its reference.
    fn record_estimate(&mut self, reference: &Self::Reference, estimate: Self::Estimate);

    /// Counts a track with no estimate, either because the detector found
    /// none or because it failed.
    fn record_no_estimate(&mut self);
}

impl Tally for KeyTally {
    type Reference = Key;
    type Estimate = Key;

    fn record_estimate(&mut self, reference: &Key, estimate: Key) {
        self.record(*reference, estimate);
    }

    fn record_no_estimate(&mut self) {
        self.record_missing();
    }
}

impl Tally for TempoTally {
    type Reference = f32;
    type Estimate = f32;

    fn record_estimate(&mut self, reference: &f32, estimate: f32) {
        self.record(*reference, estimate);
    }

    fn record_no_estimate(&mut self) {
        self.record_missing();
    }
}

/// Runs every key detector over every track.
pub fn evaluate_keys(
    tracks: &[Track<Key>],
    detectors: &[&dyn KeyDetector],
) -> Evaluation<KeyTally> {
    let names = detectors.iter().map(|detector| detector.name()).collect();
    let outcomes = measure_tracks(tracks, detectors.len(), |detector_index, audio| {
        detectors[detector_index].detect_key(audio)
    });
    tally_outcomes(tracks, names, outcomes)
}

/// Runs every tempo detector over every track.
pub fn evaluate_tempo(
    tracks: &[Track<f32>],
    detectors: &[&dyn TempoDetector],
) -> Evaluation<TempoTally> {
    let names = detectors.iter().map(|detector| detector.name()).collect();
    let outcomes = measure_tracks(tracks, detectors.len(), |detector_index, audio| {
        detectors[detector_index].detect_tempo(audio)
    });
    tally_outcomes(tracks, names, outcomes)
}

/// One detector's answer for one track, with the CPU time the call used on
/// its own thread. Errors are already flattened to their full chain.
#[derive(Debug)]
struct Measurement<Estimate> {
    estimate: Result<Option<Estimate>, String>,
    cpu_time: Duration,
}

/// Every detector's measurement for one track in detector order, or the
/// decode error chain when the track could not be decoded.
type TrackOutcome<Estimate> = Result<Vec<Measurement<Estimate>>, String>;

fn measure<Estimate>(
    detect: impl FnOnce() -> Result<Option<Estimate>, EuphoriaError>,
) -> Measurement<Estimate> {
    let started = ThreadTime::now();
    let estimate = detect();
    let cpu_time = started.elapsed();
    Measurement {
        estimate: estimate.map_err(|error| error.chain()),
        cpu_time,
    }
}

/// Decodes each track and runs every detector on it, in parallel across
/// tracks. Returns one outcome per track, in track order.
fn measure_tracks<Annotation: Sync, Estimate: Send>(
    tracks: &[Track<Annotation>],
    detector_count: usize,
    detect: impl Fn(usize, &AudioBuffer) -> Result<Option<Estimate>, EuphoriaError> + Sync,
) -> Vec<TrackOutcome<Estimate>> {
    tracks
        .par_iter()
        .map(|track| {
            let audio = decode_mono(&track.audio_path).map_err(|error| error.chain())?;
            Ok((0..detector_count)
                .map(|detector_index| measure(|| detect(detector_index, &audio)))
                .collect())
        })
        .collect()
}

/// Folds per-track outcomes, given in the same order as `tracks`, into one
/// result per detector.
fn tally_outcomes<Scores: Tally>(
    tracks: &[Track<Scores::Reference>],
    detector_names: Vec<&'static str>,
    outcomes: Vec<TrackOutcome<Scores::Estimate>>,
) -> Evaluation<Scores> {
    let mut detectors: Vec<DetectorResult<Scores>> = detector_names
        .into_iter()
        .map(|name| DetectorResult {
            name,
            tally: Scores::default(),
            total_cpu_time: Duration::ZERO,
            measured_tracks: 0,
            failures: Vec::new(),
        })
        .collect();
    let mut decode_failures = Vec::new();

    for (track, outcome) in tracks.iter().zip(outcomes) {
        let measurements = match outcome {
            Ok(measurements) => measurements,
            Err(reason) => {
                decode_failures.push(Failure {
                    track: track.name.clone(),
                    reason,
                });
                continue;
            }
        };
        for (result, measurement) in detectors.iter_mut().zip(measurements) {
            result.measured_tracks += 1;
            result.total_cpu_time += measurement.cpu_time;
            match measurement.estimate {
                Ok(Some(estimate)) => result.tally.record_estimate(&track.annotation, estimate),
                Ok(None) => result.tally.record_no_estimate(),
                Err(reason) => {
                    result.tally.record_no_estimate();
                    result.failures.push(Failure {
                        track: track.name.clone(),
                        reason,
                    });
                }
            }
        }
    }

    Evaluation {
        track_count: tracks.len(),
        decode_failures,
        detectors,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn track(name: &str, tempo: f32) -> Track<f32> {
        Track {
            name: name.to_string(),
            audio_path: PathBuf::from(format!("{name}.mp3")),
            annotation: tempo,
        }
    }

    fn measurement(estimate: Result<Option<f32>, &str>) -> Measurement<f32> {
        Measurement {
            estimate: estimate.map_err(str::to_string),
            cpu_time: Duration::from_millis(500),
        }
    }

    #[test]
    fn tallies_estimates_failures_and_decode_errors_per_detector() {
        let tracks = [
            track("one", 120.0),
            track("two", 128.0),
            track("three", 90.0),
        ];
        let outcomes = vec![
            Ok(vec![measurement(Ok(Some(120.0)))]),
            Err("cannot decode two.mp3: malformed audio data".to_string()),
            Ok(vec![measurement(Err(
                "stratum-dsp failed: analysis failed",
            ))]),
        ];

        let evaluation: Evaluation<TempoTally> =
            tally_outcomes(&tracks, vec!["stratum-dsp"], outcomes);

        assert_eq!(evaluation.track_count, 3);
        assert_eq!(
            evaluation.decode_failures,
            [Failure {
                track: "two".to_string(),
                reason: "cannot decode two.mp3: malformed audio data".to_string(),
            }]
        );
        let detector = &evaluation.detectors[0];
        assert_eq!(detector.measured_tracks, 2);
        assert_eq!(detector.total_cpu_time, Duration::from_secs(1));
        assert_eq!(detector.cpu_seconds_per_track(), Some(0.5));
        assert_eq!(detector.tally.accurate, 1);
        assert_eq!(detector.tally.missing, 1);
        assert_eq!(detector.failures[0].track, "three");
    }
}
