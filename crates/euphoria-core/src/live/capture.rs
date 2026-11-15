// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::num::NonZeroU32;

/// Where the audio to listen to comes from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ListenSource {
    /// Everything the Mac plays, except Euphoria itself.
    AllAudio,
    /// The audio of one application.
    Process {
        /// The operating system's process id.
        process_id: i32,
        /// The name shown to the user.
        name: String,
    },
}

impl ListenSource {
    /// The name the window shows as the source.
    pub fn label(&self) -> &str {
        match self {
            ListenSource::AllAudio => "All audio",
            ListenSource::Process { name, .. } => name,
        }
    }
}

/// Why listening could not start or stopped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    /// The operating system cannot capture audio from other applications.
    #[error("{0}")]
    Unsupported(String),
    /// The user has not allowed Euphoria to hear the audio of other
    /// applications.
    #[error("{0}")]
    PermissionDenied(String),
    /// Starting the capture failed for another reason.
    #[error("{0}")]
    Failed(String),
}

/// Receives mono audio, in the order it was played, on the capture thread.
pub type AudioSink = Box<dyn FnMut(&[f32]) + Send>;

/// Captures the audio the computer plays. Implementations hand audio to the
/// sink as it arrives and keep none of it. Platforms implement this, and
/// tests implement it with generated audio.
pub trait AudioCapture: Send {
    /// The applications that can be listened to alone, the ones playing
    /// audio first.
    fn sources(&self) -> Vec<ListenSource>;

    /// Starts capturing `source` into `sink` and returns the sample rate of
    /// the audio the sink receives.
    fn start(&mut self, source: &ListenSource, sink: AudioSink)
    -> Result<NonZeroU32, CaptureError>;

    /// Stops capturing. The sink is not called after this returns.
    fn stop(&mut self);
}
