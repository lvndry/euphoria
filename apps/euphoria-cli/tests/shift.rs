// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shifted exports retain corrected analysis and leave both source and existing outputs safe.

mod support;

use euphoria_core::{AudioStream, Cache, Correction, Key, Mode};
use support::{Euphoria, TemporaryFolder, fixtures_directory, standard_error};

#[test]
fn shift_writes_long_named_wav_and_flac_with_the_corrected_key_and_tempo() {
    let app = Euphoria::new();
    let folder = TemporaryFolder::new();
    let input = folder.path().join("original.flac");
    std::fs::copy(fixtures_directory().join("sine-440hz-mono.flac"), &input).unwrap();
    let original = std::fs::read(&input).unwrap();
    let key = Key::new(9, Mode::Minor);
    Cache::open(&app.cache_path())
        .unwrap()
        .set_correction(
            &input,
            &Correction::default()
                .with_key(Some(key))
                .with_bpm(Some(123.0))
                .unwrap(),
        )
        .unwrap();
    for extension in ["wav", "flac"] {
        let output = folder
            .path()
            .join(format!("{}.{}", "a".repeat(240), extension));
        let args = [
            "shift",
            "-s",
            "-3",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ];
        let shifted = app.run(&args);
        assert!(shifted.status.success(), "{}", standard_error(&shifted));
        let tags = euphoria_core::tags::read_tags(&output).unwrap();
        assert_eq!(
            tags.key.unwrap().parse::<Key>().unwrap(),
            key.transposed(-3)
        );
        assert_eq!(tags.bpm.unwrap().parse::<f32>().unwrap(), 123.0);
        let mut source = AudioStream::open(&output).unwrap();
        assert_eq!(source.sample_rate().get(), 44_100);
        let mut samples = Vec::new();
        let mut frames = 0;
        while source.read_chunk(&mut samples).unwrap() {
            frames += samples.len() / usize::from(source.channel_count().get());
        }
        assert_eq!(frames, 44_100);
        let existing = std::fs::read(&output).unwrap();
        let refused = app.run(&args);
        assert!(!refused.status.success());
        assert_eq!(std::fs::read(&output).unwrap(), existing);
        assert_eq!(std::fs::read(&input).unwrap(), original);
    }
    let same = app.run(&[
        "shift",
        "-s",
        "2",
        input.to_str().unwrap(),
        "-o",
        input.to_str().unwrap(),
        "--force",
    ]);
    assert!(!same.status.success());
    assert_eq!(std::fs::read(&input).unwrap(), original);
}
