// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_uint, c_ulong};

use crate::audio::AudioBuffer;
use crate::detect::KeyDetector;
use crate::error::EuphoriaError;
use crate::key::{Key, Mode};

unsafe extern "C" {
    fn euphoria_keyfinder_detect(
        samples: *const f32,
        sample_count: c_ulong,
        frame_rate: c_uint,
        error_message: *mut c_char,
        error_capacity: c_ulong,
    ) -> c_int;
}

const DETECTOR_NAME: &str = "libkeyfinder";
const SILENCE_CODE: c_int = 24;
const A_PITCH_CLASS: i32 = 9;
const ERROR_MESSAGE_CAPACITY: usize = 512;

/// Key detection through libkeyfinder.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyFinderDetector;

impl KeyDetector for KeyFinderDetector {
    fn name(&self) -> &'static str {
        DETECTOR_NAME
    }

    fn detect_key(&self, audio: &AudioBuffer) -> Result<Option<Key>, EuphoriaError> {
        let samples = audio.samples();
        // libkeyfinder counts samples in an unsigned int.
        let sample_count = c_uint::try_from(samples.len())
            .map_err(|_| detection_error("the track is too long for libkeyfinder".to_string()))?;
        let mut error_message = [0 as c_char; ERROR_MESSAGE_CAPACITY];
        // SAFETY: `samples` is a live slice of `sample_count` floats that the
        // wrapper only reads during the call, and `error_message` is a writable
        // buffer of the stated capacity that the wrapper always NUL terminates.
        let code = unsafe {
            euphoria_keyfinder_detect(
                samples.as_ptr(),
                c_ulong::from(sample_count),
                audio.sample_rate().get(),
                error_message.as_mut_ptr(),
                ERROR_MESSAGE_CAPACITY as c_ulong,
            )
        };
        if code < 0 {
            // SAFETY: the wrapper wrote a NUL terminated string into the buffer.
            let message = unsafe { CStr::from_ptr(error_message.as_ptr()) };
            return Err(detection_error(message.to_string_lossy().into_owned()));
        }
        key_from_code(code)
    }
}

fn detection_error(reason: String) -> EuphoriaError {
    EuphoriaError::Detection {
        detector: DETECTOR_NAME,
        reason,
        source: None,
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
        _ => Err(detection_error(format!("unexpected result code {code}"))),
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
        assert!(key_from_code(25).is_err());
    }
}
