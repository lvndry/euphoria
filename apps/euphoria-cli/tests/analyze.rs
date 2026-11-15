// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs the `euphoria` binary on the core crate's audio fixtures.

mod support;

use support::{Euphoria, TemporaryFolder, fixtures_directory, standard_error};

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
        assert_eq!(track["schema_version"], 1, "{track}");
        let duration = track["duration_seconds"].as_f64().expect("a duration");
        assert!((duration - 1.0).abs() < 0.1, "{track}");
        let path = track["path"].as_str().expect("a path");
        let energy = &track["energy"];
        let rating = energy.as_u64().expect("a track with sound has a rating");
        assert!((1..=10).contains(&rating), "{track}");
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
        let cents = tuning.as_f64().expect("a 440 Hz tone has a tuning");
        assert!(
            cents.abs() < 2.0,
            "a 440 Hz tone is at concert pitch: {track}"
        );
        assert_eq!(timeline.len(), 1, "{track}");
        assert_eq!(timeline[0]["key"]["name"], track["key"]["name"], "{track}");
        assert_eq!(timeline[0]["start_seconds"], 0.0, "{track}");
        assert_eq!(timeline[0]["end_seconds"], track["duration_seconds"]);
        let damaged = quality["damaged"].as_bool().expect("a damaged flag");
        assert!(!damaged, "{track}");
        assert_eq!(
            quality["notes"].as_array().expect("notes").is_empty(),
            !path.contains("opposite-phase"),
            "only the opposite phase file has a note: {track}"
        );
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
fn a_missing_path_is_reported_as_missing_with_status_3() {
    let output = Euphoria::new().run(&["analyze", "/definitely/not/here"]);
    assert_eq!(output.status.code(), Some(3));
    let errors = standard_error(&output);
    assert!(errors.contains("missing: /definitely/not/here"), "{errors}");
    assert!(
        errors.contains("1 file is missing: moved, deleted, or on a drive that is not connected."),
        "{errors}"
    );
    assert!(!errors.contains("error:"), "{errors}");
}

#[test]
fn a_missing_path_does_not_stop_the_other_files() {
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    let output = Euphoria::new().run(&[
        "analyze",
        "--json",
        "/definitely/not/here",
        fixture.to_str().expect("UTF-8 path"),
        "/0-also-not-here.wav",
    ]);
    assert_eq!(output.status.code(), Some(3));
    let tracks: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the output is JSON");
    let tracks = tracks.as_array().expect("the output is an array");
    assert_eq!(tracks.len(), 3, "{tracks:?}");
    let paths: Vec<&str> = tracks
        .iter()
        .map(|track| track["path"].as_str().expect("a path"))
        .collect();
    let mut sorted_paths = paths.clone();
    sorted_paths.sort_unstable();
    assert_eq!(paths, sorted_paths, "one object per file, in path order");
    let missing_track = tracks
        .iter()
        .find(|track| track["path"] == "/definitely/not/here")
        .expect("the missing path has an entry");
    assert_eq!(missing_track["missing"], true, "{missing_track}");
    assert!(missing_track["error"].is_string(), "{missing_track}");
    let analyzed_track = tracks
        .iter()
        .find(|track| track["missing"].is_null())
        .expect("the existing file has an entry");
    assert!(analyzed_track["error"].is_null(), "{analyzed_track}");
    assert!(analyzed_track.get("missing").is_none(), "{analyzed_track}");
}

#[test]
fn a_failure_and_a_missing_file_exit_with_status_1() {
    let readme = fixtures_directory().join("README.md");
    let output = Euphoria::new().run(&[
        "analyze",
        readme.to_str().expect("UTF-8 path"),
        "/definitely/not/here",
    ]);
    assert_eq!(output.status.code(), Some(1));
    let errors = standard_error(&output);
    assert!(
        errors.contains(&format!("error: {}", readme.display())),
        "{errors}"
    );
    assert!(errors.contains("missing: /definitely/not/here"), "{errors}");
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
    assert!(terminal.succeeded, "analyze failed: {:?}", terminal.output);
    assert!(
        terminal.output.contains("decoding"),
        "{:?}",
        terminal.output
    );

    // JSON goes to standard output, so stage lines would cross it: JSON only
    // gets the bar that counts files.
    let json = with_terminal(&["analyze", "--json", "--no-cache", fixture]);
    assert!(json.succeeded, "analyze --json failed: {:?}", json.output);
    assert!(!json.output.contains("decoding"), "{:?}", json.output);
    assert!(!json.output.contains("finding the"), "{:?}", json.output);
    assert!(json.output.contains("Analyzing"), "{:?}", json.output);
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
    let folder = TemporaryFolder::new();
    std::fs::create_dir(folder.path().join("--color=always")).unwrap();
    let output = euphoria
        .command(&["--color", "never", "analyze", "--", "--color=always"])
        .current_dir(folder.path())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    assert!(
        !output.stderr.contains(&0x1b),
        "{:?}",
        standard_error(&output)
    );
    assert!(standard_error(&output).contains("no audio files found"));
}

/// A folder with one real track, one unsupported audio file of two formats
/// and, on Unix, a subfolder the process cannot enter.
#[cfg(unix)]
fn library_with_a_locked_folder() -> (TemporaryFolder, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let library = TemporaryFolder::new();
    let locked = library.path().join("locked");
    std::fs::create_dir(&locked).expect("the folder is created");
    std::fs::write(locked.join("inside.mp3"), b"").expect("the file is written");
    std::fs::copy(
        fixtures_directory().join("sine-440hz-mono.wav"),
        library.path().join("tone.wav"),
    )
    .expect("the fixture is copied");
    for name in ["a.opus", "b.opus", "c.wma"] {
        std::fs::write(library.path().join(name), b"").expect("the file is written");
    }
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000))
        .expect("the folder is locked");
    (library, locked)
}

#[cfg(unix)]
fn unlock(locked: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(locked, std::fs::Permissions::from_mode(0o755))
        .expect("the folder is unlocked");
}

#[cfg(unix)]
#[test]
fn an_unreadable_folder_is_reported_and_the_rest_is_analyzed() {
    let (library, locked) = library_with_a_locked_folder();
    let cannot_enter = std::fs::read_dir(&locked).is_err();

    let output = Euphoria::new().run(&["analyze", "--json", library.path().to_str().unwrap()]);
    unlock(&locked);
    if !cannot_enter {
        return;
    }

    let stderr = standard_error(&output);
    assert_eq!(output.status.code(), Some(3), "{stderr}");
    assert!(stderr.contains("1 folder could not be read:"), "{stderr}");
    assert!(stderr.contains("Permission denied"), "{stderr}");
    assert!(
        stderr.contains("3 files skipped: 2 .opus, 1 .wma, which Euphoria can't decode yet"),
        "{stderr}"
    );
    let entries: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON");
    let entries = entries.as_array().expect("an array");
    let analyzed: Vec<_> = entries
        .iter()
        .filter(|entry| entry["error"].is_null())
        .collect();
    assert_eq!(analyzed.len(), 1);
    assert!(analyzed[0]["path"].as_str().unwrap().ends_with("tone.wav"));
    let unreadable: Vec<_> = entries
        .iter()
        .filter(|entry| entry["unreadable"] == true)
        .collect();
    assert_eq!(unreadable.len(), 1);
    assert!(unreadable[0]["path"].as_str().unwrap().ends_with("locked"));
    assert!(
        unreadable[0]["error"]
            .as_str()
            .unwrap()
            .contains("Permission denied")
    );
    let skipped = entries
        .iter()
        .filter(|entry| entry["skipped"] == true)
        .count();
    assert_eq!(skipped, 3);
}

#[test]
fn skipped_formats_are_reported_and_do_not_change_the_exit_status() {
    let library = TemporaryFolder::new();
    std::fs::copy(
        fixtures_directory().join("sine-440hz-mono.wav"),
        library.path().join("tone.wav"),
    )
    .expect("the fixture is copied");
    for name in ["a.opus", "b.opus", "c.wma"] {
        std::fs::write(library.path().join(name), b"").expect("the file is written");
    }

    let output = Euphoria::new().run(&["analyze", library.path().to_str().unwrap()]);

    let stderr = standard_error(&output);
    assert!(output.status.success(), "{stderr}");
    assert!(
        stderr.contains("3 files skipped: 2 .opus, 1 .wma, which Euphoria can't decode yet"),
        "{stderr}"
    );
}

fn write_fake_mp3(folder: &TemporaryFolder) -> std::path::PathBuf {
    let path = folder.path().join("fake.mp3");
    std::fs::write(&path, "this is not audio").expect("the file is written");
    path
}

#[test]
fn a_file_that_is_not_audio_gets_a_plain_error_without_the_decoder_wording() {
    let folder = TemporaryFolder::new();
    let fake = write_fake_mp3(&folder);
    let output = Euphoria::new().run(&["analyze", "--no-cache", fake.to_str().expect("UTF-8")]);
    assert_eq!(output.status.code(), Some(1));
    let errors = standard_error(&output);
    assert_eq!(
        errors.lines().next(),
        Some(
            format!(
                "error: {}: cannot be read as audio (not a supported or valid audio file)",
                fake.display()
            )
            .as_str()
        ),
        "{errors}"
    );
    assert!(!errors.contains("probe"), "{errors}");
    assert_eq!(errors.matches("fake.mp3").count(), 1, "{errors}");
}

#[test]
fn verbose_keeps_the_decoders_own_reason() {
    let folder = TemporaryFolder::new();
    let fake = write_fake_mp3(&folder);
    let output = Euphoria::new().run(&[
        "--verbose",
        "analyze",
        "--no-cache",
        fake.to_str().expect("UTF-8"),
    ]);
    let errors = standard_error(&output);
    assert!(errors.contains("cannot be read as audio"), "{errors}");
    assert!(errors.contains("cannot decode"), "{errors}");
}

#[test]
fn zero_jobs_is_refused_in_plain_words() {
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    let output = Euphoria::new().run(&[
        "analyze",
        "--jobs",
        "0",
        fixture.to_str().expect("UTF-8 path"),
    ]);
    assert_eq!(output.status.code(), Some(2));
    let errors = standard_error(&output);
    assert!(errors.contains("must be at least 1"), "{errors}");
    assert!(!errors.contains("non-zero"), "{errors}");
}

#[test]
fn an_empty_folder_exits_with_status_3() {
    let folder = TemporaryFolder::new();
    let output = Euphoria::new().run(&["analyze", folder.path().to_str().expect("UTF-8 path")]);
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    assert!(standard_error(&output).contains("no audio files found"));
}

#[test]
fn a_run_where_everything_is_missing_prints_no_table_header() {
    let output = Euphoria::new().run(&["analyze", "/definitely/not/here.mp3"]);
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty(), "{:?}", output.stdout);
}

#[test]
fn the_plain_table_follows_the_key_notation() {
    let fixture = fixtures_directory().join("sine-440hz-mono.wav");
    let path = fixture.to_str().expect("UTF-8 path");
    let camelot = Euphoria::new().run(&["analyze", "--no-cache", path]);
    let musical =
        Euphoria::new().run(&["analyze", "--no-cache", "--key-notation", "musical", path]);
    let first_cell = |output: &std::process::Output| {
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .nth(1)
            .and_then(|row| row.split_whitespace().next().map(str::to_string))
            .expect("a row")
    };
    assert_eq!(first_cell(&camelot), "7A");
    assert_eq!(first_cell(&musical), "Dm");
}

#[test]
fn completions_are_written_for_each_shell() {
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let output = Euphoria::new().run(&["completions", shell]);
        assert!(output.status.success(), "{shell}");
        let script = String::from_utf8_lossy(&output.stdout);
        assert!(script.contains("euphoria"), "{shell}");
        assert!(script.contains("completions"), "{shell}");
    }
    assert_eq!(
        Euphoria::new().run(&["completions", "tcsh"]).status.code(),
        Some(2)
    );
}
