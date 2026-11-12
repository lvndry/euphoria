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
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI8, AtomicU64, AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use euphoria_core::shift::PitchShifter;
use euphoria_core::stems::{Stem, StemFiles};
use euphoria_core::{AudioStream, EuphoriaError};

use crate::deck_audio::DeckMix;
use crate::reference_sound::ReferenceSound;

/// How many decoded chunks wait between the decoder and the audio output. A
/// chunk is one packet of the file, from about 25 milliseconds of an MP3 to
/// about 90 of a FLAC, so this is one to four seconds of sound.
const READ_AHEAD_CHUNKS: usize = 48;

/// How long the decoder sleeps when it has nothing to do: the read ahead is
/// full, or the track has been decoded to its end.
const DECODER_PAUSE: Duration = Duration::from_millis(4);

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
        }
    }

    /// How far the sound has got, in seconds from the beginning. Right after
    /// a seek this is where the seek asked for, before sound from there has
    /// started.
    pub(crate) fn position_seconds(&self) -> f32 {
        if let Some(shown) = self.lock_seek_requests().shown {
            return shown;
        }
        self.frames_played.load(Ordering::Relaxed) as f32 / self.sample_rate.get() as f32
    }

    /// Jumps to `seconds` from the beginning. What was decoded ahead is
    /// dropped at once, and sound resumes from the new place as soon as the
    /// decoder has a first chunk from there.
    pub(crate) fn seek_to(&self, seconds: f32) {
        let seconds = seconds.max(0.0);
        let mut requests = self.lock_seek_requests();
        requests.for_decoder = Some(seconds);
        requests.shown = Some(seconds);
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
            let mut requests = self.lock_seek_requests();
            let seconds = (self.frames_played.load(Ordering::Relaxed) as f32
                / self.sample_rate.get() as f32)
                .max(0.0);
            requests.for_decoder = Some(seconds);
            requests.shown = Some(seconds);
            self.seek_count.fetch_add(1, Ordering::Release);
        }
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

/// Which stems are heard, shared between the window, which changes it, and
/// the audio thread, which follows it within a few milliseconds.
#[derive(Debug)]
pub(crate) struct StemMix {
    audible: AtomicU8,
}

impl Default for StemMix {
    fn default() -> Self {
        Self {
            audible: AtomicU8::new(StemMix::ALL_AUDIBLE),
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

    pub(crate) fn gain(&self, layer: usize) -> f32 {
        if self.audible.load(Ordering::Relaxed) & (1 << layer) != 0 {
            1.0
        } else {
            0.0
        }
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
}

/// What the decoder hands to the audio thread.
enum Decoded {
    /// A stretch of interleaved samples.
    Chunk {
        /// The seek count the chunk was decoded under.
        seek_count: u64,
        /// The frame of the track the chunk starts at.
        start_frame: u64,
        samples: Vec<f32>,
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
    next_frame: u64,
    is_at_end: bool,
    shift: Shift,
}

/// The pitch shifter of a playing track, rebuilt when the shift changes or
/// the track seeks, since what it holds belongs to the audio before.
struct Shift {
    semitones: i8,
    shifter: Option<PitchShifter>,
    channels: usize,
    sample_rate: u32,
}

impl Shift {
    fn new(channels: usize, sample_rate: u32, semitones: i8) -> Result<Self, PlaybackProblem> {
        let mut shift = Self {
            semitones,
            shifter: None,
            channels,
            sample_rate,
        };
        shift.rebuild()?;
        Ok(shift)
    }

    fn rebuild(&mut self) -> Result<(), PlaybackProblem> {
        self.shifter = if self.semitones != 0 {
            Some(
                PitchShifter::new(self.channels, self.sample_rate, self.semitones)
                    .map_err(|_| PlaybackProblem::NotDecodable)?,
            )
        } else {
            None
        };
        Ok(())
    }

    /// `samples` shifted by `semitones`, which may have changed since the
    /// last chunk. A DSP failure stops playback rather than substituting
    /// audio in the original key or losing held samples.
    fn apply(&mut self, samples: Vec<f32>, semitones: i8) -> Result<Vec<f32>, PlaybackProblem> {
        if semitones != self.semitones {
            self.semitones = semitones;
            self.rebuild()?;
        }
        match &mut self.shifter {
            Some(shifter) => shifter
                .process(&samples)
                .map_err(|_| PlaybackProblem::NotDecodable),
            None => Ok(samples),
        }
    }

    /// Drains the held audio before EOF. Unlike live output, the streaming
    /// shifter removes its startup latency and restores exactly the input's
    /// duration once this tail has been handed to the output.
    fn finish(&mut self) -> Result<Vec<f32>, PlaybackProblem> {
        match self.shifter.take() {
            Some(shifter) => shifter.finish().map_err(|_| PlaybackProblem::NotDecodable),
            None => Ok(Vec::new()),
        }
    }
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
        match self.stream.seek(seconds) {
            Ok(reached) => {
                self.next_frame = self.state.frames_at(reached);
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
            let mut samples = Vec::new();
            let samples = match self.stream.read_chunk(&mut samples) {
                Ok(true) => match self
                    .shift
                    .apply(samples, self.state.semitones.load(Ordering::Relaxed))
                {
                    Ok(samples) => samples,
                    Err(problem) => return Decoded::Problem(problem),
                },
                Ok(false) => {
                    let tail = match self.shift.finish() {
                        Ok(tail) => tail,
                        Err(problem) => return Decoded::Problem(problem),
                    };
                    if tail.is_empty() {
                        self.is_at_end = true;
                        return Decoded::End {
                            seek_count: self.seek_count,
                            end_frame: self.next_frame,
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
            let start_frame = self.next_frame;
            let samples_per_frame =
                usize::from(self.stream.channel_count().get()) * self.stream.layer_count();
            self.next_frame += (samples.len() / samples_per_frame) as u64;
            return Decoded::Chunk {
                seek_count: self.seek_count,
                start_frame,
                samples,
            };
        }
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
    /// Whether to wait for the decoder rather than play silence, for tests
    /// that pull samples far faster than real time.
    is_patient: bool,
}

impl TrackSource {
    /// Opens the file at `path` positioned `from_seconds` in, decodes its
    /// first chunk and starts the decoder thread. This reads the start of
    /// the file, so call it off the main thread.
    pub(crate) fn open(path: &Path, from_seconds: f32) -> Result<Self, PlaybackProblem> {
        Self::open_shifted(path, from_seconds, 0)
    }

    /// Opens the file at `path` positioned `from_seconds` in, with its key
    /// shifted by `semitones`. [`VoiceState::set_shift`] changes the shift
    /// while it plays. Call this off the main thread.
    pub(crate) fn open_shifted(
        path: &Path,
        from_seconds: f32,
        semitones: i8,
    ) -> Result<Self, PlaybackProblem> {
        let stream = AudioStream::open(path).map_err(|error| PlaybackProblem::from_core(&error))?;
        Self::start(Reader::File(stream), None, semitones, from_seconds)
    }

    /// Opens the four stems of a track, to be mixed as `mix` says while
    /// they play. Like [`TrackSource::open`], call it off the main thread.
    pub(crate) fn open_stems(
        files: &StemFiles,
        mix: Arc<StemMix>,
        from_seconds: f32,
    ) -> Result<Self, PlaybackProblem> {
        let reader = StemReader::open(files).map_err(|error| PlaybackProblem::from_core(&error))?;
        Self::start(Reader::Stems(reader), Some(mix), 0, from_seconds)
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
            stream.sample_rate().get(),
            semitones,
        )?;
        let start_frame = state.frames_at(reached_seconds);
        state.frames_played.store(start_frame, Ordering::Relaxed);

        // Decode enough preroll here for the first shifted sound to be
        // ready before the audio callback asks. This runs off the UI thread.
        let mut first_chunk;
        let has_audio = loop {
            let mut input = Vec::new();
            let has_audio = stream.read_chunk(&mut input).map_err(problem)?;
            first_chunk = if has_audio {
                shift.apply(input, semitones)?
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
            is_patient: false,
        };
        let decoder = Decoder {
            stream,
            state,
            decoded: decoded_sender,
            seek_count: 0,
            next_frame: start_frame + frames_in_first_chunk,
            is_at_end: !has_audio,
            shift,
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
        self.chunk_start_frame + (self.next_sample.min(self.chunk.len()) / channel_count) as u64
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
                    samples,
                }) => {
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

        self.gains
            .iter()
            .enumerate()
            .map(|(layer, gain)| {
                gain * self.chunk[frame * channel_count + layer * channel_count + channel]
            })
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

impl Iterator for TrackSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.silence_owed > 0 {
            self.silence_owed -= 1;
            return Some(0.0);
        }
        let latest_seek_count = self.state.seek_count.load(Ordering::Acquire);
        let needs_chunk =
            self.next_sample >= self.chunk_samples() || self.chunk_seek_count != latest_seek_count;
        if needs_chunk {
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
                        self.chunk_start_frame +=
                            (self.chunk.len() / usize::from(self.channel_count.get())) as u64;
                    }
                    self.chunk.clear();
                    self.next_sample = 0;
                    self.silence_owed = usize::from(self.channel_count.get()) - 1;
                    self.state.silent_frames.fetch_add(1, Ordering::Relaxed);
                    return Some(0.0);
                }
            }
        }
        let sample = self.mixed_sample();
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
            self.state.frames_played.fetch_add(1, Ordering::Relaxed);
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
}

/// The computer's default audio output, opened the first time a track is
/// played and kept open after that.
#[derive(Default)]
pub(crate) struct SystemOutput {
    device: Option<rodio::MixerDeviceSink>,
}

impl SystemOutput {
    /// A player of its own on the output's mixer, so that it adds to what
    /// the others play.
    fn new_player(&mut self) -> Result<rodio::Player, PlaybackProblem> {
        let device = match &mut self.device {
            Some(device) => device,
            unopened => {
                let mut device = rodio::DeviceSinkBuilder::open_default_sink()
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
    }

    impl ListeningOutput {
        /// Every reference sound given so far, oldest first.
        pub(crate) fn heard(&self) -> Vec<Heard> {
            self.heard
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
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
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Instant;

    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../euphoria-core/tests/fixtures")
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
