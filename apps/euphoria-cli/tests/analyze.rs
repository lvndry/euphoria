// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs the `euphoria` binary on the core crate's audio fixtures.

mod support;

use support::{Euphoria, fixtures_directory, standard_error};

#[test]
fn analyzes_a_folder_as_json() {
    let fixtures = fixtures_directory();
    let output =
        Euphoria::new().run(&["analyze", "--json", fixtures.to_str().expect("UTF-8 path")]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let tracks: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the output is JSON");
    let tracks = tracks.as_array().expect("the output is an array");
    assert_eq!(
        tracks.len(),
        10,
        "one entry per audio fixture, README skipped"
    );
    for track in tracks {
        assert!(track["error"].is_null(), "{track}");
        let duration = track["duration_seconds"].as_f64().expect("a duration");
        assert!((duration - 1.0).abs() < 0.1, "{track}");
        let path = track["path"].as_str().expect("a path");
        let energy = &track["energy"];
        if path.contains("opposite-phase") {
            assert!(energy.is_null(), "silence has no energy rating: {track}");
        } else {
            let rating = energy.as_u64().expect("a track with sound has a rating");
            assert!((1..=10).contains(&rating), "{track}");
        }
        let quality = &track["quality"];
        assert_eq!(quality["sample_rate"], 44_100, "{track}");
        let lossless = quality["lossless"].as_bool().expect("a lossless flag");
        let lossy = [".mp3", "mono.m4a", ".mp4", ".ogg"]
            .iter()
            .any(|ending| path.ends_with(ending));
        assert_eq!(lossless, !lossy, "{track}");
        if path.contains("alac") {
            assert_eq!(
                quality["bit_depth"], 16,
                "ALAC fixture must report 16-bit depth: {track}"
            );
        } else if lossy {
            assert!(
                quality["bit_depth"].is_null(),
                "lossy fixtures have no bit depth: {track}"
            );
        }
        assert!(quality["reason"].as_str().is_some(), "{track}");
        let tuning = &track["tuning_cents"];
        let timeline = track["key_timeline"].as_array().expect("a key timeline");
        if path.contains("opposite-phase") {
            assert!(tuning.is_null(), "silence has no tuning: {track}");
            assert!(timeline.is_empty(), "silence has no key over time: {track}");
        } else {
            let cents = tuning.as_f64().expect("a 440 Hz tone has a tuning");
            assert!(
                cents.abs() < 2.0,
                "a 440 Hz tone is at concert pitch: {track}"
            );
            assert_eq!(timeline.len(), 1, "{track}");
            assert_eq!(timeline[0]["key"]["name"], track["key"]["name"], "{track}");
            assert_eq!(timeline[0]["start_seconds"], 0.0, "{track}");
            assert_eq!(timeline[0]["end_seconds"], track["duration_seconds"]);
        }
    }
}

#[test]
fn prints_a_table_with_a_header() {
    let fixture = fixtures_directory().join("sine-440hz-mono.flac");
    let output = Euphoria::new().run(&["analyze", fixture.to_str().expect("UTF-8 path")]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let table = String::from_utf8(output.stdout).expect("UTF-8 output");
    assert!(table.starts_with("Key"), "{table}");
    let header = table.lines().next().expect("a header line");
    assert!(header.contains("BPM  Energy  Conf  Quality"), "{table}");
    assert!(table.contains("Lossless"), "{table}");
    assert!(table.contains("sine-440hz-mono.flac"), "{table}");
    let summary = String::from_utf8_lossy(&output.stderr);
    assert!(
        summary.contains("Sound quality: 1 track lossless."),
        "{summary}"
    );
    assert!(
        !summary.contains('\u{1b}'),
        "no colour into a pipe: {summary:?}"
    );
}

fn analyze_json(arguments: &[&str]) -> Vec<serde_json::Value> {
    let fixtures = fixtures_directory();
    let mut command = vec!["analyze", "--json", "--no-cache"];
    command.extend_from_slice(arguments);
    command.push(fixtures.to_str().expect("UTF-8 path"));
    let output = Euphoria::new().run(&command);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tracks: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the output is JSON");
    tracks.as_array().expect("the output is an array").clone()
}

#[test]
fn json_rates_the_key_and_the_tempo_and_names_the_runner_up() {
    for track in analyze_json(&[]) {
        if track["path"].as_str().unwrap().contains("opposite-phase") {
            assert!(track["runner_up_key"].is_null(), "{track}");
            assert_eq!(track["needs_review"], false, "{track}");
            continue;
        }
        let confidence = track["key"]["confidence"].as_str().expect("a key rating");
        assert!(["high", "medium", "low"].contains(&confidence), "{track}");
        assert!(track["key"]["open_key"].is_string(), "{track}");
        assert!(track["runner_up_key"]["camelot"].is_string(), "{track}");
        assert_ne!(track["runner_up_key"]["name"], track["key"]["name"]);
        let tempo_confidence = track["bpm_confidence"].as_str().expect("a tempo rating");
        assert!(
            ["high", "medium", "low"].contains(&tempo_confidence),
            "{track}"
        );
        assert!(track["needs_review"].is_boolean(), "{track}");
    }
}

#[test]
fn a_tempo_range_reports_every_tempo_inside_it() {
    let in_default = analyze_json(&[]);
    let in_hip_hop_range = analyze_json(&["--tempo-range", "70-140"]);
    let mut compared = 0;
    for (default, hip_hop) in in_default.iter().zip(&in_hip_hop_range) {
        assert_eq!(default["path"], hip_hop["path"]);
        let (Some(default_bpm), Some(hip_hop_bpm)) =
            (default["bpm"].as_f64(), hip_hop["bpm"].as_f64())
        else {
            continue;
        };
        assert!((70.0..140.0).contains(&hip_hop_bpm), "{hip_hop}");
        let octaves = (hip_hop_bpm / default_bpm).log2();
        assert!(
            (octaves - octaves.round()).abs() < 0.01,
            "{default_bpm} and {hip_hop_bpm} are not octaves of each other"
        );
        compared += 1;
    }
    assert!(compared > 0, "no fixture has a tempo");
}

#[test]
fn a_tempo_range_wider_than_an_octave_is_refused() {
    let fixture = fixtures_directory().join("sine-440hz-mono.flac");
    let output = Euphoria::new().run(&[
        "analyze",
        "--tempo-range",
        "60-200",
        fixture.to_str().expect("UTF-8 path"),
    ]);
    assert!(!output.status.success());
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(errors.contains("at most an octave wide"), "{errors}");
}

#[test]
fn a_large_finite_tempo_range_does_not_turn_a_detected_tempo_into_null() {
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    let output = Euphoria::new().run(&[
        "analyze",
        "--json",
        "--no-cache",
        "--tempo-range",
        "2e38-3e38",
        fixture.to_str().expect("UTF-8 path"),
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tracks: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let tempo = tracks[0]["bpm"]
        .as_f64()
        .expect("a detected tempo stays a number");
    assert!(tempo.is_finite() && tempo > 0.0, "{tracks}");
    assert!(tracks[0]["error"].is_null(), "{tracks}");
}

#[test]
fn needs_review_lists_only_tracks_rated_low() {
    let all = analyze_json(&[]);
    let to_review = analyze_json(&["--needs-review"]);
    let expected: Vec<&serde_json::Value> = all
        .iter()
        .filter(|track| track["needs_review"] == true)
        .collect();
    assert_eq!(to_review.len(), expected.len());
    for (listed, expected) in to_review.iter().zip(expected) {
        assert_eq!(listed["path"], expected["path"]);
        let rated_low = listed["key"]["confidence"] == "low" || listed["bpm_confidence"] == "low";
        assert!(rated_low, "{listed}");
    }
    assert!(
        to_review
            .iter()
            .all(|track| !track["path"].as_str().unwrap().contains("opposite-phase")),
        "silence has nothing to review"
    );
}

#[test]
fn reports_an_unreadable_file_and_exits_with_failure() {
    let readme = fixtures_directory().join("README.md");
    let output = Euphoria::new().run(&["analyze", readme.to_str().expect("UTF-8 path")]);
    assert!(!output.status.success());
    let errors = String::from_utf8_lossy(&output.stderr);
    assert!(
        errors.starts_with(&format!("error: {}: ", readme.display())),
        "{errors}"
    );
    assert!(
        errors.contains("1 file could not be analyzed; the errors above say why."),
        "{errors}"
    );
}

#[test]
fn a_missing_path_is_an_error() {
    let output = Euphoria::new().run(&["analyze", "/definitely/not/here"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("/definitely/not/here"));
}

#[cfg(unix)]
#[test]
fn a_reader_that_stops_early_is_not_an_error() {
    use std::io::Read;
    use std::process::Stdio;

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_euphoria"))
        .args(["analyze", "--no-cache"])
        .arg(fixtures_directory())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the euphoria binary runs");
    drop(child.stdout.take());

    let mut errors = String::new();
    child
        .stderr
        .take()
        .expect("standard error is piped")
        .read_to_string(&mut errors)
        .expect("standard error is readable");
    let status = child.wait().expect("the process exits");

    assert!(status.success(), "{errors}");
    assert!(!errors.contains("Broken pipe"), "{errors}");
}

#[test]
fn piped_output_has_no_progress_lines() {
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    let folder = fixtures_directory();
    let euphoria = Euphoria::new();
    for arguments in [
        vec!["analyze", fixture.to_str().expect("UTF-8 path")],
        vec!["analyze", "--json", fixture.to_str().expect("UTF-8 path")],
        vec!["analyze", folder.to_str().expect("UTF-8 path")],
    ] {
        let output = euphoria.run(&arguments);
        assert!(output.status.success(), "{arguments:?}");
        let standard_error = String::from_utf8_lossy(&output.stderr);
        // No stage, spinner, bar or debug build note when standard error is
        // not a terminal.
        for unwanted in [
            "decoding",
            "finding the",
            "Analyzing",
            "debug build",
            "\r",
            "\u{1b}",
        ] {
            assert!(
                !standard_error.contains(unwanted),
                "{arguments:?} wrote {unwanted:?} to standard error: {standard_error:?}"
            );
        }
        let standard_output = String::from_utf8_lossy(&output.stdout);
        assert!(!standard_output.contains("decoding"), "{standard_output}");
        assert!(!standard_output.contains('\u{1b}'), "{standard_output:?}");
    }
}

#[test]
fn a_terminal_shows_the_stage_but_json_shows_only_the_bar() {
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    let fixture = fixture.to_str().expect("UTF-8 path");
    let euphoria = Euphoria::new();
    let with_terminal = |arguments: &[&str]| support::pty::capture(&euphoria.command(arguments));

    // A terminal gets a line per file with the stage it is in.
    let terminal = with_terminal(&["analyze", "--no-cache", fixture]);
    assert!(terminal.contains("decoding"), "{terminal:?}");

    // JSON goes to standard output, so stage lines would cross it: JSON only
    // gets the bar that counts files.
    let json = with_terminal(&["analyze", "--json", "--no-cache", fixture]);
    assert!(!json.contains("decoding"), "{json:?}");
    assert!(!json.contains("finding the"), "{json:?}");
    assert!(json.contains("Analyzing"), "{json:?}");
}

fn run_with_environment(arguments: &[&str], variables: &[(&str, &str)]) -> std::process::Output {
    let euphoria = Euphoria::new();
    let mut command = euphoria.command(arguments);
    command.env_remove("NO_COLOR").env_remove("CLICOLOR_FORCE");
    for (name, value) in variables {
        command.env(name, value);
    }
    command.output().expect("the euphoria binary runs")
}

#[test]
fn colour_follows_the_flag_and_the_environment() {
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    let fixture = fixture.to_str().expect("UTF-8 path");
    let has_colour = |output: &std::process::Output| {
        output.stdout.contains(&0x1b) || output.stderr.contains(&0x1b)
    };
    let detail = ["analyze", "--detail", fixture];
    assert!(!has_colour(&run_with_environment(&detail, &[])));
    let always = run_with_environment(&["analyze", "--color", "always", "--detail", fixture], &[]);
    assert!(has_colour(&always));
    assert!(has_colour(&run_with_environment(
        &detail,
        &[("CLICOLOR_FORCE", "1")]
    )));
    assert!(!has_colour(&run_with_environment(
        &detail,
        &[("CLICOLOR_FORCE", "1"), ("NO_COLOR", "1")]
    )));
    let never = run_with_environment(
        &["--color=never", "analyze", "--detail", fixture],
        &[("CLICOLOR_FORCE", "1")],
    );
    assert!(!has_colour(&never));
    let unknown = run_with_environment(&["analyze", "--color", "sometimes", fixture], &[]);
    assert!(!unknown.status.success());
}

#[test]
fn detail_shows_each_track_with_its_labels() {
    let fixture = fixtures_directory().join("sine-440hz-mono.flac");
    let output = Euphoria::new().run(&[
        "analyze",
        "--detail",
        "--no-cache",
        fixture.to_str().expect("UTF-8 path"),
    ]);
    assert!(output.status.success(), "{}", standard_error(&output));
    let detail = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(detail.starts_with("sine-440hz-mono.flac\n"), "{detail}");
    for label in [
        "  Key ",
        "  Tempo ",
        "  Energy ",
        "  Sound quality ",
        "  Length ",
    ] {
        assert!(detail.contains(label), "{label:?} missing from {detail}");
    }
    assert!(
        detail.contains("Lossless FLAC with a full spectrum."),
        "{detail}"
    );
    assert!(
        standard_error(&output).is_empty(),
        "one track in detail needs no summary: {}",
        standard_error(&output)
    );
}

#[test]
fn quality_lists_only_the_tracks_with_those_verdicts() {
    let lossless = analyze_json(&["--quality", "lossless"]);
    let lossy = analyze_json(&["--quality", "good,acceptable,weak,reencoded"]);
    let all = analyze_json(&[]);
    assert_eq!(lossless.len() + lossy.len(), all.len());
    assert!(
        lossless
            .iter()
            .all(|track| track["quality"]["verdict"] == "lossless")
    );
    assert!(
        lossy
            .iter()
            .all(|track| track["quality"]["verdict"] != "lossless")
    );
    assert_eq!(
        analyze_json(&["--quality", "likely-reencoded,weak-on-a-big-system"]).len(),
        analyze_json(&["--quality", "reencoded,weak"]).len()
    );
}

#[test]
fn a_narrowed_table_says_how_many_it_shows() {
    let fixtures = fixtures_directory();
    let output = Euphoria::new().run(&[
        "analyze",
        "--quality",
        "lossless",
        fixtures.to_str().expect("UTF-8 path"),
    ]);
    assert!(output.status.success(), "{}", standard_error(&output));
    let summary = standard_error(&output);
    assert!(
        summary.starts_with("Showing 6 of 10 tracks, those with sound quality lossless.\n"),
        "{summary}"
    );
    assert!(
        summary.contains("Sound quality: 6 tracks lossless, "),
        "{summary}"
    );
}

#[test]
fn help_ends_with_examples() {
    for arguments in [
        vec!["--help"],
        vec!["analyze", "--help"],
        vec!["tags", "write", "--help"],
        vec!["export", "rekordbox", "--help"],
        vec!["cache", "--help"],
    ] {
        let output = Euphoria::new().run(&arguments);
        assert!(output.status.success(), "{arguments:?}");
        let help = String::from_utf8(output.stdout).expect("UTF-8 help");
        assert!(
            help.contains("\nExamples:\n  euphoria "),
            "{arguments:?}: {help}"
        );
        assert!(!help.contains('\u{1b}'), "{arguments:?}: {help:?}");
    }
}

#[test]
fn an_empty_filtered_detail_explains_why_the_track_is_not_shown() {
    let fixture = fixtures_directory().join("sine-440hz-mono.flac");
    let output = Euphoria::new().run(&[
        "analyze",
        "--no-cache",
        "--detail",
        "--quality",
        "weak",
        fixture.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", standard_error(&output));
    assert!(output.stdout.is_empty());
    assert!(
        standard_error(&output)
            .starts_with("Showing 0 of 1 track, those with sound quality weak on a big system.\n"),
        "{}",
        standard_error(&output)
    );
}

#[test]
fn a_literal_path_after_double_dash_cannot_change_the_colour_policy() {
    let euphoria = Euphoria::new();
    let folder = support::TemporaryFolder::new();
    std::fs::create_dir(folder.path().join("--color=always")).unwrap();
    let output = euphoria
        .command(&["--color", "never", "analyze", "--", "--color=always"])
        .current_dir(folder.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        !output.stderr.contains(&0x1b),
        "{:?}",
        standard_error(&output)
    );
    assert!(standard_error(&output).contains("no audio files found"));
}
