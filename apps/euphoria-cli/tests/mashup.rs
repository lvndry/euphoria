// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `euphoria mashup`: the layering ideas for a track and a pair rendered to
//! a WAV file. Keys and tempos are set as the desktop app keeps them, and
//! stems are written straight into the stem cache, so the separation model
//! never runs.

#![cfg(feature = "stems")]

mod support;

use std::f64::consts::TAU;
use std::path::{Path, PathBuf};
use std::process::Output;

use euphoria_core::stems::{STEMS_MODEL, Stem};
use euphoria_core::{Cache, ContentHash, Correction, Key, Mode};
use support::{Euphoria, TemporaryFolder, standard_error, write_kick_track};

const STEM_SECONDS: usize = 3;

fn text(path: &Path) -> &str {
    path.to_str().expect("UTF-8 path")
}

fn assert_success(output: &Output) {
    assert!(output.status.success(), "{}", standard_error(output));
}

fn json(output: &Output) -> serde_json::Value {
    assert_success(output);
    serde_json::from_slice(&output.stdout).expect("JSON")
}

/// A 16 bit stereo WAV file at 44.1 kHz holding a tone at `hertz`, or
/// silence for `None`, as the separator writes stems.
fn write_stem(path: &Path, hertz: Option<f64>) {
    const SAMPLE_RATE: u32 = 44_100;
    let frames = STEM_SECONDS * SAMPLE_RATE as usize;
    let data_length = (frames * 4) as u32;
    let mut bytes = Vec::with_capacity(44 + frames * 4);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_length).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 4).to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_length.to_le_bytes());
    for frame in 0..frames {
        let seconds = frame as f64 / f64::from(SAMPLE_RATE);
        let sample = hertz.map_or(0.0, |hertz| 0.3 * (TAU * hertz * seconds).sin());
        let scaled = (sample * f64::from(i16::MAX)) as i16;
        bytes.extend_from_slice(&scaled.to_le_bytes());
        bytes.extend_from_slice(&scaled.to_le_bytes());
    }
    std::fs::write(path, bytes).expect("the stem is written");
}

/// Puts stems for `track` in the stem cache: the chords and leads at
/// `other_hertz`, and vocals at `vocal_hertz` or none.
fn cache_stems(euphoria: &Euphoria, track: &Path, other_hertz: f64, vocal_hertz: Option<f64>) {
    let hash = ContentHash::of_file(track).expect("the track is hashed");
    let hex: String = hash
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let directory = euphoria
        .data_directory()
        .join("stems")
        .join(format!("{STEMS_MODEL}-{hex}"));
    std::fs::create_dir_all(&directory).expect("the cache folder is created");
    for stem in Stem::ALL {
        let hertz = match stem {
            Stem::Drums => Some(150.0),
            Stem::Bass => Some(55.0),
            Stem::Other => Some(other_hertz),
            Stem::Vocals => vocal_hertz,
        };
        write_stem(&directory.join(format!("{}.wav", stem.name())), hertz);
    }
}

/// A pool of kick tracks keyed and timed as the app keeps a correction:
/// A in 8A, a singer and a bed in 8A with stems, one in 10A without stems
/// that needs a shift, and one in 2B that does not layer.
struct Pool {
    euphoria: Euphoria,
    _folder: TemporaryFolder,
    a: PathBuf,
    singer: PathBuf,
    bed: PathBuf,
    shifted: PathBuf,
}

fn pool() -> Pool {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let directory = folder.path().canonicalize().expect("the folder exists");
    let tracks = [
        ("a.wav", 124.0, Key::new(9, Mode::Minor)),
        ("singer.wav", 120.0, Key::new(9, Mode::Minor)),
        ("bed.wav", 126.0, Key::new(9, Mode::Minor)),
        ("shifted.wav", 122.0, Key::new(11, Mode::Minor)),
        ("apart.wav", 128.0, Key::new(6, Mode::Major)),
    ];
    for (name, tempo, _) in &tracks {
        write_kick_track(&directory.join(name), *tempo, 0.1);
    }
    assert_success(&euphoria.run(&["analyze", text(&directory)]));
    let mut cache = Cache::open(&euphoria.cache_path()).expect("the cache opens");
    for (name, _, key) in &tracks {
        let correction = Correction::of_key(*key)
            .with_bpm(Some(124.0))
            .and_then(|correction| correction.with_energy(Some(6)))
            .expect("a correction that can be kept");
        cache
            .set_correction(&directory.join(name), &correction)
            .expect("the correction is kept");
    }
    let a = directory.join("a.wav");
    let singer = directory.join("singer.wav");
    let bed = directory.join("bed.wav");
    cache_stems(&euphoria, &a, 330.0, Some(880.0));
    cache_stems(&euphoria, &singer, 660.0, Some(1_760.0));
    cache_stems(&euphoria, &bed, 2_500.0, None);
    Pool {
        euphoria,
        a,
        singer,
        bed,
        shifted: directory.join("shifted.wav"),
        _folder: folder,
    }
}

fn idea_names(ideas: &serde_json::Value) -> Vec<String> {
    ideas["ideas"]
        .as_array()
        .expect("ideas")
        .iter()
        .map(|idea| {
            Path::new(idea["path"].as_str().expect("a path"))
                .file_name()
                .expect("a file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

fn stems_of(idea: &serde_json::Value) -> (String, String) {
    (
        idea["stems"]["a"].as_str().expect("a part").to_string(),
        idea["stems"]["b"].as_str().expect("a part").to_string(),
    )
}

#[test]
fn ideas_say_the_shift_the_stretch_and_the_stems_and_leave_out_what_does_not_layer() {
    let pool = pool();
    let ideas = json(&pool.euphoria.run(&["mashup", text(&pool.a), "--json"]));
    assert_eq!(ideas["schema_version"], 1);
    assert_eq!(ideas["track"]["has_stems"], true);
    assert_eq!(
        idea_names(&ideas),
        ["bed.wav", "singer.wav", "shifted.wav"],
        "a bed with no vocals to mute first, a track without stems last"
    );
    let bed = &ideas["ideas"][0];
    assert_eq!(
        stems_of(bed),
        ("vocals".to_string(), "instrumental".to_string())
    );
    assert_eq!(bed["shift_semitones"], 0);
    assert_eq!(bed["reasons"][1], "Same tempo, no stretch");
    let shifted = &ideas["ideas"][2];
    assert_eq!(shifted["shift_semitones"], -2);
    assert_eq!(shifted["shifted_key"], "8A");
    assert_eq!(
        shifted["reasons"][0],
        "Shift B down 2 semitones: 10A becomes 8A, the same key as A"
    );
    assert!(shifted["stems"].is_null());
    assert!(
        shifted["reasons"][2]
            .as_str()
            .expect("a reason")
            .starts_with("Stems not compared"),
        "{shifted}"
    );

    let listed = pool.euphoria.run(&["mashup", text(&pool.a)]);
    assert_success(&listed);
    let listed = String::from_utf8_lossy(&listed.stdout);
    assert!(
        listed.starts_with("Layer over a  8A  124.0 BPM  energy 6"),
        "{listed}"
    );
    assert!(
        listed.contains("Shift B down 2 semitones: 10A becomes 8A"),
        "{listed}"
    );
    assert!(!listed.contains("apart"), "{listed}");
}

#[test]
fn a_part_chosen_for_a_ranks_the_tracks_that_go_with_it_first() {
    let pool = pool();
    let under_a_vocals =
        json(
            &pool
                .euphoria
                .run(&["mashup", text(&pool.a), "--stems", "a=vocals", "--json"]),
        );
    assert_eq!(under_a_vocals["part"], "vocals");
    assert_eq!(idea_names(&under_a_vocals)[..2], ["bed.wav", "singer.wav"]);
    for idea in &under_a_vocals["ideas"].as_array().expect("ideas")[..2] {
        assert_eq!(
            stems_of(idea),
            ("vocals".to_string(), "instrumental".to_string())
        );
    }

    let over_a_instrumental = json(&pool.euphoria.run(&[
        "mashup",
        text(&pool.a),
        "--stems",
        "a=instrumental",
        "--json",
    ]));
    assert_eq!(
        idea_names(&over_a_instrumental)[0],
        "singer.wav",
        "the bed has no vocals to put over A's instrumental"
    );
    assert_eq!(
        stems_of(&over_a_instrumental["ideas"][0]),
        ("instrumental".to_string(), "vocals".to_string())
    );

    let paired_from_b =
        json(
            &pool
                .euphoria
                .run(&["mashup", text(&pool.a), "--stems", "b=vocals", "--json"]),
        );
    assert_eq!(paired_from_b["part"], "instrumental");
}

#[test]
fn the_pool_can_be_narrowed_like_the_desktop_filter() {
    let pool = pool();
    let same_key_only = json(&pool.euphoria.run(&[
        "mashup",
        text(&pool.a),
        "--key-moves",
        "same",
        "--limit",
        "1",
        "--json",
    ]));
    assert_eq!(idea_names(&same_key_only), ["bed.wav"]);
    let out_of_range =
        json(
            &pool
                .euphoria
                .run(&["mashup", text(&pool.a), "--energy", "8-", "--json"]),
        );
    assert!(idea_names(&out_of_range).is_empty());
    let no_genre =
        json(
            &pool
                .euphoria
                .run(&["mashup", text(&pool.a), "--genre", "house", "--json"]),
        );
    assert!(
        idea_names(&no_genre).is_empty(),
        "tracks without a genre tag are out"
    );
}

/// The channels, sample rate and bits per sample of a WAV file.
fn wav_format(path: &Path) -> (u16, u32, u16) {
    let bytes = std::fs::read(path).expect("the mashup is read");
    let field = |start: usize, length: usize| bytes[start..start + length].to_vec();
    (
        u16::from_le_bytes(field(22, 2).try_into().expect("a WAV header field")),
        u32::from_le_bytes(field(24, 4).try_into().expect("a WAV header field")),
        u16::from_le_bytes(field(34, 2).try_into().expect("a WAV header field")),
    )
}

#[test]
fn one_part_given_brings_the_other_part_to_the_other_track() {
    let pool = pool();
    let folder = TemporaryFolder::new();
    let output = folder.path().join("pair.wav");
    let rendered = json(&pool.euphoria.run(&[
        "mashup",
        text(&pool.a),
        text(&pool.singer),
        "--render",
        text(&output),
        "--stems",
        "a=vocals",
        "--json",
    ]));
    assert_eq!(rendered["a"]["part"], "vocals");
    assert_eq!(rendered["b"]["part"], "instrumental");
    assert_eq!(rendered["b"]["rate"], 1.0);
    assert_eq!(wav_format(&output), (2, 44_100, 24));
    let seconds = rendered["seconds"].as_f64().expect("a length");
    assert!(
        (seconds - STEM_SECONDS as f64).abs() < 0.01,
        "the stems last {STEM_SECONDS} seconds: {seconds}"
    );

    let other_way = folder.path().join("other-way.wav");
    let rendered = json(&pool.euphoria.run(&[
        "mashup",
        text(&pool.a),
        text(&pool.singer),
        "--render",
        text(&other_way),
        "--stems",
        "b=vocals",
        "--json",
    ]));
    assert_eq!(rendered["a"]["part"], "instrumental");
    assert_eq!(rendered["b"]["part"], "vocals");

    let as_the_idea = folder.path().join("idea.wav");
    let rendered = json(&pool.euphoria.run(&[
        "mashup",
        text(&pool.a),
        text(&pool.bed),
        "--render",
        text(&as_the_idea),
        "--json",
    ]));
    assert_eq!(
        (&rendered["a"]["part"], &rendered["b"]["part"]),
        (
            &serde_json::json!("vocals"),
            &serde_json::json!("instrumental")
        ),
        "without --stems the pair plays as its idea does"
    );
}

#[test]
fn a_render_takes_the_suggested_shift_unless_one_is_given() {
    let pool = pool();
    let folder = TemporaryFolder::new();
    let output = folder.path().join("shifted.wav");
    let rendered = pool.euphoria.run(&[
        "mashup",
        text(&pool.a),
        text(&pool.shifted),
        "--render",
        text(&output),
    ]);
    assert_success(&rendered);
    let summary = String::from_utf8_lossy(&rendered.stdout);
    assert!(summary.starts_with("Wrote "), "{summary}");
    assert!(
        summary.contains("B  shifted: the whole track, shifted down 2 semitones to 8A"),
        "{summary}"
    );

    let refused = pool.euphoria.run(&[
        "mashup",
        text(&pool.a),
        text(&pool.shifted),
        "--render",
        text(&output),
    ]);
    assert!(!refused.status.success());
    assert!(
        standard_error(&refused).contains("--force"),
        "{}",
        standard_error(&refused)
    );

    let rendered = json(&pool.euphoria.run(&[
        "mashup",
        text(&pool.a),
        text(&pool.shifted),
        "--render",
        text(&output),
        "--shift",
        "1",
        "--force",
        "--json",
    ]));
    assert_eq!(rendered["b"]["shift_semitones"], 1);
    assert_eq!(rendered["b"]["part"], "full");
}

#[test]
fn a_render_never_writes_over_one_of_its_tracks() {
    let pool = pool();
    let before = std::fs::read(&pool.a).expect("the track is read");
    let refused = pool.euphoria.run(&[
        "mashup",
        text(&pool.a),
        text(&pool.shifted),
        "--render",
        text(&pool.a),
        "--force",
    ]);
    assert!(!refused.status.success());
    assert_eq!(std::fs::read(&pool.a).expect("the track is read"), before);

    let not_wav = pool.euphoria.run(&[
        "mashup",
        text(&pool.a),
        text(&pool.shifted),
        "--render",
        "pair.flac",
    ]);
    assert!(!not_wav.status.success());
    assert!(standard_error(&not_wav).contains(".wav"));
}
