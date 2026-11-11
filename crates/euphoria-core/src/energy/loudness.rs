// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::num::NonZeroU32;

use ebur128::{Channel, EbuR128, Mode};

use super::check_sample_rate;
use crate::error::EuphoriaError;

/// Measures integrated loudness in LUFS, as EBU R128 describes. Returns
/// `Ok(None)` when the standard gives no loudness: the audio is shorter than
/// one 400 millisecond block or never rises above -70 LUFS.
///
/// The memory of the meter grows with the sample rate. A sample rate
/// outside the supported range is an error before the meter is created.
pub(crate) fn measure_loudness(
    samples: &[f32],
    sample_rate: NonZeroU32,
) -> Result<Option<f32>, EuphoriaError> {
    check_sample_rate(sample_rate)?;
    let mut meter = EbuR128::new(1, sample_rate.get(), Mode::I).map_err(loudness_error)?;
    // The buffer is a mixdown that both speakers play, which is 3 dB louder
    // than the same signal from one speaker.
    meter
        .set_channel(0, Channel::DualMono)
        .map_err(loudness_error)?;
    meter.add_frames_f32(samples).map_err(loudness_error)?;
    let integrated_lufs = meter.loudness_global().map_err(loudness_error)?;
    Ok(integrated_lufs
        .is_finite()
        .then_some(integrated_lufs as f32))
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
        assert!(loudness.abs() < 0.1, "{loudness}");
    }

    #[test]
    fn halving_the_amplitude_lowers_loudness_by_six_lu() {
        let loud = measure_loudness(&sine(997.0, 0.5, 5.0), sample_rate())
            .unwrap()
            .unwrap();
        let quiet = measure_loudness(&sine(997.0, 0.25, 5.0), sample_rate())
            .unwrap()
            .unwrap();
        let difference = loud - quiet;
        assert!((difference - 6.02).abs() < 0.1, "{difference}");
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
