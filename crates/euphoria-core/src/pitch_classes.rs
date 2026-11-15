// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The pitch-class profile of a track: how strongly each of the 12 notes
//! sounds in it, every octave folded together, C first. A doubtful key can
//! be checked by eye: the notes of the key should stand out.

use crate::audio::AudioBuffer;
use crate::detect::measure_tuning;
use crate::error::EuphoriaError;

/// Notes in an octave.
pub const PITCH_CLASS_COUNT: usize = 12;

/// How strongly each of the 12 notes sounds in mono `samples` at
/// `sample_rate`, C first, from 0 to 1 with the strongest note at 1. All
/// zero when no note sounds, as in silence or when there are no samples.
///
/// Each note is heard on the scale of the track's own tuning, when it can
/// be measured, and the steady level that sounds without pitch, such as
/// noise and the body of a drum, is taken off first. Fails for a sample
/// rate of zero.
pub fn pitch_class_profile(
    samples: &[f32],
    sample_rate: u32,
) -> Result<[f32; PITCH_CLASS_COUNT], EuphoriaError> {
    let Some(sample_rate) = std::num::NonZeroU32::new(sample_rate) else {
        return Err(EuphoriaError::Detection {
            detector: "pitch-class profile",
            reason: "the sample rate is zero".to_string(),
            source: None,
        });
    };
    let audio = AudioBuffer::new(samples.to_vec(), sample_rate);
    Ok(profile_of_audio(&audio).unwrap_or([0.0; PITCH_CLASS_COUNT]))
}

/// The profile of `audio` on the scale of its measured tuning, or `None`
/// when no note sounds.
pub(crate) fn profile_of_audio(audio: &AudioBuffer) -> Option<[f32; PITCH_CLASS_COUNT]> {
    let tuning_cents = measure_tuning(audio).map_or(0.0, |tuning| tuning.cents);
    crate::detect::pitch_class_profile_at(audio, tuning_cents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_a_minor_chord_lights_a_c_and_e() {
        let sample_rate = 44_100;
        let samples: Vec<f32> = (0..sample_rate * 6)
            .map(|index| {
                let time = index as f32 / sample_rate as f32;
                [220.0_f32, 261.63, 329.63]
                    .iter()
                    .map(|frequency| 0.2 * (std::f32::consts::TAU * frequency * time).sin())
                    .sum()
            })
            .collect();

        let profile = pitch_class_profile(&samples, sample_rate as u32).unwrap();

        let mut ranked: Vec<usize> = (0..PITCH_CLASS_COUNT).collect();
        ranked.sort_by(|left, right| profile[*right].total_cmp(&profile[*left]));
        let mut strongest = ranked[..3].to_vec();
        strongest.sort_unstable();
        assert_eq!(strongest, vec![0, 4, 9], "{profile:?}");
        assert_eq!(profile.iter().copied().fold(0.0, f32::max), 1.0);
    }

    #[test]
    fn silence_and_no_samples_give_zeros_and_no_rate_fails() {
        assert_eq!(
            pitch_class_profile(&[], 44_100).unwrap(),
            [0.0; PITCH_CLASS_COUNT]
        );
        assert_eq!(
            pitch_class_profile(&vec![0.0; 44_100 * 3], 44_100).unwrap(),
            [0.0; PITCH_CLASS_COUNT]
        );
        assert!(pitch_class_profile(&[0.1; 100], 0).is_err());
    }
}
