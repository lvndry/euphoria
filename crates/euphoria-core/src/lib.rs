// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Product logic for Euphoria: decoding, key and tempo detection, energy
//! rating, harmony.
//! This crate has no user interface and prints nothing.

/// Analyzing files and batches of files.
pub mod analysis;
/// Decoding audio files to mono samples.
pub mod audio;
/// Decoding audio files a little at a time, for playback.
pub mod audio_stream;
/// Remembering analyses between runs.
pub mod cache;
/// How far to trust a detected key or tempo.
pub mod confidence;
/// Keys and tempos set by the user.
pub mod correction;
/// Key and tempo detectors.
pub mod detect;
/// Energy rating from 1 to 10 and the features behind it.
pub mod energy;
/// The error type returned by the library.
pub mod error;
/// Writing analysis results in the formats of other DJ software.
pub mod export;
/// Key relations and ranking of the tracks that mix into another.
pub mod harmony;
/// Musical keys, Camelot and Open Key notation.
pub mod key;
/// Finding audio files in folders.
pub mod library;
/// How good a file sounds for DJ use: codec, bit rate, cutoff, clipping.
pub mod quality;
/// Reading audio file tags and writing analysis results into them, with a
/// preview before and an undo after.
pub mod tags;
/// The range of tempos a track is reported at.
pub mod tempo_range;
/// Reading the title and artist stored in audio files.
pub mod track_name;

pub use analysis::{
    ANALYZER_VERSION, FileResult, Progress, TrackAnalysis, analyze_file, analyze_files,
};
pub use audio::{AudioBuffer, MAXIMUM_SAMPLE_RATE, MINIMUM_SAMPLE_RATE, decode_mono};
pub use audio_stream::AudioStream;
pub use cache::{
    Cache, CachePolicy, CachedAnalysis, CachedAnalyzer, CachedBatch, ContentHash,
    DATA_DIRECTORY_VARIABLE, analyze_files_cached, clear_cache, default_cache_path,
};
pub use confidence::Confidence;
pub use correction::Correction;
pub use detect::KeySection;
pub use energy::{
    ENERGY_CURVE_HOP_SECONDS, ENERGY_CURVE_WINDOW_SECONDS, ENERGY_MAXIMUM_SAMPLE_RATE,
    ENERGY_MINIMUM_SAMPLE_RATE, ENERGY_MODEL, ENERGY_MODEL_VERSION, EnergyCurvePoint,
    EnergyFeatures, EnergyModel, EnergyRating, FeatureAnchor, energy_curve, energy_features,
    rate_energy, rate_features,
};
pub use error::{DecodeFailure, ErrorSource, EuphoriaError, ExportProblem};
pub use export::{
    AssembledLibrary, PlaylistLayout, SkippedTrack, UnreadTags, assemble_library, playlist_count,
    playlists_from_folders,
};
pub use harmony::{
    MixMatch, MixOptions, MixTrack, Relation, TempoFit, TempoScaling, TrackFeatures, mixes_into,
};
pub use key::{Key, Mode};
pub use library::{SUPPORTED_EXTENSIONS, find_audio_files, is_supported_audio_file};
pub use tempo_range::TempoRange;
pub use track_name::{TrackName, read_track_name};
