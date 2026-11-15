// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Made up tracks for trying the interface on a large library without any
//! audio files. Nothing here is analysis: the keys and tempos are invented.

use std::path::PathBuf;

use euphoria_core::quality::{QualityReport, QualityVerdict};
use euphoria_core::{DecodeDamage, Key, Mode, TrackAnalysis};

/// Environment variable holding how many made up tracks to open with.
pub(crate) const DEMO_TRACKS_VARIABLE: &str = "EUPHORIA_DEMO_TRACKS";

const FIRST_WORDS: [&str; 12] = [
    "Low", "Paper", "Half", "Second", "Slow", "Cobalt", "Salt", "First", "Glass", "Ember", "Night",
    "Silver",
];
const SECOND_WORDS: [&str; 12] = [
    "Tide", "Lanterns", "Light", "Sun", "Orbit", "Hour", "Lines", "Signal", "House", "Room",
    "Drive", "Coast",
];
const FOLDERS: [&str; 5] = [
    "Afro House",
    "Deep House",
    "Melodic Techno",
    "Downtempo",
    "Promos",
];

/// The made up folder the made up tracks are in.
pub(crate) const DEMO_FOLDER: &str = "/Demo";

const EXTENSIONS: [&str; 4] = ["flac", "mp3", "aiff", "m4a"];

/// One made up track: where it would be on disk and what analysis would
/// have said about it.
pub(crate) struct DemoTrack {
    /// A path that does not exist.
    pub(crate) path: PathBuf,
    /// An invented result, or an invented reason for failing.
    pub(crate) outcome: Result<TrackAnalysis, String>,
}

/// How many made up tracks the environment asks for, if any.
pub(crate) fn requested_track_count() -> Option<usize> {
    std::env::var(DEMO_TRACKS_VARIABLE)
        .ok()?
        .parse()
        .ok()
        .filter(|count| *count > 0)
}

/// `count` made up tracks, the same ones every time.
pub(crate) fn tracks(count: usize) -> Vec<DemoTrack> {
    let mut state: u32 = 7;
    let mut next = move |bound: u32| {
        state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345) % 2_147_483_648;
        (state / 65_536) % bound
    };
    (0..count)
        .map(|number| {
            let first = FIRST_WORDS[next(12) as usize];
            let second = SECOND_WORDS[next(12) as usize];
            let folder = FOLDERS[next(5) as usize];
            let extension = EXTENSIONS[next(4) as usize];
            let path = PathBuf::from(format!(
                "/Demo/{folder}/{first} {second} {:05}.{extension}",
                number + 1
            ));
            let mode = if next(3) == 0 {
                Mode::Major
            } else {
                Mode::Minor
            };
            let mut analysis = TrackAnalysis {
                duration_seconds: 240.0 + next(300) as f32,
                key: (next(40) != 0).then(|| Key::new(next(12) as u8, mode)),
                bpm: (next(40) != 0).then(|| 112.0 + next(200) as f32 / 10.0),
                energy: (next(30) != 0).then(|| next(10) as u8 + 1),
                tuning_cents: None,
                key_runner_up: None,
                key_score_margin: None,
                key_source: None,
                loudness_range_lu: None,
                pitch_class_profile: None,
                tempo_estimate: None,
                tempo_score_margin: None,
                beat_grid: None,
                cues: Vec::new(),
                key_timeline: Vec::new(),
                quality: Some(demo_quality(extension, number)),
            };
            analysis.pitch_class_profile = analysis.key.map(|key| demo_profile(key, number));
            let outcome = if next(150) == 0 {
                Err(format!(
                    "cannot decode {}: malformed audio data",
                    path.display()
                ))
            } else {
                Ok(analysis)
            };
            DemoTrack { path, outcome }
        })
        .collect()
}

/// Invented notes for a track in `key`: the tonic loudest, the rest of the
/// scale loud and the other notes quiet, varied by track number.
fn demo_profile(key: Key, number: usize) -> [f32; 12] {
    let in_key = euphoria_core::notes_in_key(key);
    std::array::from_fn(|pitch_class| {
        let variation = ((number * 7 + pitch_class * 13) % 30) as f32 / 100.0;
        if pitch_class == usize::from(key.tonic()) {
            1.0
        } else if in_key[pitch_class] {
            0.55 + variation
        } else {
            0.12 + variation / 2.0
        }
    })
}

/// An invented quality report: lossless formats are lossless, and the rest
/// cycle through the lossy verdicts by track number, with an occasional
/// damaged file.
fn demo_quality(extension: &str, number: usize) -> QualityReport {
    let is_lossless = matches!(extension, "flac" | "aiff");
    let verdict = if is_lossless {
        QualityVerdict::Lossless
    } else {
        [
            QualityVerdict::Good,
            QualityVerdict::Good,
            QualityVerdict::Acceptable,
            QualityVerdict::WeakOnABigSystem,
            QualityVerdict::LikelyReencoded,
        ][number % 5]
    };
    QualityReport {
        codec: extension.to_string(),
        lossless: is_lossless,
        bitrate_kbps: (!is_lossless).then_some(320),
        sample_rate: 44_100,
        bit_depth: is_lossless.then_some(16),
        channels: Some(2),
        cutoff_hz: None,
        clipped_share: 0.0,
        integrated_loudness_lufs: None,
        true_peak_dbtp: None,
        verdict,
        damage: DecodeDamage {
            dropped_packets: u32::from(number.is_multiple_of(37)),
            ..DecodeDamage::default()
        },
        cancelled_share: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_the_requested_number_of_distinct_tracks() {
        let made = tracks(10_000);
        assert_eq!(made.len(), 10_000);
        let mut paths: Vec<&PathBuf> = made.iter().map(|track| &track.path).collect();
        paths.sort();
        paths.dedup();
        assert_eq!(paths.len(), 10_000);
    }

    #[test]
    fn the_tracks_are_the_same_every_time() {
        let first: Vec<PathBuf> = tracks(50).into_iter().map(|track| track.path).collect();
        let second: Vec<PathBuf> = tracks(50).into_iter().map(|track| track.path).collect();
        assert_eq!(first, second);
    }

    #[test]
    fn most_tracks_are_analyzed_and_a_few_fail() {
        let made = tracks(10_000);
        let failed = made.iter().filter(|track| track.outcome.is_err()).count();
        assert!(failed > 0 && failed < 500, "{failed}");
    }
}
