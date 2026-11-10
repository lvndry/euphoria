use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum EuphoriaError {
    #[error("cannot open {path}: {source}")]
    Open {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot decode {path}: {reason}")]
    Decode { path: PathBuf, reason: String },
    #[error("{path} contains no audio")]
    EmptyAudio { path: PathBuf },
    #[error("{detector} failed: {reason}")]
    Detection {
        detector: &'static str,
        reason: String,
    },
    #[error("unrecognised key text: {0}")]
    UnknownKeyText(String),
}
