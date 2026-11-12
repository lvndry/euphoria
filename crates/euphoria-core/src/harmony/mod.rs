// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Harmonic mixing: how two keys relate on the Camelot wheel, which tracks
//! mix into a given one, which to play next, and sets ordered along an
//! energy curve.

mod mix;
mod next;
mod relation;
pub mod set;

pub use mix::{
    DEFAULT_MAX_TEMPO_DIFFERENCE_PERCENT, MixMatch, MixOptions, MixTrack, TempoFit, TempoScaling,
    TrackFeatures, mixes_into,
};
pub use next::{EnergyDirection, next_tracks};
pub use relation::Relation;
pub use set::{
    CurvePreset, Deviation, EnergyCurve, LeftOut, MissingFeature, Pin, PinPosition, PlannedTrack,
    SetError, SetOptions, SetPlan, SetTrack, Transition, build_set, evaluate_set,
};
