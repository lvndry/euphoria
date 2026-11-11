// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A local SQLite database of finished analyses, so a track is analyzed once.
//!
//! # How a file is recognised
//!
//! An analysis is stored under the BLAKE3 hash of the whole file, which makes
//! a moved or renamed file the same track as before. Hashing reads every
//! byte, so each analysis also records the path, size and modification time
//! the file had. A file found at its recorded path with the recorded size and
//! modification time is served without being read at all. Anything else is
//! hashed, and analyzed only when the hash is unknown.
//!
//! Trade offs of this scheme:
//!
//! - The whole file is hashed, not a sample such as its first and last
//!   megabyte. Sampling would make a moved library faster to recognise, but
//!   two different files that agree on the sampled parts and on size would
//!   share one analysis. Stems bounced from one project do exactly that:
//!   equal length, silence at both ends. Hashing costs a small fraction of
//!   analyzing, and only runs for files that are new, moved or modified.
//! - The hash covers tags and artwork as well as audio. Finding the audio
//!   inside every supported container is a parser per format, and a mistake
//!   in one would serve a wrong result. The consequence is that a file whose
//!   tags were edited by another program is analyzed once more. Euphoria's
//!   own tag writes record the analysis for the new bytes as they write, and
//!   an undo records where the old bytes are again, so neither costs an
//!   analysis.
//! - The quick check trusts the file system. A file rewritten in place with
//!   the same size and the same modification time keeps its old analysis.
//! - The file is read once for the hash and once more for the analysis. An
//!   analysis is stored only when the size and modification time are the same
//!   after it as before the hash, so a file that changed in between is left
//!   for the next run.
//! - One analysis remembers one path. Byte for byte copies of a track share
//!   an analysis, and all but one of them are hashed again on every run.
//! - A path that is not valid Unicode is not recorded, so such a file is
//!   hashed on every run.
//!
//! # Analyzer versions
//!
//! Each analysis records the [`ANALYZER_VERSION`] that produced it and the
//! [`ENERGY_MODEL_VERSION`] behind its energy rating. Analyses from any other
//! version of either are never served, and are replaced when their file is
//! analyzed again.
//!
//! The energy model version has a column of its own, apart from the analyzer
//! version, because the energy scale is recalibrated on its own schedule.
//! Raising the one constant next to the model is then enough to mark every
//! stored rating as stale, and the database still says which model produced
//! each number.

mod batch;
mod corrections;
mod edited_cues;
mod identity;
pub(crate) mod location;
mod rewritten;
mod single;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rusqlite::types::Type;
use rusqlite::{Connection, ErrorCode, Row, TransactionBehavior, params};

pub use batch::{CachePolicy, CachedBatch, analyze_files_cached, analyze_files_cached_observed};
pub use identity::ContentHash;
pub(crate) use location::default_data_directory;
pub use location::{DATA_DIRECTORY_VARIABLE, default_cache_path};
pub use single::{CachedAnalysis, CachedAnalyzer};

use crate::analysis::{ANALYZER_VERSION, TrackAnalysis};
use crate::detect::{BeatGrid, CueSection, KeySection, SectionCue};
use crate::energy::ENERGY_MODEL_VERSION;
use crate::error::{ErrorSource, EuphoriaError};
use crate::key::Key;
use crate::quality::QualityReport;
use identity::FileStamp;

/// One entry per schema version, each bringing a database from the version
/// before it. `PRAGMA user_version` holds how many have been applied, so a
/// schema change is a new entry at the end and never an edit to an old one.
const MIGRATIONS: [&str; 9] = [
    "
    CREATE TABLE analyses (
        content_hash     BLOB    NOT NULL PRIMARY KEY,
        duration_seconds REAL    NOT NULL,
        key              TEXT,
        bpm              REAL,
        analyzer_version TEXT    NOT NULL,
        analyzed_at      INTEGER NOT NULL,
        last_known_path  TEXT,
        file_size        INTEGER NOT NULL,
        modified_at      INTEGER NOT NULL
    ) WITHOUT ROWID;
    CREATE UNIQUE INDEX analyses_last_known_path ON analyses (last_known_path);
",
    "
    ALTER TABLE analyses ADD COLUMN energy INTEGER;
    ALTER TABLE analyses ADD COLUMN energy_model_version INTEGER;
",
    "
    ALTER TABLE analyses ADD COLUMN tuning_cents REAL;
    ALTER TABLE analyses ADD COLUMN key_timeline TEXT;
",
    "
    ALTER TABLE analyses ADD COLUMN key_runner_up TEXT;
    ALTER TABLE analyses ADD COLUMN key_score_margin REAL;
    ALTER TABLE analyses ADD COLUMN tempo_estimate REAL;
    ALTER TABLE analyses ADD COLUMN tempo_score_margin REAL;
",
    "
    ALTER TABLE analyses ADD COLUMN quality TEXT;
",
    "
    ALTER TABLE analyses ADD COLUMN grid_first_downbeat REAL;
    ALTER TABLE analyses ADD COLUMN grid_bpm REAL;
    ALTER TABLE analyses ADD COLUMN grid_salience REAL;
    ALTER TABLE analyses ADD COLUMN grid_drift REAL;
",
    "
    CREATE TABLE corrections (
        content_hash BLOB    NOT NULL PRIMARY KEY,
        key          TEXT,
        bpm          REAL,
        corrected_at INTEGER NOT NULL
    ) WITHOUT ROWID;
",
    "
    ALTER TABLE analyses ADD COLUMN cues TEXT;
",
    "
    CREATE TABLE edited_cues (
        content_hash BLOB    NOT NULL PRIMARY KEY,
        cues         TEXT    NOT NULL,
        edited_at    INTEGER NOT NULL
    ) WITHOUT ROWID;
",
];

const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// How long to wait for another process that holds the database, such as a
/// second `analyze` started at the same moment, before giving up.
const BUSY_TIMEOUT: Duration = Duration::from_secs(10);

const BUSY_RETRY_PAUSE: Duration = Duration::from_millis(20);

/// What preparing a database found.
enum Schema {
    Ready,
    /// The database has this schema version, which is newer than
    /// [`SCHEMA_VERSION`].
    Newer(u32),
}

/// The analysis cache: an open SQLite database.
///
/// In its `analyses` table, `key` is a key name such as "A minor",
/// `analyzed_at` is in seconds and `modified_at` in nanoseconds since the Unix
/// epoch, and `last_known_path` is absolute and belongs to at most one row.
/// `energy` is a rating from 1 to 10 and `energy_model_version` the model
/// that decided it, which is recorded for a track with no rating too. Rows
/// stored before energy existed have neither. `tuning_cents` is the offset
/// from A = 440 Hz, and `key_timeline` holds one line per section of the key
/// over time: its start and end in seconds and its key name, separated by
/// tabs. Rows stored before those two existed, and tracks without them, hold
/// null. `key_runner_up` is a key name and `key_score_margin` how far the
/// key scored above it; `tempo_estimate` is the tempo before it is placed
/// in a range and `tempo_score_margin` what its confidence is based on.
/// Confidence levels and the reported tempo are worked out from these when
/// they are read, so neither the band edges nor the tempo range are stored.
/// `quality` holds the quality report as JSON, with the field names of
/// [`QualityReport`]; rows stored before it
/// existed hold null.
#[derive(Debug)]
pub struct Cache {
    connection: Connection,
    path: PathBuf,
    analyzer_version: String,
    energy_model_version: u32,
}

/// A change to the database, sent by analysis workers to the one thread that
/// writes.
#[derive(Debug)]
enum CacheWrite {
    /// Record a new analysis, replacing any stored for the same content.
    Store {
        content_hash: ContentHash,
        analysis: Box<TrackAnalysis>,
        location: FileLocation,
    },
    /// Record where known content is now.
    Relocate {
        content_hash: ContentHash,
        location: FileLocation,
    },
}

/// Where a file was seen and what the file system said about it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileLocation {
    absolute_path: PathBuf,
    stamp: FileStamp,
}

/// Every analysis of the current analyzer version, read once so that workers
/// can look files up without touching the database.
#[derive(Debug, Default)]
struct Snapshot {
    analyses: HashMap<ContentHash, TrackAnalysis>,
    locations: HashMap<PathBuf, (FileStamp, ContentHash)>,
}

impl Snapshot {
    /// The analysis of the file at `location`, when the path, size and
    /// modification time are the ones recorded with it.
    fn at_location(&self, location: &FileLocation) -> Option<&TrackAnalysis> {
        let (stamp, content_hash) = self.locations.get(&location.absolute_path)?;
        if *stamp != location.stamp {
            return None;
        }
        self.analyses.get(content_hash)
    }

    fn of_content(&self, content_hash: &ContentHash) -> Option<&TrackAnalysis> {
        self.analyses.get(content_hash)
    }
}

impl Cache {
    /// Opens the database at `path`, creating it and its folder when missing
    /// and bringing an older schema up to date. Several processes may open
    /// the same new database at once: each waits for the others.
    ///
    /// Fails with [`EuphoriaError::Cache`] naming the file when it cannot be
    /// created, is not a SQLite database, or was written by a newer Euphoria.
    pub fn open(path: &Path) -> Result<Self, EuphoriaError> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).map_err(cache_error(path))?;
        }
        let mut connection = Connection::open(path).map_err(cache_error(path))?;
        connection
            .busy_timeout(BUSY_TIMEOUT)
            .map_err(cache_error(path))?;
        let schema = retry_while_busy(|| prepare(&mut connection)).map_err(cache_error(path))?;
        if let Schema::Newer(found) = schema {
            return Err(cache_error(path)(format!(
                "the cache has schema version {found} and this version of Euphoria reads up to \
                 {SCHEMA_VERSION}; update Euphoria or clear the cache"
            )));
        }
        Ok(Self {
            connection,
            path: path.to_path_buf(),
            analyzer_version: ANALYZER_VERSION.to_string(),
            energy_model_version: ENERGY_MODEL_VERSION,
        })
    }

    /// The database file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Number of analyses stored, of every analyzer version.
    pub fn entry_count(&self) -> Result<u64, EuphoriaError> {
        self.connection
            .query_row("SELECT COUNT(*) FROM analyses", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(i64::unsigned_abs)
            .map_err(cache_error(&self.path))
    }

    fn snapshot(&self) -> Result<Snapshot, EuphoriaError> {
        self.read_snapshot().map_err(cache_error(&self.path))
    }

    fn read_snapshot(&self) -> rusqlite::Result<Snapshot> {
        let mut statement = self.connection.prepare(
            "SELECT content_hash, duration_seconds, key, bpm,
                    last_known_path, file_size, modified_at, energy,
                    tuning_cents, key_timeline, key_runner_up, key_score_margin,
                    tempo_estimate, tempo_score_margin, quality,
                    grid_first_downbeat, grid_bpm, grid_salience, grid_drift, cues
             FROM analyses
             WHERE analyzer_version = ?1 AND energy_model_version = ?2",
        )?;
        let mut rows =
            statement.query(params![self.analyzer_version, self.energy_model_version])?;
        let mut snapshot = Snapshot::default();
        while let Some(row) = rows.next()? {
            let content_hash = ContentHash::from_bytes(row.get(0)?);
            snapshot.analyses.insert(
                content_hash,
                TrackAnalysis {
                    duration_seconds: row.get(1)?,
                    key: key_in_column(row, 2)?,
                    bpm: row.get(3)?,
                    energy: energy_in_column(row, 7)?,
                    tuning_cents: row.get(8)?,
                    key_timeline: key_timeline_in_column(row, 9)?,
                    key_runner_up: key_in_column(row, 10)?,
                    key_score_margin: row.get(11)?,
                    tempo_estimate: tempo_in_column(row, 12)?,
                    tempo_score_margin: row.get(13)?,
                    quality: quality_in_column(row, 14)?,
                    beat_grid: beat_grid_in_columns(row, 15)?,
                    cues: cues_in_column(row, 19)?,
                },
            );
            if let Some(last_known_path) = row.get::<_, Option<String>>(4)? {
                let stamp = FileStamp {
                    size: row.get(5)?,
                    modified_nanoseconds: row.get(6)?,
                };
                snapshot
                    .locations
                    .insert(PathBuf::from(last_known_path), (stamp, content_hash));
            }
        }
        Ok(snapshot)
    }

    /// Applies writes as they arrive, until every sender is gone or a write
    /// fails. Returning drops the receiver, which tells senders to stop.
    fn apply_writes(&mut self, writes: Receiver<CacheWrite>) -> Result<(), EuphoriaError> {
        for write in writes {
            self.apply_write(&write).map_err(cache_error(&self.path))?;
        }
        Ok(())
    }

    fn apply_write(&mut self, write: &CacheWrite) -> rusqlite::Result<()> {
        self.apply_write_then(write, |_| Ok(()))
    }

    /// Applies `write` and then `then`, in one transaction: both are kept
    /// or neither is.
    fn apply_write_then(
        &mut self,
        write: &CacheWrite,
        then: impl FnOnce(&Connection) -> rusqlite::Result<()>,
    ) -> rusqlite::Result<()> {
        let (content_hash, location) = match write {
            CacheWrite::Store {
                content_hash,
                location,
                ..
            }
            | CacheWrite::Relocate {
                content_hash,
                location,
            } => (content_hash, location),
        };
        let last_known_path = location.absolute_path.to_str();
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(last_known_path) = last_known_path {
            transaction.execute(
                "UPDATE analyses SET last_known_path = NULL
                 WHERE last_known_path = ?1 AND content_hash <> ?2",
                params![last_known_path, content_hash.as_bytes()],
            )?;
        }
        match write {
            CacheWrite::Store { analysis, .. } => transaction.execute(
                "INSERT OR REPLACE INTO analyses (
                     content_hash, duration_seconds, key, bpm, analyzer_version,
                     analyzed_at, last_known_path, file_size, modified_at,
                     energy, energy_model_version, tuning_cents, key_timeline,
                     key_runner_up, key_score_margin, tempo_estimate, tempo_score_margin, quality,
                     grid_first_downbeat, grid_bpm, grid_salience, grid_drift, cues
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
                           ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
                params![
                    content_hash.as_bytes(),
                    analysis.duration_seconds,
                    analysis.key.map(|key| key.to_string()),
                    analysis.bpm,
                    self.analyzer_version,
                    seconds_since_epoch(),
                    last_known_path,
                    location.stamp.size,
                    location.stamp.modified_nanoseconds,
                    analysis.energy,
                    self.energy_model_version,
                    analysis.tuning_cents,
                    key_timeline_text(&analysis.key_timeline),
                    analysis.key_runner_up.map(|key| key.to_string()),
                    analysis.key_score_margin,
                    analysis.tempo_estimate,
                    analysis.tempo_score_margin,
                    quality_text(analysis.quality.as_ref()),
                    analysis.beat_grid.map(|grid| grid.first_downbeat_seconds),
                    analysis.beat_grid.map(|grid| grid.bpm),
                    analysis.beat_grid.map(|grid| grid.salience),
                    analysis.beat_grid.and_then(|grid| grid.drift_seconds),
                    cues_text(&analysis.cues),
                ],
            )?,
            CacheWrite::Relocate { .. } => transaction.execute(
                "UPDATE analyses
                 SET last_known_path = ?2, file_size = ?3, modified_at = ?4
                 WHERE content_hash = ?1",
                params![
                    content_hash.as_bytes(),
                    last_known_path,
                    location.stamp.size,
                    location.stamp.modified_nanoseconds,
                ],
            )?,
        };
        then(&transaction)?;
        transaction.commit()
    }

    fn remove_every_analysis(&mut self) -> rusqlite::Result<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute("DELETE FROM analyses", [])?;
        transaction.commit()
    }
}

/// Forgets every analysis in the database at `path`. Returns whether there
/// was a database to clear.
///
/// A database that opens is emptied in one transaction and its file is kept,
/// so an analysis running at the same time goes on writing to the same
/// database: what it stores after the clear is kept. A database that cannot
/// be opened, such as a damaged file or one written by a newer Euphoria, is
/// deleted instead, which is what makes clearing a way to recover from it.
/// A database that is only busy is left alone and reported as an error.
pub fn clear_cache(path: &Path) -> Result<bool, EuphoriaError> {
    if !path.try_exists().map_err(cache_error(path))? {
        return Ok(false);
    }
    match Cache::open(path) {
        Ok(mut cache) => cache.remove_every_analysis().map_err(cache_error(path))?,
        Err(error) if is_busy_failure(&error) => return Err(error),
        Err(_) => {
            remove_if_present(path).map_err(cache_error(path))?;
            for suffix in ["-wal", "-shm"] {
                let mut companion = path.as_os_str().to_owned();
                companion.push(suffix);
                remove_if_present(Path::new(&companion)).map_err(cache_error(path))?;
            }
        }
    }
    Ok(true)
}

fn is_busy_failure(error: &EuphoriaError) -> bool {
    match error {
        EuphoriaError::Cache { source, .. } => source
            .downcast_ref::<rusqlite::Error>()
            .is_some_and(is_busy),
        _ => false,
    }
}

fn is_busy(error: &rusqlite::Error) -> bool {
    matches!(
        error.sqlite_error_code(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

/// Runs `operation` again while it reports that another connection holds the
/// database. SQLite's own busy timeout does not cover every case: switching
/// the journal mode, for one, can fail at once.
fn retry_while_busy<Value>(
    mut operation: impl FnMut() -> rusqlite::Result<Value>,
) -> rusqlite::Result<Value> {
    let deadline = Instant::now() + BUSY_TIMEOUT;
    loop {
        match operation() {
            Err(error) if is_busy(&error) && Instant::now() < deadline => {
                std::thread::sleep(BUSY_RETRY_PAUSE);
            }
            outcome => return outcome,
        }
    }
}

fn remove_if_present(path: &Path) -> std::io::Result<bool> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Switches the database to write ahead logging and applies the migrations
/// it lacks.
///
/// The schema version is read again inside an immediate transaction, which
/// holds the write lock, so of several processes that find a new database
/// only one creates the schema and the others see it done.
fn prepare(connection: &mut Connection) -> rusqlite::Result<Schema> {
    let journal_mode: String =
        connection.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    if !journal_mode.eq_ignore_ascii_case("wal") {
        connection.pragma_update(None, "journal_mode", "WAL")?;
    }
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    if schema_version(connection)? == SCHEMA_VERSION {
        return Ok(Schema::Ready);
    }

    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let applied = schema_version(&transaction)?;
    if applied > SCHEMA_VERSION {
        return Ok(Schema::Newer(applied));
    }
    for migration in MIGRATIONS.iter().skip(applied as usize) {
        transaction.execute_batch(migration)?;
    }
    transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    transaction.commit()?;
    Ok(Schema::Ready)
}

fn schema_version(connection: &Connection) -> rusqlite::Result<u32> {
    connection.pragma_query_value(None, "user_version", |row| row.get(0))
}

fn key_in_column(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<Key>> {
    row.get::<_, Option<String>>(column)?
        .map(|name| name.parse())
        .transpose()
        .map_err(|error: EuphoriaError| {
            rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(error))
        })
}

/// A stored tempo estimate, which is placed in a tempo range when it is
/// shown and so must be a positive number.
fn tempo_in_column(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<f32>> {
    let Some(stored) = row.get::<_, Option<f32>>(column)? else {
        return Ok(None);
    };
    if stored.is_finite() && stored > 0.0 {
        return Ok(Some(stored));
    }
    Err(rusqlite::Error::FromSqlConversionFailure(
        column,
        Type::Real,
        format!("tempo estimate {stored} is not a positive number").into(),
    ))
}

/// A stored beat grid, from four columns starting at `first_column`: the
/// first downbeat, the tempo, the salience and the drift. `None` when the
/// tempo is null, as for a track without a grid.
/// The beat grid stored in four columns from `first_column`: the first
/// downbeat, the tempo, the salience and the drift. All four are null for a
/// track without a grid. A grid has the first three; the drift is null when
/// the track had too few stretches with clear beats to measure it. Any other
/// mix of null columns is a damaged row.
fn beat_grid_in_columns(row: &Row<'_>, first_column: usize) -> rusqlite::Result<Option<BeatGrid>> {
    let first_downbeat_seconds: Option<f64> = row.get(first_column)?;
    let bpm: Option<f64> = row.get(first_column + 1)?;
    let salience: Option<f32> = row.get(first_column + 2)?;
    let drift_seconds: Option<f32> = row.get(first_column + 3)?;
    let damaged = |problem: String| {
        rusqlite::Error::FromSqlConversionFailure(first_column, Type::Real, problem.into())
    };
    let (Some(first_downbeat_seconds), Some(bpm), Some(salience)) =
        (first_downbeat_seconds, bpm, salience)
    else {
        if first_downbeat_seconds.is_none()
            && bpm.is_none()
            && salience.is_none()
            && drift_seconds.is_none()
        {
            return Ok(None);
        }
        return Err(damaged(format!(
            "beat grid with first downbeat {first_downbeat_seconds:?}, tempo {bpm:?}, \
             salience {salience:?} and drift {drift_seconds:?} is only partly stored"
        )));
    };
    let grid = BeatGrid {
        first_downbeat_seconds,
        bpm,
        salience,
        drift_seconds,
    };
    let usable = grid.bpm.is_finite()
        && grid.bpm > 0.0
        && grid.first_downbeat_seconds.is_finite()
        && grid.first_downbeat_seconds >= 0.0
        && grid.salience.is_finite()
        && grid
            .drift_seconds
            .is_none_or(|drift| drift.is_finite() && drift >= 0.0);
    if usable {
        Ok(Some(grid))
    } else {
        Err(damaged(format!(
            "beat grid at {first_downbeat_seconds} seconds and {bpm} BPM is not a usable grid"
        )))
    }
}

fn energy_in_column(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<u8>> {
    let Some(stored) = row.get::<_, Option<i64>>(column)? else {
        return Ok(None);
    };
    u8::try_from(stored)
        .ok()
        .filter(|rating| (1..=10).contains(rating))
        .map(Some)
        .ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                column,
                Type::Integer,
                format!("energy rating {stored} is not between 1 and 10").into(),
            )
        })
}

/// The stored form of a quality report, or `None` when there is none.
fn quality_text(quality: Option<&QualityReport>) -> Option<String> {
    quality.and_then(|report| serde_json::to_string(report).ok())
}

fn quality_in_column(row: &Row<'_>, column: usize) -> rusqlite::Result<Option<QualityReport>> {
    let Some(stored) = row.get::<_, Option<String>>(column)? else {
        return Ok(None);
    };
    serde_json::from_str(&stored).map(Some).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(column, Type::Text, Box::new(error))
    })
}

/// The stored form of a key timeline, or `None` for an empty one.
fn key_timeline_text(timeline: &[KeySection]) -> Option<String> {
    if timeline.is_empty() {
        return None;
    }
    let lines: Vec<String> = timeline
        .iter()
        .map(|section| {
            format!(
                "{}\t{}\t{}",
                section.start_seconds, section.end_seconds, section.key
            )
        })
        .collect();
    Some(lines.join("\n"))
}

fn key_timeline_in_column(row: &Row<'_>, column: usize) -> rusqlite::Result<Vec<KeySection>> {
    let Some(stored) = row.get::<_, Option<String>>(column)? else {
        return Ok(Vec::new());
    };
    stored
        .lines()
        .map(|line| {
            parse_key_section(line).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    column,
                    Type::Text,
                    format!("{line:?} is not a section of a key timeline").into(),
                )
            })
        })
        .collect()
}

fn parse_key_section(line: &str) -> Option<KeySection> {
    let mut fields = line.split('\t');
    let start_seconds: f32 = fields.next()?.parse().ok()?;
    let end_seconds: f32 = fields.next()?.parse().ok()?;
    let key: Key = fields.next()?.parse().ok()?;
    let is_whole = fields.next().is_none() && start_seconds.is_finite() && end_seconds.is_finite();
    is_whole.then_some(KeySection {
        start_seconds,
        end_seconds,
        key,
    })
}

/// The stored form of the cues of a track, one `seconds<TAB>name` line per
/// cue, or `None` for none.
fn cues_text(cues: &[SectionCue]) -> Option<String> {
    if cues.is_empty() {
        return None;
    }
    let lines: Vec<String> = cues
        .iter()
        .map(|cue| format!("{}\t{}", cue.seconds, cue.section.name()))
        .collect();
    Some(lines.join("\n"))
}

fn cues_in_column(row: &Row<'_>, column: usize) -> rusqlite::Result<Vec<SectionCue>> {
    let Some(stored) = row.get::<_, Option<String>>(column)? else {
        return Ok(Vec::new());
    };
    stored
        .lines()
        .map(|line| {
            parse_cue(line).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    column,
                    Type::Text,
                    format!("{line:?} is not a cue").into(),
                )
            })
        })
        .collect()
}

fn parse_cue(line: &str) -> Option<SectionCue> {
    let (seconds, name) = line.split_once('\t')?;
    let seconds: f64 = seconds.parse().ok()?;
    let section = CueSection::from_name(name)?;
    (seconds.is_finite() && seconds >= 0.0).then_some(SectionCue { seconds, section })
}

fn seconds_since_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn cache_error<Cause: Into<ErrorSource>>(path: &Path) -> impl Fn(Cause) -> EuphoriaError {
    move |cause| EuphoriaError::Cache {
        path: path.to_path_buf(),
        source: cause.into(),
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A folder under the system's temporary folder, deleted when dropped.
    pub(crate) struct TemporaryFolder(PathBuf);

    impl TemporaryFolder {
        pub(crate) fn new() -> Self {
            static CREATED: AtomicUsize = AtomicUsize::new(0);
            let folder = std::env::temp_dir().join(format!(
                "euphoria-cache-test-{}-{}",
                std::process::id(),
                CREATED.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&folder).unwrap();
            Self(folder.canonicalize().unwrap())
        }

        pub(crate) fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TemporaryFolder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// An analyzer version that schema version 4 databases were filled by.
    pub(crate) const VERSION_4_ANALYZER: &str = "6+libkeyfinder";

    /// Creates a database as the last release with schema version 4 left
    /// it, with one analysis of `track`, confidence included, stored at its
    /// current path.
    pub(crate) fn create_version_4_database(database: &Path, track: &Path) {
        use super::identity::FileStamp;
        use super::{ContentHash, MIGRATIONS};
        use crate::energy::ENERGY_MODEL_VERSION;

        let stamp = FileStamp::of(track).unwrap();
        let connection = rusqlite::Connection::open(database).unwrap();
        for migration in &MIGRATIONS[..4] {
            connection.execute_batch(migration).unwrap();
        }
        connection.pragma_update(None, "user_version", 4).unwrap();
        connection
            .execute(
                "INSERT INTO analyses
                 VALUES (?1, 300.0, 'A minor', 124.0, ?2, 0, ?3, ?4, ?5, 6, ?6, -12.5,
                         '0\t300\tA minor', 'C major', 0.25, 124.0, 0.4)",
                rusqlite::params![
                    ContentHash::of_file(track).unwrap().as_bytes(),
                    VERSION_4_ANALYZER,
                    track.to_str().unwrap(),
                    stamp.size,
                    stamp.modified_nanoseconds,
                    ENERGY_MODEL_VERSION,
                ],
            )
            .unwrap();
    }

    /// An analyzer version that schema version 3 databases were filled by.
    pub(crate) const VERSION_3_ANALYZER: &str = "5+libkeyfinder";

    /// Creates a database as the last release with schema version 3 left
    /// it, with one analysis of `track` stored at its current path.
    pub(crate) fn create_version_3_database(database: &Path, track: &Path) {
        use super::identity::FileStamp;
        use super::{ContentHash, MIGRATIONS};
        use crate::energy::ENERGY_MODEL_VERSION;

        let stamp = FileStamp::of(track).unwrap();
        let connection = rusqlite::Connection::open(database).unwrap();
        for migration in &MIGRATIONS[..3] {
            connection.execute_batch(migration).unwrap();
        }
        connection.pragma_update(None, "user_version", 3).unwrap();
        connection
            .execute(
                "INSERT INTO analyses
                 VALUES (?1, 300.0, 'A minor', 124.0, ?2, 0, ?3, ?4, ?5, 6, ?6, -12.5,
                         '0\t300\tA minor')",
                rusqlite::params![
                    ContentHash::of_file(track).unwrap().as_bytes(),
                    VERSION_3_ANALYZER,
                    track.to_str().unwrap(),
                    stamp.size,
                    stamp.modified_nanoseconds,
                    ENERGY_MODEL_VERSION,
                ],
            )
            .unwrap();
    }

    /// An analyzer version that schema version 2 databases were filled by.
    pub(crate) const VERSION_2_ANALYZER: &str = "4+libkeyfinder";

    /// Creates a database as the last release with schema version 2 left
    /// it, with one analysis of `track` stored at its current path.
    pub(crate) fn create_version_2_database(database: &Path, track: &Path) {
        use super::identity::FileStamp;
        use super::{ContentHash, MIGRATIONS};
        use crate::energy::ENERGY_MODEL_VERSION;

        let stamp = FileStamp::of(track).unwrap();
        let connection = rusqlite::Connection::open(database).unwrap();
        connection.execute_batch(MIGRATIONS[0]).unwrap();
        connection.execute_batch(MIGRATIONS[1]).unwrap();
        connection.pragma_update(None, "user_version", 2).unwrap();
        connection
            .execute(
                "INSERT INTO analyses
                 VALUES (?1, 300.0, 'A minor', 124.0, ?2, 0, ?3, ?4, ?5, 6, ?6)",
                rusqlite::params![
                    ContentHash::of_file(track).unwrap().as_bytes(),
                    VERSION_2_ANALYZER,
                    track.to_str().unwrap(),
                    stamp.size,
                    stamp.modified_nanoseconds,
                    ENERGY_MODEL_VERSION,
                ],
            )
            .unwrap();
    }

    /// The analyzer version that schema version 1 databases were filled by.
    pub(crate) const VERSION_1_ANALYZER: &str = "1+libkeyfinder";

    /// Creates a database as the first release of the cache left it: schema
    /// version 1, with one analysis of `track` stored at its current path.
    pub(crate) fn create_version_1_database(database: &Path, track: &Path) {
        use super::identity::FileStamp;
        use super::{ContentHash, MIGRATIONS};

        let stamp = FileStamp::of(track).unwrap();
        let connection = rusqlite::Connection::open(database).unwrap();
        connection.execute_batch(MIGRATIONS[0]).unwrap();
        connection.pragma_update(None, "user_version", 1).unwrap();
        connection
            .execute(
                "INSERT INTO analyses VALUES (?1, 300.0, 'A minor', 124.0, ?2, 0, ?3, ?4, ?5)",
                rusqlite::params![
                    ContentHash::of_file(track).unwrap().as_bytes(),
                    VERSION_1_ANALYZER,
                    track.to_str().unwrap(),
                    stamp.size,
                    stamp.modified_nanoseconds,
                ],
            )
            .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::TemporaryFolder;
    use super::*;

    #[test]
    fn opening_creates_the_folder_and_an_empty_database() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("nested").join("cache.sqlite3");

        let cache = Cache::open(&database).unwrap();

        assert_eq!(cache.path(), database);
        assert_eq!(cache.entry_count().unwrap(), 0);
        let schema_version: u32 = cache
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn reopening_keeps_what_was_stored() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        let track = folder.path().join("track.wav");
        std::fs::write(&track, b"audio").unwrap();
        let location = FileLocation {
            absolute_path: track.clone(),
            stamp: FileStamp::of(&track).unwrap(),
        };
        let analysis = TrackAnalysis {
            duration_seconds: 301.25,
            key: Some("F# minor".parse().unwrap()),
            bpm: Some(127.98),
            energy: Some(7),
            tuning_cents: Some(-31.37),
            key_runner_up: Some("A major".parse().unwrap()),
            key_score_margin: Some(0.012_5),
            tempo_estimate: Some(63.99),
            tempo_score_margin: Some(0.47),
            beat_grid: Some(BeatGrid {
                first_downbeat_seconds: 0.371_25,
                bpm: 127.993_75,
                salience: 11.5,
                drift_seconds: Some(0.012_5),
            }),
            cues: vec![
                SectionCue {
                    seconds: 0.371_25,
                    section: CueSection::Start,
                },
                SectionCue {
                    seconds: 60.371_25,
                    section: CueSection::Main,
                },
                SectionCue {
                    seconds: 240.371_25,
                    section: CueSection::Outro,
                },
            ],
            key_timeline: vec![
                KeySection {
                    start_seconds: 0.0,
                    end_seconds: 187.062_5,
                    key: "F# minor".parse().unwrap(),
                },
                KeySection {
                    start_seconds: 187.062_5,
                    end_seconds: 301.25,
                    key: "A major".parse().unwrap(),
                },
            ],
            quality: Some(QualityReport {
                codec: "aac".to_string(),
                lossless: false,
                bitrate_kbps: Some(256),
                sample_rate: 44_100,
                bit_depth: None,
                channels: Some(2),
                cutoff_hz: Some(16_451.17),
                clipped_share: 0.000_123_4,
                integrated_loudness_lufs: Some(-8.75),
                true_peak_dbtp: Some(1.04),
                verdict: crate::quality::QualityVerdict::LikelyReencoded,
            }),
        };

        let mut cache = Cache::open(&database).unwrap();
        cache
            .apply_write(&CacheWrite::Store {
                content_hash: ContentHash::of_file(&track).unwrap(),
                analysis: Box::new(analysis.clone()),
                location: location.clone(),
            })
            .unwrap();
        drop(cache);

        let snapshot = Cache::open(&database).unwrap().snapshot().unwrap();
        assert_eq!(snapshot.at_location(&location), Some(&analysis));
    }

    #[test]
    fn a_file_that_is_not_a_database_is_an_error_naming_it() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        std::fs::write(&database, "this is not a database, ".repeat(400)).unwrap();

        let error = Cache::open(&database).unwrap_err();

        assert!(matches!(&error, EuphoriaError::Cache { path, .. } if *path == database));
        let description = error.chain();
        assert!(
            description.contains(database.to_str().unwrap()),
            "{description}"
        );
        assert!(description.contains("not a database"), "{description}");
    }

    #[test]
    fn a_database_that_cannot_be_opened_is_an_error_naming_it() {
        let folder = TemporaryFolder::new();

        let error = Cache::open(folder.path()).unwrap_err();

        assert!(matches!(&error, EuphoriaError::Cache { path, .. } if path == folder.path()));
    }

    #[test]
    fn a_damaged_row_is_an_error_and_not_a_panic() {
        let folder = TemporaryFolder::new();
        let cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        cache
            .connection
            .execute(
                "INSERT INTO analyses
                 VALUES (?1, 1.0, 'H dorian', NULL, ?2, 0, NULL, 1, 1, NULL, ?3, NULL, NULL,
                         NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)",
                params![[7u8; 32], ANALYZER_VERSION, ENERGY_MODEL_VERSION],
            )
            .unwrap();

        let description = cache.snapshot().unwrap_err().chain();

        assert!(description.contains("H dorian"), "{description}");
    }

    #[test]
    fn an_energy_rating_outside_the_scale_is_an_error_and_not_a_panic() {
        let folder = TemporaryFolder::new();
        let cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        cache
            .connection
            .execute(
                "INSERT INTO analyses
                 VALUES (?1, 1.0, NULL, NULL, ?2, 0, NULL, 1, 1, 11, ?3, NULL, NULL,
                         NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)",
                params![[7u8; 32], ANALYZER_VERSION, ENERGY_MODEL_VERSION],
            )
            .unwrap();

        let description = cache.snapshot().unwrap_err().chain();

        assert!(description.contains("energy rating 11"), "{description}");
    }

    #[test]
    fn a_track_with_no_energy_rating_keeps_none_and_records_the_model() {
        let folder = TemporaryFolder::new();
        let track = folder.path().join("silence.wav");
        let mut cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        store_track(&mut cache, &track, "silence");

        let snapshot = cache.snapshot().unwrap();
        let stored = snapshot
            .of_content(&ContentHash::of_file(&track).unwrap())
            .unwrap();
        assert_eq!(stored.energy, None);
        let recorded_model: u32 = cache
            .connection
            .query_row("SELECT energy_model_version FROM analyses", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(recorded_model, ENERGY_MODEL_VERSION);
    }

    #[test]
    fn a_version_1_database_upgrades_in_place_and_keeps_its_rows() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        let track = folder.path().join("track.wav");
        std::fs::write(&track, b"audio").unwrap();
        test_support::create_version_1_database(&database, &track);

        let cache = Cache::open(&database).unwrap();

        assert_eq!(schema_version(&cache.connection).unwrap(), SCHEMA_VERSION);
        assert_eq!(cache.entry_count().unwrap(), 1);
        let (key, bpm, energy, energy_model_version): (String, f64, Option<u8>, Option<u32>) =
            cache
                .connection
                .query_row(
                    "SELECT key, bpm, energy, energy_model_version FROM analyses",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap();
        assert_eq!(key, "A minor");
        assert_eq!(bpm, 124.0);
        assert_eq!(energy, None);
        assert_eq!(energy_model_version, None);
    }

    #[test]
    fn rows_from_a_version_1_database_are_not_served() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        let track = folder.path().join("track.wav");
        std::fs::write(&track, b"audio").unwrap();
        test_support::create_version_1_database(&database, &track);

        let mut cache = Cache::open(&database).unwrap();
        assert!(cache.snapshot().unwrap().analyses.is_empty());

        // Even an analyzer that kept the old version name does not get rows
        // that have no energy model recorded.
        cache.analyzer_version = test_support::VERSION_1_ANALYZER.to_string();
        assert!(cache.snapshot().unwrap().analyses.is_empty());
    }

    #[test]
    fn a_version_2_database_upgrades_in_place_and_keeps_its_rows() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        let track = folder.path().join("track.wav");
        std::fs::write(&track, b"audio").unwrap();
        test_support::create_version_2_database(&database, &track);

        let cache = Cache::open(&database).unwrap();

        assert_eq!(schema_version(&cache.connection).unwrap(), SCHEMA_VERSION);
        assert_eq!(cache.entry_count().unwrap(), 1);
        let (key, energy, tuning_cents, key_timeline): (String, u8, Option<f64>, Option<String>) =
            cache
                .connection
                .query_row(
                    "SELECT key, energy, tuning_cents, key_timeline FROM analyses",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .unwrap();
        assert_eq!(key, "A minor");
        assert_eq!(energy, 6);
        assert_eq!(tuning_cents, None);
        assert_eq!(key_timeline, None);
    }

    #[test]
    fn rows_from_a_version_2_database_are_analyzed_again_and_read_without_error() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        let track = folder.path().join("track.wav");
        std::fs::write(&track, b"audio").unwrap();
        test_support::create_version_2_database(&database, &track);

        let mut cache = Cache::open(&database).unwrap();
        assert_ne!(ANALYZER_VERSION, test_support::VERSION_2_ANALYZER);
        assert!(
            cache.snapshot().unwrap().analyses.is_empty(),
            "an analysis from before tuning and the key over time were measured must not be served"
        );

        // A row that has neither reads as a track without them.
        cache.analyzer_version = test_support::VERSION_2_ANALYZER.to_string();
        let snapshot = cache.snapshot().unwrap();
        let stored = snapshot
            .of_content(&ContentHash::of_file(&track).unwrap())
            .unwrap();
        assert_eq!(stored.key, Some("A minor".parse().unwrap()));
        assert_eq!(stored.tuning_cents, None);
        assert!(stored.key_timeline.is_empty());
    }

    #[test]
    fn a_version_4_database_upgrades_in_place_and_its_rows_are_analyzed_again() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        let track = folder.path().join("track.wav");
        std::fs::write(&track, b"audio").unwrap();
        test_support::create_version_4_database(&database, &track);

        let mut cache = Cache::open(&database).unwrap();

        assert_eq!(schema_version(&cache.connection).unwrap(), SCHEMA_VERSION);
        assert_eq!(cache.entry_count().unwrap(), 1);
        assert_ne!(ANALYZER_VERSION, test_support::VERSION_4_ANALYZER);
        assert!(
            cache.snapshot().unwrap().analyses.is_empty(),
            "an analysis from before audio quality was measured must not be served"
        );

        // The row keeps its confidence and reads as a track without a quality report.
        cache.analyzer_version = test_support::VERSION_4_ANALYZER.to_string();
        let snapshot = cache.snapshot().unwrap();
        let stored = snapshot
            .of_content(&ContentHash::of_file(&track).unwrap())
            .unwrap();
        assert_eq!(stored.key, Some("A minor".parse().unwrap()));
        assert_eq!(stored.key_runner_up, Some("C major".parse().unwrap()));
        assert_eq!(stored.key_score_margin, Some(0.25));
        assert_eq!(stored.tempo_estimate, Some(124.0));
        assert_eq!(stored.tempo_score_margin, Some(0.4));
        assert_eq!(stored.quality, None);
    }

    #[test]
    fn a_version_3_database_upgrades_in_place_and_its_rows_are_analyzed_again() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        let track = folder.path().join("track.wav");
        std::fs::write(&track, b"audio").unwrap();
        test_support::create_version_3_database(&database, &track);

        let mut cache = Cache::open(&database).unwrap();

        assert_eq!(schema_version(&cache.connection).unwrap(), SCHEMA_VERSION);
        assert_eq!(cache.entry_count().unwrap(), 1);
        assert_ne!(ANALYZER_VERSION, test_support::VERSION_3_ANALYZER);
        assert!(
            cache.snapshot().unwrap().analyses.is_empty(),
            "an analysis from before confidence was measured must not be served"
        );

        // A row that has none of the new values reads as a track without them.
        cache.analyzer_version = test_support::VERSION_3_ANALYZER.to_string();
        let snapshot = cache.snapshot().unwrap();
        let stored = snapshot
            .of_content(&ContentHash::of_file(&track).unwrap())
            .unwrap();
        assert_eq!(stored.key, Some("A minor".parse().unwrap()));
        assert_eq!(stored.tuning_cents, Some(-12.5));
        assert_eq!(stored.key_timeline.len(), 1);
        assert_eq!(stored.key_runner_up, None);
        assert_eq!(stored.key_score_margin, None);
        assert_eq!(stored.tempo_estimate, None);
        assert_eq!(stored.tempo_score_margin, None);
        assert_eq!(stored.key_confidence(), None);
        assert_eq!(stored.tempo_confidence(), None);
    }

    #[test]
    fn a_tempo_estimate_that_is_not_a_positive_number_is_an_error_and_not_a_panic() {
        for damaged in [0.0, -120.0] {
            let folder = TemporaryFolder::new();
            let cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
            cache
                .connection
                .execute(
                    "INSERT INTO analyses
                     VALUES (?1, 1.0, NULL, 120.0, ?2, 0, NULL, 1, 1, NULL, ?3, NULL, NULL,
                             NULL, NULL, ?4, NULL, NULL, NULL, NULL, NULL, NULL, NULL)",
                    params![[7u8; 32], ANALYZER_VERSION, ENERGY_MODEL_VERSION, damaged],
                )
                .unwrap();

            let description = cache.snapshot().unwrap_err().chain();

            assert!(
                description.contains("is not a positive number"),
                "{damaged}: {description}"
            );
        }
    }

    #[test]
    fn a_damaged_runner_up_key_is_an_error_and_not_a_panic() {
        let folder = TemporaryFolder::new();
        let cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        cache
            .connection
            .execute(
                "INSERT INTO analyses
                 VALUES (?1, 1.0, 'A minor', NULL, ?2, 0, NULL, 1, 1, NULL, ?3, NULL, NULL,
                         'H lydian', 0.01, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)",
                params![[7u8; 32], ANALYZER_VERSION, ENERGY_MODEL_VERSION],
            )
            .unwrap();

        let description = cache.snapshot().unwrap_err().chain();

        assert!(description.contains("H lydian"), "{description}");
    }

    #[test]
    fn a_damaged_key_timeline_is_an_error_and_not_a_panic() {
        for damaged in [
            "0\t10",
            "0\tten\tA minor",
            "0\t10\tH dorian",
            "0\t10\tA minor\textra",
            "0\tNaN\tA minor",
        ] {
            let folder = TemporaryFolder::new();
            let cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
            cache
                .connection
                .execute(
                    "INSERT INTO analyses
                     VALUES (?1, 1.0, NULL, NULL, ?2, 0, NULL, 1, 1, NULL, ?3, NULL, ?4,
                             NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)",
                    params![[7u8; 32], ANALYZER_VERSION, ENERGY_MODEL_VERSION, damaged],
                )
                .unwrap();

            let description = cache.snapshot().unwrap_err().chain();

            assert!(description.contains("key timeline"), "{description}");
        }
    }

    #[test]
    fn a_damaged_beat_grid_is_an_error_and_not_a_panic() {
        for (first_downbeat, bpm, salience, drift) in [
            ("0.2", "0.0", "10.0", "NULL"),
            ("0.2", "-128.0", "10.0", "NULL"),
            ("-0.5", "128.0", "10.0", "NULL"),
            ("0.2", "128.0", "10.0", "-0.01"),
            ("NULL", "128.0", "10.0", "NULL"),
            ("0.2", "NULL", "10.0", "0.01"),
            ("0.2", "128.0", "NULL", "NULL"),
            ("NULL", "NULL", "NULL", "0.01"),
        ] {
            let folder = TemporaryFolder::new();
            let cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
            cache
                .connection
                .execute(
                    &format!(
                        "INSERT INTO analyses
                         VALUES (?1, 1.0, NULL, NULL, ?2, 0, NULL, 1, 1, NULL, ?3, NULL, NULL,
                                 NULL, NULL, NULL, NULL, NULL,
                                 {first_downbeat}, {bpm}, {salience}, {drift}, NULL)"
                    ),
                    params![[7u8; 32], ANALYZER_VERSION, ENERGY_MODEL_VERSION],
                )
                .unwrap();

            let description = cache.snapshot().unwrap_err().chain();

            assert!(
                description.contains("is not a usable grid")
                    || description.contains("is only partly stored"),
                "{bpm} BPM from {first_downbeat}: {description}"
            );
        }
    }

    #[test]
    fn a_track_without_a_grid_reads_back_without_one() {
        let folder = TemporaryFolder::new();
        let cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        cache
            .connection
            .execute(
                "INSERT INTO analyses
                 VALUES (?1, 1.0, NULL, NULL, ?2, 0, NULL, 1, 1, NULL, ?3, NULL, NULL,
                         NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)",
                params![[7u8; 32], ANALYZER_VERSION, ENERGY_MODEL_VERSION],
            )
            .unwrap();

        let snapshot = cache.snapshot().unwrap();

        let stored = snapshot.analyses.values().next().unwrap();
        assert_eq!(stored.beat_grid, None);
    }

    #[test]
    fn a_version_6_database_gains_corrections_and_keeps_its_analyses_current() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        let connection = Connection::open(&database).unwrap();
        for migration in &MIGRATIONS[..6] {
            connection.execute_batch(migration).unwrap();
        }
        connection.pragma_update(None, "user_version", 6).unwrap();
        connection
            .execute(
                "INSERT INTO analyses
                 VALUES (?1, 300.0, 'A minor', 124.0, ?2, 0, NULL, 1, 1, 6, ?3, NULL, NULL,
                         NULL, NULL, 124.0, 0.5, NULL, NULL, NULL, NULL, NULL)",
                params![[7u8; 32], ANALYZER_VERSION, ENERGY_MODEL_VERSION],
            )
            .unwrap();
        drop(connection);

        let cache = Cache::open(&database).unwrap();

        assert_eq!(schema_version(&cache.connection).unwrap(), SCHEMA_VERSION);
        assert_eq!(SCHEMA_VERSION, 9);
        let snapshot = cache.snapshot().unwrap();
        assert_eq!(
            snapshot.analyses.len(),
            1,
            "corrections need no new analysis: the analyzer version is unchanged"
        );
        let corrections: i64 = cache
            .connection
            .query_row("SELECT count(*) FROM corrections", [], |row| row.get(0))
            .unwrap();
        assert_eq!(corrections, 0);
    }

    #[test]
    fn a_version_7_database_gains_cues_keeps_its_corrections_and_its_rows_are_analyzed_again() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        let connection = Connection::open(&database).unwrap();
        for migration in &MIGRATIONS[..7] {
            connection.execute_batch(migration).unwrap();
        }
        connection.pragma_update(None, "user_version", 7).unwrap();
        let version_7_analyzer = "9+libkeyfinder";
        connection
            .execute(
                "INSERT INTO analyses
                 VALUES (?1, 300.0, 'A minor', 124.0, ?2, 0, NULL, 1, 1, 6, ?3, NULL, NULL,
                         NULL, NULL, 124.0, 0.5, NULL, 0.25, 124.0, 10.0, NULL)",
                params![[7u8; 32], version_7_analyzer, ENERGY_MODEL_VERSION],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO corrections VALUES (?1, 'C major', NULL, 0)",
                params![[7u8; 32]],
            )
            .unwrap();
        drop(connection);

        let mut cache = Cache::open(&database).unwrap();

        assert_eq!(schema_version(&cache.connection).unwrap(), SCHEMA_VERSION);
        assert_eq!(SCHEMA_VERSION, 9);
        assert_ne!(ANALYZER_VERSION, version_7_analyzer);
        assert!(
            cache.snapshot().unwrap().analyses.is_empty(),
            "an analysis from before the cues must not be served"
        );
        let corrections: i64 = cache
            .connection
            .query_row("SELECT count(*) FROM corrections", [], |row| row.get(0))
            .unwrap();
        assert_eq!(corrections, 1, "corrections survive the upgrade");
        cache.analyzer_version = version_7_analyzer.to_string();
        let snapshot = cache.snapshot().unwrap();
        let stored = snapshot.analyses.values().next().unwrap();
        assert!(stored.beat_grid.is_some());
        assert!(stored.cues.is_empty());
    }

    #[test]
    fn a_version_8_database_gains_edited_cues_and_keeps_its_analyses_and_corrections() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        let track = folder.path().join("track.mp3");
        std::fs::write(&track, b"audio").unwrap();
        let content_hash = ContentHash::of_file(&track).unwrap();
        let connection = Connection::open(&database).unwrap();
        for migration in &MIGRATIONS[..8] {
            connection.execute_batch(migration).unwrap();
        }
        connection.pragma_update(None, "user_version", 8).unwrap();
        connection
            .execute(
                "INSERT INTO analyses
                 VALUES (?1, 300.0, 'A minor', 124.0, ?2, 0, NULL, 1, 1, 6, ?3, NULL, NULL,
                         NULL, NULL, 124.0, 0.5, NULL, 0.25, 124.0, 10.0, NULL, ?4)",
                params![
                    content_hash.as_bytes(),
                    ANALYZER_VERSION,
                    ENERGY_MODEL_VERSION,
                    "0.25\tStart"
                ],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO corrections VALUES (?1, 'C major', NULL, 0)",
                params![content_hash.as_bytes()],
            )
            .unwrap();
        drop(connection);

        let mut cache = Cache::open(&database).unwrap();

        assert_eq!(schema_version(&cache.connection).unwrap(), SCHEMA_VERSION);
        assert_eq!(SCHEMA_VERSION, 9);
        let snapshot = cache.snapshot().unwrap();
        assert_eq!(
            snapshot.analyses.len(),
            1,
            "edited cues need no new analysis: the analyzer version is unchanged"
        );
        let analysis = snapshot.analyses.values().next().unwrap().clone();
        assert_eq!(analysis.cues.len(), 1);
        assert!(cache.correction_for(&track).unwrap().is_some());
        let automatic = cache.track_cues_for(&track, &analysis).unwrap();
        assert!(!automatic.is_edited());
        assert_eq!(automatic.cues[0].name, "Start");

        cache.set_edited_cues(&track, Some(&[])).unwrap();
        assert!(cache.track_cues_for(&track, &analysis).unwrap().is_edited());
    }

    #[test]
    fn a_version_5_database_upgrades_in_place_and_its_rows_are_analyzed_again() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        let connection = Connection::open(&database).unwrap();
        for migration in &MIGRATIONS[..5] {
            connection.execute_batch(migration).unwrap();
        }
        connection.pragma_update(None, "user_version", 5).unwrap();
        let version_5_analyzer = "7+libkeyfinder";
        connection
            .execute(
                "INSERT INTO analyses
                 VALUES (?1, 300.0, 'A minor', 124.0, ?2, 0, NULL, 1, 1, 6, ?3, NULL, NULL,
                         NULL, NULL, 62.0, 0.5, NULL)",
                params![[7u8; 32], version_5_analyzer, ENERGY_MODEL_VERSION],
            )
            .unwrap();
        drop(connection);

        let mut cache = Cache::open(&database).unwrap();

        assert_eq!(schema_version(&cache.connection).unwrap(), SCHEMA_VERSION);
        assert_eq!(cache.entry_count().unwrap(), 1);
        assert_ne!(ANALYZER_VERSION, version_5_analyzer);
        assert!(
            cache.snapshot().unwrap().analyses.is_empty(),
            "an analysis from before the beat grid must not be served"
        );
        cache.analyzer_version = version_5_analyzer.to_string();
        let snapshot = cache.snapshot().unwrap();
        let stored = snapshot.analyses.values().next().unwrap();
        assert_eq!(stored.key, Some("A minor".parse().unwrap()));
        assert_eq!(stored.beat_grid, None);
    }

    #[test]
    fn a_damaged_cue_is_an_error_and_not_a_panic() {
        for cues in ["'12.5'", "'12.5\tChorus'", "'NaN\tStart'", "'-1\tStart'"] {
            let folder = TemporaryFolder::new();
            let cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
            cache
                .connection
                .execute(
                    &format!(
                        "INSERT INTO analyses
                         VALUES (?1, 1.0, NULL, NULL, ?2, 0, NULL, 1, 1, NULL, ?3, NULL, NULL,
                                 NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, {cues})"
                    ),
                    params![[7u8; 32], ANALYZER_VERSION, ENERGY_MODEL_VERSION],
                )
                .unwrap();

            let description = cache.snapshot().unwrap_err().chain();

            assert!(
                description.contains("is not a cue"),
                "{cues}: {description}"
            );
        }
    }

    #[test]
    fn a_damaged_quality_report_is_an_error_and_not_a_panic() {
        let folder = TemporaryFolder::new();
        let cache = Cache::open(&folder.path().join("cache.sqlite3")).unwrap();
        cache
            .connection
            .execute(
                "INSERT INTO analyses
                 VALUES (?1, 1.0, NULL, NULL, ?2, 0, NULL, 1, 1, NULL, ?3, NULL, NULL, NULL, NULL, NULL, NULL, '{\"codec\": 7}', NULL, NULL, NULL, NULL, NULL)",
                params![[7u8; 32], ANALYZER_VERSION, ENERGY_MODEL_VERSION],
            )
            .unwrap();

        let description = cache.snapshot().unwrap_err().chain();

        assert!(description.contains("expected a string"), "{description}");
    }

    #[test]
    fn a_key_timeline_keeps_every_digit_when_stored() {
        let timeline = vec![
            KeySection {
                start_seconds: 0.0,
                end_seconds: 45.046_71,
                key: "C major".parse().unwrap(),
            },
            KeySection {
                start_seconds: 45.046_71,
                end_seconds: 1.0 / 3.0 + 80.0,
                key: "F# minor".parse().unwrap(),
            },
        ];
        let text = key_timeline_text(&timeline).unwrap();
        let read: Vec<KeySection> = text
            .lines()
            .map(|line| parse_key_section(line).unwrap())
            .collect();
        assert_eq!(read, timeline);
        assert_eq!(key_timeline_text(&[]), None);
    }

    #[test]
    fn a_version_1_database_opened_from_many_threads_at_once_upgrades_for_all() {
        for _ in 0..10 {
            let folder = TemporaryFolder::new();
            let database = folder.path().join("cache.sqlite3");
            let track = folder.path().join("track.wav");
            std::fs::write(&track, b"audio").unwrap();
            test_support::create_version_1_database(&database, &track);
            let start = std::sync::Barrier::new(8);

            let outcomes: Vec<Result<u64, EuphoriaError>> = std::thread::scope(|scope| {
                let openers: Vec<_> = (0..8)
                    .map(|_| {
                        scope.spawn(|| {
                            start.wait();
                            Cache::open(&database)?.entry_count()
                        })
                    })
                    .collect();
                openers
                    .into_iter()
                    .map(|opener| opener.join().unwrap())
                    .collect()
            });

            for outcome in outcomes {
                assert_eq!(outcome.map_err(|error| error.chain()), Ok(1));
            }
        }
    }

    #[test]
    fn a_database_from_a_newer_euphoria_is_refused() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        Cache::open(&database)
            .unwrap()
            .connection
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .unwrap();

        let description = Cache::open(&database).unwrap_err().chain();

        assert!(description.contains("schema version"), "{description}");
    }

    #[test]
    fn a_new_database_opened_from_many_threads_at_once_opens_for_all() {
        for _ in 0..20 {
            let folder = TemporaryFolder::new();
            let database = folder.path().join("cache.sqlite3");
            let start = std::sync::Barrier::new(8);

            let outcomes: Vec<Result<u64, EuphoriaError>> = std::thread::scope(|scope| {
                let openers: Vec<_> = (0..8)
                    .map(|_| {
                        scope.spawn(|| {
                            start.wait();
                            Cache::open(&database)?.entry_count()
                        })
                    })
                    .collect();
                openers
                    .into_iter()
                    .map(|opener| opener.join().unwrap())
                    .collect()
            });

            for outcome in outcomes {
                assert_eq!(outcome.map_err(|error| error.chain()), Ok(0));
            }
        }
    }

    fn store_track(cache: &mut Cache, track: &Path, content: &str) {
        std::fs::write(track, content).unwrap();
        cache
            .apply_write(&CacheWrite::Store {
                content_hash: ContentHash::of_file(track).unwrap(),
                analysis: Box::new(TrackAnalysis {
                    duration_seconds: 1.0,
                    key: None,
                    bpm: None,
                    energy: None,
                    tuning_cents: None,
                    key_runner_up: None,
                    key_score_margin: None,
                    tempo_estimate: None,
                    tempo_score_margin: None,
                    beat_grid: None,
                    cues: Vec::new(),
                    key_timeline: Vec::new(),
                    quality: None,
                }),
                location: FileLocation {
                    absolute_path: track.to_path_buf(),
                    stamp: FileStamp::of(track).unwrap(),
                },
            })
            .unwrap();
    }

    #[test]
    fn clearing_empties_a_database_that_another_connection_keeps_writing_to() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        let mut running_analysis = Cache::open(&database).unwrap();
        store_track(
            &mut running_analysis,
            &folder.path().join("before.wav"),
            "before",
        );

        assert!(clear_cache(&database).unwrap());
        assert!(database.is_file());
        assert_eq!(running_analysis.entry_count().unwrap(), 0);

        store_track(
            &mut running_analysis,
            &folder.path().join("after.wav"),
            "after",
        );
        drop(running_analysis);
        assert_eq!(Cache::open(&database).unwrap().entry_count().unwrap(), 1);
    }

    #[test]
    fn clearing_deletes_a_database_that_cannot_be_opened() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");
        std::fs::write(&database, "this is not a database, ".repeat(400)).unwrap();

        assert!(clear_cache(&database).unwrap());
        assert!(!database.exists());
        assert_eq!(Cache::open(&database).unwrap().entry_count().unwrap(), 0);
    }

    #[test]
    fn clearing_where_there_is_no_database_creates_none() {
        let folder = TemporaryFolder::new();
        let database = folder.path().join("cache.sqlite3");

        assert!(!clear_cache(&database).unwrap());
        assert!(!database.exists());
    }
}
