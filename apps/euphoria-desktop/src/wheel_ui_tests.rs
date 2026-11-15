// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Camelot wheel's matching keys and its menu of key moves, used through
//! the real root view in a headless window.
//!
//! The library is the decks' three fixtures with the FLAC moved to 12A, so
//! the matching keys wrap round the top of the wheel: 11A and 1A one step
//! either way and 12B the relative major.

use euphoria_core::harmony::Relation;
use euphoria_core::{Key, Mode};
use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::playback::testing::SilentOutput;
use crate::ui_tests::{click, deck_tracks, index_of_track, is_shown, open_window_on, track};

const FLAC: &str = "sine-440hz-mono.flac";

fn open_twelve_a(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let mut tracks = deck_tracks();
    for track in &mut tracks {
        if track.path.ends_with(FLAC)
            && let Ok(analysis) = &mut track.outcome
        {
            analysis.key = Some(Key::new(1, Mode::Minor));
        }
    }
    let (window, view) = open_window_on(tracks, Box::new(SilentOutput), cx);
    let flac = index_of_track(&view, FLAC, cx);
    cx.update(|cx| view.update(cx, |app, cx| app.select(flac, cx)));
    cx.run_until_parked();
    (window, view)
}

/// The codes of the cells drawn as matching keys, in wheel order.
fn matching_codes(window: AnyWindowHandle, cx: &mut TestAppContext) -> Vec<String> {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        every_code()
            .into_iter()
            .filter(|code| {
                window
                    .try_find(format!("camelot-matching-{code}"))
                    .is_some()
            })
            .collect()
    })
    .unwrap()
}

fn every_code() -> Vec<String> {
    (1..=12)
        .flat_map(|number| [format!("{number}A"), format!("{number}B")])
        .collect()
}

fn press(window: AnyWindowHandle, key: &str, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press(key, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn move_toggle(relation: Relation) -> (&'static str, usize) {
    let index = Relation::ALL
        .iter()
        .position(|candidate| *candidate == relation)
        .unwrap();
    ("wheel-key-move", index)
}

#[gpui::test]
fn twelve_a_marks_eleven_a_one_a_and_twelve_b_as_matching(cx: &mut TestAppContext) {
    let (window, _) = open_twelve_a(cx);
    assert!(is_shown(window, "camelot-selected-12A", cx));
    assert_eq!(matching_codes(window, cx), ["1A", "11A", "12B"]);
}

#[gpui::test]
fn with_the_setting_off_the_wheel_marks_only_the_selected_key(cx: &mut TestAppContext) {
    let (window, view) = open_twelve_a(cx);
    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    click(window, "show-matching-keys", cx);
    assert!(!cx.update(|cx| view.read(cx).display.show_matching_keys));
    cx.update(|cx| view.update(cx, |app, cx| app.close_settings(cx)));
    assert!(matching_codes(window, cx).is_empty());
    assert!(is_shown(window, "camelot-selected-12A", cx));
    click(window, "camelot-wheel", cx);
    click(window, "wheel-show-matching-keys", cx);
    assert_eq!(matching_codes(window, cx), ["1A", "11A", "12B"]);
}

#[gpui::test]
fn a_move_toggled_on_the_wheel_changes_the_marks_and_the_next_list_filter(cx: &mut TestAppContext) {
    let (window, view) = open_twelve_a(cx);
    let wav = index_of_track(&view, "sine-440hz-stereo.wav", cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    let wav_key = cx.update(|cx| view.read(cx).session.library().tracks()[wav].key().unwrap());
    let mp3_key = cx.update(|cx| view.read(cx).session.library().tracks()[mp3].key().unwrap());
    assert_eq!(
        (wav_key.camelot(), mp3_key.camelot()),
        ("8B".into(), "9A".into())
    );

    click(window, "camelot-wheel", cx);
    assert!(is_shown(window, "wheel-menu", cx));
    click(window, move_toggle(Relation::Relative), cx);
    assert_eq!(matching_codes(window, cx), ["1A", "11A"]);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).decks.filter.relations,
            Some(vec![Relation::Same, Relation::Adjacent]),
            "the wheel changes the saved filter of the Next list"
        );
    });

    click(window, move_toggle(Relation::EnergyBoost), cx);
    assert_eq!(matching_codes(window, cx), ["1A", "2A", "11A"]);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).decks.filter.mix_options().relations,
            [Relation::Same, Relation::Adjacent, Relation::EnergyBoost]
        );
    });
    assert!(is_shown(window, "wheel-menu", cx), "toggling keeps it open");
}

#[gpui::test]
fn escape_and_a_click_outside_close_the_menu(cx: &mut TestAppContext) {
    let (window, view) = open_twelve_a(cx);
    click(window, "camelot-wheel", cx);
    assert!(is_shown(window, "wheel-menu", cx));
    press(window, "escape", cx);
    assert!(!is_shown(window, "wheel-menu", cx));
    assert!(cx.update(|cx| view.read(cx).wheel_menu_at.is_none()));

    click(window, "camelot-wheel", cx);
    assert!(is_shown(window, "wheel-menu", cx));
    click(window, track(0), cx);
    assert!(!is_shown(window, "wheel-menu", cx));
}

#[gpui::test]
fn the_menu_opened_at_the_right_edge_stays_inside_the_window(cx: &mut TestAppContext) {
    let (window, _) = open_twelve_a(cx);
    click(window, "camelot-wheel", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let menu = window.find("wheel-menu").bounds();
        let viewport = window.viewport_size();
        assert!(menu.right() <= viewport.width, "{menu:?}");
        assert!(menu.bottom() <= viewport.height, "{menu:?}");
    })
    .unwrap();
}
