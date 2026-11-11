// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Changes the key of a track by shifting its pitch, without changing its
//! tempo, and writes the shifted copy to a new file.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use crate::audio_stream::AudioStream;
use crate::error::EuphoriaError;
use crate::key::Key;
use crate::signalsmith::Stretch;
use crate::tags::{TagValues, TagWriteOptions, apply_plan, plan_write};

/// The furthest a track can be shifted, in semitones either way. Six
/// semitones reach every key, and the further a track is shifted, the more
/// its sound changes.
pub const MAXIMUM_SHIFT_SEMITONES: i8 = 6;

/// Bits per sample of the files written, enough for any source a DJ plays.
const BITS_PER_SAMPLE: u16 = 24;

/// Shifts the pitch of interleaved audio by a whole number of semitones
/// while keeping its timing, chunk by chunk, so that a long track never
/// has to be held in memory twice.
///
/// Every chunk given to [`PitchShifter::process`] comes back the same
/// length, apart from a delay at the start that [`PitchShifter::finish`]
/// makes up at the end, so the whole output is exactly as long as the
/// input and every beat stays where it was.
pub struct PitchShifter {
    stretch: Stretch,
    channels: usize,
    /// Samples per channel the shifter needs before it can start.
    input_latency: usize,
    /// Samples per channel of the output that come before the first sample
    /// of the input, and are dropped.
    output_latency: usize,
    /// The first samples, per channel, kept until there are enough to start.
    preroll: Vec<Vec<f32>>,
    started: bool,
    /// Output samples per channel still to drop.
    to_drop: usize,
    /// Input samples per channel seen.
    input_length: usize,
}

impl PitchShifter {
    /// A shifter for `channels` channels at `sample_rate` that moves every
    /// pitch by `semitones`, from -[`MAXIMUM_SHIFT_SEMITONES`] to
    /// [`MAXIMUM_SHIFT_SEMITONES`].
    pub fn new(channels: usize, sample_rate: u32, semitones: i8) -> Result<Self, String> {
        if semitones.unsigned_abs() > MAXIMUM_SHIFT_SEMITONES.unsigned_abs() {
            return Err(format!(
                "a shift of {semitones} semitones is more than {MAXIMUM_SHIFT_SEMITONES} either way"
            ));
        }
        let mut stretch = Stretch::new(channels, sample_rate)?;
        stretch.set_transpose_semitones(f32::from(semitones))?;
        let input_latency = stretch.input_latency();
        let output_latency = stretch.output_latency();
        Ok(Self {
            stretch,
            channels,
            input_latency,
            output_latency,
            preroll: vec![Vec::new(); channels],
            started: false,
            to_drop: output_latency,
            input_length: 0,
        })
    }

    /// Shifts the next stretch of interleaved audio and returns the shifted
    /// audio that is ready, interleaved.
    pub fn process(&mut self, interleaved: &[f32]) -> Result<Vec<f32>, String> {
        let mut channels = deinterleave(interleaved, self.channels);
        self.input_length += channels[0].len();
        if !self.started {
            let wanted = self.input_latency.saturating_sub(self.preroll[0].len());
            let taken = wanted.min(channels[0].len());
            for (preroll, channel) in self.preroll.iter_mut().zip(&mut channels) {
                preroll.extend(channel.drain(..taken));
            }
            if self.preroll[0].len() < self.input_latency {
                return Ok(Vec::new());
            }
            self.stretch.seek(&self.preroll, 1.0)?;
            self.started = true;
        }
        self.shift(&channels)
    }

    /// Shifts what is left and returns the rest of the output, so that the
    /// whole output is as long as the whole input.
    pub fn finish(mut self) -> Result<Vec<f32>, String> {
        if !self.started {
            // Fewer samples than the shifter needs to start: the start of
            // the track is all there is, so it is used as the pre-roll and
            // shifted from zeros on.
            let length = self.preroll[0].len();
            self.stretch.seek(&self.preroll, 1.0)?;
            self.started = true;
            let silence = vec![vec![0.0; length]; self.channels];
            let mut output = self.shift(&silence)?;
            output.extend(self.flushed()?);
            output.truncate(self.input_length * self.channels);
            return Ok(output);
        }
        // The pre-roll is still owed to the shifter as input: zeros stand in
        // for what would follow the end of the track.
        let silence = vec![vec![0.0; self.input_latency]; self.channels];
        let mut output = self.shift(&silence)?;
        output.extend(self.flushed()?);
        Ok(output)
    }

    fn shift(&mut self, channels: &[Vec<f32>]) -> Result<Vec<f32>, String> {
        let length = channels.first().map_or(0, Vec::len);
        if length == 0 {
            return Ok(Vec::new());
        }
        let mut output = vec![vec![0.0; length]; self.channels];
        self.stretch.process(channels, &mut output)?;
        let dropped = self.to_drop.min(length);
        self.to_drop -= dropped;
        for channel in &mut output {
            channel.drain(..dropped);
        }
        Ok(interleave(&output))
    }

    fn flushed(&mut self) -> Result<Vec<f32>, String> {
        let mut output = vec![vec![0.0; self.output_latency]; self.channels];
        self.stretch.flush(&mut output)?;
        Ok(interleave(&output))
    }
}

fn deinterleave(interleaved: &[f32], channels: usize) -> Vec<Vec<f32>> {
    let frames = interleaved.len() / channels;
    (0..channels)
        .map(|channel| {
            (0..frames)
                .map(|frame| interleaved[frame * channels + channel])
                .collect()
        })
        .collect()
}

fn interleave(channels: &[Vec<f32>]) -> Vec<f32> {
    let frames = channels.first().map_or(0, Vec::len);
    let mut interleaved = Vec::with_capacity(frames * channels.len());
    for frame in 0..frames {
        for channel in channels {
            interleaved.push(channel[frame]);
        }
    }
    interleaved
}

/// The audio file formats a shifted copy can be written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftedFormat {
    /// 24 bit WAV.
    Wav,
    /// 24 bit FLAC.
    Flac,
}

impl ShiftedFormat {
    /// The format that the extension of `path` names, if it is one a shifted
    /// copy can be written in.
    pub fn of_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "wav" => Some(Self::Wav),
            "flac" => Some(Self::Flac),
            _ => None,
        }
    }
}

/// What to do when the output of [`write_shifted_copy`] already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingOutput {
    /// Keep it and fail.
    Keep,
    /// Replace it.
    Replace,
}

/// What [`write_shifted_copy`] wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct ShiftedCopy {
    /// The file written.
    pub path: PathBuf,
    /// Samples per channel, the same as the original.
    pub frames: u64,
    /// The sample rate, the same as the original.
    pub sample_rate: u32,
    /// The number of channels, the same as the original.
    pub channels: u16,
    /// The key written into the copy's tags, when the original key was known.
    pub key: Option<Key>,
}

/// Writes a copy of the audio file at `input`, shifted by `semitones`, to
/// `output`, a WAV or FLAC file chosen by its extension, and tags the copy
/// with `values`, whose key is the key of the original: the copy is tagged
/// with that key shifted by `semitones`, and with the same tempo and energy.
///
/// The original is only read. The copy is written under a temporary name
/// next to `output` and moved into place once it is complete, so a failure
/// never leaves half a file, and with [`ExistingOutput::Keep`] a file that
/// appears at `output` meanwhile is kept.
pub fn write_shifted_copy(
    input: &Path,
    output: &Path,
    semitones: i8,
    values: TagValues,
    tag_options: &TagWriteOptions,
    existing: ExistingOutput,
) -> Result<ShiftedCopy, EuphoriaError> {
    let failure = |reason: String| EuphoriaError::Shift {
        path: input.to_path_buf(),
        reason,
    };
    let format = ShiftedFormat::of_path(output).ok_or_else(|| {
        failure(format!(
            "{} must end in .wav or .flac, the formats a shifted copy is written in",
            output.display()
        ))
    })?;
    if same_file(input, output) {
        return Err(failure(
            "the output is the original file, which is never changed".to_string(),
        ));
    }
    let mut stream = AudioStream::open(input)?;
    let sample_rate = stream.sample_rate().get();
    let channels = stream.channel_count().get();
    let mut shifter =
        PitchShifter::new(usize::from(channels), sample_rate, semitones).map_err(failure)?;

    let temporary = temporary_path(output);
    let written = (|| -> Result<u64, EuphoriaError> {
        let mut writer = ShiftedWriter::create(&temporary, format, channels, sample_rate)?;
        let mut chunk = Vec::new();
        while stream.read_chunk(&mut chunk)? {
            writer.write(&shifter.process(&chunk).map_err(&failure)?)?;
        }
        writer.write(&shifter.finish().map_err(&failure)?)?;
        writer.finish()
    })();
    let frames = match written {
        Ok(samples) => samples / u64::from(channels),
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
    };

    let key = values.key.map(|key| key.transposed(semitones));
    let tagged = TagValues { key, ..values };
    let tag_result =
        plan_write(&temporary, tagged, tag_options).and_then(|plan| apply_plan(&plan).map(|_| ()));
    let installed = tag_result.and_then(|()| install(&temporary, output, existing));
    if let Err(error) = installed {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(ShiftedCopy {
        path: output.to_path_buf(),
        frames,
        sample_rate,
        channels,
        key,
    })
}

fn same_file(first: &Path, second: &Path) -> bool {
    match (std::fs::canonicalize(first), std::fs::canonicalize(second)) {
        (Ok(first), Ok(second)) => first == second,
        _ => false,
    }
}

/// A hidden name next to `output` with the same extension, so that the
/// tags of the right format are written to it.
fn temporary_path(output: &Path) -> PathBuf {
    let name = output
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = output
        .extension()
        .map(|extension| extension.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    output.with_file_name(format!(
        ".{name}.{}-{unique}.partial.{extension}",
        std::process::id()
    ))
}

fn install(temporary: &Path, output: &Path, existing: ExistingOutput) -> Result<(), EuphoriaError> {
    let failure = |source: std::io::Error| EuphoriaError::ExportFile {
        path: output.to_path_buf(),
        source,
    };
    match existing {
        ExistingOutput::Replace => std::fs::rename(temporary, output).map_err(failure),
        ExistingOutput::Keep => {
            // A link takes the name only when it is free, unlike a rename.
            std::fs::hard_link(temporary, output).map_err(failure)?;
            std::fs::remove_file(temporary).map_err(failure)
        }
    }
}

/// Writes interleaved samples as 24 bit integers.
enum ShiftedWriter {
    Wav(hound::WavWriter<BufWriter<File>>, PathBuf, u64),
    Flac {
        path: PathBuf,
        samples: Vec<i32>,
        channels: u16,
        sample_rate: u32,
    },
}

impl ShiftedWriter {
    fn create(
        path: &Path,
        format: ShiftedFormat,
        channels: u16,
        sample_rate: u32,
    ) -> Result<Self, EuphoriaError> {
        Ok(match format {
            ShiftedFormat::Wav => {
                let spec = hound::WavSpec {
                    channels,
                    sample_rate,
                    bits_per_sample: BITS_PER_SAMPLE,
                    sample_format: hound::SampleFormat::Int,
                };
                let writer = hound::WavWriter::create(path, spec)
                    .map_err(|error| write_error(path, error.to_string()))?;
                Self::Wav(writer, path.to_path_buf(), 0)
            }
            ShiftedFormat::Flac => Self::Flac {
                path: path.to_path_buf(),
                samples: Vec::new(),
                channels,
                sample_rate,
            },
        })
    }

    fn write(&mut self, interleaved: &[f32]) -> Result<(), EuphoriaError> {
        match self {
            Self::Wav(writer, path, count) => {
                for sample in interleaved {
                    writer
                        .write_sample(to_integer(*sample))
                        .map_err(|error| write_error(path, error.to_string()))?;
                }
                *count += interleaved.len() as u64;
            }
            Self::Flac { samples, .. } => {
                samples.extend(interleaved.iter().map(|sample| to_integer(*sample)));
            }
        }
        Ok(())
    }

    /// Completes the file and returns how many samples it holds, counting
    /// every channel.
    fn finish(self) -> Result<u64, EuphoriaError> {
        match self {
            Self::Wav(writer, path, count) => {
                writer
                    .finalize()
                    .map_err(|error| write_error(&path, error.to_string()))?;
                Ok(count)
            }
            Self::Flac {
                path,
                samples,
                channels,
                sample_rate,
            } => {
                use flacenc::component::BitRepr;
                use flacenc::error::Verify;
                let config = flacenc::config::Encoder::default()
                    .into_verified()
                    .map_err(|error| write_error(&path, format!("{error:?}")))?;
                let source = flacenc::source::MemSource::from_samples(
                    &samples,
                    usize::from(channels),
                    usize::from(BITS_PER_SAMPLE),
                    sample_rate as usize,
                );
                let stream =
                    flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
                        .map_err(|error| write_error(&path, format!("{error:?}")))?;
                let mut sink = flacenc::bitsink::ByteSink::new();
                stream
                    .write(&mut sink)
                    .map_err(|error| write_error(&path, format!("{error:?}")))?;
                let mut bytes = sink.as_slice().to_vec();
                with_fixed_minimum_block_size(&mut bytes);
                std::fs::write(&path, bytes).map_err(|source| EuphoriaError::ExportFile {
                    path: path.clone(),
                    source,
                })?;
                Ok(samples.len() as u64)
            }
        }
    }
}

/// Sets the minimum block size of a FLAC stream with one block size to that
/// size. The encoder counts the shorter last block as the minimum, which
/// the format reserves for streams whose blocks vary, and decoders then
/// misplace the last frames: Symphonia stops reading the file there.
fn with_fixed_minimum_block_size(flac: &mut [u8]) {
    // "fLaC", the four byte header of the STREAMINFO block, then its
    // minimum and its maximum block size, two bytes each.
    if flac.len() < 12 || &flac[..4] != b"fLaC" || flac[4] & 0x7f != 0 {
        return;
    }
    let maximum = [flac[10], flac[11]];
    flac[8..10].copy_from_slice(&maximum);
}

fn write_error(path: &Path, reason: String) -> EuphoriaError {
    EuphoriaError::Shift {
        path: path.to_path_buf(),
        reason: format!("cannot write the shifted copy: {reason}"),
    }
}

/// A sample from -1 to 1 as a 24 bit integer, clipped at full scale.
fn to_integer(sample: f32) -> i32 {
    let full_scale = f64::from((1_i32 << (BITS_PER_SAMPLE - 1)) - 1);
    (f64::from(sample).clamp(-1.0, 1.0) * full_scale).round() as i32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::Mode;

    const SAMPLE_RATE: u32 = 44_100;

    /// The frequency with the most energy in `samples`, found by trying
    /// every frequency between `low` and `high` in steps of `step` hertz.
    fn strongest_frequency(samples: &[f32], low: f64, high: f64, step: f64) -> f64 {
        let mut best = (0.0, low);
        let mut frequency = low;
        while frequency <= high {
            let (mut real, mut imaginary) = (0.0_f64, 0.0_f64);
            for (index, sample) in samples.iter().enumerate() {
                let phase =
                    std::f64::consts::TAU * frequency * index as f64 / f64::from(SAMPLE_RATE);
                real += f64::from(*sample) * phase.cos();
                imaginary += f64::from(*sample) * phase.sin();
            }
            let power = real * real + imaginary * imaginary;
            if power > best.0 {
                best = (power, frequency);
            }
            frequency += step;
        }
        best.1
    }

    fn tone(frequency: f64, seconds: f64) -> Vec<f32> {
        (0..(seconds * f64::from(SAMPLE_RATE)) as usize)
            .map(|index| {
                (0.5 * (std::f64::consts::TAU * frequency * index as f64 / f64::from(SAMPLE_RATE))
                    .sin()) as f32
            })
            .collect()
    }

    fn shifted(input: &[f32], channels: usize, semitones: i8, chunk: usize) -> Vec<f32> {
        let mut shifter = PitchShifter::new(channels, SAMPLE_RATE, semitones).unwrap();
        let mut output = Vec::new();
        for piece in input.chunks(chunk * channels) {
            output.extend(shifter.process(piece).unwrap());
        }
        output.extend(shifter.finish().unwrap());
        output
    }

    #[test]
    fn a_tone_moves_by_the_semitones_asked_for_and_keeps_its_length() {
        let input = tone(440.0, 3.0);
        for semitones in [-6_i8, -3, 1, 2, 6] {
            let output = shifted(&input, 1, semitones, 4_096);
            assert_eq!(output.len(), input.len(), "{semitones}");
            let expected = 440.0 * 2_f64.powf(f64::from(semitones) / 12.0);
            let middle = &output[output.len() / 3..output.len() / 3 + 22_050];
            let found = strongest_frequency(middle, expected - 30.0, expected + 30.0, 0.5);
            assert!(
                (found - expected).abs() < 2.0,
                "{semitones} semitones: {found} Hz, expected {expected}"
            );
        }
    }

    #[test]
    fn clicks_stay_where_they_were_so_the_tempo_and_the_beats_do_not_move() {
        let mut input = vec![0.0_f32; 4 * SAMPLE_RATE as usize];
        let beat = SAMPLE_RATE as usize / 2;
        for start in (beat / 2..input.len() - 2_000).step_by(beat) {
            for (offset, sample) in input[start..start + 2_000].iter_mut().enumerate() {
                let time = offset as f64 / f64::from(SAMPLE_RATE);
                *sample = (0.8
                    * (std::f64::consts::TAU * 220.0 * time).sin()
                    * (-time / 0.01).exp()) as f32;
            }
        }

        let output = shifted(&input, 1, 3, 1_000);

        assert_eq!(output.len(), input.len());
        let onset = |samples: &[f32], from: usize| -> usize {
            let peak = samples[from..from + beat / 2]
                .iter()
                .fold(0.0_f32, |best, sample| best.max(sample.abs()));
            from + samples[from..from + beat / 2]
                .iter()
                .position(|sample| sample.abs() >= peak / 2.0)
                .unwrap_or(0)
        };
        for number in 1..6 {
            // Each click starts half a beat after the beat count, in the
            // middle of the window searched.
            let from = number * beat + beat / 4;
            let original = onset(&input, from);
            let moved = onset(&output, from);
            let offset_ms = (moved as f64 - original as f64) * 1_000.0 / f64::from(SAMPLE_RATE);
            assert!(
                offset_ms.abs() < 10.0,
                "beat {number} moved by {offset_ms} ms"
            );
        }
    }

    #[test]
    fn stereo_is_shifted_channel_by_channel_and_a_short_input_keeps_its_length() {
        let left = tone(440.0, 2.0);
        let right = tone(660.0, 2.0);
        let input: Vec<f32> = left
            .iter()
            .zip(&right)
            .flat_map(|(left, right)| [*left, *right])
            .collect();
        let output = shifted(&input, 2, 2, 3_000);
        assert_eq!(output.len(), input.len());
        let channel =
            |index: usize| -> Vec<f32> { output.iter().skip(index).step_by(2).copied().collect() };
        let middle = |samples: Vec<f32>| samples[22_050..44_100].to_vec();
        let factor = 2_f64.powf(2.0 / 12.0);
        let left_found = strongest_frequency(&middle(channel(0)), 470.0, 520.0, 0.5);
        let right_found = strongest_frequency(&middle(channel(1)), 710.0, 770.0, 0.5);
        assert!((left_found - 440.0 * factor).abs() < 2.0, "{left_found}");
        assert!((right_found - 660.0 * factor).abs() < 2.0, "{right_found}");

        let short = tone(440.0, 0.01);
        assert_eq!(shifted(&short, 1, 1, 100).len(), short.len());
    }

    #[test]
    fn a_shifted_copy_decodes_to_as_many_frames_in_wav_and_flac() {
        let folder = std::env::temp_dir().join(format!("euphoria-shift-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        // A length that is not a whole number of FLAC blocks, so the last
        // block is shorter than the others.
        let frames = 3 * SAMPLE_RATE as usize + 1_234;
        let left = tone(440.0, frames as f64 / f64::from(SAMPLE_RATE));
        let mut original = ShiftedWriter::create(
            &folder.join("original.wav"),
            ShiftedFormat::Wav,
            2,
            SAMPLE_RATE,
        )
        .unwrap();
        let interleaved: Vec<f32> = left.iter().flat_map(|sample| [*sample, -*sample]).collect();
        original.write(&interleaved).unwrap();
        original.finish().unwrap();

        for name in ["up.wav", "up.flac"] {
            let output = folder.join(name);
            let copy = write_shifted_copy(
                &folder.join("original.wav"),
                &output,
                2,
                TagValues {
                    key: Some(Key::new(9, Mode::Minor)),
                    bpm: Some(124.0),
                    energy: Some(7),
                },
                &TagWriteOptions::default(),
                ExistingOutput::Keep,
            )
            .unwrap();
            assert_eq!(copy.frames, frames as u64, "{name}");
            assert_eq!(copy.key, Some(Key::new(11, Mode::Minor)));
            let mut stream = AudioStream::open(&output).unwrap();
            let mut decoded = 0;
            let mut chunk = Vec::new();
            while stream.read_chunk(&mut chunk).unwrap() {
                decoded += chunk.len();
            }
            assert_eq!(decoded, frames * 2, "{name}");
            let tags = crate::tags::read_tags(&output).unwrap();
            assert_eq!(
                tags.key.as_deref(),
                Some(Key::new(11, Mode::Minor).camelot().as_str()),
                "{name}"
            );

            let again = write_shifted_copy(
                &folder.join("original.wav"),
                &output,
                2,
                TagValues::default(),
                &TagWriteOptions::default(),
                ExistingOutput::Keep,
            );
            assert!(again.is_err(), "{name} was replaced without being asked");
        }
        let refused = write_shifted_copy(
            &folder.join("original.wav"),
            &folder.join("original.wav"),
            1,
            TagValues::default(),
            &TagWriteOptions::default(),
            ExistingOutput::Replace,
        );
        assert!(refused.is_err(), "the original was overwritten");
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_shift_beyond_six_semitones_is_refused() {
        assert!(PitchShifter::new(1, SAMPLE_RATE, 7).is_err());
        assert!(PitchShifter::new(1, SAMPLE_RATE, -7).is_err());
        assert!(PitchShifter::new(0, SAMPLE_RATE, 1).is_err());
    }

    #[test]
    fn keys_move_around_the_twelve_tones() {
        let a_minor = Key::new(9, Mode::Minor);
        assert_eq!(a_minor.transposed(3), Key::new(0, Mode::Minor));
        assert_eq!(a_minor.transposed(-6), Key::new(3, Mode::Minor));
        assert_eq!(
            Key::new(0, Mode::Major).transposed(-1),
            Key::new(11, Mode::Major)
        );
        assert_eq!(a_minor.transposed(0), a_minor);
    }
}
