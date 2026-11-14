// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests that open the focus view in the real root view, in a headless
//! window, with clicks, keys and the pointer as a person would.

use gpui::test::{ClickOptions, TestEventExt, TestWindowExt};
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext, point, px};

use crate::app::EuphoriaApp;
use crate::demo;
use crate::focus::Lane;
use crate::playback::testing::SilentOutput;
use crate::settings::Settings;
use crate::tag_writing::testing::ScratchFolder;
use crate::ui_tests::{
    click, cued_tracks, is_shown, open_cued_library, open_library, open_window_with, selected_cues,
    track, write_stems_with_vocals_for,
};

fn is_open(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> bool {
    cx.update(|cx| view.read(cx).focus.is_open)
}

fn selected(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Option<usize> {
    cx.update(|cx| view.read(cx).selected)
}

fn press(window: AnyWindowHandle, key: &str, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press(key, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn settle(window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn open_focus(window: AnyWindowHandle, cx: &mut TestAppContext) {
    click(window, "focus-open", cx);
    settle(window, cx);
}

fn move_pointer_over(
    window: AnyWindowHandle,
    id: &'static str,
    fraction: f32,
    cx: &mut TestAppContext,
) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let bounds = window.find(id).bounds();
        let target = point(
            bounds.left() + bounds.size.width * fraction,
            bounds.top() + bounds.size.height / 2.0,
        );
        window.pointer_move(target, None, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn readout(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Option<String> {
    cx.update(|cx| view.read(cx).focus_readout())
}

#[gpui::test]
fn the_focus_button_opens_the_view_and_the_library_button_returns_with_the_selection(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_library(10, cx);
    click(window, track(3), cx);
    assert!(!is_open(&view, cx));

    open_focus(window, cx);
    assert!(is_open(&view, cx));
    assert!(is_shown(window, "focus-view", cx));
    assert!(!is_shown(window, track(3), cx), "the table is replaced");
    assert!(!is_shown(window, "focus-open", cx), "so is the inspector");

    click(window, "focus-back", cx);
    assert!(!is_open(&view, cx));
    assert_eq!(selected(&view, cx), Some(3));
    assert!(is_shown(window, track(3), cx));
}

#[gpui::test]
fn the_focus_view_shows_the_wheel_of_the_selected_tracks_key_in_every_theme(
    cx: &mut TestAppContext,
) {
    for theme in crate::theme::ThemeName::ALL {
        let (window, _) = open_window_with(
            demo::tracks(10),
            Box::new(SilentOutput),
            Settings {
                theme,
                ..Settings::default()
            },
            None,
            gpui::size(px(1280.0), px(800.0)),
            cx,
        );
        click(window, track(3), cx);
        open_focus(window, cx);
        cx.update_window(window, |_, window, _| {
            let focus = window.find("focus-view").bounds();
            let wheel = window.find("camelot-wheel").bounds();
            assert!(focus.contains(&wheel.center()), "{theme:?}");
            assert_eq!(wheel.size.width, wheel.size.height, "{theme:?}");
            assert!(wheel.size.width >= px(96.0), "{theme:?}");
        })
        .unwrap();
    }
}

#[gpui::test]
fn a_double_click_on_a_row_opens_the_view_on_that_track(cx: &mut TestAppContext) {
    let (window, view) = open_library(10, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.double_click(track(2), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(is_open(&view, cx));
    assert_eq!(selected(&view, cx), Some(2));
}

#[gpui::test]
fn enter_opens_the_view_escape_closes_it_and_f_still_stars(cx: &mut TestAppContext) {
    let (window, view) = open_library(10, cx);
    click(window, track(4), cx);
    press(window, "enter", cx);
    assert!(is_open(&view, cx));

    press(window, "f", cx);
    assert!(is_open(&view, cx), "F stars and does not leave the view");
    let starred = cx.update(|cx| view.read(cx).session.favorites().tracks().len());
    assert_eq!(starred, 1);

    press(window, "escape", cx);
    assert!(!is_open(&view, cx));
    assert_eq!(selected(&view, cx), Some(4));
}

#[gpui::test]
fn enter_leaves_the_set_builder_and_open_settings_alone(cx: &mut TestAppContext) {
    let (window, view) = open_library(10, cx);
    click(window, track(4), cx);
    view.update(cx, |app, cx| {
        app.set_builder.is_open = true;
        cx.notify();
    });
    press(window, "enter", cx);
    assert!(!is_open(&view, cx), "Enter does not cover the set builder");

    view.update(cx, |app, cx| {
        app.set_builder.is_open = false;
        app.settings_open = true;
        cx.notify();
    });
    press(window, "enter", cx);
    assert!(
        !is_open(&view, cx),
        "Enter does not open the view behind settings"
    );
}

#[gpui::test]
fn escape_closes_the_lanes_menu_before_the_view(cx: &mut TestAppContext) {
    let (window, view) = open_library(10, cx);
    click(window, track(4), cx);
    press(window, "enter", cx);
    view.update(cx, |app, cx| {
        app.focus.lanes_menu_open = true;
        cx.notify();
    });
    press(window, "escape", cx);
    assert!(is_open(&view, cx), "the first Escape closes the menu only");
    assert!(!view.read_with(cx, |app, _| app.focus.lanes_menu_open));
    press(window, "escape", cx);
    assert!(!is_open(&view, cx));
}

#[gpui::test]
fn every_lane_is_drawn_on_one_axis_with_the_sections_of_the_cues(cx: &mut TestAppContext) {
    let (window, _view) = open_cued_library(cx);
    open_focus(window, cx);
    for id in [
        "focus-overview",
        "focus-sections",
        "focus-detail",
        "focus-energy-slot",
        "focus-vocals-slot",
        "focus-stems-separate",
        "focus-chip-key",
        "focus-chip-bpm",
        "focus-chip-energy",
        "focus-chip-grid",
        "focus-transition-card",
    ] {
        assert!(is_shown(window, id, cx), "{id}");
    }
    assert!(is_shown(window, ("focus-section", 0_usize), cx));
    assert!(is_shown(window, ("focus-section", 1_usize), cx));
    assert!(!is_shown(window, ("focus-section", 2_usize), cx));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let sections = window.find("focus-sections").bounds();
        let detail = window.find("focus-detail").bounds();
        let slot = window.find("focus-energy-slot").bounds();
        assert_eq!(sections.left(), detail.left());
        assert_eq!(sections.size.width, detail.size.width);
        assert_eq!(slot.left(), detail.left());
        assert!(sections.bottom() <= detail.top());
        assert!(detail.bottom() <= slot.top());
    })
    .unwrap();
}

#[gpui::test]
fn the_cursor_reads_out_the_section_and_the_stems_under_it_across_every_lane(
    cx: &mut TestAppContext,
) {
    let scratch = ScratchFolder::new("focus-readout");
    let (window, view) = open_cued_library(cx);
    let files = write_stems_with_vocals_for(&scratch, 16, 6);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let path = app.selected_track().unwrap().path.clone();
            app.stems.mark_ready(&path, files.clone());
            app.load_focus_stems(cx);
        })
    });
    cx.run_until_parked();
    open_focus(window, cx);
    assert!(is_shown(window, "focus-stems", cx));
    assert!(is_shown(window, ("focus-stem-row", 3_usize), cx));
    assert!(!is_shown(window, "focus-stems-separate", cx));
    assert!(!is_shown(window, "focus-cursor", cx));

    move_pointer_over(window, "focus-detail", 0.15, cx);
    assert_eq!(
        readout(&view, cx).as_deref(),
        Some("0:04 \u{b7} Intro \u{b7} vocals \u{b7} drums out \u{b7} bass out \u{b7} other out")
    );
    assert!(is_shown(window, "focus-cursor", cx));

    move_pointer_over(window, "focus-sections", 0.67, cx);
    assert_eq!(readout(&view, cx).as_deref(), Some("0:20 \u{b7} Drop"));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let cursor = window.find("focus-cursor").bounds();
        let detail = window.find("focus-detail").bounds();
        let stems = window.find("focus-stems").bounds();
        assert!(cursor.top() <= detail.top() && cursor.bottom() >= stems.bottom());
    })
    .unwrap();

    cx.update_window(window, |_, window, cx| {
        window.pointer_move(point(px(2.0), px(2.0)), None, cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(readout(&view, cx), None);
}

#[gpui::test]
fn the_readout_omits_the_lanes_that_have_no_data(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    open_focus(window, cx);
    move_pointer_over(window, "focus-detail", 0.15, cx);
    assert_eq!(readout(&view, cx).as_deref(), Some("0:04 \u{b7} Intro"));
}

#[gpui::test]
fn hiding_a_lane_removes_it_and_the_choice_is_kept_for_the_next_launch(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("focus-lanes-setting");
    let settings_path = scratch.0.join("settings.json");
    let (window, view) = open_window_with(
        cued_tracks(),
        Box::new(SilentOutput),
        Settings::default(),
        Some(settings_path.clone()),
        gpui::size(px(1440.0), px(900.0)),
        cx,
    );
    open_focus(window, cx);
    assert!(is_shown(window, "focus-stems", cx) || is_shown(window, "focus-stems-separate", cx));

    click(window, "focus-lanes", cx);
    click(window, ("focus-lane-toggle", Lane::Stems.number()), cx);
    settle(window, cx);
    assert!(!is_shown(window, "focus-stems", cx));
    assert!(!is_shown(window, "focus-stems-separate", cx));
    assert!(is_shown(window, "focus-detail", cx), "the others stay");
    cx.update(|cx| assert!(!view.read(cx).focus.lanes.stems));

    let kept = Settings::load(&settings_path).unwrap();
    assert!(!kept.focus_lanes.stems);
    assert!(kept.focus_lanes.detail);

    let (window, _view) = open_window_with(
        cued_tracks(),
        Box::new(SilentOutput),
        kept,
        None,
        gpui::size(px(1440.0), px(900.0)),
        cx,
    );
    open_focus(window, cx);
    assert!(is_shown(window, "focus-detail", cx));
    assert!(!is_shown(window, "focus-stems", cx));
    assert!(!is_shown(window, "focus-stems-separate", cx));
}

#[gpui::test]
fn the_mixes_into_list_shows_what_the_inspector_lists_in_the_same_order(cx: &mut TestAppContext) {
    let (window, view) = open_library(40, cx);
    let mut chosen = None;
    for index in 0..40 {
        click(window, track(index), cx);
        if cx.update(|cx| !view.read(cx).mixes.is_empty()) {
            chosen = Some(index);
            break;
        }
    }
    assert!(chosen.is_some(), "some demo track has matches");
    let mixes: Vec<usize> = cx.update(|cx| {
        view.read(cx)
            .mixes
            .iter()
            .map(|mix| mix.track_index)
            .collect()
    });

    let order = |prefix: &'static str, window: AnyWindowHandle, cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let mut found: Vec<(usize, gpui::Pixels)> = mixes
                .iter()
                .filter_map(|index| {
                    window
                        .try_find((prefix, *index))
                        .map(|element| (*index, element.bounds().top()))
                })
                .collect();
            found.sort_by(|first, second| first.1.partial_cmp(&second.1).unwrap());
            found
                .into_iter()
                .map(|(index, _)| index)
                .collect::<Vec<_>>()
        })
        .unwrap()
    };
    open_focus(window, cx);
    let in_focus = order("focus-mix", window, cx);
    assert_eq!(in_focus, mixes, "the list is app.mixes, in order");
    assert!(is_shown(window, ("focus-mix-clash", mixes[0]), cx));

    click(window, ("focus-mix-star", mixes[0]), cx);
    cx.update(|cx| {
        let app = view.read(cx);
        let from = &app.selected_track().unwrap().path;
        let to = &app.session.library().track(mixes[0]).unwrap().path;
        assert!(app.session.favorites().transition(from, to).is_some());
    });

    click(window, ("focus-mix", mixes[1]), cx);
    cx.update(|cx| assert_eq!(view.read(cx).focus.selected_match, Some(mixes[1])));
}

#[gpui::test]
fn dragging_a_cue_flag_in_the_detail_moves_the_cue_onto_a_beat(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    open_focus(window, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let pressed_at = window
            .find(crate::views::cues::marker_id(1))
            .bounds()
            .center();
        let detail = window.find("focus-detail").bounds();
        let released_at = point(detail.left() + detail.size.width * 0.3, pressed_at.y);
        window.drag(pressed_at, released_at, cx);
    })
    .unwrap();
    cx.run_until_parked();
    let (cues, is_edited) = selected_cues(&view, cx);
    assert!(is_edited);
    assert_eq!(cues[1], (9.0, "Drop".to_string(), Some('B')));
    assert_eq!(cues[0].0, 0.0);
    assert!(is_open(&view, cx));
}

#[gpui::test]
fn the_overview_moves_the_window_and_the_zoom_buttons_resize_it(cx: &mut TestAppContext) {
    let (window, view) = open_library(5, cx);
    open_focus(window, cx);
    let duration = cx.update(|cx| {
        f64::from(
            view.read(cx)
                .selected_track()
                .unwrap()
                .duration_seconds()
                .unwrap(),
        )
    });
    let span_before = cx.update(|cx| view.read(cx).focus.viewport.span);
    assert!(span_before < duration);

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let overview = window.find("focus-overview").bounds();
        window.click_with_options(
            "focus-overview",
            ClickOptions::new().with_offset(point(overview.size.width * 0.97, px(10.0))),
            cx,
        );
    })
    .unwrap();
    cx.update(|cx| {
        let viewport = view.read(cx).focus.viewport;
        assert!((viewport.end() - duration).abs() < 1e-6, "{viewport:?}");
        assert_eq!(viewport.span, span_before);
    });

    click(window, "focus-zoom-in", cx);
    let zoomed = cx.update(|cx| view.read(cx).focus.viewport.span);
    assert!(zoomed < span_before);
    click(window, "focus-zoom-out", cx);
    click(window, "focus-zoom-out", cx);
    click(window, "focus-zoom-out", cx);
    cx.update(|cx| assert_eq!(view.read(cx).focus.viewport.span, duration));
}

#[gpui::test]
fn the_plan_tabs_show_the_transition_card_the_mashup_link_and_the_cues(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    open_focus(window, cx);
    assert!(is_shown(window, "focus-transition-card", cx));

    click(window, ("focus-plan-tab", 2_usize), cx);
    assert!(is_shown(window, ("cue-row", 0_usize), cx));
    assert!(!is_shown(window, "focus-transition-card", cx));

    click(window, ("focus-plan-tab", 1_usize), cx);
    assert!(is_shown(window, "focus-open-mashup", cx));
    click(window, "focus-open-mashup", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(!app.focus.is_open);
        assert!(app.decks.is_open);
        assert_eq!(app.decks.mode, crate::decks::DeckMode::Mashup);
    });
}
