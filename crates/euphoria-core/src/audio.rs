// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs::File;
use std::num::NonZeroU32;
use std::path::Path;

use serde::{Deserialize, Serialize};
use symphonia::core::codecs::audio::well_known::CODEC_ID_AAC;
use symphonia::core::codecs::audio::{AudioCodecParameters, AudioDecoderOptions};
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

/// The shortest stretch of channel energy a mixdown judges on, in frames:
/// about 0.37 s at 44.1 kHz. Judging a whole file would hide a stretch of
/// inverted phase, and judging each packet would mistake a short out of
/// phase bass note for a cancelled file.
const MIXDOWN_BLOCK_FRAMES: usize = 16_384;

/// A block of stereo is cancelled when its mono mixdown holds less than this
/// share of the energy of its channels. For two channels of equal level the
/// share is (1 + c) / 2 with c their correlation, so 0.05 means c below
/// -0.9, which real music does not reach. Even wide stereo stays above 0.2.
const CANCELLED_MIXDOWN_SHARE: f64 = 0.05;

/// The lost share of a file, over its declared length, from which it counts
/// as heavily damaged. A dropped packet is replaced by silence of the same
/// length, so the times after it stay right, but the sections, key and
/// tempo of what was lost are unknown. 2% is about 8 seconds of a 7 minute
/// track: past it a gap can cover a whole phrase, so the beat grid and the
/// cues are not written.
pub const SEVERE_LOST_SHARE: f32 = 0.02;

/// A file counts as shorter than it declares when the audio is missing by
/// more than 0.5% of its length and at least this many seconds. Containers
/// round their length, and encoder delay and padding move it by a few
/// hundredths of a second.
const MISSING_TOLERANCE_SECONDS: f32 = 0.25;
const MISSING_TOLERANCE_SHARE: f32 = 0.005;

/// A packet that starts more than 1/100 s after the previous one ended means
/// the reader skipped over damaged data to find the next one, as FLAC and
/// Ogg readers do. The skipped stretch is replaced by silence.
const MINIMUM_GAP_DIVISOR: u32 = 100;

/// The longest skipped stretch replaced by silence, so a garbled timestamp
/// cannot ask for hours of it.
const MAXIMUM_GAP_SECONDS: u64 = 600;

/// How many packets in a row the demuxer may fail to read before the rest of
/// the file is given up on.
const MAXIMUM_CONSECUTIVE_READ_ERRORS: u32 = 64;

/// What went wrong while a file was decoded: packets the decoder rejected,
/// and audio the container promised that never came.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct DecodeDamage {
    /// Packets the decoder or the reader could not decode and dropped. Each
    /// is replaced by silence of the length of the packet before it, so the
    /// times after it do not move.
    #[serde(default)]
    pub dropped_packets: u32,
    /// The seconds of silence put in place of dropped packets.
    #[serde(default)]
    pub concealed_seconds: f32,
    /// True when reading stopped on a read error, which is how the end of a
    /// truncated file shows, and also how some formats show a normal end.
    /// It counts as damage only with a shortfall against the declared length.
    #[serde(default)]
    pub ended_on_error: bool,
    /// The length the container declares, when it does.
    #[serde(default)]
    pub declared_seconds: Option<f32>,
    /// The length of the decoded audio, silence put in place of dropped
    /// packets included.
    #[serde(default)]
    pub decoded_seconds: f32,
}

impl DecodeDamage {
    /// The seconds the container declares that are not in the audio, or 0
    /// when the shortfall is within what containers round by.
    pub fn missing_seconds(&self) -> f32 {
        let Some(declared) = self.declared_seconds else {
            return 0.0;
        };
        let shortfall = declared - self.decoded_seconds;
        let tolerance = MISSING_TOLERANCE_SECONDS.max(declared * MISSING_TOLERANCE_SHARE);
        if shortfall > tolerance {
            shortfall
        } else {
            0.0
        }
    }

    /// True when any packet was dropped or the audio is shorter than the
    /// file declares.
    pub fn is_damaged(&self) -> bool {
        self.dropped_packets > 0 || self.missing_seconds() > 0.0
    }

    /// The share of the file that is silence put in place of dropped
    /// packets or missing at the end, from 0 to 1.
    pub fn lost_share(&self) -> f32 {
        let whole = self
            .declared_seconds
            .unwrap_or(0.0)
            .max(self.decoded_seconds);
        if whole <= 0.0 {
            return 0.0;
        }
        ((self.concealed_seconds + self.missing_seconds()) / whole).min(1.0)
    }

    /// True when the lost share reaches [`SEVERE_LOST_SHARE`]. The beat grid
    /// and cues of such a file are not written.
    pub fn is_severe(&self) -> bool {
        self.lost_share() >= SEVERE_LOST_SHARE
    }

    /// A sentence saying what is wrong, or `None` for an undamaged file.
    pub fn description(&self) -> Option<String> {
        if !self.is_damaged() {
            return None;
        }
        let mut parts = Vec::new();
        if self.dropped_packets > 0 {
            parts.push(format!(
                "{} {} could not be decoded and {:.1} s of silence took {} place",
                self.dropped_packets,
                if self.dropped_packets == 1 {
                    "packet"
                } else {
                    "packets"
                },
                self.concealed_seconds,
                if self.dropped_packets == 1 {
                    "its"
                } else {
                    "their"
                }
            ));
        }
        let missing = self.missing_seconds();
        if missing > 0.0 {
            parts.push(format!(
                "{missing:.1} s of the {:.1} s the file declares are missing, as in a truncated file",
                self.declared_seconds.unwrap_or(0.0)
            ));
        }
        Some(format!("Damaged file: {}.", parts.join(", and ")))
    }
}

/// How a decode went, handed to the observer when it ends.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct DecodeHealth {
    pub damage: DecodeDamage,
    /// The share of stereo frames, from 0 to 1, whose channels cancelled in
    /// the mono mixdown, so the left channel was used for them.
    pub cancelled_share: f32,
}

/// Sees a file while [`decode_mono_observed`] decodes it, without keeping
/// more than the mono mixdown in memory.
pub(crate) trait DecodeObserver {
    /// The parameters of the audio track, before the first packet.
    fn stream(&mut self, parameters: &AudioCodecParameters);
    /// The size of one packet of the audio track, as stored in the file.
    fn packet(&mut self, bytes: usize);
    /// One decoded chunk of interleaved samples, before the mixdown.
    fn samples(&mut self, interleaved: &[f32], channel_count: usize, sample_rate: u32);
    /// How much of the file is decoded, from 0 to 1, when the file says how
    /// long it is. Called when the share rises by a hundredth or more.
    fn decoded_share(&mut self, _share: f32) {}
    /// How the decode went, after the last packet.
    fn finished(&mut self, _health: &DecodeHealth) {}
}

/// An observer that looks at nothing.
struct Unobserved;

impl DecodeObserver for Unobserved {
    fn stream(&mut self, _: &AudioCodecParameters) {}
    fn packet(&mut self, _: usize) {}
    fn samples(&mut self, _: &[f32], _: usize, _: u32) {}
}

/// Builds the mono mixdown of interleaved chunks.
///
/// Mono audio is kept as it is. Stereo is averaged, but a block whose
/// channels cancel, as when one is the other with its phase inverted, keeps
/// only its left channel. More than two channels use only the first two,
/// the front left and right in the order Symphonia lays channels out, so the
/// centre, the bass effects channel and the surrounds do not enter the mix.
struct MixdownBuilder {
    mono: Vec<f32>,
    pending_left: Vec<f32>,
    pending_mono: Vec<f32>,
    left_energy: f64,
    right_energy: f64,
    mono_energy: f64,
    stereo_frames: u64,
    cancelled_frames: u64,
}

impl MixdownBuilder {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            mono: Vec::with_capacity(capacity),
            pending_left: Vec::new(),
            pending_mono: Vec::new(),
            left_energy: 0.0,
            right_energy: 0.0,
            mono_energy: 0.0,
            stereo_frames: 0,
            cancelled_frames: 0,
        }
    }

    fn len(&self) -> usize {
        self.mono.len() + self.pending_mono.len()
    }

    fn add(&mut self, interleaved: &[f32], channel_count: usize) {
        if channel_count < 2 {
            self.mono.extend_from_slice(interleaved);
            return;
        }
        for frame in interleaved.chunks_exact(channel_count) {
            let (left, right) = (frame[0], frame[1]);
            let mono = (left + right) * 0.5;
            self.left_energy += f64::from(left) * f64::from(left);
            self.right_energy += f64::from(right) * f64::from(right);
            self.mono_energy += f64::from(mono) * f64::from(mono);
            self.pending_left.push(left);
            self.pending_mono.push(mono);
            if self.pending_mono.len() >= MIXDOWN_BLOCK_FRAMES {
                self.flush();
            }
        }
    }

    fn add_silence(&mut self, frames: usize) {
        if self.stereo_frames == 0 && self.pending_mono.is_empty() {
            self.mono.resize(self.mono.len() + frames, 0.0);
            return;
        }
        self.pending_left
            .resize(self.pending_left.len() + frames, 0.0);
        self.pending_mono
            .resize(self.pending_mono.len() + frames, 0.0);
        while self.pending_mono.len() >= MIXDOWN_BLOCK_FRAMES {
            self.flush();
        }
    }

    fn flush(&mut self) {
        let take = self.pending_mono.len().min(MIXDOWN_BLOCK_FRAMES);
        let channel_energy = (self.left_energy + self.right_energy) * 0.5;
        let audible = channel_energy / take.max(1) as f64
            > f64::from(SILENCE_THRESHOLD) * f64::from(SILENCE_THRESHOLD);
        let cancelled = audible && self.mono_energy < CANCELLED_MIXDOWN_SHARE * channel_energy;
        if cancelled {
            self.mono.extend(self.pending_left.drain(..take));
            self.pending_mono.drain(..take);
            self.cancelled_frames += take as u64;
        } else {
            self.mono.extend(self.pending_mono.drain(..take));
            self.pending_left.drain(..take);
        }
        self.stereo_frames += take as u64;
        self.left_energy = 0.0;
        self.right_energy = 0.0;
        self.mono_energy = 0.0;
    }

    fn finish(mut self) -> (Vec<f32>, f32) {
        while !self.pending_mono.is_empty() {
            self.flush();
        }
        let share = if self.stereo_frames == 0 {
            0.0
        } else {
            (self.cancelled_frames as f64 / self.stereo_frames as f64) as f32
        };
        (self.mono, share)
    }
}

/// Decodes a whole audio file and mixes its channels into mono.
pub fn decode_mono(path: &Path) -> Result<AudioBuffer, EuphoriaError> {
    decode_mono_observed(path, &mut Unobserved)
}

/// How long a file says it is, in seconds, read from its header without
/// decoding it, or `None` when the container does not say.
pub fn declared_duration_seconds(path: &Path) -> Option<f32> {
    let format = open_format(path).ok()?;
    let track = format.default_track(TrackType::Audio)?;
    let frames = track.num_frames.filter(|frames| *frames > 0)?;
    let sample_rate = track
        .codec_params
        .as_ref()
        .and_then(|parameters| parameters.audio())
        .and_then(|parameters| parameters.sample_rate)
        .filter(|rate| *rate > 0)?;
    Some(frames as f32 / sample_rate as f32)
}

/// Decodes like [`decode_mono`] and shows every packet and every decoded
/// chunk to `observer` on the way.
pub(crate) fn decode_mono_observed(
    path: &Path,
    observer: &mut impl DecodeObserver,
) -> Result<AudioBuffer, EuphoriaError> {
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
    if let Some(profile) = unsupported_aac_profile(codec_parameters) {
        return Err(EuphoriaError::Decode {
            path: path.to_path_buf(),
            failure: DecodeFailure::UnsupportedFormat,
            source: Some(format!("{profile} is not supported, only AAC-LC").into()),
        });
    }
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(codec_parameters, &AudioDecoderOptions::default())
        .map_err(decode_error)?;
    observer.stream(codec_parameters);

    let file_bytes = std::fs::metadata(path).ok().map(|metadata| metadata.len());
    let mut mixdown = MixdownBuilder::with_capacity(frames_to_reserve(declared_frames, file_bytes));
    let mut interleaved: Vec<f32> = Vec::new();
    let mut sample_rate: u32 = 0;
    let declared_frames = declared_frames.filter(|frames| *frames > 0);
    let mut reported_hundredths = 0;
    let mut damage = DecodeDamage::default();
    let mut last_chunk_frames = 0_usize;
    let mut concealed_frames = 0_u64;
    let mut consecutive_read_errors = 0_u32;
    let mut expected_start: Option<i64> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(packet)) => {
                consecutive_read_errors = 0;
                packet
            }
            Ok(None) => break,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                damage.ended_on_error = true;
                break;
            }
            Err(SymphoniaError::DecodeError(_)) => {
                damage.dropped_packets += 1;
                consecutive_read_errors += 1;
                if consecutive_read_errors >= MAXIMUM_CONSECUTIVE_READ_ERRORS {
                    damage.ended_on_error = true;
                    break;
                }
                continue;
            }
            Err(error) => return Err(decode_error(error)),
        };
        if packet.track_id != track_id {
            continue;
        }
        let start = packet.pts.get();
        if let Some(expected) = expected_start.filter(|_| sample_rate > 0) {
            let gap = start.saturating_sub(expected);
            if gap > i64::from(sample_rate / MINIMUM_GAP_DIVISOR) {
                let gap_frames = (gap as u64).min(MAXIMUM_GAP_SECONDS * u64::from(sample_rate));
                damage.dropped_packets += 1;
                concealed_frames += gap_frames;
                mixdown.add_silence(gap_frames as usize);
            }
        }
        expected_start = Some(start.saturating_add(packet.dur.get() as i64));
        observer.packet(packet.data.len());
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
                observer.samples(&interleaved, channel_count, sample_rate);
                last_chunk_frames = interleaved.len() / channel_count;
                mixdown.add(&interleaved, channel_count);
                if let Some(declared_frames) = declared_frames {
                    let hundredths = (mixdown.len() as u64 * 100 / declared_frames).min(100);
                    if hundredths > reported_hundredths {
                        reported_hundredths = hundredths;
                        observer.decoded_share(hundredths as f32 / 100.0);
                    }
                }
            }
            Err(SymphoniaError::DecodeError(_)) => {
                damage.dropped_packets += 1;
                concealed_frames += last_chunk_frames as u64;
                mixdown.add_silence(last_chunk_frames);
            }
            Err(error) => return Err(decode_error(error)),
        }
    }

    if mixdown.len() == 0 || sample_rate == 0 {
        return Err(EuphoriaError::EmptyAudio {
            path: path.to_path_buf(),
        });
    }
    let sample_rate = NonZeroU32::new(sample_rate).expect("checked above");
    let rate = sample_rate.get() as f32;
    damage.concealed_seconds = concealed_frames as f32 / rate;
    damage.decoded_seconds = mixdown.len() as f32 / rate;
    damage.declared_seconds = declared_frames.map(|frames| frames as f32 / rate);
    let (mut mono_samples, cancelled_share) = mixdown.finish();
    observer.finished(&DecodeHealth {
        damage,
        cancelled_share,
    });
    if mono_samples.capacity() / 2 > mono_samples.len() {
        mono_samples.shrink_to_fit();
    }
    Ok(AudioBuffer::new(mono_samples, sample_rate))
}

/// The name of the AAC profile when the stream is one the decoder cannot
/// play. The decoder reads AAC-LC. HE-AAC, which adds spectral band
/// replication to a half rate core, and HE-AACv2, which adds parametric
/// stereo, declare object type 5 or 29 in the AudioSpecificConfig.
fn unsupported_aac_profile(parameters: &AudioCodecParameters) -> Option<&'static str> {
    if parameters.codec != CODEC_ID_AAC {
        return None;
    }
    let object_type = parameters.extra_data.as_deref()?.first()? >> 3;
    match object_type {
        5 => Some("HE-AAC"),
        29 => Some("HE-AACv2"),
        _ => None,
    }
}

/// Opens a file and finds the reader for its container. Tags met on the way
/// are kept in the reader's metadata.
pub(crate) fn open_format(path: &Path) -> Result<Box<dyn FormatReader>, EuphoriaError> {
    let file = File::open(path).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            EuphoriaError::Missing {
                path: path.to_path_buf(),
            }
        } else {
            EuphoriaError::Open {
                path: path.to_path_buf(),
                source,
            }
        }
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

    fn damage(
        dropped_packets: u32,
        concealed_seconds: f32,
        declared_seconds: Option<f32>,
        decoded_seconds: f32,
    ) -> DecodeDamage {
        DecodeDamage {
            dropped_packets,
            concealed_seconds,
            ended_on_error: false,
            declared_seconds,
            decoded_seconds,
        }
    }

    #[test]
    fn a_file_that_matches_its_declared_length_is_not_damaged() {
        for decoded in [420.0, 419.9, 420.2, 418.5] {
            let intact = damage(0, 0.0, Some(420.0), decoded);
            assert!(!intact.is_damaged(), "{decoded}");
            assert_eq!(intact.description(), None);
        }
        assert!(!damage(0, 0.0, None, 30.0).is_damaged());
    }

    #[test]
    fn a_shortfall_beyond_what_containers_round_by_is_damage() {
        let short_track = damage(0, 0.0, Some(420.0), 410.0);
        assert!(short_track.is_damaged());
        assert!((short_track.missing_seconds() - 10.0).abs() < 1e-3);
        let tiny = damage(0, 0.0, Some(2.0), 1.6);
        assert!(tiny.is_damaged());
    }

    #[test]
    fn a_dropped_packet_is_damage_even_when_the_length_matches() {
        let dropped = damage(1, 0.026, Some(420.0), 420.0);
        assert!(dropped.is_damaged());
        assert!(!dropped.is_severe());
        assert!(dropped.description().unwrap().contains("1 packet"));
    }

    #[test]
    fn two_percent_lost_is_heavy_damage() {
        let almost = damage(40, 8.0, Some(420.0), 420.0);
        assert!(almost.lost_share() < SEVERE_LOST_SHARE);
        assert!(!almost.is_severe());
        let heavy = damage(40, 8.6, Some(420.0), 420.0);
        assert!(heavy.is_severe());
        let cut_short = damage(0, 0.0, Some(420.0), 200.0);
        assert!(cut_short.is_severe());
        let lost_both_ways = damage(10, 4.0, Some(420.0), 414.0);
        assert!(lost_both_ways.lost_share() > 0.02);
    }

    fn mix(builder: &mut MixdownBuilder, frames: &[(f32, f32)]) {
        let interleaved: Vec<f32> = frames
            .iter()
            .flat_map(|(left, right)| [*left, *right])
            .collect();
        builder.add(&interleaved, 2);
    }

    #[test]
    fn only_the_blocks_that_cancel_use_the_left_channel() {
        let wave = |frame: usize| 0.3 * (frame as f32 * 0.05).sin();
        let mut builder = MixdownBuilder::with_capacity(0);
        let in_phase: Vec<(f32, f32)> = (0..MIXDOWN_BLOCK_FRAMES)
            .map(|frame| (wave(frame), 0.5 * wave(frame)))
            .collect();
        let inverted: Vec<(f32, f32)> = (0..MIXDOWN_BLOCK_FRAMES)
            .map(|frame| (wave(frame), -wave(frame)))
            .collect();
        mix(&mut builder, &in_phase);
        mix(&mut builder, &inverted);
        let (mono, share) = builder.finish();

        assert_eq!(mono.len(), 2 * MIXDOWN_BLOCK_FRAMES);
        assert!((share - 0.5).abs() < 1e-6, "{share}");
        assert!((mono[100] - 0.75 * wave(100)).abs() < 1e-6);
        assert!((mono[MIXDOWN_BLOCK_FRAMES + 100] - wave(100)).abs() < 1e-6);
    }

    #[test]
    fn wide_stereo_and_silence_are_never_treated_as_cancelled() {
        let mut builder = MixdownBuilder::with_capacity(0);
        let uncorrelated: Vec<(f32, f32)> = (0..MIXDOWN_BLOCK_FRAMES)
            .map(|frame| {
                (
                    0.3 * (frame as f32 * 0.05).sin(),
                    0.3 * (frame as f32 * 0.0731).sin(),
                )
            })
            .collect();
        mix(&mut builder, &uncorrelated);
        mix(&mut builder, &vec![(0.0, 0.0); MIXDOWN_BLOCK_FRAMES]);
        let (_, share) = builder.finish();
        assert_eq!(share, 0.0);
    }

    #[test]
    fn silence_for_a_dropped_packet_keeps_the_length() {
        let mut builder = MixdownBuilder::with_capacity(0);
        mix(&mut builder, &vec![(0.1, 0.1); 1000]);
        builder.add_silence(500);
        mix(&mut builder, &vec![(0.1, 0.1); 1000]);
        let (mono, _) = builder.finish();
        assert_eq!(mono.len(), 2500);
        assert_eq!(mono[1200], 0.0);
        assert!((mono[2000] - 0.1).abs() < 1e-6);
    }

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
