// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::File;
use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::error::EuphoriaError;

/// Decoded audio mixed down to one channel.
#[derive(Debug, Clone)]
pub struct AudioBuffer {
    /// Mono samples in the range -1.0 to 1.0.
    pub samples: Vec<f32>,
    /// Samples per second.
    pub sample_rate: u32,
}

impl AudioBuffer {
    /// Length of the audio in seconds.
    pub fn duration_seconds(&self) -> f32 {
        self.samples.len() as f32 / self.sample_rate as f32
    }
}

/// Decodes a whole audio file and averages its channels into mono.
pub fn decode_mono(path: &Path) -> Result<AudioBuffer, EuphoriaError> {
    let decode_error = |reason: String| EuphoriaError::Decode {
        path: path.to_path_buf(),
        reason,
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
        .map_err(|error| decode_error(error.to_string()))?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| decode_error("no audio track".to_string()))?;
    let track_id = track.id;
    let codec_parameters = track
        .codec_params
        .as_ref()
        .and_then(|parameters| parameters.audio())
        .ok_or_else(|| decode_error("no audio codec parameters".to_string()))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(codec_parameters, &AudioDecoderOptions::default())
        .map_err(|error| decode_error(error.to_string()))?;

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
            Err(error) => return Err(decode_error(error.to_string())),
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
            Err(error) => return Err(decode_error(error.to_string())),
        }
    }

    if mono_samples.is_empty() || sample_rate == 0 {
        return Err(EuphoriaError::EmptyAudio {
            path: path.to_path_buf(),
        });
    }

    Ok(AudioBuffer {
        samples: mono_samples,
        sample_rate,
    })
}
