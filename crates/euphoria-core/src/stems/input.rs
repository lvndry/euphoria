// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Decoding a file to the stereo audio the network takes: two channels at
//! its sample rate.

use std::path::Path;

use rubato::audioadapter::Adapter;
use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Fft, FixedSync, Resampler};

use crate::audio_stream::AudioStream;
use crate::error::EuphoriaError;

const RESAMPLER_CHUNK_FRAMES: usize = 4096;

/// Decodes the file at `path` into a left and a right channel at
/// `sample_rate`. Mono is played on both sides; of more than two channels,
/// the first two are kept, as Demucs does.
pub(crate) fn decode_stereo(path: &Path, sample_rate: u32) -> Result<[Vec<f32>; 2], EuphoriaError> {
    let mut stream = AudioStream::open(path)?;
    let channel_count = usize::from(stream.channel_count().get());
    let source_rate = stream.sample_rate().get();
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut interleaved = Vec::new();
    while stream.read_chunk(&mut interleaved)? {
        for frame in interleaved.chunks_exact(channel_count) {
            left.push(frame[0]);
            right.push(frame[if channel_count > 1 { 1 } else { 0 }]);
        }
    }
    if left.is_empty() {
        return Err(EuphoriaError::EmptyAudio {
            path: path.to_path_buf(),
        });
    }
    if source_rate == sample_rate {
        return Ok([left, right]);
    }
    resample([left, right], source_rate, sample_rate)
}

fn resample(
    channels: [Vec<f32>; 2],
    source_rate: u32,
    target_rate: u32,
) -> Result<[Vec<f32>; 2], EuphoriaError> {
    let resample_error = |reason: String| EuphoriaError::Separation {
        reason: format!("cannot resample from {source_rate} Hz to {target_rate} Hz: {reason}"),
        source: None,
    };
    let frames = channels[0].len();
    let mut resampler = Fft::<f32>::new(
        source_rate as usize,
        target_rate as usize,
        RESAMPLER_CHUNK_FRAMES,
        2,
        FixedSync::Input,
    )
    .map_err(|error| resample_error(error.to_string()))?;
    let input = SequentialSliceOfVecs::new(&channels, 2, frames)
        .map_err(|error| resample_error(error.to_string()))?;
    let output = resampler
        .process_all(&input, frames, None)
        .map_err(|error| resample_error(error.to_string()))?;
    let resampled_frames = output.frames();
    let samples = output.take_data();
    let mut left = Vec::with_capacity(resampled_frames);
    let mut right = Vec::with_capacity(resampled_frames);
    for frame in samples.chunks_exact(2) {
        left.push(frame[0]);
        right.push(frame[1]);
    }
    Ok([left, right])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_keeps_the_duration_and_the_tone() {
        let source_rate = 48_000;
        let frequency = 1000.0f32;
        let tone: Vec<f32> = (0..source_rate)
            .map(|index| {
                (2.0 * std::f32::consts::PI * frequency * index as f32 / source_rate as f32).sin()
            })
            .collect();
        let [left, right] = resample([tone.clone(), tone], source_rate, 44_100).unwrap();
        assert!((left.len() as i64 - 44_100).abs() <= 1, "{}", left.len());
        assert_eq!(left, right);
        let middle = 22_050;
        let expected = (2.0 * std::f32::consts::PI * frequency * middle as f32 / 44_100.0).sin();
        assert!(
            (left[middle] - expected).abs() < 1e-2,
            "{} against {expected}",
            left[middle]
        );
    }
}
