// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the command line shares with the desktop app through the core:
//! review reasons with the "Looks right" confirmations, the tempo moves,
//! the tempo and energy filters and `euphoria next`.

mod support;

use std::path::{Path, PathBuf};
use std::process::Output;

use euphoria_core::tags::{FieldEdit, TagField, TextChange, apply_plan, plan_edit};
use euphoria_core::{Cache, Correction, Key};
use support::{Euphoria, TemporaryFolder, fixtures_directory, standard_error, write_kick_track};

fn text(path: &Path) -> &str {
    path.to_str().expect("UTF-8 path")
}

fn success_json(output: &Output) -> serde_json::Value {
    assert!(output.status.success(), "{}", standard_error(output));
    serde_json::from_slice(&output.stdout).expect("JSON")
}

fn names(tracks: &[serde_json::Value]) -> Vec<String> {
    tracks
        .iter()
        .map(|track| {
            let path = track["path"].as_str().expect("a path");
            Path::new(path)
                .file_stem()
                .expect("a file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

struct Spec {
    name: &'static str,
    key: &'static str,
    bpm: f32,
    energy: u8,
}

const fn spec(name: &'static str, key: &'static str, bpm: f32, energy: u8) -> Spec {
    Spec {
        name,
        key,
        bpm,
        energy,
    }
}

/// A folder of kick tracks, each different, analyzed once and then given
/// the key, tempo and energy of its spec the way the desktop app keeps a
/// correction, so the values are known whatever the audio says.
fn pool(euphoria: &Euphoria, specs: &[Spec]) -> (TemporaryFolder, PathBuf) {
    let folder = TemporaryFolder::new();
    let pool = folder.path().join("pool");
    std::fs::create_dir(&pool).expect("the pool folder is created");
    for (index, spec) in specs.iter().enumerate() {
        write_kick_track(
            &pool.join(format!("{}.wav", spec.name)),
            100.0 + 3.0 * index as f64,
            0.25,
        );
    }
    let analyzed = euphoria.run(&["analyze", "--json", text(&pool)]);
    assert!(analyzed.status.success(), "{}", standard_error(&analyzed));
    let mut cache = Cache::open(&euphoria.cache_path()).expect("the cache opens");
    for spec in specs {
        let key: Key = spec.key.parse().expect("a key");
        let correction = Correction {
            key: Some(key),
            bpm: Some(spec.bpm),
            energy: Some(spec.energy),
            ..Correction::default()
        };
        cache
            .set_correction(&pool.join(format!("{}.wav", spec.name)), &correction)
            .expect("the correction is kept");
    }
    (folder, pool)
}

fn mixing_pool(euphoria: &Euphoria) -> (TemporaryFolder, PathBuf) {
    pool(
        euphoria,
        &[
            spec("playing", "8A", 124.0, 5),
            spec("same_up", "8A", 124.0, 7),
            spec("same_keep", "8A", 125.0, 5),
            spec("adjacent_down", "9A", 124.0, 3),
            spec("relative_keep", "8B", 126.0, 5),
            spec("far_key", "3A", 124.0, 5),
            spec("fast", "8A", 140.0, 5),
        ],
    )
}

fn next_json(euphoria: &Euphoria, pool: &Path, extra: &[&str]) -> serde_json::Value {
    let playing = pool.join("playing.wav");
    let mut arguments = vec!["next", text(&playing), "--json"];
    arguments.extend_from_slice(extra);
    success_json(&euphoria.run(&arguments))
}

#[test]
fn next_ranks_for_keep_lift_and_drop() {
    let euphoria = Euphoria::new();
    let (_folder, pool) = mixing_pool(&euphoria);

    let keep = next_json(&euphoria, &pool, &[]);
    assert_eq!(keep["schema_version"], 1);
    assert_eq!(keep["energy"], "keep");
    assert_eq!(keep["track"]["key"], "8A");
    assert_eq!(
        names(keep["next"].as_array().unwrap()),
        ["same_keep", "same_up", "relative_keep", "adjacent_down"],
        "the far key and the fast track do not mix, and the playing one is not offered"
    );
    let lift = next_json(&euphoria, &pool, &["--lift"]);
    assert_eq!(lift["energy"], "lift");
    assert_eq!(
        names(lift["next"].as_array().unwrap()),
        ["same_up", "same_keep", "relative_keep", "adjacent_down"]
    );
    let drop = next_json(&euphoria, &pool, &["--drop"]);
    assert_eq!(drop["energy"], "drop");
    assert_eq!(
        names(drop["next"].as_array().unwrap()),
        ["adjacent_down", "same_keep", "same_up", "relative_keep"]
    );
}

#[test]
fn next_rows_carry_key_move_tempo_move_and_energy_change() {
    let euphoria = Euphoria::new();
    let (_folder, pool) = mixing_pool(&euphoria);
    let keep = next_json(&euphoria, &pool, &[]);
    let rows = keep["next"].as_array().unwrap();
    let same_keep = &rows[0];
    assert_eq!(same_keep["rank"], 1);
    assert_eq!(same_keep["key"], "8A");
    assert_eq!(same_keep["key_move"], "same");
    assert_eq!(same_keep["key_move_label"], "Same key");
    assert_eq!(same_keep["tempo_percent"], 0.8);
    assert_eq!(same_keep["tempo_move"], "easy");
    assert_eq!(same_keep["tempo_scaling"], "direct");
    assert_eq!(same_keep["energy_change"], 0);
    let adjacent = &rows[3];
    assert_eq!(adjacent["key_move_label"], "One step on the wheel");
    assert_eq!(adjacent["energy_change"], -2);
    assert_eq!(rows[1]["energy_change"], 2);
    assert_eq!(rows[2]["key_move_label"], "Relative");
    assert_eq!(rows[2]["tempo_move"], "easy");
}

#[test]
fn next_prints_a_table_in_the_shared_words_and_honours_limit() {
    let euphoria = Euphoria::new();
    let (_folder, pool) = mixing_pool(&euphoria);
    let playing = pool.join("playing.wav");
    let output = euphoria.run(&["next", text(&playing), "--limit", "2"]);
    assert!(output.status.success(), "{}", standard_error(&output));
    let table = String::from_utf8_lossy(&output.stdout);
    assert!(table.starts_with("After playing"), "{table}");
    for word in ["Key", "Tempo", "Move", "Energy", "Same key", "+0.8% Easy"] {
        assert!(table.contains(word), "{word} in {table}");
    }
    assert_eq!(table.lines().count(), 4, "a header, a heading and two rows");
    assert!(standard_error(&output).contains("--limit shows more"));
}

#[test]
fn next_takes_the_pool_from_a_folder_and_the_same_filters_as_analyze() {
    let euphoria = Euphoria::new();
    let (folder, pool) = mixing_pool(&euphoria);
    let elsewhere = folder.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).expect("a folder");
    let outside = elsewhere.join("outside.wav");
    write_kick_track(&outside, 150.0, 0.25);
    let playing = pool.join("playing.wav");
    let from_elsewhere = success_json(&euphoria.run(&[
        "next",
        text(&playing),
        "--json",
        "--from",
        text(&elsewhere),
    ]));
    assert!(from_elsewhere["next"].as_array().unwrap().is_empty());

    let energetic = next_json(&euphoria, &pool, &["--energy", "6-"]);
    assert_eq!(names(energetic["next"].as_array().unwrap()), ["same_up"]);
    let fast_enough = next_json(&euphoria, &pool, &["--bpm", "125-"]);
    assert_eq!(
        names(fast_enough["next"].as_array().unwrap()),
        ["same_keep", "relative_keep"]
    );
    let none = euphoria.run(&["next", text(&playing), "--energy", "9-"]);
    assert!(none.status.success());
    assert!(standard_error(&none).contains("Loosen --bpm, --energy or --quality"));
}

#[test]
fn analyze_filters_by_tempo_range_with_open_ends_comma_decimals_and_half_double() {
    let euphoria = Euphoria::new();
    let (_folder, pool) = pool(
        &euphoria,
        &[
            spec("slow", "8A", 70.0, 2),
            spec("low", "8A", 120.0, 4),
            spec("mid", "8A", 124.5, 6),
            spec("high", "8A", 126.0, 8),
            spec("fast", "8A", 170.0, 9),
        ],
    );
    let listed = |extra: &[&str]| {
        let mut arguments = vec!["analyze", "--json"];
        arguments.extend_from_slice(extra);
        arguments.push(text(&pool));
        let mut listed = names(success_json(&euphoria.run(&arguments)).as_array().unwrap());
        listed.sort();
        listed
    };
    assert_eq!(listed(&["--bpm", "120-126"]), ["high", "low", "mid"]);
    assert_eq!(listed(&["--bpm", "126-"]), ["fast", "high"]);
    assert_eq!(listed(&["--bpm", "-70"]), ["slow"]);
    assert_eq!(listed(&["--bpm", "124,5-126,0"]), ["high", "mid"]);
    assert_eq!(
        listed(&["--bpm", "60-65", "--half-double"]),
        ["high", "low", "mid"],
        "120 to 130 is double 60 to 65"
    );
    assert_eq!(listed(&["--bpm", "35-40", "--half-double"]), ["slow"]);
    assert_eq!(listed(&["--energy", "6-"]), ["fast", "high", "mid"]);
    assert_eq!(listed(&["--energy", "6-8"]), ["high", "mid"]);
    assert_eq!(listed(&["--bpm", "120-", "--energy", "-6"]), ["low", "mid"]);
}

#[test]
fn analyze_filters_use_the_corrected_values_not_the_detected_ones() {
    let euphoria = Euphoria::new();
    let (_folder, pool) = pool(&euphoria, &[spec("only", "8A", 99.0, 3)]);
    let detected = success_json(&euphoria.run(&[
        "analyze",
        "--json",
        "--no-cache",
        text(&pool.join("only.wav")),
    ]));
    assert_ne!(detected[0]["bpm"], 99.0, "the audio says another tempo");
    let corrected = success_json(&euphoria.run(&[
        "analyze",
        "--json",
        "--bpm",
        "98,5-99,5",
        "--energy",
        "3",
        text(&pool),
    ]));
    assert_eq!(corrected.as_array().unwrap().len(), 1);
}

#[test]
fn a_range_that_does_not_read_is_refused_before_any_analysis() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let output = euphoria.run(&["analyze", "--bpm", "fast", text(folder.path())]);
    assert!(!output.status.success());
    let message = standard_error(&output);
    assert!(
        message.contains("--bpm fast: a tempo range is written 120-126"),
        "{message}"
    );
    let output = euphoria.run(&["analyze", "--energy", "11", text(folder.path())]);
    assert!(standard_error(&output).contains("--energy 11:"));
}

fn tagged_copy(
    folder: &TemporaryFolder,
    tag: impl Fn(Key) -> String,
    euphoria: &Euphoria,
) -> PathBuf {
    let path = folder.path().join("tone.mp3");
    std::fs::copy(fixtures_directory().join("sine-440hz-mono.mp3"), &path).expect("copied");
    let detected = success_json(&euphoria.run(&["analyze", "--json", text(&path)]));
    let detected: Key = detected[0]["key"]["name"]
        .as_str()
        .expect("a key")
        .parse()
        .expect("a key name");
    let edit = FieldEdit {
        field: TagField::Key,
        change: TextChange::Set(tag(detected)),
    };
    apply_plan(&plan_edit(&path, &[edit]).expect("planned")).expect("written");
    path
}

fn review_of(euphoria: &Euphoria, path: &Path, extra: &[&str]) -> Vec<serde_json::Value> {
    let mut arguments = vec!["analyze", "--json"];
    arguments.extend_from_slice(extra);
    arguments.push(text(path));
    success_json(&euphoria.run(&arguments))
        .as_array()
        .unwrap()
        .clone()
}

fn reasons(track: &serde_json::Value) -> Vec<String> {
    track["review_reasons"]
        .as_array()
        .expect("review_reasons is a list")
        .iter()
        .map(|reason| reason.as_str().expect("a word").to_string())
        .collect()
}

#[test]
fn the_json_lists_review_reasons_and_whether_the_user_reviewed() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let path = tagged_copy(
        &folder,
        |detected| detected.transposed(7).short_name(),
        &euphoria,
    );

    let track = review_of(&euphoria, &path, &[]).remove(0);
    assert!(
        reasons(&track).contains(&"key_conflict".to_string()),
        "{track}"
    );
    assert_eq!(track["needs_review"], true);
    assert_eq!(track["reviewed"], false);
    let listed = review_of(&euphoria, &path, &["--needs-review"]);
    assert_eq!(listed.len(), 1);
}

#[test]
fn looks_right_takes_a_track_out_of_needs_review() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let path = tagged_copy(
        &folder,
        |detected| detected.transposed(7).short_name(),
        &euphoria,
    );
    let mut cache = Cache::open(&euphoria.cache_path()).expect("the cache opens");

    cache
        .set_correction(&path, &Correction::default().confirming_key())
        .expect("kept");
    let track = review_of(&euphoria, &path, &[]).remove(0);
    let remaining = reasons(&track);
    assert!(!remaining.contains(&"key_conflict".to_string()), "{track}");
    assert!(!remaining.contains(&"key_unsure".to_string()), "{track}");
    assert_eq!(track["reviewed"], true);
    assert_eq!(
        track["tag_key"]["agreement"], "fifth",
        "the difference is still shown"
    );

    cache
        .set_correction(
            &path,
            &Correction::default().confirming_key().confirming_bpm(),
        )
        .expect("kept");
    let track = review_of(&euphoria, &path, &[]).remove(0);
    assert_eq!(reasons(&track), Vec::<String>::new(), "{track}");
    assert_eq!(track["needs_review"], false);
    assert_eq!(track["reviewed"], true);
    assert!(
        review_of(&euphoria, &path, &["--needs-review"]).is_empty(),
        "a confirmed track is not listed"
    );
}

#[test]
fn the_detail_names_the_reasons_and_a_confirmed_track() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let path = tagged_copy(
        &folder,
        |detected| detected.transposed(7).short_name(),
        &euphoria,
    );
    let detail = |euphoria: &Euphoria| {
        let output = euphoria.run(&["analyze", "--detail", "--color", "never", text(&path)]);
        assert!(output.status.success(), "{}", standard_error(&output));
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let before = detail(&euphoria);
    assert!(before.contains("Review"), "{before}");
    assert!(before.contains("the tags say"), "{before}");
    let mut cache = Cache::open(&euphoria.cache_path()).expect("the cache opens");
    cache
        .set_correction(
            &path,
            &Correction::default().confirming_key().confirming_bpm(),
        )
        .expect("kept");
    let after = detail(&euphoria);
    assert!(
        after.contains("checked by you, no longer in doubt"),
        "{after}"
    );
}

#[test]
fn set_build_and_plan_name_the_tempo_move() {
    let euphoria = Euphoria::new();
    let (_folder, pool) = pool(
        &euphoria,
        &[
            spec("one", "8A", 120.0, 4),
            spec("two", "8A", 123.0, 6),
            spec("three", "8A", 131.0, 8),
        ],
    );
    let output = euphoria.run(&[
        "set",
        "build",
        text(&pool),
        "--tracks",
        "3",
        "--json",
        "--max-bpm-step",
        "20",
    ]);
    let set = success_json(&output);
    let transitions = set["transitions"].as_array().unwrap();
    assert_eq!(transitions.len(), 2);
    for transition in transitions {
        let percent = transition["bpm_change_percent"].as_f64().unwrap().abs();
        let expected = if percent < 3.0 {
            "easy"
        } else if percent <= 6.0 {
            "pitch_move"
        } else {
            "big_jump"
        };
        assert_eq!(transition["tempo_move"], expected, "{transition}");
    }
    let table = euphoria.run(&[
        "set",
        "build",
        text(&pool),
        "--tracks",
        "3",
        "--max-bpm-step",
        "20",
    ]);
    let table = String::from_utf8_lossy(&table.stdout).into_owned();
    let heading = table.lines().next().unwrap();
    assert!(
        heading.contains("Step") && heading.contains("Tempo"),
        "{table}"
    );
    assert!(
        ["Easy", "Pitch move", "Big jump"]
            .iter()
            .any(|label| table.contains(label)),
        "{table}"
    );
}

#[test]
fn plan_puts_the_tempo_label_next_to_the_percent() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let first = folder.path().join("first.wav");
    let second = folder.path().join("second.wav");
    write_kick_track(&first, 120.0, 0.25);
    write_kick_track(&second, 127.0, 0.25);
    let json =
        success_json(&euphoria.run(&["plan", "--json", "--no-cache", text(&first), text(&second)]));
    let percent = json["plan"]["alignment"]["tempo_change_percent"]
        .as_f64()
        .unwrap()
        .abs();
    let (code, label) = if percent < 3.0 {
        ("easy", "Easy")
    } else if percent <= 6.0 {
        ("pitch_move", "Pitch move")
    } else {
        ("big_jump", "Big jump")
    };
    assert_eq!(json["tempo_move"], code, "{percent}");
    let output = euphoria.run(&["plan", "--no-cache", text(&first), text(&second)]);
    let table = String::from_utf8_lossy(&output.stdout);
    let tempo_line = table
        .lines()
        .find(|line| line.starts_with("Tempo") || line.contains("played at"))
        .expect("a tempo line");
    assert!(tempo_line.ends_with(&format!("{label})")), "{tempo_line}");
}
