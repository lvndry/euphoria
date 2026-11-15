// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `euphoria quality` on a small library of generated files, one per
//! problem: a truncated file, a lossless file low-passed like a 128 kbit/s
//! MP3, a 128 kbit/s MP3, a clipped file and a clean lossless file.

mod support;

use std::path::{Path, PathBuf};

use rustfft::FftPlanner;
use rustfft::num_complex::Complex;
use support::{Euphoria, TemporaryFolder, standard_error};

const SAMPLE_RATE: u32 = 44_100;
const SECONDS: usize = 4;

/// White noise from a fixed seed, so every run writes the same files.
fn noise(seed: u32, amplitude: f32) -> Vec<f32> {
    let mut state = seed;
    (0..SAMPLE_RATE as usize * SECONDS)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * amplitude
        })
        .collect()
}

/// `samples` with everything above `cutoff_hz` removed, as a lossy
/// encoder's lowpass does.
fn low_passed(samples: &[f32], cutoff_hz: f32) -> Vec<f32> {
    let length = samples.len();
    let mut spectrum: Vec<Complex<f32>> = samples
        .iter()
        .map(|sample| Complex::new(*sample, 0.0))
        .collect();
    let mut planner = FftPlanner::<f32>::new();
    planner.plan_fft_forward(length).process(&mut spectrum);
    for (bin, value) in spectrum.iter_mut().enumerate() {
        let hz = bin.min(length - bin) as f32 * SAMPLE_RATE as f32 / length as f32;
        if hz > cutoff_hz {
            *value = Complex::default();
        }
    }
    planner.plan_fft_inverse(length).process(&mut spectrum);
    spectrum
        .into_iter()
        .map(|value| value.re / length as f32)
        .collect()
}

/// A 16 bit stereo WAV whose header declares `declared_frames` frames.
fn wav_bytes(left: &[f32], right: &[f32], declared_frames: usize) -> Vec<u8> {
    let data_bytes = (declared_frames * 4) as u32;
    let mut bytes = b"RIFF".to_vec();
    bytes.extend((36 + data_bytes).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(2_u16.to_le_bytes());
    bytes.extend(SAMPLE_RATE.to_le_bytes());
    bytes.extend((SAMPLE_RATE * 4).to_le_bytes());
    bytes.extend(4_u16.to_le_bytes());
    bytes.extend(16_u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(data_bytes.to_le_bytes());
    for (left, right) in left.iter().zip(right) {
        for sample in [left, right] {
            bytes.extend(((sample.clamp(-1.0, 1.0) * 32_767.0) as i16).to_le_bytes());
        }
    }
    bytes
}

fn write_wav(path: &Path, left: &[f32], right: &[f32]) {
    std::fs::write(path, wav_bytes(left, right, left.len())).expect("the WAV file is written");
}

/// A folder holding one file per problem and a clean one.
fn library() -> TemporaryFolder {
    let folder = TemporaryFolder::new();
    let crate_folder = folder.path().join("Crate");
    std::fs::create_dir_all(&crate_folder).expect("the subfolder is created");
    let left = noise(0x2545_f491, 0.25);
    let right = noise(0x9e37_79b9, 0.25);
    write_wav(&folder.path().join("clean.wav"), &left, &right);
    write_wav(
        &crate_folder.join("fake lossless.wav"),
        &low_passed(&left, 16_000.0),
        &low_passed(&right, 16_000.0),
    );
    let loud = |samples: &[f32]| {
        samples
            .iter()
            .map(|sample| sample * 6.0)
            .collect::<Vec<_>>()
    };
    write_wav(
        &folder.path().join("clipped.wav"),
        &loud(&left),
        &loud(&right),
    );
    let half = left.len() / 2;
    std::fs::write(
        folder.path().join("truncated.wav"),
        wav_bytes(&left[..half], &right[..half], left.len()),
    )
    .expect("the truncated WAV file is written");
    std::fs::copy(
        quality_fixture("noise-128k.mp3"),
        crate_folder.join("128k.mp3"),
    )
    .expect("the MP3 fixture is copied");
    folder
}

fn quality_fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/euphoria-core/tests/quality-fixtures")
        .join(name)
}

fn run(euphoria: &Euphoria, arguments: &[&str]) -> String {
    let output = euphoria.run(arguments);
    assert!(output.status.success(), "{}", standard_error(&output));
    String::from_utf8(output.stdout).expect("the output is UTF-8")
}

fn json(euphoria: &Euphoria, arguments: &[&str]) -> serde_json::Value {
    serde_json::from_str(&run(euphoria, arguments)).expect("the output is JSON")
}

const SUMMARY: &str =
    "4 of 5 tracks to replace: 1 heavily damaged, 1 fake lossless, 1 low bit rate, 1 clipped.";

#[test]
fn lists_the_files_to_replace_worst_first_with_why() {
    let library = library();
    let folder = library.path().to_str().expect("UTF-8 path");
    let euphoria = Euphoria::new();
    let listing = run(&euphoria, &["quality", folder]);
    let lines: Vec<&str> = listing.lines().collect();
    assert_eq!(lines[0], SUMMARY, "{listing}");
    let blocks: Vec<&[&str]> = lines[1..].split(|line| line.is_empty()).skip(1).collect();
    assert_eq!(blocks.len(), 4, "{listing}");
    assert_eq!(
        blocks[0],
        [
            "Heavily damaged  truncated",
            "  Lossless: 2.0 s missing at the end",
            "  truncated.wav",
        ]
    );
    assert_eq!(
        blocks[1],
        [
            "Fake lossless    fake lossless",
            "  Likely re-encoded: WAV, but the sound stops at 16.0 kHz like a 128 kbit/s MP3",
            "  Crate/fake lossless.wav",
        ]
    );
    assert_eq!(
        blocks[2][0],
        "Low bit rate     Harbour Lights - Low Tide Dub"
    );
    assert!(
        blocks[2][1].starts_with("  Weak on a big system: MP3, 1")
            && blocks[2][1].ends_with(" kbit/s"),
        "{listing}"
    );
    assert_eq!(blocks[2][2], "  Crate/128k.mp3");
    assert_eq!(blocks[3][0], "Clipped          clipped");
    assert!(
        blocks[3][1].starts_with("  Lossless: ")
            && blocks[3][1].ends_with("% of the samples clipped at full scale"),
        "{listing}"
    );
    assert!(!listing.contains("clean"), "{listing}");
}

#[test]
fn a_second_run_reads_the_cache() {
    let library = library();
    let folder = library.path().to_str().expect("UTF-8 path");
    let euphoria = Euphoria::new();
    run(&euphoria, &["analyze", "--json", folder]);
    let output = euphoria.run(&["quality", folder]);
    assert!(output.status.success(), "{}", standard_error(&output));
    assert!(
        standard_error(&output).contains("5 of 5 tracks came from the cache."),
        "{}",
        standard_error(&output)
    );
    assert!(String::from_utf8_lossy(&output.stdout).starts_with(SUMMARY));
}

#[test]
fn json_lists_every_track_with_all_and_the_counts() {
    let library = library();
    let folder = library.path().to_str().expect("UTF-8 path");
    let euphoria = Euphoria::new();
    let document = json(&euphoria, &["quality", "--json", "--all", folder]);
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["threshold"], "weak-on-a-big-system");
    let summary = &document["summary"];
    assert_eq!(summary["tracks"], 5);
    assert_eq!(summary["to_replace"], 4);
    assert_eq!(summary["line"], SUMMARY);
    let counted: Vec<(&str, u64)> = summary["by_problem"]
        .as_array()
        .expect("counts")
        .iter()
        .map(|count| {
            (
                count["problem"].as_str().expect("a problem"),
                count["count"].as_u64().expect("a count"),
            )
        })
        .collect();
    assert_eq!(
        counted,
        [
            ("heavily-damaged", 1),
            ("fake-lossless", 1),
            ("low-bitrate", 1),
            ("clipped", 1),
        ]
    );
    let tracks = document["tracks"].as_array().expect("tracks");
    let order: Vec<(&str, &str, bool)> = tracks
        .iter()
        .map(|track| {
            (
                track["relative_path"].as_str().expect("a path"),
                track["verdict"].as_str().expect("a verdict"),
                track["to_replace"].as_bool().expect("a flag"),
            )
        })
        .collect();
    assert_eq!(
        order,
        [
            ("truncated.wav", "lossless", true),
            ("Crate/fake lossless.wav", "likely-reencoded", true),
            ("Crate/128k.mp3", "weak-on-a-big-system", true),
            ("clipped.wav", "lossless", true),
            ("clean.wav", "lossless", false),
        ]
    );
    let mp3 = &tracks[2];
    assert_eq!(mp3["artist"], "Harbour Lights");
    assert_eq!(mp3["title"], "Low Tide Dub");
    assert_eq!(mp3["problems"], serde_json::json!(["low-bitrate"]));
    assert!(Path::new(mp3["path"].as_str().expect("a path")).is_absolute());
    assert!(
        mp3["explanation"]
            .as_str()
            .expect("an explanation")
            .contains("on a club system the top end and detail suffer"),
        "{mp3}"
    );
    let truncated = &tracks[0];
    assert!(
        truncated["notes"][0]
            .as_str()
            .expect("a note")
            .starts_with("Damaged file:"),
        "{truncated}"
    );
    assert_eq!(tracks[4]["problems"], serde_json::json!([]));
    assert_eq!(tracks[4]["reasons"], serde_json::json!(["WAV"]));
}

#[test]
fn the_threshold_moves_with_min() {
    let library = library();
    let folder = library.path().to_str().expect("UTF-8 path");
    let euphoria = Euphoria::new();
    let document = json(
        &euphoria,
        &["quality", "--json", "--min", "reencoded", folder],
    );
    assert_eq!(document["threshold"], "likely-reencoded");
    let listed: Vec<&str> = document["tracks"]
        .as_array()
        .expect("tracks")
        .iter()
        .map(|track| track["relative_path"].as_str().expect("a path"))
        .collect();
    assert_eq!(
        listed,
        ["truncated.wav", "Crate/fake lossless.wav", "clipped.wav"]
    );
}

#[test]
fn writes_a_csv_file_and_a_playlist_of_the_files_to_replace() {
    let library = library();
    let folder = library.path().to_str().expect("UTF-8 path");
    let outputs = TemporaryFolder::new();
    let csv_file = outputs.path().join("quality.csv");
    let m3u_file = outputs.path().join("replace.m3u8");
    let euphoria = Euphoria::new();
    let output = euphoria.run(&[
        "quality",
        "--all",
        "--csv",
        csv_file.to_str().expect("UTF-8 path"),
        "--m3u",
        m3u_file.to_str().expect("UTF-8 path"),
        folder,
    ]);
    assert!(output.status.success(), "{}", standard_error(&output));
    let messages = standard_error(&output);
    assert!(messages.contains("Wrote 5 tracks to "), "{messages}");
    assert!(
        messages.contains("Wrote 4 tracks to replace to "),
        "{messages}"
    );

    let csv = std::fs::read_to_string(&csv_file).expect("the CSV file is written");
    let rows: Vec<&str> = csv.split("\r\n").collect();
    assert_eq!(
        rows[0],
        "Path,Artist,Title,Verdict,To replace,Problems,Reason,Full path"
    );
    assert!(
        rows[1]
            .starts_with("truncated.wav,,,Lossless,yes,heavily damaged,2.0 s missing at the end,"),
        "{csv}"
    );
    assert!(
        rows[2].starts_with(
            "Crate/fake lossless.wav,,,Likely re-encoded,yes,fake lossless,\"WAV, but the sound stops at 16.0 kHz like a 128 kbit/s MP3\","
        ),
        "{csv}"
    );
    assert!(
        rows[3].starts_with("Crate/128k.mp3,Harbour Lights,Low Tide Dub,Weak on a big system,yes,low bit rate,\"MP3, 1"),
        "{csv}"
    );
    assert!(
        rows[4].starts_with("clipped.wav,,,Lossless,yes,clipped,"),
        "{csv}"
    );
    assert!(
        rows[5].starts_with("clean.wav,,,Lossless,no,,WAV,"),
        "{csv}"
    );
    assert_eq!(rows[6], "");

    let playlist = std::fs::read_to_string(&m3u_file).expect("the playlist is written");
    let lines: Vec<&str> = playlist.lines().collect();
    assert_eq!(lines[..2], ["#EXTM3U", "#PLAYLIST:Files to replace"]);
    let entries: Vec<(&str, &str)> = lines[2..]
        .chunks(2)
        .map(|entry| (entry[0], entry[1]))
        .collect();
    assert_eq!(entries.len(), 4, "{playlist}");
    assert_eq!(entries[0].0, "#EXTINF:2,truncated");
    assert_eq!(entries[2].0, "#EXTINF:4,Harbour Lights - Low Tide Dub");
    for ((_, path), name) in entries.iter().zip([
        "truncated.wav",
        "Crate/fake lossless.wav",
        "Crate/128k.mp3",
        "clipped.wav",
    ]) {
        assert!(Path::new(path).is_absolute(), "{path}");
        assert!(path.ends_with(name), "{path}");
    }

    let again = euphoria.run(&[
        "quality",
        "--m3u",
        m3u_file.to_str().expect("UTF-8 path"),
        folder,
    ]);
    assert!(!again.status.success());
    assert!(
        standard_error(&again).contains("already exists"),
        "{}",
        standard_error(&again)
    );
}

#[test]
fn the_analyze_quality_filter_still_lists_by_verdict() {
    let library = library();
    let folder = library.path().to_str().expect("UTF-8 path");
    let euphoria = Euphoria::new();
    let tracks = json(
        &euphoria,
        &["analyze", "--json", "--quality", "reencoded,weak", folder],
    );
    let mut listed: Vec<&str> = tracks
        .as_array()
        .expect("tracks")
        .iter()
        .map(|track| track["path"].as_str().expect("a path"))
        .collect();
    listed.sort_unstable();
    assert_eq!(listed.len(), 2, "{tracks}");
    assert!(listed[0].ends_with("128k.mp3") && listed[1].ends_with("fake lossless.wav"));
}
