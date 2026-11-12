// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Round trips of the Engine DJ library update on libraries built from the
//! schemas Engine DJ creates.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use euphoria_core::EuphoriaError;
use euphoria_core::export::engine::{
    EngineOptions, EngineTrack, SUPPORTED_SCHEMAS, SchemaVersion, engine_key_value,
    preview_library_update, update_library,
};
use euphoria_core::export::engine_blobs::{
    BeatData, EMPTY_QUICK_CUE_OFFSET, QuickCue, QuickCues, TrackData, compress,
    constant_beat_grid, decompress,
};
use euphoria_core::export::rekordbox::{CueKind, CuePoint, Track};
use euphoria_core::{Key, Mode};
use rusqlite::{Connection, params};

const SAMPLE_RATE: f64 = 44_100.0;
const SAMPLES: i64 = 44_100 * 300;

/// Each fixture and the schema version Engine DJ wrote in it.
const SCHEMAS: [(&str, SchemaVersion); 4] = [
    ("engine-desktop-2.0.0-m.db.sql", SchemaVersion::new(2, 18, 0)),
    ("engine-desktop-4.0.1-m.db.sql", SchemaVersion::new(2, 21, 2)),
    ("engine-desktop-4.3.0-m.db.sql", SchemaVersion::new(3, 0, 1)),
    ("engine-desktop-5.0.0-m.db.sql", SchemaVersion::new(3, 0, 2)),
];

static NEXT_LIBRARY: AtomicUsize = AtomicUsize::new(0);

/// A folder holding an `Engine Library`, removed when dropped.
struct Library {
    folder: PathBuf,
    database: PathBuf,
}

impl Library {
    fn new(schema_file: &str) -> Self {
        let folder = std::env::temp_dir().join(format!(
            "euphoria-engine-{}-{}",
            std::process::id(),
            NEXT_LIBRARY.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&folder);
        let database_folder = folder.join("Engine Library").join("Database2");
        std::fs::create_dir_all(&database_folder).expect("the library folder is made");
        let database = database_folder.join("m.db");
        let schema = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/engine")
                .join(schema_file),
        )
        .expect("the schema is read");
        Connection::open(&database)
            .and_then(|connection| connection.execute_batch(&schema))
            .expect("the library is built");
        Self { folder, database }
    }

    fn connection(&self) -> Connection {
        Connection::open(&self.database).expect("the library opens")
    }

    fn track_path(&self, name: &str) -> PathBuf {
        self.folder.join("Music").join(name)
    }

    fn backup(&self, name: &str) -> PathBuf {
        self.folder.join(name).join("m.db")
    }

    /// Adds a track at `Music/<name>`, analysed by Engine DJ or not.
    fn add_track(&self, name: &str, analysed: bool) -> i64 {
        let connection = self.connection();
        connection
            .execute(
                "INSERT INTO Track (path, filename, length, bpm, bpmAnalyzed, key, comment, isAnalyzed, isAvailable) \
                 VALUES (?1, ?2, 300, 100, 100.0, 5, 'my note', ?3, 1)",
                params![format!("../Music/{name}"), name, analysed],
            )
            .expect("the track is added");
        let track_id = connection.last_insert_rowid();
        if analysed {
            let grid = constant_beat_grid(SAMPLE_RATE, SAMPLES, 0.1, 100.0);
            connection
                .execute(
                    "UPDATE PerformanceData SET trackData = ?1, beatData = ?2, quickCues = ?3 WHERE trackId = ?4",
                    params![
                        compress(&engine_track_data().encode()),
                        compress(
                            &BeatData {
                                sample_rate: SAMPLE_RATE,
                                samples: SAMPLES as f64,
                                is_beatgrid_set: 1,
                                default_grid: grid.clone(),
                                adjusted_grid: grid,
                                extra: vec![0; 9],
                            }
                            .encode()
                        ),
                        compress(&empty_quick_cues().encode()),
                        track_id
                    ],
                )
                .expect("the performance data is set");
        }
        track_id
    }

    fn track_row(&self, track_id: i64) -> (i64, i64, f64, String) {
        self.connection()
            .query_row(
                "SELECT key, bpm, bpmAnalyzed, comment FROM Track WHERE id = ?1",
                params![track_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .expect("the track is read")
    }

    fn blob(&self, track_id: i64, column: &str) -> Option<Vec<u8>> {
        self.connection()
            .query_row(
                &format!("SELECT {column} FROM PerformanceData WHERE trackId = ?1"),
                params![track_id],
                |row| row.get(0),
            )
            .expect("the performance data is read")
    }

    fn everything_about(&self, track_id: i64) -> Vec<String> {
        let connection = self.connection();
        let mut values = Vec::new();
        for table in ["Track", "PerformanceData"] {
            let column = if table == "Track" { "id" } else { "trackId" };
            let mut statement = connection
                .prepare(&format!("SELECT * FROM {table} WHERE {column} = ?1"))
                .expect("the row is read");
            let count = statement.column_count();
            statement
                .query_row(params![track_id], |row| {
                    for index in 0..count {
                        values.push(format!("{:?}", row.get_ref(index)?));
                    }
                    Ok(())
                })
                .expect("the row exists");
        }
        values
    }
}

impl Drop for Library {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

fn engine_track_data() -> TrackData {
    TrackData {
        sample_rate: SAMPLE_RATE,
        samples: SAMPLES,
        key: 5,
        average_loudness: [0.5; 3],
        extra: Vec::new(),
    }
}

fn empty_quick_cues() -> QuickCues {
    QuickCues {
        cues: (0..8)
            .map(|_| QuickCue {
                label: String::new(),
                sample_offset: EMPTY_QUICK_CUE_OFFSET,
                colour: [0; 4],
            })
            .collect(),
        adjusted_main_cue: EMPTY_QUICK_CUE_OFFSET,
        is_main_cue_adjusted: 0,
        default_main_cue: EMPTY_QUICK_CUE_OFFSET,
        extra: Vec::new(),
    }
}

fn exported_track(path: PathBuf) -> Track {
    let drop = |kind| CuePoint {
        name: "Drop".to_string(),
        time: Duration::from_secs(30),
        kind,
    };
    Track {
        key: Some(Key::new(9, Mode::Minor)),
        bpm: Some(128.0),
        first_beat: Some(Duration::from_millis(250)),
        cue_points: vec![drop(CueKind::Memory), drop(CueKind::Hot(1))],
        ..Track::new(path, Duration::from_secs(300))
    }
}

fn entries(tracks: &[Track]) -> Vec<EngineTrack<'_>> {
    tracks
        .iter()
        .map(|track| EngineTrack {
            track,
            energy: Some(7),
        })
        .collect()
}

fn decoded<T>(blob: Option<Vec<u8>>, decode: fn(&[u8]) -> Result<T, String>) -> T {
    decode(&decompress(&blob.expect("a blob")).expect("the blob inflates")).expect("it decodes")
}

#[test]
fn an_analysed_track_gets_key_tempo_grid_cues_and_label() {
    for (schema_file, version) in SCHEMAS {
        let library = Library::new(schema_file);
        let track_id = library.add_track("a.mp3", true);
        let tracks = [exported_track(library.track_path("a.mp3"))];
        let backup = library.backup("backup");

        let update = update_library(
            &library.database,
            &backup,
            &entries(&tracks),
            &EngineOptions::default(),
        )
        .unwrap_or_else(|error| panic!("{schema_file}: {error}"));

        assert_eq!(update.schema, Some(version), "{schema_file}");
        assert_eq!(update.updated, vec![tracks[0].path.clone()], "{schema_file}");
        assert_eq!(
            library.track_row(track_id),
            (1, 128, 128.0, "8A - Energy 7 - my note".to_string()),
            "{schema_file}"
        );
        let track_data = decoded(library.blob(track_id, "trackData"), TrackData::decode);
        assert_eq!(
            track_data,
            TrackData {
                key: 1,
                ..engine_track_data()
            },
            "{schema_file}"
        );
        let beat_data = decoded(library.blob(track_id, "beatData"), BeatData::decode);
        let grid = constant_beat_grid(SAMPLE_RATE, SAMPLES, 0.25, 128.0);
        assert_eq!(beat_data.default_grid, grid, "{schema_file}");
        assert_eq!(beat_data.adjusted_grid, grid, "{schema_file}");
        assert_eq!(beat_data.is_beatgrid_set, 1);
        assert_eq!(beat_data.extra, vec![0; 9]);
        let quick_cues = decoded(library.blob(track_id, "quickCues"), QuickCues::decode);
        assert_eq!(quick_cues.cues.len(), 8);
        assert_eq!(
            quick_cues.cues[1],
            QuickCue {
                label: "Drop".to_string(),
                sample_offset: 30.0 * SAMPLE_RATE,
                colour: [0xFF, 0xEF, 0x81, 0x30],
            }
        );
        for (pad, cue) in quick_cues.cues.iter().enumerate() {
            if pad != 1 {
                assert_eq!(cue.sample_offset, EMPTY_QUICK_CUE_OFFSET, "pad {pad}");
            }
        }
        assert_eq!(quick_cues.adjusted_main_cue, EMPTY_QUICK_CUE_OFFSET);
        assert_eq!(quick_cues.default_main_cue, EMPTY_QUICK_CUE_OFFSET);

        let quick_check: String = library
            .connection()
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .expect("the check runs");
        assert_eq!(quick_check, "ok");
        let old: (i64, String) = Connection::open(&backup)
            .and_then(|connection| {
                connection.query_row(
                    "SELECT key, comment FROM Track WHERE id = ?1",
                    params![track_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            })
            .expect("the backup opens");
        assert_eq!(old, (5, "my note".to_string()), "{schema_file}");
    }
}

#[test]
fn a_track_engine_has_not_analysed_gets_key_tempo_and_label_only() {
    for (schema_file, _) in SCHEMAS {
        let library = Library::new(schema_file);
        let track_id = library.add_track("a.mp3", false);
        let before: Vec<Option<Vec<u8>>> = ["trackData", "beatData", "quickCues"]
            .into_iter()
            .map(|column| library.blob(track_id, column))
            .collect();
        let tracks = [exported_track(library.track_path("a.mp3"))];

        let update = update_library(
            &library.database,
            &library.backup("backup"),
            &entries(&tracks),
            &EngineOptions::default(),
        )
        .expect("the update");

        assert_eq!(update.not_analysed, vec![tracks[0].path.clone()]);
        assert_eq!(library.track_row(track_id).0, 1, "{schema_file}");
        let after: Vec<Option<Vec<u8>>> = ["trackData", "beatData", "quickCues"]
            .into_iter()
            .map(|column| library.blob(track_id, column))
            .collect();
        assert_eq!(before, after, "{schema_file}");
    }
}

#[test]
fn a_track_with_its_own_hot_cues_keeps_them() {
    for (schema_file, _) in SCHEMAS {
        let library = Library::new(schema_file);
        let track_id = library.add_track("a.mp3", true);
        let mut own = empty_quick_cues();
        own.cues[0] = QuickCue {
            label: "Mine".to_string(),
            sample_offset: SAMPLE_RATE,
            colour: [0xFF, 0xF4, 0xD3, 0x38],
        };
        let own_blob = compress(&own.encode());
        library
            .connection()
            .execute(
                "UPDATE PerformanceData SET quickCues = ?1 WHERE trackId = ?2",
                params![own_blob, track_id],
            )
            .expect("the cues are set");
        let tracks = [exported_track(library.track_path("a.mp3"))];

        let update = update_library(
            &library.database,
            &library.backup("backup"),
            &entries(&tracks),
            &EngineOptions::default(),
        )
        .expect("the update");

        assert_eq!(update.kept_own_cues, vec![tracks[0].path.clone()]);
        assert_eq!(library.blob(track_id, "quickCues"), Some(own_blob));
        let beat_data = decoded(library.blob(track_id, "beatData"), BeatData::decode);
        assert_eq!(
            beat_data.adjusted_grid,
            constant_beat_grid(SAMPLE_RATE, SAMPLES, 0.25, 128.0),
            "{schema_file}"
        );
    }
}

#[test]
fn other_tracks_and_unknown_tracks() {
    for (schema_file, _) in SCHEMAS {
        let library = Library::new(schema_file);
        library.add_track("a.mp3", true);
        let other = library.add_track("other.mp3", true);
        let before = library.everything_about(other);
        let tracks = [
            exported_track(library.track_path("a.mp3")),
            exported_track(library.track_path("missing.mp3")),
        ];

        let update = update_library(
            &library.database,
            &library.backup("backup"),
            &entries(&tracks),
            &EngineOptions::default(),
        )
        .expect("the update");

        assert_eq!(library.everything_about(other), before, "{schema_file}");
        assert_eq!(update.not_in_library, vec![tracks[1].path.clone()]);
    }
}

#[test]
fn exporting_twice_is_stable() {
    for (schema_file, _) in SCHEMAS {
        let library = Library::new(schema_file);
        let track_id = library.add_track("a.mp3", true);
        let tracks = [exported_track(library.track_path("a.mp3"))];
        let export = |backup: &str| {
            update_library(
                &library.database,
                &library.backup(backup),
                &entries(&tracks),
                &EngineOptions::default(),
            )
            .expect("the update");
        };
        export("first");
        let once = (
            library.track_row(track_id),
            library.blob(track_id, "trackData"),
            library.blob(track_id, "beatData"),
        );
        export("second");
        let twice = (
            library.track_row(track_id),
            library.blob(track_id, "trackData"),
            library.blob(track_id, "beatData"),
        );
        assert_eq!(once, twice, "{schema_file}");
        assert_eq!(twice.0.3.matches("Energy").count(), 1);
    }
}

#[test]
fn a_dry_run_writes_nothing() {
    for (schema_file, _) in SCHEMAS {
        let library = Library::new(schema_file);
        library.add_track("a.mp3", true);
        let before = std::fs::read(&library.database).expect("the database is read");
        let tracks = [exported_track(library.track_path("a.mp3"))];

        let update =
            preview_library_update(&library.database, &entries(&tracks), &EngineOptions::default())
                .expect("the preview");

        assert_eq!(update.updated, vec![tracks[0].path.clone()]);
        assert_eq!(
            std::fs::read(&library.database).expect("the database is read"),
            before,
            "{schema_file}"
        );
    }
}

fn assert_refused(library: &Library, tracks: &[Track], why: &str) {
    let before = std::fs::read(&library.database).expect("the database is read");
    let backup = library.backup("refused");
    let result = update_library(
        &library.database,
        &backup,
        &entries(tracks),
        &EngineOptions::default(),
    );
    assert!(
        matches!(result, Err(EuphoriaError::EngineLibrary { .. })),
        "{why}: {result:?}"
    );
    assert_eq!(
        std::fs::read(&library.database).expect("the database is read"),
        before,
        "{why}"
    );
    assert!(!backup.exists(), "{why}");
}

#[test]
fn unknown_schema_versions_are_refused() {
    let changes = [
        ("a newer schema", "UPDATE Information SET schemaVersionMinor = 99"),
        (
            "two information rows",
            "INSERT INTO Information (uuid, schemaVersionMajor, schemaVersionMinor, schemaVersionPatch) VALUES ('x', 3, 0, 2)",
        ),
        ("an information row with id 2", "UPDATE Information SET id = 2"),
    ];
    for (why, change) in changes {
        let library = Library::new("engine-desktop-5.0.0-m.db.sql");
        library.add_track("a.mp3", true);
        library.connection().execute_batch(change).expect("the change");
        let tracks = [exported_track(library.track_path("a.mp3"))];
        assert_refused(&library, &tracks, why);
    }
}

#[test]
fn a_pending_journal_is_refused() {
    let library = Library::new("engine-desktop-5.0.0-m.db.sql");
    library.add_track("a.mp3", true);
    let tracks = [exported_track(library.track_path("a.mp3"))];
    let journal = library.database.with_file_name("m.db-journal");
    std::fs::write(&journal, [1; 10]).expect("the journal is written");
    assert_refused(&library, &tracks, "a journal with data");

    std::fs::write(&journal, []).expect("the journal is emptied");
    update_library(
        &library.database,
        &library.backup("backup"),
        &entries(&tracks),
        &EngineOptions::default(),
    )
    .expect("an empty journal is no pending write");
}

#[test]
fn an_existing_backup_is_never_replaced() {
    let library = Library::new("engine-desktop-5.0.0-m.db.sql");
    library.add_track("a.mp3", true);
    let tracks = [exported_track(library.track_path("a.mp3"))];
    let backup = library.backup("backup");
    std::fs::create_dir_all(backup.parent().expect("a folder")).expect("the folder is made");
    std::fs::write(&backup, "keep").expect("the backup is written");
    let before = std::fs::read(&library.database).expect("the database is read");

    let result = update_library(
        &library.database,
        &backup,
        &entries(&tracks),
        &EngineOptions::default(),
    );

    assert!(matches!(
        result,
        Err(EuphoriaError::ExportDestinationExists { .. })
    ));
    assert_eq!(std::fs::read_to_string(&backup).expect("the backup"), "keep");
    assert_eq!(
        std::fs::read(&library.database).expect("the database is read"),
        before
    );
}

#[test]
fn supported_schemas_cover_every_fixture() {
    for (schema_file, version) in SCHEMAS {
        let library = Library::new(schema_file);
        let stored: (u32, u32, u32) = library
            .connection()
            .query_row(
                "SELECT schemaVersionMajor, schemaVersionMinor, schemaVersionPatch FROM Information",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("the version is read");
        assert_eq!(
            SchemaVersion::new(stored.0, stored.1, stored.2),
            version,
            "{schema_file}"
        );
        assert!(SUPPORTED_SCHEMAS.contains(&version), "{schema_file}");
    }
}

#[test]
fn key_numbers_match_libdjinterop() {
    let keys = [
        "C major", "A minor", "G major", "E minor", "D major", "B minor", "A major", "F# minor",
        "E major", "C# minor", "B major", "G# minor", "F# major", "Eb minor", "Db major",
        "Bb minor", "Ab major", "F minor", "Eb major", "C minor", "Bb major", "G minor",
        "F major", "D minor",
    ];
    for (value, name) in keys.into_iter().enumerate() {
        let key: Key = name.parse().expect("a key");
        assert_eq!(engine_key_value(key), value as i64, "{name}");
    }
}
