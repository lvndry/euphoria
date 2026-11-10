//! Product logic for Euphoria: decoding, key and tempo detection, harmony.
//! This crate has no user interface and prints nothing.

pub mod audio;
pub mod detect;
pub mod error;
pub mod key;

pub use audio::{AudioBuffer, decode_mono};
pub use error::EuphoriaError;
pub use key::{Key, Mode};
