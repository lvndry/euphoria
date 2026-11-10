// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::os::raw::{c_int, c_uint, c_ulong};

use crate::audio::AudioBuffer;
use crate::detect::KeyDetector;
use crate::error::EuphoriaError;
use crate::key::{Key, Mode};

unsafe extern "C" {
    fn euphoria_keyfinder_detect(
        samples: *const f32,
        sample_count: c_ulong,
        frame_rate: c_uint,
    ) -> c_int;
}

const SILENCE_CODE: c_int = 24;
const A_PITCH_CLASS: i32 = 9;

/// Key detection through libkeyfinder.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyFinderDetector;

impl KeyDetector for KeyFinderDetector {
    fn name(&self) -> &'static str {
        "libkeyfinder"
    }

    fn detect_key(&self, audio: &AudioBuffer) -> Result<Option<Key>, EuphoriaError> {
        // SAFETY: the pointer and length describe a live slice, and the
        // wrapper only reads from it for the duration of the call.
        let code = unsafe {
            euphoria_keyfinder_detect(
                audio.samples.as_ptr(),
                audio.samples.len() as c_ulong,
                audio.sample_rate as c_uint,
            )
        };
        key_from_code(code)
    }
}

/// libkeyfinder numbers keys from A major upwards by semitone, with the
/// major key at the even index and its parallel minor at the odd one.
fn key_from_code(code: c_int) -> Result<Option<Key>, EuphoriaError> {
    match code {
        SILENCE_CODE => Ok(None),
        0..SILENCE_CODE => {
            let tonic = (A_PITCH_CLASS + code / 2).rem_euclid(12) as u8;
            let mode = if code % 2 == 0 {
                Mode::Major
            } else {
                Mode::Minor
            };
            Ok(Some(Key::new(tonic, mode)))
        }
        _ => Err(EuphoriaError::Detection {
            detector: "libkeyfinder",
            reason: format!("unexpected result code {code}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_library_codes_to_keys() {
        assert_eq!(key_from_code(0).unwrap(), Some("A major".parse().unwrap()));
        assert_eq!(key_from_code(1).unwrap(), Some("A minor".parse().unwrap()));
        assert_eq!(key_from_code(6).unwrap(), Some("C major".parse().unwrap()));
        assert_eq!(
            key_from_code(23).unwrap(),
            Some("Ab minor".parse().unwrap())
        );
        assert_eq!(key_from_code(24).unwrap(), None);
        assert!(key_from_code(-1).is_err());
    }

    #[test]
    fn hears_a_minor_in_an_a_minor_chord() {
        let sample_rate = 44_100_u32;
        let chord_frequencies = [220.0_f32, 261.63, 329.63, 440.0];
        let samples: Vec<f32> = (0..sample_rate * 20)
            .map(|index| {
                let time = index as f32 / sample_rate as f32;
                chord_frequencies
                    .iter()
                    .map(|frequency| (std::f32::consts::TAU * frequency * time).sin())
                    .sum::<f32>()
                    / chord_frequencies.len() as f32
            })
            .collect();
        let audio = AudioBuffer {
            samples,
            sample_rate,
        };
        let detected = KeyFinderDetector.detect_key(&audio).unwrap();
        assert_eq!(detected, Some("A minor".parse().unwrap()));
    }

    #[test]
    fn silence_has_no_key() {
        let silence = AudioBuffer {
            samples: vec![0.0; 44_100 * 2],
            sample_rate: 44_100,
        };
        assert_eq!(KeyFinderDetector.detect_key(&silence).unwrap(), None);
    }
}
