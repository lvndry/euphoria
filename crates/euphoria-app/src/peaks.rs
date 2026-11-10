// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reducing decoded audio to the peaks drawn as a waveform.

/// Number of peaks kept per track, enough for the widest waveform drawn.
pub(crate) const PEAK_COUNT: usize = 720;

/// Splits `samples` into `peak_count` equal spans and returns the loudest
/// absolute sample of each, scaled so the loudest peak of the track is 1.0.
/// Silent audio gives all zeros.
pub(crate) fn compute_peaks(samples: &[f32], peak_count: usize) -> Vec<f32> {
    if samples.is_empty() || peak_count == 0 {
        return Vec::new();
    }
    let mut peaks: Vec<f32> = (0..peak_count)
        .map(|peak_index| {
            let start = (peak_index * samples.len() / peak_count).min(samples.len() - 1);
            let end = ((peak_index + 1) * samples.len() / peak_count)
                .max(start + 1)
                .min(samples.len());
            samples[start..end]
                .iter()
                .fold(0.0_f32, |loudest, sample| loudest.max(sample.abs()))
        })
        .collect();
    let loudest = peaks.iter().copied().fold(0.0_f32, f32::max);
    if loudest > 0.0 {
        for peak in &mut peaks {
            *peak /= loudest;
        }
    }
    peaks
}

/// Reduces `peaks` to at most `bar_count` bars, each the loudest peak it
/// covers.
pub(crate) fn bars_from_peaks(peaks: &[f32], bar_count: usize) -> Vec<f32> {
    compute_peaks(peaks, bar_count.min(peaks.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_audio_has_no_peaks() {
        assert!(compute_peaks(&[], 10).is_empty());
        assert!(compute_peaks(&[0.5], 0).is_empty());
    }

    #[test]
    fn keeps_the_loudest_absolute_sample_of_each_span() {
        let samples = [0.1, -0.5, 0.2, 0.25, -0.1, 0.0];
        assert_eq!(compute_peaks(&samples, 3), vec![1.0, 0.5, 0.2]);
    }

    #[test]
    fn scales_quiet_audio_to_full_height() {
        let samples = [0.01, 0.02, 0.005, 0.01];
        assert_eq!(compute_peaks(&samples, 2), vec![1.0, 0.5]);
    }

    #[test]
    fn silence_stays_flat() {
        assert_eq!(compute_peaks(&[0.0; 8], 4), vec![0.0; 4]);
    }

    #[test]
    fn more_peaks_than_samples_still_covers_every_peak() {
        let peaks = compute_peaks(&[0.5, 1.0], 4);
        assert_eq!(peaks.len(), 4);
        assert!(peaks.iter().all(|peak| *peak > 0.0));
    }

    #[test]
    fn bars_never_outnumber_peaks() {
        assert_eq!(bars_from_peaks(&[1.0, 0.5, 0.25], 10).len(), 3);
        assert_eq!(bars_from_peaks(&[1.0, 0.5, 0.25, 0.5], 2), vec![1.0, 0.5]);
    }
}
