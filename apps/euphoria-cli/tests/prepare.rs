// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `euphoria prepare` writes new files at the target loudness and leaves
//! originals alone, and its dry run reports the numbers it then applies.

mod support;

use std::f32::consts::TAU;
use std::path::Path;

use serde_json::Value;
use support::{Euphoria, TemporaryFolder, fixtures_directory, standard_error};

const RATE: u32 = 44_100;

fn write_tone(path: &Path, amplitude: f32) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: RATE,
        bits_per_sample: 24,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("the tone file is created");
    for index in 0..RATE * 5 {
        let value = (TAU * 997.0 * index as f32 / RATE as f32).sin() * amplitude;
        let sample = (value * 8_388_607.0).round() as i32;
        writer.write_sample(sample).expect("a sample is written");
        writer.write_sample(sample).expect("a sample is written");
    }
    writer.finalize().expect("the tone file is finished");
}

fn json(output: &std::process::Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            standard_error(output)
        )
    })
}

#[test]
fn a_dry_run_writes_nothing_and_reports_the_numbers_the_real_run_applies() {
    let app = Euphoria::new();
    let folder = TemporaryFolder::new();
    let input = folder.path().join("quiet.wav");
    write_tone(&input, 0.02);
    let original = std::fs::read(&input).unwrap();
    let out = folder.path().join("prepared");

    let dry = app.run(&[
        "prepare",
        input.to_str().unwrap(),
        "--loudness",
        "-9",
        "--true-peak",
        "-1",
        "--out",
        out.to_str().unwrap(),
        "--dry-run",
        "--json",
        "--no-cache",
    ]);
    assert!(dry.status.success(), "{}", standard_error(&dry));
    assert!(!out.exists(), "a dry run creates nothing");
    let dry = json(&dry);
    assert_eq!(dry["settings"]["dry_run"], true);
    let planned = &dry["tracks"][0];
    assert_eq!(planned["status"], "dry_run");
    assert_eq!(planned["output_format"], "wav");
    let gain = planned["gain_db"].as_f64().unwrap();
    assert!((gain - (-9.0 - planned["measured_lufs"].as_f64().unwrap())).abs() < 0.01);

    let applied = app.run(&[
        "prepare",
        input.to_str().unwrap(),
        "--loudness",
        "-9",
        "--true-peak",
        "-1",
        "--out",
        out.to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert!(applied.status.success(), "{}", standard_error(&applied));
    let applied = json(&applied);
    let written = &applied["tracks"][0];
    assert_eq!(written["status"], "prepared");
    for key in [
        "measured_lufs",
        "true_peak_dbtp",
        "gain_db",
        "limiting_db",
        "gain_capped",
        "predicted_lufs",
        "tuning_correction_cents",
    ] {
        assert_eq!(planned[key], written[key], "{key}");
    }
    let output = out.join("quiet (prepared).wav");
    assert_eq!(written["output"], output.to_str().unwrap());
    assert!(output.is_file());
    assert_eq!(std::fs::read(&input).unwrap(), original);

    let again = app.run(&[
        "prepare",
        input.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--json",
        "--no-cache",
    ]);
    assert!(again.status.success(), "{}", standard_error(&again));
    assert!(out.join("quiet (prepared) 2.wav").is_file());
    assert!(output.is_file());
}

#[test]
fn lossy_tracks_become_flac_or_wav_and_text_output_lists_each_track() {
    let app = Euphoria::new();
    let folder = TemporaryFolder::new();
    let input = folder.path().join("song.mp3");
    std::fs::copy(fixtures_directory().join("sine-440hz-mono.mp3"), &input).unwrap();
    let flac = app.run(&["prepare", input.to_str().unwrap(), "--no-cache"]);
    assert!(flac.status.success(), "{}", standard_error(&flac));
    let text = String::from_utf8_lossy(&flac.stdout);
    assert!(text.contains("loudness"), "{text}");
    assert!(text.contains("song (prepared).flac"), "{text}");
    assert!(folder.path().join("song (prepared).flac").is_file());

    let wav = app.run(&[
        "prepare",
        input.to_str().unwrap(),
        "--lossy-as",
        "wav",
        "--suffix",
        " [club]",
        "--no-cache",
    ]);
    assert!(wav.status.success(), "{}", standard_error(&wav));
    assert!(folder.path().join("song [club].wav").is_file());
    assert!(!folder.path().join("song (prepared).mp3").exists());
}

#[test]
fn a_bad_target_is_refused_before_anything_is_written() {
    let app = Euphoria::new();
    let folder = TemporaryFolder::new();
    let input = folder.path().join("tone.wav");
    write_tone(&input, 0.1);
    let refused = app.run(&["prepare", input.to_str().unwrap(), "--loudness", "5"]);
    assert!(!refused.status.success());
    let refused = app.run(&["prepare", input.to_str().unwrap(), "--suffix", "a/b"]);
    assert!(!refused.status.success());
    assert!(!folder.path().join("tone (prepared).wav").exists());
}

#[test]
fn tune_reads_the_tuning_of_each_track_from_its_analysis() {
    let app = Euphoria::new();
    let folder = TemporaryFolder::new();
    let input = folder.path().join("a.wav");
    std::fs::copy(fixtures_directory().join("sine-440hz-stereo.wav"), &input).unwrap();
    let dry = app.run(&[
        "prepare",
        input.to_str().unwrap(),
        "--tune",
        "--dry-run",
        "--json",
        "--no-cache",
    ]);
    assert!(dry.status.success(), "{}", standard_error(&dry));
    let document = json(&dry);
    assert_eq!(document["settings"]["correct_tuning"], true);
    let track = &document["tracks"][0];
    let measured = track["tuning_cents"].as_f64();
    if let Some(cents) = measured {
        assert!(cents.abs() < 3.0, "a 440 Hz tone is in tune: {cents}");
    }
    assert!(track["tuning_correction_cents"].is_null());
}
