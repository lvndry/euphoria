// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `analyze` places each track at a set stage among the tracks analyzed
//! together, and `--stage` narrows the list to some of them.

mod support;

use std::path::Path;

use support::{Euphoria, TemporaryFolder, standard_error, write_kick_track};

const TEMPOS: [f64; 12] = [
    92.0, 98.0, 104.0, 110.0, 116.0, 122.0, 128.0, 134.0, 140.0, 148.0, 156.0, 168.0,
];

fn write_library(folder: &Path) {
    for tempo in TEMPOS {
        write_kick_track(&folder.join(format!("kick {tempo}.wav")), tempo, 0.1);
    }
}

fn analyze_json(extra: &[&str], folder: &Path) -> Vec<serde_json::Value> {
    let mut arguments = vec!["analyze", "--json", "--no-cache"];
    arguments.extend_from_slice(extra);
    arguments.push(folder.to_str().expect("UTF-8 path"));
    let output = Euphoria::new().run(&arguments);
    assert!(output.status.success(), "{}", standard_error(&output));
    serde_json::from_slice::<serde_json::Value>(&output.stdout)
        .expect("the output is JSON")
        .as_array()
        .expect("the output is an array")
        .clone()
}

fn bpm(track: &serde_json::Value) -> f64 {
    track["bpm"].as_f64().expect("a tempo")
}

#[test]
fn the_json_gives_each_track_a_stage_that_rises_with_its_tempo() {
    let folder = TemporaryFolder::new();
    write_library(folder.path());
    let mut tracks = analyze_json(&[], folder.path());
    tracks.sort_by(|first, second| bpm(first).total_cmp(&bpm(second)));
    let stages: Vec<&str> = tracks
        .iter()
        .map(|track| track["stage"].as_str().expect("every track has a stage"))
        .collect();
    let rank = |stage: &str| {
        ["warm-up", "build", "peak"]
            .iter()
            .position(|name| *name == stage)
    };
    assert!(
        stages.iter().all(|stage| rank(stage).is_some()),
        "{stages:?}"
    );
    assert!(
        stages.windows(2).all(|pair| rank(pair[0]) <= rank(pair[1])),
        "{stages:?}"
    );
    assert_eq!(stages[0], "warm-up", "{stages:?}");
    assert_eq!(stages[stages.len() - 1], "peak", "{stages:?}");
}

#[test]
fn the_stage_flag_lists_only_the_tracks_at_those_stages() {
    let folder = TemporaryFolder::new();
    write_library(folder.path());
    let everything = analyze_json(&[], folder.path());
    let staged_as = |stage: &str| {
        everything
            .iter()
            .filter(|track| track["stage"] == stage)
            .count()
    };
    let (warm_up, build, peak) = (staged_as("warm-up"), staged_as("build"), staged_as("peak"));
    assert_eq!(warm_up + build + peak, TEMPOS.len());

    let peak_only = analyze_json(&["--stage", "peak"], folder.path());
    assert_eq!(peak_only.len(), peak);
    assert!(peak_only.iter().all(|track| track["stage"] == "peak"));

    let two = analyze_json(&["--stage", "warmup,build"], folder.path());
    assert_eq!(two.len(), warm_up + build);
    assert!(two.iter().all(|track| track["stage"] != "peak"));
}

#[test]
fn a_track_alone_has_no_stage_and_a_stage_filter_lists_nothing() {
    let folder = TemporaryFolder::new();
    write_kick_track(&folder.path().join("alone.wav"), 124.0, 0.1);
    let alone = analyze_json(&[], folder.path());
    assert_eq!(alone.len(), 1);
    assert!(alone[0]["stage"].is_null(), "{}", alone[0]);
    assert!(analyze_json(&["--stage", "peak"], folder.path()).is_empty());
}

#[test]
fn closing_is_not_a_stage() {
    let folder = TemporaryFolder::new();
    write_kick_track(&folder.path().join("alone.wav"), 124.0, 0.1);
    let output = Euphoria::new().run(&[
        "analyze",
        "--stage",
        "closing",
        folder.path().to_str().expect("UTF-8 path"),
    ]);
    assert!(!output.status.success());
    assert!(
        standard_error(&output).contains("closing"),
        "{}",
        standard_error(&output)
    );
}
