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

#[test]
fn a_damaged_cache_is_an_error_naming_the_file() {
    let euphoria = Euphoria::new();
    std::fs::write(euphoria.cache_path(), "not a database ".repeat(500))
        .expect("the damaged cache is written");
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");

    let output = euphoria.run(&["analyze", text(&fixture)]);

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let errors = standard_error(&output);
    assert!(errors.starts_with("error: "), "{errors}");
    assert!(errors.contains(text(&euphoria.cache_path())), "{errors}");
    assert!(!errors.contains("panicked"), "{errors}");

    assert_success(&euphoria.run(&["cache", "clear"]));
    assert_success(&euphoria.run(&["analyze", text(&fixture)]));
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
fn cache_clear_deletes_the_database() {
    let euphoria = Euphoria::new();
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    assert_success(&euphoria.run(&["analyze", text(&fixture)]));

    let cleared = euphoria.run(&["cache", "clear"]);
    assert_success(&cleared);
    assert!(String::from_utf8_lossy(&cleared.stdout).starts_with("Deleted "));
    assert!(!euphoria.cache_path().exists());

    let cleared_again = euphoria.run(&["cache", "clear"]);
    assert_success(&cleared_again);
    assert!(String::from_utf8_lossy(&cleared_again.stdout).starts_with("There is no cache at "));

    let after_clearing = euphoria.run(&["analyze", text(&fixture)]);
    assert_success(&after_clearing);
    assert!(!standard_error(&after_clearing).contains(FROM_THE_CACHE));
}
