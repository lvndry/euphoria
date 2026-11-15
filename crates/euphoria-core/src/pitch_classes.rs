// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The pitch-class profile of a track: how strongly each of the 12 notes
//! sounds in it, every octave folded together, C first. A doubtful key can
//! be checked by eye: the notes of the key should stand out.

#[cfg(feature = "libkeyfinder")]
use crate::error::EuphoriaError;
use crate::key::{Key, Mode};

/// Notes in an octave.
pub const PITCH_CLASS_COUNT: usize = 12;

/// How strongly each of the 12 notes sounds in mono `samples` at
/// `sample_rate`, C first, from 0 to 1 with the strongest note at 1. All
/// zero when no note sounds, as in silence or when there are no samples.
///
/// It is libkeyfinder's chromagram of the samples averaged over time, after
/// audio at least 20 cents from concert pitch is brought back to it: the
/// same profile an analysis stores. Fails when libkeyfinder fails, as for
/// audio longer than it can count, or for a sample rate of zero.
#[cfg(feature = "libkeyfinder")]
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
    if samples.is_empty() {
        return Ok([0.0; PITCH_CLASS_COUNT]);
    }
    let audio = crate::audio::AudioBuffer::new(samples.to_vec(), sample_rate);
    Ok(crate::detect::KeyFinderDetector
        .pitch_class_profile(&audio)?
        .unwrap_or([0.0; PITCH_CLASS_COUNT]))
}

/// Totals scaled so the largest is 1, or `None` when no note sounds at all
/// or a total is not a finite number.
pub(crate) fn normalised(totals: [f64; PITCH_CLASS_COUNT]) -> Option<[f32; PITCH_CLASS_COUNT]> {
    if totals
        .iter()
        .any(|total| !total.is_finite() || *total < 0.0)
    {
        return None;
    }
    let largest = totals.iter().copied().fold(0.0_f64, f64::max);
    (largest > 0.0).then(|| std::array::from_fn(|index| (totals[index] / largest) as f32))
}

/// Which of the 12 notes, C first, belong to the scale of `key`: the major
/// scale, or the natural minor scale, which has the same notes as its
/// relative major.
pub fn notes_in_key(key: Key) -> [bool; PITCH_CLASS_COUNT] {
    const MAJOR_STEPS: [usize; 7] = [0, 2, 4, 5, 7, 9, 11];
    const MINOR_STEPS: [usize; 7] = [0, 2, 3, 5, 7, 8, 10];
    let steps = match key.mode() {
        Mode::Major => MAJOR_STEPS,
        Mode::Minor => MINOR_STEPS,
    };
    let mut in_key = [false; PITCH_CLASS_COUNT];
    for step in steps {
        in_key[(usize::from(key.tonic()) + step) % PITCH_CLASS_COUNT] = true;
    }
    in_key
}

const SHARP_NAMES: [&str; PITCH_CLASS_COUNT] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
const FLAT_NAMES: [&str; PITCH_CLASS_COUNT] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];
const COMMON_NAMES: [&str; PITCH_CLASS_COUNT] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];

/// The names of the 12 notes, C first, spelled as the key signature of
/// `key` writes them: with sharps in keys with sharps, with flats in keys
/// with flats. Without a key, and in C major and A minor, each black key
/// gets its most common name.
pub fn note_names(key: Option<Key>) -> [&'static str; PITCH_CLASS_COUNT] {
    let Some(key) = key else {
        return COMMON_NAMES;
    };
    let major_tonic = match key.mode() {
        Mode::Major => key.tonic(),
        Mode::Minor => (key.tonic() + 3) % 12,
    };
    let sharps = usize::from(major_tonic) * 7 % PITCH_CLASS_COUNT;
    match sharps {
        0 => COMMON_NAMES,
        1..=6 => SHARP_NAMES,
        _ => FLAT_NAMES,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> Key {
        text.parse().unwrap()
    }

    #[test]
    fn the_strongest_note_scales_to_one() {
        let mut totals = [1.0; PITCH_CLASS_COUNT];
        totals[9] = 4.0;
        let profile = normalised(totals).unwrap();
        assert_eq!(profile[9], 1.0);
        assert_eq!(profile[0], 0.25);
    }

    #[test]
    fn nothing_heard_is_no_profile() {
        assert_eq!(normalised([0.0; PITCH_CLASS_COUNT]), None);
        let mut totals = [1.0; PITCH_CLASS_COUNT];
        totals[3] = f64::NAN;
        assert_eq!(normalised(totals), None);
    }

    #[test]
    fn relative_keys_share_their_notes() {
        assert_eq!(notes_in_key(key("A minor")), notes_in_key(key("C major")));
        assert_eq!(notes_in_key(key("8A")), notes_in_key(key("8B")));
        let a_minor = notes_in_key(key("A minor"));
        assert_eq!(a_minor.iter().filter(|in_key| **in_key).count(), 7);
        assert!(!a_minor[1] && !a_minor[6]);
        let e_major = notes_in_key(key("E major"));
        assert!(e_major[8] && e_major[1] && !e_major[0]);
    }

    #[test]
    fn note_names_follow_the_key_signature() {
        assert_eq!(note_names(Some(key("E major")))[8], "G#");
        assert_eq!(note_names(Some(key("C# minor")))[8], "G#");
        assert_eq!(note_names(Some(key("Ab major")))[8], "Ab");
        assert_eq!(note_names(Some(key("F minor")))[1], "Db");
        assert_eq!(note_names(Some(key("F major")))[10], "Bb");
        assert_eq!(note_names(Some(key("F# major")))[6], "F#");
        assert_eq!(note_names(Some(key("A minor")))[10], "Bb");
        assert_eq!(note_names(None)[1], "C#");
    }

    #[cfg(feature = "libkeyfinder")]
    #[test]
    fn an_a_minor_chord_lights_a_c_and_e() {
        let sample_rate = 44_100;
        let frequencies = [220.0_f32, 261.63, 329.63];
        let samples: Vec<f32> = (0..sample_rate * 6)
            .map(|index| {
                let time = index as f32 / sample_rate as f32;
                frequencies
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

    #[cfg(feature = "libkeyfinder")]
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
