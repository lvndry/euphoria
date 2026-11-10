// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Product logic for Euphoria: decoding, key and tempo detection, harmony.
//! This crate has no user interface and prints nothing.

/// Analyzing files and batches of files.
pub mod analysis;
/// Decoding audio files to mono samples.
pub mod audio;
/// Key and tempo detectors.
pub mod detect;
/// The error type returned by the library.
pub mod error;
/// Musical keys and Camelot notation.
pub mod key;
/// Finding audio files in folders.
pub mod library;

pub use analysis::{FileResult, Progress, TrackAnalysis, analyze_file, analyze_files};
pub use audio::{AudioBuffer, decode_mono};
pub use error::{DecodeFailure, ErrorSource, EuphoriaError};
pub use key::{Key, Mode};
pub use library::{SUPPORTED_EXTENSIONS, find_audio_files, is_supported_audio_file};
