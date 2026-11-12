// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A safe wrapper around Signalsmith Stretch, the time stretcher and pitch
//! shifter vendored in `vendor/` and reached through `signalsmith_shim.cpp`.

use crate::shift::{deinterleave, interleave};
use std::ffi::{c_double, c_float, c_int};
use std::ptr::NonNull;

#[repr(C)]
struct RawStretch {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn euphoria_stretch_new(channels: c_int, sample_rate: c_float) -> *mut RawStretch;
    fn euphoria_stretch_free(stretch: *mut RawStretch);
    fn euphoria_stretch_reset(stretch: *mut RawStretch);
    fn euphoria_stretch_input_latency(stretch: *const RawStretch) -> c_int;
    fn euphoria_stretch_output_latency(stretch: *const RawStretch) -> c_int;
    fn euphoria_stretch_set_transpose_semitones(
        stretch: *mut RawStretch,
        semitones: c_float,
    ) -> c_int;
    fn euphoria_stretch_seek(
        stretch: *mut RawStretch,
        inputs: *const *const c_float,
        channels: c_int,
        input_samples: c_int,
        playback_rate: c_double,
    ) -> c_int;
    fn euphoria_stretch_process(
        stretch: *mut RawStretch,
        inputs: *const *const c_float,
        input_samples: c_int,
        outputs: *const *mut c_float,
        output_samples: c_int,
        channels: c_int,
    ) -> c_int;
    fn euphoria_stretch_flush(
        stretch: *mut RawStretch,
        outputs: *const *mut c_float,
        output_samples: c_int,
        channels: c_int,
    ) -> c_int;
}

/// One stretcher for a fixed number of channels. Audio is passed one slice
/// per channel; every channel of a call must hold the same number of
/// samples.
pub struct Stretch {
    raw: NonNull<RawStretch>,
    channels: usize,
}

// SAFETY: the stretcher holds only its own buffers, and every call takes `&mut self`
// or reads plain numbers, so it may move to another thread.
unsafe impl Send for Stretch {}

impl Stretch {
    /// A stretcher with Signalsmith's default settings for `channels`
    /// channels at `sample_rate`.
    pub fn new(channels: usize, sample_rate: u32) -> Result<Self, String> {
        let count = c_int::try_from(channels)
            .ok()
            .filter(|count| *count > 0)
            .ok_or_else(|| format!("cannot stretch audio with {channels} channels"))?;
        // SAFETY: the shim checks both numbers and returns null on failure.
        let raw = unsafe { euphoria_stretch_new(count, sample_rate as c_float) };
        NonNull::new(raw)
            .map(|raw| Self { raw, channels })
            .ok_or_else(|| "the time stretcher could not be created".to_string())
    }

    /// Samples per channel the stretcher needs before its first output.
    pub fn input_latency(&self) -> usize {
        // SAFETY: `raw` is a live stretcher created by the shim.
        usize::try_from(unsafe { euphoria_stretch_input_latency(self.raw.as_ptr()) }).unwrap_or(0)
    }

    /// Samples per channel by which the output trails the input.
    pub fn output_latency(&self) -> usize {
        // SAFETY: `raw` is a live stretcher created by the shim.
        usize::try_from(unsafe { euphoria_stretch_output_latency(self.raw.as_ptr()) }).unwrap_or(0)
    }

    /// Multiplies every frequency by `factor`, leaving the formants alone.
    pub fn set_transpose_factor(&mut self, factor: f32) -> Result<(), String> {
        // SAFETY: `raw` is a live stretcher created by the shim.
        status(unsafe {
            euphoria_stretch_set_transpose_semitones(self.raw.as_ptr(), 12.0 * factor.log2())
        })
    }

    /// Moves every pitch by `semitones`.
    pub fn set_transpose_semitones(&mut self, semitones: f32) -> Result<(), String> {
        // SAFETY: `raw` is a live stretcher created by the shim.
        status(unsafe { euphoria_stretch_set_transpose_semitones(self.raw.as_ptr(), semitones) })
    }

    /// Gives the stretcher audio that comes before what it processes next,
    /// without producing output.
    pub fn seek(&mut self, inputs: &[Vec<f32>], playback_rate: f64) -> Result<(), String> {
        let samples = self.samples_of(inputs)?;
        let pointers: Vec<*const c_float> = inputs.iter().map(|channel| channel.as_ptr()).collect();
        // SAFETY: one pointer per channel, each to `samples` readable values,
        // which outlive the call.
        status(unsafe {
            euphoria_stretch_seek(
                self.raw.as_ptr(),
                pointers.as_ptr(),
                self.channel_count(),
                samples,
                playback_rate,
            )
        })
    }

    /// Stretches `inputs` into `outputs`; their lengths set the ratio.
    pub fn process(&mut self, inputs: &[Vec<f32>], outputs: &mut [Vec<f32>]) -> Result<(), String> {
        let input_samples = self.samples_of(inputs)?;
        let output_samples = self.samples_of(outputs)?;
        let input_pointers: Vec<*const c_float> =
            inputs.iter().map(|channel| channel.as_ptr()).collect();
        let output_pointers: Vec<*mut c_float> = outputs
            .iter_mut()
            .map(|channel| channel.as_mut_ptr())
            .collect();
        // SAFETY: one pointer per channel, to `input_samples` readable and
        // `output_samples` writable values, which outlive the call.
        status(unsafe {
            euphoria_stretch_process(
                self.raw.as_ptr(),
                input_pointers.as_ptr(),
                input_samples,
                output_pointers.as_ptr(),
                output_samples,
                self.channel_count(),
            )
        })
    }

    /// Writes what the stretcher still holds into `outputs`.
    pub fn flush(&mut self, outputs: &mut [Vec<f32>]) -> Result<(), String> {
        let output_samples = self.samples_of(outputs)?;
        let pointers: Vec<*mut c_float> = outputs
            .iter_mut()
            .map(|channel| channel.as_mut_ptr())
            .collect();
        // SAFETY: one pointer per channel, to `output_samples` writable
        // values, which outlive the call.
        status(unsafe {
            euphoria_stretch_flush(
                self.raw.as_ptr(),
                pointers.as_ptr(),
                output_samples,
                self.channel_count(),
            )
        })
    }

    /// Forgets what it holds, as after a jump in the track.
    pub fn reset(&mut self) {
        // SAFETY: `raw` is a live stretcher created by the shim.
        unsafe { euphoria_stretch_reset(self.raw.as_ptr()) }
    }

    /// Stretches one block of interleaved `input` into interleaved
    /// `output`; the longer side sets the ratio the other is stretched to.
    pub fn process_interleaved(&mut self, input: &[f32], output: &mut [f32]) -> Result<(), String> {
        if !input.len().is_multiple_of(self.channels) || !output.len().is_multiple_of(self.channels)
        {
            return Err("an interleaved block ends mid-frame".to_string());
        }
        let inputs = deinterleave(input, self.channels);
        let mut outputs = vec![vec![0.0; output.len() / self.channels]; self.channels];
        let status = self.process(&inputs, &mut outputs);
        if status.is_ok() {
            let mixed = interleave(&outputs);
            output.copy_from_slice(&mixed);
        }
        status
    }

    fn channel_count(&self) -> c_int {
        c_int::try_from(self.channels).unwrap_or(c_int::MAX)
    }

    /// The samples per channel of `channels`, which must be one per channel
    /// of the stretcher and all of one length.
    fn samples_of(&self, channels: &[Vec<f32>]) -> Result<c_int, String> {
        if channels.len() != self.channels {
            return Err(format!(
                "{} channels given to a stretcher of {}",
                channels.len(),
                self.channels
            ));
        }
        let length = channels.first().map_or(0, Vec::len);
        if channels.iter().any(|channel| channel.len() != length) {
            return Err("channels of different lengths".to_string());
        }
        c_int::try_from(length).map_err(|_| format!("{length} samples is too many for one call"))
    }
}

impl Drop for Stretch {
    fn drop(&mut self) {
        // SAFETY: `raw` was created by the shim and is freed only here.
        unsafe { euphoria_stretch_free(self.raw.as_ptr()) }
    }
}

fn status(code: c_int) -> Result<(), String> {
    match code {
        0 => Ok(()),
        -1 => Err("the time stretcher was given invalid audio".to_string()),
        _ => Err("the time stretcher failed".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mismatched_channels_and_lengths_are_errors_and_not_undefined_behaviour() {
        let mut stretch = Stretch::new(2, 44_100).unwrap();
        assert!(stretch.seek(&[vec![0.0; 10]], 1.0).is_err());
        assert!(
            stretch
                .process(
                    &[vec![0.0; 10], vec![0.0; 9]],
                    &mut [vec![0.0; 10], vec![0.0; 10]]
                )
                .is_err()
        );
        assert!(stretch.flush(&mut [vec![0.0; 4]]).is_err());
        assert!(stretch.flush(&mut [vec![0.0; 4], vec![0.0; 4]]).is_ok());
        assert!(Stretch::new(0, 44_100).is_err());
        assert!(Stretch::new(1, 0).is_err());
        assert!(stretch.input_latency() > 0);
        assert!(stretch.output_latency() > 0);
    }
}
