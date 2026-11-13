// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Writes key, tempo, beat grid, hot cues and the energy label into the
//! tracks of an existing Engine DJ library, the SQLite database
//! `Engine Library/Database2/m.db` of Engine DJ 2.0 and later.
//!
//! The layout follows libdjinterop (<https://github.com/xsco/libdjinterop>,
//! LGPL-3.0), the library Mixxx exports to Engine DJ with, at commit
//! `d61727c5`: the schemas in `src/djinterop/engine/schema/`, the blobs in
//! `src/djinterop/engine/v2/*_blob.cpp` and the key numbering in
//! `include/djinterop/musical_key.hpp`.
//!
//! Engine DJ does not support programs that write its database. Its
//! support article on third-party database tools
//! (<https://support.denondj.com/en/support/solutions/articles/69000834165>)
//! sets the rules this module keeps: Engine DJ must be closed, and the
//! schema must not change. So the database is only ever changed with
//! `UPDATE` of existing rows inside one transaction, after a copy of it is
//! saved, and only when its schema version is one this module was checked
//! against. A track Engine DJ has not analysed has no sample rate in its
//! performance data, so it gets its key, tempo and label but no beat grid
//! or cues.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use super::engine_blobs::{
    BeatData, EMPTY_QUICK_CUE_OFFSET, QUICK_CUE_COUNT, QuickCue, QuickCues, TrackData, compress,
    constant_beat_grid, decompress,
};
use super::rekordbox::{CueKind, Track};
use crate::error::EuphoriaError;
use crate::key::{Key, Mode};
use crate::tags::KeyNotation;
use crate::tags::LabelPlacement;
use crate::tags::label::{label_text, with_label};

/// The schema versions of `Information` this module writes, from Engine DJ
/// 2.0 to Engine DJ 4.5 and 5.0. Any other version is refused.
pub const SUPPORTED_SCHEMAS: [SchemaVersion; 10] = [
    SchemaVersion::new(2, 18, 0),
    SchemaVersion::new(2, 20, 1),
    SchemaVersion::new(2, 20, 2),
    SchemaVersion::new(2, 20, 3),
    SchemaVersion::new(2, 21, 0),
    SchemaVersion::new(2, 21, 1),
    SchemaVersion::new(2, 21, 2),
    SchemaVersion::new(3, 0, 0),
    SchemaVersion::new(3, 0, 1),
    SchemaVersion::new(3, 0, 2),
];

/// The colours Engine DJ gives its eight hot cue pads, as alpha, red, green
/// and blue, as found in a real library by Open Engine Lib
/// (<https://github.com/benedikt-weyer/Open-Engine-Lib>). No pad 7 was set
/// in that library, so its colour is Euphoria's choice.
const PAD_COLOURS: [[u8; 4]; QUICK_CUE_COUNT] = [
    [0xFF, 0xF4, 0xD3, 0x38],
    [0xFF, 0xEF, 0x81, 0x30],
    [0xFF, 0xAA, 0x55, 0xC4],
    [0xFF, 0xCE, 0x32, 0x39],
    [0xFF, 0x86, 0xC6, 0x4B],
    [0xFF, 0x20, 0xC6, 0x70],
    [0xFF, 0x20, 0xC6, 0xC6],
    [0xFF, 0x15, 0x71, 0xE2],
];

const REQUIRED_TRACK_COLUMNS: [&str; 6] = ["id", "path", "key", "bpm", "bpmAnalyzed", "comment"];
const REQUIRED_PERFORMANCE_COLUMNS: [&str; 4] = ["trackId", "trackData", "beatData", "quickCues"];
const MAXIMUM_LABEL_BYTES: usize = 255;

/// The version of an Engine DJ database schema, from its `Information`
/// table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SchemaVersion {
    /// The major version: 2 for Engine DJ 2.0 to 4.0, 3 from Engine DJ 4.1.
    pub major: u32,
    /// The minor version.
    pub minor: u32,
    /// The patch version.
    pub patch: u32,
}

impl SchemaVersion {
    /// The version `major.minor.patch`.
    pub const fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl std::fmt::Display for SchemaVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Choices that change what an Engine DJ export writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EngineOptions {
    /// Where the key and energy label goes: the start or the end of the
    /// comment, or nowhere. Engine DJ has no grouping field, so
    /// [`LabelPlacement::Grouping`] writes no label.
    pub label: LabelPlacement,
    /// How the key is spelled in the label. Engine DJ's Key column is
    /// filled from the key itself, in the notation set in its preferences.
    pub label_notation: KeyNotation,
}

/// One track to write, with the energy rating that goes into its label.
#[derive(Debug, Clone, Copy)]
pub struct EngineTrack<'track> {
    /// The track, as [`super::assemble_library_with`] builds it. Its
    /// `comment` is not used: the label is made from `key` and `energy`.
    pub track: &'track Track,
    /// The energy rating, 1 to 10.
    pub energy: Option<u8>,
}

/// What an update changed, or would change in a dry run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EngineUpdate {
    /// The schema version of the library.
    pub schema: Option<SchemaVersion>,
    /// The tracks whose row was updated, in the order given.
    pub updated: Vec<PathBuf>,
    /// The tracks that match no track of the library, in the order given.
    pub not_in_library: Vec<PathBuf>,
    /// The tracks Engine DJ has not analysed, which got no beat grid or
    /// cues because their sample rate is not known.
    pub not_analysed: Vec<PathBuf>,
    /// The tracks that have hot cues of their own in Engine DJ, which were
    /// kept: no cue was written to them.
    pub kept_own_cues: Vec<PathBuf>,
    /// The number of cues left out because they are on no pad from 1 to 8.
    pub cues_left_out: usize,
}

/// The library database of the Engine Library folder `library`.
pub fn library_database(library: &Path) -> PathBuf {
    library.join("Database2").join("m.db")
}

/// The number Engine DJ stores for a key: the circle of fifths from C
/// major, each major key followed by its relative minor, so 0 is C major,
/// 1 A minor, 2 G major and 23 D minor.
pub fn engine_key_value(key: Key) -> i64 {
    let open_key_index = i64::from((key.camelot_number() + 4) % 12);
    let minor = match key.mode() {
        Mode::Major => 0,
        Mode::Minor => 1,
    };
    open_key_index * 2 + minor
}

/// The key an Engine DJ key number stands for, or `None` outside 0 to 23.
pub fn key_from_engine_value(value: i64) -> Option<Key> {
    if !(0..24).contains(&value) {
        return None;
    }
    let open_key_index = (value / 2) as u8;
    let major_tonic = (open_key_index * 7) % 12;
    Some(if value % 2 == 0 {
        Key::new(major_tonic, Mode::Major)
    } else {
        Key::new((major_tonic + 9) % 12, Mode::Minor)
    })
}

/// Checks the library at `database` and works out what writing `tracks`
/// into it would change, without writing anything.
///
/// # Errors
///
/// As [`update_library`], without the errors of writing.
pub fn preview_library_update(
    database: &Path,
    tracks: &[EngineTrack<'_>],
    options: &EngineOptions,
) -> Result<EngineUpdate, EuphoriaError> {
    check_no_pending_write(database)?;
    let connection = open(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let schema = check_schema(&connection, database)?;
    let plan = plan(&connection, database, tracks, options)?;
    Ok(plan.update(schema, tracks))
}

/// Writes `tracks` into the Engine DJ library at `database`, after saving
/// a copy of it as `backup`, which must not exist.
///
/// Before anything is written the database is checked: no write of another
/// program may be pending (a `m.db-journal` or `m.db-wal` file with data),
/// `Information` must hold one row, with id 1, whose version is one of
/// [`SUPPORTED_SCHEMAS`], and the `Track` and `PerformanceData` columns
/// written must exist. Every change is an `UPDATE` of an existing row, in
/// one transaction, and the database is checked with SQLite's quick check
/// afterwards.
///
/// A track is matched by the path Engine DJ stores, relative to the
/// Engine Library folder, once resolved; failing that, by the real path of
/// the file it names. For each matched track the `key`, `bpm` (rounded)
/// and `bpmAnalyzed` columns are set, the key inside the `trackData` blob
/// too, and the label goes into `comment` as `options` says. A track with
/// a first beat and a tempo that Engine DJ has analysed gets a constant
/// beat grid in `beatData`. Hot cues on pads 1 to 8 go into `quickCues`
/// when the track has no hot cue of its own; the main cue is kept.
///
/// # Errors
///
/// Returns [`EuphoriaError::EngineLibrary`] when the library cannot be
/// read, fails a check, or holds a blob that cannot be decoded, and
/// [`EuphoriaError::ExportFile`] when the backup cannot be written. Nothing
/// is changed then. The quick check failing after the write is an
/// [`EuphoriaError::EngineLibrary`] that names the backup.
pub fn update_library(
    database: &Path,
    backup: &Path,
    tracks: &[EngineTrack<'_>],
    options: &EngineOptions,
) -> Result<EngineUpdate, EuphoriaError> {
    check_no_pending_write(database)?;
    let reader = open(database, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let schema = check_schema(&reader, database)?;
    let plan = plan(&reader, database, tracks, options)?;
    drop(reader);
    let update = plan.update(schema, tracks);
    if plan.rows.is_empty() {
        return Ok(update);
    }

    copy_new(database, backup)?;
    let mut connection = open(database, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|error| {
            refuse(
                database,
                format!("it cannot be locked for writing ({error})"),
            )
        })?;
    if check_schema(&transaction, database)? != schema {
        return Err(refuse(
            database,
            "it changed while the export ran".to_string(),
        ));
    }
    for row in &plan.rows {
        apply(&transaction, row)
            .map_err(|error| refuse(database, format!("a track could not be updated ({error})")))?;
    }
    transaction.commit().map_err(|error| {
        refuse(
            database,
            format!("the changes could not be saved ({error})"),
        )
    })?;
    let quick_check: String = connection
        .query_row("PRAGMA quick_check", [], |row| row.get(0))
        .map_err(|error| refuse(database, format!("it could not be checked ({error})")))?;
    if quick_check != "ok" {
        return Err(refuse(
            database,
            format!(
                "SQLite's check of it failed after the export ({quick_check}); put the copy at {} back in its place",
                backup.display()
            ),
        ));
    }
    Ok(update)
}

fn refuse(database: &Path, reason: String) -> EuphoriaError {
    EuphoriaError::EngineLibrary {
        path: database.to_path_buf(),
        reason,
    }
}

fn open(database: &Path, flags: OpenFlags) -> Result<Connection, EuphoriaError> {
    if !database.is_file() {
        return Err(refuse(database, "there is no such file".to_string()));
    }
    Connection::open_with_flags(database, flags | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|error| refuse(database, format!("it cannot be opened ({error})")))
}

/// Refuses a database with a journal that holds data: another program,
/// such as Engine DJ, is writing it or stopped halfway through.
fn check_no_pending_write(database: &Path) -> Result<(), EuphoriaError> {
    for suffix in ["-journal", "-wal"] {
        let mut name = database.as_os_str().to_owned();
        name.push(suffix);
        let journal = PathBuf::from(name);
        if let Ok(metadata) = std::fs::metadata(&journal)
            && metadata.len() > 0
        {
            return Err(refuse(
                database,
                format!(
                    "{} holds a write that is not finished; open and close Engine DJ, then export again",
                    journal.display()
                ),
            ));
        }
    }
    Ok(())
}

/// The schema version, once the database passed every check.
fn check_schema(connection: &Connection, database: &Path) -> Result<SchemaVersion, EuphoriaError> {
    let fail = |reason: String| refuse(database, reason);
    let rows: Vec<(i64, i64, i64, i64)> = connection
        .prepare(
            "SELECT id, schemaVersionMajor, schemaVersionMinor, schemaVersionPatch FROM Information",
        )
        .and_then(|mut statement| {
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?
                .collect()
        })
        .map_err(|error| fail(format!("it is not an Engine DJ library ({error})")))?;
    let [(id, major, minor, patch)] = rows.as_slice() else {
        return Err(fail(format!(
            "its Information table holds {} rows, not one",
            rows.len()
        )));
    };
    if *id != 1 {
        return Err(fail(format!("its Information row has id {id}, not 1")));
    }
    let version = [major, minor, patch].map(|part| u32::try_from(*part).ok());
    let schema = match version {
        [Some(major), Some(minor), Some(patch)] => SchemaVersion::new(major, minor, patch),
        _ => {
            return Err(fail(format!(
                "its schema version {major}.{minor}.{patch} is not valid"
            )));
        }
    };
    if !SUPPORTED_SCHEMAS.contains(&schema) {
        return Err(fail(format!(
            "its schema version {schema} is not one Euphoria was checked against ({} to {}); a newer Engine DJ may store tracks differently",
            SUPPORTED_SCHEMAS[0],
            SUPPORTED_SCHEMAS[SUPPORTED_SCHEMAS.len() - 1]
        )));
    }
    let journal_mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .map_err(|error| fail(format!("its journal mode cannot be read ({error})")))?;
    if journal_mode.eq_ignore_ascii_case("wal") {
        return Err(fail(
            "it uses a write-ahead log, which Engine DJ does not".to_string(),
        ));
    }
    for (table, columns) in [
        ("Track", REQUIRED_TRACK_COLUMNS.as_slice()),
        ("PerformanceData", REQUIRED_PERFORMANCE_COLUMNS.as_slice()),
    ] {
        let present: Vec<String> = connection
            .prepare(&format!("PRAGMA table_info({table})"))
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| row.get::<_, String>(1))?
                    .collect()
            })
            .map_err(|error| fail(format!("its {table} table cannot be read ({error})")))?;
        if let Some(missing) = columns
            .iter()
            .find(|column| !present.iter().any(|name| name == *column))
        {
            return Err(fail(format!("its {table} table has no {missing} column")));
        }
    }
    Ok(schema)
}

/// The `trackData`, `beatData` and `quickCues` blobs of a track, each
/// `None` when the column is NULL.
type PerformanceBlobs = (Option<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>);

/// The new values of one row of `Track` and its performance data.
#[derive(Debug)]
struct RowUpdate {
    track_id: i64,
    key: Option<i64>,
    bpm: Option<(i64, f64)>,
    comment: Option<String>,
    track_data: Option<Vec<u8>>,
    beat_data: Option<Vec<u8>>,
    quick_cues: Option<Vec<u8>>,
}

impl RowUpdate {
    /// True when the row carries no value to write: `apply` would run
    /// nothing on it.
    fn is_noop(&self) -> bool {
        self.key.is_none()
            && self.bpm.is_none()
            && self.comment.is_none()
            && self.track_data.is_none()
            && self.beat_data.is_none()
            && self.quick_cues.is_none()
    }
}

#[derive(Debug, Default)]
struct Plan {
    rows: Vec<RowUpdate>,
    matched: Vec<bool>,
    updated: Vec<bool>,
    not_analysed: Vec<bool>,
    kept_own_cues: Vec<bool>,
    cues_left_out: usize,
}

impl Plan {
    fn update(&self, schema: SchemaVersion, tracks: &[EngineTrack<'_>]) -> EngineUpdate {
        let pick = |flags: &[bool]| -> Vec<PathBuf> {
            tracks
                .iter()
                .zip(flags)
                .filter(|(_, flag)| **flag)
                .map(|(entry, _)| entry.track.path.clone())
                .collect()
        };
        let unmatched: Vec<bool> = self.matched.iter().map(|flag| !flag).collect();
        EngineUpdate {
            schema: Some(schema),
            updated: pick(&self.updated),
            not_in_library: pick(&unmatched),
            not_analysed: pick(&self.not_analysed),
            kept_own_cues: pick(&self.kept_own_cues),
            cues_left_out: self.cues_left_out,
        }
    }
}

/// `path` with `.` and `..` worked out without asking the file system.
fn lexically_normal(path: &Path) -> PathBuf {
    let mut normal = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normal.pop();
            }
            other => normal.push(other),
        }
    }
    normal
}

fn plan(
    connection: &Connection,
    database: &Path,
    tracks: &[EngineTrack<'_>],
    options: &EngineOptions,
) -> Result<Plan, EuphoriaError> {
    let fail = |reason: String| refuse(database, reason);
    let library = database
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| fail("it is not inside an Engine Library folder".to_string()))?;
    let by_path: HashMap<&Path, usize> = tracks
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.track.path.as_path(), index))
        .collect();
    let by_file_name: HashMap<String, Vec<usize>> =
        tracks
            .iter()
            .enumerate()
            .fold(HashMap::new(), |mut names, (index, entry)| {
                if let Some(name) = entry.track.path.file_name() {
                    names
                        .entry(name.to_string_lossy().to_lowercase())
                        .or_insert_with(Vec::new)
                        .push(index);
                }
                names
            });

    let rows: Vec<(i64, String, Option<String>)> = connection
        .prepare("SELECT id, path, comment FROM Track WHERE path IS NOT NULL")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
                .collect()
        })
        .map_err(|error| fail(format!("its tracks cannot be read ({error})")))?;

    let mut plan = Plan {
        matched: vec![false; tracks.len()],
        updated: vec![false; tracks.len()],
        not_analysed: vec![false; tracks.len()],
        kept_own_cues: vec![false; tracks.len()],
        ..Plan::default()
    };
    for (track_id, stored_path, comment) in rows {
        let absolute = lexically_normal(&library.join(&stored_path));
        let index = by_path.get(absolute.as_path()).copied().or_else(|| {
            let name = absolute.file_name()?.to_string_lossy().to_lowercase();
            let candidates = by_file_name.get(&name)?;
            let resolved = std::fs::canonicalize(&absolute).ok()?;
            candidates
                .iter()
                .copied()
                .find(|index| tracks[*index].track.path == resolved)
        });
        let Some(index) = index else {
            continue;
        };
        if plan.matched[index] {
            continue;
        }
        let performance: Option<PerformanceBlobs> = connection
            .query_row(
                "SELECT trackData, beatData, quickCues FROM PerformanceData WHERE trackId = ?1",
                params![track_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(|error| {
                fail(format!(
                    "the performance data of {stored_path} cannot be read ({error})"
                ))
            })?;
        let row = row_update(
            track_id,
            comment.as_deref(),
            performance,
            tracks[index],
            options,
            &mut plan,
            index,
        )
        .map_err(|reason| fail(format!("{stored_path}: {reason}")))?;
        plan.matched[index] = true;
        // A matched track whose values all match what the library already
        // holds changes nothing, and is not reported as updated.
        if !row.is_noop() {
            plan.updated[index] = true;
            plan.rows.push(row);
        }
    }
    Ok(plan)
}

fn row_update(
    track_id: i64,
    comment: Option<&str>,
    performance: Option<PerformanceBlobs>,
    entry: EngineTrack<'_>,
    options: &EngineOptions,
    plan: &mut Plan,
    index: usize,
) -> Result<RowUpdate, String> {
    let track = entry.track;
    let key = track.key.map(engine_key_value);
    let bpm = track
        .bpm
        .filter(|bpm| bpm.is_finite() && *bpm > 0.0)
        .map(|bpm| (bpm.round() as i64, bpm));
    let label = label_text(track.key, entry.energy, options.label_notation);
    let comment = match (options.label, label) {
        (LabelPlacement::CommentStart, Some(label)) => Some(with_label(&label, comment, false)),
        (LabelPlacement::CommentEnd, Some(label)) => Some(with_label(&label, comment, true)),
        _ => None,
    };
    let mut update = RowUpdate {
        track_id,
        key,
        bpm,
        comment,
        track_data: None,
        beat_data: None,
        quick_cues: None,
    };

    let (track_data_blob, beat_data_blob, quick_cues_blob) = performance.unwrap_or_default();
    let track_data = match track_data_blob.as_deref().map(decompress).transpose()? {
        Some(payload) if !payload.is_empty() => Some(TrackData::decode(&payload)?),
        _ => None,
    };
    let Some(mut track_data) = track_data.filter(|data| data.sample_rate > 0.0 && data.samples > 0)
    else {
        let wants_performance = track.first_beat.is_some() || !track.cue_points.is_empty();
        plan.not_analysed[index] = wants_performance;
        return Ok(update);
    };
    if let Some(key) = key
        && i64::from(track_data.key) != key
    {
        track_data.key = key as i32;
        update.track_data = Some(compress(&track_data.encode()));
    }

    if let (Some(first_beat), Some((_, bpm))) = (track.first_beat, bpm) {
        let grid = constant_beat_grid(
            track_data.sample_rate,
            track_data.samples,
            first_beat.as_secs_f64(),
            bpm,
        )?;
        let existing = match beat_data_blob.as_deref().map(decompress).transpose()? {
            Some(payload) if !payload.is_empty() => Some(BeatData::decode(&payload)?),
            _ => None,
        };
        let beat_data = BeatData {
            sample_rate: track_data.sample_rate,
            samples: track_data.samples as f64,
            is_beatgrid_set: 1,
            default_grid: grid.clone(),
            adjusted_grid: grid,
            extra: existing.map_or_else(|| vec![0; 9], |existing| existing.extra),
        };
        update.beat_data = Some(compress(&beat_data.encode()));
    }

    if !track.cue_points.is_empty() {
        let existing = match quick_cues_blob.as_deref().map(decompress).transpose()? {
            Some(payload) if !payload.is_empty() => Some(QuickCues::decode(&payload)?),
            _ => None,
        };
        let has_own = existing.as_ref().is_some_and(|existing| {
            existing
                .cues
                .iter()
                .any(|cue| cue.sample_offset != EMPTY_QUICK_CUE_OFFSET)
        });
        if has_own {
            plan.kept_own_cues[index] = true;
        } else {
            let mut cues: Vec<QuickCue> = (0..QUICK_CUE_COUNT)
                .map(|_| QuickCue {
                    label: String::new(),
                    sample_offset: EMPTY_QUICK_CUE_OFFSET,
                    colour: [0; 4],
                })
                .collect();
            for cue_point in &track.cue_points {
                match cue_point.kind {
                    CueKind::Hot(pad) if usize::from(pad) < QUICK_CUE_COUNT => {
                        cues[usize::from(pad)] = QuickCue {
                            label: truncated(&cue_point.name, MAXIMUM_LABEL_BYTES),
                            sample_offset: cue_point.time.as_secs_f64() * track_data.sample_rate,
                            colour: PAD_COLOURS[usize::from(pad)],
                        };
                    }
                    CueKind::Hot(_) => plan.cues_left_out += 1,
                    CueKind::Memory => {
                        let on_a_pad = track.cue_points.iter().any(|other| {
                            matches!(other.kind, CueKind::Hot(_))
                                && other.time == cue_point.time
                                && other.name == cue_point.name
                        });
                        if !on_a_pad {
                            plan.cues_left_out += 1;
                        }
                    }
                }
            }
            let (adjusted_main_cue, is_main_cue_adjusted, default_main_cue, extra) = match existing
            {
                Some(existing) => (
                    existing.adjusted_main_cue,
                    existing.is_main_cue_adjusted,
                    existing.default_main_cue,
                    existing.extra,
                ),
                None => (
                    EMPTY_QUICK_CUE_OFFSET,
                    0,
                    EMPTY_QUICK_CUE_OFFSET,
                    Vec::new(),
                ),
            };
            update.quick_cues = Some(compress(
                &QuickCues {
                    cues,
                    adjusted_main_cue,
                    is_main_cue_adjusted,
                    default_main_cue,
                    extra,
                }
                .encode(),
            ));
        }
    }
    Ok(update)
}

/// `text` cut to at most `limit` bytes, at a character boundary.
fn truncated(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

fn apply(connection: &Connection, row: &RowUpdate) -> rusqlite::Result<()> {
    if let Some(key) = row.key {
        connection.execute(
            "UPDATE Track SET key = ?1 WHERE id = ?2",
            params![key, row.track_id],
        )?;
    }
    if let Some((rounded, exact)) = row.bpm {
        connection.execute(
            "UPDATE Track SET bpm = ?1, bpmAnalyzed = ?2 WHERE id = ?3",
            params![rounded, exact, row.track_id],
        )?;
    }
    if let Some(comment) = &row.comment {
        connection.execute(
            "UPDATE Track SET comment = ?1 WHERE id = ?2",
            params![comment, row.track_id],
        )?;
    }
    for (column, blob) in [
        ("trackData", &row.track_data),
        ("beatData", &row.beat_data),
        ("quickCues", &row.quick_cues),
    ] {
        if let Some(blob) = blob {
            connection.execute(
                &format!("UPDATE PerformanceData SET {column} = ?1 WHERE trackId = ?2"),
                params![blob, row.track_id],
            )?;
        }
    }
    Ok(())
}

/// Copies `database` to `backup`, which must not exist yet.
fn copy_new(database: &Path, backup: &Path) -> Result<(), EuphoriaError> {
    let file_error = |source: std::io::Error| EuphoriaError::ExportFile {
        path: backup.to_path_buf(),
        source,
    };
    if let Some(folder) = backup.parent() {
        std::fs::create_dir_all(folder).map_err(file_error)?;
    }
    let mut source = std::fs::File::open(database).map_err(|source| EuphoriaError::ExportFile {
        path: database.to_path_buf(),
        source,
    })?;
    let mut copy = std::fs::File::options()
        .write(true)
        .create_new(true)
        .open(backup)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                EuphoriaError::ExportDestinationExists {
                    path: backup.to_path_buf(),
                }
            } else {
                file_error(error)
            }
        })?;
    std::io::copy(&mut source, &mut copy)
        .and_then(|_| copy.sync_all())
        .map_err(file_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_numbers_follow_the_circle_of_fifths() {
        let expected = [
            (0, "C"),
            (1, "Am"),
            (2, "G"),
            (3, "Em"),
            (7, "F#m"),
            (12, "F#"),
            (13, "Ebm"),
            (14, "Db"),
            (19, "Cm"),
            (22, "F"),
            (23, "Dm"),
        ];
        for (value, name) in expected {
            let spelled = match name.strip_suffix('m') {
                Some(tonic) => format!("{tonic} minor"),
                None => format!("{name} major"),
            };
            let key: Key = spelled.parse().unwrap();
            assert_eq!(engine_key_value(key), value, "{name}");
        }
        for value in 0..24 {
            assert_eq!(
                engine_key_value(key_from_engine_value(value).unwrap()),
                value
            );
        }
        assert_eq!(key_from_engine_value(24), None);
        assert_eq!(key_from_engine_value(-1), None);
    }

    #[test]
    fn stored_paths_are_resolved_against_the_library_folder() {
        assert_eq!(
            lexically_normal(&Path::new("/Users/dj/Music/Engine Library").join("../DJ/a.mp3")),
            PathBuf::from("/Users/dj/Music/DJ/a.mp3")
        );
    }

    #[test]
    fn labels_are_cut_at_a_character_boundary() {
        assert_eq!(truncated("abcé", 4), "abc");
        assert_eq!(truncated("abc", 4), "abc");
    }
}
