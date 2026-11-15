// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::f32::consts::TAU;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::key::{Key, Mode};

use super::*;

const SAMPLE_RATE: u32 = 44_100;

fn rate() -> NonZeroU32 {
    NonZeroU32::new(SAMPLE_RATE).unwrap()
}

fn noise(state: &mut u32) -> f32 {
    *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    (*state >> 8) as f32 / (1 << 24) as f32 * 2.0 - 1.0
}

pub(super) fn kick_pattern(bpm: f32, seconds: f32) -> Vec<f32> {
    let total = (seconds * SAMPLE_RATE as f32) as usize;
    let beat = (60.0 / bpm * SAMPLE_RATE as f32) as usize;
    let mut samples = vec![0.0; total];
    let mut state = 7;
    let mut start = 0;
    while start < total {
        let kick_length = (0.18 * SAMPLE_RATE as f32) as usize;
        let mut phase = 0.0;
        for offset in 0..kick_length.min(total - start) {
            let time = offset as f32 / SAMPLE_RATE as f32;
            let frequency = 50.0 + 110.0 * (-time * 30.0).exp();
            phase += TAU * frequency / SAMPLE_RATE as f32;
            samples[start + offset] += 0.8 * (-time * 14.0).exp() * phase.sin();
        }
        let hat_start = start + beat / 2;
        for offset in 0..(0.04 * SAMPLE_RATE as f32) as usize {
            if hat_start + offset < total {
                let decay = (-(offset as f32) / (0.01 * SAMPLE_RATE as f32)).exp();
                samples[hat_start + offset] += 0.25 * decay * noise(&mut state);
            }
        }
        start += beat;
    }
    samples
}

pub(super) fn chord(frequencies: &[f32], seconds: f32, gain: f32) -> Vec<f32> {
    let total = (seconds * SAMPLE_RATE as f32) as usize;
    (0..total)
        .map(|index| {
            let time = index as f32 / SAMPLE_RATE as f32;
            let mut value = 0.0;
            for frequency in frequencies {
                for harmonic in 1..=4 {
                    value += (TAU * frequency * harmonic as f32 * time).sin() / harmonic as f32;
                }
            }
            gain * value / frequencies.len() as f32
        })
        .collect()
}

pub(super) fn mix(first: &[f32], second: &[f32]) -> Vec<f32> {
    first
        .iter()
        .zip(second)
        .map(|(left, right)| left + right)
        .collect()
}

fn analyzer_after(samples: &[f32]) -> LiveAnalyzer {
    let mut analyzer = LiveAnalyzer::new(rate(), LiveSettings::default());
    for chunk in samples.chunks(441) {
        analyzer.push(chunk);
    }
    analyzer
}

#[test]
fn a_kick_pattern_gives_its_tempo_within_two_bpm() {
    for bpm in [100.0, 124.0, 140.0] {
        let analyzer = analyzer_after(&kick_pattern(bpm, 14.0));
        let reading = analyzer.analyze().unwrap();
        let tempo = reading.tempo.expect("a tempo");
        assert!((tempo.bpm - bpm).abs() <= 2.0, "{bpm}: {}", tempo.bpm);
        assert!(!reading.listening);
    }
}

fn steps_apart(first: Key, second: Key) -> u8 {
    let difference = (first.camelot_number() + 12 - second.camelot_number()) % 12;
    difference.min(12 - difference)
}

#[cfg(feature = "drone")]
#[test]
fn a_held_chord_gives_the_key_the_offline_detector_gives_and_one_that_mixes_with_it() {
    let a_minor = [110.0, 130.81, 164.81];
    let audio = chord(&a_minor, 12.0, 0.3);
    let analyzer = analyzer_after(&audio);
    let key = analyzer.analyze().unwrap().key.expect("a key").key;
    let offline = crate::detect::DroneDetector
        .read_key(&crate::audio::AudioBuffer::new(audio, rate()))
        .unwrap()
        .key
        .expect("a key");
    assert_eq!(key, offline);
    assert!(
        steps_apart(key, Key::new(9, Mode::Minor)) <= 1,
        "{key} for an A minor chord"
    );
}

#[test]
fn a_kick_pattern_over_a_chord_gives_both_within_seconds() {
    let kicks = kick_pattern(126.0, 12.0);
    let pad = chord(&[130.81, 164.81, 196.0], 12.0, 0.25);
    let analyzer = analyzer_after(&mix(&kicks, &pad));
    let reading = analyzer.analyze().unwrap();
    let key = reading.key.expect("a key").key;
    assert!(
        steps_apart(key, Key::new(0, Mode::Major)) <= 1,
        "{key} for a C major pad"
    );
    assert!((reading.tempo.expect("a tempo").bpm - 126.0).abs() <= 2.0);
    assert!(reading.energy.is_some());
}

#[test]
fn nothing_is_read_before_enough_audio_has_played() {
    let analyzer = analyzer_after(&kick_pattern(124.0, 3.0));
    let reading = analyzer.analyze().unwrap();
    assert!(reading.listening);
    assert!(reading.tempo.is_none() && reading.key.is_none());
    let analyzer = analyzer_after(&kick_pattern(124.0, 7.0));
    let reading = analyzer.analyze().unwrap();
    assert!(reading.tempo.is_some());
    assert!(reading.key.is_none());
}

#[test]
fn silence_and_very_quiet_audio_read_as_listening() {
    let silent = analyzer_after(&vec![0.0; SAMPLE_RATE as usize * 12]);
    let reading = silent.analyze().unwrap();
    assert!(reading.listening);
    assert!(reading.key.is_none() && reading.tempo.is_none() && reading.energy.is_none());

    let quiet: Vec<f32> = kick_pattern(124.0, 12.0)
        .iter()
        .map(|sample| sample * 0.0005)
        .collect();
    let reading = analyzer_after(&quiet).analyze().unwrap();
    assert!(reading.listening);
    assert!(reading.tempo.is_none());
}

#[test]
fn going_quiet_drops_the_old_readings_and_forgets_the_audio() {
    let mut samples = kick_pattern(124.0, 12.0);
    let mut analyzer = analyzer_after(&samples);
    assert!(analyzer.analyze().unwrap().tempo.is_some());
    samples.clear();
    for chunk in vec![0.0_f32; SAMPLE_RATE as usize * 3].chunks(441) {
        analyzer.push(chunk);
    }
    let reading = analyzer.analyze().unwrap();
    assert!(reading.listening);
    assert!(reading.tempo.is_none() && reading.key.is_none());
    assert!(analyzer.buffered_seconds() < 0.5);
    for chunk in kick_pattern(124.0, 3.0).chunks(441) {
        analyzer.push(chunk);
    }
    assert!(analyzer.analyze().unwrap().tempo.is_none());
}

#[test]
fn the_level_follows_the_audio() {
    let mut analyzer = LiveAnalyzer::new(rate(), LiveSettings::default());
    analyzer.push(&vec![0.5; SAMPLE_RATE as usize / 2]);
    let loud = analyzer.level().unwrap();
    assert!((loud.rms_db + 6.0).abs() < 0.1, "{}", loud.rms_db);
    assert!((loud.peak_db + 6.0).abs() < 0.1);
    analyzer.push(&vec![0.0; SAMPLE_RATE as usize / 2]);
    assert!(analyzer.level().unwrap().meter_position() < 0.01);
    assert!(loud.meter_position() > 0.8);
}

struct FakeCapture {
    audio: Vec<f32>,
    started_with: Arc<Mutex<Option<ListenSource>>>,
    stopped: Arc<Mutex<bool>>,
    refuse: Option<CaptureError>,
    sink: Option<AudioSink>,
}

impl FakeCapture {
    fn playing(audio: Vec<f32>) -> Self {
        Self {
            audio,
            started_with: Arc::default(),
            stopped: Arc::default(),
            refuse: None,
            sink: None,
        }
    }
}

impl AudioCapture for FakeCapture {
    fn sources(&self) -> Vec<ListenSource> {
        vec![ListenSource::AllAudio]
    }

    fn start(
        &mut self,
        source: &ListenSource,
        mut sink: AudioSink,
    ) -> Result<NonZeroU32, CaptureError> {
        if let Some(error) = &self.refuse {
            return Err(error.clone());
        }
        *self.started_with.lock().unwrap() = Some(source.clone());
        for chunk in self.audio.chunks(441) {
            sink(chunk);
        }
        self.sink = Some(sink);
        Ok(rate())
    }

    fn stop(&mut self) {
        self.sink = None;
        *self.stopped.lock().unwrap() = true;
    }
}

fn wait_for(engine: &LiveEngine, ready: impl Fn(&LiveReading) -> bool) -> LiveReading {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let reading = engine.latest();
        if ready(&reading) || Instant::now() > deadline {
            return reading;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn the_engine_reads_audio_from_a_capture_and_stops_it() {
    let capture = FakeCapture::playing(kick_pattern(128.0, 14.0));
    let started_with = Arc::clone(&capture.started_with);
    let stopped = Arc::clone(&capture.stopped);
    let source = ListenSource::Process {
        process_id: 42,
        name: "Player".into(),
    };
    let mut engine = LiveEngine::start(
        Box::new(capture),
        source.clone(),
        LiveSettings::default(),
        Duration::from_millis(100),
    )
    .unwrap();
    let reading = wait_for(&engine, |reading| reading.tempo.is_some());
    assert!((reading.tempo.unwrap().bpm - 128.0).abs() <= 2.0);
    assert_eq!(started_with.lock().unwrap().as_ref(), Some(&source));
    assert_eq!(engine.source().label(), "Player");
    engine.stop();
    assert!(*stopped.lock().unwrap());
}

#[test]
fn the_engine_reports_why_capture_could_not_start() {
    let mut capture = FakeCapture::playing(Vec::new());
    capture.refuse = Some(CaptureError::PermissionDenied("not allowed".into()));
    let result = LiveEngine::start(
        Box::new(capture),
        ListenSource::AllAudio,
        LiveSettings::default(),
        Duration::from_millis(100),
    );
    assert_eq!(
        result.err(),
        Some(CaptureError::PermissionDenied("not allowed".into()))
    );
}
