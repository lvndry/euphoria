// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Making sound: a track is decoded by the core a packet at a time and fed
//! to the system's audio output, so playing starts as soon as the file is
//! open and never waits for the whole track.

use std::num::{NonZeroU16, NonZeroU32};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use euphoria_core::{AudioStream, EuphoriaError};

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

/// What the sound of one track is doing, shared between the audio thread
/// that produces it and the window that shows it.
#[derive(Debug)]
pub(crate) struct VoiceState {
    sample_rate: NonZeroU32,
    frames_played: AtomicU64,
    requested_position: Mutex<Option<f32>>,
    /// True while `requested_position` holds a seek the audio thread has not
    /// taken yet, so that thread can check with one load per sample.
    has_request: AtomicBool,
    is_finished: AtomicBool,
    problem: Mutex<Option<PlaybackProblem>>,
}

impl VoiceState {
    fn new(sample_rate: NonZeroU32, from_seconds: f32) -> Self {
        let state = Self {
            sample_rate,
            frames_played: AtomicU64::new(0),
            requested_position: Mutex::new(None),
            has_request: AtomicBool::new(false),
            is_finished: AtomicBool::new(false),
            problem: Mutex::new(None),
        };
        state.set_position(from_seconds);
        state
    }

    /// How far the sound has got, in seconds from the beginning. Right after
    /// a seek this is where the seek asked for, before the audio thread has
    /// caught up.
    pub(crate) fn position_seconds(&self) -> f32 {
        if let Some(requested) = *self.lock_requested_position() {
            return requested;
        }
        self.frames_played.load(Ordering::Relaxed) as f32 / self.sample_rate.get() as f32
    }

    /// Asks the audio thread to jump to `seconds` from the beginning.
    pub(crate) fn seek_to(&self, seconds: f32) {
        *self.lock_requested_position() = Some(seconds.max(0.0));
        self.has_request.store(true, Ordering::Release);
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

    fn set_position(&self, seconds: f32) {
        let frames = (f64::from(seconds.max(0.0)) * f64::from(self.sample_rate.get())).round();
        self.frames_played.store(frames as u64, Ordering::Relaxed);
    }

    fn lock_requested_position(&self) -> std::sync::MutexGuard<'_, Option<f32>> {
        self.requested_position
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// The samples of one track, produced as the audio output asks for them.
pub(crate) struct TrackSource {
    stream: AudioStream,
    state: Arc<VoiceState>,
    chunk: Vec<f32>,
    next_sample: usize,
}

impl TrackSource {
    /// Opens the file at `path` positioned `from_seconds` in. This reads the
    /// start of the file, so call it off the main thread.
    pub(crate) fn open(path: &Path, from_seconds: f32) -> Result<Self, PlaybackProblem> {
        let problem = |error: EuphoriaError| PlaybackProblem::from_core(&error);
        let mut stream = AudioStream::open(path).map_err(problem)?;
        if from_seconds > 0.0 {
            stream.seek(from_seconds).map_err(problem)?;
        }
        let state = Arc::new(VoiceState::new(stream.sample_rate(), from_seconds));
        Ok(Self {
            stream,
            state,
            chunk: Vec::new(),
            next_sample: 0,
        })
    }

    /// The state the window reads while this source plays.
    pub(crate) fn state(&self) -> Arc<VoiceState> {
        Arc::clone(&self.state)
    }

    /// The length the file declares, in seconds, when it declares one.
    pub(crate) fn duration_seconds(&self) -> Option<f32> {
        self.stream.duration_seconds()
    }

    /// Decodes the next chunk, first jumping to wherever a seek asked for.
    /// Returns false when there is nothing more to play.
    fn refill(&mut self) -> bool {
        let frames_handed_out = self.next_sample / usize::from(self.channel_count().get());
        self.state
            .frames_played
            .fetch_add(frames_handed_out as u64, Ordering::Relaxed);
        self.chunk.clear();
        self.next_sample = 0;

        let requested = if self.state.has_request.swap(false, Ordering::AcqRel) {
            self.state.lock_requested_position().take()
        } else {
            None
        };
        if let Some(seconds) = requested {
            match self.stream.seek(seconds) {
                Ok(reached) => self.state.set_position(reached),
                Err(error) => return self.stop_on(&error),
            }
        }
        match self.stream.read_chunk(&mut self.chunk) {
            Ok(true) => true,
            Ok(false) => {
                self.state.is_finished.store(true, Ordering::Relaxed);
                false
            }
            Err(error) => self.stop_on(&error),
        }
    }

    fn stop_on(&mut self, error: &EuphoriaError) -> bool {
        *self
            .state
            .problem
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(PlaybackProblem::from_core(error));
        self.state.is_finished.store(true, Ordering::Relaxed);
        self.chunk.clear();
        false
    }

    fn channel_count(&self) -> NonZeroU16 {
        self.stream.channel_count()
    }
}

impl Iterator for TrackSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let needs_chunk =
            self.next_sample >= self.chunk.len() || self.state.has_request.load(Ordering::Acquire);
        if needs_chunk && (self.state.is_finished() || !self.refill()) {
            return None;
        }
        let sample = self.chunk[self.next_sample];
        self.next_sample += 1;
        Some(sample)
    }
}

impl rodio::Source for TrackSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> NonZeroU16 {
        self.channel_count()
    }

    fn sample_rate(&self) -> NonZeroU32 {
        self.stream.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.stream.duration_seconds().map(Duration::from_secs_f32)
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
}

/// The computer's default audio output, opened the first time a track is
/// played and kept open after that.
#[derive(Default)]
pub(crate) struct SystemOutput {
    device: Option<rodio::MixerDeviceSink>,
}

impl AudioOutput for SystemOutput {
    fn play(&mut self, source: TrackSource) -> Result<Box<dyn Voice>, PlaybackProblem> {
        let device = match &mut self.device {
            Some(device) => device,
            unopened => {
                let mut device = rodio::DeviceSinkBuilder::open_default_sink()
                    .map_err(|_| PlaybackProblem::NoOutput)?;
                device.log_on_drop(false);
                unopened.insert(device)
            }
        };
        let player = rodio::Player::connect_new(device.mixer());
        player.append(source);
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
    use super::*;

    /// An output that makes no sound and needs no audio device: it accepts
    /// every source and keeps it without ever asking it for samples.
    #[derive(Default)]
    pub(crate) struct SilentOutput;

    struct SilentVoice(#[allow(dead_code)] TrackSource);

    impl Voice for SilentVoice {
        fn pause(&self) {}

        fn resume(&self) {}
    }

    impl AudioOutput for SilentOutput {
        fn play(&mut self, source: TrackSource) -> Result<Box<dyn Voice>, PlaybackProblem> {
            Ok(Box::new(SilentVoice(source)))
        }
    }

    /// An output on a computer with no audio device.
    #[derive(Default)]
    pub(crate) struct NoOutput;

    impl AudioOutput for NoOutput {
        fn play(&mut self, _: TrackSource) -> Result<Box<dyn Voice>, PlaybackProblem> {
            Err(PlaybackProblem::NoOutput)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../euphoria-core/tests/fixtures")
            .join(name)
    }

    fn open(name: &str, from_seconds: f32) -> TrackSource {
        TrackSource::open(&fixture(name), from_seconds).unwrap()
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
    fn the_first_samples_come_without_decoding_the_whole_file() {
        let mut source = open("sine-440hz-mono.mp3", 0.0);
        assert!(source.next().is_some());
        assert!(source.chunk.len() < 44_100 / 4);
        assert!(!source.state().is_finished());
    }

    #[test]
    fn the_position_follows_the_samples_handed_out() {
        let mut source = open("sine-440hz-mono.wav", 0.0);
        let state = source.state();
        assert_eq!(state.position_seconds(), 0.0);
        let half_a_second = 44_100 / 2;
        assert_eq!(source.by_ref().take(half_a_second).count(), half_a_second);
        let position = state.position_seconds();
        assert!((0.4..=0.5).contains(&position), "{position}");
    }

    #[test]
    fn playing_to_the_end_finishes_without_a_problem() {
        let mut source = open("sine-440hz-mono.flac", 0.0);
        let state = source.state();
        let sample_count = source.by_ref().count();
        assert_eq!(sample_count, 44_100);
        assert!(state.is_finished());
        assert_eq!(state.problem(), None);
        assert!((state.position_seconds() - 1.0).abs() < 0.01);
        assert_eq!(source.next(), None);
    }

    #[test]
    fn a_source_can_start_part_of_the_way_in() {
        let mut source = open("sine-440hz-mono.wav", 0.75);
        let state = source.state();
        assert!((state.position_seconds() - 0.75).abs() < 1.0e-3);
        let remaining = source.by_ref().count();
        assert!(remaining.abs_diff(44_100 / 4) <= 64, "{remaining}");
    }

    #[test]
    fn a_seek_moves_the_position_at_once_and_the_sound_at_the_next_chunk() {
        let mut source = open("sine-440hz-mono.wav", 0.0);
        let state = source.state();
        source.by_ref().take(1_000).for_each(drop);

        state.seek_to(0.5);
        assert_eq!(state.position_seconds(), 0.5);
        let remaining = source.by_ref().count();
        assert!(
            (44_100 / 2..44_100 / 2 + 8_192).contains(&remaining),
            "{remaining} samples played after the seek"
        );
        assert!(state.is_finished());
    }

    #[test]
    fn seeking_back_after_the_end_does_not_revive_a_finished_source() {
        let mut source = open("sine-440hz-mono.wav", 0.0);
        let state = source.state();
        source.by_ref().for_each(drop);
        state.seek_to(0.0);
        assert_eq!(source.next(), None);
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
    fn a_file_that_vanishes_while_playing_stops_with_a_problem_or_plays_out() {
        let copy = std::env::temp_dir().join(format!(
            "euphoria-playback-vanishing-{}.wav",
            std::process::id()
        ));
        std::fs::copy(fixture("sine-440hz-mono.wav"), &copy).unwrap();
        let mut source = TrackSource::open(&copy, 0.0).unwrap();
        let state = source.state();
        assert!(source.next().is_some());
        std::fs::remove_file(&copy).unwrap();

        source.by_ref().for_each(drop);
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
