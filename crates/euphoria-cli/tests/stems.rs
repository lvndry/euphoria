// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Export cached stems without loading or downloading the network.

#![cfg(feature = "stems")]

mod support;

use std::path::{Path, PathBuf};

use euphoria_core::ContentHash;
use euphoria_core::stems::{STEMS_MODEL, Stem};
use support::{Euphoria, TemporaryFolder, fixtures_directory, standard_error};

fn cache_stems(app: &Euphoria, input: &Path) -> Vec<u8> {
    let hash = ContentHash::of_file(input).expect("the fixture is hashed");
    let hex: String = hash
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let directory = app
        .data_directory()
        .join("stems")
        .join(format!("{STEMS_MODEL}-{hex}"));
    std::fs::create_dir_all(&directory).expect("the cache fixture folder is created");
    let audio = std::fs::read(fixtures_directory().join("sine-440hz-stereo.wav"))
        .expect("the WAV fixture is read");
    for stem in Stem::ALL {
        std::fs::write(directory.join(format!("{}.wav", stem.name())), &audio)
            .expect("the cache fixture is written");
    }
    audio
}

fn input(folder: &TemporaryFolder, name: &str) -> PathBuf {
    let path = folder.path().join(name);
    std::fs::copy(fixtures_directory().join("sine-440hz-mono.wav"), &path)
        .expect("the input fixture is copied");
    path
}

#[test]
fn forcing_stems_into_the_inputs_folder_never_replaces_the_input() {
    let app = Euphoria::new();
    let folder = TemporaryFolder::new();
    let input = input(&folder, "vocals.wav");
    let original = std::fs::read(&input).unwrap();
    cache_stems(&app, &input);
    let result = app.run(&[
        "stems",
        input.to_str().unwrap(),
        "--output",
        folder.path().to_str().unwrap(),
        "--force",
    ]);
    assert_eq!(
        std::fs::read(&input).unwrap(),
        original,
        "the input was replaced by a stem"
    );
    assert!(!result.status.success(), "{}", standard_error(&result));
}

#[test]
fn forcing_an_output_hard_link_leaves_the_original_bytes_unchanged() {
    let app = Euphoria::new();
    let folder = TemporaryFolder::new();
    let input = input(&folder, "original.wav");
    let original = std::fs::read(&input).unwrap();
    let separated = cache_stems(&app, &input);
    let output = folder.path().join("outputs");
    std::fs::create_dir(&output).unwrap();
    std::fs::hard_link(&input, output.join("vocals.wav")).unwrap();
    let result = app.run(&[
        "stems",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--force",
    ]);
    assert!(result.status.success(), "{}", standard_error(&result));
    assert_eq!(
        std::fs::read(&input).unwrap(),
        original,
        "replacing a stem must not truncate a linked original"
    );
    for stem in Stem::ALL {
        assert_eq!(
            std::fs::read(output.join(format!("{}.wav", stem.name()))).unwrap(),
            separated
        );
    }
}

#[test]
fn a_failed_forced_export_keeps_the_previous_complete_set() {
    let app = Euphoria::new();
    let folder = TemporaryFolder::new();
    let input = input(&folder, "original.wav");
    cache_stems(&app, &input);
    let output = folder.path().join("outputs");
    std::fs::create_dir(&output).unwrap();
    for stem in [Stem::Drums, Stem::Bass, Stem::Other] {
        std::fs::write(
            output.join(format!("{}.wav", stem.name())),
            b"previous stem",
        )
        .unwrap();
    }
    std::fs::create_dir(output.join("vocals.wav")).unwrap();
    std::fs::write(output.join("vocals.wav/keep"), b"unrelated directory").unwrap();
    let result = app.run(&[
        "stems",
        input.to_str().unwrap(),
        "--output",
        output.to_str().unwrap(),
        "--force",
    ]);
    assert!(!result.status.success());
    for stem in [Stem::Drums, Stem::Bass, Stem::Other] {
        assert_eq!(
            std::fs::read(output.join(format!("{}.wav", stem.name()))).unwrap(),
            b"previous stem"
        );
    }
    assert_eq!(
        std::fs::read(output.join("vocals.wav/keep")).unwrap(),
        b"unrelated directory"
    );
}
