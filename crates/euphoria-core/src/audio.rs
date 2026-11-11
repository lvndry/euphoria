// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::File;
use std::num::NonZeroU32;
use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, TrackType};
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
        is_silent(&self.samples)
    }
}

const SILENCE_THRESHOLD: f32 = 1.0e-5;

/// The most samples reserved up front, one hour at 48 kHz. Longer audio
/// grows the buffer as it decodes.
const MAXIMUM_RESERVED_FRAMES: u64 = 60 * 60 * 48_000;

/// The most samples reserved up front for each byte of the file. Common
/// files hold fewer: 16 bit stereo WAV holds 0.25 frames in a byte, a
/// 128 kbit/s MP3 at 44.1 kHz about 2.8. A file that holds more, such as a
/// very low bit rate one, grows the buffer past the reservation.
const MAXIMUM_RESERVED_FRAMES_PER_BYTE: u64 = 4;

/// How many samples to reserve before decoding. Reserving the whole length
/// at once keeps the buffer from being reallocated as it grows, which leaves
/// released memory resident across the tracks of a batch.
///
/// The length a file declares is not trusted on its own, since a truncated
/// or damaged file can declare hours of audio. It is bounded by what a file
/// of that size on disk could plausibly hold.
fn frames_to_reserve(declared_frames: Option<u64>, file_bytes: Option<u64>) -> usize {
    let plausible_frames = file_bytes
        .unwrap_or(0)
        .saturating_mul(MAXIMUM_RESERVED_FRAMES_PER_BYTE);
    let frames = declared_frames
        .unwrap_or(0)
        .min(plausible_frames)
        .min(MAXIMUM_RESERVED_FRAMES);
    usize::try_from(frames).unwrap_or(0)
}

/// True when no sample rises above roughly -100 dBFS.
pub(crate) fn is_silent(samples: &[f32]) -> bool {
    samples
        .iter()
        .all(|sample| sample.abs() < SILENCE_THRESHOLD)
}

/// The lowest sample rate Euphoria analyzes, in samples per second.
pub const MINIMUM_SAMPLE_RATE: u32 = 8_000;
/// The highest sample rate Euphoria analyzes, in samples per second.
///
/// Audio files run from 8 kHz (telephone quality) to 384 kHz (the highest
/// rate in use for high resolution audio). The working memory of every
/// analysis grows with the sample rate, so a file that declares a rate
/// outside this range, which only a damaged file does, is refused when it is
/// decoded.
pub const MAXIMUM_SAMPLE_RATE: u32 = 384_000;

/// Decodes a whole audio file and averages its channels into mono.
pub fn decode_mono(path: &Path) -> Result<AudioBuffer, EuphoriaError> {
    let decode_error = |error: SymphoniaError| decode_error(path, error);
    let missing = |failure: DecodeFailure| EuphoriaError::Decode {
        path: path.to_path_buf(),
        failure,
        source: None,
    };

    let mut format = open_format(path)?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| missing(DecodeFailure::NoAudioTrack))?;
    let track_id = track.id;
    let declared_frames = track.num_frames;
    let codec_parameters = track
        .codec_params
        .as_ref()
        .and_then(|parameters| parameters.audio())
        .ok_or_else(|| missing(DecodeFailure::NoAudioTrack))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(codec_parameters, &AudioDecoderOptions::default())
        .map_err(decode_error)?;

    let file_bytes = std::fs::metadata(path).ok().map(|metadata| metadata.len());
    let mut mono_samples: Vec<f32> =
        Vec::with_capacity(frames_to_reserve(declared_frames, file_bytes));
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
                if !(MINIMUM_SAMPLE_RATE..=MAXIMUM_SAMPLE_RATE).contains(&sample_rate) {
                    return Err(EuphoriaError::UnsupportedSampleRate {
                        sample_rate,
                        minimum: MINIMUM_SAMPLE_RATE,
                        maximum: MAXIMUM_SAMPLE_RATE,
                    });
                }
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
    if mono_samples.capacity() / 2 > mono_samples.len() {
        mono_samples.shrink_to_fit();
    }
    Ok(AudioBuffer::new(mono_samples, sample_rate))
}

/// Opens a file and finds the reader for its container. Tags met on the way
/// are kept in the reader's metadata.
pub(crate) fn open_format(path: &Path) -> Result<Box<dyn FormatReader>, EuphoriaError> {
    let file = File::open(path).map_err(|source| EuphoriaError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        hint.with_extension(extension);
    }

    symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|error| decode_error(path, error))
}

pub(crate) fn decode_error(path: &Path, error: SymphoniaError) -> EuphoriaError {
    EuphoriaError::Decode {
        path: path.to_path_buf(),
        failure: classify(&error),
        source: Some(Box::new(error)),
    }
}

fn classify(error: &SymphoniaError) -> DecodeFailure {
    match error {
        SymphoniaError::Unsupported(_) => DecodeFailure::UnsupportedFormat,
        SymphoniaError::IoError(_) => DecodeFailure::Read,
        _ => DecodeFailure::Malformed,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    const ONE_HOUR_AT_44_1_KHZ: u64 = 60 * 60 * 44_100;

    #[test]
    fn a_plausible_declared_length_is_reserved_whole() {
        let six_minutes = 6 * 60 * 44_100;
        let file_bytes = 12_000_000;
        assert_eq!(
            frames_to_reserve(Some(six_minutes), Some(file_bytes)),
            six_minutes as usize
        );
    }

    #[test]
    fn a_declared_length_the_file_cannot_hold_is_bounded_by_the_file_size() {
        assert_eq!(
            frames_to_reserve(Some(ONE_HOUR_AT_44_1_KHZ), Some(50_000)),
            200_000
        );
        assert_eq!(frames_to_reserve(Some(u64::MAX), Some(1_000)), 4_000);
    }

    #[test]
    fn nothing_is_reserved_without_a_declared_length_or_a_file_size() {
        assert_eq!(frames_to_reserve(None, Some(12_000_000)), 0);
        assert_eq!(frames_to_reserve(Some(ONE_HOUR_AT_44_1_KHZ), None), 0);
    }

    #[test]
    fn the_reservation_never_exceeds_one_hour_at_48_khz() {
        assert_eq!(
            frames_to_reserve(Some(u64::MAX), Some(u64::MAX)),
            MAXIMUM_RESERVED_FRAMES as usize
        );
    }

    /// The one second WAV fixture with a header that declares an hour of
    /// samples.
    fn wav_declaring_an_hour() -> PathBuf {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sine-440hz-mono.wav");
        let mut bytes = std::fs::read(fixture).unwrap();
        let data_chunk = bytes
            .windows(4)
            .position(|window| window == b"data")
            .unwrap();
        let bytes_per_frame = u64::from(u16::from_le_bytes([bytes[32], bytes[33]]));
        let declared_bytes = u32::try_from(ONE_HOUR_AT_44_1_KHZ * bytes_per_frame).unwrap();
        bytes[data_chunk + 4..data_chunk + 8].copy_from_slice(&declared_bytes.to_le_bytes());
        let riff_bytes = declared_bytes + data_chunk as u32;
        bytes[4..8].copy_from_slice(&riff_bytes.to_le_bytes());
        let path = std::env::temp_dir().join(format!(
            "euphoria-declares-an-hour-{}.wav",
            std::process::id()
        ));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn a_short_file_declaring_a_huge_length_reserves_little_and_decodes() {
        let path = wav_declaring_an_hour();
        let file_bytes = std::fs::metadata(&path).unwrap().len();
        let declared_frames = open_format(&path)
            .unwrap()
            .default_track(TrackType::Audio)
            .unwrap()
            .num_frames;

        let audio = decode_mono(&path);
        std::fs::remove_file(&path).unwrap();
        let audio = audio.unwrap();

        assert_eq!(declared_frames, Some(ONE_HOUR_AT_44_1_KHZ));
        let reserved = frames_to_reserve(declared_frames, Some(file_bytes));
        let ten_seconds = 10 * 44_100;
        assert!(reserved < ten_seconds, "{reserved} samples reserved");
        assert!((audio.duration_seconds() - 1.0).abs() < 0.01);
        let two_seconds = 2 * 44_100;
        assert!(audio.samples.capacity() < two_seconds);
    }
}
