// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Ordering a pool of tracks into a set that follows an energy curve, with
//! every transition in a compatible key and a small tempo step.

mod curve;
mod solve;

use super::mix::MixTrack;
use super::relation::Relation;
use crate::key::Key;

pub use curve::{CurvePreset, EnergyCurve, HIGHEST_ENERGY, LOWEST_ENERGY, normalised_position};

/// The largest tempo step between two tracks, in beats per minute, that
/// [`SetOptions::new`] accepts without counting it as a deviation.
pub const DEFAULT_MAX_BPM_STEP: f32 = 5.0;

/// How far, in energy points, a track may be from the curve before the plan
/// reports it, in [`SetOptions::new`].
pub const DEFAULT_ENERGY_TOLERANCE: f32 = 1.0;

/// How many partial sets the search keeps at each step, in
/// [`SetOptions::new`].
pub const DEFAULT_BEAM_WIDTH: usize = 256;

/// The most usable tracks a pool may hold. The search keeps a cost for
/// every pair of tracks, so its memory grows with the square of the pool:
/// 64 MB at this size.
pub const MAX_POOL_SIZE: usize = 4_000;

/// The key moves [`SetOptions::new`] allows: same, adjacent, relative and
/// diagonal keys, and the energy boost of two steps up.
pub const DEFAULT_SET_RELATIONS: [Relation; 5] = [
    Relation::Same,
    Relation::Adjacent,
    Relation::Relative,
    Relation::Diagonal,
    Relation::EnergyBoost,
];

/// What the set builder needs to know about a track: its key, tempo and
/// energy, and its artist when the tags name one.
pub trait SetTrack: MixTrack {
    /// The artist, or `None` when the tags do not name one.
    fn artist(&self) -> Option<&str>;
}

impl<Track: SetTrack + ?Sized> SetTrack for &Track {
    fn artist(&self) -> Option<&str> {
        (**self).artist()
    }
}

/// Where a pinned track goes in the set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PinPosition {
    /// At this position, counted from 0 for the first track.
    At(usize),
    /// Last, whatever the length of the set.
    Last,
}

/// A track of the pool that must be at one position of the set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pin {
    /// The index of the track in the pool.
    pub track: usize,
    /// Where it goes.
    pub position: PinPosition,
}

/// What the set should be like.
#[derive(Debug, Clone, PartialEq)]
pub struct SetOptions {
    /// How many tracks the set has. A pool with fewer usable tracks gives a
    /// shorter set, and the plan says so.
    pub track_count: usize,
    /// The energy the set should follow.
    pub curve: EnergyCurve,
    /// The key moves allowed between two tracks.
    pub relations: Vec<Relation>,
    /// The largest tempo step allowed between two tracks, in beats per
    /// minute.
    pub max_bpm_step: f32,
    /// Whether two tracks by the same artist may follow each other. Tracks
    /// whose artist is unknown never count as repeats.
    pub allow_repeated_artists: bool,
    /// Tracks that must be at a given position.
    pub pins: Vec<Pin>,
    /// How far, in energy points, a track may be from the curve before the
    /// plan reports it as a deviation. It does not change the search, which
    /// always gets as close to the curve as it can.
    pub energy_tolerance: f32,
    /// How many partial sets the search keeps at each step. Wider searches
    /// find better sets more slowly.
    pub beam_width: usize,
}

impl SetOptions {
    /// A set of `track_count` tracks following `curve`, with the default
    /// key moves, tempo step and tolerance, no artist twice in a row and no
    /// pinned tracks.
    pub fn new(track_count: usize, curve: EnergyCurve) -> Self {
        Self {
            track_count,
            curve,
            relations: DEFAULT_SET_RELATIONS.to_vec(),
            max_bpm_step: DEFAULT_MAX_BPM_STEP,
            allow_repeated_artists: false,
            pins: Vec::new(),
            energy_tolerance: DEFAULT_ENERGY_TOLERANCE,
            beam_width: DEFAULT_BEAM_WIDTH,
        }
    }
}

/// Why a set cannot be built at all.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SetError {
    /// The curve has no levels.
    #[error("the energy curve has no levels")]
    EmptyCurve,
    /// The curve is neither a preset name nor numbers separated by commas.
    #[error(
        "cannot read the energy curve {text:?}: give numbers from 1 to 10 separated by commas, such as 4,6,8,9,7,5, or a preset: warm-up, peak-time or journey"
    )]
    UnreadableCurve {
        /// The text given.
        text: String,
    },
    /// A level of the curve is outside the energy scale.
    #[error("the energy curve level {level} is outside the energy scale of 1 to 10")]
    CurveOutOfRange {
        /// The level outside the scale.
        level: f32,
    },
    /// The set would have no tracks.
    #[error("a set needs at least one track")]
    NoTracks,
    /// No track of the pool has a key, a tempo and an energy.
    #[error("no track of the pool has a key, a tempo and an energy, so there is nothing to order")]
    NothingUsable,
    /// The pool has more usable tracks than [`MAX_POOL_SIZE`].
    #[error(
        "the pool has {size} tracks with a key, a tempo and an energy, more than the {limit} a set can be chosen from; choose a folder or a playlist"
    )]
    PoolTooLarge {
        /// The usable tracks in the pool.
        size: usize,
        /// [`MAX_POOL_SIZE`].
        limit: usize,
    },
    /// A pin or an order names a track that is not in the pool.
    #[error("the track {track} is not in the pool")]
    PinnedTrackMissing {
        /// The index given.
        track: usize,
    },
    /// A pinned or ordered track lacks a key, a tempo or an energy.
    #[error("the track {track} has no {missing}, so it cannot be placed")]
    PinnedTrackUnusable {
        /// The index of the track in the pool.
        track: usize,
        /// What it lacks.
        missing: MissingFeature,
    },
    /// A pin is past the end of the set.
    #[error("a track is pinned at position {position}, past the end of a set of {track_count}")]
    PinPastTheEnd {
        /// The position, counted from 0.
        position: usize,
        /// The length of the set.
        track_count: usize,
    },
    /// Two pins share a position or a track, or an order names a track
    /// twice.
    #[error("track {track} is placed twice, or two tracks at position {position}")]
    ConflictingPins {
        /// The position, counted from 0.
        position: usize,
        /// The index of the track in the pool.
        track: usize,
    },
}

/// What a track of the pool lacks to be placed in a set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MissingFeature {
    /// It has no detected or set key.
    Key,
    /// It has no tempo.
    Tempo,
    /// It has no energy rating.
    Energy,
}

impl MissingFeature {
    /// The name of what is missing, such as "key".
    pub fn name(self) -> &'static str {
        match self {
            MissingFeature::Key => "key",
            MissingFeature::Tempo => "tempo",
            MissingFeature::Energy => "energy",
        }
    }
}

impl std::fmt::Display for MissingFeature {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.name())
    }
}

/// A track of the pool left out of the search because it lacks something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LeftOut {
    /// The index of the track in the pool.
    pub track: usize,
    /// What it lacks, the first of key, tempo and energy.
    pub missing: MissingFeature,
}

/// One track of a planned set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlannedTrack {
    /// The index of the track in the pool.
    pub track: usize,
    /// Where it falls in the set, from 0 for the first track to 1 for the
    /// last.
    pub position: f32,
    /// The energy the curve asks for here.
    pub target_energy: f32,
    /// The energy of the track.
    pub energy: f32,
    /// The key of the track.
    pub key: Key,
    /// The tempo of the track in beats per minute.
    pub bpm: f32,
    /// Whether the track was pinned here.
    pub pinned: bool,
}

impl PlannedTrack {
    /// The energy of the track minus the energy the curve asks for.
    pub fn energy_error(&self) -> f32 {
        self.energy - self.target_energy
    }
}

/// How one track of the set leads into the next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transition {
    /// The key move from the earlier track to the later one, or `None` when
    /// the move has no name in harmonic mixing.
    pub relation: Option<Relation>,
    /// Whether the key move is one of [`SetOptions::relations`].
    pub key_allowed: bool,
    /// The later tempo minus the earlier one, in beats per minute.
    pub bpm_change: f32,
    /// The tempo change in percent of the earlier tempo.
    pub bpm_change_percent: f32,
    /// Whether the tempo step is within [`SetOptions::max_bpm_step`].
    pub tempo_allowed: bool,
    /// Whether both tracks are by the same artist.
    pub same_artist: bool,
}

impl Transition {
    /// Whether the transition keeps every rule of the options.
    pub fn is_clean(&self, allow_repeated_artists: bool) -> bool {
        self.key_allowed && self.tempo_allowed && (allow_repeated_artists || !self.same_artist)
    }
}

/// Where a planned set falls short of what was asked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Deviation {
    /// The pool has fewer usable tracks than the set asked for.
    ShortPool {
        /// The length asked for.
        requested: usize,
        /// The usable tracks in the pool, the length of the set.
        available: usize,
    },
    /// A track is further from the curve than the tolerance.
    EnergyOffTarget {
        /// The position of the track in the set, counted from 0.
        position: usize,
        /// The energy the curve asks for.
        target: f32,
        /// The energy of the track.
        actual: f32,
    },
    /// The move into the track at `position` is not an allowed key move.
    KeyClash {
        /// The position of the later track, counted from 0.
        position: usize,
        /// The move, or `None` when it has no name.
        relation: Option<Relation>,
    },
    /// The tempo step into the track at `position` is larger than allowed.
    TempoJump {
        /// The position of the later track, counted from 0.
        position: usize,
        /// The tempo change in beats per minute.
        bpm_change: f32,
    },
    /// The track at `position` is by the same artist as the one before it.
    RepeatedArtist {
        /// The position of the later track, counted from 0.
        position: usize,
    },
}

/// An ordered set, with how each track meets the curve and how each
/// transition mixes.
#[derive(Debug, Clone, PartialEq)]
pub struct SetPlan {
    /// The tracks in order.
    pub tracks: Vec<PlannedTrack>,
    /// How each track leads into the next: one fewer than the tracks.
    pub transitions: Vec<Transition>,
    /// Where the set falls short of the curve or the rules, in order of
    /// position. Empty when the set does all that was asked.
    pub deviations: Vec<Deviation>,
    /// The tracks of the pool left out of the search.
    pub left_out: Vec<LeftOut>,
    /// The cost the search minimised: the squared energy errors, the key
    /// and tempo steps, and a large penalty for each broken rule.
    pub cost: f64,
    /// A cost no set can go below, found by allowing tracks to repeat. The
    /// closer `cost` is to it, the nearer the set is to the best possible.
    pub lower_bound: f64,
}

impl SetPlan {
    /// The mean distance of the tracks' energy from the curve.
    pub fn mean_energy_error(&self) -> f32 {
        if self.tracks.is_empty() {
            return 0.0;
        }
        self.tracks
            .iter()
            .map(|track| track.energy_error().abs())
            .sum::<f32>()
            / self.tracks.len() as f32
    }

    /// Whether every transition keeps the key, tempo and artist rules.
    pub fn every_transition_is_clean(&self) -> bool {
        !self.deviations.iter().any(|deviation| {
            matches!(
                deviation,
                Deviation::KeyClash { .. }
                    | Deviation::TempoJump { .. }
                    | Deviation::RepeatedArtist { .. }
            )
        })
    }
}

/// Orders tracks of `pool` into a set that follows `options.curve` as
/// closely as it can while every transition is an allowed key move, a
/// tempo step within `options.max_bpm_step` and, unless allowed, a change
/// of artist.
///
/// Tracks without a key, a tempo or an energy are left out and listed in
/// [`SetPlan::left_out`]. When no set keeps every rule, or none follows the
/// curve within the tolerance, the plan is the best compromise found and
/// [`SetPlan::deviations`] says where it falls short. The result depends
/// only on the arguments, never on timing or threads.
///
/// The search is a beam search guided by an exact dynamic programme over
/// the relaxed problem in which tracks may repeat: the programme gives, for
/// every track at every position, the lowest cost of finishing the set from
/// there, so the beam ranks partial sets by their cost so far plus that
/// bound, and the bound of the whole set is reported in
/// [`SetPlan::lower_bound`]. A final pass of swaps and replacements polishes
/// the best set found.
pub fn build_set<Track: SetTrack>(
    pool: &[Track],
    options: &SetOptions,
) -> Result<SetPlan, SetError> {
    solve::build(pool, options)
}

/// Explains a set in the order given, as indices into `pool`: how each
/// track meets `options.curve` and how each transition mixes, with the same
/// deviations as [`build_set`]. Use it after reordering a built set by hand.
///
/// `options.track_count` is ignored, since the order sets the length, and a
/// track counts as pinned when one of `options.pins` puts it where it is.
/// The plan's `lower_bound` is its own `cost`, since the order is fixed.
/// A track named twice, missing from the pool, or lacking a key, a tempo or
/// an energy is an error.
pub fn evaluate_set<Track: SetTrack>(
    pool: &[Track],
    order: &[usize],
    options: &SetOptions,
) -> Result<SetPlan, SetError> {
    solve::evaluate(pool, order, options)
}
