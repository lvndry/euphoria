// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::num::{NonZeroU16, NonZeroU32};
use std::path::{Path, PathBuf};

use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::units::{Time, TimeBase};

use crate::audio::{MAXIMUM_SAMPLE_RATE, MINIMUM_SAMPLE_RATE, decode_error, open_format};
use crate::error::{DecodeFailure, EuphoriaError};

/// An audio file decoded a little at a time, for playing it.
///
/// Where [`decode_mono`](crate::decode_mono) reads a whole file before it
/// returns, a stream decodes one packet per call, a few hundredths of a
/// second of audio, so the first sound is ready as soon as the file is open.
/// The channels are kept as they are in the file.
pub struct AudioStream {
    path: PathBuf,
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    sample_rate: NonZeroU32,
    channel_count: NonZeroU16,
    duration_seconds: Option<f32>,
    /// The chunk decoded while opening, handed out by the first read.
    first_chunk: Option<Vec<f32>>,
    /// Frames to drop from the next chunks, so that playing resumes exactly
    /// where a seek asked for and not at the start of its packet.
    frames_to_skip: u64,
    /// True once the end of the file was read or seeked past.
    is_finished: bool,
}

impl std::fmt::Debug for AudioStream {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AudioStream")
            .field("path", &self.path)
            .field("sample_rate", &self.sample_rate)
            .field("channel_count", &self.channel_count)
            .field("duration_seconds", &self.duration_seconds)
            .finish_non_exhaustive()
    }
}

impl AudioStream {
    /// Opens the file and decodes its first packet, which is what tells the
    /// sample rate and the number of channels for certain.
    ///
    /// Fails as [`decode_mono`](crate::decode_mono) does: when the file cannot
    /// be opened, is not audio Euphoria can decode, or holds no audio.
    pub fn open(path: &Path) -> Result<Self, EuphoriaError> {
        let OpenTrack {
            mut format,
            mut decoder,
            track_id,
            time_base,
            declared_frames,
        } = open_track(path)?;

        let mut first_chunk = Vec::new();
        let first_layout = decode_next_packet(
            path,
            &mut *format,
            &mut *decoder,
            track_id,
            &mut first_chunk,
        )?;
        let Some(layout) = first_layout else {
            return Err(EuphoriaError::EmptyAudio {
                path: path.to_path_buf(),
            });
        };
        if !(MINIMUM_SAMPLE_RATE..=MAXIMUM_SAMPLE_RATE).contains(&layout.sample_rate.get()) {
            return Err(EuphoriaError::UnsupportedSampleRate {
                sample_rate: layout.sample_rate.get(),
                minimum: MINIMUM_SAMPLE_RATE,
                maximum: MAXIMUM_SAMPLE_RATE,
            });
        }

        Ok(Self {
            path: path.to_path_buf(),
            format,
            decoder,
            track_id,
            time_base,
            sample_rate: layout.sample_rate,
            channel_count: layout.channel_count,
            duration_seconds: declared_frames
                .map(|frames| frames as f32 / layout.sample_rate.get() as f32),
            first_chunk: Some(first_chunk),
            frames_to_skip: 0,
            is_finished: false,
        })
    }

    /// Samples per second of each channel.
    pub fn sample_rate(&self) -> NonZeroU32 {
        self.sample_rate
    }

    /// Number of channels. Samples are interleaved: one per channel, frame
    /// after frame.
    pub fn channel_count(&self) -> NonZeroU16 {
        self.channel_count
    }

    /// The length the file declares, in seconds, when it declares one.
    pub fn duration_seconds(&self) -> Option<f32> {
        self.duration_seconds
    }

    /// Decodes the next stretch of audio into `interleaved`, replacing what
    /// was there. Returns false at the end of the file, leaving `interleaved`
    /// empty.
    ///
    /// A packet whose channel layout or sample rate differs from the first
    /// one is an error, since the stream promises one layout throughout.
    pub fn read_chunk(&mut self, interleaved: &mut Vec<f32>) -> Result<bool, EuphoriaError> {
        if self.is_finished {
            interleaved.clear();
            return Ok(false);
        }
        if let Some(first_chunk) = self.first_chunk.take() {
            *interleaved = first_chunk;
            return Ok(true);
        }
        loop {
            let layout = decode_next_packet(
                &self.path,
                &mut *self.format,
                &mut *self.decoder,
                self.track_id,
                interleaved,
            )?;
            let Some(layout) = layout else {
                self.is_finished = true;
                interleaved.clear();
                return Ok(false);
            };
            if layout.sample_rate != self.sample_rate || layout.channel_count != self.channel_count
            {
                return Err(EuphoriaError::Decode {
                    path: self.path.clone(),
                    failure: DecodeFailure::Malformed,
                    source: Some("the audio changes its sample rate or channels partway".into()),
                });
            }
            if self.frames_to_skip == 0 {
                return Ok(true);
            }
            let channel_count = usize::from(self.channel_count.get());
            let frames_in_chunk = (interleaved.len() / channel_count) as u64;
            let frames_dropped = self.frames_to_skip.min(frames_in_chunk);
            self.frames_to_skip -= frames_dropped;
            interleaved.drain(..frames_dropped as usize * channel_count);
            if !interleaved.is_empty() {
                return Ok(true);
            }
        }
    }

    /// Moves to `seconds` from the start, so that the next chunk read begins
    /// there. Returns the position reached, in seconds.
    ///
    /// A negative position, or one that is not a number, counts as the
    /// start. A position at or past the declared end, or too far away to be
    /// a time in any file, leaves the stream at its end: the next read
    /// returns false. When the file declares no length, the position
    /// returned for such a seek is the one asked for.
    pub fn seek(&mut self, seconds: f32) -> Result<f32, EuphoriaError> {
        let seconds = seconds.max(0.0);
        self.first_chunk = None;
        self.frames_to_skip = 0;
        if let Some(duration) = self.duration_seconds
            && seconds >= duration
        {
            self.is_finished = true;
            return Ok(duration);
        }
        let Some(time) = Time::try_from_secs_f64(f64::from(seconds)) else {
            self.is_finished = true;
            return Ok(seconds);
        };
        if self.is_finished {
            // Some demuxers cannot seek once they have read to the end, so a
            // finished stream starts over from a freshly opened file.
            let reopened = open_track(&self.path)?;
            self.format = reopened.format;
            self.decoder = reopened.decoder;
            self.track_id = reopened.track_id;
            self.time_base = reopened.time_base;
            self.is_finished = false;
        }

        let seeked = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time,
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|error| decode_error(&self.path, error))?;
        self.decoder.reset();
        match landing(seeked.required_ts.get(), seeked.actual_ts.get()) {
            Some(Landing::Before(units)) => {
                self.frames_to_skip = self.frames_in(units);
                Ok(seconds)
            }
            Some(Landing::After(units)) => {
                let overshoot = self.frames_in(units) as f32 / self.sample_rate.get() as f32;
                Ok(seconds + overshoot)
            }
            None => Err(EuphoriaError::Decode {
                path: self.path.clone(),
                failure: DecodeFailure::Malformed,
                source: Some("seeking reported a position that cannot be".into()),
            }),
        }
    }

    /// Converts a count of the track's time base units to frames. The two
    /// are the same for every format Euphoria reads, where the time base is
    /// one over the sample rate.
    fn frames_in(&self, time_base_units: u64) -> u64 {
        let Some(time_base) = self.time_base else {
            return time_base_units;
        };
        let seconds = time_base
            .calc_time_saturating(symphonia::core::units::Timestamp::new(
                i64::try_from(time_base_units).unwrap_or(i64::MAX),
            ))
            .as_secs_f64();
        (seconds * f64::from(self.sample_rate.get())).round() as u64
    }
}

/// Where a seek put the demuxer relative to the timestamp that was wanted,
/// in the track's time base units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Landing {
    /// At or before the target, which is what an accurate seek promises.
    /// The audio in between is decoded and dropped.
    Before(u64),
    /// After the target. Playing starts from there, since the audio before
    /// it cannot be reached, and the position reported says so.
    After(u64),
}

/// Compares the timestamp a seek wanted with the one it reached. `None`
/// when the two are so far apart that their distance is not a number of
/// units at all, which no real file produces.
fn landing(required_timestamp: i64, actual_timestamp: i64) -> Option<Landing> {
    let units_before_target = required_timestamp.checked_sub(actual_timestamp)?;
    Some(match u64::try_from(units_before_target) {
        Ok(units) => Landing::Before(units),
        Err(_) => Landing::After(units_before_target.unsigned_abs()),
    })
}

/// The demuxer and decoder of a file's audio track, positioned at its start.
struct OpenTrack {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    time_base: Option<TimeBase>,
    declared_frames: Option<u64>,
}

fn open_track(path: &Path) -> Result<OpenTrack, EuphoriaError> {
    let missing = |failure: DecodeFailure| EuphoriaError::Decode {
        path: path.to_path_buf(),
        failure,
        source: None,
    };
    let format = open_format(path)?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| missing(DecodeFailure::NoAudioTrack))?;
    let track_id = track.id;
    let time_base = track.time_base;
    let declared_frames = track.num_frames;
    let codec_parameters = track
        .codec_params
        .as_ref()
        .and_then(|parameters| parameters.audio())
        .ok_or_else(|| missing(DecodeFailure::NoAudioTrack))?;
    let decoder = symphonia::default::get_codecs()
        .make_audio_decoder(codec_parameters, &AudioDecoderOptions::default())
        .map_err(|error| decode_error(path, error))?;
    Ok(OpenTrack {
        format,
        decoder,
        track_id,
        time_base,
        declared_frames,
    })
}

/// The sample rate and channel count of a decoded chunk.
struct ChunkLayout {
    sample_rate: NonZeroU32,
    channel_count: NonZeroU16,
}

/// Decodes the next packet of the audio track into `interleaved`. Returns
/// `None` at the end of the file. Packets the decoder cannot make sense of
/// are skipped, as when decoding a whole file.
fn decode_next_packet(
    path: &Path,
    format: &mut dyn FormatReader,
    decoder: &mut dyn AudioDecoder,
    track_id: u32,
    interleaved: &mut Vec<f32>,
) -> Result<Option<ChunkLayout>, EuphoriaError> {
    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => return Ok(None),
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                return Ok(None);
            }
            Err(error) => return Err(decode_error(path, error)),
        };
        if packet.track_id != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) => {
                let sample_rate = NonZeroU32::new(decoded.spec().rate());
                let channel_count =
                    NonZeroU16::new(u16::try_from(decoded.num_planes()).unwrap_or(0));
                let (Some(sample_rate), Some(channel_count)) = (sample_rate, channel_count) else {
                    continue;
                };
                decoded.copy_to_vec_interleaved(interleaved);
                if interleaved.is_empty() {
                    continue;
                }
                return Ok(Some(ChunkLayout {
                    sample_rate,
                    channel_count,
                }));
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(decode_error(path, error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn read_everything(stream: &mut AudioStream) -> Vec<f32> {
        let mut all = Vec::new();
        let mut chunk = Vec::new();
        while stream.read_chunk(&mut chunk).unwrap() {
            assert!(!chunk.is_empty());
            all.extend_from_slice(&chunk);
        }
        assert!(chunk.is_empty());
        all
    }

    #[test]
    fn reports_the_layout_and_length_of_the_file() {
        let stereo = AudioStream::open(&fixture("sine-440hz-stereo.wav")).unwrap();
        assert_eq!(stereo.channel_count().get(), 2);
        assert_eq!(stereo.sample_rate().get(), 44_100);
        assert!((stereo.duration_seconds().unwrap() - 1.0).abs() < 0.01);

        let mono = AudioStream::open(&fixture("sine-440hz-mono.flac")).unwrap();
        assert_eq!(mono.channel_count().get(), 1);
    }

    #[test]
    fn streams_the_same_audio_as_decoding_the_whole_file() {
        for name in [
            "sine-440hz-mono.wav",
            "sine-440hz-mono.flac",
            "sine-440hz-mono.mp3",
            "sine-440hz-mono.m4a",
            "sine-440hz-mono.aiff",
        ] {
            let path = fixture(name);
            let whole = crate::decode_mono(&path).unwrap();
            let mut stream = AudioStream::open(&path).unwrap();
            let streamed = read_everything(&mut stream);
            assert_eq!(streamed.len(), whole.samples().len(), "{name}");
            assert_eq!(streamed, whole.samples(), "{name}");
        }
    }

    #[test]
    fn reads_in_many_small_chunks_and_not_the_whole_file_at_once() {
        let mut stream = AudioStream::open(&fixture("sine-440hz-mono.mp3")).unwrap();
        let mut chunk = Vec::new();
        let mut chunk_count = 0;
        while stream.read_chunk(&mut chunk).unwrap() {
            assert!(
                chunk.len() < 44_100 / 4,
                "a chunk of {} samples",
                chunk.len()
            );
            chunk_count += 1;
        }
        assert!(chunk_count > 10, "{chunk_count} chunks");
    }

    #[test]
    fn stereo_samples_are_interleaved_frame_by_frame() {
        let mut stream = AudioStream::open(&fixture("sine-440hz-stereo.wav")).unwrap();
        let samples = read_everything(&mut stream);
        assert_eq!(samples.len(), 2 * 44_100);
    }

    #[test]
    fn seeking_resumes_at_the_position_asked_for() {
        for name in [
            "sine-440hz-mono.wav",
            "sine-440hz-mono.flac",
            "sine-440hz-mono.mp3",
            "sine-440hz-mono.m4a",
        ] {
            let path = fixture(name);
            let mut stream = AudioStream::open(&path).unwrap();
            let total = read_everything(&mut stream).len();

            let reached = stream.seek(0.25).unwrap();
            assert!((reached - 0.25).abs() < 1.0e-6, "{name}");
            let remaining = read_everything(&mut stream).len();
            let expected = total - 44_100 / 4;
            assert!(
                remaining.abs_diff(expected) <= 64,
                "{name}: {remaining} samples left, expected about {expected}"
            );
        }
    }

    #[test]
    fn seeking_back_to_the_start_plays_the_whole_file_again() {
        let mut stream = AudioStream::open(&fixture("sine-440hz-mono.wav")).unwrap();
        let first_pass = read_everything(&mut stream);
        stream.seek(0.0).unwrap();
        assert_eq!(read_everything(&mut stream), first_pass);
    }

    #[test]
    fn seeking_past_the_end_leaves_nothing_to_read() {
        let mut stream = AudioStream::open(&fixture("sine-440hz-mono.wav")).unwrap();
        stream.seek(30.0).unwrap();
        let mut chunk = vec![1.0];
        assert!(!stream.read_chunk(&mut chunk).unwrap());
        assert!(chunk.is_empty());
    }

    #[test]
    fn a_seek_too_far_to_be_a_time_ends_a_stream_of_unknown_length() {
        for unrepresentable in [f32::MAX, f32::INFINITY, 1.0e30] {
            let mut stream = AudioStream::open(&fixture("sine-440hz-mono.wav")).unwrap();
            stream.duration_seconds = None;
            assert_eq!(stream.seek(unrepresentable).unwrap(), unrepresentable);
            let mut chunk = vec![1.0];
            assert!(!stream.read_chunk(&mut chunk).unwrap());
            assert!(chunk.is_empty());

            assert_eq!(stream.seek(0.5).unwrap(), 0.5);
            let remaining = read_everything(&mut stream).len();
            assert!(remaining.abs_diff(44_100 / 2) <= 64, "{remaining}");
        }
    }

    #[test]
    fn a_seek_that_is_not_a_number_or_is_negative_goes_to_the_start() {
        let mut stream = AudioStream::open(&fixture("sine-440hz-mono.wav")).unwrap();
        let whole = read_everything(&mut stream).len();
        for start in [f32::NAN, -5.0, f32::NEG_INFINITY] {
            assert_eq!(stream.seek(start).unwrap(), 0.0);
            assert_eq!(read_everything(&mut stream).len(), whole);
        }
    }

    #[test]
    fn a_seek_result_is_read_without_overflow() {
        assert_eq!(landing(1_000, 1_000), Some(Landing::Before(0)));
        assert_eq!(landing(1_000, 424), Some(Landing::Before(576)));
        assert_eq!(landing(1_000, 1_024), Some(Landing::After(24)));
        assert_eq!(landing(0, i64::MAX), Some(Landing::After(i64::MAX as u64)));
        assert_eq!(landing(-10, -4), Some(Landing::After(6)));
        assert_eq!(landing(i64::MAX, i64::MIN), None);
        assert_eq!(landing(i64::MIN, i64::MAX), None);
        assert_eq!(landing(i64::MIN, 0), Some(Landing::After(1 << 63)));
    }

    #[test]
    fn a_missing_file_and_a_file_that_is_not_audio_fail_to_open() {
        assert!(matches!(
            AudioStream::open(Path::new("/definitely/not/here.mp3")),
            Err(EuphoriaError::Open { .. })
        ));
        assert!(matches!(
            AudioStream::open(&fixture("README.md")),
            Err(EuphoriaError::Decode { .. })
        ));
    }
}
