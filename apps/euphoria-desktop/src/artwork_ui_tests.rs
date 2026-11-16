// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests that the cover of a track is drawn where a track is shown, in a
//! headless window, and that the music note stays when there is none.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext};
use image::{DynamicImage, ImageFormat, Rgb, RgbImage};

use crate::app::EuphoriaApp;
use crate::demo::DemoTrack;
use crate::playback::testing::SilentOutput;
use crate::tag_writing::testing::ScratchFolder;
use crate::ui_tests::{click, is_shown, open_library, open_window_on, track};
use euphoria_core::TrackAnalysis;

fn cover_png() -> Vec<u8> {
    let picture = RgbImage::from_fn(120, 90, |x, y| Rgb([(x * 2) as u8, (y * 2) as u8, 90]));
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(picture)
        .write_to(&mut bytes, ImageFormat::Png)
        .expect("the picture encodes");
    bytes.into_inner()
}

fn analysed() -> TrackAnalysis {
    TrackAnalysis {
        duration_seconds: 1.0,
        key: None,
        bpm: None,
        energy: None,
        tuning_cents: None,
        key_runner_up: None,
        key_score_margin: None,
        key_source: None,
        tempo_estimate: None,
        tempo_score_margin: None,
        tempo_octave_evidence: None,
        beat_grid: None,
        cues: Vec::new(),
        key_timeline: Vec::new(),
        quality: None,
        loudness_range_lu: None,
        pitch_class_profile: None,
    }
}

fn open_one_track(
    folder: &Path,
    with_cover: bool,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/euphoria-core/tests/fixtures/sine-440hz-mono.mp3");
    let path: PathBuf = folder.join("track.mp3");
    std::fs::copy(fixture, &path).expect("the fixture is copied");
    if with_cover {
        std::fs::write(folder.join("cover.png"), cover_png()).expect("the cover is written");
    }
    let tracks = vec![DemoTrack {
        path,
        outcome: Ok(analysed()),
    }];
    let (window, view) = open_window_on(tracks, Box::new(SilentOutput), cx);
    click(window, track(0), cx);
    (window, view)
}

fn settle(window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui::test]
fn the_inspector_and_the_player_bar_show_the_cover_of_the_selected_track(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("cover-shown");
    let (window, view) = open_one_track(&scratch.0, true, cx);
    settle(window, cx);

    assert!(is_shown(window, "inspector-cover", cx));
    assert!(is_shown(window, "player-cover", cx));
    cx.update(|cx| assert_eq!(view.read(cx).artwork().decode_count(), 1));
}

#[gpui::test]
fn a_track_without_a_cover_keeps_the_placeholder(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("cover-missing");
    let (window, _view) = open_one_track(&scratch.0, false, cx);
    settle(window, cx);

    assert!(!is_shown(window, "inspector-cover", cx));
    assert!(!is_shown(window, "player-cover", cx));
}

#[gpui::test]
fn a_big_library_reads_no_cover_until_a_track_is_shown(cx: &mut TestAppContext) {
    let (window, view) = open_library(2_000, cx);
    settle(window, cx);

    cx.update(|cx| {
        let store = view.read(cx).artwork();
        assert!(store.len() <= 4, "{} covers asked for", store.len());
    });
}

#[gpui::test]
fn the_focus_view_shows_the_cover_in_its_header(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("cover-focus");
    let (window, _view) = open_one_track(&scratch.0, true, cx);
    click(window, "focus-open", cx);
    settle(window, cx);
    settle(window, cx);

    assert!(is_shown(window, "focus-cover", cx));
}
