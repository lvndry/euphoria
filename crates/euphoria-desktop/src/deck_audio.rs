// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The sound of the two decks: one source the audio output plays, which
//! mixes deck A and deck B a block at a time.
//!
//! Each deck plays a [`TrackSource`], so its file is decoded on a thread of
//! its own ahead of what is heard, as in the player. Every deck goes
//! through a Signalsmith Stretch time stretcher, which plays it faster or
//! slower than recorded, keeping its pitch unless key lock is off, so that
//! one deck's beats run at the other's tempo.
//!
//! Both decks are mixed in the same block, so a deck can start on the
//! other's next downbeat to the sample: the mix counts the frames until the
//! other deck's input reaches its downbeat and starts the deck after that
//! many frames, allowing for the time each stretcher holds the sound.
//!
//! The window talks to the mix through a channel of commands, and reads
//! what it needs, such as the position of each deck, from shared counters.

use std::num::{NonZeroU16, NonZeroU32};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use signalsmith_stretch::Stretch;

use crate::playback::{PlaybackProblem, TrackSource};

/// The sample rate the decks are mixed at. The output converts it to the
/// device's own.
pub(crate) const MIX_SAMPLE_RATE: u32 = 44_100;

/// Frames mixed at a time. About 6 milliseconds, short enough for a
/// command to take effect at once.
const BLOCK_FRAMES: usize = 256;

const MIN_RATE: f64 = 0.5;
const MAX_RATE: f64 = 2.0;

/// The beats in a bar, in the 4/4 time of dance music.
const BEATS_PER_BAR: f64 = 4.0;

/// One of the two decks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum DeckId {
    A,
    B,
}

impl DeckId {
    /// Both decks.
    pub(crate) const BOTH: [DeckId; 2] = [DeckId::A, DeckId::B];

    fn index(self) -> usize {
        match self {
            DeckId::A => 0,
            DeckId::B => 1,
        }
    }

    /// The other deck.
    pub(crate) fn other(self) -> DeckId {
        match self {
            DeckId::A => DeckId::B,
            DeckId::B => DeckId::A,
        }
    }

    /// The deck's letter.
    pub(crate) fn letter(self) -> &'static str {
        match self {
            DeckId::A => "A",
            DeckId::B => "B",
        }
    }
}

/// Where the beats of a track are: its first downbeat and its tempo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GridTiming {
    /// Seconds from the start of the track to its first downbeat.
    pub(crate) first_downbeat_seconds: f64,
    /// The tempo of the grid in beats per minute.
    pub(crate) bpm: f64,
}

impl GridTiming {
    /// Seconds from one downbeat to the next.
    pub(crate) fn bar_seconds(&self) -> f64 {
        BEATS_PER_BAR * 60.0 / self.bpm
    }

    /// The first downbeat at or after `seconds`.
    pub(crate) fn next_downbeat(&self, seconds: f64) -> f64 {
        let bar = self.bar_seconds();
        let bars = ((seconds - self.first_downbeat_seconds) / bar)
            .ceil()
            .max(0.0);
        self.first_downbeat_seconds + bars * bar
    }
}

/// The playback speed that gives a track at `own_bpm` the tempo
/// `target_bpm`, reading it at half or double speed when that is closer,
/// as a DJ beatmatches a track counted at half time.
pub(crate) fn matching_rate(own_bpm: f64, target_bpm: f64) -> f64 {
    if own_bpm <= 0.0 || target_bpm <= 0.0 || !own_bpm.is_finite() || !target_bpm.is_finite() {
        return 1.0;
    }
    [1.0, 2.0, 0.5]
        .into_iter()
        .map(|scale| target_bpm * scale / own_bpm)
        .min_by(|first, second| (first.ln().abs()).total_cmp(&second.ln().abs()))
        .unwrap_or(1.0)
        .clamp(MIN_RATE, MAX_RATE)
}

/// A change the window asks of the mix.
pub(crate) enum DeckCommand {
    /// Puts a track on a deck, paused.
    Load {
        deck: DeckId,
        source: Box<TrackSource>,
        grid: Option<GridTiming>,
    },
    /// Plays a deck. With `quantize_to`, it waits for the next downbeat of
    /// that deck when that deck is playing and both have a beat grid.
    Play {
        deck: DeckId,
        quantize_to: Option<DeckId>,
    },
    /// Holds a deck where it is.
    Pause(DeckId),
    /// Moves a deck to `seconds` into its track.
    Seek { deck: DeckId, seconds: f64 },
    /// Plays a deck at `rate` times its own speed.
    SetRate { deck: DeckId, rate: f64 },
    /// Updates the grid after a manual tempo correction.
    SetGrid {
        deck: DeckId,
        grid: Option<GridTiming>,
    },
    /// Keeps a deck's pitch when its speed changes, or lets it follow.
    SetKeylock { deck: DeckId, keylock: bool },
}

/// What one deck is doing, for the window to read.
#[derive(Debug, Default)]
pub(crate) struct DeckMeter {
    /// The position heard, in seconds into the track, as f64 bits.
    position_bits: AtomicU64,
    playing: AtomicBool,
    finished: AtomicBool,
    pending_load: AtomicBool,
    problem: Mutex<Option<PlaybackProblem>>,
    /// True while the deck waits for the other's downbeat to start.
    waiting: AtomicBool,
    /// The volume the window set, fader and crossfader together, as f32
    /// bits.
    gain_bits: AtomicU32,
    /// Time spent mixing this deck, in nanoseconds.
    busy_nanoseconds: AtomicU64,
    /// Frames this deck was mixed for.
    mixed_frames: AtomicU64,
    /// Frames of silence the decoder could not fill in time.
    starved_frames: AtomicU64,
}

impl DeckMeter {
    /// The position heard, in seconds into the track.
    pub(crate) fn position_seconds(&self) -> f64 {
        f64::from_bits(self.position_bits.load(Ordering::Relaxed))
    }

    /// True while the deck plays, or waits to start.
    #[cfg(test)]
    pub(crate) fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    /// True after all the loaded track's buffered sound has played.
    pub(crate) fn is_finished(&self) -> bool {
        !self.pending_load.load(Ordering::Acquire) && self.finished.load(Ordering::Relaxed)
    }

    /// Why the loaded track stopped early, when it did.
    pub(crate) fn problem(&self) -> Option<PlaybackProblem> {
        self.problem
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// True while the deck waits for the other deck's downbeat.
    pub(crate) fn is_waiting(&self) -> bool {
        self.waiting.load(Ordering::Relaxed)
    }

    /// Sets the volume, from 0 to 1.
    pub(crate) fn set_gain(&self, gain: f32) {
        self.gain_bits
            .store(gain.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    fn gain(&self) -> f32 {
        f32::from_bits(self.gain_bits.load(Ordering::Relaxed))
    }

    /// How much of real time mixing this deck has taken: 0.05 means 5
    /// percent of one core.
    pub(crate) fn load(&self) -> f64 {
        let frames = self.mixed_frames.load(Ordering::Relaxed);
        if frames == 0 {
            return 0.0;
        }
        let busy = self.busy_nanoseconds.load(Ordering::Relaxed) as f64 / 1e9;
        busy / (frames as f64 / f64::from(MIX_SAMPLE_RATE))
    }

    /// Frames of silence played because the decoder fell behind.
    pub(crate) fn starved_frames(&self) -> u64 {
        self.starved_frames.load(Ordering::Relaxed)
    }
}

/// The window's end of the mix.
#[derive(Clone)]
pub(crate) struct DeckLink {
    commands: Sender<DeckCommand>,
    meters: [Arc<DeckMeter>; 2],
}

impl DeckLink {
    /// Sends a command. A mix that has gone away ignores it.
    pub(crate) fn send(&self, command: DeckCommand) {
        let loading = match &command {
            DeckCommand::Load { deck, .. } => Some(*deck),
            _ => None,
        };
        if let Some(deck) = loading {
            self.meter(deck).pending_load.store(true, Ordering::Release);
        }
        if self.commands.send(command).is_err()
            && let Some(deck) = loading
        {
            self.meter(deck)
                .pending_load
                .store(false, Ordering::Release);
        }
    }

    /// What `deck` is doing.
    pub(crate) fn meter(&self, deck: DeckId) -> &DeckMeter {
        &self.meters[deck.index()]
    }
}

/// A track on a deck.
struct LoadedTrack {
    source: TrackSource,
    grid: Option<GridTiming>,
    channel_count: usize,
    input_rate: u32,
    stretch: Stretch,
    rate: f64,
    keylock: bool,
    /// The transposition last given to the stretcher.
    transpose: f32,
    playing: bool,
    /// Frames of silence before the deck starts, while it waits for the
    /// other deck's downbeat.
    start_countdown: Option<u64>,
    /// The part of an input frame owed from the last block.
    input_fraction: f64,
    input: Vec<f32>,
    output: Vec<f32>,
    is_finished: bool,
    tail_input_samples: usize,
    tail_output_frames: usize,
    has_rendered: bool,
    start_seconds: f64,
}

impl LoadedTrack {
    fn new(source: TrackSource, grid: Option<GridTiming>) -> Self {
        let channel_count = usize::from(rodio::Source::channels(&source).get());
        let input_rate = rodio::Source::sample_rate(&source).get();
        let stretch = Stretch::preset_default(channel_count as u32, MIX_SAMPLE_RATE);
        let start_seconds = source.next_frame() as f64 / f64::from(input_rate);
        let mut track = Self {
            source,
            grid,
            channel_count,
            input_rate,
            stretch,
            rate: 1.0,
            keylock: true,
            transpose: 0.0,
            playing: false,
            start_countdown: None,
            input_fraction: 0.0,
            input: Vec::new(),
            output: Vec::new(),
            is_finished: false,
            tail_input_samples: 0,
            tail_output_frames: 0,
            has_rendered: false,
            start_seconds,
        };
        track.update_transpose();
        track
    }

    /// Input frames per output frame.
    fn speed(&self) -> f64 {
        self.rate * f64::from(self.input_rate) / f64::from(MIX_SAMPLE_RATE)
    }

    /// The stretcher keeps the pitch of the input in samples, so a track
    /// recorded at another rate than the mix needs transposing back; with
    /// key lock off the pitch follows the speed, as on a turntable.
    fn update_transpose(&mut self) {
        let mut transpose = self.input_rate as f32 / MIX_SAMPLE_RATE as f32;
        if !self.keylock {
            transpose *= self.rate as f32;
        }
        if (transpose - self.transpose).abs() > f32::EPSILON {
            self.stretch.set_transpose_factor(transpose, None);
            self.transpose = transpose;
        }
    }

    /// How long the stretcher holds the sound, in output seconds: what it
    /// takes in before its output starts, and its output buffer.
    fn delay_seconds(&self) -> f64 {
        self.stretch.input_latency() as f64 / (f64::from(self.input_rate) * self.rate)
            + self.stretch.output_latency() as f64 / f64::from(MIX_SAMPLE_RATE)
    }

    /// Seconds into the track of the next input frame.
    fn input_seconds(&self) -> f64 {
        self.source.next_frame() as f64 / f64::from(self.input_rate)
    }

    /// The position heard now, in seconds into the track.
    fn heard_seconds(&self) -> f64 {
        let input = self.input_seconds();
        if !self.has_rendered {
            return input;
        }
        let drained =
            self.tail_input_samples as f64 / self.channel_count as f64 / f64::from(self.input_rate);
        (input + drained - self.delay_seconds() * self.rate)
            .max(self.start_seconds)
            .min(input)
    }

    /// Mixes `frames` frames of this deck into `mix`, stereo, at `gain`.
    fn render(&mut self, mix: &mut [f32], frames: usize, gain: f32) {
        let mut silent = 0;
        if let Some(countdown) = self.start_countdown {
            if countdown >= frames as u64 {
                self.start_countdown = Some(countdown - frames as u64);
                return;
            }
            silent = countdown as usize;
            self.start_countdown = None;
        }
        let frames = frames - silent;
        if frames == 0 || self.is_finished {
            return;
        }
        self.update_transpose();
        let wanted = frames as f64 * self.speed() + self.input_fraction;
        let input_frames = wanted.floor() as usize;
        self.input_fraction = wanted - input_frames as f64;
        let channels = self.channel_count;
        self.input.clear();
        let draining_output = self.tail_input_samples >= self.stretch.input_latency() * channels;
        for _ in 0..input_frames * channels {
            match self.source.next() {
                Some(sample) => self.input.push(sample),
                None => {
                    self.tail_input_samples += 1;
                    self.input.push(0.0);
                }
            }
        }
        self.output.clear();
        self.output.resize(frames * channels, 0.0);
        self.stretch.process(&self.input, &mut self.output);
        self.has_rendered = true;
        // Input ending leaves sound in both halves of the stretcher. Keep
        // feeding silence until that sound has reached the output.
        if draining_output {
            self.tail_output_frames += frames;
        }
        self.is_finished =
            self.tail_input_samples > 0 && self.tail_output_frames >= self.stretch.output_latency();
        for frame in 0..frames {
            let left = self.output[frame * channels];
            let right = if channels > 1 {
                self.output[frame * channels + 1]
            } else {
                left
            };
            let at = (silent + frame) * 2;
            mix[at] += left * gain;
            mix[at + 1] += right * gain;
        }
    }
}

/// The source the audio output plays: both decks, mixed.
pub(crate) struct DeckMix {
    commands: Receiver<DeckCommand>,
    meters: [Arc<DeckMeter>; 2],
    decks: [Option<LoadedTrack>; 2],
    block: Vec<f32>,
    next_sample: usize,
}

/// A new mix with nothing on its decks, and the window's end of it.
pub(crate) fn deck_mix() -> (DeckLink, DeckMix) {
    let (commands, received) = channel();
    let meters = [
        Arc::new(DeckMeter::default()),
        Arc::new(DeckMeter::default()),
    ];
    for meter in &meters {
        meter.set_gain(1.0);
    }
    let link = DeckLink {
        commands,
        meters: meters.clone(),
    };
    let mix = DeckMix {
        commands: received,
        meters,
        decks: [None, None],
        block: Vec::new(),
        next_sample: 0,
    };
    (link, mix)
}

impl DeckMix {
    fn deck_mut(&mut self, deck: DeckId) -> Option<&mut LoadedTrack> {
        self.decks[deck.index()].as_mut()
    }

    fn apply(&mut self, command: DeckCommand) {
        match command {
            DeckCommand::Load { deck, source, grid } => {
                self.decks[deck.index()] = Some(LoadedTrack::new(*source, grid));
                let meter = &self.meters[deck.index()];
                meter.finished.store(false, Ordering::Relaxed);
                *meter.problem.lock().unwrap_or_else(PoisonError::into_inner) = None;
                meter.pending_load.store(false, Ordering::Release);
            }
            DeckCommand::Play { deck, quantize_to } => {
                let countdown =
                    quantize_to.and_then(|other| self.frames_until_downbeat(deck, other));
                if let Some(track) = self.deck_mut(deck) {
                    track.playing = true;
                    track.start_countdown = countdown;
                }
            }
            DeckCommand::Pause(deck) => {
                if let Some(track) = self.deck_mut(deck) {
                    track.playing = false;
                    track.start_countdown = None;
                }
            }
            DeckCommand::Seek { deck, seconds } => {
                if let Some(track) = self.deck_mut(deck) {
                    track.source.state().seek_to(seconds as f32);
                    track.stretch.reset();
                    track.input_fraction = 0.0;
                    track.is_finished = false;
                    track.tail_input_samples = 0;
                    track.tail_output_frames = 0;
                    track.has_rendered = false;
                    track.start_seconds = track.input_seconds();
                }
            }
            DeckCommand::SetGrid { deck, grid } => {
                if let Some(track) = self.deck_mut(deck) {
                    track.grid = grid;
                }
            }
            DeckCommand::SetRate { deck, rate } => {
                if let Some(track) = self.deck_mut(deck) {
                    track.rate = rate.clamp(MIN_RATE, MAX_RATE);
                }
            }
            DeckCommand::SetKeylock { deck, keylock } => {
                if let Some(track) = self.deck_mut(deck) {
                    track.keylock = keylock;
                }
            }
        }
    }

    /// Output frames from now until `deck`, started now from where it
    /// stands, would sound its position at the moment `other` sounds its
    /// next downbeat. `None` when `other` is not playing or either deck
    /// has no grid, and the deck then starts at once.
    fn frames_until_downbeat(&self, deck: DeckId, other: DeckId) -> Option<u64> {
        let starting = self.decks[deck.index()].as_ref()?;
        let leading = self.decks[other.index()].as_ref()?;
        if !leading.playing || leading.start_countdown.is_some() {
            return None;
        }
        let starting_grid = starting.grid?;
        let grid = leading.grid?;
        let input_now = leading.input_seconds();
        // The other deck's input reaches its next downbeat after this many
        // output seconds, and sounds it its own delay later. This deck,
        // started then, sounds its first frame its own delay later.
        let downbeat = grid.next_downbeat(input_now);
        let until_input = (downbeat - input_now) / leading.rate;
        let held = starting.heard_seconds();
        let until_own_downbeat = (starting_grid.next_downbeat(held) - held) / starting.rate;
        let startup_delay = if starting.has_rendered {
            0.0
        } else {
            starting.delay_seconds()
        };
        let start = until_input + leading.delay_seconds() - until_own_downbeat - startup_delay;
        let bar = grid.bar_seconds() / leading.rate;
        let start = start + (-start / bar).ceil().max(0.0) * bar;
        Some((start * f64::from(MIX_SAMPLE_RATE)).round() as u64)
    }

    fn render_block(&mut self) {
        while let Ok(command) = self.commands.try_recv() {
            self.apply(command);
        }
        self.block.clear();
        self.block.resize(BLOCK_FRAMES * 2, 0.0);
        for deck in DeckId::BOTH {
            let meter = Arc::clone(&self.meters[deck.index()]);
            let Some(track) = self.decks[deck.index()].as_mut() else {
                meter.playing.store(false, Ordering::Relaxed);
                continue;
            };
            if track.playing {
                let began = Instant::now();
                let starved_before = track.source.state().silent_frames();
                track.render(&mut self.block, BLOCK_FRAMES, meter.gain());
                meter.busy_nanoseconds.fetch_add(
                    u64::try_from(began.elapsed().as_nanos()).unwrap_or(u64::MAX),
                    Ordering::Relaxed,
                );
                meter
                    .mixed_frames
                    .fetch_add(BLOCK_FRAMES as u64, Ordering::Relaxed);
                let starved = track.source.state().silent_frames() - starved_before;
                meter.starved_frames.fetch_add(starved, Ordering::Relaxed);
                if track.is_finished {
                    track.playing = false;
                }
            }
            meter
                .position_bits
                .store(track.heard_seconds().to_bits(), Ordering::Relaxed);
            if track.is_finished && !meter.finished.load(Ordering::Relaxed) {
                *meter.problem.lock().unwrap_or_else(PoisonError::into_inner) =
                    track.source.state().problem();
            }
            meter.finished.store(track.is_finished, Ordering::Relaxed);
            meter.playing.store(track.playing, Ordering::Relaxed);
            meter
                .waiting
                .store(track.start_countdown.is_some(), Ordering::Relaxed);
        }
        self.next_sample = 0;
    }
}

impl Iterator for DeckMix {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.next_sample >= self.block.len() {
            self.render_block();
        }
        let sample = self.block[self.next_sample];
        self.next_sample += 1;
        Some(sample)
    }
}

impl rodio::Source for DeckMix {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> NonZeroU16 {
        NonZeroU16::new(2).unwrap_or(NonZeroU16::MIN)
    }

    fn sample_rate(&self) -> NonZeroU32 {
        NonZeroU32::new(MIX_SAMPLE_RATE).unwrap_or(NonZeroU32::MIN)
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    /// A folder under the system's temporary folder, deleted when dropped.
    struct ScratchFolder(PathBuf);

    impl ScratchFolder {
        fn new(name: &str) -> Self {
            let folder =
                std::env::temp_dir().join(format!("euphoria-decks-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&folder);
            std::fs::create_dir_all(&folder).unwrap();
            Self(folder)
        }
    }

    impl Drop for ScratchFolder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Writes a mono 16 bit WAV file of `seconds` of clicks at `bpm`, the
    /// first on `first_downbeat`, louder on each downbeat.
    fn write_click_track(path: &Path, bpm: f64, first_downbeat: f64, seconds: f64) {
        let rate = 44_100_u32;
        let frames = (seconds * f64::from(rate)) as usize;
        let mut samples = vec![0_i16; frames];
        let beat = 60.0 / bpm;
        let mut number = 0;
        loop {
            let at = first_downbeat + number as f64 * beat;
            if at >= seconds {
                break;
            }
            let level = if number % 4 == 0 { 0.9 } else { 0.4 };
            let start = (at * f64::from(rate)) as usize;
            for offset in 0..(0.004 * f64::from(rate)) as usize {
                let Some(sample) = samples.get_mut(start + offset) else {
                    break;
                };
                let time = offset as f64 / f64::from(rate);
                let envelope = (-time / 0.0015).exp();
                *sample = (level
                    * envelope
                    * (std::f64::consts::TAU * 2_000.0 * time).sin()
                    * f64::from(i16::MAX)) as i16;
            }
            number += 1;
        }
        let data_length = (samples.len() * 2) as u32;
        let mut bytes = Vec::new();
        bytes.extend(b"RIFF");
        bytes.extend((36 + data_length).to_le_bytes());
        bytes.extend(b"WAVEfmt ");
        bytes.extend(16_u32.to_le_bytes());
        bytes.extend(1_u16.to_le_bytes());
        bytes.extend(1_u16.to_le_bytes());
        bytes.extend(rate.to_le_bytes());
        bytes.extend((rate * 2).to_le_bytes());
        bytes.extend(2_u16.to_le_bytes());
        bytes.extend(16_u16.to_le_bytes());
        bytes.extend(b"data");
        bytes.extend(data_length.to_le_bytes());
        for sample in samples {
            bytes.extend(sample.to_le_bytes());
        }
        std::fs::write(path, bytes).unwrap();
    }

    fn pull(mix: &mut DeckMix, seconds: f64) -> Vec<f32> {
        let samples = (seconds * f64::from(MIX_SAMPLE_RATE)) as usize * 2;
        (0..samples).map(|_| mix.next().unwrap()).collect()
    }

    /// The start of each click in a stereo mix, in seconds, with whether
    /// it is loud, a downbeat.
    fn onsets(samples: &[f32]) -> Vec<(f64, bool)> {
        let mut found = Vec::new();
        let mut quiet_until = 0;
        for (frame, pair) in samples.chunks(2).enumerate() {
            let level = pair[0].abs();
            if frame >= quiet_until && level > 0.1 {
                // The loudest point of the next few milliseconds tells a
                // downbeat from a beat.
                let window = &samples[frame * 2..(frame * 2 + 2 * 441).min(samples.len())];
                let peak = window
                    .iter()
                    .fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
                found.push((frame as f64 / f64::from(MIX_SAMPLE_RATE), peak > 0.6));
                quiet_until = frame + (0.15 * f64::from(MIX_SAMPLE_RATE)) as usize;
            }
        }
        found
    }

    #[derive(Debug)]
    struct Alignment {
        mean_milliseconds: f64,
        largest_milliseconds: f64,
        downbeats_matched: usize,
        beats: usize,
    }

    /// Plays a click track at `a_bpm` on deck A, then starts one at `b_bpm`
    /// on deck B on its fourth downbeat, matched to A's tempo and started on
    /// A's next downbeat, and measures how far B's clicks land from A's.
    fn measure_alignment(a_bpm: f64, b_bpm: f64, b_start: Option<f64>) -> Alignment {
        let scratch = ScratchFolder::new(&format!("click-{a_bpm}-{b_bpm}"));
        let a_path = scratch.0.join("a.wav");
        let b_path = scratch.0.join("b.wav");
        let a_grid = GridTiming {
            first_downbeat_seconds: 0.5,
            bpm: a_bpm,
        };
        let b_grid = GridTiming {
            first_downbeat_seconds: 0.25,
            bpm: b_bpm,
        };
        write_click_track(&a_path, a_bpm, 0.5, 40.0);
        write_click_track(&b_path, b_bpm, 0.25, 40.0);

        let run = |a_gain: f32, b_gain: f32| {
            let (link, mut mix) = deck_mix();
            for (deck, path, grid) in [(DeckId::A, &a_path, a_grid), (DeckId::B, &b_path, b_grid)] {
                let source = TrackSource::open(path, 0.0).unwrap().patient();
                link.send(DeckCommand::Load {
                    deck,
                    source: Box::new(source),
                    grid: Some(grid),
                });
            }
            link.meter(DeckId::A).set_gain(a_gain);
            link.meter(DeckId::B).set_gain(b_gain);
            link.send(DeckCommand::SetRate {
                deck: DeckId::B,
                rate: matching_rate(b_bpm, a_bpm),
            });
            link.send(DeckCommand::Seek {
                deck: DeckId::B,
                seconds: b_start
                    .unwrap_or(b_grid.first_downbeat_seconds + 3.0 * b_grid.bar_seconds()),
            });
            link.send(DeckCommand::Play {
                deck: DeckId::A,
                quantize_to: None,
            });
            let mut samples = pull(&mut mix, 3.3);
            link.send(DeckCommand::Play {
                deck: DeckId::B,
                quantize_to: Some(DeckId::A),
            });
            samples.extend(pull(&mut mix, 16.0));
            samples
        };
        let a_onsets = onsets(&run(1.0, 0.0));
        let b_onsets = onsets(&run(0.0, 1.0));
        assert!(b_onsets.len() > 20, "{}", b_onsets.len());

        let mut errors = Vec::new();
        let mut downbeats_matched = 0;
        for (at, is_downbeat) in &b_onsets {
            let (nearest, nearest_is_downbeat) = a_onsets
                .iter()
                .min_by(|first, second| (first.0 - at).abs().total_cmp(&(second.0 - at).abs()))
                .copied()
                .unwrap();
            errors.push((at - nearest).abs() * 1_000.0);
            if *is_downbeat && nearest_is_downbeat {
                downbeats_matched += 1;
            }
        }
        Alignment {
            mean_milliseconds: errors.iter().sum::<f64>() / errors.len() as f64,
            largest_milliseconds: errors.iter().copied().fold(0.0, f64::max),
            downbeats_matched,
            beats: errors.len(),
        }
    }

    #[test]
    fn a_rate_matches_the_tempo_reading_half_or_double_when_closer() {
        assert!((matching_rate(125.0, 120.0) - 0.96).abs() < 1e-9);
        assert!((matching_rate(64.0, 128.0) - 1.0).abs() < 1e-9);
        assert!((matching_rate(174.0, 87.0) - 1.0).abs() < 1e-9);
        assert_eq!(matching_rate(0.0, 120.0), 1.0);
        assert_eq!(matching_rate(50.0, 400.0), MAX_RATE);
        assert_eq!(matching_rate(400.0, 50.0), MIN_RATE);
    }

    #[test]
    fn downbeats_come_on_the_bar() {
        let grid = GridTiming {
            first_downbeat_seconds: 0.5,
            bpm: 120.0,
        };
        assert_eq!(grid.bar_seconds(), 2.0);
        assert_eq!(grid.next_downbeat(0.2), 0.5);
        assert_eq!(grid.next_downbeat(3.0), 4.5);
    }

    #[test]
    fn a_deck_started_on_the_other_decks_downbeat_stays_on_its_beats() {
        for (a_bpm, b_bpm) in [(120.0, 125.0), (128.0, 122.0)] {
            let alignment = measure_alignment(a_bpm, b_bpm, None);
            eprintln!(
                "A at {a_bpm} BPM, B at {b_bpm} BPM matched to it: {} beats of B, mean {:.2} ms \
                 and largest {:.2} ms from A's, {} downbeats on A's downbeats",
                alignment.beats,
                alignment.mean_milliseconds,
                alignment.largest_milliseconds,
                alignment.downbeats_matched
            );
            assert!(alignment.mean_milliseconds < 5.0, "{a_bpm} {b_bpm}");
            assert!(alignment.largest_milliseconds < 10.0, "{a_bpm} {b_bpm}");
            assert!(
                alignment.downbeats_matched * 4 >= alignment.beats - 4,
                "B's downbeats land on A's"
            );
        }
    }

    #[test]
    fn starting_between_beats_still_aligns_the_tracks_bars() {
        let alignment = measure_alignment(120.0, 125.0, Some(0.25 + 0.5 * 60.0 / 125.0));
        assert!(alignment.mean_milliseconds < 5.0, "{alignment:?}");
        assert!(alignment.largest_milliseconds < 10.0, "{alignment:?}");
        assert!(
            alignment.downbeats_matched * 4 >= alignment.beats - 4,
            "{alignment:?}"
        );
    }

    #[test]
    fn a_paused_deck_is_silent_and_holds_its_place() {
        let scratch = ScratchFolder::new("pause");
        let path = scratch.0.join("a.wav");
        write_click_track(&path, 120.0, 0.0, 8.0);
        let (link, mut mix) = deck_mix();
        link.send(DeckCommand::Load {
            deck: DeckId::A,
            source: Box::new(TrackSource::open(&path, 0.0).unwrap().patient()),
            grid: None,
        });
        link.send(DeckCommand::Play {
            deck: DeckId::A,
            quantize_to: None,
        });
        pull(&mut mix, 1.0);
        link.send(DeckCommand::Pause(DeckId::A));
        pull(&mut mix, 0.01);
        let held = link.meter(DeckId::A).position_seconds();
        let paused = pull(&mut mix, 1.0);
        assert!(paused.iter().all(|sample| *sample == 0.0));
        assert_eq!(link.meter(DeckId::A).position_seconds(), held);
        assert!(!link.meter(DeckId::A).is_playing());
        assert!((0.8..1.1).contains(&held), "{held}");
    }

    #[test]
    fn pausing_and_resuming_preserves_buffered_audio_and_the_heard_position() {
        let scratch = ScratchFolder::new("pause-continuity");
        let path = scratch.0.join("track.wav");
        write_click_track(&path, 120.0, 0.1, 5.0);
        let run = |pause: bool| {
            let (link, mut mix) = deck_mix();
            mix.apply(DeckCommand::Load {
                deck: DeckId::A,
                source: Box::new(TrackSource::open(&path, 0.0).unwrap().patient()),
                grid: None,
            });
            mix.apply(DeckCommand::Play {
                deck: DeckId::A,
                quantize_to: None,
            });
            for _ in 0..64 {
                mix.render_block();
            }
            let heard = link.meter(DeckId::A).position_seconds();
            if pause {
                mix.apply(DeckCommand::Pause(DeckId::A));
                mix.render_block();
                assert_eq!(link.meter(DeckId::A).position_seconds(), heard);
                assert!(mix.block.iter().all(|sample| *sample == 0.0));
                mix.apply(DeckCommand::Play {
                    deck: DeckId::A,
                    quantize_to: None,
                });
            }
            let mut resumed = Vec::new();
            for _ in 0..128 {
                mix.render_block();
                resumed.extend_from_slice(&mix.block);
            }
            resumed
        };
        assert_eq!(run(true), run(false));
    }

    #[test]
    fn the_last_onset_is_heard_before_the_deck_finishes() {
        let scratch = ScratchFolder::new("ending-tail");
        let path = scratch.0.join("track.wav");
        write_click_track(&path, 120.0, 0.98, 1.0);
        let (link, mut mix) = deck_mix();
        mix.apply(DeckCommand::Load {
            deck: DeckId::A,
            source: Box::new(TrackSource::open(&path, 0.0).unwrap().patient()),
            grid: None,
        });
        mix.apply(DeckCommand::Play {
            deck: DeckId::A,
            quantize_to: None,
        });
        let output = pull(&mut mix, 2.0);
        let peak = output.iter().copied().map(f32::abs).fold(0.0, f32::max);
        assert!(peak > 0.05, "final onset was cut off: peak {peak}");
        assert!(!link.meter(DeckId::A).is_playing());
        assert!(link.meter(DeckId::A).is_finished());
        assert_eq!(link.meter(DeckId::A).problem(), None);
        link.send(DeckCommand::Load {
            deck: DeckId::A,
            source: Box::new(TrackSource::open(&path, 0.0).unwrap().patient()),
            grid: None,
        });
        assert!(!link.meter(DeckId::A).is_finished());
        mix.render_block();
        assert!(!link.meter(DeckId::A).is_finished());
    }

    #[test]
    fn rate_changes_while_draining_keep_the_last_onset() {
        let scratch = ScratchFolder::new("ending-rate-change");
        let path = scratch.0.join("track.wav");
        write_click_track(&path, 120.0, 0.98, 1.0);
        let (link, mut mix) = deck_mix();
        mix.apply(DeckCommand::Load {
            deck: DeckId::A,
            source: Box::new(TrackSource::open(&path, 0.0).unwrap().patient()),
            grid: None,
        });
        mix.apply(DeckCommand::SetRate {
            deck: DeckId::A,
            rate: 2.0,
        });
        mix.apply(DeckCommand::Play {
            deck: DeckId::A,
            quantize_to: None,
        });
        let mut peak = 0.0_f32;
        for _ in 0..1000 {
            mix.render_block();
            peak = mix.block.iter().copied().map(f32::abs).fold(peak, f32::max);
            if mix.decks[0].as_ref().unwrap().tail_input_samples >= 3072 {
                break;
            }
        }
        assert!(mix.decks[0].as_ref().unwrap().tail_input_samples >= 3072);
        mix.apply(DeckCommand::SetRate {
            deck: DeckId::A,
            rate: 0.5,
        });
        for _ in 0..200 {
            mix.render_block();
            peak = mix.block.iter().copied().map(f32::abs).fold(peak, f32::max);
        }
        assert!(
            peak > 0.05,
            "final onset was cut off after rate change: peak {peak}"
        );
        assert!(link.meter(DeckId::A).is_finished());
    }

    /// Real tracks for the load measurement: benchmark previews, or the
    /// files `EUPHORIA_DECK_TRACKS` names, separated by colons.
    fn measurement_tracks() -> Option<(PathBuf, PathBuf)> {
        if let Ok(named) = std::env::var("EUPHORIA_DECK_TRACKS") {
            let mut paths = named.split(':').map(PathBuf::from);
            return Some((paths.next()?, paths.next()?));
        }
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/data/key/audio");
        let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
            .ok()?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|extension| extension == "mp3"))
            .collect();
        files.sort();
        Some((files.first()?.clone(), files.get(1)?.clone()))
    }

    /// Both decks with real tracks, B time stretched to 96 percent, playing.
    fn two_playing_decks(a: &Path, b: &Path, patient: bool) -> (DeckLink, DeckMix) {
        let (link, mix) = deck_mix();
        for (deck, path) in [(DeckId::A, a), (DeckId::B, b)] {
            let source = TrackSource::open(path, 0.0).unwrap();
            let source = if patient { source.patient() } else { source };
            link.send(DeckCommand::Load {
                deck,
                source: Box::new(source),
                grid: None,
            });
            link.send(DeckCommand::Play {
                deck,
                quantize_to: None,
            });
        }
        link.send(DeckCommand::SetRate {
            deck: DeckId::B,
            rate: 0.96,
        });
        (link, mix)
    }

    /// Analyses running on every core but two, as when a folder is being
    /// analyzed while the decks play.
    fn busy_analysis(
        tracks: [PathBuf; 2],
    ) -> (Arc<AtomicBool>, Vec<std::thread::JoinHandle<usize>>) {
        let stop = Arc::new(AtomicBool::new(false));
        let workers = std::thread::available_parallelism()
            .map_or(2, std::num::NonZeroUsize::get)
            .saturating_sub(2)
            .max(1);
        let handles = (0..workers)
            .map(|worker| {
                let stop = Arc::clone(&stop);
                let path = tracks[worker % 2].clone();
                std::thread::spawn(move || {
                    let mut analyzed = 0;
                    while !stop.load(Ordering::Relaxed) {
                        let _ = euphoria_core::analyze_file(&path);
                        analyzed += 1;
                    }
                    analyzed
                })
            })
            .collect();
        (stop, handles)
    }

    /// Run with `cargo test --release -p euphoria -- --ignored --nocapture
    /// two_decks_keep_up`. Mixes two real tracks, one time stretched, as
    /// fast as it can for the load, then in real time for starved frames,
    /// each without and with analysis on every other core.
    #[test]
    #[ignore = "a measurement on real tracks, run by hand in release"]
    fn two_decks_keep_up() {
        let Some((a, b)) = measurement_tracks() else {
            eprintln!("no tracks to measure with");
            return;
        };
        for with_analysis in [false, true] {
            let analysis = with_analysis.then(|| busy_analysis([a.clone(), b.clone()]));
            let (link, mut mix) = two_playing_decks(&a, &b, true);
            let began = Instant::now();
            let seconds = 60.0;
            pull(&mut mix, seconds);
            let wall = began.elapsed().as_secs_f64();
            eprintln!(
                "{} analysis: {seconds} s of two decks mixed in {:.2} s, {:.1}% of one core; \
                 deck A {:.1}%, deck B (stretched to 96%) {:.1}%",
                if with_analysis { "with" } else { "without" },
                wall,
                wall / seconds * 100.0,
                link.meter(DeckId::A).load() * 100.0,
                link.meter(DeckId::B).load() * 100.0
            );

            let (link, mut mix) = two_playing_decks(&a, &b, false);
            let block = BLOCK_FRAMES * 2;
            let real_seconds = 20.0;
            let blocks = (real_seconds * f64::from(MIX_SAMPLE_RATE)) as usize / BLOCK_FRAMES;
            let started = Instant::now();
            let mut late_blocks = 0;
            for number in 0..blocks {
                let due = Duration::from_secs_f64(
                    (number * BLOCK_FRAMES) as f64 / f64::from(MIX_SAMPLE_RATE),
                );
                if let Some(wait) = due.checked_sub(started.elapsed()) {
                    std::thread::sleep(wait);
                } else if started.elapsed() - due > Duration::from_millis(6) {
                    late_blocks += 1;
                }
                for _ in 0..block {
                    mix.next();
                }
            }
            eprintln!(
                "{} analysis, in real time for {real_seconds} s: deck A starved {} frames, deck B \
                 {} frames; {late_blocks} blocks over 6 ms late",
                if with_analysis { "with" } else { "without" },
                link.meter(DeckId::A).starved_frames(),
                link.meter(DeckId::B).starved_frames()
            );
            if let Some((stop, handles)) = analysis {
                stop.store(true, Ordering::Relaxed);
                let analyzed: usize = handles
                    .into_iter()
                    .map(|handle| handle.join().unwrap())
                    .sum();
                eprintln!("  ({analyzed} analyses ran alongside)");
            }
        }
    }
}
