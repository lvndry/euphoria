// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Harmonic mixing: how two keys relate on the Camelot wheel, and which
//! tracks mix into a given one, which to play next, and which layer over
//! it as a mashup.

mod mashup;
mod mix;
mod next;
mod relation;

pub use mashup::{
    DEFAULT_MASHUP_TEMPO_PERCENT, DEFAULT_MAX_KEY_SHIFT, KeyFit, Layering, MashupMatch,
    MashupOptions, MashupReason, MashupTrack, STEM_BAND_EDGES_HZ, STEM_BANDS, Side, StemPresence,
    StemProfile, mashups_for,
};
pub use mix::{
    DEFAULT_MAX_TEMPO_DIFFERENCE_PERCENT, MixMatch, MixOptions, MixTrack, TempoFit, TempoScaling,
    TrackFeatures, mixes_into,
};
pub use next::{EnergyDirection, next_tracks};
pub use relation::Relation;
