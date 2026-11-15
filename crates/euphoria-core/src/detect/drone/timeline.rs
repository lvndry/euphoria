// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The key over time from drone-v2's scores of short stretches of a track.

use std::ops::Range;

use super::cqt::{HOP, SAMPLE_RATE};
use super::network::{KEY_COUNT, mean_of_columns};
use crate::audio::AudioBuffer;
use crate::detect::KeySection;
use crate::detect::key_sections::{ChangeRules, keys_against_track_key, runs, window_hops};
use crate::key::Key;

/// Feature frames in each column the network scores.
const FRAMES_PER_COLUMN: usize = 8;

/// Columns in each window whose key is read, about 7.4 seconds: long
/// enough to hold a bar or two of every chord of a loop.
const COLUMNS_PER_WINDOW: usize = 5;

/// A window whose audio is this far below the loudest window of the track
/// in level, 60 dB, or that has none at all, is silence and holds no
/// evidence of a key.
const SILENT_WINDOW_LEVEL: f64 = 1.0e-3;

/// When the key over time leaves the key of the track. See
/// [`keys_against_track_key`]. The scores are log probabilities, so a
/// margin of 3.25 asks the other key to be about 26 times as likely as the
/// key of the track in every window of the section, on average.
///
/// On synthesized chord progressions that move a fifth away, the new key is
/// 3.3 to 4.8 ahead per window, and in the windows that stay in the key it
/// is 5 behind. The margin was chosen on every other GiantSteps preview in
/// name order, among margins with which those moves still show: of its
/// previews whose expert annotation has one key, 1.0% are split into more
/// than one key, against 1.7% with libkeyfinder, and none of the held out
/// half, against 0.7%.
const CHANGE_RULES: ChangeRules = ChangeRules {
    per_window_margin: 3.25,
    section_cost: 0.0,
    minimum_windows: 4,
};

/// The key of each stretch of a track, from the scores the network gave
/// each column of it, measured against `track_key`.
///
/// Silence belongs to no section, so the timeline has a hole where the
/// track is silent, and starts late or ends early when the track starts or
/// ends in silence. When every stretch with sound is in one key, each
/// section carries `track_key`, and a track without silence is then one
/// section from start to end.
pub(super) fn timeline(
    audio: &AudioBuffer,
    column_logits: &[[f32; KEY_COUNT]],
    track_key: Key,
) -> Vec<KeySection> {
    let duration = audio.duration_seconds();
    let windows = window_hops(column_logits.len(), COLUMNS_PER_WINDOW);
    let window_scores = window_scores(audio, column_logits, &windows);
    let keys = keys_against_track_key(&window_scores, class_of(track_key), CHANGE_RULES);
    let keyed_runs: Vec<(Range<usize>, usize)> = runs(&keys)
        .into_iter()
        .filter_map(|(run_windows, class)| Some((run_windows, class?)))
        .collect();
    let stays_in_one_key = keyed_runs.windows(2).all(|pair| pair[0].1 == pair[1].1);
    keyed_runs
        .into_iter()
        .map(|(run_windows, class)| KeySection {
            start_seconds: if run_windows.start == 0 {
                0.0
            } else {
                boundary_seconds(windows[run_windows.start].start).min(duration)
            },
            end_seconds: if run_windows.end == windows.len() {
                duration
            } else {
                boundary_seconds(windows[run_windows.end].start).min(duration)
            },
            key: if stays_in_one_key {
                track_key
            } else {
                super::key_of_class(class)
            },
        })
        .collect()
}

/// The log probability of every key in every window, or `None` for a
/// silent window.
fn window_scores(
    audio: &AudioBuffer,
    column_logits: &[[f32; KEY_COUNT]],
    windows: &[Range<usize>],
) -> Vec<Option<[f64; KEY_COUNT]>> {
    let levels: Vec<f64> = windows
        .iter()
        .map(|columns| {
            level_between(
                audio,
                column_seconds(columns.start),
                column_seconds(columns.end),
            )
        })
        .collect();
    let loudest = levels.iter().copied().fold(0.0, f64::max);
    windows
        .iter()
        .zip(levels)
        .map(|(columns, level)| {
            (level > 0.0 && level >= SILENT_WINDOW_LEVEL * loudest)
                .then(|| log_probabilities(&mean_of_columns(&column_logits[columns.clone()])))
        })
        .collect()
}

/// The root mean square of the samples between two times.
fn level_between(audio: &AudioBuffer, start_seconds: f64, end_seconds: f64) -> f64 {
    let sample_rate = f64::from(audio.sample_rate().get());
    let samples = audio.samples();
    let first = ((start_seconds * sample_rate) as usize).min(samples.len());
    let last = ((end_seconds * sample_rate) as usize).clamp(first, samples.len());
    let stretch = &samples[first..last];
    if stretch.is_empty() {
        return 0.0;
    }
    let power: f64 = stretch
        .iter()
        .map(|sample| f64::from(*sample) * f64::from(*sample))
        .sum::<f64>()
        / stretch.len() as f64;
    power.sqrt()
}

fn log_probabilities(logits: &[f32; KEY_COUNT]) -> [f64; KEY_COUNT] {
    let highest = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let total: f64 = logits
        .iter()
        .map(|logit| f64::from(logit - highest).exp())
        .sum();
    let offset = f64::from(highest) + total.ln();
    std::array::from_fn(|key| f64::from(logits[key]) - offset)
}

/// Where the column that starts at `column` begins: the feature frames are
/// centred on multiples of the hop, so half a hop before its first frame.
fn column_seconds(column: usize) -> f64 {
    ((column * FRAMES_PER_COLUMN) as f64 - 0.5).max(0.0) * HOP as f64 / f64::from(SAMPLE_RATE)
}

/// Where the window that starts at `column` takes over from the one before.
fn boundary_seconds(column: usize) -> f32 {
    column_seconds(column) as f32
}

fn class_of(key: Key) -> usize {
    usize::from(key.tonic()) * 2 + usize::from(key.mode() == crate::key::Mode::Minor)
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;
    use crate::detect::DroneDetector;
    use crate::detect::synthesis::{SAMPLE_RATE, buffer, chord_progression, pink_noise};

    fn key(text: &str) -> Key {
        text.parse().unwrap()
    }

    fn silence(seconds: f32) -> Vec<f32> {
        vec![0.0; (seconds * SAMPLE_RATE as f32) as usize]
    }

    fn progression(name: &str, seconds: f32) -> Vec<f32> {
        chord_progression(key(name), 0.0, seconds)
    }

    /// A kick drum on every beat at 120 BPM, with no other instrument.
    fn kick_drum(seconds: f32) -> Vec<f32> {
        let samples_per_beat = SAMPLE_RATE as usize / 2;
        let sample_count = (seconds * SAMPLE_RATE as f32) as usize;
        (0..sample_count)
            .map(|index| {
                let since_beat = (index % samples_per_beat) as f32 / SAMPLE_RATE as f32;
                let frequency = 50.0 + 100.0 * (-since_beat / 0.03).exp();
                0.8 * (TAU * frequency * since_beat).sin() * (-since_beat / 0.15).exp()
            })
            .collect()
    }

    /// Seconds in one window, the furthest a section boundary can be from
    /// where the music changes.
    fn window_seconds() -> f32 {
        column_seconds(COLUMNS_PER_WINDOW) as f32
    }

    fn timeline_of(samples: Vec<f32>) -> (Vec<KeySection>, f32) {
        let audio = buffer(samples);
        let reading = DroneDetector.read_key_over_time(&audio).unwrap();
        (reading.timeline, audio.duration_seconds())
    }

    #[test]
    fn a_progression_in_one_key_is_one_section_in_that_key() {
        let audio = buffer(progression("C major", 60.0));
        let reading = DroneDetector.read_key_over_time(&audio).unwrap();
        assert_eq!(reading.key, Some(key("C major")));
        assert_eq!(
            reading.timeline,
            [KeySection {
                start_seconds: 0.0,
                end_seconds: audio.duration_seconds(),
                key: key("C major"),
            }]
        );
    }

    #[test]
    fn a_track_that_changes_key_shows_both_keys_at_about_the_right_time() {
        let change_seconds = 48.0;
        for (first, second) in [("C major", "F# minor"), ("A minor", "E minor")] {
            let (timeline, duration) = timeline_of(
                [
                    progression(first, change_seconds),
                    progression(second, 32.0),
                ]
                .concat(),
            );
            let [before, after] = timeline[..] else {
                panic!("{first} to {second}: {timeline:?}");
            };
            assert_eq!(before.key, key(first));
            assert_eq!(after.key, key(second));
            assert_eq!(before.start_seconds, 0.0);
            assert_eq!(before.end_seconds, after.start_seconds);
            assert_eq!(after.end_seconds, duration);
            assert!(
                (after.start_seconds - change_seconds).abs() < 6.0,
                "{first} to {second}: the change is reported at {} seconds",
                after.start_seconds
            );
        }
    }

    #[test]
    fn a_few_seconds_in_another_key_do_not_split_the_timeline() {
        let (timeline, _) = timeline_of(
            [
                progression("C major", 40.0),
                progression("F# major", 4.0),
                progression("C major", 40.0),
            ]
            .concat(),
        );
        assert_eq!(timeline.len(), 1, "{timeline:?}");
        assert_eq!(timeline[0].key, key("C major"));
    }

    #[test]
    fn a_move_a_fifth_away_in_the_middle_of_a_track_shows() {
        let (timeline, _) = timeline_of(
            [
                progression("C major", 40.0),
                progression("G major", 36.0),
                progression("C major", 40.0),
            ]
            .concat(),
        );
        let keys: Vec<Key> = timeline.iter().map(|section| section.key).collect();
        assert_eq!(
            keys,
            [key("C major"), key("G major"), key("C major")],
            "{timeline:?}"
        );
        let middle = timeline[1];
        assert!((middle.start_seconds - 40.0).abs() < 8.0, "{middle:?}");
        assert!((middle.end_seconds - 76.0).abs() < 8.0, "{middle:?}");
    }

    #[test]
    fn silence_between_two_keys_is_a_hole_in_the_timeline() {
        let (timeline, duration) = timeline_of(
            [
                progression("C major", 40.0),
                silence(30.0),
                progression("F# minor", 40.0),
            ]
            .concat(),
        );
        let window = window_seconds();
        let [before, after] = timeline[..] else {
            panic!("{timeline:?}");
        };
        assert_eq!((before.key, after.key), (key("C major"), key("F# minor")));
        assert_eq!(before.start_seconds, 0.0);
        assert_eq!(after.end_seconds, duration);
        assert!((40.0 - window..=40.0 + window).contains(&before.end_seconds));
        assert!((70.0 - window..=70.0 + window).contains(&after.start_seconds));
        assert!(after.start_seconds - before.end_seconds > 30.0 - 2.0 * window);
    }

    #[test]
    fn silence_inside_a_track_in_one_key_splits_it_into_two_sections_in_that_key() {
        let (timeline, _) = timeline_of(
            [
                progression("C major", 40.0),
                silence(30.0),
                progression("C major", 40.0),
            ]
            .concat(),
        );
        let [before, after] = timeline[..] else {
            panic!("{timeline:?}");
        };
        assert_eq!((before.key, after.key), (key("C major"), key("C major")));
        assert!(before.end_seconds < 55.0 && after.start_seconds > 55.0);
    }

    #[test]
    fn a_track_that_starts_or_ends_in_silence_has_no_section_there() {
        let window = window_seconds();
        let (timeline, duration) =
            timeline_of([silence(20.0), progression("C major", 60.0)].concat());
        let [section] = timeline[..] else {
            panic!("{timeline:?}");
        };
        assert_eq!(section.key, key("C major"));
        assert!((20.0 - window..=20.0 + window).contains(&section.start_seconds));
        assert_eq!(section.end_seconds, duration);

        let (timeline, _) = timeline_of([progression("C major", 60.0), silence(20.0)].concat());
        let [section] = timeline[..] else {
            panic!("{timeline:?}");
        };
        assert_eq!(section.start_seconds, 0.0);
        assert!((60.0 - window..=60.0 + window).contains(&section.end_seconds));
    }

    #[test]
    fn a_passage_far_quieter_than_the_track_counts_as_silence() {
        let whisper: Vec<f32> = progression("F# minor", 30.0)
            .iter()
            .map(|sample| sample * 3.0e-4)
            .collect();
        let (timeline, _) = timeline_of(
            [
                progression("C major", 40.0),
                whisper,
                progression("C major", 40.0),
            ]
            .concat(),
        );
        assert_eq!(timeline.len(), 2, "{timeline:?}");
        assert!(timeline.iter().all(|section| section.key == key("C major")));
    }

    #[test]
    fn a_passage_with_sound_but_no_harmony_keeps_the_key_around_it() {
        for (label, passage) in [
            ("kick drum", kick_drum(30.0)),
            ("noise", pink_noise(30 * SAMPLE_RATE as usize, 0.3)),
        ] {
            let samples = [
                progression("C major", 40.0),
                passage,
                progression("C major", 40.0),
            ]
            .concat();
            let (timeline, duration) = timeline_of(samples);
            assert_eq!(
                timeline,
                [KeySection {
                    start_seconds: 0.0,
                    end_seconds: duration,
                    key: key("C major"),
                }],
                "{label}"
            );
        }
    }

    #[test]
    fn a_change_of_key_across_a_drum_break_falls_within_the_break() {
        let (timeline, _) = timeline_of(
            [
                progression("C major", 40.0),
                kick_drum(30.0),
                progression("F# minor", 40.0),
            ]
            .concat(),
        );
        let window = window_seconds();
        let [before, after] = timeline[..] else {
            panic!("{timeline:?}");
        };
        assert_eq!((before.key, after.key), (key("C major"), key("F# minor")));
        assert_eq!(before.end_seconds, after.start_seconds);
        assert!((40.0 - window..=70.0 + window).contains(&after.start_seconds));
    }

    #[test]
    fn silent_audio_has_no_timeline() {
        let audio = buffer(silence(4.0));
        assert_eq!(
            DroneDetector.read_key_over_time(&audio).unwrap(),
            crate::detect::KeyReading::default()
        );
    }
}
