// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The note strip in the inspector: 12 bars for the notes the key analysis
//! heard, the notes of the key lit, and a line asking for a new analysis
//! when a track was analyzed before notes were measured. In a headless
//! window with the made up demo library.

use euphoria_core::PITCH_CLASS_COUNT;
use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Entity, Pixels, TestAppContext};

use crate::app::EuphoriaApp;
use crate::demo::{self, DemoTrack};
use crate::playback::testing::SilentOutput;
use crate::tag_settings::NotationChoice;
use crate::theme::ThemeName;
use crate::ui_tests::{is_shown, open_window_on};

const TRACKS: usize = 20;

fn open(tracks: Vec<DemoTrack>, cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_window_on(tracks, Box::new(SilentOutput), cx)
}

fn without_notes(mut tracks: Vec<DemoTrack>) -> Vec<DemoTrack> {
    for track in &mut tracks {
        if let Ok(analysis) = &mut track.outcome {
            analysis.pitch_class_profile = None;
        }
    }
    tracks
}

fn selected_profile(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> [f32; 12] {
    cx.update(|cx| {
        view.read(cx)
            .selected_track()
            .and_then(|track| track.analysis())
            .and_then(|analysis| analysis.pitch_class_profile)
            .expect("the selected demo track has notes")
    })
}

fn bar_heights(window: AnyWindowHandle, cx: &mut TestAppContext) -> Vec<Pixels> {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        (0..PITCH_CLASS_COUNT)
            .map(|pitch_class| {
                window
                    .find(("note-level", pitch_class))
                    .bounds()
                    .size
                    .height
            })
            .collect()
    })
    .unwrap()
}

#[gpui::test]
fn the_inspector_shows_twelve_bars_for_the_selected_track(cx: &mut TestAppContext) {
    let (window, _) = open(demo::tracks(TRACKS), cx);
    assert!(is_shown(window, "note-strip", cx));
    assert!(is_shown(window, "notes-key", cx));
    for pitch_class in 0..PITCH_CLASS_COUNT {
        assert!(
            is_shown(window, ("note-bar", pitch_class), cx),
            "{pitch_class}"
        );
    }
    assert!(!is_shown(window, "notes-not-measured", cx));
}

#[gpui::test]
fn the_loudest_note_has_the_tallest_bar(cx: &mut TestAppContext) {
    let (window, view) = open(demo::tracks(TRACKS), cx);
    let profile = selected_profile(&view, cx);
    let heights = bar_heights(window, cx);
    let loudest = (0..PITCH_CLASS_COUNT)
        .max_by(|left, right| profile[*left].total_cmp(&profile[*right]))
        .unwrap();
    let quietest = (0..PITCH_CLASS_COUNT)
        .min_by(|left, right| profile[*left].total_cmp(&profile[*right]))
        .unwrap();
    assert!(
        heights.iter().all(|height| *height <= heights[loudest]),
        "{heights:?}"
    );
    assert!(heights[quietest] < heights[loudest], "{heights:?}");
}

#[gpui::test]
fn a_track_analyzed_before_notes_were_measured_asks_for_a_new_analysis(cx: &mut TestAppContext) {
    let (window, _) = open(without_notes(demo::tracks(TRACKS)), cx);
    assert!(is_shown(window, "notes-not-measured", cx));
    assert!(!is_shown(window, "note-strip", cx));
}

#[gpui::test]
fn the_strip_stays_with_another_key_notation_and_in_every_theme(cx: &mut TestAppContext) {
    let (window, view) = open(demo::tracks(TRACKS), cx);
    for notation in NotationChoice::ALL {
        cx.update(|cx| view.update(cx, |app, cx| app.set_key_notation(notation, cx)));
        assert!(is_shown(window, "notes-key", cx), "{notation:?}");
    }
    for name in ThemeName::ALL {
        cx.update(|cx| view.update(cx, |app, cx| app.set_theme(name, cx)));
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let inspector = window.find("inspector").bounds();
            let first = window.find(("note-bar", 0_usize)).bounds();
            let last = window.find(("note-bar", PITCH_CLASS_COUNT - 1)).bounds();
            assert!(inspector.contains(&first.center()), "{name:?}");
            assert!(inspector.contains(&last.center()), "{name:?}");
            assert!(first.right() < last.left(), "{name:?}");
        })
        .unwrap();
    }
}
