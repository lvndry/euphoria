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
    /// The path is not valid Unicode, which a format that stores paths as
    /// text cannot hold.
    PathNotUnicode,
    /// A path contains characters XML cannot represent, even as references.
    PathNotXml,
    /// The time an export writes as a last change holds a day, month or
    /// seconds of the day that date does not have.
    InvalidModifiedTime,
}

impl fmt::Display for ExportProblem {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            ExportProblem::NotAbsolutePath => "not an absolute local path",
            ExportProblem::DuplicatePath => "listed more than once",
            ExportProblem::NotInCollection => "in a playlist but not in the collection",
            ExportProblem::InvalidTempo => {
                "tempo is not a positive number at the export's precision"
            }
            ExportProblem::InvalidHotCue => "hot cue pad is out of range",
            ExportProblem::PathNotUnicode => "path is not valid Unicode",
            ExportProblem::PathNotXml => "path contains characters XML cannot represent",
            ExportProblem::InvalidModifiedTime => "the modified time does not name a real date",
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
    /// The tags of a file could not be read.
    #[error("cannot read tags of {path}")]
    TagRead {
        /// The file whose tags could not be read.
        path: PathBuf,
        /// The tag reader's own error.
        source: ErrorSource,
    },
    /// The tags of a file could not be written. The file is left as it was.
    #[error("cannot write tags of {path}")]
    TagWrite {
        /// The file whose tags could not be written.
        path: PathBuf,
        /// The tag writer's own error.
        source: ErrorSource,
    },
    /// The log that keeps the backups of tag writes could not be read or
    /// written. A file whose backup could not be saved was not changed.
    #[error("cannot use the log of tag writes at {path}")]
    TagWriteLog {
        /// The log file, or the folder the logs are kept in.
        path: PathBuf,
        /// What went wrong with it.
        source: ErrorSource,
    },
    /// A tag field does not hold the value a write plan or a backup expects,
    /// so nothing was written.
    #[error("{field} of {path} does not hold the expected value")]
    TagConflict {
        /// The file whose tags changed.
        path: PathBuf,
        /// The tag field that holds an unexpected value.
        field: String,
    },
    /// A track cannot be represented in an export.
    #[error("cannot export {path}: {problem}")]
    Export {
        /// The audio file the export was asked to describe.
        path: PathBuf,
        /// What is wrong with it.
        problem: ExportProblem,
    },
    /// A name, comment or cue name contains characters XML 1.0 cannot
    /// represent, even as references.
    #[error("a name or comment contains characters XML cannot represent")]
    ExportTextNotXml,
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
    /// The playlists database could not be opened, read or written.
    #[error("cannot use the playlists at {path}")]
    Playlists {
        /// The playlists database file.
        path: PathBuf,
        /// What went wrong, usually the database library's own error.
        source: ErrorSource,
    },
    /// Whether the user corrected the file's key or tempo could not be
    /// checked, so the file is left out of a tag write or an export rather
    /// than given the detected values against the user's choice.
    #[error("cannot check whether the key or tempo of {path} was corrected, so it was left out")]
    CorrectionUnchecked {
        /// The audio file.
        path: PathBuf,
        /// Why the check failed: the file or the cache could not be read.
        source: ErrorSource,
    },
    /// The audio has a sample rate outside the range Euphoria analyzes.
    #[error(
        "a sample rate of {sample_rate} Hz is not supported: Euphoria reads audio from {minimum} to {maximum} Hz"
    )]
    UnsupportedSampleRate {
        /// The sample rate of the audio, in samples per second.
        sample_rate: u32,
        /// The lowest supported sample rate.
        minimum: u32,
        /// The highest supported sample rate.
        maximum: u32,
    },
    /// A number given to the energy rating is not usable: not finite, or a
    /// tempo that is not positive.
    #[error("cannot rate energy: {name} is {value}")]
    InvalidEnergyInput {
        /// Which input it was, such as "onset rate" or "tempo".
        name: &'static str,
        /// The value that was given.
        value: f32,
    },
    /// A tempo range that cannot give every track one tempo.
    #[error("cannot use the tempo range {slowest} to {fastest}: {reason}")]
    InvalidTempoRange {
        /// The slowest tempo given.
        slowest: f32,
        /// The fastest tempo given.
        fastest: f32,
        /// What is wrong with the range.
        reason: &'static str,
    },
    /// Text that is not a tempo range, which is written as two numbers
    /// joined by a hyphen, such as `70-140`.
    #[error(
        "cannot read the tempo range {0:?}: write it as two numbers joined by a hyphen, such as 70-140"
    )]
    UnreadableTempoRange(String),
    /// A key or tempo correction that cannot be kept.
    #[error("cannot keep this correction: {reason}")]
    InvalidCorrection {
        /// What is wrong with it.
        reason: &'static str,
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
