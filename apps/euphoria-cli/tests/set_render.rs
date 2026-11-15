// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! `euphoria set render` on short generated kick tracks.

mod support;

use std::path::{Path, PathBuf};

use support::{Euphoria, TemporaryFolder, standard_error, write_kicks_lasting};

fn three_tracks(folder: &Path) -> Vec<PathBuf> {
    [("one.wav", 122.0), ("two.wav", 124.0), ("three.wav", 126.0)]
        .into_iter()
        .map(|(name, tempo)| {
            let path = folder.join(name);
            write_kicks_lasting(&path, 50, 0.25, |_| tempo);
            path
        })
        .collect()
}

fn text(path: &Path) -> String {
    std::fs::read_to_string(path).expect("the file is written")
}

#[test]
fn a_set_renders_to_one_mix_with_its_tracklist_cue_sheet_and_chapters() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let tracks = three_tracks(folder.path());
    let mix = folder.path().join("mix.wav");
    let mut arguments = vec!["set", "render"];
    arguments.extend(tracks.iter().map(|track| track.to_str().unwrap()));
    arguments.extend([
        "--out",
        mix.to_str().unwrap(),
        "--tracklist",
        "--cue",
        "--chapters",
        "--no-cache",
    ]);
    let output = euphoria.run(&arguments);
    assert!(output.status.success(), "{}", standard_error(&output));
    let plan = String::from_utf8_lossy(&output.stdout);
    assert!(plan.contains("3 tracks"), "{plan}");
    assert_eq!(plan.matches("↳ ").count(), 2, "{plan}");

    let header = std::fs::read(&mix).expect("the mix is written");
    assert_eq!(&header[0..4], b"RIFF");
    assert_eq!(&header[8..12], b"WAVE");

    let cue = text(&folder.path().join("mix.cue"));
    assert!(cue.contains("FILE \"mix.wav\" WAVE"), "{cue}");
    assert_eq!(cue.matches("TRACK ").count(), 3, "{cue}");
    assert!(cue.contains("INDEX 01 00:00:00"), "{cue}");
    let chapters = text(&folder.path().join("mix.chapters.txt"));
    assert!(chapters.starts_with("0:00 one\n"), "{chapters}");
    assert_eq!(chapters.lines().count(), 3, "{chapters}");
    let tracklist = text(&folder.path().join("mix.tracklist.txt"));
    assert!(tracklist.contains("01. 0:00 one"), "{tracklist}");
    assert!(tracklist.contains("Length "), "{tracklist}");

    let again = euphoria.run(&arguments);
    assert!(!again.status.success());
    assert!(
        standard_error(&again).contains("Pass --force to replace it"),
        "{}",
        standard_error(&again)
    );
}

#[test]
fn a_dry_run_shows_the_plan_and_writes_nothing() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let tracks = three_tracks(folder.path());
    let playlist = folder.path().join("set.m3u8");
    std::fs::write(&playlist, "#EXTM3U\none.wav\ntwo.wav\n").unwrap();
    let mix = folder.path().join("mix.flac");
    let output = euphoria.run(&[
        "set",
        "render",
        playlist.to_str().unwrap(),
        "--out",
        mix.to_str().unwrap(),
        "--cue",
        "--dry-run",
    ]);
    assert!(output.status.success(), "{}", standard_error(&output));
    let plan = String::from_utf8_lossy(&output.stdout);
    assert!(plan.starts_with("set  2 tracks"), "{plan}");
    assert!(!mix.exists());
    assert!(!folder.path().join("mix.cue").exists());
    assert_eq!(tracks.len(), 3);
}

#[test]
fn a_folder_or_an_unknown_format_is_refused_before_any_work() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let tracks = three_tracks(folder.path());
    let folder_output = euphoria.run(&[
        "set",
        "render",
        folder.path().to_str().unwrap(),
        "--dry-run",
    ]);
    assert!(!folder_output.status.success());
    assert!(standard_error(&folder_output).contains("has no play order"));

    let ogg = folder.path().join("mix.ogg");
    let ogg_output = euphoria.run(&[
        "set",
        "render",
        tracks[0].to_str().unwrap(),
        "--out",
        ogg.to_str().unwrap(),
    ]);
    assert!(!ogg_output.status.success());
    assert!(standard_error(&ogg_output).contains("must end in .wav, .flac or .mp3"));

    let clash = euphoria.run(&[
        "set",
        "render",
        tracks[0].to_str().unwrap(),
        "--out",
        folder.path().join("mix.wav").to_str().unwrap(),
        "--format",
        "flac",
    ]);
    assert!(!clash.status.success());
    assert!(standard_error(&clash).contains("but --format is flac"));
}

#[test]
fn a_transition_locked_in_the_app_is_kept() {
    use euphoria_core::{BlendBars, LockedTransition, PlaylistEntry, PlaylistStore};
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let tracks = three_tracks(folder.path());
    let entry = |path: &Path| PlaylistEntry::for_file(path, None).expect("the file is read");
    let lock = LockedTransition {
        from: entry(&tracks[0]),
        to: entry(&tracks[1]),
        blend: BlendBars {
            a_start_bar: 4,
            b_start_bar: 0,
            bars: 16,
            bass_swap_bar: Some(9),
        },
    };
    let mut store = PlaylistStore::open(&euphoria.data_directory().join("playlists.sqlite3"))
        .expect("the playlists database opens");
    store.lock_transition(&lock).expect("the lock is kept");
    drop(store);

    let output = euphoria.run(&[
        "set",
        "render",
        tracks[0].to_str().unwrap(),
        tracks[1].to_str().unwrap(),
        "--dry-run",
        "--no-cache",
    ]);
    assert!(output.status.success(), "{}", standard_error(&output));
    let plan = String::from_utf8_lossy(&output.stdout);
    assert!(plan.contains("↳ Locked, placed by hand. "), "{plan}");
    assert!(plan.contains("16 bars"), "{plan}");
}

#[test]
fn a_set_renders_to_mp3_with_a_chapter_per_track() {
    let euphoria = Euphoria::new();
    let folder = TemporaryFolder::new();
    let tracks = three_tracks(folder.path());
    let mix = folder.path().join("mix.mp3");
    let output = euphoria.run(&[
        "set",
        "render",
        tracks[0].to_str().unwrap(),
        tracks[1].to_str().unwrap(),
        "--out",
        mix.to_str().unwrap(),
        "--mp3-bitrate",
        "192",
        "--cue",
        "--no-cache",
    ]);
    assert!(output.status.success(), "{}", standard_error(&output));
    assert!(standard_error(&output).contains("MP3 at 192 kbps"));
    let bytes = std::fs::read(&mix).expect("the mix is written");
    assert_eq!(&bytes[..3], b"ID3");
    assert_eq!(
        bytes.windows(4).filter(|window| *window == b"CHAP").count(),
        2
    );
    let cue = text(&folder.path().join("mix.cue"));
    assert!(cue.contains("FILE \"mix.mp3\" MP3"), "{cue}");

    let refused = euphoria.run(&[
        "set",
        "render",
        tracks[0].to_str().unwrap(),
        "--out",
        mix.to_str().unwrap(),
        "--mp3-bitrate",
        "300",
    ]);
    assert!(!refused.status.success());
    assert!(standard_error(&refused).contains("an MP3 bitrate is one of"));
}
