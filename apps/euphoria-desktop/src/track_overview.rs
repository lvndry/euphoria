// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The energy curve and the key timeline of a track, as stretches of the
//! track from its start (0) to its end (1), so that they line up with the
//! waveform and the playhead drawn above them.

use std::sync::Arc;

use euphoria_core::stems::VocalWindow;
use euphoria_core::{EnergyCurvePoint, EuphoriaError, Key, KeySection};

use crate::format;

/// The energy curve of a track, once worked out.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EnergyCurve {
    /// The steps of the curve, empty when the track has no window to rate.
    Steps(Arc<[EnergyStep]>),
    /// The core could not rate the audio; the text says why.
    Unavailable(String),
}

/// The energy curve from what the core returned for audio
/// `duration_seconds` long. A failure is kept as one, so that it is not
/// mistaken for a track without rated windows.
pub(crate) fn energy_curve_from(
    points: Result<Vec<EnergyCurvePoint>, EuphoriaError>,
    duration_seconds: f32,
) -> EnergyCurve {
    match points {
        Ok(points) => EnergyCurve::Steps(energy_steps(&points, duration_seconds).into()),
        Err(error) => EnergyCurve::Unavailable(error.chain()),
    }
}

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
    let windows: Vec<(f32, f32)> = points
        .iter()
        .map(|point| (point.start_seconds, point.duration_seconds))
        .collect();
    window_steps(&windows, duration_seconds)
        .into_iter()
        .zip(points)
        .map(|((start, end), point)| EnergyStep {
            start,
            end,
            rating: point.rating.as_ref().map(|rating| rating.rating),
        })
        .filter(|step| step.end > step.start)
        .collect()
}

/// Where each of the overlapping `windows`, as a start and a length in
/// seconds, is drawn along a track `duration_seconds` long, as fractions of
/// it: the lanes under the waveform share this, so they step together.
fn window_steps(windows: &[(f32, f32)], duration_seconds: f32) -> Vec<(f32, f32)> {
    if duration_seconds <= 0.0 || !duration_seconds.is_finite() {
        return Vec::new();
    }
    let middles: Vec<f32> = windows
        .iter()
        .map(|(start, length)| start + length / 2.0)
        .collect();
    let fraction = |seconds: f32| (seconds / duration_seconds).clamp(0.0, 1.0);
    windows
        .iter()
        .enumerate()
        .map(|(index, (window_start, window_length))| {
            let start = match index {
                0 => 0.0,
                _ => (middles[index - 1] + middles[index]) / 2.0,
            };
            let end = match middles.get(index + 1) {
                Some(next) => (middles[index] + next) / 2.0,
                None => window_start + window_length,
            };
            (fraction(start), fraction(end))
        })
        .collect()
}

/// What the tooltip over the energy curve says about the point `fraction`
/// of the way along a track `duration_seconds` long.
pub(crate) fn energy_reading(steps: &[EnergyStep], duration_seconds: f32, fraction: f32) -> String {
    let time = format::duration(fraction.clamp(0.0, 1.0) * duration_seconds);
    match steps
        .iter()
        .find(|step| fraction >= step.start && fraction < step.end)
        .or_else(|| steps.last().filter(|step| fraction >= step.end))
        .and_then(|step| step.rating)
    {
        Some(rating) => format!("Energy {rating} at {time}"),
        None => format!("Silent at {time}"),
    }
}

/// What the tooltip on the "Energy" label says.
pub(crate) const ENERGY_EXPLANATION: &str = "Energy over time, 1 to 10, rated every 4 seconds";

/// What the tooltip on the "Vocals" label says.
pub(crate) const VOCALS_EXPLANATION: &str =
    "Where the vocals are, from the separated vocals stem, in the same 4 second steps";

/// The vocals lane of a track, once worked out.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum VocalLane {
    /// The stems are being looked for, or the vocals measured.
    Checking,
    /// The track has no separated stems.
    NoStems,
    /// The windows of the track, each with whether the vocals sound in it.
    Ready(Arc<[VocalWindow]>),
    /// The vocals stem could not be read; the text says why.
    Unavailable(String),
}

/// The vocals lane for a track whose separated stems are `files`, or the
/// lane of a track without stems. Reads the vocals stem the first time and
/// the measurement kept next to the stems after that.
pub(crate) fn vocal_lane_from(files: Option<euphoria_core::stems::StemFiles>) -> VocalLane {
    let Some(files) = files else {
        return VocalLane::NoStems;
    };
    match euphoria_core::stems::vocal_activity(&files) {
        Ok(activity) => VocalLane::Ready(activity.windows.into()),
        Err(error) => VocalLane::Unavailable(error.chain()),
    }
}

/// A stretch of the track with vocals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VocalStretch {
    /// Where the stretch starts, as a fraction of the track.
    pub(crate) start: f32,
    /// Where it ends, as a fraction of the track.
    pub(crate) end: f32,
}

/// The stretches with vocals in a track `duration_seconds` long, with the
/// windows that follow each other with vocals joined into one.
pub(crate) fn vocal_stretches(windows: &[VocalWindow], duration_seconds: f32) -> Vec<VocalStretch> {
    let spans: Vec<(f32, f32)> = windows
        .iter()
        .map(|window| (window.start_seconds, window.duration_seconds))
        .collect();
    let mut stretches: Vec<VocalStretch> = Vec::new();
    for ((start, end), window) in window_steps(&spans, duration_seconds)
        .into_iter()
        .zip(windows)
    {
        if !window.active || end <= start {
            continue;
        }
        match stretches.last_mut() {
            Some(last) if start <= last.end => last.end = last.end.max(end),
            _ => stretches.push(VocalStretch { start, end }),
        }
    }
    stretches
}

/// What the tooltip over the vocals lane says about the point `fraction`
/// of the way along a track `duration_seconds` long: the stretch with
/// vocals it is in, or the stretch without them.
pub(crate) fn vocal_reading(
    stretches: &[VocalStretch],
    duration_seconds: f32,
    fraction: f32,
) -> String {
    let time = |fraction: f32| format::duration(fraction * duration_seconds);
    if let Some(stretch) = stretches
        .iter()
        .find(|stretch| fraction >= stretch.start && fraction < stretch.end)
    {
        return format!(
            "Vocals from {} to {}",
            time(stretch.start),
            time(stretch.end)
        );
    }
    let gap_start = stretches
        .iter()
        .rev()
        .find(|stretch| stretch.end <= fraction)
        .map_or(0.0, |stretch| stretch.end);
    let gap_end = stretches
        .iter()
        .find(|stretch| stretch.start > fraction)
        .map_or(1.0, |stretch| stretch.start);
    format!("No vocals from {} to {}", time(gap_start), time(gap_end))
}

/// How far along a lane `bounds` wide the pointer is, from 0 to 1.
pub(crate) fn pointer_fraction(pointer_x: f32, left: f32, width: f32) -> f32 {
    if width <= 0.0 {
        return 0.0;
    }
    ((pointer_x - left) / width).clamp(0.0, 1.0)
}

/// The shortest a key section may be for the timeline to be shown.
pub(crate) const SHORTEST_SHOWN_KEY_SECTION_SECONDS: f32 = 30.0;

/// Whether the key timeline of a track whose overall key is `track_key` is
/// shown at all. The sections are not always consistent with the key of
/// the whole track yet, so a timeline is shown only when that key is one of
/// its sections and every section lasts at least
/// [`SHORTEST_SHOWN_KEY_SECTION_SECONDS`]; otherwise it could contradict the
/// key shown above it. Relaxing this rule is a change to this function only.
pub(crate) fn shows_key_timeline(track_key: Option<Key>, sections: &[KeySection]) -> bool {
    let Some(track_key) = track_key else {
        return false;
    };
    sections.iter().any(|section| section.key == track_key)
        && sections.iter().all(|section| {
            section.end_seconds - section.start_seconds >= SHORTEST_SHOWN_KEY_SECTION_SECONDS
        })
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
    if duration_seconds <= 0.0
        || !duration_seconds.is_finite()
        || sections
            .first()
            .is_none_or(|first| sections.iter().all(|section| section.key == first.key))
    {
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
    fn a_failed_curve_is_unavailable_with_its_reason_and_not_an_empty_curve() {
        let failed = energy_curve_from(
            Err(EuphoriaError::UnsupportedSampleRate {
                sample_rate: 4000,
                minimum: 8000,
                maximum: 384_000,
            }),
            10.0,
        );
        match failed {
            EnergyCurve::Unavailable(reason) => assert!(reason.contains("4000"), "{reason}"),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            energy_curve_from(Ok(Vec::new()), 10.0),
            EnergyCurve::Steps(Vec::new().into())
        );
        assert_eq!(
            energy_curve_from(Ok(vec![point(0.0, 10.0, Some(4))]), 10.0),
            EnergyCurve::Steps(
                vec![EnergyStep {
                    start: 0.0,
                    end: 1.0,
                    rating: Some(4)
                }]
                .into()
            )
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

    fn window(start_seconds: f32, active: bool) -> VocalWindow {
        VocalWindow {
            start_seconds,
            duration_seconds: 8.0,
            active,
        }
    }

    #[test]
    fn the_energy_tooltip_reads_the_rating_at_the_pointer_and_says_silent_in_a_gap() {
        let steps = energy_steps(
            &[
                point(0.0, 8.0, Some(9)),
                point(4.0, 8.0, Some(3)),
                point(8.0, 8.0, None),
            ],
            160.0,
        );
        assert_eq!(energy_reading(&steps, 160.0, 0.0), "Energy 9 at 0:00");
        assert_eq!(
            energy_reading(&steps, 160.0, 6.0 / 160.0),
            "Energy 3 at 0:06"
        );
        assert_eq!(
            energy_reading(&steps, 160.0, 12.0 / 160.0),
            "Silent at 0:12"
        );
        assert_eq!(energy_reading(&steps, 160.0, 0.5), "Silent at 1:20");
        assert_eq!(
            energy_reading(&steps, 160.0, 7.0 / 160.0),
            "Energy 3 at 0:07"
        );
    }

    #[test]
    fn the_energy_explanation_states_the_window_and_the_hop_of_the_core() {
        assert_eq!(
            ENERGY_EXPLANATION,
            "Energy over time, 1 to 10, rated every 4 seconds"
        );
        assert_eq!(euphoria_core::energy::ENERGY_CURVE_HOP_SECONDS, 4.0);
        assert_eq!(euphoria_core::energy::ENERGY_CURVE_WINDOW_SECONDS, 8.0);
    }

    #[test]
    fn the_vocal_stretches_use_the_steps_of_the_energy_curve_and_join_neighbours() {
        let windows = [
            window(0.0, false),
            window(4.0, true),
            window(8.0, true),
            window(12.0, false),
            window(16.0, true),
        ];
        let energy = energy_steps(
            &[
                point(0.0, 8.0, Some(5)),
                point(4.0, 8.0, Some(5)),
                point(8.0, 8.0, Some(5)),
                point(12.0, 8.0, Some(5)),
                point(16.0, 8.0, Some(5)),
            ],
            24.0,
        );
        let stretches = vocal_stretches(&windows, 24.0);
        assert_eq!(stretches.len(), 2);
        assert_eq!(stretches[0].start, energy[1].start);
        assert_eq!(stretches[0].end, energy[2].end);
        assert_eq!(stretches[1].start, energy[4].start);
        assert_eq!(stretches[1].end, energy[4].end);
        assert!(vocal_stretches(&windows, 0.0).is_empty());
        assert!(vocal_stretches(&[window(0.0, false)], 8.0).is_empty());
    }

    #[test]
    fn the_vocals_tooltip_names_the_stretch_with_vocals_or_the_gap_without() {
        let windows = [
            window(0.0, false),
            window(4.0, false),
            window(8.0, true),
            window(12.0, true),
            window(16.0, false),
            window(20.0, false),
            window(24.0, false),
        ];
        let stretches = vocal_stretches(&windows, 32.0);
        assert_eq!(
            vocal_reading(&stretches, 32.0, 12.0 / 32.0),
            "Vocals from 0:10 to 0:18"
        );
        assert_eq!(
            vocal_reading(&stretches, 32.0, 2.0 / 32.0),
            "No vocals from 0:00 to 0:10"
        );
        assert_eq!(
            vocal_reading(&stretches, 32.0, 30.0 / 32.0),
            "No vocals from 0:18 to 0:32"
        );
        assert_eq!(
            vocal_reading(&[], 100.0, 0.5),
            "No vocals from 0:00 to 1:40"
        );
    }

    #[test]
    fn a_track_without_stems_has_no_vocals_to_draw_and_a_missing_stem_says_why() {
        assert_eq!(vocal_lane_from(None), VocalLane::NoStems);
        let missing = euphoria_core::stems::StemFiles::in_directory(
            std::env::temp_dir().join("euphoria-no-such-stems"),
        );
        assert!(matches!(
            vocal_lane_from(Some(missing)),
            VocalLane::Unavailable(_)
        ));
    }

    #[test]
    fn the_pointer_is_placed_along_the_lane_and_kept_within_it() {
        assert_eq!(pointer_fraction(150.0, 100.0, 200.0), 0.25);
        assert_eq!(pointer_fraction(50.0, 100.0, 200.0), 0.0);
        assert_eq!(pointer_fraction(500.0, 100.0, 200.0), 1.0);
        assert_eq!(pointer_fraction(150.0, 100.0, 0.0), 0.0);
    }

    fn section(start_seconds: f32, end_seconds: f32, tonic: u8) -> KeySection {
        KeySection {
            start_seconds,
            end_seconds,
            key: Key::new(tonic, Mode::Minor),
        }
    }

    #[test]
    fn a_timeline_is_shown_only_when_it_agrees_with_the_key_of_the_track() {
        let sections = [section(0.0, 80.0, 9), section(100.0, 200.0, 4)];
        let a_minor = Some(Key::new(9, Mode::Minor));
        assert!(shows_key_timeline(a_minor, &sections));
        assert!(shows_key_timeline(
            Some(Key::new(4, Mode::Minor)),
            &sections
        ));

        let e_flat_minor = Some(Key::new(3, Mode::Minor));
        assert!(
            !shows_key_timeline(e_flat_minor, &sections),
            "the key of the track is none of the sections"
        );
        assert!(!shows_key_timeline(None, &sections));
        assert!(!shows_key_timeline(
            Some(Key::new(9, Mode::Major)),
            &sections
        ));
    }

    #[test]
    fn a_timeline_with_a_section_shorter_than_thirty_seconds_is_not_shown() {
        let a_minor = Some(Key::new(9, Mode::Minor));
        let brief = [
            section(0.0, 80.0, 9),
            section(80.0, 109.0, 4),
            section(109.0, 200.0, 9),
        ];
        assert!(!shows_key_timeline(a_minor, &brief));
        let just_long_enough = [section(0.0, 80.0, 9), section(80.0, 110.0, 4)];
        assert!(shows_key_timeline(a_minor, &just_long_enough));
    }

    #[test]
    fn one_key_draws_no_timeline_and_several_keep_their_silent_gaps() {
        assert!(key_stretches(&[section(0.0, 200.0, 9)], 200.0).is_empty());
        assert!(
            key_stretches(&[section(0.0, 80.0, 9), section(100.0, 200.0, 9)], 200.0).is_empty()
        );
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
