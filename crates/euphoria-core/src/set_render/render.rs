// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Writing a mix a block at a time: each track is decoded, stretched to the
//! speed its placement asks for, filtered for the bass swap, faded, summed
//! and kept under the ceiling, then encoded. Memory stays the same whatever
//! the length of the set.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use super::timeline::{MixTimeline, Placement, mix_timeline};
use super::wav::WavWriter;
use super::{MixPlan, RenderTrack};
use crate::audio_stream::AudioStream;
use crate::error::EuphoriaError;
use crate::shift::flac::FlacWriter;
use crate::shift::{
    ExistingOutput, create_temporary_file, deinterleave, install, interleave, same_file,
};
use crate::signalsmith::Stretch;

/// The loudest a sample of the mix is: 1 dB below full scale. Louder
/// moments, where two tracks add up, are turned down by a limiter that
/// sees them coming.
pub const MIX_CEILING: f32 = 0.891;

/// The channels of a mix.
const CHANNELS: usize = 2;
/// Output frames rendered at a time.
const BLOCK_FRAMES: usize = 4_096;
/// Where the bass of a track is split from the rest for the bass swap.
const BASS_SPLIT_HERTZ: f64 = 180.0;
/// How far ahead the limiter looks.
const LIMITER_LOOKAHEAD_SECONDS: f64 = 0.005;
/// How fast the limiter lets go.
const LIMITER_RELEASE_SECONDS: f64 = 0.15;

/// The file formats a mix is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixFormat {
    /// 24 bit WAV, RF64 past 4 GiB.
    Wav,
    /// 24 bit FLAC.
    Flac,
    /// Constant bitrate MP3 from LAME, with the title and a chapter per
    /// track in its ID3v2 tag.
    Mp3 {
        /// Kilobits per second, one of [`MP3_BITRATES`].
        bitrate_kbps: u16,
    },
}

/// The bitrates an MP3 mix can be written at, in kilobits per second.
pub const MP3_BITRATES: [u16; 8] = [96, 112, 128, 160, 192, 224, 256, 320];

/// The bitrate of an MP3 mix unless another is asked for.
pub const DEFAULT_MP3_BITRATE_KBPS: u16 = 320;

impl MixFormat {
    /// The format the extension of `path` names, if it is one a mix can be
    /// written in.
    pub fn of_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?.to_ascii_lowercase();
        match extension.as_str() {
            "wav" => Some(Self::Wav),
            "flac" => Some(Self::Flac),
            "mp3" => Some(Self::Mp3 {
                bitrate_kbps: DEFAULT_MP3_BITRATE_KBPS,
            }),
            _ => None,
        }
    }

    /// The extension of the format, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Mp3 { .. } => "mp3",
        }
    }
}

/// How to write a mix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    /// The file format.
    pub format: MixFormat,
    /// What to do when the output exists.
    pub existing: ExistingOutput,
}

/// Hears how far a render has got and may stop it.
pub trait RenderProgress {
    /// `done_seconds` of the mix's `total_seconds` are written.
    fn rendered(&mut self, done_seconds: f64, total_seconds: f64);

    /// True to stop the render. Nothing is written then.
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// A [`RenderProgress`] that listens to nothing and never stops.
pub struct NoProgress;

impl RenderProgress for NoProgress {
    fn rendered(&mut self, _done_seconds: f64, _total_seconds: f64) {}
}

/// What [`render_mix`] wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderedMix {
    /// The file written.
    pub path: PathBuf,
    /// Frames in the file.
    pub frames: u64,
    /// Its sample rate.
    pub sample_rate: u32,
    /// Where each track sits in it.
    pub timeline: MixTimeline,
}

/// Renders `plan` as one stereo mix and writes it to `output`, 24 bit, in
/// the format of `options`.
///
/// The sample rate is the one most tracks have, the first track's on a
/// tie; tracks at another rate are converted. The tracks are only read: an
/// `output` that is one of them is an error. The mix is written under a
/// temporary name next to `output` and moved into place once complete, so
/// a failure or a cancel never leaves half a file.
pub fn render_mix(
    plan: &MixPlan,
    output: &Path,
    options: &RenderOptions,
    progress: &mut dyn RenderProgress,
) -> Result<RenderedMix, EuphoriaError> {
    if plan.tracks.is_empty() {
        return Err(render_error(output, "the set has no tracks".to_string()));
    }
    for track in &plan.tracks {
        if same_file(&track.path, output) {
            return Err(render_error(
                output,
                "the output is one of the tracks, which are never changed".to_string(),
            ));
        }
    }
    let timeline = mix_timeline(plan).map_err(|reason| render_error(output, reason))?;
    let sample_rate = mix_sample_rate(&plan.tracks)?;
    let total_frames = (timeline.length_seconds * f64::from(sample_rate)).round() as u64;

    let (file, temporary) =
        create_temporary_file(output).map_err(|source| EuphoriaError::ExportFile {
            path: output.to_path_buf(),
            source,
        })?;
    let written = (|| {
        let mut writer = MixWriter::create(
            file,
            &temporary,
            options.format,
            sample_rate,
            &MixTags::of(plan, &timeline, output),
        )?;
        let mut mixer = Mixer::new(plan, &timeline, sample_rate);
        let mut limiter = Limiter::new(sample_rate);
        let mut block = Vec::with_capacity(BLOCK_FRAMES * CHANNELS);
        let mut limited = Vec::with_capacity(BLOCK_FRAMES * CHANNELS);
        let mut rendered = 0_u64;
        while rendered < total_frames {
            if progress.is_cancelled() {
                return Err(EuphoriaError::MixCancelled);
            }
            let frames = (total_frames - rendered).min(BLOCK_FRAMES as u64) as usize;
            mixer.render(rendered, frames, &mut block)?;
            limited.clear();
            limiter.process(&block, &mut limited);
            writer.write(&limited)?;
            rendered += frames as u64;
            progress.rendered(
                rendered as f64 / f64::from(sample_rate),
                timeline.length_seconds,
            );
        }
        limited.clear();
        limiter.finish(&mut limited);
        writer.write(&limited)?;
        writer.finish()
    })();
    let samples = match written {
        Ok(samples) => samples,
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
    };
    if let Err(error) = install(&temporary, output, options.existing) {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    Ok(RenderedMix {
        path: output.to_path_buf(),
        frames: samples / CHANNELS as u64,
        sample_rate,
        timeline,
    })
}

fn render_error(path: &Path, reason: String) -> EuphoriaError {
    EuphoriaError::MixRender {
        path: path.to_path_buf(),
        reason,
    }
}

/// The sample rate most tracks have, the first track's on a tie. Every
/// track is opened, so a file that cannot be read fails before anything is
/// rendered.
fn mix_sample_rate(tracks: &[RenderTrack]) -> Result<u32, EuphoriaError> {
    let mut counts: Vec<(u32, usize)> = Vec::new();
    for track in tracks {
        let rate = AudioStream::open(&track.path)?.sample_rate().get();
        match counts.iter_mut().find(|(known, _)| *known == rate) {
            Some((_, count)) => *count += 1,
            None => counts.push((rate, 1)),
        }
    }
    let most = counts.iter().map(|(_, count)| *count).max().unwrap_or(0);
    Ok(counts
        .iter()
        .find(|(_, count)| *count == most)
        .map_or(44_100, |(rate, _)| *rate))
}

enum MixWriter {
    Wav(WavWriter, PathBuf),
    Flac(Box<FlacWriter>),
    #[cfg(feature = "mp3")]
    Mp3(Box<super::mp3::Mp3Writer>, PathBuf),
}

/// What a file format that carries tags is told of the mix: its title,
/// from the name of the file, and where each track starts and ends.
struct MixTags {
    title: String,
    chapters: Vec<(String, f64, f64)>,
}

impl MixTags {
    fn of(plan: &MixPlan, timeline: &MixTimeline, output: &Path) -> Self {
        let starts: Vec<f64> = timeline
            .placements
            .iter()
            .map(|placement| placement.start_seconds)
            .collect();
        let chapters = plan
            .tracks
            .iter()
            .zip(&starts)
            .enumerate()
            .map(|(index, (track, start))| {
                let end = starts
                    .get(index + 1)
                    .copied()
                    .unwrap_or(timeline.length_seconds);
                (track.display_name(), *start, end)
            })
            .collect();
        Self {
            title: output
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default(),
            chapters,
        }
    }
}

impl MixWriter {
    fn create(
        file: std::fs::File,
        path: &Path,
        format: MixFormat,
        sample_rate: u32,
        tags: &MixTags,
    ) -> Result<Self, EuphoriaError> {
        Ok(match format {
            MixFormat::Wav => Self::Wav(
                WavWriter::new(file, CHANNELS as u16, sample_rate)
                    .map_err(|error| render_error(path, error.to_string()))?,
                path.to_path_buf(),
            ),
            MixFormat::Flac => Self::Flac(Box::new(FlacWriter::new(
                file,
                path,
                CHANNELS as u16,
                sample_rate,
                render_error,
            )?)),
            #[cfg(feature = "mp3")]
            MixFormat::Mp3 { bitrate_kbps } => {
                let chapters: Vec<super::mp3::Chapter> = tags
                    .chapters
                    .iter()
                    .map(|(title, start, end)| super::mp3::Chapter {
                        title: title.clone(),
                        start_milliseconds: (start * 1000.0).round() as u32,
                        end_milliseconds: (end * 1000.0).round() as u32,
                    })
                    .collect();
                let writer = super::mp3::Mp3Writer::new(
                    file,
                    sample_rate,
                    bitrate_kbps,
                    &tags.title,
                    &chapters,
                )
                .map_err(|reason| render_error(path, reason))?;
                Self::Mp3(Box::new(writer), path.to_path_buf())
            }
            #[cfg(not(feature = "mp3"))]
            MixFormat::Mp3 { .. } => {
                let _ = (file, &tags.title, &tags.chapters);
                return Err(render_error(
                    path,
                    "this build of Euphoria has no MP3 encoder".to_string(),
                ));
            }
        })
    }

    fn write(&mut self, interleaved: &[f32]) -> Result<(), EuphoriaError> {
        match self {
            Self::Wav(writer, path) => writer
                .write(interleaved)
                .map_err(|error| render_error(path, error.to_string())),
            Self::Flac(writer) => writer.write(interleaved),
            #[cfg(feature = "mp3")]
            Self::Mp3(writer, path) => writer
                .write(interleaved)
                .map_err(|reason| render_error(path, reason)),
        }
    }

    fn finish(self) -> Result<u64, EuphoriaError> {
        match self {
            Self::Wav(writer, path) => writer
                .finish()
                .map_err(|error| render_error(&path, error.to_string())),
            Self::Flac(writer) => writer.finish(),
            #[cfg(feature = "mp3")]
            Self::Mp3(writer, path) => writer
                .finish()
                .map_err(|reason| render_error(&path, reason)),
        }
    }
}

/// Sums the tracks that play in each block.
struct Mixer<'plan> {
    plan: &'plan MixPlan,
    timeline: &'plan MixTimeline,
    sample_rate: u32,
    next_track: usize,
    voices: Vec<Voice>,
    scratch: Vec<f32>,
}

impl<'plan> Mixer<'plan> {
    fn new(plan: &'plan MixPlan, timeline: &'plan MixTimeline, sample_rate: u32) -> Self {
        Self {
            plan,
            timeline,
            sample_rate,
            next_track: 0,
            voices: Vec::new(),
            scratch: Vec::new(),
        }
    }

    /// Fills `block` with `frames` frames of the mix from frame `first`.
    fn render(
        &mut self,
        first: u64,
        frames: usize,
        block: &mut Vec<f32>,
    ) -> Result<(), EuphoriaError> {
        block.clear();
        block.resize(frames * CHANNELS, 0.0);
        let last = first + frames as u64;
        while let Some(placement) = self.timeline.placements.get(self.next_track) {
            if self.frame_of(placement.start_seconds) >= last {
                break;
            }
            let track = &self.plan.tracks[self.next_track];
            self.voices.push(Voice::open(
                track,
                placement.clone(),
                self.sample_rate,
                self.frame_of(placement.start_seconds),
                self.frame_of(placement.end_seconds),
            )?);
            self.next_track += 1;
        }
        for voice in &mut self.voices {
            let from = voice.start_frame.max(first);
            let to = voice.end_frame.min(last);
            if from >= to {
                continue;
            }
            let count = (to - from) as usize;
            voice.render(count, &mut self.scratch)?;
            let offset = (from - first) as usize * CHANNELS;
            for (mixed, sample) in block[offset..offset + count * CHANNELS]
                .iter_mut()
                .zip(&self.scratch)
            {
                *mixed += sample;
            }
        }
        self.voices.retain(|voice| voice.end_frame > last);
        Ok(())
    }

    fn frame_of(&self, seconds: f64) -> u64 {
        (seconds * f64::from(self.sample_rate)).round().max(0.0) as u64
    }
}

/// Reads one file as stereo frames, with silence once it ends.
struct StreamReader {
    stream: AudioStream,
    channels: usize,
    chunk: Vec<f32>,
    used: usize,
    ended: bool,
}

impl StreamReader {
    fn open(path: &Path, start_seconds: f64) -> Result<Self, EuphoriaError> {
        let mut stream = AudioStream::open(path)?;
        if start_seconds > 0.0 {
            stream.seek(start_seconds as f32)?;
        }
        Ok(Self {
            channels: usize::from(stream.channel_count().get()),
            stream,
            chunk: Vec::new(),
            used: 0,
            ended: false,
        })
    }

    /// Appends `frames` stereo frames to `output`, interleaved.
    fn read(&mut self, output: &mut Vec<f32>, frames: usize) -> Result<(), EuphoriaError> {
        let start = output.len();
        output.resize(start + frames * CHANNELS, 0.0);
        let mut frame = 0;
        while frame < frames && !self.ended {
            if self.used >= self.chunk.len() {
                self.used = 0;
                if !self.stream.read_chunk(&mut self.chunk)? {
                    self.ended = true;
                    break;
                }
            }
            let left = self.chunk[self.used];
            let right = if self.channels > 1 {
                self.chunk[self.used + 1]
            } else {
                left
            };
            output[start + frame * CHANNELS] = left;
            output[start + frame * CHANNELS + 1] = right;
            self.used += self.channels;
            frame += 1;
        }
        Ok(())
    }
}

/// How long the switch from a track to its stems without vocals takes:
/// one beat at 120 BPM.
const VOCAL_CUT_SECONDS: f64 = 0.5;

/// The stems of a track without its vocals, which take over from the
/// track at a point of its own audio.
struct Instrumental {
    stems: Vec<StreamReader>,
    /// The frame, counted from where reading started, at which the stems
    /// have fully taken over.
    switch_frame: f64,
    fade_frames: f64,
    scratch: Vec<f32>,
}

/// Reads a track as stereo frames, switching to its stems without vocals
/// when its blend out cuts them.
struct Reader {
    track: StreamReader,
    instrumental: Option<Instrumental>,
    /// Frames read so far.
    position: u64,
}

impl Reader {
    fn open(track: &RenderTrack, placement: &Placement) -> Result<Self, EuphoriaError> {
        let start_seconds = placement.source_start_seconds;
        let reader = StreamReader::open(&track.path, start_seconds)?;
        let instrumental = match placement.instrumental_from_seconds {
            Some(from_seconds) => {
                let sample_rate = f64::from(reader.stream.sample_rate().get());
                let mut stems = Vec::with_capacity(track.instrumental_stems.len());
                for stem in &track.instrumental_stems {
                    let opened = StreamReader::open(stem, start_seconds)?;
                    if opened.stream.sample_rate() != reader.stream.sample_rate() {
                        return Err(render_error(
                            stem,
                            "the stems are at another sample rate than the track, so its vocals \
                             cannot be cut"
                                .to_string(),
                        ));
                    }
                    stems.push(opened);
                }
                Some(Instrumental {
                    stems,
                    switch_frame: (from_seconds - start_seconds) * sample_rate,
                    fade_frames: VOCAL_CUT_SECONDS * sample_rate,
                    scratch: Vec::new(),
                })
            }
            None => None,
        };
        Ok(Self {
            track: reader,
            instrumental,
            position: 0,
        })
    }

    fn sample_rate(&self) -> u32 {
        self.track.stream.sample_rate().get()
    }

    /// Appends `frames` stereo frames to `output`, interleaved.
    fn read(&mut self, output: &mut Vec<f32>, frames: usize) -> Result<(), EuphoriaError> {
        let start = output.len();
        self.track.read(output, frames)?;
        if let Some(instrumental) = &mut self.instrumental {
            instrumental.scratch.clear();
            instrumental.scratch.resize(frames * CHANNELS, 0.0);
            let mut stem_frames = Vec::with_capacity(frames * CHANNELS);
            for stem in &mut instrumental.stems {
                stem_frames.clear();
                stem.read(&mut stem_frames, frames)?;
                for (sum, sample) in instrumental.scratch.iter_mut().zip(&stem_frames) {
                    *sum += sample;
                }
            }
            for (index, (frame, stems)) in output[start..]
                .as_chunks_mut::<CHANNELS>()
                .0
                .iter_mut()
                .zip(instrumental.scratch.as_chunks::<CHANNELS>().0)
                .enumerate()
            {
                let at = (self.position + index as u64) as f64;
                let share = ((at - (instrumental.switch_frame - instrumental.fade_frames))
                    / instrumental.fade_frames)
                    .clamp(0.0, 1.0) as f32;
                for (sample, stem) in frame.iter_mut().zip(stems) {
                    *sample = *sample * (1.0 - share) + stem * share;
                }
            }
        }
        self.position += frames as u64;
        Ok(())
    }
}

/// How a track's sound reaches the mix.
enum Stage {
    /// Untouched: the track plays at its own speed and the mix's rate.
    Direct,
    /// Through the time stretcher, which keeps the pitch.
    Stretched {
        stretch: Box<Stretch>,
        /// Input frames given so far, the pre-roll included.
        consumed: u64,
        /// Input frames given before the first output frame.
        preroll: u64,
        /// Output frames the stretcher trails by.
        latency: usize,
        /// The track's sample rate over the mix's.
        rate_ratio: f64,
    },
}

/// One track being played into the mix.
struct Voice {
    placement: Placement,
    path: PathBuf,
    reader: Reader,
    stage: Stage,
    sample_rate: u32,
    source_rate: u32,
    start_frame: u64,
    end_frame: u64,
    /// Output frames handed out so far.
    emitted: u64,
    bass_filters: [Biquad; CHANNELS],
    input: Vec<f32>,
}

impl Voice {
    fn open(
        track: &RenderTrack,
        placement: Placement,
        sample_rate: u32,
        start_frame: u64,
        end_frame: u64,
    ) -> Result<Self, EuphoriaError> {
        let failure = |reason: String| render_error(&track.path, reason);
        let mut reader = Reader::open(track, &placement)?;
        let source_rate = reader.sample_rate();
        let stage = if placement.is_stretched() || source_rate != sample_rate {
            let rate_ratio = f64::from(source_rate) / f64::from(sample_rate);
            let mut stretch = Stretch::new(CHANNELS, sample_rate).map_err(failure)?;
            // The stretcher keeps the pitch in samples, so a track at
            // another rate than the mix is transposed back.
            stretch
                .set_transpose_factor(rate_ratio as f32)
                .map_err(failure)?;
            let preroll_frames = stretch.input_latency();
            let mut preroll = Vec::new();
            reader.read(&mut preroll, preroll_frames)?;
            let speed = placement.entry_rate * rate_ratio;
            stretch
                .seek(&deinterleave(&preroll, CHANNELS), speed)
                .map_err(failure)?;
            Stage::Stretched {
                latency: stretch.output_latency(),
                stretch: Box::new(stretch),
                consumed: preroll_frames as u64,
                preroll: preroll_frames as u64,
                rate_ratio,
            }
        } else {
            Stage::Direct
        };
        let mut voice = Self {
            placement,
            path: track.path.clone(),
            reader,
            stage,
            sample_rate,
            source_rate,
            start_frame,
            end_frame,
            emitted: 0,
            bass_filters: [
                Biquad::low_pass(BASS_SPLIT_HERTZ, sample_rate),
                Biquad::low_pass(BASS_SPLIT_HERTZ, sample_rate),
            ],
            input: Vec::new(),
        };
        if let Stage::Stretched { latency, .. } = voice.stage {
            // What the stretcher gives first comes from before the start,
            // so it is played and dropped.
            let mut dropped = Vec::new();
            let target = voice.input_target(0);
            voice.stretch_frames(latency, &mut dropped, target)?;
        }
        Ok(voice)
    }

    /// Input frames the stretcher should have been given once it has
    /// handed out `emitted` frames, so its output stays on the timeline.
    fn input_target(&self, emitted: u64) -> u64 {
        let Stage::Stretched {
            preroll,
            latency,
            rate_ratio,
            ..
        } = self.stage
        else {
            return 0;
        };
        let seconds = emitted as f64 / f64::from(self.sample_rate);
        let source = self.placement.source_offset_at(seconds) * f64::from(self.source_rate);
        let lookahead = latency as f64 * self.placement.rate_at(seconds) * rate_ratio;
        preroll + (source + lookahead).round().max(0.0) as u64
    }

    /// Stretches `frames` output frames into `output`, giving the
    /// stretcher input up to the `target` frame.
    fn stretch_frames(
        &mut self,
        frames: usize,
        output: &mut Vec<f32>,
        target: u64,
    ) -> Result<(), EuphoriaError> {
        let Stage::Stretched { consumed, .. } = self.stage else {
            return Ok(());
        };
        let input_frames = target.saturating_sub(consumed) as usize;
        self.input.clear();
        self.reader.read(&mut self.input, input_frames)?;
        let mut outputs = vec![vec![0.0; frames]; CHANNELS];
        if let Stage::Stretched {
            stretch, consumed, ..
        } = &mut self.stage
        {
            stretch
                .process(&deinterleave(&self.input, CHANNELS), &mut outputs)
                .map_err(|reason| render_error(&self.path, reason))?;
            *consumed += input_frames as u64;
        }
        output.clear();
        output.extend(interleave(&outputs));
        Ok(())
    }

    /// Writes the next `frames` frames of the track, faded and filtered,
    /// into `output`.
    fn render(&mut self, frames: usize, output: &mut Vec<f32>) -> Result<(), EuphoriaError> {
        match self.stage {
            Stage::Direct => {
                output.clear();
                self.reader.read(output, frames)?;
            }
            Stage::Stretched { .. } => {
                let target = self.input_target(self.emitted + frames as u64);
                self.stretch_frames(frames, output, target)?;
            }
        }
        let sample_rate = f64::from(self.sample_rate);
        for (index, frame) in output.as_chunks_mut::<CHANNELS>().0.iter_mut().enumerate() {
            let mix_seconds = (self.start_frame + self.emitted + index as u64) as f64 / sample_rate;
            let (volume, bass) = self.placement.gains_at(mix_seconds);
            for (sample, filter) in frame.iter_mut().zip(&mut self.bass_filters) {
                let low = filter.process(*sample);
                let high = *sample - low;
                *sample = volume * (high + bass * low);
            }
        }
        self.emitted += frames as u64;
        Ok(())
    }
}

/// A two-pole low-pass filter, from the Audio EQ Cookbook.
struct Biquad {
    feedforward: [f64; 3],
    feedback: [f64; 2],
    state: [f64; 2],
}

impl Biquad {
    fn low_pass(hertz: f64, sample_rate: u32) -> Self {
        let angle = std::f64::consts::TAU * hertz / f64::from(sample_rate);
        let alpha = angle.sin() / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let cosine = angle.cos();
        let normaliser = 1.0 + alpha;
        Self {
            feedforward: [
                (1.0 - cosine) / 2.0 / normaliser,
                (1.0 - cosine) / normaliser,
                (1.0 - cosine) / 2.0 / normaliser,
            ],
            feedback: [-2.0 * cosine / normaliser, (1.0 - alpha) / normaliser],
            state: [0.0; 2],
        }
    }

    fn process(&mut self, sample: f32) -> f32 {
        let input = f64::from(sample);
        let output = self.feedforward[0] * input + self.state[0];
        self.state[0] = self.feedforward[1] * input - self.feedback[0] * output + self.state[1];
        self.state[1] = self.feedforward[2] * input - self.feedback[1] * output;
        output as f32
    }
}

/// Keeps the mix under [`MIX_CEILING`]: it delays the sound by a few
/// milliseconds and turns it down smoothly before a peak arrives.
struct Limiter {
    delay: VecDeque<[f32; CHANNELS]>,
    lookahead: usize,
    /// The gains each peak in the delay needs, with their frame number,
    /// smallest at the front.
    needed: VecDeque<(u64, f32)>,
    frame: u64,
    gain: f32,
    attack: f32,
    release: f32,
}

impl Limiter {
    fn new(sample_rate: u32) -> Self {
        let lookahead = (LIMITER_LOOKAHEAD_SECONDS * f64::from(sample_rate)).ceil() as usize;
        let lookahead = lookahead.max(1);
        Self {
            delay: VecDeque::with_capacity(lookahead + 1),
            lookahead,
            needed: VecDeque::new(),
            frame: 0,
            gain: 1.0,
            attack: 1.0 - (-5.0 / lookahead as f32).exp(),
            release: 1.0 - (-1.0 / (LIMITER_RELEASE_SECONDS * f64::from(sample_rate)) as f32).exp(),
        }
    }

    fn process(&mut self, interleaved: &[f32], output: &mut Vec<f32>) {
        for frame in interleaved.as_chunks::<CHANNELS>().0 {
            self.push(*frame, output);
        }
    }

    fn finish(&mut self, output: &mut Vec<f32>) {
        for _ in 0..self.lookahead {
            self.push([0.0; CHANNELS], output);
        }
    }

    fn push(&mut self, frame: [f32; CHANNELS], output: &mut Vec<f32>) {
        let peak = frame[0].abs().max(frame[1].abs());
        let needed = if peak > MIX_CEILING {
            MIX_CEILING / peak
        } else {
            1.0
        };
        while self.needed.back().is_some_and(|(_, gain)| *gain >= needed) {
            self.needed.pop_back();
        }
        self.needed.push_back((self.frame, needed));
        while self
            .needed
            .front()
            .is_some_and(|(frame, _)| frame + (self.lookahead as u64) < self.frame)
        {
            self.needed.pop_front();
        }
        let target = self.needed.front().map_or(1.0, |(_, gain)| *gain);
        let speed = if target < self.gain {
            self.attack
        } else {
            self.release
        };
        self.gain += (target - self.gain) * speed;
        self.delay.push_back(frame);
        self.frame += 1;
        if self.delay.len() > self.lookahead
            && let Some(delayed) = self.delay.pop_front()
        {
            for sample in delayed {
                output.push((sample * self.gain).clamp(-MIX_CEILING, MIX_CEILING));
            }
        }
    }
}

#[cfg(test)]
pub(super) fn limit_for_test(interleaved: &[f32], sample_rate: u32) -> Vec<f32> {
    let mut limiter = Limiter::new(sample_rate);
    let mut output = Vec::new();
    limiter.process(interleaved, &mut output);
    limiter.finish(&mut output);
    output
}
