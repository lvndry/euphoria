// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading the key, tempo and energy of audio as it plays, from a rolling
//! buffer of the latest seconds.
//!
//! The audio stays in memory: nothing here writes or sends it anywhere, and
//! the buffer holds about half a minute at most.

mod analyzer;
mod buffer;
mod capture;
mod engine;
#[cfg(target_os = "macos")]
mod macos;

pub use analyzer::{LevelReading, LiveAnalyzer, LiveKey, LiveReading, LiveSettings, LiveTempo};
pub use buffer::RollingBuffer;
pub use capture::{AudioCapture, AudioSink, CaptureError, ListenSource};
pub use engine::{ANALYSIS_INTERVAL, LiveEngine};
#[cfg(target_os = "macos")]
pub use macos::ProcessTapCapture;

#[cfg(test)]
mod tests;
