// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Product logic for Euphoria: decoding, key and tempo detection, harmony.
//! This crate has no user interface and prints nothing.

/// Analyzing files and batches of files.
pub mod analysis;
/// Decoding audio files to mono samples.
pub mod audio;
/// Remembering analyses between runs.
pub mod cache;
/// Key and tempo detectors.
pub mod detect;
/// The error type returned by the library.
pub mod error;
/// Key relations and ranking of the tracks that mix into another.
pub mod harmony;
/// Musical keys, Camelot and Open Key notation.
pub mod key;
/// Finding audio files in folders.
pub mod library;

pub use analysis::{
    ANALYZER_VERSION, FileResult, Progress, TrackAnalysis, analyze_file, analyze_files,
};
pub use audio::{AudioBuffer, decode_mono};
pub use cache::{
    Cache, CachePolicy, CachedBatch, ContentHash, DATA_DIRECTORY_VARIABLE, analyze_files_cached,
    clear_cache, default_cache_path,
};
pub use error::{DecodeFailure, ErrorSource, EuphoriaError};
pub use harmony::{
    MixMatch, MixOptions, MixTrack, Relation, TempoFit, TempoScaling, TrackFeatures, mixes_into,
};
pub use key::{Key, Mode};
pub use library::{SUPPORTED_EXTENSIONS, find_audio_files, is_supported_audio_file};
