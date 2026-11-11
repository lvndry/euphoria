// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Made up tracks for trying the interface on a large library without any
//! audio files. Nothing here is analysis: the keys and tempos are invented.

use std::path::PathBuf;

use euphoria_core::{Key, Mode, TrackAnalysis};

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
            let analysis = TrackAnalysis {
                duration_seconds: 240.0 + next(300) as f32,
                key: (next(40) != 0).then(|| Key::new(next(12) as u8, mode)),
                bpm: (next(40) != 0).then(|| 112.0 + next(200) as f32 / 10.0),
                energy: (next(30) != 0).then(|| next(10) as u8 + 1),
                tuning_cents: None,
                key_runner_up: None,
                key_score_margin: None,
                tempo_estimate: None,
                tempo_score_margin: None,
                key_timeline: Vec::new(),
            };
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
