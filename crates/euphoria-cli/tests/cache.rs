// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs the `euphoria` binary against a cache in a temporary folder.

mod support;

use std::path::Path;
use std::process::Output;

use support::{Euphoria, TemporaryFolder, fixtures_directory, standard_error};

const FROM_THE_CACHE: &str = "came from the cache.";

fn text(path: &Path) -> &str {
    path.to_str().expect("UTF-8 path")
}

fn assert_success(output: &Output) {
    assert!(output.status.success(), "{}", standard_error(output));
}

#[test]
fn a_second_run_comes_from_the_cache_and_prints_the_same_results() {
    let euphoria = Euphoria::new();
    let fixtures = fixtures_directory();
    let arguments = ["analyze", "--json", text(&fixtures)];

    let first_run = euphoria.run(&arguments);
    let second_run = euphoria.run(&arguments);

    assert_success(&first_run);
    assert_success(&second_run);
    assert!(!standard_error(&first_run).contains(FROM_THE_CACHE));
    assert_eq!(
        standard_error(&second_run),
        "7 of 7 tracks came from the cache.\n"
    );
    assert_eq!(first_run.stdout, second_run.stdout);
    assert!(euphoria.cache_path().is_file());
}

#[test]
fn a_renamed_and_moved_file_comes_from_the_cache() {
    let euphoria = Euphoria::new();
    let music = TemporaryFolder::new();
    let original = music.path().join("original.flac");
    std::fs::copy(fixtures_directory().join("sine-440hz-mono.flac"), &original)
        .expect("the fixture is copied");
    assert_success(&euphoria.run(&["analyze", text(&original)]));

    let moved = music.path().join("sorted").join("renamed.flac");
    std::fs::create_dir_all(moved.parent().expect("a parent folder")).expect("a folder is created");
    std::fs::rename(&original, &moved).expect("the file is moved");
    let second_run = euphoria.run(&["analyze", text(music.path())]);

    assert_success(&second_run);
    assert_eq!(
        standard_error(&second_run),
        "1 of 1 track came from the cache.\n"
    );
    assert!(String::from_utf8_lossy(&second_run.stdout).contains("renamed.flac"));
}

#[test]
fn no_cache_neither_reads_nor_creates_the_cache() {
    let euphoria = Euphoria::new();
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");

    let without_cache = euphoria.run(&["analyze", "--no-cache", text(&fixture)]);
    assert_success(&without_cache);
    assert!(!euphoria.cache_path().exists());

    assert_success(&euphoria.run(&["analyze", text(&fixture)]));
    let bypassed = euphoria.run(&["analyze", "--no-cache", text(&fixture)]);
    assert_success(&bypassed);
    assert!(!standard_error(&bypassed).contains(FROM_THE_CACHE));
}

#[test]
fn reanalyze_ignores_the_cache_and_keeps_it_filled() {
    let euphoria = Euphoria::new();
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    assert_success(&euphoria.run(&["analyze", text(&fixture)]));

    let reanalyzed = euphoria.run(&["analyze", "--reanalyze", text(&fixture)]);
    let next_run = euphoria.run(&["analyze", text(&fixture)]);

    assert_success(&reanalyzed);
    assert!(!standard_error(&reanalyzed).contains(FROM_THE_CACHE));
    assert!(standard_error(&next_run).contains(FROM_THE_CACHE));
}

#[test]
fn no_cache_and_reanalyze_cannot_be_combined() {
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    let output = Euphoria::new().run(&["analyze", "--no-cache", "--reanalyze", text(&fixture)]);
    assert!(!output.status.success());
    assert!(standard_error(&output).contains("cannot be used with"));
}

#[test]
fn a_failed_file_is_tried_again_on_the_next_run() {
    let euphoria = Euphoria::new();
    let readme = fixtures_directory().join("README.md");

    for _ in 0..2 {
        let output = euphoria.run(&["analyze", text(&readme)]);
        assert!(!output.status.success());
        let errors = standard_error(&output);
        assert!(errors.contains("Could not analyze 1 file:"), "{errors}");
        assert!(!errors.contains(FROM_THE_CACHE), "{errors}");
    }
}

/// Asserts that the run analyzed the file and warned that it did so without
/// the cache at `cache_path`.
fn assert_analyzed_with_a_cache_warning(output: &Output, cache_path: &Path) {
    assert_success(output);
    let table = String::from_utf8_lossy(&output.stdout);
    assert!(table.starts_with("Key"), "{table}");
    assert!(table.contains("sine-440hz-mono.wav"), "{table}");
    let errors = standard_error(output);
    assert!(
        errors.starts_with("warning: analyzing without the cache: "),
        "{errors}"
    );
    assert!(errors.contains(text(cache_path)), "{errors}");
    assert!(errors.contains("euphoria cache clear"), "{errors}");
    assert!(errors.contains("--no-cache"), "{errors}");
    assert!(!errors.contains(FROM_THE_CACHE), "{errors}");
}

#[test]
fn a_damaged_cache_is_a_warning_and_the_files_are_still_analyzed() {
    let euphoria = Euphoria::new();
    let damaged_content = "not a database ".repeat(500);
    std::fs::write(euphoria.cache_path(), &damaged_content).expect("the damaged cache is written");
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");

    let output = euphoria.run(&["analyze", text(&fixture)]);

    assert_analyzed_with_a_cache_warning(&output, &euphoria.cache_path());
    assert!(standard_error(&output).contains("not a database"));
    assert_eq!(
        std::fs::read_to_string(euphoria.cache_path()).expect("the damaged cache is read"),
        damaged_content
    );

    assert_success(&euphoria.run(&["cache", "clear"]));
    let after_clearing = euphoria.run(&["analyze", text(&fixture)]);
    assert_success(&after_clearing);
    assert_eq!(standard_error(&after_clearing), "");
}

#[test]
fn a_damaged_cache_does_not_hide_a_failed_file() {
    let euphoria = Euphoria::new();
    std::fs::write(euphoria.cache_path(), "not a database ".repeat(500))
        .expect("the damaged cache is written");
    let readme = fixtures_directory().join("README.md");

    let output = euphoria.run(&["analyze", "--reanalyze", text(&readme)]);

    assert!(!output.status.success());
    let errors = standard_error(&output);
    assert!(
        errors.contains("warning: analyzing without the cache"),
        "{errors}"
    );
    assert!(errors.contains("Could not analyze 1 file:"), "{errors}");
}

#[test]
fn a_cache_with_a_damaged_row_is_a_warning_and_the_files_are_still_analyzed() {
    let euphoria = Euphoria::new();
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    assert_success(&euphoria.run(&["analyze", text(&fixture)]));
    rusqlite::Connection::open(euphoria.cache_path())
        .expect("the cache opens")
        .execute("UPDATE analyses SET key = 'H dorian'", [])
        .expect("the row is damaged");

    let output = euphoria.run(&["analyze", text(&fixture)]);

    assert_analyzed_with_a_cache_warning(&output, &euphoria.cache_path());
}

/// The schema that the first release of the cache created.
const VERSION_1_SCHEMA: &str = "
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
    PRAGMA user_version = 1;
";

#[test]
fn a_cache_from_before_energy_upgrades_in_place_and_its_tracks_are_analyzed_again() {
    let euphoria = Euphoria::new();
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    let arguments = ["analyze", "--json", text(&fixture)];
    assert_success(&euphoria.run(&arguments));
    std::fs::remove_file(euphoria.cache_path()).expect("the new cache is removed");
    let old_cache = rusqlite::Connection::open(euphoria.cache_path()).expect("the cache opens");
    old_cache
        .execute_batch(VERSION_1_SCHEMA)
        .expect("the old schema is created");
    old_cache
        .execute(
            "INSERT INTO analyses
             VALUES (x'07', 300.0, 'A minor', 124.0, '1+libkeyfinder', 0, NULL, 1, 1)",
            [],
        )
        .expect("an old row is stored");
    drop(old_cache);

    let after_upgrade = euphoria.run(&arguments);
    let next_run = euphoria.run(&arguments);

    assert_success(&after_upgrade);
    assert_eq!(standard_error(&after_upgrade), "");
    assert_eq!(
        standard_error(&next_run),
        "1 of 1 track came from the cache.\n"
    );
    assert_eq!(after_upgrade.stdout, next_run.stdout);
    let upgraded = rusqlite::Connection::open(euphoria.cache_path()).expect("the cache opens");
    let schema_version: u32 = upgraded
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .expect("a schema version");
    assert_eq!(schema_version, 3);
    let old_rows: u32 = upgraded
        .query_row(
            "SELECT COUNT(*) FROM analyses WHERE analyzer_version = '1+libkeyfinder'",
            [],
            |row| row.get(0),
        )
        .expect("a count");
    assert_eq!(old_rows, 1, "rows of other tracks are kept");
    let with_energy_model: u32 = upgraded
        .query_row(
            "SELECT COUNT(*) FROM analyses WHERE energy_model_version IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .expect("a count");
    assert_eq!(with_energy_model, 1);
    let with_tuning_and_key_timeline: u32 = upgraded
        .query_row(
            "SELECT COUNT(*) FROM analyses
             WHERE tuning_cents IS NOT NULL AND key_timeline IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .expect("a count");
    assert_eq!(with_tuning_and_key_timeline, 1);
}

#[cfg(unix)]
#[test]
fn a_read_only_data_folder_is_a_warning_and_the_files_are_still_analyzed() {
    use std::os::unix::fs::PermissionsExt;

    let euphoria = Euphoria::new();
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    let set_mode = |mode| {
        std::fs::set_permissions(
            euphoria.data_directory(),
            std::fs::Permissions::from_mode(mode),
        )
        .expect("the folder's permissions are set");
    };

    set_mode(0o555);
    let without_database = euphoria.run(&["analyze", text(&fixture)]);
    set_mode(0o755);
    assert_analyzed_with_a_cache_warning(&without_database, &euphoria.cache_path());
    assert!(!euphoria.cache_path().exists());

    assert_success(&euphoria.run(&["analyze", text(&fixture)]));
    set_mode(0o555);
    let with_database = euphoria.run(&["analyze", "--reanalyze", text(&fixture)]);
    set_mode(0o755);
    assert_analyzed_with_a_cache_warning(&with_database, &euphoria.cache_path());
}

#[test]
fn processes_started_together_on_an_empty_data_folder_all_succeed() {
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    for round in 0..25 {
        let euphoria = Euphoria::new();
        let processes: Vec<_> = (0..6)
            .map(|_| {
                euphoria
                    .command(&["analyze", text(&fixture)])
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .expect("the euphoria binary starts")
            })
            .collect();
        for process in processes {
            let output = process.wait_with_output().expect("the process finishes");
            let errors = standard_error(&output);
            assert!(output.status.success(), "round {round}: {errors}");
            assert!(!errors.contains("warning"), "round {round}: {errors}");
            assert!(String::from_utf8_lossy(&output.stdout).starts_with("Key"));
        }
    }
}

#[test]
fn a_relative_data_folder_is_taken_from_the_current_directory() {
    let working_directory = TemporaryFolder::new();
    let working_directory = working_directory
        .path()
        .canonicalize()
        .expect("the temporary folder resolves");

    let output = Euphoria::new()
        .command(&["cache", "path"])
        .env("EUPHORIA_DATA_DIR", "relative/data")
        .current_dir(&working_directory)
        .output()
        .expect("the euphoria binary runs");

    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        text(&working_directory.join("relative/data/analysis-cache.sqlite3"))
    );
}

#[test]
fn cache_path_prints_the_database_location() {
    let euphoria = Euphoria::new();
    let output = euphoria.run(&["cache", "path"]);
    assert_success(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        text(&euphoria.cache_path())
    );
}

#[test]
fn cache_clear_forgets_every_result() {
    let euphoria = Euphoria::new();
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    assert_success(&euphoria.run(&["analyze", text(&fixture)]));

    let cleared = euphoria.run(&["cache", "clear"]);
    assert_success(&cleared);
    assert!(String::from_utf8_lossy(&cleared.stdout).starts_with("Cleared the cache at "));

    let after_clearing = euphoria.run(&["analyze", text(&fixture)]);
    assert_success(&after_clearing);
    assert!(!standard_error(&after_clearing).contains(FROM_THE_CACHE));
}

#[test]
fn cache_clear_without_a_cache_says_so_and_creates_none() {
    let euphoria = Euphoria::new();

    let cleared = euphoria.run(&["cache", "clear"]);

    assert_success(&cleared);
    assert!(String::from_utf8_lossy(&cleared.stdout).starts_with("There is no cache at "));
    assert!(!euphoria.cache_path().exists());
}
