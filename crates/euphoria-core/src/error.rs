// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::PathBuf;

/// Everything that can go wrong in the library.
#[derive(Debug, thiserror::Error)]
pub enum EuphoriaError {
    /// The file could not be opened.
    #[error("cannot open {path}: {source}")]
    Open {
        /// The file that failed to open.
        path: PathBuf,
        /// The underlying input or output error.
        source: std::io::Error,
    },
    /// The file was opened but its audio could not be decoded.
    #[error("cannot decode {path}: {reason}")]
    Decode {
        /// The file that failed to decode.
        path: PathBuf,
        /// What the decoder reported.
        reason: String,
    },
    /// The file decoded to zero samples.
    #[error("{path} contains no audio")]
    EmptyAudio {
        /// The file with no audio.
        path: PathBuf,
    },
    /// A detector failed on audio it was given.
    #[error("{detector} failed: {reason}")]
    Detection {
        /// Name of the detector that failed.
        detector: &'static str,
        /// What the detector reported.
        reason: String,
    },
    /// Text could not be read as a key name such as "A minor".
    #[error("unrecognised key text: {0}")]
    UnknownKeyText(String),
}
