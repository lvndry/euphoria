// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pins the direction of the energy rating with synthesized audio: louder,
//! busier, bassier and faster never rate lower, and silence has no rating.

use std::f32::consts::TAU;
use std::num::NonZeroU32;

use euphoria_core::{
    AudioBuffer, ENERGY_CURVE_HOP_SECONDS, ENERGY_CURVE_WINDOW_SECONDS, ENERGY_MODEL_VERSION,
    EnergyRating, energy_curve, energy_features, rate_energy, rate_features,
};

const SAMPLE_RATE: u32 = 44_100;
const KICK_FREQUENCY: f32 = 60.0;
const KICK_DECAY_PER_SECOND: f32 = 30.0;

fn buffer(samples: Vec<f32>) -> AudioBuffer {
    let sample_rate = NonZeroU32::new(SAMPLE_RATE).expect("the sample rate is not zero");
    AudioBuffer::new(samples, sample_rate)
}

fn sample_count(seconds: f32) -> usize {
    (SAMPLE_RATE as f32 * seconds) as usize
}

fn silence(seconds: f32) -> Vec<f32> {
    vec![0.0; sample_count(seconds)]
}

fn sine(frequency: f32, amplitude: f32, seconds: f32) -> Vec<f32> {
    (0..sample_count(seconds))
        .map(|index| (TAU * frequency * index as f32 / SAMPLE_RATE as f32).sin() * amplitude)
        .collect()
}

fn noise(amplitude: f32, seconds: f32) -> Vec<f32> {
    let mut state = 0x2545_f491_u32;
    (0..sample_count(seconds))
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * amplitude
        })
        .collect()
}

/// Bursts of a decaying tone at `frequency`. At 60 Hz it sounds like a kick
/// drum.
fn pulse_train(frequency: f32, pulses_per_second: f32, amplitude: f32, seconds: f32) -> Vec<f32> {
    let period = 1.0 / pulses_per_second;
    (0..sample_count(seconds))
        .map(|index| {
            let time = index as f32 / SAMPLE_RATE as f32;
            let since_pulse = (time + period / 2.0) % period;
            (TAU * frequency * since_pulse).sin()
                * (-since_pulse * KICK_DECAY_PER_SECOND).exp()
                * amplitude
        })
        .collect()
}

fn mix(first: &[f32], second: &[f32]) -> Vec<f32> {
    first
        .iter()
        .zip(second)
        .map(|(first_sample, second_sample)| first_sample + second_sample)
        .collect()
}

fn rate(samples: Vec<f32>) -> EnergyRating {
    rate_energy(&buffer(samples), None)
        .expect("rating failed")
        .expect("the audio has a rating")
}

fn assert_never_lower(lower: &EnergyRating, higher: &EnergyRating) {
    assert!(
        higher.score >= lower.score && higher.rating >= lower.rating,
        "expected {higher:?} to rate at least as high as {lower:?}"
    );
}

#[test]
fn silence_has_no_rating() {
    let audio = buffer(silence(5.0));
    assert_eq!(energy_features(&audio).unwrap(), None);
    assert_eq!(rate_energy(&audio, None).unwrap(), None);
    assert_eq!(rate_energy(&audio, Some(128.0)).unwrap(), None);
}

#[test]
fn audio_shorter_than_one_second_has_no_rating() {
    let audio = buffer(sine(440.0, 0.5, 0.5));
    assert_eq!(rate_energy(&audio, None).unwrap(), None);
    assert_eq!(rate_energy(&buffer(Vec::new()), None).unwrap(), None);
}

#[test]
fn a_quiet_sine_rates_one() {
    let rating = rate(sine(440.0, 0.01, 5.0));
    assert_eq!(rating.rating, 1);
    assert_eq!(rating.model_version, ENERGY_MODEL_VERSION);
    assert_eq!(rating.tempo, None);
}

#[test]
fn loud_broadband_noise_rates_above_a_quiet_sine() {
    let quiet_sine = rate(sine(440.0, 0.01, 5.0));
    let loud_noise = rate(noise(0.9, 5.0));
    assert!(loud_noise.features.integrated_loudness_lufs > -10.0);
    assert!(loud_noise.features.spectral_flux > quiet_sine.features.spectral_flux);
    assert!(loud_noise.rating > quiet_sine.rating);
}

#[test]
fn louder_never_rates_lower() {
    let mut previous: Option<EnergyRating> = None;
    for amplitude in [0.02, 0.05, 0.1, 0.2, 0.4, 0.8] {
        let rating = rate(pulse_train(KICK_FREQUENCY, 4.0, amplitude, 8.0));
        if let Some(previous) = previous {
            assert!(
                rating.features.integrated_loudness_lufs
                    > previous.features.integrated_loudness_lufs
            );
            assert_never_lower(&previous, &rating);
        }
        previous = Some(rating);
    }
    let quiet_noise = rate(noise(0.05, 8.0));
    let loud_noise = rate(noise(0.9, 8.0));
    assert!(loud_noise.rating > quiet_noise.rating);
}

#[test]
fn faster_pulses_never_rate_lower() {
    let mut previous: Option<EnergyRating> = None;
    for pulses_per_second in [1.0, 2.0, 4.0, 8.0] {
        let rating = rate(pulse_train(KICK_FREQUENCY, pulses_per_second, 0.8, 8.0));
        assert!(
            (rating.features.onset_rate - pulses_per_second).abs() < 0.2,
            "{pulses_per_second} pulses per second gave {:?}",
            rating.features
        );
        if let Some(previous) = previous {
            assert_never_lower(&previous, &rating);
        }
        previous = Some(rating);
    }
    let slowest = rate(pulse_train(KICK_FREQUENCY, 1.0, 0.8, 8.0));
    let fastest = rate(pulse_train(KICK_FREQUENCY, 8.0, 0.8, 8.0));
    assert!(fastest.rating > slowest.rating);
}

#[test]
fn busier_never_rates_lower() {
    let tone = sine(440.0, 0.3, 8.0);
    let held = rate(tone.clone());
    let with_clicks = rate(mix(&tone, &pulse_train(3_000.0, 4.0, 0.3, 8.0)));
    assert!(with_clicks.features.spectral_flux > held.features.spectral_flux);
    assert!(with_clicks.features.onset_rate > held.features.onset_rate);
    assert_never_lower(&held, &with_clicks);
}

#[test]
fn bassier_never_rates_lower() {
    let thin = rate(pulse_train(400.0, 4.0, 0.5, 8.0));
    let bassy = rate(pulse_train(KICK_FREQUENCY, 4.0, 0.5, 8.0));
    assert!(bassy.features.low_frequency_share > 0.8, "{bassy:?}");
    assert!(thin.features.low_frequency_share < 0.2, "{thin:?}");

    let only_bass_differs = rate_features(
        &euphoria_core::EnergyFeatures {
            low_frequency_share: bassy.features.low_frequency_share,
            ..thin.features
        },
        None,
    );
    assert_never_lower(&thin, &only_bass_differs);
    assert!(only_bass_differs.score > thin.score);
}

#[test]
fn a_faster_tempo_never_rates_lower() {
    let audio = buffer(pulse_train(KICK_FREQUENCY, 2.0, 0.5, 8.0));
    let mut previous: Option<EnergyRating> = None;
    for tempo in [90.0, 110.0, 124.0, 128.0, 140.0, 174.0] {
        let rating = rate_energy(&audio, Some(tempo)).unwrap().unwrap();
        assert_eq!(rating.tempo, Some(tempo));
        if let Some(previous) = previous {
            assert_never_lower(&previous, &rating);
        }
        previous = Some(rating);
    }
}

#[test]
fn rating_is_reproducible() {
    let samples = mix(
        &pulse_train(KICK_FREQUENCY, 4.0, 0.5, 8.0),
        &noise(0.05, 8.0),
    );
    assert_eq!(rate(samples.clone()), rate(samples));
}

#[test]
fn the_curve_rises_when_the_audio_gets_more_energetic() {
    let mut samples = sine(440.0, 0.02, 16.0);
    samples.extend(mix(
        &pulse_train(KICK_FREQUENCY, 8.0, 0.8, 16.0),
        &noise(0.2, 16.0),
    ));
    let curve = energy_curve(&buffer(samples), None).unwrap();

    let expected_points =
        ((32.0 - ENERGY_CURVE_WINDOW_SECONDS) / ENERGY_CURVE_HOP_SECONDS) as usize + 1;
    assert_eq!(curve.len(), expected_points);
    for (index, point) in curve.iter().enumerate() {
        let expected_start = index as f32 * ENERGY_CURVE_HOP_SECONDS;
        assert!((point.start_seconds - expected_start).abs() < 1.0e-3);
        assert!((point.duration_seconds - ENERGY_CURVE_WINDOW_SECONDS).abs() < 1.0e-3);
    }

    let ratings: Vec<u8> = curve
        .iter()
        .map(|point| point.rating.expect("every window has signal").rating)
        .collect();
    assert!(
        ratings.windows(2).all(|pair| pair[0] <= pair[1]),
        "{ratings:?}"
    );
    assert!(ratings[ratings.len() - 1] > ratings[0], "{ratings:?}");
}

#[test]
fn each_curve_point_is_the_rating_of_its_window() {
    let samples = mix(
        &pulse_train(KICK_FREQUENCY, 4.0, 0.5, 14.0),
        &sine(440.0, 0.1, 14.0),
    );
    let curve = energy_curve(&buffer(samples.clone()), Some(120.0)).unwrap();
    let point = curve[1];
    let start = sample_count(point.start_seconds);
    let end = start + sample_count(point.duration_seconds);
    let window = buffer(samples[start..end].to_vec());
    assert_eq!(point.rating, rate_energy(&window, Some(120.0)).unwrap());
}

#[test]
fn silent_windows_of_the_curve_have_no_rating() {
    let mut samples = silence(12.0);
    samples.extend(pulse_train(KICK_FREQUENCY, 4.0, 0.5, 12.0));
    let curve = energy_curve(&buffer(samples), None).unwrap();
    assert_eq!(curve[0].rating, None);
    assert!(curve[curve.len() - 1].rating.is_some());
}

#[test]
fn audio_shorter_than_one_window_gives_a_single_point() {
    let curve = energy_curve(&buffer(pulse_train(KICK_FREQUENCY, 4.0, 0.5, 3.0)), None).unwrap();
    assert_eq!(curve.len(), 1);
    assert_eq!(curve[0].start_seconds, 0.0);
    assert!((curve[0].duration_seconds - 3.0).abs() < 1.0e-3);
    assert!(curve[0].rating.is_some());
}

#[test]
fn empty_audio_has_an_empty_curve() {
    assert!(energy_curve(&buffer(Vec::new()), None).unwrap().is_empty());
}
