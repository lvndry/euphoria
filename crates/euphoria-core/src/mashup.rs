// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rendering a mashup to a WAV file: two or more tracks, or chosen stems of
//! them, played together from given points, each stretched to a shared
//! tempo and shifted to a shared key, mixed and written as a new file.
//!
//! The tracks themselves are only read. Each layer goes through Signalsmith
//! Stretch, the time stretcher behind the decks and the key change, and its
//! start latency is taken out, so the first frame of the file is the first
//! frame of every layer and beats placed together stay together.

use std::io::BufWriter;
use std::path::{Path, PathBuf};

use crate::audio_stream::AudioStream;
use crate::error::EuphoriaError;
use crate::shift::{
    ExistingOutput, MAXIMUM_SHIFT_SEMITONES, create_temporary_file, deinterleave, install,
    interleave, same_file,
};
use crate::signalsmith::Stretch;

/// The sample rate a mashup is rendered and written at.
pub const MASHUP_SAMPLE_RATE: u32 = 44_100;

/// The slowest and fastest a layer is played, as a share of its own speed.
pub const MASHUP_RATE_RANGE: (f64, f64) = (0.5, 2.0);

/// The loudest a sample of the written file is: 1 dB below full scale. A
/// mix that would go above is turned down as a whole.
pub const MASHUP_CEILING: f32 = 0.891;

/// Output frames rendered at a time.
const BLOCK_FRAMES: usize = 4_096;

/// The channels a mashup is rendered in.
const CHANNELS: usize = 2;

/// Where the sound of a layer comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum LayerAudio {
    /// A whole track.
    File(PathBuf),
    /// The stems of a track, each at its own level from 0 to 1: the files
    /// and levels in the same order, all files at one sample rate.
    Stems(Vec<(PathBuf, f32)>),
}

/// One track of a mashup and how it is played.
#[derive(Debug, Clone, PartialEq)]
pub struct MashupLayer {
    /// The track, or its stems.
    pub audio: LayerAudio,
    /// Where in the track the layer starts, in seconds: it is heard from
    /// the first frame of the mashup.
    pub start_seconds: f64,
    /// The speed it plays at, as a share of its own, within
    /// [`MASHUP_RATE_RANGE`]. The pitch stays put.
    pub rate: f64,
    /// Semitones its pitch is shifted by, within
    /// [`MAXIMUM_SHIFT_SEMITONES`] either way.
    pub shift_semitones: i8,
    /// Its level in the mix, from 0 to 1.
    pub gain: f32,
}

/// What [`write_mashup`] wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct WrittenMashup {
    /// The file written.
    pub path: PathBuf,
    /// Frames in the file, at [`MASHUP_SAMPLE_RATE`].
    pub frames: u64,
    /// The gain the whole mix was turned down by to stay under
    /// [`MASHUP_CEILING`], 1 when it was not.
    pub gain: f32,
}

/// Renders `layers` together, stereo and interleaved at
/// [`MASHUP_SAMPLE_RATE`], until the last layer ends, or for `seconds` when
/// given. Nothing is turned down: samples may exceed full scale. The whole
/// mix is held in memory; [`write_mashup`] renders a block at a time.
pub fn render_mashup(
    layers: &[MashupLayer],
    seconds: Option<f64>,
) -> Result<Vec<f32>, EuphoriaError> {
    let mut renderer = Renderer::new(layers, seconds)?;
    let mut mix = Vec::new();
    while let Some(block) = renderer.next_block()? {
        mix.extend_from_slice(block);
    }
    mix.truncate(renderer.length_frames() * CHANNELS);
    Ok(mix)
}

/// Mixes the layers a block at a time.
struct Renderer {
    voices: Vec<LayerVoice>,
    limit: Option<usize>,
    rendered: usize,
    block: Vec<f32>,
}

impl Renderer {
    fn new(layers: &[MashupLayer], seconds: Option<f64>) -> Result<Self, EuphoriaError> {
        if let Some(seconds) = seconds
            && !(seconds.is_finite() && seconds >= 0.0)
        {
            return Err(mashup_error(
                Path::new(""),
                format!("a length of {seconds} seconds is not a length"),
            ));
        }
        Ok(Self {
            voices: layers
                .iter()
                .map(LayerVoice::open)
                .collect::<Result<Vec<_>, _>>()?,
            limit: seconds.map(|seconds| (seconds * f64::from(MASHUP_SAMPLE_RATE)) as usize),
            rendered: 0,
            block: vec![0.0; BLOCK_FRAMES * CHANNELS],
        })
    }

    /// The next block of the mix, or `None` at the end. The last block may
    /// run past the end; [`Renderer::length_frames`] says where it is.
    fn next_block(&mut self) -> Result<Option<&[f32]>, EuphoriaError> {
        let frames = match self.limit {
            Some(limit) if self.rendered >= limit => return Ok(None),
            Some(limit) => BLOCK_FRAMES.min(limit - self.rendered),
            None if self.voices.iter().all(LayerVoice::is_finished) => return Ok(None),
            None => BLOCK_FRAMES,
        };
        let block = &mut self.block[..frames * CHANNELS];
        block.fill(0.0);
        for voice in &mut self.voices {
            voice.render(block)?;
        }
        self.rendered += frames;
        Ok(Some(&self.block[..frames * CHANNELS]))
    }

    /// Frames in the whole mix, once every block is rendered.
    fn length_frames(&self) -> usize {
        match self.limit {
            Some(limit) => limit.min(self.rendered),
            None => self
                .voices
                .iter()
                .filter_map(LayerVoice::end_frame)
                .max()
                .unwrap_or(0)
                .min(self.rendered),
        }
    }
}

/// Renders `layers` as [`render_mashup`] does and writes them to `output`
/// as a 24 bit stereo WAV file, turned down as a whole when the mix would
/// go above [`MASHUP_CEILING`].
///
/// The mix is rendered a block at a time into a spool file next to
/// `output`, then read back, turned down when needed and encoded, so memory
/// stays the same whatever the length.
///
/// No file of a layer is written to: an `output` that is one of them is an
/// error. The file is written under a temporary name next to `output` and
/// moved into place once complete, so a failure never leaves half a file.
pub fn write_mashup(
    layers: &[MashupLayer],
    seconds: Option<f64>,
    output: &Path,
    existing: ExistingOutput,
) -> Result<WrittenMashup, EuphoriaError> {
    for layer in layers {
        if layer.audio.paths().any(|path| same_file(path, output)) {
            return Err(mashup_error(
                output,
                "the output is one of the tracks, which are never changed".to_string(),
            ));
        }
    }
    let mut renderer = Renderer::new(layers, seconds)?;
    let file_error = |source| EuphoriaError::ExportFile {
        path: output.to_path_buf(),
        source,
    };
    let (spool_file, spool) = create_temporary_file(output).map_err(file_error)?;
    let result = (|| {
        let peak = spool_mix(&mut renderer, spool_file).map_err(file_error)?;
        let frames = renderer.length_frames();
        let gain = if peak > MASHUP_CEILING {
            MASHUP_CEILING / peak
        } else {
            1.0
        };
        let (file, temporary) = create_temporary_file(output).map_err(file_error)?;
        let written = write_wav(file, &spool, frames, gain)
            .map_err(|error| mashup_error(output, error.to_string()));
        let installed = written.and_then(|()| install(&temporary, output, existing));
        if let Err(error) = installed {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
        Ok(WrittenMashup {
            path: output.to_path_buf(),
            frames: frames as u64,
            gain,
        })
    })();
    let _ = std::fs::remove_file(&spool);
    result
}

/// Writes every block of the mix to `spool` as raw f32 samples and returns
/// the peak of the samples within the mix's length.
fn spool_mix(renderer: &mut Renderer, spool: std::fs::File) -> std::io::Result<f32> {
    use std::io::Write;
    let mut writer = BufWriter::new(spool);
    let mut peaks = Vec::new();
    while let Some(block) = renderer.next_block().map_err(std::io::Error::other)? {
        let mut block_peak = 0.0_f32;
        for sample in block {
            block_peak = block_peak.max(sample.abs());
            writer.write_all(&sample.to_le_bytes())?;
        }
        peaks.push(block_peak);
    }
    writer.flush()?;
    // The last block can run past the end with silence only, so every
    // block's peak counts.
    Ok(peaks.into_iter().fold(0.0, f32::max))
}

fn write_wav(
    file: std::fs::File,
    spool: &Path,
    frames: usize,
    gain: f32,
) -> Result<(), hound::Error> {
    use std::io::Read;
    let spec = hound::WavSpec {
        channels: CHANNELS as u16,
        sample_rate: MASHUP_SAMPLE_RATE,
        bits_per_sample: 24,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::new(BufWriter::new(file), spec)?;
    let mut reader = std::io::BufReader::new(std::fs::File::open(spool)?);
    let full_scale = f32::from(1_u16 << 15) * 256.0 - 1.0;
    let mut bytes = [0_u8; 4];
    for _ in 0..frames * CHANNELS {
        reader.read_exact(&mut bytes)?;
        let sample = f32::from_le_bytes(bytes) * gain;
        writer.write_sample((sample.clamp(-1.0, 1.0) * full_scale).round() as i32)?;
    }
    writer.finalize()
}

fn mashup_error(path: &Path, reason: String) -> EuphoriaError {
    EuphoriaError::Mashup {
        path: path.to_path_buf(),
        reason,
    }
}

impl LayerAudio {
    fn paths(&self) -> Box<dyn Iterator<Item = &Path> + '_> {
        match self {
            LayerAudio::File(path) => Box::new(std::iter::once(path.as_path())),
            LayerAudio::Stems(stems) => Box::new(stems.iter().map(|(path, _)| path.as_path())),
        }
    }
}

/// The sound of a layer before stretching: stereo frames at the files'
/// own rate, with stems mixed at their levels.
struct LayerReader {
    streams: Vec<(AudioStream, f32, Vec<f32>, usize)>,
    sample_rate: u32,
}

impl LayerReader {
    fn open(audio: &LayerAudio, start_seconds: f64) -> Result<Self, EuphoriaError> {
        let sources: Vec<(&Path, f32)> = match audio {
            LayerAudio::File(path) => vec![(path.as_path(), 1.0)],
            LayerAudio::Stems(stems) => stems
                .iter()
                .map(|(path, level)| (path.as_path(), level.clamp(0.0, 1.0)))
                .collect(),
        };
        let mut streams = Vec::with_capacity(sources.len());
        let mut sample_rate = None;
        for (path, level) in sources {
            let mut stream = AudioStream::open(path)?;
            let channels = stream.channel_count().get();
            if usize::from(channels) > CHANNELS {
                return Err(mashup_error(
                    path,
                    format!(
                        "it has {channels} channels; a mashup layer is mono or stereo, so mix it down first"
                    ),
                ));
            }
            let rate = stream.sample_rate().get();
            if *sample_rate.get_or_insert(rate) != rate {
                return Err(mashup_error(
                    path,
                    "the stems of a layer have different sample rates".to_string(),
                ));
            }
            if start_seconds > 0.0 {
                stream.seek(start_seconds as f32)?;
            }
            streams.push((stream, level, Vec::new(), 0));
        }
        let Some(sample_rate) = sample_rate else {
            return Err(mashup_error(
                Path::new(""),
                "a layer has no audio".to_string(),
            ));
        };
        Ok(Self {
            streams,
            sample_rate,
        })
    }

    /// Adds the next `frames` stereo frames to `output`, interleaved, and
    /// returns how many frames there were before every file ended; the
    /// rest are silence.
    fn read(&mut self, output: &mut Vec<f32>, frames: usize) -> Result<usize, EuphoriaError> {
        let start = output.len();
        output.resize(start + frames * CHANNELS, 0.0);
        let mut longest = 0;
        for (stream, level, chunk, used) in &mut self.streams {
            let channels = usize::from(stream.channel_count().get());
            let mut frame = 0;
            while frame < frames {
                if *used >= chunk.len() {
                    *used = 0;
                    if !stream.read_chunk(chunk)? {
                        break;
                    }
                }
                let left = chunk[*used];
                let right = if channels > 1 { chunk[*used + 1] } else { left };
                output[start + frame * CHANNELS] += left * *level;
                output[start + frame * CHANNELS + 1] += right * *level;
                *used += channels;
                frame += 1;
            }
            longest = longest.max(frame);
        }
        Ok(longest)
    }
}

/// A layer being rendered.
struct LayerVoice {
    reader: LayerReader,
    stretch: Stretch,
    /// Input frames per output frame.
    speed: f64,
    gain: f32,
    /// The part of an input frame owed from the last block.
    input_fraction: f64,
    /// Input frames read that held sound, pre-roll included.
    sounding_input: usize,
    /// True once every file of the layer has ended.
    input_ended: bool,
    /// Output frames handed out, after the start latency.
    emitted: usize,
    input: Vec<f32>,
    output: Vec<f32>,
}

impl LayerVoice {
    fn open(layer: &MashupLayer) -> Result<Self, EuphoriaError> {
        let first = layer.audio.paths().next().unwrap_or(Path::new(""));
        let first_path = first.to_path_buf();
        let failure = |reason: String| mashup_error(&first_path, reason);
        let (slowest, fastest) = MASHUP_RATE_RANGE;
        if !(slowest..=fastest).contains(&layer.rate) {
            return Err(failure(format!(
                "a speed of {} is outside {slowest} to {fastest}",
                layer.rate
            )));
        }
        if layer.shift_semitones.unsigned_abs() > MAXIMUM_SHIFT_SEMITONES.unsigned_abs() {
            return Err(failure(format!(
                "a shift of {} semitones is more than {MAXIMUM_SHIFT_SEMITONES} either way",
                layer.shift_semitones
            )));
        }
        let mut reader = LayerReader::open(&layer.audio, layer.start_seconds)?;
        let mut stretch = Stretch::new(CHANNELS, MASHUP_SAMPLE_RATE).map_err(failure)?;
        // The stretcher keeps the pitch in samples, so a file at another
        // rate than the mix is transposed back as well as shifted.
        let rate_correction =
            12.0 * (f64::from(reader.sample_rate) / f64::from(MASHUP_SAMPLE_RATE)).log2();
        stretch
            .set_transpose_semitones(f32::from(layer.shift_semitones) + rate_correction as f32)
            .map_err(failure)?;
        let speed = layer.rate * f64::from(reader.sample_rate) / f64::from(MASHUP_SAMPLE_RATE);
        let mut preroll = Vec::new();
        let preroll_frames = stretch.input_latency();
        let sounding = reader.read(&mut preroll, preroll_frames)?;
        stretch
            .seek(&deinterleave(&preroll, CHANNELS), speed)
            .map_err(failure)?;
        let start_latency = stretch.output_latency();
        let mut voice = Self {
            reader,
            stretch,
            speed,
            gain: layer.gain.clamp(0.0, 1.0),
            input_fraction: 0.0,
            sounding_input: sounding,
            input_ended: sounding < preroll_frames,
            emitted: 0,
            input: Vec::new(),
            output: Vec::new(),
        };
        // The output trails the input by the start latency: what comes
        // out first is from before the layer's start, and is dropped.
        voice.advance(start_latency)?;
        voice.emitted = 0;
        Ok(voice)
    }

    /// The output frame at which the layer's sound ends, once its files
    /// have ended.
    fn end_frame(&self) -> Option<usize> {
        self.input_ended
            .then(|| (self.sounding_input as f64 / self.speed).ceil() as usize)
    }

    fn is_finished(&self) -> bool {
        self.end_frame()
            .is_some_and(|end_frame| self.emitted >= end_frame)
    }

    /// Stretches the next `frames` output frames into `output`.
    fn advance(&mut self, frames: usize) -> Result<(), EuphoriaError> {
        let exact = frames as f64 * self.speed + self.input_fraction;
        let input_frames = exact.floor() as usize;
        self.input_fraction = exact - input_frames as f64;
        self.input.clear();
        let sounding = self.reader.read(&mut self.input, input_frames)?;
        if !self.input_ended {
            self.sounding_input += sounding;
            self.input_ended = sounding < input_frames;
        }
        let mut outputs = vec![vec![0.0; frames]; CHANNELS];
        if frames > 0 {
            self.stretch
                .process(&deinterleave(&self.input, CHANNELS), &mut outputs)
                .map_err(|reason| mashup_error(Path::new(""), reason))?;
        }
        self.output = interleave(&outputs);
        self.emitted += frames;
        Ok(())
    }

    /// Adds the layer's next frames to `block`, stereo and interleaved.
    fn render(&mut self, block: &mut [f32]) -> Result<(), EuphoriaError> {
        if self.is_finished() {
            return Ok(());
        }
        let frames = block.len() / CHANNELS;
        let first = self.emitted;
        self.advance(frames)?;
        let usable = self.end_frame().map_or(frames, |end_frame| {
            end_frame.saturating_sub(first).min(frames)
        });
        for (mixed, sample) in block.iter_mut().zip(&self.output).take(usable * CHANNELS) {
            *mixed += sample * self.gain;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_mono(path: &Path, sample_rate: u32, samples: &[f32]) {
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(path, spec).unwrap();
        for sample in samples {
            writer
                .write_sample((sample.clamp(-1.0, 1.0) * 32_767.0) as i16)
                .unwrap();
        }
        writer.finalize().unwrap();
    }

    /// Short clicks at `bpm` from `first_beat` seconds, for `seconds`.
    fn clicks(sample_rate: u32, bpm: f64, first_beat: f64, seconds: f64) -> Vec<f32> {
        let rate = f64::from(sample_rate);
        let mut samples = vec![0.0; (seconds * rate) as usize];
        let mut beat = first_beat;
        while beat < seconds {
            let start = (beat * rate) as usize;
            for offset in 0..(0.003 * rate) as usize {
                if let Some(sample) = samples.get_mut(start + offset) {
                    let time = offset as f64 / rate;
                    *sample = (0.8
                        * (-time / 0.001).exp()
                        * (std::f64::consts::TAU * 2_000.0 * time).sin())
                        as f32;
                }
            }
            beat += 60.0 / bpm;
        }
        samples
    }

    fn tone(sample_rate: u32, hertz: f32, seconds: f32) -> Vec<f32> {
        (0..(seconds * sample_rate as f32) as usize)
            .map(|index| {
                0.5 * (std::f32::consts::TAU * hertz * index as f32 / sample_rate as f32).sin()
            })
            .collect()
    }

    /// Seconds at which the left channel rises above a threshold after a
    /// quiet stretch.
    fn onsets(mix: &[f32]) -> Vec<f64> {
        let mut found = Vec::new();
        let mut quiet = usize::MAX;
        let (frames, _) = mix.as_chunks::<2>();
        for (frame, pair) in frames.iter().enumerate() {
            if pair[0].abs() > 0.1 {
                if quiet > 2_000 {
                    found.push(frame as f64 / f64::from(MASHUP_SAMPLE_RATE));
                }
                quiet = 0;
            } else {
                quiet = quiet.saturating_add(1);
            }
        }
        found
    }

    /// The frequency of the left channel, from its rising zero crossings.
    fn frequency(mix: &[f32]) -> f64 {
        let left: Vec<f32> = mix.as_chunks::<2>().0.iter().map(|pair| pair[0]).collect();
        let middle = &left[left.len() / 4..left.len() * 3 / 4];
        let crossings = middle
            .windows(2)
            .filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0)
            .count();
        crossings as f64 * f64::from(MASHUP_SAMPLE_RATE) / middle.len() as f64
    }

    fn layer(path: &Path, start_seconds: f64, rate: f64) -> MashupLayer {
        MashupLayer {
            audio: LayerAudio::File(path.to_path_buf()),
            start_seconds,
            rate,
            shift_semitones: 0,
            gain: 1.0,
        }
    }

    #[test]
    fn a_stretched_layer_from_another_rate_keeps_to_the_first_layers_beats() {
        let folder = tempfile::tempdir().unwrap();
        let first = folder.path().join("first.wav");
        let second = folder.path().join("second.wav");
        write_mono(&first, 44_100, &clicks(44_100, 120.0, 0.5, 12.0));
        write_mono(&second, 48_000, &clicks(48_000, 125.0, 0.3, 12.0));
        let first_layer = layer(&first, 0.5, 1.0);
        let second_layer = layer(&second, 0.3, 120.0 / 125.0);
        let first_onsets = onsets(&render_mashup(&[first_layer], Some(8.0)).unwrap());
        let second_onsets = onsets(&render_mashup(&[second_layer], Some(8.0)).unwrap());
        assert_eq!(first_onsets.len(), 16, "{first_onsets:?}");
        assert_eq!(second_onsets.len(), 16, "{second_onsets:?}");
        for (beat, (first, second)) in first_onsets.iter().zip(&second_onsets).enumerate() {
            let expected = beat as f64 * 0.5;
            assert!((first - expected).abs() < 0.003, "beat {beat}: {first}");
            assert!((second - expected).abs() < 0.003, "beat {beat}: {second}");
        }
    }

    #[test]
    fn a_shift_moves_the_pitch_without_changing_the_length() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("tone.wav");
        write_mono(&path, 44_100, &tone(44_100, 440.0, 3.0));
        let mut shifted = layer(&path, 0.0, 1.0);
        shifted.shift_semitones = 2;
        let mix = render_mashup(&[shifted], None).unwrap();
        let expected = 440.0 * 2_f64.powf(2.0 / 12.0);
        assert!(
            (frequency(&mix) - expected).abs() < 3.0,
            "{}",
            frequency(&mix)
        );
        let frames = mix.len() / 2;
        assert!((frames as i64 - 3 * 44_100).abs() <= 1, "{frames}");
    }

    #[test]
    fn stems_are_mixed_at_their_levels() {
        let folder = tempfile::tempdir().unwrap();
        let low = folder.path().join("low.wav");
        let high = folder.path().join("high.wav");
        write_mono(&low, 44_100, &tone(44_100, 220.0, 2.0));
        write_mono(&high, 44_100, &tone(44_100, 1_760.0, 2.0));
        let stems = |low_level: f32, high_level: f32| MashupLayer {
            audio: LayerAudio::Stems(vec![(low.clone(), low_level), (high.clone(), high_level)]),
            ..layer(&low, 0.0, 1.0)
        };
        let low_only = render_mashup(&[stems(1.0, 0.0)], None).unwrap();
        assert!((frequency(&low_only) - 220.0).abs() < 3.0);
        let high_only = render_mashup(&[stems(0.0, 1.0)], None).unwrap();
        assert!((frequency(&high_only) - 1_760.0).abs() < 5.0);
    }

    #[test]
    fn the_mashup_lasts_until_its_longest_layer_ends() {
        let folder = tempfile::tempdir().unwrap();
        let long = folder.path().join("long.wav");
        let short = folder.path().join("short.wav");
        write_mono(&long, 44_100, &tone(44_100, 300.0, 2.0));
        write_mono(&short, 44_100, &tone(44_100, 300.0, 1.0));
        let mix = render_mashup(&[layer(&short, 0.0, 1.0), layer(&long, 0.5, 1.0)], None).unwrap();
        let frames = mix.len() / 2;
        assert!((frames as i64 - 66_150).abs() <= 1, "{frames} frames");
        let slowed = render_mashup(&[layer(&short, 0.0, 0.5)], None).unwrap();
        assert!(((slowed.len() / 2) as i64 - 88_200).abs() <= 1);
    }

    #[test]
    fn writing_turns_a_loud_mix_down_and_never_touches_the_tracks() {
        let folder = tempfile::tempdir().unwrap();
        let track = folder.path().join("track.wav");
        write_mono(&track, 44_100, &tone(44_100, 440.0, 1.0));
        let before = std::fs::read(&track).unwrap();
        let loud = [layer(&track, 0.0, 1.0), layer(&track, 0.0, 1.0)];
        let output = folder.path().join("mashup.wav");
        let written = write_mashup(&loud, None, &output, ExistingOutput::Keep).unwrap();
        assert!(
            written.gain < 0.95,
            "two 0.5 tones peak near 1: {}",
            written.gain
        );
        let reader = hound::WavReader::open(&output).unwrap();
        let spec = reader.spec();
        assert_eq!(
            (spec.channels, spec.sample_rate, spec.bits_per_sample),
            (2, 44_100, 24)
        );
        assert_eq!(u64::from(reader.duration()), written.frames);
        let peak = reader
            .into_samples::<i32>()
            .map(|sample| sample.unwrap().unsigned_abs())
            .max()
            .unwrap() as f32
            / 8_388_607.0;
        assert!((peak - MASHUP_CEILING).abs() < 0.01, "{peak}");
        assert_eq!(std::fs::read(&track).unwrap(), before);

        let refused = write_mashup(&loud, None, &track, ExistingOutput::Replace);
        assert!(matches!(refused, Err(EuphoriaError::Mashup { .. })));
        assert_eq!(std::fs::read(&track).unwrap(), before);
        assert!(write_mashup(&loud, None, &output, ExistingOutput::Keep).is_err());
    }

    #[test]
    fn a_layer_ends_where_its_track_does_from_any_start_and_speed() {
        let folder = tempfile::tempdir().unwrap();
        let short = folder.path().join("short.wav");
        write_mono(&short, 44_100, &tone(44_100, 300.0, 1.0));
        for (start, rate, expected) in
            [(0.25, 1.0, 33_075), (0.25, 0.5, 66_150), (0.5, 2.0, 11_025)]
        {
            let mix = render_mashup(&[layer(&short, start, rate)], None).unwrap();
            let frames = (mix.len() / 2) as i64;
            assert!((frames - expected).abs() <= 1, "{start} {rate}: {frames}");
        }
    }

    #[test]
    fn surround_layers_and_endless_lengths_are_refused() {
        let folder = tempfile::tempdir().unwrap();
        let surround = folder.path().join("surround.wav");
        let spec = hound::WavSpec {
            channels: 3,
            sample_rate: 44_100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&surround, spec).unwrap();
        for _ in 0..44_100 * 3 {
            writer.write_sample(1_000_i16).unwrap();
        }
        writer.finalize().unwrap();
        let refused = render_mashup(&[layer(&surround, 0.0, 1.0)], None);
        assert!(
            matches!(&refused, Err(EuphoriaError::Mashup { reason, .. }) if reason.contains("3 channels")),
            "{refused:?}"
        );
        let stereo = folder.path().join("tone.wav");
        write_mono(&stereo, 44_100, &tone(44_100, 440.0, 0.5));
        for endless in [f64::INFINITY, f64::NAN, -1.0] {
            assert!(render_mashup(&[layer(&stereo, 0.0, 1.0)], Some(endless)).is_err());
        }
    }

    #[test]
    fn writing_leaves_no_spool_or_partial_file_behind() {
        let folder = tempfile::tempdir().unwrap();
        let track = folder.path().join("track.wav");
        write_mono(&track, 44_100, &tone(44_100, 440.0, 1.0));
        let output = folder.path().join("mashup.wav");
        let written = write_mashup(
            &[layer(&track, 0.0, 1.0)],
            None,
            &output,
            ExistingOutput::Keep,
        )
        .unwrap();
        assert_eq!(written.gain, 1.0);
        assert!((written.frames as i64 - 44_100).abs() <= 1);
        let mut names: Vec<String> = std::fs::read_dir(folder.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["mashup.wav", "track.wav"]);
    }

    #[test]
    fn speeds_and_shifts_out_of_range_are_refused() {
        let folder = tempfile::tempdir().unwrap();
        let track = folder.path().join("track.wav");
        write_mono(&track, 44_100, &tone(44_100, 440.0, 0.5));
        assert!(render_mashup(&[layer(&track, 0.0, 3.0)], None).is_err());
        let mut shifted = layer(&track, 0.0, 1.0);
        shifted.shift_semitones = 7;
        assert!(render_mashup(&[shifted], None).is_err());
    }
}
