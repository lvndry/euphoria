// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::num::NonZeroU32;

use crate::audio::AudioBuffer;
use crate::confidence::Confidence;
use crate::detect::{KeyReading, StratumDetector};
use crate::energy::rate_energy;
use crate::error::EuphoriaError;
use crate::fingerprint::Match;
use crate::key::Key;
use crate::tempo_range::TempoRange;

use super::buffer::RollingBuffer;

/// How a live analysis reads the audio, and when it gives up on it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveSettings {
    /// How much recent audio the key is read from.
    pub key_window_seconds: f32,
    /// How much recent audio the tempo and the energy are read from.
    pub tempo_window_seconds: f32,
    /// The key is not shown before this much audio has played.
    pub minimum_key_seconds: f32,
    /// The tempo and the energy are not shown before this much audio has
    /// played.
    pub minimum_tempo_seconds: f32,
    /// Audio whose level stays under this, in dBFS, counts as silence.
    pub quiet_level_db: f32,
    /// After this long in silence, everything heard before is forgotten.
    pub forget_after_quiet_seconds: f32,
    /// The range the tempo is folded into.
    pub tempo_range: TempoRange,
}

impl Default for LiveSettings {
    fn default() -> Self {
        Self {
            key_window_seconds: 25.0,
            tempo_window_seconds: 12.0,
            minimum_key_seconds: 8.0,
            minimum_tempo_seconds: 6.0,
            quiet_level_db: -50.0,
            forget_after_quiet_seconds: 2.0,
            tempo_range: TempoRange::DEFAULT,
        }
    }
}

/// The loudness of the latest audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelReading {
    /// Root mean square level of the latest tenth of a second, in dBFS.
    pub rms_db: f32,
    /// Highest sample of that stretch, in dBFS.
    pub peak_db: f32,
}

impl LevelReading {
    /// The level of silence, under what a meter can show.
    pub const FLOOR_DB: f32 = -90.0;

    /// Where the level sits on a meter from -60 dBFS (0.0) to 0 dBFS (1.0).
    pub fn meter_position(&self) -> f32 {
        ((self.rms_db + 60.0) / 60.0).clamp(0.0, 1.0)
    }
}

/// The key heard in the latest audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveKey {
    /// The most likely key.
    pub key: Key,
    /// How far to trust it.
    pub confidence: Confidence,
    /// The next most likely key.
    pub runner_up: Option<Key>,
}

/// The tempo heard in the latest audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveTempo {
    /// Beats per minute, in the analysis' tempo range.
    pub bpm: f32,
    /// How far to trust it.
    pub confidence: Confidence,
}

/// What the live analysis knows now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveReading {
    /// The latest level, or `None` before any audio arrived.
    pub level: Option<LevelReading>,
    /// True when the audio is silent or too quiet to read, or there has not
    /// been enough of it yet. Nothing below is then shown.
    pub listening: bool,
    /// The key, once enough audio has played.
    pub key: Option<LiveKey>,
    /// The tempo, once enough audio has played.
    pub tempo: Option<LiveTempo>,
    /// The energy from 1 to 10 of the latest audio.
    pub energy: Option<u8>,
    /// How much audio the readings come from, in seconds.
    pub heard_seconds: f32,
    /// The track of the library the audio is from, when a matcher is
    /// listening for one and found it.
    pub matched: Option<Match>,
    /// How long the audio has been exactly zero, which is what a capture
    /// that macOS has not allowed delivers.
    pub no_signal_seconds: f32,
}

impl LiveReading {
    /// Nothing heard yet.
    pub const fn listening() -> Self {
        Self {
            level: None,
            listening: true,
            key: None,
            tempo: None,
            energy: None,
            heard_seconds: 0.0,
            matched: None,
            no_signal_seconds: 0.0,
        }
    }
}

const LEVEL_BLOCK_SECONDS: f32 = 0.1;
/// How long the audio has to stay under the quiet level to count as quiet,
/// so the gaps between the hits of a sparse beat do not.
/// Samples below this are exact silence, not a quiet recording.
const NO_SIGNAL_PEAK: f32 = 1.0e-7;
const QUIET_HOLD_SECONDS: f32 = 0.5;

/// Reads the key, tempo and energy of audio as it arrives, from a rolling
/// buffer of the latest audio.
pub struct LiveAnalyzer {
    settings: LiveSettings,
    buffer: RollingBuffer,
    block_samples: usize,
    block_sum_squares: f64,
    block_peak: f32,
    block_filled: usize,
    level: Option<LevelReading>,
    quiet_samples: usize,
    no_signal_samples: usize,
}

impl LiveAnalyzer {
    /// An analyzer of audio at `sample_rate`.
    pub fn new(sample_rate: NonZeroU32, settings: LiveSettings) -> Self {
        let capacity = settings
            .key_window_seconds
            .max(settings.tempo_window_seconds)
            + 1.0;
        Self {
            block_samples: ((LEVEL_BLOCK_SECONDS * sample_rate.get() as f32) as usize).max(1),
            settings,
            buffer: RollingBuffer::new(sample_rate, capacity),
            block_sum_squares: 0.0,
            block_peak: 0.0,
            block_filled: 0,
            level: None,
            quiet_samples: 0,
            no_signal_samples: 0,
        }
    }

    /// The latest level.
    pub fn level(&self) -> Option<LevelReading> {
        self.level
    }

    /// How much audio is buffered, in seconds.
    pub fn buffered_seconds(&self) -> f32 {
        self.buffer.seconds()
    }

    /// Takes in the next mono audio. After the audio has been quiet for
    /// [`LiveSettings::forget_after_quiet_seconds`], everything buffered is
    /// dropped, so a new track is read on its own.
    pub fn push(&mut self, samples: &[f32]) {
        let sample_rate = self.buffer.sample_rate().get() as f32;
        let mut remaining = samples;
        while !remaining.is_empty() {
            let take = remaining.len().min(self.block_samples - self.block_filled);
            let (block, rest) = remaining.split_at(take);
            for sample in block {
                self.block_sum_squares += f64::from(*sample) * f64::from(*sample);
                self.block_peak = self.block_peak.max(sample.abs());
            }
            self.buffer.push(block);
            self.block_filled += take;
            if self.block_filled == self.block_samples {
                self.finish_block(sample_rate);
            }
            remaining = rest;
        }
    }

    fn finish_block(&mut self, sample_rate: f32) {
        let mean_square = self.block_sum_squares / self.block_samples as f64;
        let reading = LevelReading {
            rms_db: decibels(mean_square.sqrt() as f32),
            peak_db: decibels(self.block_peak),
        };
        self.level = Some(reading);
        if self.block_peak < NO_SIGNAL_PEAK {
            self.no_signal_samples += self.block_samples;
        } else {
            self.no_signal_samples = 0;
        }
        if reading.rms_db < self.settings.quiet_level_db {
            self.quiet_samples += self.block_samples;
            let quiet_seconds = self.quiet_samples as f32 / sample_rate;
            if quiet_seconds >= self.settings.forget_after_quiet_seconds {
                self.buffer.clear();
            }
        } else {
            self.quiet_samples = 0;
        }
        self.block_sum_squares = 0.0;
        self.block_peak = 0.0;
        self.block_filled = 0;
    }

    /// True while the latest audio is under the quiet level.
    pub fn is_quiet(&self) -> bool {
        self.quiet_samples as f32 >= QUIET_HOLD_SECONDS * self.buffer.sample_rate().get() as f32
    }

    /// The latest `seconds` of audio, for matching it against a library.
    pub fn recent_audio(&self, seconds: f32) -> AudioBuffer {
        self.buffer.latest(seconds)
    }

    /// A reading with only the level, for while nothing is read.
    pub fn listening_reading(&self) -> LiveReading {
        LiveReading {
            level: self.level,
            heard_seconds: self.buffer.seconds(),
            no_signal_seconds: self.no_signal_samples as f32
                / self.buffer.sample_rate().get() as f32,
            ..LiveReading::listening()
        }
    }

    /// Reads the key, tempo and energy of the buffered audio. While the
    /// audio is quiet or there is too little of it, nothing is read and the
    /// reading says it is listening.
    pub fn analyze(&self) -> Result<LiveReading, EuphoriaError> {
        let heard_seconds = self.buffer.seconds();
        let mut reading = self.listening_reading();
        if self.is_quiet() || heard_seconds < self.settings.minimum_tempo_seconds {
            return Ok(reading);
        }
        reading.listening = false;
        let recent = self.buffer.latest(self.settings.tempo_window_seconds);
        reading.tempo = self.read_tempo(&recent)?;
        reading.energy =
            rate_energy(&recent, reading.tempo.map(|tempo| tempo.bpm))?.map(|rating| rating.rating);
        if heard_seconds >= self.settings.minimum_key_seconds {
            let window = self.buffer.latest(self.settings.key_window_seconds);
            reading.key = read_key(&window)?;
        }
        Ok(reading)
    }

    fn read_tempo(&self, audio: &AudioBuffer) -> Result<Option<LiveTempo>, EuphoriaError> {
        Ok(StratumDetector.read_tempo(audio)?.map(|tempo| LiveTempo {
            bpm: tempo.tempo_in(self.settings.tempo_range),
            confidence: tempo.confidence(),
        }))
    }
}

fn read_key(audio: &AudioBuffer) -> Result<Option<LiveKey>, EuphoriaError> {
    let reading = key_reading(audio)?;
    Ok(reading.key.map(|key| LiveKey {
        key,
        confidence: reading.confidence().unwrap_or(Confidence::Low),
        runner_up: reading.runner_up,
    }))
}

#[cfg(feature = "drone")]
fn key_reading(audio: &AudioBuffer) -> Result<KeyReading, EuphoriaError> {
    crate::detect::DroneDetector.read_key(audio)
}

#[cfg(not(feature = "drone"))]
fn key_reading(audio: &AudioBuffer) -> Result<KeyReading, EuphoriaError> {
    use crate::detect::KeyDetector;
    Ok(KeyReading {
        key: StratumDetector.detect_key(audio)?,
        ..KeyReading::default()
    })
}

fn decibels(amplitude: f32) -> f32 {
    if amplitude <= 0.0 {
        LevelReading::FLOOR_DB
    } else {
        (20.0 * amplitude.log10()).max(LevelReading::FLOOR_DB)
    }
}
