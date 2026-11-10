//! Product logic for Euphoria: decoding, key and tempo detection, harmony.
//! This crate has no user interface and prints nothing.

/// Decoding audio files to mono samples.
pub mod audio;
/// Key and tempo detectors.
pub mod detect;
/// The error type returned by the library.
pub mod error;
/// Musical keys and Camelot notation.
pub mod key;

pub use audio::{AudioBuffer, decode_mono};
pub use error::EuphoriaError;
pub use key::{Key, Mode};
