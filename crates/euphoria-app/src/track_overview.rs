// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The energy curve and the key timeline of a track, as stretches of the
//! track from its start (0) to its end (1), so that they line up with the
//! waveform and the playhead drawn above them.

use euphoria_core::{EnergyCurvePoint, Key, KeySection};

/// A stretch of the energy curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct EnergyStep {
    /// Where the stretch starts, as a fraction of the track.
    pub(crate) start: f32,
    /// Where it ends, as a fraction of the track.
    pub(crate) end: f32,
    /// The energy rating from 1 to 10, or `None` where the track is silent.
    pub(crate) rating: Option<u8>,
}

/// The steps of an energy curve over a track `duration_seconds` long.
///
/// The core rates overlapping windows, 8 seconds long every 4 seconds. Each
/// rating is drawn around the middle of its window, from halfway after the
/// middle of the window before to halfway before the middle of the window
/// after, so a breakdown shows where it is heard. The first step starts at
/// the start of the track and the last ends at the end of its window, so
/// the steps cover the track without gaps or overlaps.
pub(crate) fn energy_steps(points: &[EnergyCurvePoint], duration_seconds: f32) -> Vec<EnergyStep> {
    if duration_seconds <= 0.0 || !duration_seconds.is_finite() {
        return Vec::new();
    }
    let middles: Vec<f32> = points
        .iter()
        .map(|point| point.start_seconds + point.duration_seconds / 2.0)
        .collect();
    let fraction = |seconds: f32| (seconds / duration_seconds).clamp(0.0, 1.0);
    points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let start = match index {
                0 => 0.0,
                _ => (middles[index - 1] + middles[index]) / 2.0,
            };
            let end = match middles.get(index + 1) {
                Some(next) => (middles[index] + next) / 2.0,
                None => point.start_seconds + point.duration_seconds,
            };
            EnergyStep {
                start: fraction(start),
                end: fraction(end),
                rating: point.rating.as_ref().map(|rating| rating.rating),
            }
        })
        .filter(|step| step.end > step.start)
        .collect()
}

/// A stretch of the track in one key.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct KeyStretch {
    /// Where the stretch starts, as a fraction of the track.
    pub(crate) start: f32,
    /// Where it ends, as a fraction of the track.
    pub(crate) end: f32,
    /// The key of the stretch.
    pub(crate) key: Key,
}

/// The key timeline of a track `duration_seconds` long, or nothing when the
/// track stays in one key, since the key of the whole track already says
/// that. Silent stretches between sections stay gaps.
pub(crate) fn key_stretches(sections: &[KeySection], duration_seconds: f32) -> Vec<KeyStretch> {
    if sections.len() < 2 || duration_seconds <= 0.0 || !duration_seconds.is_finite() {
        return Vec::new();
    }
    let fraction = |seconds: f32| (seconds / duration_seconds).clamp(0.0, 1.0);
    sections
        .iter()
        .map(|section| KeyStretch {
            start: fraction(section.start_seconds),
            end: fraction(section.end_seconds),
            key: section.key,
        })
        .filter(|stretch| stretch.end > stretch.start)
        .collect()
}

#[cfg(test)]
mod tests {
    use euphoria_core::Mode;

    use super::*;

    fn point(start_seconds: f32, duration_seconds: f32, rating: Option<u8>) -> EnergyCurvePoint {
        EnergyCurvePoint {
            start_seconds,
            duration_seconds,
            rating: rating.map(|rating| euphoria_core::EnergyRating {
                rating,
                score: f32::from(rating) / 10.0,
                features: euphoria_core::EnergyFeatures {
                    integrated_loudness_lufs: -9.0,
                    loudness_range_lu: 5.0,
                    onset_rate: 2.0,
                    low_frequency_share: 0.3,
                    spectral_flux: 0.1,
                },
                tempo: None,
                model_version: 0,
            }),
        }
    }

    #[test]
    fn each_rating_is_drawn_around_the_middle_of_its_window_and_the_steps_cover_the_track() {
        let points = [
            point(0.0, 8.0, Some(9)),
            point(4.0, 8.0, Some(2)),
            point(8.0, 8.0, Some(10)),
        ];
        let steps = energy_steps(&points, 16.0);
        assert_eq!(
            steps,
            [
                EnergyStep {
                    start: 0.0,
                    end: 6.0 / 16.0,
                    rating: Some(9)
                },
                EnergyStep {
                    start: 6.0 / 16.0,
                    end: 10.0 / 16.0,
                    rating: Some(2)
                },
                EnergyStep {
                    start: 10.0 / 16.0,
                    end: 1.0,
                    rating: Some(10)
                },
            ]
        );
    }

    #[test]
    fn a_silent_window_is_a_step_without_a_rating() {
        let steps = energy_steps(&[point(0.0, 8.0, None), point(4.0, 8.0, Some(5))], 12.0);
        assert_eq!(steps[0].rating, None);
        assert_eq!(steps[1].rating, Some(5));
    }

    #[test]
    fn the_partial_hop_at_the_end_is_left_out_as_the_core_leaves_it_out() {
        let steps = energy_steps(&[point(0.0, 8.0, Some(5)), point(4.0, 8.0, Some(6))], 14.0);
        assert_eq!(steps.last().unwrap().end, 12.0 / 14.0);
    }

    #[test]
    fn a_track_shorter_than_a_window_is_one_step_over_all_of_it() {
        assert_eq!(
            energy_steps(&[point(0.0, 5.0, Some(3))], 5.0),
            [EnergyStep {
                start: 0.0,
                end: 1.0,
                rating: Some(3)
            }]
        );
        assert!(energy_steps(&[], 5.0).is_empty());
        assert!(energy_steps(&[point(0.0, 5.0, Some(3))], 0.0).is_empty());
    }

    fn section(start_seconds: f32, end_seconds: f32, tonic: u8) -> KeySection {
        KeySection {
            start_seconds,
            end_seconds,
            key: Key::new(tonic, Mode::Minor),
        }
    }

    #[test]
    fn one_key_draws_no_timeline_and_several_keep_their_silent_gaps() {
        assert!(key_stretches(&[section(0.0, 200.0, 9)], 200.0).is_empty());
        let stretches = key_stretches(
            &[
                section(0.0, 80.0, 9),
                section(100.0, 200.0, 4),
                section(190.0, 260.0, 2),
            ],
            200.0,
        );
        assert_eq!(stretches.len(), 3);
        assert_eq!((stretches[0].start, stretches[0].end), (0.0, 0.4));
        assert_eq!((stretches[1].start, stretches[1].end), (0.5, 1.0));
        assert_eq!(stretches[2].end, 1.0, "clipped to the end of the track");
        assert!(
            stretches[1].start > stretches[0].end,
            "the silence stays a gap"
        );
    }
}
