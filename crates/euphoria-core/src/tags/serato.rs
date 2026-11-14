// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Serato DJ's cue points and beat grid, which Serato keeps in the audio
//! file's own tags rather than in its library.
//!
//! The layout follows the reverse engineered description in
//! Holzhaus/serato-tags and the reader and writer in Mixxx
//! (`src/track/serato`). Both were made by reading files Serato wrote; no
//! vendor documentation exists.
//!
//! | Data | MP3 and AIFF (ID3v2 `GEOB`) | M4A (freeform atom) | FLAC (Vorbis comment) |
//! | --- | --- | --- | --- |
//! | Cue points, colours | `Serato Markers2` | `com.serato.dj:markersv2` | `SERATO_MARKERS_V2` |
//! | Beat grid | `Serato BeatGrid` | `com.serato.dj:beatgrid` | `SERATO_BEATGRID` |
//! | First 5 cues, older copy | `Serato Markers_` | `com.serato.dj:markers` | none |
//!
//! Writes go through the same plan, backup and undo as every other tag
//! field: [`add_serato_markers`] adds field changes to a [`WritePlan`].
//! Cues the file already has are never removed or moved.

use std::fmt;
use std::time::Duration;

use super::native::{Purpose, TagFile};
use super::{FieldChange, TagFormat, WritePlan, base64};
use crate::analysis::TrackAnalysis;
use crate::confidence::Confidence;
use crate::error::EuphoriaError;
use crate::track_cues::{Cue, TrackCues};

/// The number of hot cue pads Serato DJ shows by default.
pub const SERATO_HOT_CUES: u8 = 8;

/// Serato's default colour of each hot cue pad, as stored in the file:
/// red, orange, blue, yellow, green, magenta, cyan and purple. Taken from
/// Mixxx's Serato track metadata palette.
pub const SERATO_CUE_COLOURS: [[u8; 3]; 8] = [
    [0xCC, 0x00, 0x00],
    [0xCC, 0x88, 0x00],
    [0x00, 0x00, 0xCC],
    [0xCC, 0xCC, 0x00],
    [0x00, 0xCC, 0x00],
    [0xCC, 0x00, 0xCC],
    [0x00, 0xCC, 0xCC],
    [0x88, 0x00, 0xCC],
];

const OBJECT_HEADER: &[u8] = b"application/octet-stream\0\0";
const MARKERS2_NAME: &str = "Serato Markers2";
const BEAT_GRID_NAME: &str = "Serato BeatGrid";
const MARKERS_NAME: &str = "Serato Markers_";
const MARKERS2_VERSION: [u8; 2] = [0x01, 0x01];
const BEAT_GRID_VERSION: [u8; 2] = [0x01, 0x00];
const MARKERS_VERSION: [u8; 2] = [0x02, 0x05];
const MINIMUM_MARKERS2_LENGTH: usize = 470;
/// Serato breaks base64 text into lines of 72 characters, 54 bytes.
const BYTES_PER_LINE: usize = 54;
const MARKERS_CUE_SLOTS: usize = 5;
const MARKERS_ENTRIES: u32 = 14;
const MARKERS_LOOP_TYPE: u8 = 3;
const MARKERS_CUE_TYPE: u8 = 1;
const ID3_MARKERS_ENTRY_LENGTH: usize = 22;
const MP4_MARKERS_ENTRY_LENGTH: usize = 19;
/// The base64 digit Mixxx and Serato add after some fields.
const FILLER_DIGIT: u8 = b'A';

/// What to give Serato for one track.
#[derive(Debug, Clone, PartialEq)]
pub struct SeratoMarkers {
    /// The cue points, in the order of the pads they would like: the first
    /// on pad 1, the second on pad 2 and so on. At most
    /// [`SERATO_HOT_CUES`] are placed.
    pub cues: Vec<SeratoCue>,
    /// A beat grid with one tempo, or `None` to leave the grid alone.
    pub beat_grid: Option<SeratoGrid>,
}

impl SeratoMarkers {
    /// The cues and beat grid of an analysis, as the rekordbox export
    /// takes them: a grid rated low confidence is left out, and so are the
    /// cues, which sit on its bars.
    pub fn from_analysis(analysis: &TrackAnalysis) -> Self {
        Self::of_cues(analysis, None)
    }

    /// The cues and beat grid of a track as a DJ uses them: `edited_cues` is
    /// the set the user set, which wins over the cues analysis placed in
    /// `analysis` even when it is empty, and otherwise analysis places them.
    /// Pass `analysis` with the correction applied: a corrected tempo drops
    /// the grid and the cues analysis placed, which sit on the grid of the
    /// detected tempo, but never the user's. A grid rated low confidence is
    /// left out, and so are the cues analysis placed on its bars. The
    /// user's cues are written with or without a grid, except a cue past
    /// the end of the track and a memory cue, on no hot cue pad, which is
    /// not a Serato cue point.
    pub fn of_cues(analysis: &TrackAnalysis, edited_cues: Option<Vec<Cue>>) -> Self {
        let grid = analysis
            .beat_grid
            .filter(|grid| grid.confidence() != Confidence::Low);
        let cues = match edited_cues {
            Some(cues) => TrackCues::within(cues, f64::from(analysis.duration_seconds))
                .cues
                .iter()
                .filter_map(|cue| {
                    let pad = cue.hot_cue?.pad();
                    Some(SeratoCue::of(
                        Duration::try_from_secs_f64(cue.seconds).ok()?,
                        cue.name.clone(),
                        Some(pad),
                    ))
                })
                .collect(),
            None if grid.is_some() => analysis
                .cues
                .iter()
                .filter_map(|cue| {
                    Some(SeratoCue::of(
                        Duration::try_from_secs_f64(cue.seconds).ok()?,
                        cue.section.name().to_string(),
                        None,
                    ))
                })
                .collect(),
            None => Vec::new(),
        };
        Self {
            cues,
            beat_grid: grid.and_then(|grid| {
                Duration::try_from_secs_f64(grid.first_downbeat_seconds)
                    .ok()
                    .map(|first_beat| SeratoGrid {
                        first_beat,
                        bpm: grid.bpm,
                    })
            }),
        }
    }
}

/// A hot cue to place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeratoCue {
    /// The cue's label.
    pub name: String,
    /// Where it is, from the start of the track.
    pub position: Duration,
    /// The hot cue pad the cue is on, or `None` when the caller did not ask
    /// for one: the writer then takes the next pad in order.
    pub pad: Option<u8>,
}

impl SeratoCue {
    /// A cue at `position`, named `name`, on pad `pad` when the caller asks
    /// for one.
    pub fn of(position: Duration, name: String, pad: Option<u8>) -> Self {
        Self {
            name,
            position,
            pad,
        }
    }
}

/// A beat grid with a constant tempo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeratoGrid {
    /// The first downbeat.
    pub first_beat: Duration,
    /// The tempo in beats per minute.
    pub bpm: f64,
}

/// A cue of Euphoria's and the pad it went to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacedCue {
    /// The cue's label.
    pub name: String,
    /// The pad it is on, 0 for pad 1.
    pub pad: u8,
    /// The pad it wanted, which differs from `pad` when one of your cues
    /// was there.
    pub wanted_pad: u8,
}

/// What happened to the beat grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GridOutcome {
    /// No grid was asked for.
    NotAsked,
    /// The file had no Serato grid, and gets Euphoria's.
    Written,
    /// The file already has the same grid.
    AlreadyThere,
    /// The file has a grid of its own, which is kept. Its first marker's
    /// position in seconds and its last tempo are given.
    KeptYours {
        /// Where the file's grid starts, in seconds.
        position: f32,
        /// The file's grid tempo.
        bpm: f32,
    },
}

/// Why no Serato data was written to a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeratoSkip {
    /// WAV files: Serato is not known to read cue points from their tags.
    Wav,
    /// Ogg Vorbis files, where Serato keeps its data in another layout that
    /// is not worked out well enough to write.
    Ogg,
    /// The file has an ID3v2.2 tag, which has no room for Serato's data.
    OldId3,
    /// The file's Serato data could not be read, so it is left as it is.
    Unreadable {
        /// The field that could not be read.
        field: String,
        /// What is wrong with it.
        reason: String,
    },
}

impl fmt::Display for SeratoSkip {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wav => formatter
                .write_str("no Serato cues or grid: Serato reads none from WAV tags"),
            Self::Ogg => formatter.write_str(
                "no Serato cues or grid: Serato's layout in Ogg files is not known well enough to write",
            ),
            Self::OldId3 => formatter
                .write_str("no Serato cues or grid: its ID3v2.2 tag cannot hold them"),
            Self::Unreadable { field, reason } => write!(
                formatter,
                "no Serato cues or grid: its {} could not be read ({reason}), so it is left as it is",
                serato_field_name(field).unwrap_or(field)
            ),
        }
    }
}

/// What [`add_serato_markers`] did with one file, so it can be told to the
/// user.
#[derive(Debug, Clone, PartialEq)]
pub struct SeratoReport {
    /// Euphoria's cues that were added, with their pads.
    pub placed: Vec<PlacedCue>,
    /// Euphoria's cues that the file already had at the same position.
    pub already_there: usize,
    /// Euphoria's cues left out because every pad holds one of yours.
    pub no_free_pad: Vec<String>,
    /// How many cue points of yours the file has, all kept.
    pub kept: usize,
    /// What happened to the beat grid.
    pub grid: GridOutcome,
    /// Set when nothing was written for Serato, and why.
    pub skipped: Option<SeratoSkip>,
}

impl SeratoReport {
    fn new(grid: GridOutcome) -> Self {
        Self {
            placed: Vec::new(),
            already_there: 0,
            no_free_pad: Vec::new(),
            kept: 0,
            grid,
            skipped: None,
        }
    }

    /// True when your cues or grid kept Euphoria's from their place: a cue
    /// moved to another pad or left out, or a grid of yours kept.
    pub fn has_conflicts(&self) -> bool {
        self.placed.iter().any(|cue| cue.pad != cue.wanted_pad)
            || !self.no_free_pad.is_empty()
            || matches!(self.grid, GridOutcome::KeptYours { .. })
    }

    /// One sentence for each thing the user should know: why nothing was
    /// written for Serato, a cue moved to another pad or left out, or a
    /// grid of yours kept. Empty when everything went where it was meant to.
    pub fn notes(&self) -> Vec<String> {
        let mut notes: Vec<String> = self.skipped.iter().map(ToString::to_string).collect();
        if self.skipped.is_none() && self.grid == GridOutcome::NotAsked {
            let has_cues =
                !self.placed.is_empty() || self.already_there > 0 || !self.no_free_pad.is_empty();
            notes.push(
                if has_cues {
                    "no Serato beat grid: it has none rated medium or high, or you set its tempo"
                } else {
                    "no Serato cues or grid: it has no beat grid rated medium or high, or you \
                     set its tempo, and no cues of yours on a pad"
                }
                .to_string(),
            );
        }
        for cue in &self.placed {
            if cue.pad != cue.wanted_pad {
                notes.push(format!(
                    "{} went to pad {}, since pad {} holds a cue of yours",
                    if cue.name.is_empty() {
                        "a cue"
                    } else {
                        &cue.name
                    },
                    cue.pad + 1,
                    cue.wanted_pad + 1
                ));
            }
        }
        if !self.no_free_pad.is_empty() {
            notes.push(format!(
                "{} left out: every pad holds a cue of yours",
                self.no_free_pad.join(", ")
            ));
        }
        if let GridOutcome::KeptYours { position, bpm } = self.grid {
            notes.push(format!(
                "your Serato beat grid ({bpm:.2} BPM from {position:.3} s) is kept, not replaced"
            ));
        }
        notes
    }
}

/// Where a file keeps Serato's data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Container {
    Id3,
    Mp4,
    Flac,
}

impl Container {
    fn markers2_field(self) -> &'static str {
        match self {
            Container::Id3 => "GEOB:Serato Markers2",
            Container::Mp4 => "----:com.serato.dj:markersv2",
            Container::Flac => "SERATO_MARKERS_V2",
        }
    }

    fn beat_grid_field(self) -> &'static str {
        match self {
            Container::Id3 => "GEOB:Serato BeatGrid",
            Container::Mp4 => "----:com.serato.dj:beatgrid",
            Container::Flac => "SERATO_BEATGRID",
        }
    }

    fn markers_field(self) -> Option<&'static str> {
        match self {
            Container::Id3 => Some("GEOB:Serato Markers_"),
            Container::Mp4 => Some("----:com.serato.dj:markers"),
            Container::Flac => None,
        }
    }
}

/// The name of the Serato data `field` holds, for a field this module
/// writes, such as "cue points" for `SERATO_MARKERS_V2`.
pub fn serato_field_name(field: &str) -> Option<&'static str> {
    [Container::Id3, Container::Mp4, Container::Flac]
        .into_iter()
        .find_map(|container| {
            if field == container.markers2_field() {
                Some("Serato cue points")
            } else if field == container.beat_grid_field() {
                Some("Serato beat grid")
            } else if Some(field) == container.markers_field() {
                Some("Serato cue points, older copy")
            } else {
                None
            }
        })
}

/// A short description of the Serato data in `value`, a value of `field`,
/// for showing a change: the cues it holds or its grid. `None` when the
/// field is not Serato's or the value cannot be read.
pub fn describe_serato_value(field: &str, value: &str) -> Option<String> {
    let container = [Container::Id3, Container::Mp4, Container::Flac]
        .into_iter()
        .find(|container| {
            field == container.markers2_field()
                || field == container.beat_grid_field()
                || Some(field) == container.markers_field()
        })?;
    if field == container.markers2_field() {
        let markers = Markers2::from_value(container, value).ok()?;
        let cues: Vec<String> = markers
            .cues()
            .iter()
            .map(|cue| {
                let label = if cue.name.is_empty() {
                    String::new()
                } else {
                    format!(" {}", cue.name)
                };
                format!(
                    "{}{label} at {}",
                    cue.pad + 1,
                    clock(Duration::from_millis(u64::from(cue.position)))
                )
            })
            .collect();
        return Some(if cues.is_empty() {
            "no cue points".to_string()
        } else {
            format!("cue points {}", cues.join(", "))
        });
    }
    if field == container.beat_grid_field() {
        let grid = BeatGrid::from_value(container, value).ok()?;
        return Some(match (grid.markers.first(), grid.markers.last()) {
            (Some(first), Some(GridMarker::Terminal { bpm, .. })) => format!(
                "beat grid from {:.3} s at {bpm:.2} BPM{}",
                first.position(),
                if grid.markers.len() > 1 {
                    format!(", {} markers", grid.markers.len())
                } else {
                    String::new()
                }
            ),
            _ => "no beat grid".to_string(),
        });
    }
    let markers = OldMarkers::from_value(container, value).ok()?;
    let set = markers
        .cue_slots()
        .iter()
        .filter(|slot| slot.is_some())
        .count();
    Some(format!("{set} of the first 5 cue points"))
}

fn clock(position: Duration) -> String {
    let milliseconds = position.as_millis();
    format!(
        "{}:{:02}.{:03}",
        milliseconds / 60_000,
        milliseconds / 1000 % 60,
        milliseconds % 1000
    )
}

/// Adds to `plan` the changes that give the file Serato's copy of
/// `markers`, and says what was done.
///
/// Every cue point, loop, flip, colour and lock the file already has stays
/// as it is. Euphoria's cues go on free pads only: a cue whose pad holds one
/// of yours goes to the next free pad, and one that finds no free pad is
/// left out. A cue at the same position as one the file has, to the
/// millisecond, is not added again, so writing twice changes nothing. The
/// beat grid is written only to a file without one. When the file also has
/// Serato's older `Markers_` copy of the first five cues, which Serato
/// prefers, Euphoria's cues on pads 1 to 5 are added there too, and an M4A
/// file gets that copy when it has none, since Serato reads the first five
/// cues of an M4A file only from it.
///
/// Nothing is added for WAV files, ID3v2.2 tags or Serato data that cannot
/// be read; the report says why.
///
/// # Errors
///
/// Returns the errors of reading the file's tag, or
/// [`EuphoriaError::TagWrite`] when a grid's tempo is not a positive number.
pub fn add_serato_markers(
    plan: &mut WritePlan,
    markers: &SeratoMarkers,
) -> Result<SeratoReport, EuphoriaError> {
    let grid_asked = if markers.beat_grid.is_some() {
        GridOutcome::Written
    } else {
        GridOutcome::NotAsked
    };
    if let Some(grid) = markers.beat_grid
        && !(grid.bpm.is_finite() && grid.bpm > 0.0)
    {
        return Err(EuphoriaError::TagWrite {
            path: plan.path.clone(),
            source: format!("a beat grid at {} BPM cannot be written", grid.bpm).into(),
        });
    }
    let file = TagFile::load(&plan.path, Purpose::Rewriting)?;
    let mut report = SeratoReport::new(grid_asked);
    let container = match file.format() {
        TagFormat::Id3v2 if file.has_info_list() => {
            report.skipped = Some(SeratoSkip::Wav);
            return Ok(report);
        }
        TagFormat::Id3v2 if file.id3_version() == Some(2) => {
            report.skipped = Some(SeratoSkip::OldId3);
            return Ok(report);
        }
        TagFormat::VorbisComments if file.is_ogg() => {
            report.skipped = Some(SeratoSkip::Ogg);
            return Ok(report);
        }
        TagFormat::Id3v2 => Container::Id3,
        TagFormat::Mp4 => Container::Mp4,
        TagFormat::VorbisComments => Container::Flac,
    };

    let unreadable = |field: &str, reason: String| SeratoSkip::Unreadable {
        field: field.to_string(),
        reason,
    };
    let markers2_field = container.markers2_field();
    let current_markers2 = file.get(markers2_field)?;
    let mut markers2 = match &current_markers2 {
        Some(value) => match Markers2::from_value(container, value) {
            Ok(markers2) => markers2,
            Err(reason) => {
                report.skipped = Some(unreadable(markers2_field, reason));
                return Ok(report);
            }
        },
        None => Markers2::new(),
    };
    let markers_field = container.markers_field();
    let current_markers = match markers_field {
        Some(field) => file.get(field)?,
        None => None,
    };
    let mut old_markers = match (&current_markers, markers_field) {
        (Some(value), Some(field)) => match OldMarkers::from_value(container, value) {
            Ok(old_markers) => Some(old_markers),
            Err(reason) => {
                report.skipped = Some(unreadable(field, reason));
                return Ok(report);
            }
        },
        _ => None,
    };
    let beat_grid_field = container.beat_grid_field();
    let current_grid = file.get(beat_grid_field)?;
    let existing_grid = match &current_grid {
        Some(value) => match BeatGrid::from_value(container, value) {
            Ok(grid) => Some(grid),
            Err(reason) => {
                report.skipped = Some(unreadable(beat_grid_field, reason));
                return Ok(report);
            }
        },
        None => None,
    };

    // The cues the file has, from both copies.
    let existing_cues = markers2.cues();
    let mut taken = [false; SERATO_HOT_CUES as usize];
    let mut taken_positions: Vec<u32> = Vec::new();
    for cue in &existing_cues {
        if let Some(slot) = taken.get_mut(usize::from(cue.pad)) {
            *slot = true;
        }
        taken_positions.push(cue.position);
    }
    if let Some(old_markers) = &old_markers {
        for (pad, slot) in old_markers.cue_slots().iter().enumerate() {
            if let Some(position) = slot {
                taken[pad] = true;
                taken_positions.push(*position);
            }
        }
    }
    report.kept = existing_cues.len();

    for (index, cue) in markers
        .cues
        .iter()
        .take(usize::from(SERATO_HOT_CUES))
        .enumerate()
    {
        let position = u32::try_from(cue.position.as_millis()).unwrap_or(u32::MAX);
        if taken_positions.contains(&position) {
            report.already_there += 1;
            continue;
        }
        // A cue the caller set on a pad keeps it when that pad is free;
        // one the analysis placed takes the next pad in order.
        let wanted_pad = cue.pad.unwrap_or(u8::try_from(index).unwrap_or(u8::MAX));
        let pad = if taken.get(usize::from(wanted_pad)).copied().unwrap_or(true) {
            taken.iter().position(|is_taken| !is_taken)
        } else {
            Some(usize::from(wanted_pad))
        };
        let Some(pad) = pad else {
            report.no_free_pad.push(cue.name.clone());
            continue;
        };
        taken[pad] = true;
        taken_positions.push(position);
        let pad = pad as u8;
        let colour = SERATO_CUE_COLOURS[usize::from(pad)];
        markers2.add_cue(pad, position, colour, &cue.name);
        if usize::from(pad) < MARKERS_CUE_SLOTS {
            if old_markers.is_none() && container == Container::Mp4 {
                old_markers = Some(OldMarkers::empty(container));
            }
            if let Some(old_markers) = &mut old_markers {
                old_markers.set_cue(usize::from(pad), position, colour);
            }
        }
        report.placed.push(PlacedCue {
            name: cue.name.clone(),
            pad,
            wanted_pad,
        });
    }

    let mut changes: Vec<(String, Option<String>, String)> = Vec::new();
    if !report.placed.is_empty() {
        changes.push((
            markers2_field.to_string(),
            current_markers2,
            markers2.to_value(container),
        ));
        if let (Some(old_markers), Some(field)) = (&old_markers, markers_field) {
            let value = old_markers.to_value(container);
            if current_markers.as_deref() != Some(value.as_str()) {
                changes.push((field.to_string(), current_markers, value));
            }
        }
    }

    if let Some(grid) = markers.beat_grid {
        let position = grid.first_beat.as_secs_f64() as f32;
        let bpm = grid.bpm as f32;
        match existing_grid
            .as_ref()
            .filter(|grid| !grid.markers.is_empty())
        {
            Some(existing) => {
                let first = existing.markers[0].position();
                let last_bpm = match existing.markers.last() {
                    Some(GridMarker::Terminal { bpm, .. }) => *bpm,
                    _ => 0.0,
                };
                report.grid = if existing.markers.len() == 1
                    && (first - position).abs() < 0.001
                    && (last_bpm - bpm).abs() < 0.01
                {
                    GridOutcome::AlreadyThere
                } else {
                    GridOutcome::KeptYours {
                        position: first,
                        bpm: last_bpm,
                    }
                };
            }
            None => {
                let new_grid = BeatGrid {
                    markers: vec![GridMarker::Terminal { position, bpm }],
                    footer: existing_grid.as_ref().map_or(0, |grid| grid.footer),
                    filler: existing_grid.as_ref().and_then(|grid| grid.filler),
                };
                changes.push((
                    beat_grid_field.to_string(),
                    current_grid,
                    new_grid.to_value(container),
                ));
            }
        }
    }

    for (field, current, new) in changes {
        let item = file.raw(&field)?;
        plan.changes.push(FieldChange {
            field,
            current,
            new: Some(new),
        });
        plan.current_items.push(item);
    }
    Ok(report)
}

/// A cue entry of `Serato Markers2`, decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Markers2Cue {
    pad: u8,
    position: u32,
    name: String,
}

/// One entry of `Serato Markers2`: its type and its data, kept as read.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    kind: Vec<u8>,
    data: Vec<u8>,
}

/// `Serato Markers2`: entries such as `COLOR`, `BPMLOCK`, `CUE`, `LOOP` and
/// `FLIP`, and the length Serato set aside for the field.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Markers2 {
    entries: Vec<Entry>,
    allocated: usize,
    /// The last base64 digit of a FLAC field, which Serato sets in a way
    /// nobody has worked out and which Mixxx keeps.
    flac_last_digit: u8,
}

impl Markers2 {
    /// What Serato writes for a track it analyzed but gave no cues: no
    /// track colour and an unlocked grid.
    fn new() -> Self {
        Self {
            entries: vec![
                Entry {
                    kind: b"COLOR".to_vec(),
                    data: vec![0x00, 0xFF, 0xFF, 0xFF],
                },
                Entry {
                    kind: b"BPMLOCK".to_vec(),
                    data: vec![0x00],
                },
            ],
            allocated: 0,
            flac_last_digit: FILLER_DIGIT,
        }
    }

    fn from_value(container: Container, value: &str) -> Result<Self, String> {
        match container {
            Container::Id3 => {
                let payload = base64::decode(value.as_bytes()).ok_or("not base64")?;
                Self::from_payload(&payload)
            }
            Container::Mp4 | Container::Flac => {
                let payload = unwrap_object(value.as_bytes(), MARKERS2_NAME)?;
                let mut markers2 = Self::from_payload(&payload)?;
                if container == Container::Flac {
                    markers2.flac_last_digit = *value.as_bytes().last().ok_or("empty")?;
                }
                Ok(markers2)
            }
        }
    }

    /// Reads the data as an ID3 frame holds it: a version, base64 text,
    /// then zeros up to the length Serato set aside.
    fn from_payload(payload: &[u8]) -> Result<Self, String> {
        let text = payload
            .strip_prefix(&MARKERS2_VERSION)
            .ok_or("unknown Serato Markers2 version")?;
        let text_end = text
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(text.len());
        let inner = base64::decode(&text[..text_end]).ok_or("the cue data is not base64")?;
        let mut rest = inner
            .strip_prefix(&MARKERS2_VERSION)
            .ok_or("unknown Serato Markers2 entry version")?;
        let mut entries = Vec::new();
        loop {
            let Some(name_end) = rest.iter().position(|byte| *byte == 0) else {
                // Serato's base64 sometimes loses the last zero.
                if rest.is_empty() {
                    break;
                }
                return Err("an entry has no end".to_string());
            };
            if name_end == 0 {
                break;
            }
            let kind = rest[..name_end].to_vec();
            let length_bytes = rest
                .get(name_end + 1..name_end + 5)
                .ok_or("an entry is cut short")?;
            let length = u32::from_be_bytes(
                length_bytes
                    .try_into()
                    .map_err(|_| "an entry is cut short")?,
            ) as usize;
            let data_start = name_end + 5;
            let data = rest
                .get(data_start..data_start + length)
                .ok_or("an entry is cut short")?
                .to_vec();
            if kind == b"CUE" {
                decode_cue(&data)?;
            }
            entries.push(Entry { kind, data });
            rest = &rest[data_start + length..];
        }
        Ok(Self {
            entries,
            allocated: payload.len(),
            flac_last_digit: FILLER_DIGIT,
        })
    }

    fn cues(&self) -> Vec<Markers2Cue> {
        self.entries
            .iter()
            .filter(|entry| entry.kind == b"CUE")
            .filter_map(|entry| decode_cue(&entry.data).ok())
            .collect()
    }

    /// Adds a cue after the last cue the entries hold, or after the colour
    /// and lock when they hold none.
    fn add_cue(&mut self, pad: u8, position: u32, colour: [u8; 3], name: &str) {
        let mut data = vec![0x00, pad];
        data.extend(position.to_be_bytes());
        data.push(0x00);
        data.extend(colour);
        data.extend([0x00, 0x00]);
        data.extend(name.as_bytes());
        data.push(0x00);
        let at = self
            .entries
            .iter()
            .rposition(|entry| {
                entry.kind == b"CUE" || entry.kind == b"COLOR" || entry.kind == b"BPMLOCK"
            })
            .map_or(self.entries.len(), |index| index + 1);
        self.entries.insert(
            at,
            Entry {
                kind: b"CUE".to_vec(),
                data,
            },
        );
    }

    fn payload(&self) -> Vec<u8> {
        let mut inner = MARKERS2_VERSION.to_vec();
        for entry in &self.entries {
            inner.extend(&entry.kind);
            inner.push(0);
            inner.extend((entry.data.len() as u32).to_be_bytes());
            inner.extend(&entry.data);
        }
        inner.push(0);
        let mut payload = MARKERS2_VERSION.to_vec();
        payload.extend(serato_base64(&inner, true));
        let length = if self.allocated > payload.len() {
            self.allocated
        } else {
            (payload.len() + 1).max(MINIMUM_MARKERS2_LENGTH)
        };
        payload.resize(length, 0);
        payload
    }

    fn to_value(&self, container: Container) -> String {
        let payload = self.payload();
        match container {
            Container::Id3 => base64::encode(&payload, true),
            Container::Mp4 | Container::Flac => {
                let mut text = wrap_object(&payload, MARKERS2_NAME);
                if container == Container::Flac
                    && let Some(last) = text.last_mut()
                {
                    *last = self.flac_last_digit;
                }
                String::from_utf8(text).unwrap_or_default()
            }
        }
    }
}

fn decode_cue(data: &[u8]) -> Result<Markers2Cue, String> {
    if data.len() < 13 || data[0] != 0 || data[6] != 0 {
        return Err("a cue entry is damaged".to_string());
    }
    let name_bytes = &data[12..];
    let name_end = name_bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(name_bytes.len());
    Ok(Markers2Cue {
        pad: data[1],
        position: u32::from_be_bytes([data[2], data[3], data[4], data[5]]),
        name: String::from_utf8_lossy(&name_bytes[..name_end]).into_owned(),
    })
}

/// A marker of `Serato BeatGrid`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum GridMarker {
    /// A marker followed by another, `beats` beats later.
    NonTerminal { position: f32, beats: u32 },
    /// The last marker, with the tempo from it to the end.
    Terminal { position: f32, bpm: f32 },
}

impl GridMarker {
    fn position(&self) -> f32 {
        match self {
            GridMarker::NonTerminal { position, .. } | GridMarker::Terminal { position, .. } => {
                *position
            }
        }
    }
}

/// `Serato BeatGrid`: its markers and the byte after them, whose meaning
/// is unknown and which is kept.
#[derive(Debug, Clone, PartialEq)]
struct BeatGrid {
    markers: Vec<GridMarker>,
    footer: u8,
    /// The extra base64 digit after an M4A or FLAC field.
    filler: Option<u8>,
}

impl BeatGrid {
    fn from_value(container: Container, value: &str) -> Result<Self, String> {
        match container {
            Container::Id3 => {
                let data = base64::decode(value.as_bytes()).ok_or("not base64")?;
                Self::from_data(&data, None)
            }
            Container::Mp4 | Container::Flac => {
                let (filler, data) = unwrap_object_with_filler(value.as_bytes(), BEAT_GRID_NAME)?;
                Self::from_data(&data, Some(filler))
            }
        }
    }

    fn from_data(data: &[u8], filler: Option<u8>) -> Result<Self, String> {
        let rest = data
            .strip_prefix(&BEAT_GRID_VERSION)
            .ok_or("unknown Serato BeatGrid version")?;
        let count_bytes: [u8; 4] = rest
            .get(..4)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or("the beat grid is cut short")?;
        let count = u32::from_be_bytes(count_bytes) as usize;
        let markers_end = 4 + count * 8;
        if rest.len() != markers_end + 1 {
            return Err("the beat grid has the wrong length".to_string());
        }
        let markers = (0..count)
            .map(|index| {
                let start = 4 + index * 8;
                let position = f32::from_be_bytes([
                    rest[start],
                    rest[start + 1],
                    rest[start + 2],
                    rest[start + 3],
                ]);
                let value = [
                    rest[start + 4],
                    rest[start + 5],
                    rest[start + 6],
                    rest[start + 7],
                ];
                if index + 1 == count {
                    GridMarker::Terminal {
                        position,
                        bpm: f32::from_be_bytes(value),
                    }
                } else {
                    GridMarker::NonTerminal {
                        position,
                        beats: u32::from_be_bytes(value),
                    }
                }
            })
            .collect();
        Ok(Self {
            markers,
            footer: rest[markers_end],
            filler,
        })
    }

    fn data(&self) -> Vec<u8> {
        let mut data = BEAT_GRID_VERSION.to_vec();
        data.extend((self.markers.len() as u32).to_be_bytes());
        for marker in &self.markers {
            match marker {
                GridMarker::NonTerminal { position, beats } => {
                    data.extend(position.to_be_bytes());
                    data.extend(beats.to_be_bytes());
                }
                GridMarker::Terminal { position, bpm } => {
                    data.extend(position.to_be_bytes());
                    data.extend(bpm.to_be_bytes());
                }
            }
        }
        data.push(self.footer);
        data
    }

    fn to_value(&self, container: Container) -> String {
        match container {
            Container::Id3 => base64::encode(&self.data(), true),
            Container::Mp4 | Container::Flac => {
                let mut text = wrap_object(&self.data(), BEAT_GRID_NAME);
                text.push(self.filler.unwrap_or(FILLER_DIGIT));
                String::from_utf8(text).unwrap_or_default()
            }
        }
    }
}

/// `Serato Markers_`: Serato's older copy of the first five cues and nine
/// loops, which it prefers to `Serato Markers2`. Entries are kept as read;
/// only the cue slots Euphoria fills are rewritten.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OldMarkers {
    container: Container,
    entries: Vec<Vec<u8>>,
    /// The track colour after the entries.
    tail: Vec<u8>,
    filler: u8,
}

impl OldMarkers {
    fn entry_length(container: Container) -> usize {
        match container {
            Container::Mp4 => MP4_MARKERS_ENTRY_LENGTH,
            _ => ID3_MARKERS_ENTRY_LENGTH,
        }
    }

    /// The copy Serato writes for a track without cues or loops.
    fn empty(container: Container) -> Self {
        let entry = |kind: u8| -> Vec<u8> {
            match container {
                Container::Mp4 => {
                    let mut entry = vec![0xFF; 8];
                    entry.extend([0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);
                    entry.extend([0x00, 0x00, 0x00, kind, 0x00]);
                    entry
                }
                _ => {
                    let mut entry = vec![0x7F; 10];
                    entry.extend([0x00, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F]);
                    entry.extend([0x00, 0x00, 0x00, 0x00, kind, 0x00]);
                    entry
                }
            }
        };
        let mut entries: Vec<Vec<u8>> = (0..MARKERS_CUE_SLOTS).map(|_| entry(0)).collect();
        entries.extend(
            (MARKERS_CUE_SLOTS..MARKERS_ENTRIES as usize).map(|_| entry(MARKERS_LOOP_TYPE)),
        );
        let tail = match container {
            Container::Mp4 => vec![0x00, 0xFF, 0xFF, 0xFF],
            _ => serato32(u32::from_be_bytes([0, 0xFF, 0xFF, 0xFF])).to_vec(),
        };
        Self {
            container,
            entries,
            tail,
            filler: FILLER_DIGIT,
        }
    }

    fn from_value(container: Container, value: &str) -> Result<Self, String> {
        let (filler, data) = match container {
            Container::Mp4 => unwrap_object_with_filler(value.as_bytes(), MARKERS_NAME)?,
            _ => (
                FILLER_DIGIT,
                base64::decode(value.as_bytes()).ok_or("not base64")?,
            ),
        };
        let rest = data
            .strip_prefix(&MARKERS_VERSION)
            .ok_or("unknown Serato Markers_ version")?;
        let count = rest
            .get(..4)
            .map(|bytes| u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            .ok_or("Serato Markers_ is cut short")?;
        if count != MARKERS_ENTRIES {
            return Err(format!("Serato Markers_ has {count} entries, not 14"));
        }
        let length = Self::entry_length(container);
        let entries_end = 4 + MARKERS_ENTRIES as usize * length;
        if rest.len() != entries_end + 4 {
            return Err("Serato Markers_ has the wrong length".to_string());
        }
        let entries: Vec<Vec<u8>> = rest[4..entries_end]
            .chunks(length)
            .map(<[u8]>::to_vec)
            .collect();
        let kind_offset = length - 2;
        if entries[..MARKERS_CUE_SLOTS]
            .iter()
            .any(|entry| !matches!(entry[kind_offset], 0 | MARKERS_CUE_TYPE))
        {
            return Err("Serato Markers_ has a loop among its cues".to_string());
        }
        Ok(Self {
            container,
            entries,
            tail: rest[entries_end..].to_vec(),
            filler,
        })
    }

    /// The position in milliseconds of each of the five cue slots that is
    /// set.
    fn cue_slots(&self) -> Vec<Option<u32>> {
        self.entries[..MARKERS_CUE_SLOTS]
            .iter()
            .map(|entry| match self.container {
                Container::Mp4 => {
                    let position = u32::from_be_bytes([entry[0], entry[1], entry[2], entry[3]]);
                    (position != u32::MAX).then_some(position)
                }
                _ => (entry[0] == 0x00)
                    .then(|| from_serato32([entry[1], entry[2], entry[3], entry[4]])),
            })
            .collect()
    }

    fn set_cue(&mut self, slot: usize, position: u32, colour: [u8; 3]) {
        let entry = match self.container {
            Container::Mp4 => {
                let mut entry = position.to_be_bytes().to_vec();
                entry.extend([0xFF; 4]);
                entry.extend([0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x00]);
                entry.extend(colour);
                entry.extend([MARKERS_CUE_TYPE, 0x00]);
                entry
            }
            _ => {
                let mut entry = vec![0x00];
                entry.extend(serato32(position & 0x00FF_FFFF));
                entry.push(0x7F);
                entry.extend([0x7F; 4]);
                entry.extend([0x00, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F]);
                entry.extend(serato32(u32::from_be_bytes([
                    0, colour[0], colour[1], colour[2],
                ])));
                entry.extend([MARKERS_CUE_TYPE, 0x00]);
                entry
            }
        };
        self.entries[slot] = entry;
    }

    fn data(&self) -> Vec<u8> {
        let mut data = MARKERS_VERSION.to_vec();
        data.extend(MARKERS_ENTRIES.to_be_bytes());
        for entry in &self.entries {
            data.extend(entry);
        }
        data.extend(&self.tail);
        data
    }

    fn to_value(&self, container: Container) -> String {
        match container {
            Container::Mp4 => {
                let mut text = wrap_object(&self.data(), MARKERS_NAME);
                text.push(self.filler);
                String::from_utf8(text).unwrap_or_default()
            }
            _ => base64::encode(&self.data(), true),
        }
    }
}

/// A 24 bit number spread over four bytes of seven bits each, as
/// `Serato Markers_` stores positions and colours.
fn serato32(value: u32) -> [u8; 4] {
    [
        ((value >> 21) & 0x7F) as u8,
        ((value >> 14) & 0x7F) as u8,
        ((value >> 7) & 0x7F) as u8,
        (value & 0x7F) as u8,
    ]
}

fn from_serato32(bytes: [u8; 4]) -> u32 {
    bytes
        .iter()
        .fold(0, |value, byte| (value << 7) | u32::from(byte & 0x7F))
}

/// `data` in base64 as Serato writes it: no padding, a line break every 72
/// digits, and with `chop` the last digit of a final group left out when
/// that group does not fill three bytes.
fn serato_base64(data: &[u8], chop: bool) -> Vec<u8> {
    let mut text = Vec::with_capacity(data.len() * 4 / 3 + data.len() / BYTES_PER_LINE + 4);
    for (index, block) in data.chunks(BYTES_PER_LINE).enumerate() {
        if index > 0 {
            text.push(b'\n');
        }
        let mut encoded = base64::encode(block, false).into_bytes();
        if chop && block.len() % 3 != 0 {
            encoded.pop();
        }
        text.extend(encoded);
    }
    text
}

/// The base64 text of an M4A or FLAC field: the object header and name,
/// then `data`.
fn wrap_object(data: &[u8], name: &str) -> Vec<u8> {
    let mut object = OBJECT_HEADER.to_vec();
    object.extend(name.as_bytes());
    object.push(0);
    object.extend(data);
    serato_base64(&object, false)
}

/// The data of an M4A or FLAC field's base64 `text`, after its header and
/// name.
fn unwrap_object(text: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let object = base64::decode(text).ok_or("not base64")?;
    strip_object_header(&object, name)
}

/// As [`unwrap_object`], for a field with one extra digit at its end,
/// which is returned with the data.
fn unwrap_object_with_filler(text: &[u8], name: &str) -> Result<(u8, Vec<u8>), String> {
    let (filler, digits) = text.split_last().ok_or("empty")?;
    let object = base64::decode(digits).ok_or("not base64")?;
    Ok((*filler, strip_object_header(&object, name)?))
}

fn strip_object_header(object: &[u8], name: &str) -> Result<Vec<u8>, String> {
    let mut header = OBJECT_HEADER.to_vec();
    header.extend(name.as_bytes());
    header.push(0);
    object
        .strip_prefix(header.as_slice())
        .map(<[u8]>::to_vec)
        .ok_or_else(|| format!("it does not start with the {name} header"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures(folder: &str) -> Vec<(String, Vec<u8>)> {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/serato-fixtures")
            .join(folder);
        let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(&directory)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                (
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    std::fs::read(&path).unwrap(),
                )
            })
            .collect();
        files.sort();
        assert!(!files.is_empty(), "{folder}");
        files
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    // Each fixture is a field Serato DJ wrote, from Mixxx's test data. A
    // field read and written again must come back byte for byte.
    #[test]
    fn markers2_written_by_serato_round_trip_byte_for_byte() {
        for (name, payload) in fixtures("mp3/markers2") {
            let value = base64::encode(&payload, true);
            let markers2 = Markers2::from_value(Container::Id3, &value).unwrap();
            assert_eq!(markers2.to_value(Container::Id3), value, "{name}");
        }
        for (container, folder) in [
            (Container::Mp4, "mp4/markers2"),
            (Container::Flac, "flac/markers2"),
        ] {
            for (name, bytes) in fixtures(folder) {
                let value = text(&bytes);
                let markers2 = Markers2::from_value(container, &value).unwrap();
                assert_eq!(markers2.to_value(container), value, "{folder}/{name}");
            }
        }
    }

    #[test]
    fn beat_grids_written_by_serato_round_trip_byte_for_byte() {
        for (name, data) in fixtures("mp3/beatgrid") {
            let value = base64::encode(&data, true);
            let grid = BeatGrid::from_value(Container::Id3, &value).unwrap();
            assert_eq!(grid.to_value(Container::Id3), value, "{name}");
        }
        for (container, folder) in [
            (Container::Mp4, "mp4/beatgrid"),
            (Container::Flac, "flac/beatgrid"),
        ] {
            for (name, bytes) in fixtures(folder) {
                let value = text(&bytes);
                let grid = BeatGrid::from_value(container, &value).unwrap();
                assert_eq!(grid.to_value(container), value, "{folder}/{name}");
                assert!(
                    matches!(grid.markers[..], [GridMarker::Terminal { bpm, .. }] if (bpm - 115.0).abs() < 0.001),
                    "{grid:?}"
                );
            }
        }
    }

    #[test]
    fn old_markers_written_by_serato_round_trip_byte_for_byte() {
        for (name, data) in fixtures("mp3/markers_") {
            let value = base64::encode(&data, true);
            let markers = OldMarkers::from_value(Container::Id3, &value).unwrap();
            assert_eq!(markers.to_value(Container::Id3), value, "{name}");
        }
        for (name, bytes) in fixtures("mp4/markers_") {
            let value = text(&bytes);
            let markers = OldMarkers::from_value(Container::Mp4, &value).unwrap();
            assert_eq!(markers.to_value(Container::Mp4), value, "{name}");
        }
    }

    #[test]
    fn cues_written_by_serato_are_read_with_their_pads_and_positions() {
        let fixture = fixtures("mp3/markers2")
            .into_iter()
            .find(|(name, _)| name.starts_with("hotcue-positions"))
            .unwrap()
            .1;
        let markers2 =
            Markers2::from_value(Container::Id3, &base64::encode(&fixture, true)).unwrap();
        // The file is named after its cues: 0:00.0, 3:38.4, 1:00.0, 0:00.1
        // and 0:01.0.
        let positions: Vec<(u8, u32)> = markers2
            .cues()
            .iter()
            .map(|cue| (cue.pad, cue.position))
            .collect();
        assert_eq!(positions.len(), 5);
        let rounded: Vec<(u8, u32)> = positions
            .iter()
            .map(|(pad, position)| (*pad, (position + 50) / 100))
            .collect();
        assert_eq!(rounded, [(0, 0), (1, 2185), (2, 600), (3, 1), (4, 10)]);

        let old = fixtures("mp3/markers_")
            .into_iter()
            .find(|(name, _)| name.starts_with("hotcue-positions"))
            .unwrap()
            .1;
        let old = OldMarkers::from_value(Container::Id3, &base64::encode(&old, true)).unwrap();
        let slots: Vec<Option<u32>> = old.cue_slots();
        assert_eq!(
            slots,
            positions
                .iter()
                .map(|(_, position)| Some(*position))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn an_empty_old_copy_matches_what_serato_writes_for_an_analyzed_track() {
        for (container, folder) in [
            (Container::Id3, "mp3/markers_"),
            (Container::Mp4, "mp4/markers_"),
        ] {
            let analyzed = fixtures(folder)
                .into_iter()
                .find(|(name, _)| name == "analyzed.octet-stream")
                .unwrap()
                .1;
            let value = match container {
                Container::Mp4 => text(&analyzed),
                _ => base64::encode(&analyzed, true),
            };
            assert_eq!(
                OldMarkers::empty(container).to_value(container),
                value,
                "{folder}"
            );
        }
    }

    #[test]
    fn serato32_matches_the_documented_examples() {
        assert_eq!(serato32(0), [0, 0, 0, 0]);
        // #CC0000, stored by Serato as 06 30 00 00.
        assert_eq!(serato32(0x00CC_0000), [0x06, 0x30, 0x00, 0x00]);
        // White, stored as 07 7F 7F 7F.
        assert_eq!(serato32(0x00FF_FFFF), [0x07, 0x7F, 0x7F, 0x7F]);
        for value in [0, 1, 127, 128, 0x0012_3456, 0x00FF_FFFF] {
            assert_eq!(from_serato32(serato32(value)), value);
        }
    }

    #[test]
    fn a_new_markers2_payload_is_padded_to_470_bytes() {
        let mut markers2 = Markers2::new();
        markers2.add_cue(0, 1_500, SERATO_CUE_COLOURS[0], "Start");
        let payload = markers2.payload();
        assert_eq!(payload.len(), MINIMUM_MARKERS2_LENGTH);
        let read = Markers2::from_payload(&payload).unwrap();
        assert_eq!(
            read.cues(),
            vec![Markers2Cue {
                pad: 0,
                position: 1_500,
                name: "Start".to_string()
            }]
        );
        assert_eq!(read.entries, markers2.entries);
    }
}
