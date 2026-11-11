// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Checks that every key and tempo detector honours the contract of its
//! trait. Each detector is registered with one macro call at the bottom.

use std::f32::consts::TAU;
use std::num::NonZeroU32;

use euphoria_core::detect::{KeyDetector, StratumDetector, TempoDetector};
use euphoria_core::{AudioBuffer, Key};

const SAMPLE_RATE: u32 = 44_100;
const A_MINOR_CHORD_FREQUENCIES: [f32; 4] = [220.0, 261.63, 329.63, 440.0];
const CHORD_NOTE_AMPLITUDE: f32 = 0.2;
const SILENCE_SECONDS: f32 = 2.0;
const VERY_SHORT_SAMPLES: [f32; 4] = [0.5, -0.25, 0.75, -0.5];
const CLICK_TRACK_TEMPO: f32 = 120.0;
const CLICK_TRACK_SECONDS: f32 = 12.0;
const CLICK_FREQUENCY: f32 = 1_000.0;
const CLICK_DECAY_SECONDS: f32 = 0.02;

/// What a key detector must report for the A minor chord.
enum ChordExpectation {
    #[cfg_attr(not(feature = "libkeyfinder"), expect(dead_code))]
    Exactly(&'static str),
    AnyKey,
}

fn sample_rate() -> NonZeroU32 {
    NonZeroU32::new(SAMPLE_RATE).expect("the sample rate is not zero")
}

fn silence() -> AudioBuffer {
    let sample_count = (SAMPLE_RATE as f32 * SILENCE_SECONDS) as usize;
    AudioBuffer::new(vec![0.0; sample_count], sample_rate())
}

fn very_short_audio() -> AudioBuffer {
    AudioBuffer::new(VERY_SHORT_SAMPLES.to_vec(), sample_rate())
}

fn a_minor_chord(seconds: f32) -> AudioBuffer {
    let sample_count = (SAMPLE_RATE as f32 * seconds) as usize;
    let samples = (0..sample_count)
        .map(|index| {
            let time = index as f32 / SAMPLE_RATE as f32;
            A_MINOR_CHORD_FREQUENCIES
                .iter()
                .map(|frequency| (TAU * frequency * time).sin() * CHORD_NOTE_AMPLITUDE)
                .sum()
        })
        .collect();
    AudioBuffer::new(samples, sample_rate())
}

/// A decaying tone burst on every beat.
fn click_track(sample_rate: u32) -> AudioBuffer {
    let beat_seconds = 60.0 / CLICK_TRACK_TEMPO;
    let sample_count = (sample_rate as f32 * CLICK_TRACK_SECONDS) as usize;
    let samples = (0..sample_count)
        .map(|index| {
            let since_beat = (index as f32 / sample_rate as f32) % beat_seconds;
            (TAU * CLICK_FREQUENCY * since_beat).sin() * (-since_beat / CLICK_DECAY_SECONDS).exp()
        })
        .collect();
    AudioBuffer::new(
        samples,
        NonZeroU32::new(sample_rate).expect("the sample rate is not zero"),
    )
}

fn check_silence_has_no_key(detector: &dyn KeyDetector) {
    let key = detector
        .detect_key(&silence())
        .expect("key detection failed on silence");
    assert_eq!(key, None, "{} found a key in silence", detector.name());
}

fn check_very_short_audio_has_no_key(detector: &dyn KeyDetector) {
    let key = detector
        .detect_key(&very_short_audio())
        .expect("key detection failed on very short audio");
    assert_eq!(key, None, "{} found a key in four samples", detector.name());
}

fn check_a_minor_chord(
    detector: &dyn KeyDetector,
    chord_seconds: f32,
    expectation: ChordExpectation,
) {
    let key = detector
        .detect_key(&a_minor_chord(chord_seconds))
        .expect("key detection failed on the chord");
    eprintln!("{} on the A minor chord: {key:?}", detector.name());
    match expectation {
        ChordExpectation::Exactly(text) => {
            let expected: Key = text.parse().expect("the expected key is valid key text");
            assert_eq!(key, Some(expected), "{}", detector.name());
        }
        ChordExpectation::AnyKey => {
            assert!(key.is_some(), "{} found no key in a chord", detector.name());
        }
    }
}

fn check_silence_has_no_tempo(detector: &dyn TempoDetector) {
    let tempo = detector
        .detect_tempo(&silence())
        .expect("tempo detection failed on silence");
    assert_eq!(tempo, None, "{} found a tempo in silence", detector.name());
}

fn check_very_short_audio_has_no_tempo(detector: &dyn TempoDetector) {
    let tempo = detector
        .detect_tempo(&very_short_audio())
        .expect("tempo detection failed on very short audio");
    assert_eq!(
        tempo,
        None,
        "{} found a tempo in four samples",
        detector.name()
    );
}

macro_rules! key_detector_contract {
    ($module:ident, $detector:expr, chord_seconds: $chord_seconds:expr, expect: $expectation:expr) => {
        mod $module {
            use super::*;

            #[test]
            fn silence_has_no_key() {
                check_silence_has_no_key(&$detector);
            }

            #[test]
            fn very_short_audio_has_no_key() {
                check_very_short_audio_has_no_key(&$detector);
            }

            #[test]
            fn a_minor_chord_is_detected() {
                check_a_minor_chord(&$detector, $chord_seconds, $expectation);
            }
        }
    };
}

macro_rules! tempo_detector_contract {
    ($module:ident, $detector:expr) => {
        mod $module {
            use super::*;

            #[test]
            fn silence_has_no_tempo() {
                check_silence_has_no_tempo(&$detector);
            }

            #[test]
            fn very_short_audio_has_no_tempo() {
                check_very_short_audio_has_no_tempo(&$detector);
            }
        }
    };
}

#[cfg(feature = "libkeyfinder")]
key_detector_contract!(keyfinder_key, euphoria_core::detect::KeyFinderDetector, chord_seconds: 20.0, expect: ChordExpectation::Exactly("A minor"));
#[cfg(feature = "libkeyfinder")]
key_detector_contract!(
    keyfinder_tuning_corrected_key,
    euphoria_core::detect::KeyFinderVariant {
        tuning_correction: true,
        decision: euphoria_core::detect::KeyDecision::WholeTrack,
    },
    chord_seconds: 20.0,
    expect: ChordExpectation::Exactly("A minor")
);
#[cfg(feature = "libkeyfinder")]
key_detector_contract!(
    keyfinder_sections_vote_key,
    euphoria_core::detect::KeyFinderVariant {
        tuning_correction: true,
        decision: euphoria_core::detect::KeyDecision::SectionVote,
    },
    chord_seconds: 20.0,
    expect: ChordExpectation::Exactly("A minor")
);
// stratum-dsp is slow in debug builds and gives the same answer from half a second on.
key_detector_contract!(stratum_key, StratumDetector, chord_seconds: 2.0, expect: ChordExpectation::AnyKey);
tempo_detector_contract!(stratum_tempo, StratumDetector);

#[test]
fn stratum_finds_the_tempo_of_a_click_track_at_common_sample_rates() {
    for sample_rate in [44_100, 48_000, 96_000] {
        let tempo = StratumDetector
            .detect_tempo(&click_track(sample_rate))
            .expect("tempo detection failed on the click track")
            .expect("no tempo found in the click track");
        assert!(
            (tempo - CLICK_TRACK_TEMPO).abs() < 0.04 * CLICK_TRACK_TEMPO,
            "{tempo} BPM at {sample_rate} Hz"
        );
    }
}

#[test]
fn stratum_analyze_matches_the_trait_methods() {
    let audio = a_minor_chord(2.0);
    let analysis = StratumDetector.analyze(&audio).unwrap();
    assert!(analysis.key.is_some());
    assert!(analysis.tempo.is_some());
    assert_eq!(StratumDetector.detect_key(&audio).unwrap(), analysis.key);
    assert_eq!(
        StratumDetector.detect_tempo(&audio).unwrap(),
        analysis.tempo
    );
}
