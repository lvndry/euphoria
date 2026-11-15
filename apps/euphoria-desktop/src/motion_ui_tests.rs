// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The motion of dialogs, strips, the focus view and the theme in the real
//! root view. The headless harness reduces motion, so surfaces come in at
//! once; the animated path is checked with a clock moved by hand.

use std::time::Duration;

use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, TestAppContext};

use crate::motion::testing::FakeClock;
use crate::motion::{self, Transition};
use crate::surface_motion::{Modal, ModalFades, Presence, Strip, StripGrowth};
use crate::theme::ThemeName;
use crate::ui_tests::{click, is_shown, open_fixture_library};
use crate::views::sidebar::{RowCount, favorites_count, needs_review_count};

fn render(window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
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
fn settings_open_and_close_at_once_under_reduce_motion(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    render(window, cx);
    assert!(is_shown(window, "settings-panel", cx));
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.modal_fades.value(Modal::Settings), 1.0);
        assert!(!app.modal_fades.is_moving());
    });
    cx.update(|cx| view.update(cx, |app, cx| app.close_settings(cx)));
    cx.update(|cx| assert!(!view.read(cx).settings_open, "closed in the same frame"));
    assert!(!is_shown(window, "settings-panel", cx));
}

#[gpui::test]
fn the_export_sheet_and_tag_panel_open_at_once_under_reduce_motion(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    click(window, "write-tags", cx);
    assert!(is_shown(window, "tag-panel", cx));
    cx.update(|cx| assert_eq!(view.read(cx).modal_fades.value(Modal::TagPanel), 1.0));
    press(window, "escape", cx);
    assert!(!is_shown(window, "tag-panel", cx));

    cx.update(|cx| view.update(cx, |app, cx| app.open_library_export(cx)));
    render(window, cx);
    assert!(is_shown(window, "library-export", cx));
    cx.update(|cx| assert_eq!(view.read(cx).modal_fades.value(Modal::Export), 1.0));
}

#[gpui::test]
fn a_dialog_fades_in_and_out_on_the_clock_and_ends_closed(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    let clock = FakeClock::new();
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.modal_fades = ModalFades::with_clock(&clock)
        });
    });
    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    render(window, cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.modal_fades.value(Modal::Settings), 0.0);
        assert!(app.modal_fades.is_moving());
    });
    assert!(
        is_shown(window, "settings-panel", cx),
        "present from the first frame"
    );

    clock.at(motion::PANEL.as_secs_f32());
    render(window, cx);
    cx.update(|cx| assert_eq!(view.read(cx).modal_fades.value(Modal::Settings), 1.0));

    cx.update(|cx| view.update(cx, |app, cx| app.close_settings(cx)));
    cx.update(|cx| assert!(view.read(cx).settings_open, "still on screen, fading"));
    clock.at(motion::PANEL.as_secs_f32() + 0.06);
    render(window, cx);
    cx.update(|cx| {
        let value = view.read(cx).modal_fades.value(Modal::Settings);
        assert!(value > 0.0 && value < 1.0, "{value}");
    });
    cx.executor()
        .advance_clock(motion::FAST + Duration::from_millis(5));
    cx.run_until_parked();
    cx.update(|cx| assert!(!view.read(cx).settings_open));
    render(window, cx);
    assert!(!is_shown(window, "settings-panel", cx));
}

#[gpui::test]
fn a_notice_strip_is_at_full_height_at_once_under_reduce_motion(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.notice = Some("Something went wrong".to_string());
            cx.notify();
        })
    });
    render(window, cx);
    render(window, cx);
    let full = cx
        .update_window(window, |_, window, _| {
            window
                .find(("strip-clip", Strip::Notice.index()))
                .bounds()
                .size
                .height
        })
        .unwrap();
    assert!(full > gpui::px(0.0));
    cx.update(|cx| assert_eq!(view.read(cx).strip_growth.value(Strip::Notice), 1.0));
}

#[gpui::test]
fn a_notice_strip_grows_to_its_height_on_the_clock(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    let clock = FakeClock::new();
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.strip_growth = StripGrowth::with_clock(&clock)
        });
    });
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.notice = Some("Something went wrong".to_string());
            cx.notify();
        })
    });
    render(window, cx);
    let height_at = |seconds: f32, cx: &mut TestAppContext| {
        clock.at(seconds);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window
                .find(("strip-clip", Strip::Notice.index()))
                .bounds()
                .size
                .height
        })
        .unwrap()
    };
    let start = height_at(0.0, cx);
    let middle = height_at(0.05, cx);
    let end = height_at(motion::LAYOUT.as_secs_f32(), cx);
    assert!(
        start < middle && middle < end,
        "{start:?} {middle:?} {end:?}"
    );
    cx.update(|cx| assert_eq!(view.read(cx).strip_growth.value(Strip::Notice), 1.0));
}

#[gpui::test]
fn the_focus_view_is_in_at_once_under_reduce_motion_and_out_again(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    press(window, "enter", cx);
    render(window, cx);
    assert!(is_shown(window, "focus-view", cx));
    assert!(!is_shown(window, "all-tracks", cx), "the sidebar is away");
    press(window, "escape", cx);
    render(window, cx);
    assert!(!is_shown(window, "focus-view", cx));
    assert!(is_shown(window, "all-tracks", cx));
    cx.update(|cx| assert!(!view.read(cx).focus_fade.is_shown_or_moving()));
}

#[gpui::test]
fn the_focus_view_crossfades_with_the_library_on_the_clock(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    let clock = FakeClock::new();
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.focus_fade = Presence::at_rest(false).with_sources(clock.sources());
        })
    });
    press(window, "enter", cx);
    render(window, cx);
    assert!(is_shown(window, "focus-view", cx));
    assert!(
        is_shown(window, "all-tracks", cx),
        "the library shows through"
    );

    clock.at(motion::PANEL.as_secs_f32());
    render(window, cx);
    render(window, cx);
    assert!(is_shown(window, "focus-view", cx));
    assert!(!is_shown(window, "all-tracks", cx), "the crossfade is over");

    press(window, "escape", cx);
    render(window, cx);
    assert!(is_shown(window, "focus-view", cx), "fading out");
    assert!(is_shown(window, "all-tracks", cx));
    clock.at(motion::PANEL.as_secs_f32() * 2.0 + 0.01);
    render(window, cx);
    render(window, cx);
    assert!(!is_shown(window, "focus-view", cx));
}

#[gpui::test]
fn a_theme_switch_applies_at_once_under_reduce_motion(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update(|cx| view.update(cx, |app, cx| app.set_theme(ThemeName::Paper, cx)));
    render(window, cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.theme_fade.is_none());
        assert_eq!(
            app.theme.palette,
            crate::theme::AppTheme::named(ThemeName::Paper).palette
        );
    });
}

#[gpui::test]
fn a_theme_switch_blends_and_settles_on_the_new_palette(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    let clock = FakeClock::new();
    let slate = crate::theme::AppTheme::named(ThemeName::Slate).palette;
    let paper = crate::theme::AppTheme::named(ThemeName::Paper).palette;
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.set_theme(ThemeName::Paper, cx);
            app.theme.palette = slate;
            app.theme_fade = crate::surface_motion::ThemeFade::between_with(
                slate,
                paper,
                Transition::at_rest(false).with_sources(clock.sources()),
            );
            assert!(app.theme_fade.is_some());
        })
    });
    clock.at(0.08);
    render(window, cx);
    cx.update(|cx| {
        let background = view.read(cx).theme.palette.background;
        assert_ne!(background, slate.background);
        assert_ne!(background, paper.background);
    });
    clock.at(motion::THEME.as_secs_f32() + 0.01);
    render(window, cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.theme.palette, paper);
        assert!(app.theme_fade.is_none());
    });
}

#[gpui::test]
fn counts_that_would_jump_show_an_ellipsis_while_analysis_runs(cx: &mut TestAppContext) {
    let (_window, view) = open_fixture_library(cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            assert!(matches!(needs_review_count(app), RowCount::Settled(_)));
            assert!(!favorites_count(app).text().ends_with('\u{2026}'));
            app.session.pretend_queued(5);
            assert!(needs_review_count(app).text().ends_with('\u{2026}'));
            assert!(favorites_count(app).text().ends_with('\u{2026}'));
            for _ in 0..5 {
                app.session.pretend_finished();
            }
            assert!(!needs_review_count(app).text().ends_with('\u{2026}'));
            assert!(!favorites_count(app).text().ends_with('\u{2026}'));
        })
    });
}

#[gpui::test]
fn the_toolbar_says_how_long_analysis_has_left(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.session.pretend_queued(100);
            app.session.pretend_finished();
            cx.notify();
        })
    });
    render(window, cx);
    assert!(
        !is_shown(window, "analysis-time-left", cx),
        "too early to say"
    );
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.session.pretend_started_seconds_ago(60);
            cx.notify();
        })
    });
    render(window, cx);
    assert!(is_shown(window, "analysis-time-left", cx));
    cx.update(|cx| {
        let label = view.read(cx).session.progress().time_left_label();
        assert!(label.unwrap().contains("left"));
    });
}
