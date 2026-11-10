// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;
use std::path::PathBuf;

/// The underlying cause of an error, kept so callers can walk the chain
/// without depending on the libraries the core uses internally.
pub type ErrorSource = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Why a file could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeFailure {
    /// The container or codec is not one the decoder supports.
    UnsupportedFormat,
    /// The file has no audio track.
    NoAudioTrack,
    /// The audio data is damaged or not what the container claims.
    Malformed,
    /// Reading the file failed partway through.
    Read,
}

impl fmt::Display for DecodeFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            DecodeFailure::UnsupportedFormat => "unsupported format",
            DecodeFailure::NoAudioTrack => "no audio track",
            DecodeFailure::Malformed => "malformed audio data",
            DecodeFailure::Read => "read error",
        })
    }
}

/// Why a track could not be written to an export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportProblem {
    /// The path is not an absolute path on a local drive.
    NotAbsolutePath,
    /// The path belongs to more than one track of the collection.
    DuplicatePath,
    /// A playlist names the path but the collection has no such track.
    NotInCollection,
    /// The tempo is not a number, or is below the smallest one an export
    /// can hold.
    InvalidTempo,
    /// A hot cue is on a pad the track does not have.
    InvalidHotCue,
}

impl fmt::Display for ExportProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            ExportProblem::NotAbsolutePath => "not an absolute local path",
            ExportProblem::DuplicatePath => "listed more than once",
            ExportProblem::NotInCollection => "in a playlist but not in the collection",
            ExportProblem::InvalidTempo => "tempo is not a number of at least 0.01",
            ExportProblem::InvalidHotCue => "hot cue pad is out of range",
        })
    }
}

/// Everything that can go wrong in the library.
#[derive(Debug, thiserror::Error)]
pub enum EuphoriaError {
    /// The file could not be opened.
    #[error("cannot open {path}")]
    Open {
        /// The file that failed to open.
        path: PathBuf,
        /// The underlying input or output error.
        source: std::io::Error,
    },
    /// A file or folder could not be read, while looking for audio files or
    /// while identifying one for the analysis cache.
    #[error("cannot read {path}")]
    Scan {
        /// The file or folder that could not be read.
        path: PathBuf,
        /// The underlying input or output error.
        source: std::io::Error,
    },
    /// The file was opened but its audio could not be decoded.
    #[error("cannot decode {path}: {failure}")]
    Decode {
        /// The file that failed to decode.
        path: PathBuf,
        /// What kind of failure it was.
        failure: DecodeFailure,
        /// The decoder's own error, when it gave one.
        #[source]
        source: Option<ErrorSource>,
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
        /// The detector library's own error, when it gave one.
        #[source]
        source: Option<ErrorSource>,
    },
    /// A track cannot be represented in an export.
    #[error("cannot export {path}: {problem}")]
    Export {
        /// The audio file the export was asked to describe.
        path: PathBuf,
        /// What is wrong with it.
        problem: ExportProblem,
    },
    /// The playlist folders of an export are nested too deeply.
    #[error("cannot export folder {folder:?}: nested more than {maximum} folders deep")]
    ExportFolderDepth {
        /// Name of the first folder beyond the limit.
        folder: String,
        /// The deepest level a folder may sit at.
        maximum: usize,
    },
    /// The destination of an export rejected the data.
    #[error("cannot write the export")]
    ExportWrite {
        /// The underlying input or output error.
        source: std::io::Error,
    },
    /// The file an export was to be written to exists and was to be kept.
    #[error("{path} already exists")]
    ExportDestinationExists {
        /// The file that is in the way.
        path: PathBuf,
    },
    /// The file of an export could not be created, written or moved into
    /// place.
    #[error("cannot write {path}")]
    ExportFile {
        /// The file the export was to be written to.
        path: PathBuf,
        /// The underlying input or output error.
        source: std::io::Error,
    },
    /// Text could not be read as a key: a name such as "A minor", a Camelot
    /// code such as "8A" or an Open Key code such as "1m".
    #[error("unrecognised key text: {0:?}")]
    UnknownKeyText(String),
    /// The analysis cache could not be opened, read or written.
    #[error("cannot use the analysis cache at {path}")]
    Cache {
        /// The cache database file.
        path: PathBuf,
        /// What went wrong, usually the database library's own error.
        source: ErrorSource,
    },
    /// No folder for Euphoria's data could be worked out from the environment.
    #[error(
        "cannot find a folder for Euphoria's data: set the EUPHORIA_DATA_DIR environment variable to one"
    )]
    NoDataDirectory,
}

impl EuphoriaError {
    /// The error and every cause beneath it, joined with ": ".
    pub fn chain(&self) -> String {
        let mut description = self.to_string();
        let mut cause = std::error::Error::source(self);
        while let Some(error) = cause {
            description.push_str(": ");
            description.push_str(&error.to_string());
            cause = error.source();
        }
        description
    }
}
