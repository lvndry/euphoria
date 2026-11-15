// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A stretch of a track as it is and as it would be prepared, made in
//! memory for listening to both, at the same loudness, so the comparison
//! is about the sound and not about the level.

use std::path::Path;

use ebur128::{EbuR128, Mode};

use super::limiter::TruePeakLimiter;
use super::{LIMITER_MARGIN_DB, PrepareSettings, TrackPlan, failure};
use crate::audio_stream::AudioStream;
use crate::error::EuphoriaError;
use crate::shift::PitchShifter;

/// A stretch of a track, twice.
#[derive(Debug, Clone)]
pub struct ComparisonClip {
    /// Frames per second.
    pub sample_rate: u32,
    /// Interleaved channels.
    pub channels: u16,
    /// The original, turned to the loudness of the prepared version.
    pub original: Vec<f32>,
    /// The prepared version: gain, limiter and tuning correction of the
    /// plan, as the written copy would have them.
    pub prepared: Vec<f32>,
    /// Where in the track the clip starts, in seconds.
    pub start_seconds: f32,
}

fn segment_lufs(samples: &[f32], channels: u16, sample_rate: u32) -> Option<f32> {
    let mut meter = EbuR128::new(u32::from(channels), sample_rate, Mode::I).ok()?;
    meter.add_frames_f32(samples).ok()?;
    let loudness = meter.loudness_global().ok()?;
    loudness.is_finite().then_some(loudness as f32)
}

/// Makes the comparison of `seconds` of the track at `input`, from
/// `start_seconds`, for `plan` made with `settings`. Nothing is written.
pub fn comparison_clip(
    input: &Path,
    plan: &TrackPlan,
    settings: &PrepareSettings,
    start_seconds: f32,
    seconds: f32,
) -> Result<ComparisonClip, EuphoriaError> {
    let mut stream = AudioStream::open(input)?;
    let sample_rate = stream.sample_rate().get();
    let channels = stream.channel_count().get();
    let channel_count = usize::from(channels);
    let start = if start_seconds > 0.0 {
        stream.seek(start_seconds)?
    } else {
        0.0
    };
    let wanted = (seconds * sample_rate as f32) as usize * channel_count;
    let mut original = Vec::with_capacity(wanted);
    let mut chunk = Vec::new();
    while original.len() < wanted && stream.read_chunk(&mut chunk)? {
        original.extend_from_slice(&chunk);
    }
    original.truncate(wanted / channel_count * channel_count);
    if original.is_empty() {
        return Err(failure(input, "there is no audio at that position"));
    }

    let gain = 10.0_f32.powf(plan.gain_db / 20.0);
    let mut shifted = match plan.tuning_correction_cents {
        Some(cents) => {
            let mut shifter = PitchShifter::with_cents(channel_count, sample_rate, cents)
                .map_err(|reason| failure(input, reason))?;
            let mut out = shifter
                .process(&original)
                .map_err(|reason| failure(input, reason))?;
            out.extend(shifter.finish().map_err(|reason| failure(input, reason))?);
            out
        }
        None => original.clone(),
    };
    for sample in &mut shifted {
        *sample *= gain;
    }
    let mut limiter = TruePeakLimiter::new(
        channel_count,
        sample_rate,
        settings.true_peak_ceiling_db - LIMITER_MARGIN_DB,
    );
    let mut prepared = limiter.push(&shifted);
    prepared.extend(limiter.finish());

    let match_gain = match (
        segment_lufs(&prepared, channels, sample_rate),
        segment_lufs(&original, channels, sample_rate),
    ) {
        (Some(prepared_lufs), Some(original_lufs)) => {
            10.0_f32.powf((prepared_lufs - original_lufs) / 20.0)
        }
        _ => gain,
    };
    for sample in &mut original {
        *sample *= match_gain;
    }
    let peak = original
        .iter()
        .chain(&prepared)
        .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
    if peak > 1.0 {
        for sample in original.iter_mut().chain(prepared.iter_mut()) {
            *sample /= peak;
        }
    }
    Ok(ComparisonClip {
        sample_rate,
        channels,
        original,
        prepared,
        start_seconds: start,
    })
}
