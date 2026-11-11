// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The chromagram libkeyfinder computes for a track, and its scoring of
//! chroma vectors against the 24 keys.

use std::ffi::CStr;
use std::ops::Range;
use std::os::raw::{c_char, c_int, c_uint, c_ulong};

use crate::error::EuphoriaError;

/// Keys libkeyfinder scores a chroma vector against.
pub(crate) const KEY_COUNT: usize = 24;

pub(crate) const DETECTOR_NAME: &str = "libkeyfinder";
const ERROR_MESSAGE_CAPACITY: usize = 512;

#[repr(C)]
struct RawChromagram {
    _opaque: [u8; 0],
}

unsafe extern "C" {
    fn euphoria_keyfinder_chromagram_new(
        samples: *const f32,
        sample_count: c_ulong,
        frame_rate: c_uint,
        error_message: *mut c_char,
        error_capacity: c_ulong,
    ) -> *mut RawChromagram;
    fn euphoria_keyfinder_chromagram_hop_count(chromagram: *const RawChromagram) -> c_ulong;
    fn euphoria_keyfinder_chromagram_samples_per_hop(chromagram: *const RawChromagram) -> c_ulong;
    fn euphoria_keyfinder_chromagram_copy(chromagram: *const RawChromagram, magnitudes: *mut f64);
    fn euphoria_keyfinder_chromagram_free(chromagram: *mut RawChromagram);
    fn euphoria_keyfinder_band_count() -> c_ulong;
    fn euphoria_keyfinder_key_scores(
        chroma_vectors: *const f64,
        vector_count: c_ulong,
        scores: *mut f64,
        error_message: *mut c_char,
        error_capacity: c_ulong,
    ) -> c_int;
}

/// Frees the chromagram the wrapper allocated when it goes out of scope.
struct OwnedChromagram(*mut RawChromagram);

impl Drop for OwnedChromagram {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `euphoria_keyfinder_chromagram_new`,
        // is not null, and is freed only here.
        unsafe { euphoria_keyfinder_chromagram_free(self.0) }
    }
}

/// How strongly each note of six octaves sounds in each analysis frame of a
/// track. Frames overlap: each is four hops long.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Chromagram {
    magnitudes: Vec<f64>,
    band_count: usize,
    samples_per_hop: usize,
}

impl Chromagram {
    /// Computes the chromagram of mono samples. `frame_rate` is the sample
    /// rate libkeyfinder is told, which sets the frequency it takes each
    /// note to have.
    pub(crate) fn of_samples(samples: &[f32], frame_rate: u32) -> Result<Self, EuphoriaError> {
        // libkeyfinder counts samples in an unsigned int.
        let sample_count = c_uint::try_from(samples.len())
            .map_err(|_| detection_error("the track is too long for libkeyfinder".to_string()))?;
        let mut error_message = [0 as c_char; ERROR_MESSAGE_CAPACITY];
        // SAFETY: `samples` is a live slice of `sample_count` floats that the
        // wrapper only reads during the call, and `error_message` is a writable
        // buffer of the stated capacity that the wrapper NUL terminates when
        // it returns null.
        let raw = unsafe {
            euphoria_keyfinder_chromagram_new(
                samples.as_ptr(),
                c_ulong::from(sample_count),
                frame_rate,
                error_message.as_mut_ptr(),
                ERROR_MESSAGE_CAPACITY as c_ulong,
            )
        };
        if raw.is_null() {
            return Err(error_from_message(&error_message));
        }
        let owned = OwnedChromagram(raw);
        // SAFETY: `owned.0` is a live chromagram, and `magnitudes` has room
        // for the hop count times the band count values the wrapper copies.
        unsafe {
            let hop_count = euphoria_keyfinder_chromagram_hop_count(owned.0) as usize;
            let band_count = euphoria_keyfinder_band_count() as usize;
            let mut magnitudes = vec![0.0_f64; hop_count * band_count];
            euphoria_keyfinder_chromagram_copy(owned.0, magnitudes.as_mut_ptr());
            Ok(Self {
                magnitudes,
                band_count,
                samples_per_hop: euphoria_keyfinder_chromagram_samples_per_hop(owned.0) as usize,
            })
        }
    }

    /// Number of analysis frames.
    pub(crate) fn hop_count(&self) -> usize {
        self.magnitudes.len() / self.band_count
    }

    /// Input samples between the starts of two frames.
    pub(crate) fn samples_per_hop(&self) -> usize {
        self.samples_per_hop
    }

    /// The mean chroma vector of a run of frames, as libkeyfinder averages
    /// them for a whole track. All zero for an empty run.
    pub(crate) fn mean_of_hops(&self, hops: Range<usize>) -> Vec<f64> {
        let mut mean = vec![0.0_f64; self.band_count];
        let hop_count = hops.len() as f64;
        for hop in hops {
            let frame = &self.magnitudes[hop * self.band_count..(hop + 1) * self.band_count];
            for (total, magnitude) in mean.iter_mut().zip(frame) {
                *total += magnitude / hop_count;
            }
        }
        mean
    }
}

/// Scores chroma vectors, laid end to end, against every key. Each vector
/// gets [`KEY_COUNT`] scores from 0 to 1 in libkeyfinder key code order. A
/// vector with no signal scores zero for every key.
pub(crate) fn key_scores(chroma_vectors: &[f64]) -> Result<Vec<[f64; KEY_COUNT]>, EuphoriaError> {
    // SAFETY: the function takes no arguments and returns a constant.
    let band_count = unsafe { euphoria_keyfinder_band_count() } as usize;
    let vector_count = chroma_vectors.len() / band_count;
    let mut scores = vec![[0.0_f64; KEY_COUNT]; vector_count];
    let mut error_message = [0 as c_char; ERROR_MESSAGE_CAPACITY];
    // SAFETY: the wrapper reads `vector_count` vectors of `band_count` values,
    // which `chroma_vectors` holds, and writes `KEY_COUNT` scores for each
    // into `scores`, which has room for them. `error_message` is a writable
    // buffer of the stated capacity.
    let code = unsafe {
        euphoria_keyfinder_key_scores(
            chroma_vectors.as_ptr(),
            vector_count as c_ulong,
            scores.as_mut_ptr().cast::<f64>(),
            error_message.as_mut_ptr(),
            ERROR_MESSAGE_CAPACITY as c_ulong,
        )
    };
    if code < 0 {
        return Err(error_from_message(&error_message));
    }
    Ok(scores)
}

fn error_from_message(error_message: &[c_char; ERROR_MESSAGE_CAPACITY]) -> EuphoriaError {
    // SAFETY: the wrapper wrote a NUL terminated string into the buffer.
    let message = unsafe { CStr::from_ptr(error_message.as_ptr()) };
    detection_error(message.to_string_lossy().into_owned())
}

pub(crate) fn detection_error(reason: String) -> EuphoriaError {
    EuphoriaError::Detection {
        detector: DETECTOR_NAME,
        reason,
        source: None,
    }
}
