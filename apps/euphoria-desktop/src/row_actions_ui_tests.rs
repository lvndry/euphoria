// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The icon buttons of a table row, used in a headless window with the
//! pointer and the keys a person would use.

use std::path::PathBuf;

use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::playback::testing::SilentOutput;
use crate::ui_tests::{click, deck_tracks, open_library, open_window_on, track};

fn playable_tracks() -> Vec<crate::demo::DemoTrack> {
    deck_tracks()
        .into_iter()
        .enumerate()
        .map(|(number, mut track)| {
            if let Ok(analysis) = &mut track.outcome {
                analysis.duration_seconds = 20.5;
                analysis.key = Some(euphoria_core::Key::new(9, euphoria_core::Mode::Minor));
                analysis.bpm = Some(120.0);
                analysis.energy = Some(5 + number as u8);
            }
            track
        })
        .collect()
}

fn open_playable(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_window_on(playable_tracks(), Box::new(SilentOutput), cx)
}

fn is_shown(
    window: AnyWindowHandle,
    id: impl Into<gpui::ElementId>,
    cx: &mut TestAppContext,
) -> bool {
    let id = id.into();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some_and(|element| element.visible())
    })
    .unwrap()
}

fn hover_row(window: AnyWindowHandle, index: usize, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.hover(track(index), cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn click_on_row(window: AnyWindowHandle, index: usize, id: &'static str, cx: &mut TestAppContext) {
    hover_row(window, index, cx);
    cx.update_window(window, |_, window, cx| window.click((id, index), cx))
        .unwrap();
    cx.run_until_parked();
}

fn held_track(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Option<PathBuf> {
    cx.update(|cx| {
        view.read(cx)
            .player
            .track()
            .map(std::path::Path::to_path_buf)
    })
}

fn is_sounding(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> bool {
    cx.update(|cx| view.read(cx).player.offers_pause())
}

fn path_of(view: &Entity<EuphoriaApp>, index: usize, cx: &mut TestAppContext) -> PathBuf {
    cx.update(|cx| {
        view.read(cx)
            .session
            .library()
            .track(index)
            .unwrap()
            .path
            .clone()
    })
}

fn starred(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<PathBuf> {
    cx.update(|cx| view.read(cx).session.favorites().tracks().to_vec())
}

fn press(window: AnyWindowHandle, key: &str, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press(key, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui::test]
fn the_buttons_show_on_hover_and_on_the_selected_row_and_nowhere_else(cx: &mut TestAppContext) {
    let (window, view) = open_library(10, cx);
    cx.update(|cx| assert_eq!(view.read(cx).selected, Some(0)));
    assert!(
        is_shown(window, ("row-actions", 0usize), cx),
        "the selected row"
    );
    assert!(!is_shown(window, ("row-actions", 4usize), cx));
    assert!(!is_shown(window, ("row-play", 4usize), cx));

    hover_row(window, 4, cx);
    assert!(is_shown(window, ("row-actions", 4usize), cx), "on hover");
    assert!(is_shown(window, ("row-play", 4usize), cx));
    assert!(is_shown(window, ("star", 4usize), cx));
    assert!(is_shown(window, ("row-add", 4usize), cx));

    hover_row(window, 6, cx);
    assert!(
        !is_shown(window, ("row-actions", 4usize), cx),
        "pointer left"
    );
    assert!(is_shown(window, ("row-actions", 6usize), cx));
    assert!(
        is_shown(window, ("row-actions", 0usize), cx),
        "still selected"
    );

    click(window, track(2), cx);
    assert!(is_shown(window, ("row-actions", 2usize), cx));
}

#[gpui::test]
fn showing_the_buttons_does_not_move_the_title(cx: &mut TestAppContext) {
    let (window, _view) = open_library(10, cx);
    let bounds = |cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.find(("title-cell", 4usize)).bounds().origin
        })
        .unwrap()
    };
    let before = bounds(cx);
    hover_row(window, 4, cx);
    assert_eq!(before, bounds(cx), "the title starts where it did");
}

#[gpui::test]
fn a_row_is_starred_from_its_icon_and_the_star_stays_while_not_hovered(cx: &mut TestAppContext) {
    let (window, view) = open_library(10, cx);
    click_on_row(window, 5, "star", cx);
    assert_eq!(starred(&view, cx), [path_of(&view, 5, cx)]);
    hover_row(window, 7, cx);
    assert!(
        !is_shown(window, ("row-actions", 5usize), cx),
        "the buttons go with the pointer; the star mark stays in the row"
    );
    click_on_row(window, 5, "star", cx);
    assert!(starred(&view, cx).is_empty());
}

#[gpui::test]
fn the_s_key_stars_the_selected_row_like_the_f_key(cx: &mut TestAppContext) {
    let (window, view) = open_library(10, cx);
    click(window, track(3), cx);
    press(window, "s", cx);
    assert_eq!(starred(&view, cx), [path_of(&view, 3, cx)]);
    press(window, "f", cx);
    assert!(starred(&view, cx).is_empty());
}

#[gpui::test]
fn a_row_plays_from_its_icon_and_the_space_bar_pauses_it(cx: &mut TestAppContext) {
    let (window, view) = open_playable(cx);
    click_on_row(window, 1, "row-play", cx);
    assert_eq!(held_track(&view, cx), Some(path_of(&view, 1, cx)));
    assert!(is_sounding(&view, cx));
    cx.update(|cx| assert_eq!(view.read(cx).selected, Some(1)));

    press(window, "space", cx);
    assert!(!is_sounding(&view, cx), "Space pauses the row's track");
    press(window, "space", cx);
    assert!(is_sounding(&view, cx), "and plays it again");

    click_on_row(window, 1, "row-play", cx);
    assert!(
        !is_sounding(&view, cx),
        "the icon of the sounding row pauses"
    );

    click_on_row(window, 2, "row-play", cx);
    assert_eq!(held_track(&view, cx), Some(path_of(&view, 2, cx)));
    assert!(is_sounding(&view, cx));
}

fn open_with_a_set_of_two(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let (window, view) = open_playable(cx);
    click(window, "set-builder-entry", cx);
    click(window, "set-shorter", cx);
    click(window, "build-set", cx);
    click(window, "all-tracks", cx);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx)
                .set_builder
                .planned
                .as_ref()
                .unwrap()
                .paths
                .len(),
            2
        )
    });
    (window, view)
}

fn set_paths(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<PathBuf> {
    cx.update(|cx| {
        view.read(cx)
            .set_builder
            .planned
            .as_ref()
            .unwrap()
            .paths
            .clone()
    })
}

fn the_track_outside_the_set(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> usize {
    let in_set = set_paths(view, cx);
    (0..3)
        .find(|index| !in_set.contains(&path_of(view, *index, cx)))
        .unwrap()
}

#[gpui::test]
fn a_row_is_added_to_the_set_from_its_icon(cx: &mut TestAppContext) {
    let (window, view) = open_with_a_set_of_two(cx);
    let outside = the_track_outside_the_set(&view, cx);
    click_on_row(window, outside, "row-add", cx);
    let paths = set_paths(&view, cx);
    assert_eq!(paths.len(), 3);
    assert_eq!(paths[2], path_of(&view, outside, cx), "at the end");
    cx.update(|cx| {
        let planned = view.read(cx).set_builder.planned.as_ref().unwrap();
        assert!(planned.locked.iter().all(|locked| !locked));
        assert_eq!(planned.plan.tracks.len(), 3, "the set is explained again");
    });

    click_on_row(window, outside, "row-add", cx);
    assert_eq!(set_paths(&view, cx).len(), 3, "a track joins a set once");
}

#[gpui::test]
fn the_a_key_adds_the_selected_row_to_the_set(cx: &mut TestAppContext) {
    let (window, view) = open_with_a_set_of_two(cx);
    let outside = the_track_outside_the_set(&view, cx);
    click(window, track(outside), cx);
    press(window, "a", cx);
    assert_eq!(set_paths(&view, cx).len(), 3);
    assert_eq!(
        *set_paths(&view, cx).last().unwrap(),
        path_of(&view, outside, cx)
    );
}

#[gpui::test]
fn adding_without_a_set_says_how_to_get_one(cx: &mut TestAppContext) {
    let (window, view) = open_playable(cx);
    click_on_row(window, 1, "row-add", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.set_builder.planned.is_none());
        assert!(
            app.notice
                .iter()
                .any(|notice| notice.contains("no set to add to"))
        );
    });
}

#[gpui::test]
fn the_letter_keys_type_in_the_search_field_and_do_nothing_else(cx: &mut TestAppContext) {
    let (window, view) = open_with_a_set_of_two(cx);
    let outside = the_track_outside_the_set(&view, cx);
    click(window, track(outside), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("search", cx);
        for key in ["s", "a", "f", "m", "space"] {
            window.press(key, cx);
        }
    })
    .unwrap();
    cx.run_until_parked();
    assert!(starred(&view, cx).is_empty(), "S and F type");
    assert_eq!(set_paths(&view, cx).len(), 2, "A types");
    assert!(!is_sounding(&view, cx), "Space types");
    cx.update(|cx| assert!(!view.read(cx).metronome.is_running(), "M types"));
}

#[gpui::test]
fn a_long_title_ends_before_the_buttons_instead_of_running_under_them(cx: &mut TestAppContext) {
    let (window, view) = open_library(10, cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.column_widths
                .set(crate::columns::ColumnId::Title, 150.0);
            cx.notify();
        })
    });
    let edges = |cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let title = window.find(("title-cell", 4usize)).bounds();
            let actions = window.find(("row-actions", 4usize)).bounds();
            (title, actions)
        })
        .unwrap()
    };
    let (resting_title, _) = edges(cx);
    hover_row(window, 4, cx);
    let (shown_title, actions) = edges(cx);
    assert!(
        shown_title.right() <= actions.left(),
        "the title ends before the buttons: {shown_title:?} {actions:?}"
    );
    assert_eq!(
        shown_title.left(),
        resting_title.left(),
        "the title does not move"
    );
    assert!(
        resting_title.right() > shown_title.right(),
        "without hover the title uses the full width"
    );
}
