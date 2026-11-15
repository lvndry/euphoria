// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rendering a set as one continuous mix: every track in play order, each
//! blended into the next over a window the transition planner chose, with
//! the incoming track time-stretched to the tempo of the one playing, its
//! pitch kept, and the bass handed over from one to the other.
//!
//! [`plan_mix`] picks the windows, [`mix_timeline`] lays the tracks on the
//! time of the mix, [`render_mix`] writes it a block at a time, and
//! [`cue_sheet`], [`tracklist`] and [`chapters`] describe where each track
//! starts.

mod plan;
mod render;
mod sheets;
mod timeline;
mod wav;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use crate::detect::BeatGrid;

pub use plan::{MixInput, SEARCHED_WINDOWS, plan_mix};
pub use render::{
    MIX_CEILING, MixFormat, NoProgress, RenderOptions, RenderProgress, RenderedMix, render_mix,
};
pub use sheets::{chapters, cue_sheet, format_mix_time, tracklist};
pub use timeline::{Crossfade, MixTimeline, Placement, TEMPO_RAMP_SECONDS, mix_timeline};

/// A track of the mix.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderTrack {
    /// The audio file. It is only read.
    pub path: PathBuf,
    /// The artist from the tags, when they name one.
    pub artist: Option<String>,
    /// The title from the tags, when they give one.
    pub title: Option<String>,
    /// The length of the track, in seconds.
    pub duration_seconds: f64,
    /// The beat grid, or `None` when the track has none: it is then played
    /// after the track before it ends, not blended.
    pub grid: Option<BeatGrid>,
}

impl RenderTrack {
    /// "Artist - Title" from the tags, the title alone without an artist,
    /// and the file name when the tags say nothing.
    pub fn display_name(&self) -> String {
        match (&self.artist, &self.title) {
            (Some(artist), Some(title)) => format!("{artist} - {title}"),
            (None, Some(title)) => title.clone(),
            _ => file_stem(&self.path),
        }
    }

    /// The title, or the file name when the tags give none.
    pub fn title_or_file_name(&self) -> String {
        self.title.clone().unwrap_or_else(|| file_stem(&self.path))
    }
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// A blend of one track into the next, in the bars of each track.
pub use crate::locked_transitions::BlendBars as Blend;

/// How one track goes into the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixMove {
    /// The two play together over a window.
    Blend(Blend),
    /// The first plays to its end and the next starts from its beginning.
    Sequence,
}

/// The move from one track into the next, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct MixTransition {
    /// What happens.
    pub mix: MixMove,
    /// Why, in one line.
    pub reason: String,
}

/// The tracks of a mix in play order, and the transition between each and
/// the next.
#[derive(Debug, Clone, PartialEq)]
pub struct MixPlan {
    /// The tracks, in order.
    pub tracks: Vec<RenderTrack>,
    /// One fewer than the tracks: the move from each track into the next.
    pub transitions: Vec<MixTransition>,
}
