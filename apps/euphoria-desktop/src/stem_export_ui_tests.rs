// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of saving stems in the real root view, in a headless window. The
//! stems are generated: four 16 bit stereo WAV files of tones, the way the
//! separator leaves them.

use std::f64::consts::TAU;
use std::path::{Path, PathBuf};

use euphoria_core::key::{Key, Mode};
use euphoria_core::stems::{Mixdown, STEMS_SAMPLE_RATE, Stem, StemFiles, StemShift};
use euphoria_core::tags::read_tags;
use euphoria_core::{AudioStream, TrackAnalysis};
use gpui::{Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::demo::DemoTrack;
use crate::stem_export::{StemExportRequest, StemExportTarget, mix_file_name, stems_request};
use crate::tag_writing::testing::ScratchFolder;
use crate::ui_tests::{click, is_shown, open_window_on};

const FRAMES: usize = 44_100;
const TRACK_KEY_TONIC: u8 = 9;

fn stem_samples(stem: Stem, loudness: f64) -> Vec<f32> {
    let frequency = match stem {
        Stem::Drums => 150.0,
        Stem::Bass => 55.0,
        Stem::Other => 330.0,
        Stem::Vocals => 880.0,
    };
    (0..FRAMES)
        .flat_map(|frame| {
            let seconds = frame as f64 / f64::from(STEMS_SAMPLE_RATE);
            let left = (TAU * frequency * seconds).sin() * loudness;
            let right = (TAU * frequency * 1.01 * seconds).sin() * loudness;
            [left as f32, right as f32]
        })
        .collect()
}

fn write_wav(path: &Path, samples: &[f32]) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: STEMS_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for sample in samples {
        writer
            .write_sample((sample.clamp(-1.0, 1.0) * 32767.0).round() as i16)
            .unwrap();
    }
    writer.finalize().unwrap();
}

fn read_wav(path: &Path) -> Vec<f32> {
    let mut stream = AudioStream::open(path).unwrap();
    let mut samples = Vec::new();
    let mut chunk = Vec::new();
    while stream.read_chunk(&mut chunk).unwrap() {
        samples.extend_from_slice(&chunk);
    }
    samples
}

fn track_with_a_key() -> DemoTrack {
    DemoTrack {
        path: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/euphoria-core/tests/fixtures/sine-440hz-stereo.wav"),
        outcome: Ok(TrackAnalysis {
            duration_seconds: 1.0,
            key: Some(Key::new(TRACK_KEY_TONIC, Mode::Minor)),
            bpm: Some(120.0),
            energy: Some(5),
            tuning_cents: None,
            key_runner_up: None,
            key_score_margin: None,
            tempo_estimate: None,
            tempo_score_margin: None,
            beat_grid: None,
            cues: Vec::new(),
            key_timeline: Vec::new(),
            quality: None,
        }),
    }
}

fn open_keyed_library(cx: &mut TestAppContext) -> (gpui::AnyWindowHandle, Entity<EuphoriaApp>) {
    let (window, view) = open_window_on(
        vec![track_with_a_key()],
        Box::new(crate::playback::testing::SilentOutput),
        cx,
    );
    cx.update(|cx| view.update(cx, |app, cx| app.select(0, cx)));
    (window, view)
}

/// Marks the selected track as separated into generated stems of `loudness`.
fn separate_selected_track(
    view: &Entity<EuphoriaApp>,
    scratch: &ScratchFolder,
    loudness: f64,
    cx: &mut TestAppContext,
) -> StemFiles {
    let files = StemFiles::in_directory(scratch.0.join("stems"));
    std::fs::create_dir_all(files.directory()).unwrap();
    for stem in Stem::ALL {
        write_wav(&files.path(stem), &stem_samples(stem, loudness));
    }
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let path = app.selected_track().unwrap().path.clone();
            app.stems.mark_ready(&path, files.clone());
            app.stems_changed(cx);
        })
    });
    files
}

fn separated_track(
    scratch: &ScratchFolder,
    loudness: f64,
    cx: &mut TestAppContext,
) -> (gpui::AnyWindowHandle, Entity<EuphoriaApp>, StemFiles) {
    let (window, view) = open_keyed_library(cx);
    let files = separate_selected_track(&view, scratch, loudness, cx);
    (window, view, files)
}

fn export(
    view: &Entity<EuphoriaApp>,
    request: impl FnOnce(&EuphoriaApp) -> StemExportRequest,
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let request = request(app);
            app.begin_stem_export(request, cx);
        })
    });
    cx.run_until_parked();
}

fn notice(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> String {
    cx.update(|cx| view.read(cx).notice.clone().unwrap_or_default())
}

fn stems_into(folder: PathBuf, shifted: bool) -> impl FnOnce(&EuphoriaApp) -> StemExportRequest {
    move |app| {
        let source = app.stem_export_source(shifted).unwrap();
        stems_request(source.title, source.files, &folder, source.shift, false)
    }
}

fn mix_into(
    destination: PathBuf,
    mixdown: Mixdown,
    shifted: bool,
) -> impl FnOnce(&EuphoriaApp) -> StemExportRequest {
    move |app| {
        let source = app.stem_export_source(shifted).unwrap();
        StemExportRequest {
            title: source.title,
            files: source.files,
            target: StemExportTarget::Mix(mixdown, destination),
            shift: source.shift,
            replace: true,
        }
    }
}

fn shift_by(view: &Entity<EuphoriaApp>, semitones: i8, cx: &mut TestAppContext) {
    cx.update(|cx| view.update(cx, |app, cx| app.set_key_shift(semitones, cx)));
}

fn within_rounding(actual: &[f32], expected: &[f32], least_significant_bits: f32) {
    assert_eq!(actual.len(), expected.len());
    let worst = actual
        .iter()
        .zip(expected)
        .map(|(actual, expected)| (actual - expected).abs())
        .fold(0.0f32, f32::max);
    assert!(
        worst <= least_significant_bits / 32768.0,
        "off by {} least significant bits",
        worst * 32768.0
    );
}

#[gpui::test]
fn export_stems_writes_the_four_files_each_with_its_stem(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("stem-export-four");
    let (_, view, files) = separated_track(&scratch, 0.2, cx);
    let folder = scratch.0.join("out");
    std::fs::create_dir_all(&folder).unwrap();

    export(&view, stems_into(folder.clone(), false), cx);

    for stem in Stem::ALL {
        let written = folder.join(format!("sine-440hz-stereo - {}.wav", stem.name()));
        assert!(written.is_file(), "{}", written.display());
        assert_eq!(read_wav(&written), read_wav(&files.path(stem)));
    }
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 4);
    let message = notice(&view, cx);
    assert!(
        message.starts_with("Saved the stems of sine-440hz-stereo to"),
        "{message}"
    );
}

#[gpui::test]
fn export_asks_before_replacing_and_keeps_the_file_when_declined(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("stem-export-replace");
    let (window, view, files) = separated_track(&scratch, 0.2, cx);
    let folder = scratch.0.join("out");
    std::fs::create_dir_all(&folder).unwrap();
    let existing = folder.join("sine-440hz-stereo - bass.wav");
    std::fs::write(&existing, b"mine").unwrap();

    export(&view, stems_into(folder.clone(), false), cx);
    assert_eq!(std::fs::read(&existing).unwrap(), b"mine");
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
    assert!(is_shown(window, "stem-export-confirmation", cx));

    click(window, "cancel-stem-export", cx);
    cx.run_until_parked();
    assert!(!is_shown(window, "stem-export-confirmation", cx));
    assert_eq!(std::fs::read(&existing).unwrap(), b"mine");
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);

    export(&view, stems_into(folder.clone(), false), cx);
    click(window, "replace-stem-export", cx);
    cx.run_until_parked();
    assert_eq!(read_wav(&existing), read_wav(&files.path(Stem::Bass)));
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 4);
}

#[gpui::test]
fn the_acapella_is_the_vocals_and_the_instrumental_the_sum_of_the_rest(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("stem-export-mixes");
    let (_, view, files) = separated_track(&scratch, 0.2, cx);
    let acapella = scratch.0.join("acapella.wav");
    let instrumental = scratch.0.join("instrumental.wav");

    export(
        &view,
        mix_into(acapella.clone(), Mixdown::Acapella, false),
        cx,
    );
    export(
        &view,
        mix_into(instrumental.clone(), Mixdown::Instrumental, false),
        cx,
    );

    within_rounding(
        &read_wav(&acapella),
        &read_wav(&files.path(Stem::Vocals)),
        2.0,
    );
    let drums = read_wav(&files.path(Stem::Drums));
    let bass = read_wav(&files.path(Stem::Bass));
    let other = read_wav(&files.path(Stem::Other));
    let sum: Vec<f32> = (0..drums.len())
        .map(|index| drums[index] + bass[index] + other[index])
        .collect();
    within_rounding(&read_wav(&instrumental), &sum, 2.0);
    let message = notice(&view, cx);
    assert!(!message.contains("scaled down"), "{message}");
}

#[gpui::test]
fn a_mix_that_would_clip_is_scaled_down_and_says_so(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("stem-export-clipping");
    let (_, view, _) = separated_track(&scratch, 0.6, cx);
    let instrumental = scratch.0.join("instrumental.wav");

    export(
        &view,
        mix_into(instrumental.clone(), Mixdown::Instrumental, false),
        cx,
    );

    let written = read_wav(&instrumental);
    let peak = written
        .iter()
        .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
    assert!(peak > 0.99 && peak <= 1.0, "{peak}");
    let message = notice(&view, cx);
    assert!(message.contains("scaled down by"), "{message}");
}

#[gpui::test]
fn shifted_exports_are_tagged_with_the_new_key_and_keep_their_length(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("stem-export-shifted");
    let (window, view, files) = separated_track(&scratch, 0.2, cx);
    assert!(!is_shown(window, "stems-export-stems-shifted", cx));
    shift_by(&view, 2, cx);
    assert!(is_shown(window, "stems-export-stems-shifted", cx));
    assert!(is_shown(window, "stems-export-acapella-shifted", cx));
    assert!(is_shown(window, "stems-export-instrumental-shifted", cx));
    let expected = Key::new(TRACK_KEY_TONIC, Mode::Minor).transposed(2);

    let folder = scratch.0.join("out");
    std::fs::create_dir_all(&folder).unwrap();
    export(&view, stems_into(folder.clone(), true), cx);
    for stem in Stem::ALL {
        let written = folder.join(format!("sine-440hz-stereo - {} (+2).wav", stem.name()));
        let tags = read_tags(&written).unwrap();
        assert_eq!(
            tags.key.as_deref().and_then(Key::parse_file_key),
            Some(expected),
            "{:?}",
            tags.key
        );
        assert_eq!(read_wav(&written).len(), read_wav(&files.path(stem)).len());
    }

    let acapella = scratch.0.join(mix_file_name(
        "sine-440hz-stereo",
        Mixdown::Acapella,
        Some(&StemShift {
            semitones: 2,
            values: Default::default(),
        }),
    ));
    export(
        &view,
        mix_into(acapella.clone(), Mixdown::Acapella, true),
        cx,
    );
    let tags = read_tags(&acapella).unwrap();
    assert_eq!(
        tags.key.as_deref().and_then(Key::parse_file_key),
        Some(expected)
    );
    assert_eq!(
        read_wav(&acapella).len(),
        read_wav(&files.path(Stem::Vocals)).len()
    );
    let message = notice(&view, cx);
    assert!(message.contains("shifted by +2 semitones"), "{message}");
}

#[gpui::test]
fn the_export_buttons_show_once_the_stems_are_ready(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("stem-export-buttons");
    let (window, view) = open_keyed_library(cx);
    assert!(!is_shown(window, "stems-export-stems", cx));

    separate_selected_track(&view, &scratch, 0.2, cx);
    assert!(is_shown(window, "stems-export-stems", cx));
    assert!(is_shown(window, "stems-export-acapella", cx));
    assert!(is_shown(window, "stems-export-instrumental", cx));
    assert!(!is_shown(window, "stems-export-stems-shifted", cx));
}
