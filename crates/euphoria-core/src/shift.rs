// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Changes the key of a track by shifting its pitch, without changing its
//! tempo, and writes the shifted copy to a new file.

use std::collections::hash_map::RandomState;
use std::fs::File;
use std::hash::{BuildHasher, Hasher};
use std::io::BufWriter;
use std::path::{Path, PathBuf};

mod flac;

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
// Hound's 24-bit WAVEFORMATEXTENSIBLE header uses 60 bytes of RIFF's
// u32 length, plus at most one byte of data-chunk padding.
const MAXIMUM_WAV_SAMPLES: u64 = (u32::MAX as u64 - 61) / (BITS_PER_SAMPLE as u64 / 8);

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

    let (file, temporary) =
        create_temporary_file(output).map_err(|source| EuphoriaError::ExportFile {
            path: output.to_path_buf(),
            source,
        })?;
    let written = (|| -> Result<u64, EuphoriaError> {
        let mut writer = ShiftedWriter::create(file, &temporary, format, channels, sample_rate)?;
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

/// Claims a short name of our own, preserving the extension for tag detection.
fn create_temporary_file(output: &Path) -> std::io::Result<(File, PathBuf)> {
    let extension = output.extension().unwrap_or_default();
    for _ in 0..64 {
        let mut name = std::ffi::OsString::from(format!(
            ".euphoria-shift-{:016x}.partial.",
            RandomState::new().build_hasher().finish()
        ));
        name.push(extension);
        let temporary = output.with_file_name(name);
        match File::options()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((file, temporary)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "every temporary file name is taken",
    ))
}

fn install(temporary: &Path, output: &Path, existing: ExistingOutput) -> Result<(), EuphoriaError> {
    use crate::export::rekordbox::{ExistingFile, install};
    install(
        temporary,
        output,
        match existing {
            ExistingOutput::Keep => ExistingFile::Keep,
            ExistingOutput::Replace => ExistingFile::Replace,
        },
    )
}

/// Writes interleaved samples as 24 bit integers.
enum ShiftedWriter {
    Wav(hound::WavWriter<BufWriter<File>>, PathBuf, u64),
    Flac(Box<flac::FlacWriter>),
}

impl ShiftedWriter {
    fn create(
        file: File,
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
                let writer = hound::WavWriter::new(BufWriter::new(file), spec)
                    .map_err(|error| write_error(path, error.to_string()))?;
                Self::Wav(writer, path.to_path_buf(), 0)
            }
            ShiftedFormat::Flac => Self::Flac(Box::new(flac::FlacWriter::new(
                file,
                path,
                channels,
                sample_rate,
            )?)),
        })
    }

    fn write(&mut self, interleaved: &[f32]) -> Result<(), EuphoriaError> {
        match self {
            Self::Wav(writer, path, count) => {
                let total = count.checked_add(interleaved.len() as u64)
                    .filter(|total| *total <= MAXIMUM_WAV_SAMPLES)
                    .ok_or_else(|| write_error(path, "the shifted WAV would exceed RIFF's 4 GiB limit; use FLAC for a longer copy".to_string()))?;
                for sample in interleaved {
                    writer
                        .write_sample(to_integer(*sample))
                        .map_err(|error| write_error(path, error.to_string()))?;
                }
                *count = total;
            }
            Self::Flac(writer) => writer.write(interleaved)?,
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
            Self::Flac(writer) => writer.finish(),
        }
    }
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
mod export_tests {
    use super::*;

    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "euphoria-shift-export-{name}-{}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_valid_long_output_filename_can_be_written_without_touching_the_source() {
        let scratch = Scratch::new("long-name");
        let input =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sine-440hz-mono.wav");
        let original = std::fs::read(&input).unwrap();
        let output = scratch.0.join(format!("{}.wav", "a".repeat(240)));
        let copy = write_shifted_copy(
            &input,
            &output,
            2,
            TagValues::default(),
            &TagWriteOptions::default(),
            ExistingOutput::Keep,
        )
        .unwrap();
        assert!(copy.frames > 0);
        assert!(output.is_file());
        assert_eq!(std::fs::read(&input).unwrap(), original);
    }

    #[test]
    fn wav_export_refuses_a_chunk_that_would_overflow_the_riff_length() {
        let scratch = Scratch::new("riff-limit");
        for (index, written) in [((u32::MAX as u64 - 60) / 3), u64::MAX]
            .into_iter()
            .enumerate()
        {
            let path = scratch.0.join(format!("limit-{index}.wav"));
            let mut writer = ShiftedWriter::create(
                File::create(&path).unwrap(),
                &path,
                ShiftedFormat::Wav,
                1,
                44_100,
            )
            .unwrap();
            if let ShiftedWriter::Wav(_, _, count) = &mut writer {
                *count = written;
            }
            let error = writer
                .write(&[0.25])
                .expect_err("RIFF's length must not wrap");
            assert!(error.to_string().contains("FLAC"));
        }
    }

    #[test]
    fn flac_frames_are_written_as_chunks_arrive_and_decode_with_correct_length() {
        let scratch = Scratch::new("streaming");
        let output = scratch.0.join("stream.flac");
        let mut writer = ShiftedWriter::create(
            File::create(&output).unwrap(),
            &output,
            ShiftedFormat::Flac,
            2,
            44_100,
        )
        .unwrap();
        let chunk: Vec<f32> = (0..8192)
            .map(|i| (((i * 6271) % 65521) as f32 / 65521.0 - 0.5) * 0.8)
            .collect();
        for _ in 0..32 {
            writer.write(&chunk).unwrap();
        }
        assert!(
            std::fs::metadata(&output).map(|m| m.len()).unwrap_or(0) > 42,
            "completed FLAC frames must reach disk without retaining the whole track"
        );
        writer.write(&chunk[..600]).unwrap();
        let samples = writer.finish().unwrap();
        assert_eq!(samples, 32 * chunk.len() as u64 + 600);
        let mut stream = AudioStream::open(&output).unwrap();
        assert_eq!(stream.channel_count().get(), 2);
        assert_eq!(stream.sample_rate().get(), 44_100);
        let mut decoded = Vec::new();
        let mut count = 0;
        while stream.read_chunk(&mut decoded).unwrap() {
            for sample in &decoded {
                let expected = chunk[count as usize % chunk.len()];
                assert!(
                    (*sample - expected).abs() < 2.0e-7,
                    "sample {count}: {sample} vs {expected}"
                );
                count += 1;
            }
        }
        assert_eq!(count, samples);
        use flacenc::source::Fill;
        let mut expected = flacenc::source::Context::new(24, 2);
        let integers: Vec<i32> = chunk.iter().map(|sample| to_integer(*sample)).collect();
        for _ in 0..32 {
            expected.fill_interleaved(&integers).unwrap();
        }
        expected.fill_interleaved(&integers[..600]).unwrap();
        let bytes = std::fs::read(&output).unwrap();
        assert_eq!(&bytes[26..42], &expected.md5_digest());
    }
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
            File::create(folder.join("original.wav")).unwrap(),
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
