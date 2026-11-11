// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sounds to check a key by ear: a soft drone of a key's root and fifth, and
//! the notes of a small piano, played over the track through the same
//! output. Both are made here, sample by sample, so they need no files.
//!
//! Every sound reads one shared level as it plays, so moving the volume
//! changes the drone at once rather than from the next one on.

use std::f64::consts::TAU;
use std::num::{NonZeroU16, NonZeroU32};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use euphoria_core::Key;

/// The sample rate of every reference sound. The output converts it to its
/// own when the two differ.
const SAMPLE_RATE: u32 = 44_100;

/// How loud the drone is at full level, all its notes together. A track is
/// mastered close to full scale, so this sits well under it.
const DRONE_GAIN: f32 = 0.16;

/// How loud a piano note is at full level, as it is struck.
const PIANO_GAIN: f32 = 0.3;

/// How long the drone takes to swell in, so it starts without a click.
const DRONE_FADE_IN_SECONDS: f64 = 0.4;

/// How long a piano note rings before it ends.
const PIANO_NOTE_SECONDS: f64 = 2.0;

/// How quickly a piano note dies away: its loudness halves in this time.
const PIANO_HALF_LIFE_SECONDS: f64 = 0.35;

/// How long a piano note takes to reach its loudest, so it starts without a
/// click.
const PIANO_ATTACK_SECONDS: f64 = 0.005;

/// The MIDI note of the lowest C of the drone: C3, so the drone sits under
/// most melodies and over most basses.
const DRONE_LOWEST_C: u8 = 48;

/// The MIDI note of the C the piano starts on: middle C.
pub(crate) const PIANO_LOWEST_C: u8 = 60;

/// How loud the reference sounds are, 0 to 1, shared by every sound playing
/// and by the volume control.
#[derive(Debug, Clone)]
pub(crate) struct ReferenceLevel(Arc<AtomicU32>);

impl ReferenceLevel {
    /// A level of `level`, kept between 0 and 1.
    pub(crate) fn new(level: f32) -> Self {
        Self(Arc::new(AtomicU32::new(level.clamp(0.0, 1.0).to_bits())))
    }

    /// The level now.
    pub(crate) fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }

    /// Changes the level of every sound sharing it, kept between 0 and 1.
    pub(crate) fn set(&self, level: f32) {
        self.0
            .store(level.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
}

/// The frequency of MIDI note `note` with concert A at 440 Hz moved by
/// `tuning_cents`, so the reference matches a track tuned a little sharp or
/// flat.
pub(crate) fn frequency(note: u8, tuning_cents: f32) -> f32 {
    let semitones_from_a = f64::from(note) - 69.0 + f64::from(tuning_cents) / 100.0;
    (440.0 * 2f64.powf(semitones_from_a / 12.0)) as f32
}

/// One sine of a sound.
#[derive(Debug, Clone, Copy)]
struct Partial {
    frequency: f64,
    amplitude: f64,
    /// How much faster than the sound this partial dies away; higher
    /// harmonics of a struck string fade first.
    decay_factor: f64,
}

/// How a sound's loudness moves over time.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    /// Swells in, then holds until it is stopped.
    Drone,
    /// Struck: rises at once, dies away and ends.
    Struck,
}

/// A reference sound: a drone or a piano note, in mono.
#[derive(Debug)]
pub(crate) struct ReferenceSound {
    partials: Vec<Partial>,
    shape: Shape,
    gain: f64,
    level: ReferenceLevel,
    frame: u64,
}

impl ReferenceSound {
    /// The root and the fifth of `key`, with the root an octave lower under
    /// them, held until stopped. A minor and a major key on the same root
    /// share a drone: the third is left out on purpose, so the drone never
    /// takes a side; whether the track sounds minor or major over it is
    /// for the ear, with the piano.
    pub(crate) fn drone(key: Key, tuning_cents: f32, level: ReferenceLevel) -> Self {
        let root = DRONE_LOWEST_C + key.tonic();
        let partials = [(root - 12, 0.3), (root, 0.45), (root + 7, 0.25)]
            .map(|(note, amplitude)| Partial {
                frequency: f64::from(frequency(note, tuning_cents)),
                amplitude,
                decay_factor: 0.0,
            })
            .to_vec();
        Self {
            partials,
            shape: Shape::Drone,
            gain: f64::from(DRONE_GAIN),
            level,
            frame: 0,
        }
    }

    /// MIDI note `note` struck on a soft, piano like instrument.
    pub(crate) fn piano_note(note: u8, tuning_cents: f32, level: ReferenceLevel) -> Self {
        let fundamental = f64::from(frequency(note, tuning_cents));
        let partials = [
            (1.0, 0.62, 1.0),
            (2.0, 0.24, 1.6),
            (3.0, 0.1, 2.4),
            (4.0, 0.04, 3.2),
        ]
        .map(|(harmonic, amplitude, decay_factor)| Partial {
            frequency: fundamental * harmonic,
            amplitude,
            decay_factor,
        })
        .to_vec();
        Self {
            partials,
            shape: Shape::Struck,
            gain: f64::from(PIANO_GAIN),
            level,
            frame: 0,
        }
    }

    /// The frequencies the sound is made of, lowest first.
    #[cfg(test)]
    pub(crate) fn frequencies(&self) -> Vec<f32> {
        self.partials
            .iter()
            .map(|partial| partial.frequency as f32)
            .collect()
    }

    /// True for a drone, which holds until stopped.
    #[cfg(test)]
    pub(crate) fn is_drone(&self) -> bool {
        self.shape == Shape::Drone
    }

    fn total_frames(&self) -> Option<u64> {
        match self.shape {
            Shape::Drone => None,
            Shape::Struck => Some((PIANO_NOTE_SECONDS * f64::from(SAMPLE_RATE)) as u64),
        }
    }
}

impl Iterator for ReferenceSound {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.total_frames().is_some_and(|total| self.frame >= total) {
            return None;
        }
        let seconds = self.frame as f64 / f64::from(SAMPLE_RATE);
        self.frame += 1;
        let (envelope, decay_rate) = match self.shape {
            Shape::Drone => ((seconds / DRONE_FADE_IN_SECONDS).min(1.0), 0.0),
            Shape::Struck => (
                (seconds / PIANO_ATTACK_SECONDS).min(1.0),
                std::f64::consts::LN_2 / PIANO_HALF_LIFE_SECONDS,
            ),
        };
        let sum: f64 = self
            .partials
            .iter()
            .map(|partial| {
                let decay = (-decay_rate * partial.decay_factor * seconds).exp();
                partial.amplitude * decay * (TAU * partial.frequency * seconds).sin()
            })
            .sum();
        let level = f64::from(self.level.get());
        Some((sum * envelope * self.gain * level) as f32)
    }
}

impl rodio::Source for ReferenceSound {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> NonZeroU16 {
        NonZeroU16::MIN
    }

    fn sample_rate(&self) -> NonZeroU32 {
        NonZeroU32::new(SAMPLE_RATE).unwrap_or(NonZeroU32::MIN)
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_frames()
            .map(|_| Duration::from_secs_f64(PIANO_NOTE_SECONDS))
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use euphoria_core::Mode;
    use rodio::Source;

    use super::*;
    use crate::playback::TrackSource;

    fn peak(samples: &[f32]) -> f32 {
        samples
            .iter()
            .fold(0.0, |loudest, sample| loudest.max(sample.abs()))
    }

    fn take_seconds(sound: &mut ReferenceSound, seconds: f64) -> Vec<f32> {
        sound
            .by_ref()
            .take((seconds * f64::from(SAMPLE_RATE)) as usize)
            .collect()
    }

    #[test]
    fn notes_follow_equal_temperament_and_the_track_tuning() {
        assert!((frequency(69, 0.0) - 440.0).abs() < 1e-3);
        assert!((frequency(60, 0.0) - 261.626).abs() < 1e-2);
        assert!((frequency(81, 0.0) - 880.0).abs() < 1e-3);
        let sharp = frequency(69, 20.0);
        assert!((sharp - 445.1).abs() < 0.1, "{sharp}");
    }

    #[test]
    fn a_drone_holds_the_root_and_fifth_of_its_key() {
        let level = ReferenceLevel::new(1.0);
        let a_minor = ReferenceSound::drone(Key::new(9, Mode::Minor), 0.0, level.clone());
        let frequencies = a_minor.frequencies();
        assert_eq!(frequencies.len(), 3);
        assert!((frequencies[0] - 110.0).abs() < 0.01);
        assert!((frequencies[1] - 220.0).abs() < 0.01);
        assert!((frequencies[2] - 329.63).abs() < 0.01);
        let a_major = ReferenceSound::drone(Key::new(9, Mode::Major), 0.0, level);
        assert_eq!(a_major.frequencies(), frequencies);
        assert!(a_minor.is_drone());
        assert_eq!(a_minor.total_duration(), None);
    }

    #[test]
    fn a_drone_swells_in_and_stays_soft_under_full_scale() {
        let mut drone =
            ReferenceSound::drone(Key::new(0, Mode::Major), 0.0, ReferenceLevel::new(1.0));
        let start = take_seconds(&mut drone, 0.01);
        let held = take_seconds(&mut drone, 2.0);
        assert!(peak(&start) < 0.01, "{}", peak(&start));
        assert!(peak(&held) > 0.05, "{}", peak(&held));
        assert!(peak(&held) <= DRONE_GAIN, "{}", peak(&held));
        assert!(drone.next().is_some());
    }

    #[test]
    fn the_level_moves_a_sound_that_is_already_playing() {
        let level = ReferenceLevel::new(1.0);
        let mut drone = ReferenceSound::drone(Key::new(2, Mode::Minor), 0.0, level.clone());
        let full = peak(&take_seconds(&mut drone, 1.0)[22_050..]);
        level.set(0.25);
        let quarter = peak(&take_seconds(&mut drone, 0.5));
        assert!((quarter / full - 0.25).abs() < 0.03, "{full} {quarter}");
        level.set(0.0);
        assert_eq!(peak(&take_seconds(&mut drone, 0.1)), 0.0);
        level.set(7.0);
        assert_eq!(level.get(), 1.0);
    }

    #[test]
    fn a_piano_note_rings_and_ends_on_its_own() {
        let mut note =
            ReferenceSound::piano_note(PIANO_LOWEST_C + 9, 0.0, ReferenceLevel::new(1.0));
        assert!(!note.is_drone());
        assert!((note.frequencies()[0] - 440.0).abs() < 0.01);
        assert_eq!(note.channels().get(), 1);
        let struck = take_seconds(&mut note, 0.1);
        let fading = take_seconds(&mut note, 1.8);
        assert!(peak(&struck) > 0.2, "{}", peak(&struck));
        assert!(peak(&struck) <= PIANO_GAIN);
        assert!(peak(&fading[fading.len() - 4_410..]) < 0.05 * peak(&struck));
        let rest = note.by_ref().count();
        assert!(rest > 0 && rest < 9_000, "{rest}");
        assert_eq!(note.next(), None);
    }

    /// The tone and a piano note are opened beside a real track and mixed
    /// the way the system output mixes them. Nothing here is heard: it
    /// checks that the samples meet and add up, not how they sound.
    #[test]
    fn the_drone_and_the_piano_mix_over_a_playing_track() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../euphoria-core/tests/fixtures/sine-440hz-mono.wav");
        let channels = NonZeroU16::new(2).unwrap();
        let sample_rate = NonZeroU32::new(48_000).unwrap();
        let mixed = |with_reference: bool| {
            let (mixer, mut output) = rodio::mixer::mixer(channels, sample_rate);
            mixer.add(TrackSource::open(&fixture, 0.0).unwrap());
            if with_reference {
                let level = ReferenceLevel::new(1.0);
                mixer.add(ReferenceSound::drone(
                    Key::new(9, Mode::Minor),
                    0.0,
                    level.clone(),
                ));
                mixer.add(ReferenceSound::piano_note(PIANO_LOWEST_C, 0.0, level));
            }
            let mut samples = Vec::new();
            while samples.len() < 96_000 {
                match output.next() {
                    Some(sample) => samples.push(sample),
                    None => break,
                }
            }
            samples
        };
        let track_alone = mixed(false);
        let together = mixed(true);
        assert!(track_alone.len() > 90_000, "{}", track_alone.len());
        let difference: Vec<f32> = together
            .iter()
            .zip(&track_alone)
            .map(|(both, track)| both - track)
            .collect();
        let after_the_swell = &difference[48_000..];
        assert!(peak(after_the_swell) > 0.1, "{}", peak(after_the_swell));
        assert!(
            peak(&together) <= peak(&track_alone) + DRONE_GAIN + PIANO_GAIN + 0.01,
            "{} {}",
            peak(&together),
            peak(&track_alone)
        );
    }
}
