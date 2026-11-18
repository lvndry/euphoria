// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Making sound: a track is decoded by the core a packet at a time and fed
//! to the system's audio output, so playing starts as soon as the file is
//! open and never waits for the whole track.
//!
//! Decoding happens on a thread of its own, about a second ahead of what is
//! heard. The audio output only ever takes samples that are already decoded,
//! so a slow disk or a busy processor does not make it wait: at worst it
//! plays a moment of silence until the decoder catches up.

use std::collections::VecDeque;
use std::num::{NonZeroU16, NonZeroU32};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI8, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use euphoria_core::shift::PitchShifter;
use euphoria_core::stems::{Stem, StemFiles};
use euphoria_core::{AudioStream, EuphoriaError};

use crate::deck_audio::DeckMix;
use crate::metronome::Metronome;
use crate::reference_sound::ReferenceSound;

mod tempo_stage;

use tempo_stage::TempoStage;
pub(crate) use tempo_stage::{PlaybackTempo, TempoMode};

/// How many decoded chunks wait between the decoder and the audio output. A
/// chunk is one packet of the file, from about 25 milliseconds of an MP3 to
/// about 90 of a FLAC, so this is one to four seconds of sound.
const READ_AHEAD_CHUNKS: usize = 48;

/// How long the decoder sleeps when it has nothing to do: the read ahead is
/// full, or the track has been decoded to its end.
const DECODER_PAUSE: Duration = Duration::from_millis(4);

/// How long the end of a loop crossfades into its start, short enough to
/// keep the beat where it is and long enough that the join never clicks.
const LOOP_CROSSFADE_SECONDS: f64 = 0.005;

/// How long the sound fades out before, and in after, a jump made while a
/// loop is set or changed, so the jump never clicks.
const DECLICK_SECONDS: f64 = 0.003;

/// The shortest loop the player plays, in seconds.
pub(crate) const SHORTEST_LOOP_SECONDS: f64 = 0.05;

/// Most loop passes the decoder joins in one chunk, which only a loop far
/// shorter than a chunk of the file ever needs.
const MOST_WRAPS_PER_CHUNK: usize = 64;

/// Why a track cannot be played, in words short enough for the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlaybackProblem {
    /// The computer has no usable audio output.
    NoOutput,
    /// The file is no longer where it was.
    FileMissing,
    /// The file is there but cannot be opened.
    FileUnreadable,
    /// The file is not audio Euphoria can decode, or is damaged.
    NotDecodable,
}

impl PlaybackProblem {
    /// The message shown in the player.
    pub(crate) fn message(&self) -> &'static str {
        match self {
            PlaybackProblem::NoOutput => "No audio output to play through",
            PlaybackProblem::FileMissing => "This file is no longer there",
            PlaybackProblem::FileUnreadable => "Could not open this file",
            PlaybackProblem::NotDecodable => "Could not play this file",
        }
    }

    fn from_core(error: &EuphoriaError) -> Self {
        match error {
            EuphoriaError::Missing { .. } => PlaybackProblem::FileMissing,
            EuphoriaError::Open { source, .. } | EuphoriaError::Scan { source, .. }
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                PlaybackProblem::FileMissing
            }
            EuphoriaError::Open { .. } | EuphoriaError::Scan { .. } => {
                PlaybackProblem::FileUnreadable
            }
            _ => PlaybackProblem::NotDecodable,
        }
    }
}

/// A seek that has been asked for.
#[derive(Debug, Default)]
struct SeekRequests {
    /// Where the decoder should go next, until it has taken the request.
    for_decoder: Option<f32>,
    /// Where the window should show the position, until sound from there is
    /// playing.
    shown: Option<f32>,
    /// Whether the latest seek fades the sound out and back in, for a jump
    /// that keeps the music going, rather than cutting at once.
    is_soft: bool,
}

/// A loop in frames of the track: from `start` up to, and not including,
/// `end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LoopFrames {
    pub(crate) start: u64,
    pub(crate) end: u64,
}

impl LoopFrames {
    fn length(self) -> u64 {
        self.end - self.start
    }

    /// `frame` brought into the loop, keeping its place in the bar: a frame
    /// before the loop is left alone, one at or past its end is moved back
    /// by whole passes.
    fn folded(self, frame: u64) -> u64 {
        if frame < self.end || self.length() == 0 {
            return frame;
        }
        self.start + (frame - self.start) % self.length()
    }
}

/// The loop the decoder plays and how far it has decoded, behind one lock
/// so the window can change the loop knowing what was decoded under the
/// old one.
#[derive(Debug, Default)]
struct LoopShared {
    region: Option<LoopFrames>,
    /// The frame of the track the decoder reads next.
    decoded_input_frame: u64,
    /// How many times the decoder has jumped back to a loop's start.
    decoded_wraps: u64,
}

/// What the sound of one track is doing, shared between the decoder, the
/// audio thread that plays it and the window that shows it.
#[derive(Debug)]
pub(crate) struct VoiceState {
    sample_rate: NonZeroU32,
    frames_played: AtomicU64,
    /// Counts seeks. Decoded audio carries the count it was decoded under,
    /// and audio from before the latest seek is thrown away unplayed.
    seek_count: AtomicU64,
    seek_requests: Mutex<SeekRequests>,
    /// Frames of silence played because the decoder had nothing ready.
    silent_frames: AtomicU64,
    is_finished: AtomicBool,
    is_abandoned: AtomicBool,
    problem: Mutex<Option<PlaybackProblem>>,
    /// Semitones the sound is shifted by, changed while it plays.
    semitones: AtomicI8,
    /// The speed the sound plays at, as f64 bits, changed while it plays.
    tempo_rate: AtomicU64,
    tempo_key_lock: AtomicBool,
    /// What the tempo stage is doing, as a [`TempoMode`] number.
    tempo_mode: AtomicU8,
    /// The sound is held, so nothing follows its position.
    is_held: AtomicBool,
    looping: Mutex<LoopShared>,
    /// The loop's first and end frames, for the window to read without the
    /// lock, `u64::MAX` without a loop.
    loop_start_frame: AtomicU64,
    loop_end_frame: AtomicU64,
    /// The decoder's count of loop jumps as of the chunk being heard.
    heard_wraps: AtomicU64,
}

impl VoiceState {
    fn new(sample_rate: NonZeroU32) -> Self {
        Self {
            sample_rate,
            frames_played: AtomicU64::new(0),
            seek_count: AtomicU64::new(0),
            seek_requests: Mutex::new(SeekRequests::default()),
            silent_frames: AtomicU64::new(0),
            is_finished: AtomicBool::new(false),
            is_abandoned: AtomicBool::new(false),
            problem: Mutex::new(None),
            semitones: AtomicI8::new(0),
            tempo_rate: AtomicU64::new(1.0_f64.to_bits()),
            tempo_key_lock: AtomicBool::new(true),
            tempo_mode: AtomicU8::new(0),
            is_held: AtomicBool::new(false),
            looping: Mutex::new(LoopShared::default()),
            loop_start_frame: AtomicU64::new(u64::MAX),
            loop_end_frame: AtomicU64::new(u64::MAX),
            heard_wraps: AtomicU64::new(0),
        }
    }

    /// The speed and key lock the sound is asked to play with.
    pub(crate) fn tempo(&self) -> PlaybackTempo {
        PlaybackTempo {
            rate: f64::from_bits(self.tempo_rate.load(Ordering::Relaxed)),
            key_lock: self.tempo_key_lock.load(Ordering::Relaxed),
        }
    }

    /// What the tempo stage is doing now.
    #[cfg(test)]
    pub(crate) fn tempo_mode(&self) -> TempoMode {
        match self.tempo_mode.load(Ordering::Relaxed) {
            1 => TempoMode::Stretching,
            2 => TempoMode::Resampling,
            _ => TempoMode::Bypass,
        }
    }

    fn note_tempo_mode(&self, mode: TempoMode) {
        let number = match mode {
            TempoMode::Bypass => 0,
            TempoMode::Stretching => 1,
            TempoMode::Resampling => 2,
        };
        self.tempo_mode.store(number, Ordering::Relaxed);
    }

    /// Plays at `tempo` from the current playhead on, without restarting:
    /// what was decoded ahead at the old speed is dropped and decoding goes
    /// on from the same place in the track.
    pub(crate) fn set_tempo(&self, tempo: PlaybackTempo) {
        let tempo = tempo.limited();
        if self.tempo() == tempo {
            return;
        }
        self.tempo_rate
            .store(tempo.rate.to_bits(), Ordering::Relaxed);
        self.tempo_key_lock.store(tempo.key_lock, Ordering::Relaxed);
        self.seek_to_playhead();
    }

    /// Marks the sound as held (paused) or running again.
    pub(crate) fn set_held(&self, is_held: bool) {
        self.is_held.store(is_held, Ordering::Relaxed);
    }

    /// Whether the sound is held.
    pub(crate) fn is_held(&self) -> bool {
        self.is_held.load(Ordering::Relaxed)
    }

    /// The playhead in track frames as the audio thread has it, without
    /// any pending seek.
    pub(crate) fn playhead_frames(&self) -> u64 {
        self.frames_played.load(Ordering::Relaxed)
    }

    /// Frames per second of the track.
    pub(crate) fn sample_rate(&self) -> u32 {
        self.sample_rate.get()
    }

    /// A state for a track of `sample_rate`, whose playhead a test moves.
    #[cfg(test)]
    pub(crate) fn for_tests(sample_rate: u32) -> Arc<Self> {
        Arc::new(Self::new(
            NonZeroU32::new(sample_rate).expect("a nonzero sample rate"),
        ))
    }

    /// Puts the playhead at `frames` of the track.
    #[cfg(test)]
    pub(crate) fn set_playhead_frames(&self, frames: u64) {
        self.frames_played.store(frames, Ordering::Relaxed);
    }

    /// How far the sound has got, in seconds from the beginning. Right after
    /// a seek this is where the seek asked for, before sound from there has
    /// started.
    pub(crate) fn position_seconds(&self) -> f32 {
        if let Some(shown) = self.lock_seek_requests().shown {
            return shown;
        }
        self.heard_frame() as f32 / self.sample_rate.get() as f32
    }

    /// The frame heard now, brought back into the loop when the sound
    /// handed out has just crossed its end.
    fn heard_frame(&self) -> u64 {
        let played = self.frames_played.load(Ordering::Relaxed);
        match self.loop_frames() {
            Some(region) => region.folded(played),
            None => played,
        }
    }

    fn loop_frames(&self) -> Option<LoopFrames> {
        let start = self.loop_start_frame.load(Ordering::Relaxed);
        let end = self.loop_end_frame.load(Ordering::Relaxed);
        (start != u64::MAX && end != u64::MAX && end > start).then_some(LoopFrames { start, end })
    }

    /// The loop the sound plays, in seconds from the start of the track.
    #[cfg(test)]
    pub(crate) fn loop_seconds(&self) -> Option<(f64, f64)> {
        let region = self.loop_frames()?;
        let rate = f64::from(self.sample_rate.get());
        Some((region.start as f64 / rate, region.end as f64 / rate))
    }

    /// Loops the sound between `region`, in seconds from the start of the
    /// track, or stops looping with `None`, so it plays on past the end.
    /// With `entry` the sound jumps there; otherwise it carries on from the
    /// playhead. Every change takes effect seamlessly where nothing ahead
    /// was decoded under the old loop; otherwise the sound fades for a few
    /// milliseconds around the playhead and goes on under the new one.
    pub(crate) fn set_loop(&self, region: Option<(f64, f64)>, entry: Option<f64>) {
        let rate = f64::from(self.sample_rate.get());
        let region = region
            .filter(|(start, end)| {
                start.is_finite()
                    && end.is_finite()
                    && *start >= 0.0
                    && end - start >= SHORTEST_LOOP_SECONDS
            })
            .map(|(start, end)| LoopFrames {
                start: (start * rate).round() as u64,
                end: (end * rate).round() as u64,
            });
        let mut looping = self.lock_looping();
        let heard = self.heard_frame();
        let decoded_another_way = looping.decoded_wraps > self.heard_wraps.load(Ordering::Relaxed)
            || region.is_some_and(|region| looping.decoded_input_frame > region.end);
        looping.region = region;
        let (start, end) = region.map_or((u64::MAX, u64::MAX), |region| (region.start, region.end));
        self.loop_start_frame.store(start, Ordering::Relaxed);
        self.loop_end_frame.store(end, Ordering::Relaxed);
        drop(looping);
        let target = match entry {
            Some(seconds) => Some((seconds.max(0.0) * rate).round() as u64),
            None if decoded_another_way => {
                let ahead = heard + (DECLICK_SECONDS * rate).round() as u64;
                Some(region.map_or(ahead, |region| region.folded(ahead)))
            }
            None => None,
        };
        if let Some(frame) = target {
            let mut requests = self.lock_seek_requests();
            let seconds = (frame as f64 / rate) as f32;
            requests.for_decoder = Some(seconds);
            requests.shown = Some(seconds);
            requests.is_soft = true;
            self.seek_count.fetch_add(1, Ordering::Release);
        }
    }

    fn lock_looping(&self) -> MutexGuard<'_, LoopShared> {
        self.looping.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Jumps to `seconds` from the beginning. What was decoded ahead is
    /// dropped at once, and sound resumes from the new place as soon as the
    /// decoder has a first chunk from there.
    pub(crate) fn seek_to(&self, seconds: f32) {
        let seconds = seconds.max(0.0);
        let mut requests = self.lock_seek_requests();
        requests.for_decoder = Some(seconds);
        requests.shown = Some(seconds);
        requests.is_soft = false;
        self.seek_count.fetch_add(1, Ordering::Release);
    }

    /// True once the track has played to its end or stopped on a problem.
    pub(crate) fn is_finished(&self) -> bool {
        self.is_finished.load(Ordering::Relaxed)
    }

    /// Why the sound stopped early, when it did.
    pub(crate) fn problem(&self) -> Option<PlaybackProblem> {
        self.problem
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Frames of silence played while waiting for the decoder.
    pub(crate) fn silent_frames(&self) -> u64 {
        self.silent_frames.load(Ordering::Relaxed)
    }

    /// Changes the key at the current playhead, discarding packets already
    /// decoded in the old key, including while the voice is paused.
    ///
    /// The playhead is read while the seek's own lock is held, so the audio
    /// callback cannot hand out more samples in the old key between reading
    /// it and the seek dropping the decoded-ahead audio: those frames would
    /// otherwise be played twice in the new key.
    pub(crate) fn set_shift(&self, semitones: i8) {
        if self.semitones.swap(semitones, Ordering::Relaxed) != semitones {
            self.seek_to_playhead();
        }
    }

    fn seek_to_playhead(&self) {
        let mut requests = self.lock_seek_requests();
        let seconds = (self.frames_played.load(Ordering::Relaxed) as f32
            / self.sample_rate.get() as f32)
            .max(0.0);
        requests.for_decoder = Some(seconds);
        requests.shown = Some(seconds);
        requests.is_soft = false;
        self.seek_count.fetch_add(1, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn shift_is(&self, semitones: i8) -> bool {
        self.semitones.load(Ordering::Relaxed) == semitones
    }

    fn frames_at(&self, seconds: f32) -> u64 {
        (f64::from(seconds.max(0.0)) * f64::from(self.sample_rate.get())).round() as u64
    }

    fn stop_on(&self, problem: PlaybackProblem) {
        *self.problem.lock().unwrap_or_else(PoisonError::into_inner) = Some(problem);
        self.is_finished.store(true, Ordering::Relaxed);
    }

    fn lock_seek_requests(&self) -> MutexGuard<'_, SeekRequests> {
        self.seek_requests
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// How long a stem takes to fade in or out when it is muted or brought
/// back, short enough to feel instant and long enough not to click.
const STEM_FADE_SECONDS: f32 = 0.01;

/// Which stems are heard and how loud, shared between the window, which
/// changes it, and the audio thread, which follows it within a few
/// milliseconds.
#[derive(Debug)]
pub(crate) struct StemMix {
    audible: AtomicU8,
    /// The level of each stem from 0 to 1, as f32 bits, in the order of
    /// [`Stem::ALL`].
    levels: [AtomicU32; 4],
}

impl Default for StemMix {
    fn default() -> Self {
        Self {
            audible: AtomicU8::new(StemMix::ALL_AUDIBLE),
            levels: std::array::from_fn(|_| AtomicU32::new(1.0_f32.to_bits())),
        }
    }
}

impl StemMix {
    const ALL_AUDIBLE: u8 = (1 << Stem::ALL.len()) - 1;

    /// Makes exactly the stems for which `audible` holds heard.
    pub(crate) fn set_audible(&self, audible: impl Fn(Stem) -> bool) {
        let mask = Stem::ALL
            .iter()
            .enumerate()
            .filter(|(_, stem)| audible(**stem))
            .fold(0u8, |mask, (index, _)| mask | 1 << index);
        self.audible.store(mask, Ordering::Relaxed);
    }

    /// Sets how loud `stem` plays when heard, from 0 to 1.
    pub(crate) fn set_level(&self, stem: Stem, level: f32) {
        if let Some(index) = Stem::ALL.iter().position(|each| *each == stem) {
            self.levels[index].store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        }
    }

    pub(crate) fn gain(&self, layer: usize) -> f32 {
        if self.audible.load(Ordering::Relaxed) & (1 << layer) == 0 {
            return 0.0;
        }
        self.levels
            .get(layer)
            .map_or(0.0, |level| f32::from_bits(level.load(Ordering::Relaxed)))
    }
}

/// Where decoded audio comes from: one file, or the four stems of a track
/// read side by side so that the audio thread can mix them.
enum Reader {
    File(AudioStream),
    Stems(StemReader),
}

impl Reader {
    fn sample_rate(&self) -> NonZeroU32 {
        match self {
            Reader::File(stream) => stream.sample_rate(),
            Reader::Stems(stems) => stems.streams[0].sample_rate(),
        }
    }

    fn channel_count(&self) -> NonZeroU16 {
        match self {
            Reader::File(stream) => stream.channel_count(),
            Reader::Stems(stems) => stems.streams[0].channel_count(),
        }
    }

    /// Sounds laid side by side in every frame: one for a file, one per
    /// stem for stems.
    fn layer_count(&self) -> usize {
        match self {
            Reader::File(_) => 1,
            Reader::Stems(stems) => stems.streams.len(),
        }
    }

    fn duration_seconds(&self) -> Option<f32> {
        match self {
            Reader::File(stream) => stream.duration_seconds(),
            Reader::Stems(stems) => stems.streams[0].duration_seconds(),
        }
    }

    fn read_chunk(&mut self, samples: &mut Vec<f32>) -> Result<bool, EuphoriaError> {
        match self {
            Reader::File(stream) => stream.read_chunk(samples),
            Reader::Stems(stems) => stems.read_chunk(samples),
        }
    }

    fn seek(&mut self, seconds: f32) -> Result<f32, EuphoriaError> {
        match self {
            Reader::File(stream) => stream.seek(seconds),
            Reader::Stems(stems) => stems.seek(seconds),
        }
    }

    fn seek_frame(&mut self, frame: u64) -> Result<u64, EuphoriaError> {
        match self {
            Reader::File(stream) => stream.seek_frame(frame),
            Reader::Stems(stems) => stems.seek_frame(frame),
        }
    }
}

/// The four stem files of a track, decoded in step. Each chunk holds, for
/// every frame, the frame of every stem in turn.
struct StemReader {
    streams: Vec<AudioStream>,
    /// Samples decoded from each stream and not handed out yet.
    pending: Vec<VecDeque<f32>>,
    is_at_end: Vec<bool>,
    chunk: Vec<f32>,
}

impl StemReader {
    fn open(files: &StemFiles) -> Result<Self, EuphoriaError> {
        let streams = Stem::ALL
            .iter()
            .map(|stem| AudioStream::open(&files.path(*stem)))
            .collect::<Result<Vec<_>, _>>()?;
        let count = streams.len();
        Ok(Self {
            streams,
            pending: vec![VecDeque::new(); count],
            is_at_end: vec![false; count],
            chunk: Vec::new(),
        })
    }

    fn read_chunk(&mut self, samples: &mut Vec<f32>) -> Result<bool, EuphoriaError> {
        samples.clear();
        let channel_count = usize::from(self.streams[0].channel_count().get());
        for index in 0..self.streams.len() {
            while self.pending[index].is_empty() && !self.is_at_end[index] {
                if self.streams[index].read_chunk(&mut self.chunk)? {
                    self.pending[index].extend(self.chunk.drain(..));
                } else {
                    self.is_at_end[index] = true;
                }
            }
        }
        let pending_frames = |pending: &VecDeque<f32>| pending.len() / channel_count;
        let still_decoding = self
            .pending
            .iter()
            .zip(&self.is_at_end)
            .filter(|(_, is_at_end)| !**is_at_end)
            .map(|(pending, _)| pending_frames(pending))
            .min();
        // Once every stem has ended, what is left of the longest is handed
        // out, with silence for the others.
        let frames = still_decoding
            .unwrap_or_else(|| self.pending.iter().map(pending_frames).max().unwrap_or(0));
        if frames == 0 {
            return Ok(false);
        }
        samples.reserve(frames * channel_count * self.streams.len());
        for _ in 0..frames {
            for pending in &mut self.pending {
                for _ in 0..channel_count {
                    samples.push(pending.pop_front().unwrap_or(0.0));
                }
            }
        }
        Ok(true)
    }

    fn seek(&mut self, seconds: f32) -> Result<f32, EuphoriaError> {
        let mut reached = seconds;
        for (index, stream) in self.streams.iter_mut().enumerate() {
            let landed = stream.seek(seconds)?;
            if index == 0 {
                reached = landed;
            }
        }
        self.pending.iter_mut().for_each(VecDeque::clear);
        self.is_at_end.fill(false);
        Ok(reached)
    }

    fn seek_frame(&mut self, frame: u64) -> Result<u64, EuphoriaError> {
        let mut reached = frame;
        for (index, stream) in self.streams.iter_mut().enumerate() {
            let landed = stream.seek_frame(frame)?;
            if index == 0 {
                reached = landed;
            }
        }
        self.pending.iter_mut().for_each(VecDeque::clear);
        self.is_at_end.fill(false);
        Ok(reached)
    }
}

/// What the decoder hands to the audio thread.
enum Decoded {
    /// A stretch of interleaved samples.
    Chunk {
        /// The seek count the chunk was decoded under.
        seek_count: u64,
        /// The frame of the track the chunk starts at.
        start_frame: u64,
        /// Frames of the track played per frame of the chunk.
        rate: f64,
        samples: Vec<f32>,
        /// How many times the decoder had jumped back to a loop's start by
        /// the end of the chunk.
        wraps: u64,
    },
    /// The track has no more audio after this point.
    End { seek_count: u64, end_frame: u64 },
    /// Decoding failed and stops here.
    Problem(PlaybackProblem),
}

/// Decodes a track ahead of the sound, on its own thread, until the track
/// source that started it is dropped.
struct Decoder {
    stream: Reader,
    state: Arc<VoiceState>,
    decoded: SyncSender<Decoded>,
    seek_count: u64,
    /// The frame of the track the next chunk starts at, which moves by the
    /// rate for every frame handed out.
    next_frame: f64,
    is_at_end: bool,
    shift: Shift,
    /// The frame of the track the stream reads next.
    input_frame: u64,
    /// Loops whose end the decoded sound will cross, after which the
    /// position the chunks carry moves back to the loop's start.
    folds: VecDeque<LoopFrames>,
    wraps: u64,
}

/// The pitch shifter of a playing track, rebuilt when the shift changes or
/// the track seeks, since what it holds belongs to the audio before. Stems
/// get a shifter each, so every stem is shifted exactly as it would be
/// alone and none colours the analysis of another.
struct Shift {
    semitones: i8,
    shifters: Vec<PitchShifter>,
    channels: usize,
    layer_count: usize,
    sample_rate: u32,
    tempo: TempoStage,
}

impl Shift {
    fn new(
        channels: usize,
        layer_count: usize,
        sample_rate: u32,
        semitones: i8,
        tempo: PlaybackTempo,
    ) -> Result<Self, PlaybackProblem> {
        let mut shift = Self {
            semitones,
            shifters: Vec::new(),
            channels,
            layer_count,
            sample_rate,
            tempo: TempoStage::new(channels, layer_count, sample_rate, tempo)?,
        };
        shift.rebuild()?;
        Ok(shift)
    }

    fn rebuild(&mut self) -> Result<(), PlaybackProblem> {
        self.shifters = if self.semitones != 0 {
            (0..self.layer_count)
                .map(|_| {
                    PitchShifter::new(self.channels, self.sample_rate, self.semitones)
                        .map_err(|_| PlaybackProblem::NotDecodable)
                })
                .collect::<Result<_, _>>()?
        } else {
            Vec::new()
        };
        self.tempo.rebuild()
    }

    /// `samples` shifted by `semitones`, which may have changed since the
    /// last chunk. A DSP failure stops playback rather than substituting
    /// audio in the original key or losing held samples.
    fn apply(
        &mut self,
        samples: Vec<f32>,
        semitones: i8,
        tempo: PlaybackTempo,
    ) -> Result<Vec<f32>, PlaybackProblem> {
        if semitones != self.semitones {
            self.semitones = semitones;
            self.rebuild()?;
        }
        let shifted = self.shifted(samples)?;
        self.tempo.apply(shifted, tempo)
    }

    fn shifted(&mut self, samples: Vec<f32>) -> Result<Vec<f32>, PlaybackProblem> {
        if self.shifters.is_empty() {
            return Ok(samples);
        }
        let layers = if self.layer_count == 1 {
            vec![samples]
        } else {
            split_layers(&samples, self.channels, self.layer_count)
        };
        let shifted = self
            .shifters
            .iter_mut()
            .zip(&layers)
            .map(|(shifter, layer)| shifter.process(layer))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| PlaybackProblem::NotDecodable)?;
        join_layers(shifted, self.channels)
    }

    /// Drains the held audio before EOF. Unlike live output, the streaming
    /// shifter removes its startup latency and restores exactly the input's
    /// duration once this tail has been handed to the output.
    fn finish(&mut self) -> Result<Vec<f32>, PlaybackProblem> {
        let tails = std::mem::take(&mut self.shifters)
            .into_iter()
            .map(PitchShifter::finish)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| PlaybackProblem::NotDecodable)?;
        let tail = join_layers(tails, self.channels)?;
        let mut tail = self.tempo.apply(tail, self.tempo.tempo())?;
        tail.extend(self.tempo.finish()?);
        Ok(tail)
    }
}

/// Fades the first `frames` frames of `incoming` in while `outgoing` fades
/// out under them, so that their sum keeps the level of either.
fn crossfade_into(incoming: &mut [f32], outgoing: &[f32], samples_per_frame: usize, frames: usize) {
    for frame in 0..frames.min(incoming.len() / samples_per_frame) {
        let progress = (frame as f32 + 0.5) / frames as f32;
        let fade_in = (progress * std::f32::consts::FRAC_PI_2).sin().powi(2);
        let first = frame * samples_per_frame;
        let incoming_frame = &mut incoming[first..first + samples_per_frame];
        for (offset, sample) in incoming_frame.iter_mut().enumerate() {
            let leaving = outgoing.get(first + offset).copied().unwrap_or(0.0);
            *sample = *sample * fade_in + leaving * (1.0 - fade_in);
        }
    }
}

/// The samples of each layer of `samples`, in which every frame holds
/// `channels` samples of each of `layer_count` layers in turn.
fn split_layers(samples: &[f32], channels: usize, layer_count: usize) -> Vec<Vec<f32>> {
    let mut layers = vec![Vec::with_capacity(samples.len() / layer_count); layer_count];
    for frame in samples.chunks_exact(channels * layer_count) {
        for (layer, frame_of_layer) in layers.iter_mut().zip(frame.chunks_exact(channels)) {
            layer.extend_from_slice(frame_of_layer);
        }
    }
    layers
}

/// The layers laid back side by side in every frame, as
/// [`split_layers`] found them.
fn join_layers(mut layers: Vec<Vec<f32>>, channels: usize) -> Result<Vec<f32>, PlaybackProblem> {
    if layers.len() <= 1 {
        return Ok(layers.pop().unwrap_or_default());
    }
    let first = &layers[0];
    if layers.iter().any(|layer| layer.len() != first.len()) {
        return Err(PlaybackProblem::NotDecodable);
    }
    let mut joined = Vec::with_capacity(first.len() * layers.len());
    for frame in 0..first.len() / channels {
        for layer in &layers {
            joined.extend_from_slice(&layer[frame * channels..(frame + 1) * channels]);
        }
    }
    Ok(joined)
}

impl Decoder {
    fn run(mut self) {
        while !self.state.is_abandoned.load(Ordering::Relaxed) {
            if !self.follow_seeks() {
                return;
            }
            if self.is_at_end {
                std::thread::sleep(DECODER_PAUSE);
                continue;
            }
            let message = self.decode_next();
            let stops_here = matches!(message, Decoded::Problem(_));
            if !self.hand_over(message) || stops_here {
                return;
            }
        }
    }

    /// Moves the stream to where the latest seek asked for. Returns false
    /// when the seek failed, which ends the sound.
    fn follow_seeks(&mut self) -> bool {
        let latest = self.state.seek_count.load(Ordering::Acquire);
        if latest == self.seek_count {
            return true;
        }
        self.seek_count = latest;
        let request = self.state.lock_seek_requests().for_decoder.take();
        let Some(seconds) = request else {
            return true;
        };
        match self.stream.seek_frame(self.state.frames_at(seconds)) {
            Ok(reached) => {
                self.next_frame = reached as f64;
                self.input_frame = reached;
                self.folds.clear();
                self.state.lock_looping().decoded_input_frame = reached;
                self.is_at_end = false;
                if let Err(problem) = self.shift.rebuild() {
                    self.state.stop_on(problem);
                    return false;
                }
                true
            }
            Err(error) => {
                let _ = self
                    .decoded
                    .try_send(Decoded::Problem(PlaybackProblem::from_core(&error)));
                self.state.stop_on(PlaybackProblem::from_core(&error));
                false
            }
        }
    }

    fn decode_next(&mut self) -> Decoded {
        loop {
            let samples = match self.next_input() {
                Ok(Some(samples)) => match self.shift.apply(
                    samples,
                    self.state.semitones.load(Ordering::Relaxed),
                    self.state.tempo(),
                ) {
                    Ok(samples) => samples,
                    Err(problem) => return Decoded::Problem(problem),
                },
                Ok(None) => {
                    let tail = match self.shift.finish() {
                        Ok(tail) => tail,
                        Err(problem) => return Decoded::Problem(problem),
                    };
                    if tail.is_empty() {
                        self.is_at_end = true;
                        return Decoded::End {
                            seek_count: self.seek_count,
                            end_frame: self.next_frame.round() as u64,
                        };
                    }
                    tail
                }
                Err(error) => return Decoded::Problem(PlaybackProblem::from_core(&error)),
            };
            // Initial packets can be held until the shifter has its preroll.
            // Never hand an empty chunk to the sample iterator.
            if samples.is_empty() {
                continue;
            }
            let start_frame = self.next_frame.round() as u64;
            let samples_per_frame =
                usize::from(self.stream.channel_count().get()) * self.stream.layer_count();
            let rate = self.shift.tempo.rate();
            self.state.note_tempo_mode(self.shift.tempo.mode());
            self.next_frame += (samples.len() / samples_per_frame) as f64 * rate;
            while let Some(fold) = self.folds.front().copied() {
                if self.next_frame < fold.end as f64 {
                    break;
                }
                self.next_frame -= fold.length() as f64;
                self.folds.pop_front();
            }
            return Decoded::Chunk {
                seek_count: self.seek_count,
                start_frame,
                rate,
                samples,
                wraps: self.wraps,
            };
        }
    }

    fn samples_per_frame(&self) -> usize {
        usize::from(self.stream.channel_count().get()) * self.stream.layer_count()
    }

    /// Reads the next chunk of the track as the sound should play it: where
    /// the chunk crosses the end of the loop, what follows the end is
    /// crossfaded into the loop's start, read from there exactly. `None` at
    /// the end of the track, outside a loop.
    fn next_input(&mut self) -> Result<Option<Vec<f32>>, EuphoriaError> {
        let samples_per_frame = self.samples_per_frame();
        let crossfade_frames =
            (LOOP_CROSSFADE_SECONDS * f64::from(self.state.sample_rate())).round() as usize;
        let mut looping = self.state.lock_looping();
        let mut segment = Vec::new();
        let mut has_more = self.stream.read_chunk(&mut segment)?;
        let mut segment_start = self.input_frame;
        let mut output = Vec::new();
        for _ in 0..MOST_WRAPS_PER_CHUNK {
            let segment_end = segment_start + (segment.len() / samples_per_frame) as u64;
            let crosses_end = looping.region.filter(|region| {
                segment_start <= region.end
                    && segment_end > region.start
                    && (segment_end > region.end || !has_more)
            });
            let Some(region) = crosses_end else {
                self.input_frame = segment_end;
                output.append(&mut segment);
                break;
            };
            let kept_frames = (region.end.min(segment_end) - segment_start) as usize;
            let mut outgoing = segment.split_off(kept_frames * samples_per_frame);
            output.append(&mut segment);
            let mut more = Vec::new();
            while has_more && outgoing.len() < crossfade_frames * samples_per_frame {
                has_more = self.stream.read_chunk(&mut more)?;
                outgoing.append(&mut more);
            }
            outgoing.truncate(crossfade_frames * samples_per_frame);
            let reached = self.stream.seek_frame(region.start)?;
            let mut head = Vec::new();
            has_more = true;
            while has_more && head.len() < crossfade_frames * samples_per_frame {
                has_more = self.stream.read_chunk(&mut more)?;
                head.append(&mut more);
            }
            if head.is_empty() {
                self.input_frame = reached;
                break;
            }
            crossfade_into(&mut head, &outgoing, samples_per_frame, crossfade_frames);
            self.wraps += 1;
            self.folds.push_back(region);
            segment = head;
            segment_start = reached;
        }
        looping.decoded_input_frame = self.input_frame;
        looping.decoded_wraps = self.wraps;
        if output.is_empty() && !has_more {
            return Ok(None);
        }
        Ok(Some(output))
    }

    /// Waits for room in the read ahead and puts `message` there. Gives the
    /// message up when a seek makes it stale. Returns false when the sound
    /// is over and decoding should stop.
    fn hand_over(&mut self, mut message: Decoded) -> bool {
        loop {
            match self.decoded.try_send(message) {
                Ok(()) => return true,
                Err(TrySendError::Disconnected(_)) => return false,
                Err(TrySendError::Full(unsent)) => {
                    if self.state.is_abandoned.load(Ordering::Relaxed) {
                        return false;
                    }
                    let is_stale = !matches!(unsent, Decoded::Problem(_))
                        && self.state.seek_count.load(Ordering::Acquire) != self.seek_count;
                    if is_stale {
                        return true;
                    }
                    message = unsent;
                    std::thread::sleep(DECODER_PAUSE);
                }
            }
        }
    }
}

/// What the audio thread found when it looked for the next chunk.
enum NextChunk {
    Ready,
    NotDecodedYet,
    NothingMore,
}

/// The samples of one track, handed to the audio output as it asks for them.
/// It never decodes and never waits: it plays what the decoder has ready.
pub(crate) struct TrackSource {
    state: Arc<VoiceState>,
    decoded: Receiver<Decoded>,
    sample_rate: NonZeroU32,
    channel_count: NonZeroU16,
    duration_seconds: Option<f32>,
    chunk: Vec<f32>,
    /// The seek count the current chunk was decoded under.
    chunk_seek_count: u64,
    next_sample: usize,
    /// Samples of silence still owed to complete a silent frame.
    silence_owed: usize,
    /// Sounds laid side by side in every frame of a chunk: one for a file,
    /// four for stems, which are mixed as they are handed out.
    layer_count: usize,
    /// Which stems are heard, for stems.
    mix: Option<Arc<StemMix>>,
    /// The gain each layer is played at, moving towards what the mix asks
    /// for by `fade_step` a frame.
    gains: Vec<f32>,
    fade_step: f32,
    /// The frame the current chunk starts at.
    chunk_start_frame: u64,
    /// Frames of the track played per frame of the current chunk.
    chunk_rate: f64,
    /// Whether to wait for the decoder rather than play silence, for tests
    /// that pull samples far faster than real time.
    is_patient: bool,
    declick: Declick,
    /// Frames a declick fades over.
    declick_frames: usize,
    /// The gain of the frame being handed out.
    frame_gain: f32,
}

/// Where the sound is in fading around a soft seek.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Declick {
    Steady,
    /// The sound from before the seek fades out over these last frames.
    FadingOut {
        frames_left: usize,
    },
    /// The sound from after the seek has faded in this far.
    FadingIn {
        frames_done: usize,
    },
}

fn declick_frames(sample_rate: NonZeroU32) -> usize {
    ((DECLICK_SECONDS * f64::from(sample_rate.get())).round() as usize).max(1)
}

impl TrackSource {
    /// Opens the file at `path` positioned `from_seconds` in, unshifted.
    #[cfg(test)]
    pub(crate) fn open(path: &Path, from_seconds: f32) -> Result<Self, PlaybackProblem> {
        Self::open_shifted(path, from_seconds, 0)
    }

    /// Opens the file at `path` positioned `from_seconds` in, with its key
    /// shifted by `semitones`, decodes its first chunk and starts the
    /// decoder thread. [`VoiceState::set_shift`] changes the shift while it
    /// plays. This reads the start of the file, so call it off the main
    /// thread.
    pub(crate) fn open_shifted(
        path: &Path,
        from_seconds: f32,
        semitones: i8,
    ) -> Result<Self, PlaybackProblem> {
        let stream = AudioStream::open(path).map_err(|error| PlaybackProblem::from_core(&error))?;
        Self::start(Reader::File(stream), None, semitones, from_seconds)
    }

    /// Opens the four stems of a track, unshifted.
    #[cfg(test)]
    pub(crate) fn open_stems(
        files: &StemFiles,
        mix: Arc<StemMix>,
        from_seconds: f32,
    ) -> Result<Self, PlaybackProblem> {
        Self::open_stems_shifted(files, mix, from_seconds, 0)
    }

    /// Opens the four stems of a track, to be mixed as `mix` says while
    /// they play, with their key shifted by `semitones`. Like
    /// [`TrackSource::open_shifted`], call it off the main thread.
    pub(crate) fn open_stems_shifted(
        files: &StemFiles,
        mix: Arc<StemMix>,
        from_seconds: f32,
        semitones: i8,
    ) -> Result<Self, PlaybackProblem> {
        let reader = StemReader::open(files).map_err(|error| PlaybackProblem::from_core(&error))?;
        Self::start(Reader::Stems(reader), Some(mix), semitones, from_seconds)
    }

    fn start(
        mut stream: Reader,
        mix: Option<Arc<StemMix>>,
        semitones: i8,
        from_seconds: f32,
    ) -> Result<Self, PlaybackProblem> {
        let problem = |error: EuphoriaError| PlaybackProblem::from_core(&error);
        let reached_seconds = if from_seconds > 0.0 {
            stream.seek(from_seconds).map_err(problem)?
        } else {
            0.0
        };
        let state = Arc::new(VoiceState::new(stream.sample_rate()));
        state.semitones.store(semitones, Ordering::Relaxed);
        let channel_count = stream.channel_count();
        let layer_count = stream.layer_count();
        let mut shift = Shift::new(
            usize::from(channel_count.get()),
            layer_count,
            stream.sample_rate().get(),
            semitones,
            PlaybackTempo::default(),
        )?;
        let start_frame = state.frames_at(reached_seconds);
        state.frames_played.store(start_frame, Ordering::Relaxed);

        // Decode enough preroll here for the first shifted sound to be
        // ready before the audio callback asks. This runs off the UI thread.
        let mut first_chunk;
        let samples_per_input_frame = usize::from(channel_count.get()) * layer_count;
        let mut input_frame = start_frame;
        let has_audio = loop {
            let mut input = Vec::new();
            let has_audio = stream.read_chunk(&mut input).map_err(problem)?;
            input_frame += (input.len() / samples_per_input_frame) as u64;
            first_chunk = if has_audio {
                shift.apply(input, semitones, PlaybackTempo::default())?
            } else {
                shift.finish()?
            };
            if !first_chunk.is_empty() || !has_audio {
                break !first_chunk.is_empty();
            }
        };
        if !has_audio {
            state.is_finished.store(true, Ordering::Relaxed);
        }
        let frames_in_first_chunk =
            (first_chunk.len() / (usize::from(channel_count.get()) * layer_count)) as u64;
        let gains = (0..layer_count)
            .map(|layer| mix.as_ref().map_or(1.0, |mix| mix.gain(layer)))
            .collect();

        let (decoded_sender, decoded) = sync_channel(READ_AHEAD_CHUNKS);
        let source = Self {
            state: Arc::clone(&state),
            decoded,
            sample_rate: stream.sample_rate(),
            channel_count,
            duration_seconds: stream.duration_seconds(),
            chunk: first_chunk,
            chunk_seek_count: 0,
            next_sample: 0,
            silence_owed: 0,
            layer_count,
            mix,
            gains,
            fade_step: 1.0 / (STEM_FADE_SECONDS * stream.sample_rate().get() as f32),
            chunk_start_frame: start_frame,
            chunk_rate: 1.0,
            is_patient: false,
            declick: Declick::Steady,
            declick_frames: declick_frames(stream.sample_rate()),
            frame_gain: 1.0,
        };
        state.lock_looping().decoded_input_frame = input_frame;
        let decoder = Decoder {
            stream,
            state,
            decoded: decoded_sender,
            seek_count: 0,
            next_frame: (start_frame + frames_in_first_chunk) as f64,
            is_at_end: !has_audio,
            shift,
            input_frame,
            folds: VecDeque::new(),
            wraps: 0,
        };
        std::thread::Builder::new()
            .name("euphoria-playback-decoder".to_string())
            .spawn(move || decoder.run())
            .map_err(|_| PlaybackProblem::NotDecodable)?;
        Ok(source)
    }

    /// The state the window reads while this source plays.
    pub(crate) fn state(&self) -> Arc<VoiceState> {
        Arc::clone(&self.state)
    }

    /// The length the file declares, in seconds, when it declares one.
    pub(crate) fn duration_seconds(&self) -> Option<f32> {
        self.duration_seconds
    }

    /// The frame the next sample belongs to, exactly: the position of the
    /// sound handed out so far, which the state only gives a chunk at a
    /// time. Right after a seek it is where the seek asked for.
    pub(crate) fn next_frame(&self) -> u64 {
        let latest_seek_count = self.state.seek_count.load(Ordering::Acquire);
        if self.chunk_seek_count != latest_seek_count {
            return self.state.frames_at(self.state.position_seconds());
        }
        let channel_count = usize::from(self.channel_count.get());
        self.chunk_start_frame
            + self.track_frames((self.next_sample.min(self.chunk_samples()) / channel_count) as u64)
    }

    /// Frames of the track that `output_frames` of the current chunk cover.
    fn track_frames(&self, output_frames: u64) -> u64 {
        (output_frames as f64 * self.chunk_rate).round() as u64
    }

    /// The source, waiting for the decoder whenever it is behind rather
    /// than playing silence, for tests that pull samples as fast as they
    /// can.
    #[cfg(test)]
    pub(crate) fn patient(mut self) -> Self {
        self.is_patient = true;
        self
    }

    /// Takes the next chunk the decoder has ready, passing over anything
    /// decoded before the latest seek.
    fn take_next_chunk(&mut self, latest_seek_count: u64) -> NextChunk {
        loop {
            match self.decoded.try_recv() {
                Ok(Decoded::Chunk { seek_count, .. }) | Ok(Decoded::End { seek_count, .. })
                    if seek_count != latest_seek_count => {}
                Ok(Decoded::Chunk {
                    seek_count,
                    start_frame,
                    rate,
                    samples,
                    wraps,
                }) => {
                    self.state.heard_wraps.store(wraps, Ordering::Relaxed);
                    self.chunk_rate = rate;
                    self.chunk = samples;
                    self.chunk_seek_count = seek_count;
                    self.next_sample = 0;
                    self.chunk_start_frame = start_frame;
                    self.state
                        .frames_played
                        .store(start_frame, Ordering::Relaxed);
                    self.state.lock_seek_requests().shown = None;
                    return NextChunk::Ready;
                }
                Ok(Decoded::End { end_frame, .. }) => {
                    self.state.frames_played.store(end_frame, Ordering::Relaxed);
                    self.state.lock_seek_requests().shown = None;
                    self.state.is_finished.store(true, Ordering::Relaxed);
                    return NextChunk::NothingMore;
                }
                Ok(Decoded::Problem(problem)) => {
                    self.state.stop_on(problem);
                    return NextChunk::NothingMore;
                }
                Err(TryRecvError::Empty) if self.is_patient => {
                    std::thread::sleep(Duration::from_micros(200));
                }
                Err(TryRecvError::Empty) => return NextChunk::NotDecodedYet,
                Err(TryRecvError::Disconnected) => {
                    self.state.is_finished.store(true, Ordering::Relaxed);
                    return NextChunk::NothingMore;
                }
            }
        }
    }
}

impl Drop for TrackSource {
    fn drop(&mut self) {
        self.state.is_abandoned.store(true, Ordering::Relaxed);
    }
}

impl TrackSource {
    /// Output samples in the current chunk: its frames times the channels.
    fn chunk_samples(&self) -> usize {
        self.chunk.len() / self.layer_count
    }

    /// The output sample at `next_sample`: the sample itself for a file,
    /// the stems mixed at their gains for stems.
    fn mixed_sample(&mut self) -> f32 {
        if self.layer_count == 1 {
            return self.chunk[self.next_sample];
        }
        let channel_count = usize::from(self.channel_count.get());
        let frame = self.next_sample / channel_count;
        let channel = self.next_sample % channel_count;
        if channel == 0 {
            self.fade_towards_mixed_levels();
        }
        let frame_start = frame * channel_count * self.layer_count;
        self.gains
            .iter()
            .enumerate()
            .map(|(layer, gain)| gain * self.chunk[frame_start + layer * channel_count + channel])
            .sum()
    }

    /// Brings each layer's level a step closer to the level the mix asks
    /// for, at the start of each frame.
    fn fade_towards_mixed_levels(&mut self) {
        let mix = match self.mix.as_ref() {
            Some(mix) => mix,
            None => return,
        };
        for (layer, gain) in self.gains.iter_mut().enumerate() {
            let target = mix.gain(layer);
            *gain += (target - *gain).clamp(-self.fade_step, self.fade_step);
        }
    }
}

impl TrackSource {
    /// The gain of the next frame, a step further through any declick.
    fn next_declick_gain(&mut self) -> f32 {
        let length = self.declick_frames as f32;
        match &mut self.declick {
            Declick::Steady => 1.0,
            Declick::FadingOut { frames_left } => {
                let gain = *frames_left as f32 / length;
                *frames_left = frames_left.saturating_sub(1);
                gain
            }
            Declick::FadingIn { frames_done } => {
                *frames_done += 1;
                let gain = *frames_done as f32 / length;
                if *frames_done >= self.declick_frames {
                    self.declick = Declick::Steady;
                }
                gain
            }
        }
    }
}

impl Iterator for TrackSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.silence_owed > 0 {
            self.silence_owed -= 1;
            return Some(0.0);
        }
        let latest_seek_count = self.state.seek_count.load(Ordering::Acquire);
        let channel_count = usize::from(self.channel_count.get());
        let seek_is_waiting = self.chunk_seek_count != latest_seek_count;
        if seek_is_waiting
            && self.declick == Declick::Steady
            && self.next_sample.is_multiple_of(channel_count)
            && self.state.lock_seek_requests().is_soft
        {
            self.declick = if self.next_sample < self.chunk_samples() {
                Declick::FadingOut {
                    frames_left: self.declick_frames,
                }
            } else {
                Declick::FadingIn { frames_done: 0 }
            };
        }
        let is_fading_out =
            matches!(self.declick, Declick::FadingOut { frames_left } if frames_left > 0);
        let needs_chunk =
            self.next_sample >= self.chunk_samples() || (seek_is_waiting && !is_fading_out);
        if needs_chunk {
            if matches!(self.declick, Declick::FadingOut { .. }) {
                self.declick = Declick::FadingIn { frames_done: 0 };
            }
            if self.state.is_finished() {
                return None;
            }
            match self.take_next_chunk(latest_seek_count) {
                NextChunk::Ready => {}
                NextChunk::NothingMore => return None,
                NextChunk::NotDecodedYet => {
                    // One whole frame of silence, so the channels stay in
                    // step, then look again.
                    if self.chunk_seek_count == latest_seek_count {
                        self.chunk_start_frame += self.track_frames(
                            (self.chunk_samples() / usize::from(self.channel_count.get())) as u64,
                        );
                    }
                    self.chunk.clear();
                    self.next_sample = 0;
                    self.silence_owed = usize::from(self.channel_count.get()) - 1;
                    self.state.silent_frames.fetch_add(1, Ordering::Relaxed);
                    return Some(0.0);
                }
            }
        }
        if self.next_sample.is_multiple_of(channel_count) {
            self.frame_gain = self.next_declick_gain();
        }
        let sample = self.mixed_sample() * self.frame_gain;
        self.next_sample += 1;
        if self
            .next_sample
            .is_multiple_of(usize::from(self.channel_count.get()))
        {
            // A change of shift reads this counter while the same lock is
            // held, so the frames handed out and the seek it asks for stay
            // in one piece. The lock is taken only at a frame boundary
            // (channel_count samples in), not per individual sample.
            let _counting_the_frame = self.state.lock_seek_requests();
            let channel_count = usize::from(self.channel_count.get());
            let played = self.chunk_start_frame
                + self.track_frames((self.next_sample / channel_count) as u64);
            self.state.frames_played.store(played, Ordering::Relaxed);
        }
        Some(sample)
    }
}

impl rodio::Source for TrackSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> NonZeroU16 {
        self.channel_count
    }

    fn sample_rate(&self) -> NonZeroU32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.duration_seconds.map(Duration::from_secs_f32)
    }
}

/// What the player or a deck asks the output to open: a track's file or
/// its stems, where to start, and the key shift.
pub(crate) struct OpenRequest {
    pub(crate) path: PathBuf,
    pub(crate) from_seconds: f32,
    pub(crate) semitones: i8,
    /// The stems to play in place of the file, and how they are mixed.
    pub(crate) stems: Option<(StemFiles, Arc<StemMix>)>,
}

impl OpenRequest {
    /// Opens the file or the stems and decodes their first chunk. This
    /// reads the start of the file, so it runs off the main thread.
    pub(crate) fn open(self) -> Result<TrackSource, PlaybackProblem> {
        match self.stems {
            Some((files, mix)) => {
                TrackSource::open_stems_shifted(&files, mix, self.from_seconds, self.semitones)
            }
            None => TrackSource::open_shifted(&self.path, self.from_seconds, self.semitones),
        }
    }
}

/// The answer to an [`OpenRequest`], once the source is open or could not
/// be opened.
pub(crate) type Opening = async_channel::Receiver<Result<TrackSource, PlaybackProblem>>;

/// Opens `request` on the calling thread and hands the answer over at
/// once.
pub(crate) fn open_here(request: OpenRequest) -> Opening {
    let (sender, receiver) = async_channel::bounded(1);
    let _ = sender.try_send(request.open());
    receiver
}

/// A sound that is playing or paused. Dropping it stops the sound.
pub(crate) trait Voice {
    /// Holds the sound where it is.
    fn pause(&self);
    /// Carries on from where the sound was held.
    fn resume(&self);
}

/// Somewhere sound can be played.
pub(crate) trait AudioOutput {
    /// Starts playing `source` at once.
    fn play(&mut self, source: TrackSource) -> Result<Box<dyn Voice>, PlaybackProblem>;

    /// Starts playing a reference sound at once, mixed over whatever else
    /// is playing.
    fn play_reference(&mut self, sound: ReferenceSound) -> Result<Box<dyn Voice>, PlaybackProblem>;

    /// Starts playing the mix of the two decks, which plays until the voice
    /// is dropped.
    fn play_decks(&mut self, mix: DeckMix) -> Result<Box<dyn Voice>, PlaybackProblem>;

    /// Starts playing the metronome's clicks at once, mixed over whatever
    /// else is playing, until the voice is dropped or the clicks end.
    fn play_metronome(&mut self, clicks: Metronome) -> Result<Box<dyn Voice>, PlaybackProblem>;

    /// Opens what `request` asks for, off the main thread. Outputs made for
    /// tests open it at once on the calling thread, so tests stay in step.
    fn open(&self, request: OpenRequest) -> Opening {
        open_here(request)
    }
}

/// The computer's default audio output, opened the first time a track is
/// played and kept open after that.
#[derive(Default)]
pub(crate) struct SystemOutput {
    device: Option<rodio::MixerDeviceSink>,
    underruns: crate::output_stream::Underruns,
}

impl SystemOutput {
    /// A player of its own on the output's mixer, so that it adds to what
    /// the others play.
    fn new_player(&mut self) -> Result<rodio::Player, PlaybackProblem> {
        let device = match &mut self.device {
            Some(device) => device,
            unopened => {
                let mut device = crate::output_stream::open_output(&self.underruns)
                    .map_err(|_| PlaybackProblem::NoOutput)?;
                device.log_on_drop(false);
                unopened.insert(device)
            }
        };
        Ok(rodio::Player::connect_new(device.mixer()))
    }
}

impl AudioOutput for SystemOutput {
    fn play(&mut self, source: TrackSource) -> Result<Box<dyn Voice>, PlaybackProblem> {
        let player = self.new_player()?;
        player.append(source);
        Ok(Box::new(SystemVoice(player)))
    }

    fn play_reference(&mut self, sound: ReferenceSound) -> Result<Box<dyn Voice>, PlaybackProblem> {
        let player = self.new_player()?;
        player.append(sound);
        Ok(Box::new(SystemVoice(player)))
    }

    fn play_decks(&mut self, mix: DeckMix) -> Result<Box<dyn Voice>, PlaybackProblem> {
        let player = self.new_player()?;
        player.append(mix);
        Ok(Box::new(SystemVoice(player)))
    }

    fn play_metronome(&mut self, clicks: Metronome) -> Result<Box<dyn Voice>, PlaybackProblem> {
        let player = self.new_player()?;
        player.append(clicks);
        Ok(Box::new(SystemVoice(player)))
    }

    /// Opens the track on a thread of its own rather than the app's shared
    /// background pool, where a start would queue behind waveform decodes
    /// and artwork for as long as they take.
    fn open(&self, request: OpenRequest) -> Opening {
        let (sender, receiver) = async_channel::bounded(1);
        let _ = std::thread::Builder::new()
            .name("euphoria-playback-open".to_string())
            .spawn(move || {
                let _ = sender.send_blocking(request.open());
            });
        receiver
    }
}

struct SystemVoice(rodio::Player);

impl Voice for SystemVoice {
    fn pause(&self) {
        self.0.pause();
    }

    fn resume(&self) {
        self.0.play();
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use std::sync::atomic::AtomicBool;

    use super::*;

    /// An output that makes no sound and needs no audio device: it accepts
    /// every source and keeps it without ever asking it for samples.
    #[derive(Default)]
    pub(crate) struct SilentOutput;

    struct SilentVoice<Sound>(#[allow(dead_code)] Sound);

    impl<Sound> Voice for SilentVoice<Sound> {
        fn pause(&self) {}

        fn resume(&self) {}
    }

    impl AudioOutput for SilentOutput {
        fn play(&mut self, source: TrackSource) -> Result<Box<dyn Voice>, PlaybackProblem> {
            Ok(Box::new(SilentVoice(source)))
        }

        fn play_reference(
            &mut self,
            sound: ReferenceSound,
        ) -> Result<Box<dyn Voice>, PlaybackProblem> {
            Ok(Box::new(SilentVoice(sound)))
        }

        fn play_decks(&mut self, mix: DeckMix) -> Result<Box<dyn Voice>, PlaybackProblem> {
            Ok(Box::new(SilentVoice(mix)))
        }

        fn play_metronome(&mut self, clicks: Metronome) -> Result<Box<dyn Voice>, PlaybackProblem> {
            Ok(Box::new(SilentVoice(clicks)))
        }
    }

    /// A reference sound a [`ListeningOutput`] was given, and whether it is
    /// still sounding.
    #[derive(Debug, Clone)]
    pub(crate) struct Heard {
        /// The frequencies the sound is made of.
        pub(crate) frequencies: Vec<f32>,
        /// True for a drone, false for a piano note.
        pub(crate) is_drone: bool,
        sounding: Arc<AtomicBool>,
    }

    impl Heard {
        /// True until the sound is stopped.
        pub(crate) fn is_sounding(&self) -> bool {
            self.sounding.load(Ordering::Relaxed)
        }
    }

    /// A silent output that notes every reference sound it is given, so a
    /// test can tell what would be heard over the track.
    #[derive(Default, Clone)]
    pub(crate) struct ListeningOutput {
        heard: Arc<Mutex<Vec<Heard>>>,
        metronomes: Arc<Mutex<Vec<Arc<AtomicBool>>>>,
    }

    impl ListeningOutput {
        /// Every reference sound given so far, oldest first.
        pub(crate) fn heard(&self) -> Vec<Heard> {
            self.heard
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }

        /// How many metronomes were started, and how many of those are
        /// still sounding.
        pub(crate) fn metronomes(&self) -> (usize, usize) {
            let metronomes = self
                .metronomes
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let sounding = metronomes
                .iter()
                .filter(|sounding| sounding.load(Ordering::Relaxed))
                .count();
            (metronomes.len(), sounding)
        }

        /// The reference sounds still sounding, oldest first.
        pub(crate) fn sounding(&self) -> Vec<Heard> {
            self.heard()
                .into_iter()
                .filter(Heard::is_sounding)
                .collect()
        }
    }

    struct ListenedVoice(Arc<AtomicBool>);

    impl Voice for ListenedVoice {
        fn pause(&self) {}

        fn resume(&self) {}
    }

    impl Drop for ListenedVoice {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Relaxed);
        }
    }

    impl AudioOutput for ListeningOutput {
        fn play(&mut self, source: TrackSource) -> Result<Box<dyn Voice>, PlaybackProblem> {
            Ok(Box::new(SilentVoice(source)))
        }

        fn play_reference(
            &mut self,
            sound: ReferenceSound,
        ) -> Result<Box<dyn Voice>, PlaybackProblem> {
            let sounding = Arc::new(AtomicBool::new(true));
            self.heard
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(Heard {
                    frequencies: sound.frequencies(),
                    is_drone: sound.is_drone(),
                    sounding: sounding.clone(),
                });
            Ok(Box::new(ListenedVoice(sounding)))
        }

        fn play_decks(&mut self, mix: DeckMix) -> Result<Box<dyn Voice>, PlaybackProblem> {
            Ok(Box::new(SilentVoice(mix)))
        }

        fn play_metronome(&mut self, _: Metronome) -> Result<Box<dyn Voice>, PlaybackProblem> {
            let sounding = Arc::new(AtomicBool::new(true));
            self.metronomes
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(sounding.clone());
            Ok(Box::new(ListenedVoice(sounding)))
        }
    }

    /// A silent output whose tracks open only when the test says so, as a
    /// slow disk or a long decode would, and that notes which tracks it
    /// plays and which of them still sound.
    #[derive(Default, Clone)]
    pub(crate) struct SlowOpeningOutput {
        held: Arc<Mutex<SlowOpenings>>,
    }

    #[derive(Default)]
    struct SlowOpenings {
        waiting: VecDeque<(
            OpenRequest,
            async_channel::Sender<Result<TrackSource, PlaybackProblem>>,
        )>,
        opened: Vec<(Arc<VoiceState>, PathBuf)>,
        voices: Vec<(PathBuf, Arc<AtomicBool>)>,
    }

    impl SlowOpeningOutput {
        fn lock(&self) -> MutexGuard<'_, SlowOpenings> {
            self.held.lock().unwrap_or_else(PoisonError::into_inner)
        }

        /// The tracks asked for that have not opened yet, oldest first.
        pub(crate) fn waiting(&self) -> Vec<PathBuf> {
            self.lock()
                .waiting
                .iter()
                .map(|(request, _)| request.path.clone())
                .collect()
        }

        /// Opens the oldest track asked for and hands it over.
        pub(crate) fn finish_opening(&self) {
            let Some((request, answer)) = self.lock().waiting.pop_front() else {
                panic!("no track is opening");
            };
            let path = request.path.clone();
            let opened = request.open();
            if let Ok(source) = &opened {
                self.lock().opened.push((source.state(), path));
            }
            let _ = answer.try_send(opened);
        }

        /// Answers the oldest track asked for with a problem instead.
        pub(crate) fn fail_opening(&self) {
            let Some((_, answer)) = self.lock().waiting.pop_front() else {
                panic!("no track is opening");
            };
            let _ = answer.try_send(Err(PlaybackProblem::NotDecodable));
        }

        /// The tracks whose sound is still playing, oldest first.
        pub(crate) fn sounding(&self) -> Vec<PathBuf> {
            self.lock()
                .voices
                .iter()
                .filter(|(_, sounding)| sounding.load(Ordering::Relaxed))
                .map(|(path, _)| path.clone())
                .collect()
        }
    }

    impl AudioOutput for SlowOpeningOutput {
        fn play(&mut self, source: TrackSource) -> Result<Box<dyn Voice>, PlaybackProblem> {
            let state = source.state();
            let mut held = self.lock();
            let path = held
                .opened
                .iter()
                .find(|(opened, _)| Arc::ptr_eq(opened, &state))
                .map(|(_, path)| path.clone())
                .unwrap_or_default();
            let sounding = Arc::new(AtomicBool::new(true));
            held.voices.push((path, sounding.clone()));
            Ok(Box::new(SoundingVoice {
                sounding,
                _source: source,
            }))
        }

        fn play_reference(
            &mut self,
            sound: ReferenceSound,
        ) -> Result<Box<dyn Voice>, PlaybackProblem> {
            Ok(Box::new(SilentVoice(sound)))
        }

        fn play_decks(&mut self, mix: DeckMix) -> Result<Box<dyn Voice>, PlaybackProblem> {
            Ok(Box::new(SilentVoice(mix)))
        }

        fn play_metronome(&mut self, clicks: Metronome) -> Result<Box<dyn Voice>, PlaybackProblem> {
            Ok(Box::new(SilentVoice(clicks)))
        }

        fn open(&self, request: OpenRequest) -> Opening {
            let (sender, receiver) = async_channel::bounded(1);
            self.lock().waiting.push_back((request, sender));
            receiver
        }
    }

    struct SoundingVoice {
        sounding: Arc<AtomicBool>,
        _source: TrackSource,
    }

    impl Voice for SoundingVoice {
        fn pause(&self) {}

        fn resume(&self) {}
    }

    impl Drop for SoundingVoice {
        fn drop(&mut self) {
            self.sounding.store(false, Ordering::Relaxed);
        }
    }

    /// An output on a computer with no audio device.
    #[derive(Default)]
    pub(crate) struct NoOutput;

    impl AudioOutput for NoOutput {
        fn play(&mut self, _: TrackSource) -> Result<Box<dyn Voice>, PlaybackProblem> {
            Err(PlaybackProblem::NoOutput)
        }

        fn play_reference(&mut self, _: ReferenceSound) -> Result<Box<dyn Voice>, PlaybackProblem> {
            Err(PlaybackProblem::NoOutput)
        }

        fn play_decks(&mut self, _: DeckMix) -> Result<Box<dyn Voice>, PlaybackProblem> {
            Err(PlaybackProblem::NoOutput)
        }

        fn play_metronome(&mut self, _: Metronome) -> Result<Box<dyn Voice>, PlaybackProblem> {
            Err(PlaybackProblem::NoOutput)
        }
    }
    /// A deterministic decoder feed for tests that need a read-ahead gap.
    pub(crate) struct ControlledFeed(SyncSender<Decoded>);

    impl ControlledFeed {
        /// Makes one more chunk available at its original content position.
        pub(crate) fn push(&self, start_frame: u64, samples: Vec<f32>) {
            self.0
                .send(Decoded::Chunk {
                    seek_count: 0,
                    start_frame,
                    rate: 1.0,
                    samples,
                    wraps: 0,
                })
                .expect("the controlled source is still alive");
        }
    }

    /// A mono 44.1 kHz source with a supplied first chunk and no decoder
    /// thread. An empty feed inserts silence exactly as a late decoder does.
    pub(crate) fn controlled_source(
        samples: Vec<f32>,
        start_frame: u64,
    ) -> (TrackSource, ControlledFeed) {
        let sample_rate = NonZeroU32::new(44_100).expect("nonzero sample rate");
        let state = Arc::new(VoiceState::new(sample_rate));
        state.frames_played.store(start_frame, Ordering::Relaxed);
        let (sender, decoded) = sync_channel(4);
        let source = TrackSource {
            state,
            decoded,
            sample_rate,
            channel_count: NonZeroU16::MIN,
            duration_seconds: None,
            chunk: samples,
            chunk_seek_count: 0,
            next_sample: 0,
            silence_owed: 0,
            layer_count: 1,
            mix: None,
            gains: vec![1.0],
            fade_step: 1.0,
            chunk_start_frame: start_frame,
            chunk_rate: 1.0,
            is_patient: false,
            declick: Declick::Steady,
            declick_frames: declick_frames(sample_rate),
            frame_gain: 1.0,
        };
        (source, ControlledFeed(sender))
    }
}

#[cfg(test)]
mod loop_tests;
#[cfg(test)]
mod stem_tests;
#[cfg(test)]
mod tempo_tests;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Instant;

    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/euphoria-core/tests/fixtures")
            .join(name)
    }

    fn open(name: &str, from_seconds: f32) -> TrackSource {
        TrackSource::open(&fixture(name), from_seconds).unwrap()
    }

    /// Pulls samples as an audio output would, far faster than real time,
    /// until `enough` says to stop or the source ends. Returns how many
    /// samples of the track came out, not counting the silence played while
    /// the decoder was catching up.
    fn pull_until(source: &mut TrackSource, enough: impl Fn(usize) -> bool) -> usize {
        let state = source.state();
        let channel_count = usize::from(source.channel_count.get());
        let silence_before = state.silent_frames() as usize * channel_count;
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut pulled = 0_usize;
        loop {
            let silence = state.silent_frames() as usize * channel_count - silence_before;
            if enough(pulled.saturating_sub(silence)) {
                return pulled.saturating_sub(silence);
            }
            assert!(Instant::now() < deadline, "the source never finished");
            if source.next().is_none() {
                let silence = state.silent_frames() as usize * channel_count - silence_before;
                return pulled.saturating_sub(silence);
            }
            pulled += 1;
        }
    }

    fn pull_everything(source: &mut TrackSource) -> usize {
        pull_until(source, |_| false)
    }

    /// The frequency of a steady tone in `samples` of one channel, from the
    /// time between its rising zero crossings.
    fn tone_frequency(samples: &[f32], sample_rate: f32) -> f32 {
        let crossings: Vec<usize> = samples
            .windows(2)
            .enumerate()
            .filter(|(_, pair)| pair[0] < 0.0 && pair[1] >= 0.0)
            .map(|(index, _)| index)
            .collect();
        let span = (crossings[crossings.len() - 1] - crossings[0]) as f32;
        (crossings.len() - 1) as f32 * sample_rate / span
    }

    /// The next `count` samples the output would get, waiting for the
    /// decoder rather than counting silence.
    fn next_sound(source: &mut TrackSource, count: usize) -> Vec<f32> {
        let state = source.state();
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut samples = Vec::with_capacity(count);
        while samples.len() < count {
            assert!(Instant::now() < deadline, "the decoder fell silent");
            let silence_before = state.silent_frames();
            let Some(sample) = source.next() else { break };
            if state.silent_frames() == silence_before {
                samples.push(sample);
            }
        }
        samples
    }

    #[test]
    fn shifted_playback_keeps_short_tracks_and_the_last_onset() {
        for frames in [441_usize, 44_100] {
            let path = std::env::temp_dir().join(format!(
                "euphoria-shifted-tail-{}-{frames}.wav",
                std::process::id()
            ));
            let mut bytes = Vec::new();
            bytes.extend_from_slice(b"RIFF");
            bytes.extend_from_slice(&(36 + frames as u32 * 2).to_le_bytes());
            bytes.extend_from_slice(b"WAVEfmt ");
            bytes.extend_from_slice(&16_u32.to_le_bytes());
            bytes.extend_from_slice(&1_u16.to_le_bytes());
            bytes.extend_from_slice(&1_u16.to_le_bytes());
            bytes.extend_from_slice(&44_100_u32.to_le_bytes());
            bytes.extend_from_slice(&88_200_u32.to_le_bytes());
            bytes.extend_from_slice(&2_u16.to_le_bytes());
            bytes.extend_from_slice(&16_u16.to_le_bytes());
            bytes.extend_from_slice(b"data");
            bytes.extend_from_slice(&(frames as u32 * 2).to_le_bytes());
            for frame in 0..frames {
                let sample = if frame >= frames - 441 {
                    ((frame as f32 * 440.0 * std::f32::consts::TAU / 44_100.0).sin() * 16_000.0)
                        as i16
                } else {
                    0
                };
                bytes.extend_from_slice(&sample.to_le_bytes());
            }
            std::fs::write(&path, bytes).unwrap();
            let mut source = TrackSource::open_shifted(&path, 0.0, 2).unwrap();
            let output = next_sound(&mut source, frames + 1);
            std::fs::remove_file(path).unwrap();
            assert_eq!(output.len(), frames);
            let peak = output.iter().copied().map(f32::abs).fold(0.0_f32, f32::max);
            assert!(
                peak > 0.01,
                "{frames}-frame track lost its last onset: {peak}"
            );
        }
    }

    #[test]
    fn a_shifted_seek_keeps_the_remaining_duration_and_original_end_position() {
        let mut source =
            TrackSource::open_shifted(&fixture("sine-440hz-mono.wav"), 0.0, 2).unwrap();
        next_sound(&mut source, 4_410);
        let state = source.state();
        state.seek_to(0.5);
        let output = next_sound(&mut source, 22_051);
        assert_eq!(output.len(), 22_050);
        assert!((state.position_seconds() - 1.0).abs() < 0.0001);
        let found = tone_frequency(&output[4_410..13_230], 44_100.0);
        assert!(
            (found - 440.0 * 2_f32.powf(2.0 / 12.0)).abs() < 3.0,
            "{found}"
        );
    }

    #[test]
    fn changing_the_shift_discards_audio_already_decoded_in_the_old_key() {
        let mut source =
            TrackSource::open_shifted(&fixture("sine-440hz-mono.wav"), 0.0, 2).unwrap();
        // Fill the read-ahead before the user changes the control, as with
        // a track paused while its initial packets are decoded.
        std::thread::sleep(Duration::from_millis(200));
        source.state().set_shift(-3);
        next_sound(&mut source, 8_820);
        let found = tone_frequency(&next_sound(&mut source, 8_820), 44_100.0);
        assert!(
            (found - 440.0 * 2_f32.powf(-3.0 / 12.0)).abs() < 3.0,
            "{found}"
        );
    }

    #[test]
    fn a_shifted_source_plays_in_the_new_key_and_follows_a_change_of_shift() {
        let mut source =
            TrackSource::open_shifted(&fixture("sine-440hz-mono.wav"), 0.0, 2).unwrap();
        let state = source.state();
        let rate = source.sample_rate.get() as f32;
        // Past the shifter's latency, the tone is two semitones higher.
        next_sound(&mut source, 8_820);
        let up = tone_frequency(&next_sound(&mut source, 8_820), rate);
        assert!((up - 440.0 * 2_f32.powf(2.0 / 12.0)).abs() < 3.0, "{up}");

        state.set_shift(-3);
        // The shift applies from the next chunk the decoder decodes, so the
        // change lands a chunk or more after the call: discard what still
        // plays in the old key until the tone settles on the new one.
        let target = 440.0 * 2_f32.powf(-3.0 / 12.0);
        let mut down = 0.0;
        for _ in 0..48 {
            next_sound(&mut source, 4_410);
            down = tone_frequency(&next_sound(&mut source, 4_410), rate);
            if (down - target).abs() < 3.0 {
                break;
            }
        }
        assert!((down - target).abs() < 3.0, "{down}");
    }

    #[test]
    fn a_source_tells_the_output_its_layout_and_length() {
        use rodio::Source;
        let source = open("sine-440hz-stereo.wav", 0.0);
        assert_eq!(source.channels().get(), 2);
        assert_eq!(Source::sample_rate(&source).get(), 44_100);
        let length = source.total_duration().unwrap().as_secs_f32();
        assert!((length - 1.0).abs() < 0.01);
        assert!((source.duration_seconds().unwrap() - 1.0).abs() < 0.01);
    }

    #[test]
    fn the_first_sample_is_ready_without_waiting_for_the_decoder() {
        let mut source = open("sine-440hz-mono.mp3", 0.0);
        let state = source.state();
        assert!(source.chunk.len() < 44_100 / 4);
        assert!(source.next().is_some());
        assert_eq!(state.silent_frames(), 0);
        assert!(!state.is_finished());
    }

    #[test]
    fn the_position_follows_the_samples_handed_out() {
        let mut source = open("sine-440hz-mono.wav", 0.0);
        let state = source.state();
        assert_eq!(state.position_seconds(), 0.0);
        let half_a_second = 44_100 / 2;
        pull_until(&mut source, |samples| samples >= half_a_second);
        let position = state.position_seconds();
        assert!((0.4..=0.5).contains(&position), "{position}");
    }

    #[test]
    fn playing_to_the_end_finishes_without_a_problem() {
        for name in [
            "sine-440hz-mono.flac",
            "sine-440hz-mono.mp3",
            "sine-440hz-mono.m4a",
        ] {
            let mut source = open(name, 0.0);
            let state = source.state();
            let whole = euphoria_core::decode_mono(&fixture(name)).unwrap();
            assert_eq!(
                pull_everything(&mut source),
                whole.samples().len(),
                "{name}"
            );
            assert!(state.is_finished(), "{name}");
            assert_eq!(state.problem(), None, "{name}");
            let end = whole.duration_seconds();
            assert!((state.position_seconds() - end).abs() < 0.01, "{name}");
            assert_eq!(source.next(), None, "{name}");
        }
    }

    #[test]
    fn stereo_frames_stay_whole_through_silence() {
        let mut source = open("sine-440hz-stereo.wav", 0.0);
        let state = source.state();
        let mut pulled = 0_usize;
        while source.next().is_some() {
            pulled += 1;
        }
        assert_eq!(pulled % 2, 0);
        assert_eq!(pulled - state.silent_frames() as usize * 2, 2 * 44_100);
    }

    #[test]
    fn a_source_can_start_part_of_the_way_in() {
        let mut source = open("sine-440hz-mono.wav", 0.75);
        let state = source.state();
        assert!((state.position_seconds() - 0.75).abs() < 1.0e-3);
        let remaining = pull_everything(&mut source);
        assert!(remaining.abs_diff(44_100 / 4) <= 64, "{remaining}");
    }

    #[test]
    fn a_seek_shows_at_once_and_drops_what_was_decoded_ahead() {
        let mut source = open("sine-440hz-mono.wav", 0.0);
        let state = source.state();
        pull_until(&mut source, |samples| samples >= 1_000);

        state.seek_to(0.5);
        assert_eq!(state.position_seconds(), 0.5);
        let remaining = pull_everything(&mut source);
        assert!(
            remaining.abs_diff(44_100 / 2) <= 64,
            "{remaining} samples played after the seek"
        );
        assert!(state.is_finished());
        assert!((state.position_seconds() - 1.0).abs() < 0.01);
    }

    #[test]
    fn a_seek_backwards_plays_the_earlier_part_again() {
        let mut source = open("sine-440hz-mono.flac", 0.0);
        let state = source.state();
        pull_until(&mut source, |samples| samples >= 30_000);
        state.seek_to(0.25);
        let remaining = pull_everything(&mut source);
        assert!(remaining.abs_diff(44_100 * 3 / 4) <= 64, "{remaining}");
    }

    #[test]
    fn several_seeks_in_a_row_end_at_the_last_one() {
        let mut source = open("sine-440hz-mono.wav", 0.0);
        let state = source.state();
        pull_until(&mut source, |samples| samples >= 500);
        for seconds in [0.1, 0.9, 0.3, 0.6] {
            state.seek_to(seconds);
        }
        assert_eq!(state.position_seconds(), 0.6);
        let remaining = pull_everything(&mut source);
        assert!(remaining.abs_diff(44_100 * 4 / 10) <= 64, "{remaining}");
    }

    #[test]
    fn sound_resumes_within_a_moment_of_a_seek() {
        let mut source = open("sine-440hz-mono.mp3", 0.0);
        let state = source.state();
        pull_until(&mut source, |samples| samples >= 2_000);

        state.seek_to(0.5);
        let asked_at = Instant::now();
        let silence_before = state.silent_frames();
        let mut sounded = false;
        while asked_at.elapsed() < Duration::from_secs(5) {
            let before = state.silent_frames();
            if source.next().is_none() {
                break;
            }
            if state.silent_frames() == before {
                sounded = true;
                break;
            }
        }
        assert!(sounded, "no sound came back after the seek");
        // Sound comes back about 2 ms after the seek on an M5, idle or with
        // every core busy. The bound leaves room for a busy machine to set
        // the decoder thread aside for a while.
        assert!(
            asked_at.elapsed() < Duration::from_secs(1),
            "sound took {:?} to come back",
            asked_at.elapsed()
        );
        assert!(state.silent_frames() >= silence_before);
        let position = state.position_seconds();
        assert!((0.45..=0.6).contains(&position), "{position}");
    }

    #[test]
    fn seeking_after_the_end_does_not_revive_a_finished_source() {
        let mut source = open("sine-440hz-mono.wav", 0.0);
        let state = source.state();
        pull_everything(&mut source);
        state.seek_to(0.0);
        assert_eq!(source.next(), None);
    }

    #[test]
    fn the_decoder_stops_reading_ahead_when_the_buffer_is_full_and_ends_with_the_source() {
        let source = open("sine-440hz-mono.mp3", 0.0);
        let state = source.state();
        std::thread::sleep(Duration::from_millis(100));
        assert!(!state.is_finished());
        assert_eq!(Arc::strong_count(&state), 3);

        drop(source);
        let deadline = Instant::now() + Duration::from_secs(5);
        while Arc::strong_count(&state) > 1 {
            assert!(Instant::now() < deadline, "the decoder thread kept running");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn a_vanished_file_and_a_file_that_is_not_audio_have_their_own_messages() {
        let missing = TrackSource::open(Path::new("/definitely/not/here.mp3"), 0.0);
        assert_eq!(missing.err(), Some(PlaybackProblem::FileMissing));

        let not_audio = TrackSource::open(&fixture("README.md"), 0.0);
        assert_eq!(not_audio.err(), Some(PlaybackProblem::NotDecodable));
    }

    #[test]
    fn every_problem_has_a_short_message() {
        for problem in [
            PlaybackProblem::NoOutput,
            PlaybackProblem::FileMissing,
            PlaybackProblem::FileUnreadable,
            PlaybackProblem::NotDecodable,
        ] {
            let message = problem.message();
            assert!(!message.is_empty() && message.len() <= 40, "{message}");
            assert!(!message.ends_with('.'));
        }
    }

    #[test]
    fn a_decoding_problem_reaches_the_window_and_ends_the_sound() {
        let mut source = open("sine-440hz-mono.wav", 0.0);
        let state = source.state();
        state.stop_on(PlaybackProblem::NotDecodable);
        source.next_sample = source.chunk.len();
        assert_eq!(source.next(), None);
        assert_eq!(state.problem(), Some(PlaybackProblem::NotDecodable));
        assert!(state.is_finished());
    }

    #[test]
    fn a_file_that_vanishes_while_playing_ends_one_way_or_the_other() {
        let copy = std::env::temp_dir().join(format!(
            "euphoria-playback-vanishing-{}.wav",
            std::process::id()
        ));
        std::fs::copy(fixture("sine-440hz-mono.wav"), &copy).unwrap();
        let mut source = TrackSource::open(&copy, 0.0).unwrap();
        let state = source.state();
        assert!(source.next().is_some());
        std::fs::remove_file(&copy).unwrap();

        pull_everything(&mut source);
        assert!(state.is_finished());
    }

    #[test]
    fn the_test_outputs_accept_or_refuse_without_a_device() {
        use testing::{NoOutput, SilentOutput};
        assert!(SilentOutput.play(open("sine-440hz-mono.wav", 0.0)).is_ok());
        let refused = NoOutput.play(open("sine-440hz-mono.wav", 0.0));
        assert_eq!(refused.err(), Some(PlaybackProblem::NoOutput));
    }
}
