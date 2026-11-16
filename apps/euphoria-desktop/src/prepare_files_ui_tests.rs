// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of the prepare sheet in the real root view, in a headless window.

use std::path::PathBuf;

use euphoria_core::prepare::{LossyOutput, PrepareSettings, plan_track};
use gpui::{Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::prepare_files::PrepareStage;
use crate::tag_writing::testing::ScratchFolder;
use crate::toast::ToastAction;
use crate::ui_tests::{click, is_shown, open_fixture_library};

fn open_sheet(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) {
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.select_all_shown(cx);
            app.open_prepare_files(cx);
        })
    });
    cx.run_until_parked();
}

fn rows(
    view: &Entity<EuphoriaApp>,
    cx: &mut TestAppContext,
) -> Vec<crate::prepare_files::PreviewRow> {
    cx.update(|cx| view.read(cx).prepare_panel.as_ref().unwrap().rows.clone())
}

fn fixture_paths() -> Vec<PathBuf> {
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/euphoria-core/tests/fixtures");
    [
        "sine-440hz-mono.flac",
        "sine-440hz-mono.mp3",
        "sine-440hz-stereo.wav",
    ]
    .into_iter()
    .map(|name| fixtures.join(name))
    .collect()
}

#[gpui::test]
fn the_sheet_lists_each_selected_file_with_the_numbers_of_a_dry_run(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    assert!(!is_shown(window, "prepare-files-card", cx));
    open_sheet(&view, cx);
    assert!(is_shown(window, "prepare-files-card", cx));
    for index in 0..3_usize {
        assert!(is_shown(window, ("prepare-row", index), cx));
    }
    for id in [
        "prepare-loudness--14",
        "prepare-loudness--11",
        "prepare-loudness--9",
        "prepare-loudness--7",
        "prepare-tune",
        "prepare-lossy-flac",
        "prepare-lossy-wav",
        "prepare-choose-folder",
        "prepare-confirm",
        "prepare-cancel",
    ] {
        assert!(is_shown(window, id, cx), "{id}");
    }

    let shown = rows(&view, cx);
    assert_eq!(shown.len(), 3);
    let settings = PrepareSettings::default();
    for (row, path) in shown.iter().zip(fixture_paths()) {
        let dry_run = plan_track(&path, &settings, None, LossyOutput::Flac).unwrap();
        assert_eq!(row.plan(), Some(&dry_run), "{}", row.file_name);
    }
    let formats: Vec<_> = shown
        .iter()
        .map(|row| row.output.as_ref().unwrap().extension().unwrap().to_owned())
        .collect();
    assert_eq!(
        formats,
        ["flac", "flac", "wav"],
        "the MP3 never stays lossy"
    );

    click(window, "prepare-loudness--14", cx);
    for row in rows(&view, cx) {
        assert_eq!(row.plan().unwrap().target_lufs, -14.0);
    }
    click(window, "prepare-lossy-wav", cx);
    assert_eq!(
        rows(&view, cx)[1]
            .output
            .as_ref()
            .unwrap()
            .extension()
            .unwrap(),
        "wav"
    );
    click(window, "prepare-tune", cx);
    cx.update(|cx| {
        assert!(
            view.read(cx)
                .prepare_panel
                .as_ref()
                .unwrap()
                .settings
                .correct_tuning
        );
    });
    click(window, "prepare-cancel", cx);
    assert!(!is_shown(window, "prepare-files-card", cx));
}

#[gpui::test]
fn preparing_writes_new_files_leaves_originals_alone_and_ends_with_a_toast(
    cx: &mut TestAppContext,
) {
    let scratch = ScratchFolder::new("prepare-files");
    let output = scratch.0.join("prepared");
    let originals: Vec<Vec<u8>> = fixture_paths()
        .iter()
        .map(|path| std::fs::read(path).unwrap())
        .collect();
    let (window, view) = open_fixture_library(cx);
    open_sheet(&view, cx);
    let folder = output.clone();
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.change_prepare_options(|panel| panel.output_folder = Some(folder), cx)
        })
    });
    assert!(
        rows(&view, cx)
            .iter()
            .all(|row| row.output.as_ref().unwrap().starts_with(&output))
    );
    assert!(!output.exists(), "the preview writes nothing");

    click(window, "prepare-confirm", cx);

    let names: Vec<String> = std::fs::read_dir(&output)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    for expected in [
        "sine-440hz-mono (prepared).flac",
        "sine-440hz-mono (prepared) 2.flac",
        "sine-440hz-stereo (prepared).wav",
    ] {
        assert!(names.iter().any(|name| name == expected), "{names:?}");
    }
    for (path, original) in fixture_paths().iter().zip(&originals) {
        assert_eq!(&std::fs::read(path).unwrap(), original);
    }
    cx.update(|cx| {
        let app = view.read(cx);
        let toast = app.toasts.current().expect("a toast ends the write");
        assert_eq!(toast.message, "Prepared 3 files");
        assert!(matches!(toast.action, Some(ToastAction::Reveal(_))));
        assert!(app.prepare_panel.is_none(), "the sheet closes when done");
    });
}

#[gpui::test]
fn a_progress_bar_shows_while_files_are_written(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    open_sheet(&view, cx);
    assert!(!is_shown(window, "prepare-progress", cx));
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.prepare_panel.as_mut().unwrap().stage = PrepareStage::Writing;
            cx.notify();
        })
    });
    assert!(is_shown(window, "prepare-progress", cx));
    assert!(is_shown(window, "prepare-writing", cx));
    assert!(!is_shown(window, "prepare-confirm", cx));
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.prepare_panel.as_mut().unwrap().stage = PrepareStage::Setup;
            cx.notify();
        })
    });
    assert!(is_shown(window, "prepare-confirm", cx));
}

#[gpui::test]
fn nothing_opens_without_a_selection(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.selected = None;
            app.marked.clear();
            app.open_prepare_files(cx)
        })
    });
    assert!(!is_shown(window, "prepare-files-card", cx));
}

fn scratch_library(
    name: &str,
    cx: &mut TestAppContext,
) -> (
    ScratchFolder,
    Vec<PathBuf>,
    gpui::AnyWindowHandle,
    Entity<EuphoriaApp>,
) {
    let scratch = ScratchFolder::new(name);
    let copies: Vec<PathBuf> = fixture_paths()
        .iter()
        .map(|path| {
            let copy = scratch.0.join(path.file_name().unwrap());
            std::fs::copy(path, &copy).unwrap();
            copy
        })
        .collect();
    let tracks = copies
        .iter()
        .map(|path| crate::demo::DemoTrack {
            path: path.clone(),
            outcome: Ok(euphoria_core::TrackAnalysis {
                duration_seconds: 1.0,
                key: None,
                bpm: None,
                energy: None,
                tuning_cents: None,
                key_runner_up: None,
                key_score_margin: None,
                key_source: None,
                loudness_range_lu: None,
                pitch_class_profile: None,
                tempo_estimate: None,
                tempo_score_margin: None,
                beat_grid: None,
                cues: Vec::new(),
                key_timeline: Vec::new(),
                quality: None,
            }),
        })
        .collect();
    let (window, view) = crate::ui_tests::open_window_on(
        tracks,
        Box::new(crate::playback::testing::SilentOutput),
        cx,
    );
    (scratch, copies, window, view)
}

#[gpui::test]
fn tag_only_writes_replaygain_into_the_originals_and_revert_puts_them_back(
    cx: &mut TestAppContext,
) {
    let (_scratch, copies, window, view) = scratch_library("prepare-tag-only", cx);
    let audio_before: Vec<_> = copies.iter().map(|path| decoded(path)).collect();
    open_sheet(&view, cx);
    click(window, "prepare-mode-tags", cx);
    assert!(is_shown(window, "prepare-tag-only-note", cx));
    assert!(!is_shown(window, "prepare-tune", cx));
    assert!(!is_shown(window, "prepare-revert", cx));

    click(window, "prepare-confirm", cx);

    for (path, before) in copies.iter().zip(&audio_before) {
        let fields = euphoria_core::prepare::read_replaygain(path).unwrap();
        assert!(fields.gain.unwrap().ends_with(" dB"));
        assert!(fields.peak.is_some());
        assert_eq!(&decoded(path), before, "the audio is untouched");
    }
    assert!(
        !copies[0]
            .with_file_name("sine-440hz-mono (prepared).flac")
            .exists()
    );
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.toasts.current().unwrap().message, "Tagged 3 files");
        assert!(!app.last_prepare_manifests.is_empty());
    });

    open_sheet(&view, cx);
    assert!(is_shown(window, "prepare-revert", cx));
    click(window, "prepare-revert", cx);
    for path in &copies {
        let fields = euphoria_core::prepare::read_replaygain(path).unwrap();
        assert_eq!(fields, euphoria_core::prepare::ReplayGainFields::default());
    }
    cx.update(|cx| assert!(view.read(cx).last_prepare_manifests.is_empty()));
}

fn decoded(path: &std::path::Path) -> Vec<f32> {
    let mut stream = euphoria_core::AudioStream::open(path).unwrap();
    let mut all = Vec::new();
    let mut chunk = Vec::new();
    while stream.read_chunk(&mut chunk).unwrap() {
        all.extend_from_slice(&chunk);
    }
    all
}

#[gpui::test]
fn reverting_a_copy_run_removes_the_copies_and_nothing_else(cx: &mut TestAppContext) {
    let (scratch, copies, window, view) = scratch_library("prepare-revert", cx);
    open_sheet(&view, cx);
    click(window, "prepare-confirm", cx);
    let made: Vec<_> = std::fs::read_dir(&scratch.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("(prepared)"))
        .collect();
    assert_eq!(made.len(), 3, "{made:?}");
    open_sheet(&view, cx);
    click(window, "prepare-revert", cx);
    let left: Vec<_> = std::fs::read_dir(&scratch.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        left.iter().all(|name| !name.contains("(prepared)")),
        "{left:?}"
    );
    assert!(
        left.iter()
            .all(|name| !name.starts_with("euphoria-prepare-"))
    );
    for copy in &copies {
        assert!(copy.exists());
    }
}

#[gpui::test]
fn a_partial_revert_can_be_retried_after_the_file_is_restored(cx: &mut TestAppContext) {
    let (scratch, _copies, window, view) = scratch_library("prepare-revert-retry", cx);
    open_sheet(&view, cx);
    click(window, "prepare-confirm", cx);
    let prepared = std::fs::read_dir(&scratch.0)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().contains("(prepared)"))
        .unwrap();
    let original = std::fs::read(&prepared).unwrap();
    std::fs::write(&prepared, b"changed after preparing").unwrap();

    open_sheet(&view, cx);
    click(window, "prepare-revert", cx);
    assert_eq!(
        std::fs::read(&prepared).unwrap(),
        b"changed after preparing"
    );
    cx.update(|cx| assert!(!view.read(cx).last_prepare_manifests.is_empty()));

    std::fs::write(&prepared, original).unwrap();
    assert!(is_shown(window, "prepare-revert", cx));
    click(window, "prepare-revert", cx);
    assert!(!prepared.exists());
    cx.update(|cx| assert!(view.read(cx).last_prepare_manifests.is_empty()));
}

#[gpui::test]
fn the_comparison_plays_both_versions_in_place_and_writes_nothing(cx: &mut TestAppContext) {
    let (scratch, _copies, window, view) = scratch_library("prepare-ab", cx);
    open_sheet(&view, cx);
    assert!(is_shown(window, "prepare-ab-hint", cx));
    click(window, ("prepare-row", 2_usize), cx);
    assert!(is_shown(window, "prepare-ab-listen", cx));
    click(window, "prepare-ab-listen", cx);
    assert!(is_shown(window, "prepare-ab-prepared", cx));
    assert!(is_shown(window, "prepare-ab-original", cx));
    cx.update(|cx| {
        let app = view.read(cx);
        let comparison = app
            .prepare_panel
            .as_ref()
            .unwrap()
            .comparison
            .as_ref()
            .unwrap();
        let clip = comparison.clip.as_ref().unwrap();
        assert_eq!(clip.original.len(), clip.prepared.len());
        assert!(
            comparison
                .prepared
                .load(std::sync::atomic::Ordering::Relaxed)
        );
    });
    click(window, "prepare-ab-original", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        let comparison = app
            .prepare_panel
            .as_ref()
            .unwrap()
            .comparison
            .as_ref()
            .unwrap();
        assert!(
            !comparison
                .prepared
                .load(std::sync::atomic::Ordering::Relaxed)
        );
    });
    let files_now = std::fs::read_dir(&scratch.0).unwrap().count();
    assert_eq!(files_now, 3, "nothing is written for the comparison");
    click(window, "prepare-ab-stop", cx);
    assert!(is_shown(window, "prepare-ab-listen", cx));
}
