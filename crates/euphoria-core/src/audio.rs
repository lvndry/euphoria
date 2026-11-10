// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::File;
use std::num::NonZeroU32;
use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::error::{DecodeFailure, EuphoriaError};

/// Decoded audio mixed down to one channel.
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    samples: Vec<f32>,
    sample_rate: NonZeroU32,
}

impl AudioBuffer {
    /// Wraps mono samples, nominally in the range -1.0 to 1.0.
    pub fn new(samples: Vec<f32>, sample_rate: NonZeroU32) -> Self {
        Self {
            samples,
            sample_rate,
        }
    }

    /// The mono samples.
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Samples per second.
    pub fn sample_rate(&self) -> NonZeroU32 {
        self.sample_rate
    }

    /// Length of the audio in seconds.
    pub fn duration_seconds(&self) -> f32 {
        self.samples.len() as f32 / self.sample_rate.get() as f32
    }

    /// True when no sample rises above roughly -100 dBFS, so there is nothing
    /// to analyze.
    pub fn is_silent(&self) -> bool {
        self.samples
            .iter()
            .all(|sample| sample.abs() < SILENCE_THRESHOLD)
    }
}

const SILENCE_THRESHOLD: f32 = 1.0e-5;

/// Decodes a whole audio file and averages its channels into mono.
pub fn decode_mono(path: &Path) -> Result<AudioBuffer, EuphoriaError> {
    let decode_error = |error: SymphoniaError| EuphoriaError::Decode {
        path: path.to_path_buf(),
        failure: classify(&error),
        source: Some(Box::new(error)),
    };
    let missing = |failure: DecodeFailure| EuphoriaError::Decode {
        path: path.to_path_buf(),
        failure,
        source: None,
    };

    let file = File::open(path).map_err(|source| EuphoriaError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        hint.with_extension(extension);
    }

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(decode_error)?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| missing(DecodeFailure::NoAudioTrack))?;
    let track_id = track.id;
    let codec_parameters = track
        .codec_params
        .as_ref()
        .and_then(|parameters| parameters.audio())
        .ok_or_else(|| missing(DecodeFailure::NoAudioTrack))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(codec_parameters, &AudioDecoderOptions::default())
        .map_err(decode_error)?;

    let mut mono_samples: Vec<f32> = Vec::new();
    let mut interleaved: Vec<f32> = Vec::new();
    let mut sample_rate: u32 = 0;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(error) => return Err(decode_error(error)),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                sample_rate = decoded.spec().rate();
                let channel_count = decoded.num_planes().max(1);
                decoded.copy_to_vec_interleaved(&mut interleaved);
                mono_samples.extend(
                    interleaved
                        .chunks_exact(channel_count)
                        .map(|frame| frame.iter().sum::<f32>() / channel_count as f32),
                );
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(decode_error(error)),
        }
    }

    let sample_rate = NonZeroU32::new(sample_rate).filter(|_| !mono_samples.is_empty());
    let Some(sample_rate) = sample_rate else {
        return Err(EuphoriaError::EmptyAudio {
            path: path.to_path_buf(),
        });
    };
    Ok(AudioBuffer::new(mono_samples, sample_rate))
}

fn classify(error: &SymphoniaError) -> DecodeFailure {
    match error {
        SymphoniaError::Unsupported(_) => DecodeFailure::UnsupportedFormat,
        SymphoniaError::IoError(_) => DecodeFailure::Read,
        _ => DecodeFailure::Malformed,
    }
}
