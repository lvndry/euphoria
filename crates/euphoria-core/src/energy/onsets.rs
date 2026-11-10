// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

/// A frame is an onset only when no frame within this distance is stronger.
const PEAK_REACH_SECONDS: f32 = 0.03;
/// The surrounding level an onset must stand out from is the mean strength
/// within this distance.
const AVERAGE_REACH_SECONDS: f32 = 0.1;
/// Two onsets are at least this far apart.
const MINIMUM_GAP_SECONDS: f32 = 0.05;
/// How far above its surroundings an onset rises, as a multiple of the mean
/// strength of the whole signal.
const PROMINENCE_RATIO: f32 = 0.5;
/// The smallest prominence that counts, which keeps numerical noise in a
/// steady signal from registering as onsets.
const MINIMUM_PROMINENCE: f32 = 1.0e-3;

/// Counts the onsets in an onset strength envelope: the local peaks that
/// stand out from the frames around them.
pub(crate) fn count_onsets(strength: &[f32], frames_per_second: f32) -> usize {
    if strength.is_empty() {
        return 0;
    }
    let frame_count = |seconds: f32| ((seconds * frames_per_second).round() as usize).max(1);
    let peak_reach = frame_count(PEAK_REACH_SECONDS);
    let average_reach = frame_count(AVERAGE_REACH_SECONDS);
    let minimum_gap = frame_count(MINIMUM_GAP_SECONDS);

    let prominence = (mean(strength) * PROMINENCE_RATIO).max(MINIMUM_PROMINENCE);
    let mut onset_count = 0;
    let mut last_onset: Option<usize> = None;
    for (index, &value) in strength.iter().enumerate() {
        if last_onset.is_some_and(|last| index - last < minimum_gap) {
            continue;
        }
        let neighbours = surrounding(strength, index, peak_reach);
        if neighbours.iter().any(|&neighbour| neighbour > value) {
            continue;
        }
        if value < mean(surrounding(strength, index, average_reach)) + prominence {
            continue;
        }
        onset_count += 1;
        last_onset = Some(index);
    }
    onset_count
}

fn surrounding(values: &[f32], index: usize, reach: usize) -> &[f32] {
    let start = index.saturating_sub(reach);
    let end = (index + reach + 1).min(values.len());
    &values[start..end]
}

fn mean(values: &[f32]) -> f32 {
    values.iter().sum::<f32>() / values.len() as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAMES_PER_SECOND: f32 = 100.0;

    fn envelope_with_peaks(frame_count: usize, peak_spacing: usize, peak_height: f32) -> Vec<f32> {
        (0..frame_count)
            .map(|index| {
                if index % peak_spacing == peak_spacing / 2 {
                    peak_height
                } else {
                    0.01
                }
            })
            .collect()
    }

    #[test]
    fn an_empty_envelope_has_no_onsets() {
        assert_eq!(count_onsets(&[], FRAMES_PER_SECOND), 0);
    }

    #[test]
    fn a_flat_envelope_has_no_onsets() {
        assert_eq!(count_onsets(&[0.0; 500], FRAMES_PER_SECOND), 0);
        assert_eq!(count_onsets(&[0.3; 500], FRAMES_PER_SECOND), 0);
    }

    #[test]
    fn every_isolated_peak_is_one_onset() {
        let envelope = envelope_with_peaks(1_000, 50, 1.0);
        assert_eq!(count_onsets(&envelope, FRAMES_PER_SECOND), 20);
    }

    #[test]
    fn the_count_does_not_depend_on_the_scale_of_the_envelope() {
        let envelope = envelope_with_peaks(1_000, 25, 1.0);
        let scaled: Vec<f32> = envelope.iter().map(|value| value * 0.1).collect();
        assert_eq!(
            count_onsets(&envelope, FRAMES_PER_SECOND),
            count_onsets(&scaled, FRAMES_PER_SECOND)
        );
    }

    #[test]
    fn peaks_closer_than_the_minimum_gap_count_once() {
        let mut envelope = vec![0.01; 200];
        envelope[100] = 1.0;
        envelope[102] = 1.0;
        assert_eq!(count_onsets(&envelope, FRAMES_PER_SECOND), 1);
    }

    #[test]
    fn small_ripples_on_a_high_level_are_not_onsets() {
        let envelope: Vec<f32> = (0..1_000)
            .map(|index| 1.0 + 0.05 * (index as f32 * 0.7).sin())
            .collect();
        assert_eq!(count_onsets(&envelope, FRAMES_PER_SECOND), 0);
    }
}
