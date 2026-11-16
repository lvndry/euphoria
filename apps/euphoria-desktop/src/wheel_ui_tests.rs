// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Camelot wheel's matching keys, its menu of key moves and the same
//! moves in the settings, used through the real root view in a headless
//! window.
//!
//! The library is the decks' three fixtures with the FLAC moved to 12A, so
//! the matching keys wrap round the top of the wheel: 11A and 1A one step
//! either way and 12B the relative major.

use euphoria_core::harmony::{MixOptions, Relation};
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

fn index_of(relation: Relation) -> usize {
    Relation::ALL
        .iter()
        .position(|candidate| *candidate == relation)
        .unwrap()
}

fn move_toggle(relation: Relation) -> (&'static str, usize) {
    ("wheel-key-move", index_of(relation))
}

fn settings_toggle(relation: Relation) -> (&'static str, usize) {
    ("key-move", index_of(relation))
}

fn open_key_moves_setting(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) {
    cx.update(|cx| view.update(cx, |app, cx| app.open_key_moves_setting(cx)));
    cx.run_until_parked();
}

fn counted(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<Relation> {
    cx.update(|cx| view.read(cx).decks.filter.mix_options().relations)
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

#[gpui::test]
fn a_move_toggled_in_the_settings_changes_the_marks_and_the_next_list_filter(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_twelve_a(cx);
    open_key_moves_setting(&view, cx);
    click(window, settings_toggle(Relation::Relative), cx);
    assert_eq!(matching_codes(window, cx), ["1A", "11A"]);
    assert_eq!(counted(&view, cx), [Relation::Same, Relation::Adjacent]);

    click(window, settings_toggle(Relation::EnergyBoost), cx);
    assert_eq!(matching_codes(window, cx), ["1A", "2A", "11A"]);
    assert_eq!(
        counted(&view, cx),
        [Relation::Same, Relation::Adjacent, Relation::EnergyBoost]
    );

    cx.update(|cx| view.update(cx, |app, cx| app.close_settings(cx)));
    click(window, "camelot-wheel", cx);
    click(window, move_toggle(Relation::EnergyBoost), cx);
    cx.update(|cx| view.update(cx, |app, cx| app.close_wheel_menu(cx)));
    open_key_moves_setting(&view, cx);
    assert_eq!(counted(&view, cx), [Relation::Same, Relation::Adjacent]);
    click(window, settings_toggle(Relation::EnergyBoost), cx);
    assert_eq!(
        counted(&view, cx),
        [Relation::Same, Relation::Adjacent, Relation::EnergyBoost],
        "the settings and the wheel menu change one filter"
    );
}

#[gpui::test]
fn reset_brings_back_the_default_moves_from_the_settings_and_the_menu(cx: &mut TestAppContext) {
    let (window, view) = open_twelve_a(cx);
    open_key_moves_setting(&view, cx);
    click(window, settings_toggle(Relation::Relative), cx);
    click(window, settings_toggle(Relation::MoodChange), cx);
    click(window, "reset-key-moves", cx);
    assert!(cx.update(|cx| view.read(cx).decks.filter.relations.is_none()));
    assert_eq!(counted(&view, cx), MixOptions::default().relations);
    assert_eq!(matching_codes(window, cx), ["1A", "11A", "12B"]);

    cx.update(|cx| view.update(cx, |app, cx| app.close_settings(cx)));
    click(window, "camelot-wheel", cx);
    click(window, move_toggle(Relation::Adjacent), cx);
    assert_eq!(matching_codes(window, cx), ["12B"]);
    click(window, "wheel-reset-key-moves", cx);
    assert_eq!(counted(&view, cx), MixOptions::default().relations);
    assert_eq!(matching_codes(window, cx), ["1A", "11A", "12B"]);
}

#[gpui::test]
fn a_line_under_the_wheel_says_when_only_same_key_is_ticked(cx: &mut TestAppContext) {
    let (window, view) = open_twelve_a(cx);
    assert!(!is_shown(window, "wheel-moves-hint", cx));
    click(window, "camelot-wheel", cx);
    click(window, move_toggle(Relation::Adjacent), cx);
    click(window, move_toggle(Relation::Relative), cx);
    assert_eq!(counted(&view, cx), [Relation::Same]);
    assert!(matching_codes(window, cx).is_empty());
    assert!(is_shown(window, "wheel-moves-hint", cx));

    press(window, "escape", cx);
    click(window, "wheel-moves-hint", cx);
    assert!(
        is_shown(window, "wheel-menu", cx),
        "the line opens the moves"
    );
    click(window, move_toggle(Relation::Adjacent), cx);
    assert!(!is_shown(window, "wheel-moves-hint", cx));
    assert_eq!(matching_codes(window, cx), ["1A", "11A"]);
}

#[gpui::test]
fn the_line_stays_away_while_the_wheel_marks_nothing(cx: &mut TestAppContext) {
    let (window, view) = open_twelve_a(cx);
    click(window, "camelot-wheel", cx);
    click(window, move_toggle(Relation::Adjacent), cx);
    click(window, move_toggle(Relation::Relative), cx);
    click(window, "wheel-show-matching-keys", cx);
    assert!(!cx.update(|cx| view.read(cx).display.show_matching_keys));
    assert!(!is_shown(window, "wheel-moves-hint", cx));
}

#[gpui::test]
fn all_key_settings_opens_the_settings_at_the_key_moves(cx: &mut TestAppContext) {
    let (window, view) = open_twelve_a(cx);
    click(window, "camelot-wheel", cx);
    click(window, "wheel-all-key-settings", cx);
    assert!(cx.update(|cx| view.read(cx).settings_open));
    assert!(!is_shown(window, "wheel-menu", cx));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let body = window.find("settings-body").bounds();
        let group = window.find("key-moves").bounds();
        assert!(
            (group.top() - body.top()).abs() < gpui::px(1.0),
            "{group:?} at the top of {body:?}"
        );
    })
    .unwrap();
}
