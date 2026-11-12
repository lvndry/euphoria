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
/// Playlists the user builds, kept by the content of their files.
pub mod playlists;
/// How good a file sounds for DJ use: codec, bit rate, cutoff, clipping.
pub mod quality;
/// Separating tracks into vocals, drums, bass and the rest.
#[cfg(feature = "stems")]
pub mod stems;
/// Reading audio file tags and writing analysis results into them, with a
/// preview before and an undo after.
pub mod tags;
/// The range of tempos a track is reported at.
pub mod tempo_range;
/// The cues of a track: those analysis placed, or the user's.
pub mod track_cues;
/// Reading the title and artist stored in audio files.
pub mod track_name;

pub use analysis::{
    ANALYZER_VERSION, AnalysisObserver, AnalysisStage, FileResult, Progress, TrackAnalysis,
    Unobserved, analyze_file, analyze_file_observed, analyze_files, analyze_files_observed,
};
pub use audio::{AudioBuffer, MAXIMUM_SAMPLE_RATE, MINIMUM_SAMPLE_RATE, decode_mono};
pub use audio_stream::AudioStream;
pub use cache::{
    Cache, CachePolicy, CachedAnalysis, CachedAnalyzer, CachedBatch, ContentHash,
    DATA_DIRECTORY_VARIABLE, DetectedValues, StoredCorrection, analyze_files_cached,
    analyze_files_cached_observed, clear_cache, default_cache_path,
};
pub use confidence::Confidence;
pub use correction::Correction;
pub use detect::{BeatGrid, CueSection, KeySection, MAXIMUM_CUES, SectionCue};
pub use energy::{
    ENERGY_CURVE_HOP_SECONDS, ENERGY_CURVE_WINDOW_SECONDS, ENERGY_MAXIMUM_SAMPLE_RATE,
    ENERGY_MINIMUM_SAMPLE_RATE, ENERGY_MODEL, ENERGY_MODEL_VERSION, EnergyCurvePoint,
    EnergyFeatures, EnergyModel, EnergyRating, FeatureAnchor, energy_curve, energy_features,
    rate_energy, rate_features,
};
pub use error::{DecodeFailure, ErrorSource, EuphoriaError, ExportProblem};
pub use export::{
    AssembledLibrary, CORRECTIONS_SCHEMA_VERSION, ExportContents, M3uTrack, PathInclusion,
    PlaylistLayout, SkippedTrack, UnreadTags, assemble_library, assemble_library_with,
    playlist_count, playlists_from_folders, render_corrections_jsonl, render_m3u8,
    write_corrections_file, write_m3u8_file,
};
pub use harmony::{
    MixMatch, MixOptions, MixTrack, Relation, TempoFit, TempoScaling, TrackFeatures, mixes_into,
};
pub use key::{Key, Mode};
pub use library::{SUPPORTED_EXTENSIONS, find_audio_files, is_supported_audio_file};
pub use playlists::{
    EntryCheck, EntryLocation, PlaylistEntry, PlaylistStore, StoredPlaylist,
    default_playlists_path, locate_entry, locate_moved_files,
};
pub use tempo_range::TempoRange;
pub use track_cues::{Cue, CueOrigin, HotCue, TrackCues, check_cues};
pub use track_name::{TrackName, read_track_name};
