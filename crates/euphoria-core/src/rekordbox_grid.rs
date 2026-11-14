// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The beat grid a DJ already fixed in rekordbox, read from the collection
//! exported as XML, to use in place of the one Euphoria detected.
//!
//! rekordbox writes a grid as `TEMPO` elements inside each `TRACK`, as
//! described in Pioneer DJ's "rekordbox xml Format List", published on the
//! developer page of rekordbox.com:
//!
//! - `Inizio`: the position of the marker in seconds from the start of the
//!   track.
//! - `Bpm`: the tempo from that marker on.
//! - `Metro`: the meter, such as `4/4`.
//! - `Battito`: the beat of the bar the marker sits on, from 1.
//!
//! A track has one marker for each change of tempo. Euphoria's
//! [`BeatGrid`] has one tempo, so only a grid whose markers all lie on one
//! constant grid is used. Any other is reported, with why, and left out.
//!
//! Who wins when grids disagree is decided by [`choose_grid`]: a tempo the
//! user set beats the rekordbox grid, which beats the detected one.
//!
//! This has been tested against XML written to that description, not
//! against a collection exported by rekordbox itself.

use std::path::{Path, PathBuf};

use crate::analysis::TrackAnalysis;
use crate::audio::{AudioBuffer, decode_mono};
use crate::correction::Correction;
use crate::detect::{BeatGrid, cues_on_grid};
use crate::error::EuphoriaError;

const LOCATION_PREFIX: &str = "file://localhost";
const BEATS_PER_BAR: f64 = 4.0;
/// How far a marker may lie from the grid of the first marker, as a share
/// of a beat, and still count as on it. rekordbox writes positions to
/// milliseconds, so a marker on the grid is off by far less.
const MARKER_TOLERANCE_BEATS: f64 = 0.02;

/// Where the beat grid of a track comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridSource {
    /// Euphoria detected it, or the track has none.
    Detected,
    /// It is the grid of the rekordbox collection.
    Rekordbox,
    /// The user set the tempo, so no grid was fitted to a different one.
    User,
}

impl GridSource {
    /// The word for the source shown to people and written in JSON.
    pub fn label(self) -> &'static str {
        match self {
            GridSource::Detected => "detected",
            GridSource::Rekordbox => "rekordbox",
            GridSource::User => "you",
        }
    }
}

/// Why a grid of several markers is not one constant grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableReason {
    /// The markers have different tempos.
    TempoChanges,
    /// The markers have one tempo, but a later one is not on the beats
    /// counted from the first.
    GridShift,
}

/// What a rekordbox collection says about the beat grid of one track.
#[derive(Debug, Clone, PartialEq)]
pub enum RekordboxGrid {
    /// One tempo from the first marker to the end, in 4/4. Safe to use.
    Constant(BeatGrid),
    /// The grid changes tempo or shifts. Not used.
    Variable {
        /// How many markers the track has.
        markers: usize,
        /// The slowest tempo of the markers.
        slowest_bpm: f64,
        /// The fastest tempo of the markers.
        fastest_bpm: f64,
        /// How the markers differ.
        reason: VariableReason,
    },
    /// A marker in a meter other than 4/4, which a bar of Euphoria's grid
    /// cannot express. Not used.
    UnsupportedMeter {
        /// The meter as rekordbox wrote it.
        meter: String,
    },
    /// A marker whose position, tempo or beat is missing or not a number.
    /// Not used.
    Unreadable,
    /// The track has no beat grid in the collection.
    Missing,
}

impl RekordboxGrid {
    /// The grid to use, when this one can be.
    pub fn usable(&self) -> Option<BeatGrid> {
        match self {
            RekordboxGrid::Constant(grid) => Some(*grid),
            _ => None,
        }
    }
}

/// A track of the collection with what it says about the beat grid.
#[derive(Debug, Clone, PartialEq)]
pub struct CollectionGrid {
    /// The local file the track is, when its location is a local file URL.
    pub path: Option<PathBuf>,
    /// The track's title, artist and location, to name it to the user.
    pub description: String,
    /// What the collection says about its beat grid.
    pub grid: RekordboxGrid,
}

/// Why a file is not a collection that can be read.
#[derive(Debug, thiserror::Error)]
pub enum CollectionError {
    /// The file is not well formed XML.
    #[error("it is not well formed XML")]
    NotXml(#[source] roxmltree::Error),
    /// The file is XML, but not a rekordbox collection.
    #[error("it is not a rekordbox XML collection: it has no DJ_PLAYLISTS and COLLECTION")]
    NotACollection,
}

/// Reads the beat grid of every track of a rekordbox collection exported as
/// XML, in the order of the collection.
///
/// # Errors
///
/// [`CollectionError`] when `text` is not a rekordbox collection.
pub fn read_collection_grids(text: &str) -> Result<Vec<CollectionGrid>, CollectionError> {
    let document = roxmltree::Document::parse(text).map_err(CollectionError::NotXml)?;
    let root = document.root_element();
    let collection = root
        .has_tag_name("DJ_PLAYLISTS")
        .then(|| root.children().find(|node| node.has_tag_name("COLLECTION")))
        .flatten()
        .ok_or(CollectionError::NotACollection)?;
    Ok(collection
        .children()
        .filter(|node| node.has_tag_name("TRACK"))
        .map(|track| {
            let location = track.attribute("Location");
            CollectionGrid {
                path: location.and_then(rekordbox_location_path),
                description: format!(
                    "{} by {} at {}",
                    track.attribute("Name").unwrap_or("(no title)"),
                    track.attribute("Artist").unwrap_or("(no artist)"),
                    location.unwrap_or("(no location)"),
                ),
                grid: grid_of_markers(
                    track
                        .children()
                        .filter(|node| node.has_tag_name("TEMPO"))
                        .map(|marker| Marker::read(&marker)),
                ),
            }
        })
        .collect())
}

#[derive(Debug, Clone, PartialEq)]
struct Marker {
    seconds: f64,
    bpm: f64,
    meter: String,
    beat: f64,
}

impl Marker {
    fn read(node: &roxmltree::Node<'_, '_>) -> Option<Self> {
        let number = |name: &str| {
            node.attribute(name)?
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
        };
        let bpm = number("Bpm").filter(|bpm| *bpm > 0.0)?;
        let beat = number("Battito").filter(|beat| (1.0..=BEATS_PER_BAR).contains(beat))?;
        Some(Self {
            seconds: number("Inizio")?,
            bpm,
            meter: node.attribute("Metro")?.trim().to_string(),
            beat,
        })
    }

    fn first_downbeat_seconds(&self) -> f64 {
        self.seconds - (self.beat - 1.0) * 60.0 / self.bpm
    }
}

fn grid_of_markers(markers: impl Iterator<Item = Option<Marker>>) -> RekordboxGrid {
    let Some(markers): Option<Vec<Marker>> = markers.collect() else {
        return RekordboxGrid::Unreadable;
    };
    let Some(first) = markers.first() else {
        return RekordboxGrid::Missing;
    };
    if let Some(odd) = markers.iter().find(|marker| marker.meter != "4/4") {
        return RekordboxGrid::UnsupportedMeter {
            meter: odd.meter.clone(),
        };
    }
    let slowest_bpm = markers
        .iter()
        .map(|marker| marker.bpm)
        .fold(f64::INFINITY, f64::min);
    let fastest_bpm = markers
        .iter()
        .map(|marker| marker.bpm)
        .fold(f64::NEG_INFINITY, f64::max);
    let variable = |reason| RekordboxGrid::Variable {
        markers: markers.len(),
        slowest_bpm,
        fastest_bpm,
        reason,
    };
    if slowest_bpm != fastest_bpm {
        return variable(VariableReason::TempoChanges);
    }
    let beat_seconds = 60.0 / first.bpm;
    let off_the_grid = markers.iter().skip(1).any(|marker| {
        let beats = (marker.seconds - first.seconds) / beat_seconds;
        let counted = beats + (first.beat - marker.beat);
        let bar_offset = counted - (counted / BEATS_PER_BAR).round() * BEATS_PER_BAR;
        (beats - beats.round()).abs() > MARKER_TOLERANCE_BEATS
            || bar_offset.abs() > MARKER_TOLERANCE_BEATS
    });
    if off_the_grid {
        return variable(VariableReason::GridShift);
    }
    RekordboxGrid::Constant(BeatGrid::imported(
        first.first_downbeat_seconds(),
        first.bpm,
    ))
}

/// The grid a track is given.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GridChoice {
    /// The tempo was set by the user, who needs no grid from elsewhere.
    User,
    /// The grid of the rekordbox collection.
    Rekordbox(BeatGrid),
    /// The one Euphoria detected.
    Detected,
}

impl GridChoice {
    /// The source of the grid this choice gives a track.
    pub fn source(self) -> GridSource {
        match self {
            GridChoice::User => GridSource::User,
            GridChoice::Rekordbox(_) => GridSource::Rekordbox,
            GridChoice::Detected => GridSource::Detected,
        }
    }
}

/// Decides whose grid a track gets: the user's correction beats the
/// rekordbox grid, which beats the detected one.
///
/// A correction of the tempo is the only correction of the grid, and it
/// drops the detected grid (see [`Correction::apply`]), so the rekordbox
/// grid is not brought back over it. A correction of the key alone leaves
/// the choice to the collection.
pub fn choose_grid(correction: &Correction, rekordbox: Option<&RekordboxGrid>) -> GridChoice {
    if correction.bpm.is_some() {
        return GridChoice::User;
    }
    match rekordbox.and_then(RekordboxGrid::usable) {
        Some(grid) => GridChoice::Rekordbox(grid),
        None => GridChoice::Detected,
    }
}

impl TrackAnalysis {
    /// This analysis with `grid` as its beat grid and tempo, and its cues
    /// placed on the bars of `grid` in `audio`.
    ///
    /// The tempo is reported as the grid has it, whatever the tempo range,
    /// and is no longer rated for confidence: the DJ set it.
    pub fn on_imported_grid(mut self, grid: BeatGrid, audio: &AudioBuffer) -> Self {
        self.bpm = Some(grid.bpm as f32);
        self.tempo_estimate = None;
        self.tempo_score_margin = None;
        self.cues = cues_on_grid(audio, &grid);
        self.beat_grid = Some(grid);
        self
    }

    /// [`TrackAnalysis::on_imported_grid`] for the audio file at `path`,
    /// which is decoded again to place the cues.
    ///
    /// # Errors
    ///
    /// [`EuphoriaError::Decode`] when the file cannot be decoded.
    pub fn on_imported_grid_of_file(
        self,
        grid: BeatGrid,
        path: &Path,
    ) -> Result<Self, EuphoriaError> {
        Ok(self.on_imported_grid(grid, &decode_mono(path)?))
    }
}

/// The path of a `file://localhost/` URL as rekordbox writes it, with its
/// percent escapes decoded: `/Users/dj/a.mp3` for a Mac location, and
/// `C:/Music/a.mp3` for a Windows one. `None` for any other URL.
pub fn rekordbox_location_path(location: &str) -> Option<PathBuf> {
    let encoded = location.strip_prefix(LOCATION_PREFIX)?;
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut rest = encoded.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        if byte == b'%' {
            let hex = std::str::from_utf8(after.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &after[2..];
        } else {
            bytes.push(byte);
            rest = after;
        }
    }
    let path = String::from_utf8(bytes).ok()?;
    let without_slash = path.strip_prefix('/')?;
    let drive = without_slash.as_bytes();
    let has_drive_letter =
        drive.len() >= 3 && drive[0].is_ascii_alphabetic() && drive[1] == b':' && drive[2] == b'/';
    Some(PathBuf::from(if has_drive_letter {
        without_slash
    } else {
        &path
    }))
}
