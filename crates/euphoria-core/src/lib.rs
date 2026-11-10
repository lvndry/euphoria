// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Product logic for Euphoria: decoding, key and tempo detection, harmony.
//! This crate has no user interface and prints nothing.

/// Decoding audio files to mono samples.
pub mod audio;
/// Key and tempo detectors.
pub mod detect;
/// The error type returned by the library.
pub mod error;
/// Writing analysis results in the formats of other DJ software.
pub mod export;
/// Musical keys and Camelot notation.
pub mod key;

pub use audio::{AudioBuffer, decode_mono};
pub use error::{DecodeFailure, ErrorSource, EuphoriaError, ExportProblem};
pub use key::{Key, Mode};
