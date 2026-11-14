// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Opening the computer's audio output: how much sound waits in its buffer,
//! and what is done when the system reports that the buffer ran dry.
//!
//! The output asks for sound in blocks of `buffer frames`. The decks and
//! the player must answer each ask within one block's duration, so a bigger
//! block gives them more time before the sound breaks up, and costs the
//! same amount of delay between a command and what is heard. The mix of the
//! decks and the position of each track count frames, never time, so the
//! delay moves nothing relative: both decks sound that much later.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rodio::cpal::traits::HostTrait;
use rodio::cpal::{BufferSize, SupportedBufferSize};
use rodio::{CpalError, CpalErrorKind, DeviceSinkBuilder, DeviceSinkError, MixerDeviceSink};

/// How much sound one block of the output holds, in milliseconds. The
/// output hands the player its blocks about this far apart.
pub(crate) const BUFFER_MILLISECONDS: u32 = 40;

/// How often an underrun is reported as a warning rather than as a debug
/// message after the first one, so a struggling machine does not fill the
/// terminal.
const WARN_EVERY: u64 = 10;

/// The block size to ask the device for: the frames in `milliseconds` at
/// `sample_rate`, rounded up to a power of two and kept within what the
/// device supports.
pub(crate) fn choose_buffer_size(
    sample_rate: u32,
    milliseconds: u32,
    supported: &SupportedBufferSize,
) -> BufferSize {
    let wanted = u64::from(sample_rate) * u64::from(milliseconds) / 1000;
    let wanted = u32::try_from(wanted)
        .unwrap_or(u32::MAX / 2)
        .max(1)
        .next_power_of_two();
    match supported {
        SupportedBufferSize::Range { min, max } => {
            BufferSize::Fixed(wanted.max(*min).min((*max).max(*min)))
        }
        SupportedBufferSize::Unknown => BufferSize::Fixed(wanted),
    }
}

/// How many times the system has said the output ran dry.
#[derive(Clone, Default)]
pub(crate) struct Underruns(Arc<AtomicU64>);

impl Underruns {
    #[cfg(test)]
    pub(crate) fn count(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    pub(crate) fn error_callback(&self) -> impl FnMut(CpalError) + Send + 'static {
        let underruns = self.clone();
        move |error| underruns.report(&error)
    }

    fn report(&self, error: &CpalError) {
        if error.kind() != CpalErrorKind::Xrun {
            log::error!(target: "audio", "output stream error: {error}");
            return;
        }
        let count = self.0.fetch_add(1, Ordering::Relaxed) + 1;
        if count == 1 || count.is_multiple_of(WARN_EVERY) {
            log::warn!(target: "audio", "the output ran short of sound ({count} so far)");
        } else {
            log::debug!(target: "audio", "the output ran short of sound ({count} so far)");
        }
    }
}

/// The default output with a block of `frames` (or the output's own choice
/// when `None`), reporting problems to `underruns`.
pub(crate) fn open_with_buffer(
    frames: Option<u32>,
    underruns: &Underruns,
) -> Result<MixerDeviceSink, DeviceSinkError> {
    let mut builder = DeviceSinkBuilder::from_default_device()?;
    if let Some(frames) = frames {
        builder = builder.with_buffer_size(BufferSize::Fixed(frames));
    }
    builder
        .with_error_callback(underruns.error_callback())
        .open_stream()
}

/// The default output with a block of [`BUFFER_MILLISECONDS`]. When the
/// device refuses that size, the output's own choice is used, and when
/// that fails too, whatever output opens.
pub(crate) fn open_output(underruns: &Underruns) -> Result<MixerDeviceSink, DeviceSinkError> {
    let wanted = wanted_frames();
    match open_with_buffer(wanted, underruns) {
        Ok(sink) => Ok(sink),
        Err(refusal) => {
            log::warn!(target: "audio", "the output refused a {wanted:?} frame buffer: {refusal}");
            open_with_buffer(None, underruns).or_else(|_| DeviceSinkBuilder::open_default_sink())
        }
    }
}

fn wanted_frames() -> Option<u32> {
    let device = rodio::cpal::default_host().default_output_device()?;
    let config = rodio::DeviceTrait::default_output_config(&device).ok()?;
    match choose_buffer_size(
        config.sample_rate(),
        BUFFER_MILLISECONDS,
        config.buffer_size(),
    ) {
        BufferSize::Fixed(frames) => Some(frames),
        BufferSize::Default => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(size: BufferSize) -> u32 {
        match size {
            BufferSize::Fixed(frames) => frames,
            BufferSize::Default => panic!("expected a fixed size"),
        }
    }

    #[test]
    fn the_size_is_a_power_of_two_covering_the_wanted_time() {
        let range = SupportedBufferSize::Range { min: 14, max: 4096 };
        assert_eq!(frames(choose_buffer_size(44_100, 40, &range)), 2048);
        assert_eq!(frames(choose_buffer_size(48_000, 20, &range)), 1024);
        assert_eq!(frames(choose_buffer_size(48_000, 10, &range)), 512);
    }

    #[test]
    fn the_size_stays_within_what_the_device_supports() {
        let narrow = SupportedBufferSize::Range { min: 64, max: 1024 };
        assert_eq!(frames(choose_buffer_size(48_000, 90, &narrow)), 1024);
        let large_minimum = SupportedBufferSize::Range {
            min: 512,
            max: 8192,
        };
        assert_eq!(frames(choose_buffer_size(48_000, 1, &large_minimum)), 512);
    }

    #[test]
    fn an_unknown_range_is_asked_for_the_wanted_size() {
        let size = choose_buffer_size(44_100, 90, &SupportedBufferSize::Unknown);
        assert_eq!(frames(size), 4096);
    }

    #[test]
    fn a_broken_range_does_not_panic() {
        let backwards = SupportedBufferSize::Range { min: 2048, max: 64 };
        assert_eq!(frames(choose_buffer_size(48_000, 90, &backwards)), 2048);
    }

    #[test]
    fn only_overruns_are_counted() {
        let underruns = Underruns::default();
        let mut callback = underruns.error_callback();
        callback(CpalErrorKind::Xrun.into());
        callback(CpalErrorKind::Xrun.into());
        callback(CpalErrorKind::DeviceNotAvailable.into());
        assert_eq!(underruns.count(), 2);
    }
}
