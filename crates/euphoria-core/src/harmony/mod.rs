// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Harmonic mixing: how two keys relate on the Camelot wheel, and which
//! tracks mix into a given one.

mod mix;
mod relation;

pub use mix::{
    DEFAULT_MAX_TEMPO_DIFFERENCE_PERCENT, MixMatch, MixOptions, MixTrack, TempoFit, TempoScaling,
    TrackFeatures, mixes_into,
};
pub use relation::Relation;
