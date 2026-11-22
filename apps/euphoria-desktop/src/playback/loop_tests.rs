// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Loops played through the same path as the rest of a track, against
//! files written here: a ramp whose every sample says which frame it is,
//! so a loop's length and its joins can be counted to the frame, and a
//! steady tone, whose joins must neither click nor fall silent.

use std::f64::consts::TAU;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use euphoria_core::stems::{Stem, StemFiles};

use std::collections::VecDeque;
use std::sync::mpsc::sync_channel;

use super::{
    Decoded, Decoder, LOOP_CROSSFADE_SECONDS, PlaybackTempo, Reader, Shift, StemMix, TrackSource,
    VoiceState,
};

const SAMPLE_RATE: u32 = 44_100;

/// The ramp moves this much a frame, small enough for every frame of a
/// few seconds to have its own value in a 32 bit float.
const RAMP_STEP: f64 = 1.0e-6;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "euphoria-loop-playback-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        Self(directory)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Writes 32 bit float samples as a WAV file, so the ramp keeps every
/// frame's value exactly.
fn write_float_wav(path: &Path, interleaved: &[f32], channels: u16) {
    let data_bytes = (interleaved.len() * 4) as u32;
    let mut writer = BufWriter::new(File::create(path).unwrap());
    writer.write_all(b"RIFF").unwrap();
    writer.write_all(&(36 + data_bytes).to_le_bytes()).unwrap();
    writer.write_all(b"WAVEfmt ").unwrap();
    writer.write_all(&16_u32.to_le_bytes()).unwrap();
    writer.write_all(&3_u16.to_le_bytes()).unwrap();
    writer.write_all(&channels.to_le_bytes()).unwrap();
    writer.write_all(&SAMPLE_RATE.to_le_bytes()).unwrap();
    writer
        .write_all(&(SAMPLE_RATE * u32::from(channels) * 4).to_le_bytes())
        .unwrap();
    writer.write_all(&(channels * 4).to_le_bytes()).unwrap();
    writer.write_all(&32_u16.to_le_bytes()).unwrap();
    writer.write_all(b"data").unwrap();
    writer.write_all(&data_bytes.to_le_bytes()).unwrap();
    for sample in interleaved {
        writer.write_all(&sample.to_le_bytes()).unwrap();
    }
    writer.flush().unwrap();
}

fn ramp(seconds: f64) -> Vec<f32> {
    let frames = (seconds * f64::from(SAMPLE_RATE)) as usize;
    (0..frames)
        .map(|frame| (frame as f64 * RAMP_STEP) as f32)
        .collect()
}

fn tone(seconds: f64, hertz: f64) -> Vec<f32> {
    let frames = (seconds * f64::from(SAMPLE_RATE)) as usize;
    (0..frames)
        .map(|frame| ((TAU * hertz * frame as f64 / f64::from(SAMPLE_RATE)).sin() * 0.5) as f32)
        .collect()
}

/// The frame a ramp sample stands for.
fn frame_of(sample: f32) -> i64 {
    (f64::from(sample) / RAMP_STEP).round() as i64
}

fn frames(seconds: f64) -> i64 {
    (seconds * f64::from(SAMPLE_RATE)).round() as i64
}

/// Samples long enough for any fade around a jump to be over.
const DECLICK_SAMPLES: usize = 400;

fn crossfade_frames() -> i64 {
    frames(LOOP_CROSSFADE_SECONDS)
}

/// The next `count` samples the output would get, waiting for the decoder
/// rather than counting silence, and failing if it ever fell behind.
fn next_sound(source: &mut TrackSource, count: usize) -> Vec<f32> {
    let state = source.state();
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut samples = Vec::with_capacity(count);
    while samples.len() < count {
        assert!(Instant::now() < deadline, "the decoder fell silent");
        let Some(sample) = source.next() else { break };
        samples.push(sample);
    }
    assert_eq!(
        state.silent_frames(),
        0,
        "a patient source never goes silent"
    );
    samples
}

/// Checks that `samples` of the ramp run `from` on frame by frame, through
/// passes of the loop `start..end` once they reach its end, joined by the
/// crossfade. Returns how many passes began.
fn assert_loops_exactly(samples: &[f32], from: i64, start: i64, end: i64) -> usize {
    let length = end - start;
    let mut expected = from;
    let mut since_join = i64::MAX;
    let mut passes = 0;
    for (index, sample) in samples.iter().enumerate() {
        if expected == end {
            expected = start;
            since_join = 0;
            passes += 1;
        }
        if since_join >= crossfade_frames() {
            assert_eq!(
                frame_of(*sample),
                expected,
                "sample {index}: the loop {start}..{end} is {length} frames, joined exactly"
            );
        }
        expected += 1;
        since_join = since_join.saturating_add(1);
    }
    passes
}

#[test]
fn a_loop_repeats_to_the_frame_with_no_gap_and_is_left_where_it_ends() {
    let scratch = Scratch::new("ramp");
    let path = scratch.file("ramp.wav");
    write_float_wav(&path, &ramp(4.0), 1);
    let mut source = TrackSource::open(&path, 0.0).unwrap().patient();
    let state = source.state();
    let _ = next_sound(&mut source, 1_000);
    let start = frames(0.25);
    let end = frames(0.25) + 10_007;
    state.set_loop(Some((start as f64 / 44_100.0, end as f64 / 44_100.0)), None);
    let heard = next_sound(&mut source, 60_000);
    let settled = &heard[DECLICK_SAMPLES..];
    assert_eq!(
        frame_of(settled[0]),
        1_000 + DECLICK_SAMPLES as i64,
        "setting the loop keeps the sound where it was"
    );
    let passes = assert_loops_exactly(settled, frame_of(settled[0]), start, end);
    assert!(passes >= 4, "{passes} passes");
    let position = f64::from(state.position_seconds());
    assert!(
        (0.25..=end as f64 / 44_100.0).contains(&position),
        "the playhead stays in the loop: {position}"
    );

    state.set_loop(None, None);
    let after = next_sound(&mut source, 60_000);
    let last = frame_of(*after.last().unwrap());
    assert!(
        last > end + 30_000,
        "left, the track plays on past the loop's end: {last}"
    );
    assert!(f64::from(state.position_seconds()) > end as f64 / 44_100.0);
}

#[test]
fn the_join_of_a_loop_never_clicks_and_never_falls_silent() {
    let scratch = Scratch::new("tone");
    let path = scratch.file("tone.wav");
    write_float_wav(&path, &tone(4.0, 440.0), 1);
    let mut source = TrackSource::open(&path, 0.0).unwrap().patient();
    // A length that is not a whole number of cycles, so the join meets the
    // tone at another phase.
    source.state().set_loop(Some((0.5, 0.5 + 0.3013)), None);
    let heard = next_sound(&mut source, 44_100 * 3);
    let steepest_tone_step = 0.5 * TAU * 440.0 / 44_100.0;
    let largest_step = heard
        .windows(2)
        .map(|pair| f64::from((pair[1] - pair[0]).abs()))
        .fold(0.0, f64::max);
    assert!(
        largest_step < steepest_tone_step * 1.5,
        "no step larger than the tone's own: {largest_step} against {steepest_tone_step}"
    );
    let block = 441;
    let quietest = heard
        .chunks(block)
        .map(|chunk| {
            chunk
                .iter()
                .map(|sample| sample.abs())
                .fold(0.0_f32, f32::max)
        })
        .fold(f32::MAX, f32::min);
    assert!(quietest > 0.2, "every 10 ms still sounds: {quietest}");
}

#[test]
fn halving_a_loop_past_its_new_end_jumps_back_into_it_and_doubling_carries_on() {
    let scratch = Scratch::new("halve");
    let path = scratch.file("ramp.wav");
    write_float_wav(&path, &ramp(6.0), 1);
    let mut source = TrackSource::open(&path, 0.0).unwrap().patient();
    let state = source.state();
    state.set_loop(Some((0.0, 2.0)), None);
    let _ = next_sound(&mut source, frames(1.5) as usize);
    state.set_loop(Some((0.0, 1.0)), None);
    let halved = next_sound(&mut source, 120_000);
    assert!(
        halved[2_000..]
            .iter()
            .all(|sample| frame_of(*sample) <= frames(1.0) + crossfade_frames()),
        "after the jump every frame is in the halved loop"
    );
    state.set_loop(Some((0.0, 2.0)), None);
    let doubled = next_sound(&mut source, 120_000);
    assert!(
        doubled
            .iter()
            .any(|sample| frame_of(*sample) > frames(1.0) + 100),
        "doubled, the loop reaches its new end"
    );
}

#[test]
fn a_loop_plays_seamlessly_with_the_tempo_changed_and_key_lock_on() {
    let scratch = Scratch::new("tempo");
    let path = scratch.file("tone.wav");
    write_float_wav(&path, &tone(6.0, 440.0), 1);
    let mut source = TrackSource::open(&path, 0.0).unwrap().patient();
    let state = source.state();
    state.set_tempo(PlaybackTempo {
        rate: 1.1,
        key_lock: true,
    });
    state.set_loop(Some((0.5, 1.5)), None);
    let mut heard = Vec::new();
    let mut positions = Vec::new();
    for _ in 0..40 {
        heard.extend(next_sound(&mut source, 4_410));
        positions.push(f64::from(state.position_seconds()));
    }
    let settled = &heard[44_100 / 2..];
    let quietest = settled
        .chunks(441)
        .map(|chunk| {
            chunk
                .iter()
                .map(|sample| sample.abs())
                .fold(0.0_f32, f32::max)
        })
        .fold(f32::MAX, f32::min);
    assert!(
        quietest > 0.15,
        "stretched, every 10 ms still sounds: {quietest}"
    );
    assert!(
        positions[10..]
            .iter()
            .all(|position| (0.45..=1.55).contains(position)),
        "the playhead stays in the loop at the new tempo: {positions:?}"
    );
}

#[test]
fn the_stems_of_a_track_loop_together_to_the_frame() {
    let scratch = Scratch::new("stems");
    let directory = scratch.file("stems");
    std::fs::create_dir_all(&directory).unwrap();
    let files = StemFiles::in_directory(directory);
    let ramp = ramp(3.0);
    for (index, stem) in Stem::ALL.iter().enumerate() {
        let scaled: Vec<f32> = ramp
            .iter()
            .flat_map(|sample| {
                let value = sample * (index as f32 + 1.0);
                [value, value]
            })
            .collect();
        write_float_wav(&files.path(*stem), &scaled, 2);
    }
    let mix = Arc::new(StemMix::default());
    mix.set_audible(|stem| stem == Stem::Vocals);
    let mut source = TrackSource::open_stems(&files, mix, 0.0).unwrap().patient();
    let start = frames(0.5);
    let end = start + 8_821;
    source
        .state()
        .set_loop(Some((start as f64 / 44_100.0, end as f64 / 44_100.0)), None);
    let heard = next_sound(&mut source, 160_000);
    let vocals_index = Stem::ALL
        .iter()
        .position(|stem| *stem == Stem::Vocals)
        .unwrap() as f32;
    let left: Vec<f32> = heard
        .chunks(2)
        .skip(1_000)
        .map(|frame| frame[0] / (vocals_index + 1.0))
        .collect();
    let passes = assert_loops_exactly(&left, 1_000, start, end);
    assert!(passes >= 3, "{passes} passes");
}

#[test]
fn a_loop_changed_while_the_decoder_reads_drops_the_chunk_decoded_under_the_old_region() {
    // The decoder reads the chunk that crosses the old loop's end, and the
    // loop is doubled before the read finishes: set_loop then has nothing
    // decoded under the old loop it can see (the decoder has not published
    // yet), so it decides no recovery seek is needed, and only the chunk's
    // own generation check can tell its audio was decoded the old way.
    let scratch = Scratch::new("stale-region");
    let path = scratch.file("ramp.wav");
    write_float_wav(&path, &ramp(4.0), 1);

    let (decoded, _receiver) = sync_channel(4);
    let mut stream = euphoria_core::AudioStream::open(&path).unwrap();
    let start = 43_800;
    let reached = stream.seek_frame(start).unwrap();
    assert_eq!(reached, start);
    let state = VoiceState::for_tests(SAMPLE_RATE);
    let mut decoder = Decoder {
        stream: Reader::File(stream),
        state: Arc::clone(&state),
        decoded,
        seek_count: 0,
        next_frame: reached as f64,
        is_at_end: false,
        shift: Shift::new(1, 1, SAMPLE_RATE, 0, PlaybackTempo::default()).unwrap(),
        input_frame: reached,
        folds: VecDeque::new(),
        wraps: 0,
    };

    // The loop ends 300 frames after the read starts, so the chunk the
    // decoder reads next crosses its end and would wrap back to 0.
    state.set_loop(Some((0.0, 1.0)), None);
    // The seam lands after the decoder has read the loop and before its
    // file I/O: the loop doubles, as a window's set_loop would in a live
    // run, and no recovery seek follows, because the decoder has not
    // published the chunk it is reading.
    let hooked = Arc::clone(&state);
    state.note_next_decode(Some(Arc::new(move || {
        hooked.set_loop(Some((0.0, 2.0)), None);
    })));

    let decoded = decoder.decode_next();
    let Decoded::Chunk {
        start_frame,
        wraps,
        samples,
        ..
    } = decoded
    else {
        panic!("the chunk was not decoded under the new loop");
    };
    assert_eq!(start_frame, start);
    // The chunk reaches past the old loop's end, so reading it under the
    // old loop would have wrapped back to 0; it does not.
    let last = frame_of(*samples.last().unwrap());
    assert!(
        last > 44_100,
        "the chunk must cross the old loop's end: {last}"
    );
    assert_eq!(wraps, 0, "the wrap decoded under the old loop was counted");
    let start = start as i64;
    for (index, sample) in samples.iter().enumerate() {
        assert_eq!(
            frame_of(*sample),
            start + index as i64,
            "sample {index}: the old loop's wrap was published"
        );
    }
    // And the window sees the state the loop change asked for, not what the
    // discarded chunk published.
    let looping = state.lock_looping();
    assert_eq!(
        looping.decoded_wraps, 0,
        "the wrap decoded under the old loop was published to the window"
    );
    assert_eq!(
        looping.decoded_input_frame,
        start as u64 + samples.len() as u64
    );
    assert_eq!(looping.loop_generation, 2);
    assert_eq!(decoder.wraps, 0);
    assert_eq!(decoder.input_frame, start as u64 + samples.len() as u64);
}
