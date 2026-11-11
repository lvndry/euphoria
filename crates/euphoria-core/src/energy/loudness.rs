// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::num::NonZeroU32;

use ebur128::{Channel, EbuR128, Mode};

use super::check_sample_rate;
use crate::error::EuphoriaError;

/// Loudness of a stretch of audio, measured as EBU R128 describes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Loudness {
    /// Integrated loudness in LUFS.
    pub(crate) integrated_lufs: f32,
    /// Loudness range in LU.
    pub(crate) range_lu: f32,
}

/// Measures integrated loudness and loudness range. Returns `Ok(None)` when
/// the standard gives no loudness: the audio is shorter than one 400
/// millisecond block or never rises above -70 LUFS.
///
/// The meter keeps three seconds of filtered audio, so its memory grows with
/// the sample rate. A sample rate outside the supported range is an error
/// before the meter is created.
pub(crate) fn measure_loudness(
    samples: &[f32],
    sample_rate: NonZeroU32,
) -> Result<Option<Loudness>, EuphoriaError> {
    check_sample_rate(sample_rate)?;
    let mut meter =
        EbuR128::new(1, sample_rate.get(), Mode::I | Mode::LRA).map_err(loudness_error)?;
    // The buffer is a mixdown that both speakers play, which is 3 dB louder
    // than the same signal from one speaker.
    meter
        .set_channel(0, Channel::DualMono)
        .map_err(loudness_error)?;
    meter.add_frames_f32(samples).map_err(loudness_error)?;
    let integrated_lufs = meter.loudness_global().map_err(loudness_error)?;
    if !integrated_lufs.is_finite() {
        return Ok(None);
    }
    let range_lu = meter.loudness_range().map_err(loudness_error)?;
    Ok(Some(Loudness {
        integrated_lufs: integrated_lufs as f32,
        range_lu: range_lu as f32,
    }))
}

fn loudness_error(error: ebur128::Error) -> EuphoriaError {
    EuphoriaError::Detection {
        detector: "ebur128",
        reason: "loudness measurement failed".to_string(),
        source: Some(Box::new(error)),
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;

    const SAMPLE_RATE: u32 = 48_000;

    fn sample_rate() -> NonZeroU32 {
        NonZeroU32::new(SAMPLE_RATE).unwrap()
    }

    fn sine(frequency: f32, amplitude: f32, seconds: f32) -> Vec<f32> {
        let sample_count = (SAMPLE_RATE as f32 * seconds) as usize;
        (0..sample_count)
            .map(|index| (TAU * frequency * index as f32 / SAMPLE_RATE as f32).sin() * amplitude)
            .collect()
    }

    #[test]
    fn a_full_scale_sine_from_both_speakers_measures_zero_lufs() {
        let loudness = measure_loudness(&sine(997.0, 1.0, 5.0), sample_rate())
            .unwrap()
            .unwrap();
        assert!(
            loudness.integrated_lufs.abs() < 0.1,
            "{}",
            loudness.integrated_lufs
        );
    }

    #[test]
    fn halving_the_amplitude_lowers_loudness_by_six_lu() {
        let loud = measure_loudness(&sine(997.0, 0.5, 5.0), sample_rate())
            .unwrap()
            .unwrap();
        let quiet = measure_loudness(&sine(997.0, 0.25, 5.0), sample_rate())
            .unwrap()
            .unwrap();
        let difference = loud.integrated_lufs - quiet.integrated_lufs;
        assert!((difference - 6.02).abs() < 0.1, "{difference}");
    }

    #[test]
    fn a_steady_tone_has_no_loudness_range() {
        let loudness = measure_loudness(&sine(440.0, 0.5, 10.0), sample_rate())
            .unwrap()
            .unwrap();
        assert!(loudness.range_lu < 0.1, "{}", loudness.range_lu);
    }

    #[test]
    fn a_tone_that_alternates_between_two_levels_has_a_loudness_range() {
        let mut samples = Vec::new();
        for _ in 0..3 {
            samples.extend(sine(440.0, 0.5, 6.0));
            samples.extend(sine(440.0, 0.05, 6.0));
        }
        let loudness = measure_loudness(&samples, sample_rate()).unwrap().unwrap();
        assert!(loudness.range_lu > 10.0, "{}", loudness.range_lu);
    }

    #[test]
    fn an_unsupported_sample_rate_is_an_error_whatever_the_length() {
        for sample_count in [0, 4, 100_000] {
            for rate in [1, 7_999, 384_001, u32::MAX] {
                let outcome =
                    measure_loudness(&vec![0.5; sample_count], NonZeroU32::new(rate).unwrap());
                assert!(
                    matches!(
                        outcome,
                        Err(EuphoriaError::UnsupportedSampleRate { sample_rate, .. })
                            if sample_rate == rate
                    ),
                    "{rate} Hz with {sample_count} samples gave {outcome:?}"
                );
            }
        }
    }

    #[test]
    fn silence_has_no_loudness() {
        let silence = vec![0.0; SAMPLE_RATE as usize * 2];
        assert_eq!(measure_loudness(&silence, sample_rate()).unwrap(), None);
    }

    #[test]
    fn audio_shorter_than_one_gating_block_has_no_loudness() {
        let samples = sine(440.0, 0.5, 0.1);
        assert_eq!(measure_loudness(&samples, sample_rate()).unwrap(), None);
    }
}
