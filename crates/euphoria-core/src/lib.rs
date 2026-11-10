// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Product logic for Euphoria: decoding, key and tempo detection, energy
//! rating, harmony.
//! This crate has no user interface and prints nothing.

/// Decoding audio files to mono samples.
pub mod audio;
/// Key and tempo detectors.
pub mod detect;
/// Energy rating from 1 to 10 and the features behind it.
pub mod energy;
/// The error type returned by the library.
pub mod error;
/// Musical keys and Camelot notation.
pub mod key;

pub use audio::{AudioBuffer, decode_mono};
pub use energy::{
    ENERGY_CURVE_HOP_SECONDS, ENERGY_CURVE_WINDOW_SECONDS, ENERGY_MODEL, ENERGY_MODEL_VERSION,
    EnergyCurvePoint, EnergyFeatures, EnergyModel, EnergyRating, FeatureAnchor, energy_curve,
    energy_features, rate_energy, rate_features,
};
pub use error::{DecodeFailure, ErrorSource, EuphoriaError};
pub use key::{Key, Mode};
