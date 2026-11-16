// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of loops in the focus view, in the real root view in a headless
//! window: the loop buttons and keys, a loop drawn on the waveform and
//! moved by its ends, leaving a loop, and saving one to play again.

use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Entity, Modifiers, TestAppContext, point};

use crate::app::EuphoriaApp;
use crate::focus::PlanTab;
use crate::ui_tests::{click, is_shown, open_cued_library};
use crate::views::loops::{length_id, saved_loop_id};

/// A loop's start and end, in seconds.
type Region = (f64, f64);

fn settle(window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    })
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

/// The cued library with its track playing in the focus view.
fn playing_in_focus(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let (window, view) = open_cued_library(cx);
    click(window, "focus-open", cx);
    settle(window, cx);
    press(window, "space", cx);
    settle(window, cx);
    assert!(
        cx.update(|cx| view.read(cx).can_loop_selection()),
        "the track plays"
    );
    (window, view)
}

/// The loop the focus view keeps and the one the sound plays.
fn loops(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> (Option<Region>, Option<Region>) {
    cx.update(|cx| {
        let app = view.read(cx);
        let kept = app.focus.active_loop.as_ref().map(|active| active.region());
        let heard = app
            .voice
            .as_ref()
            .and_then(|voice| voice.state.loop_seconds());
        (kept, heard)
    })
}

fn assert_looping(view: &Entity<EuphoriaApp>, expected: (f64, f64), cx: &mut TestAppContext) {
    let (kept, heard) = loops(view, cx);
    let (start, end) = kept.expect("a loop is kept");
    assert!(
        (start - expected.0).abs() < 1e-6 && (end - expected.1).abs() < 1e-6,
        "{start}..{end} against {expected:?}"
    );
    let (heard_start, heard_end) = heard.expect("the sound loops");
    assert!(
        (heard_start - expected.0).abs() < 1e-3 && (heard_end - expected.1).abs() < 1e-3,
        "heard {heard_start}..{heard_end} against {expected:?}"
    );
}

fn assert_not_looping(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) {
    assert_eq!(loops(view, cx), (None, None));
}

#[gpui::test]
fn the_loop_buttons_loop_from_the_beat_and_halve_double_and_exit(cx: &mut TestAppContext) {
    let (window, view) = playing_in_focus(cx);
    assert!(is_shown(window, "focus-loop-bar", cx));
    assert!(!is_shown(window, "focus-loop-region", cx));

    click(window, length_id(16.0), cx);
    assert_looping(&view, (0.0, 8.0), cx);
    assert!(is_shown(window, "focus-loop-region", cx));
    assert!(is_shown(window, "focus-loop-summary", cx));

    click(window, "focus-loop-halve", cx);
    assert_looping(&view, (0.0, 4.0), cx);
    click(window, "focus-loop-double", cx);
    click(window, "focus-loop-double", cx);
    assert_looping(&view, (0.0, 16.0), cx);

    click(window, length_id(0.5), cx);
    assert_looping(&view, (0.0, 0.25), cx);

    click(window, "focus-loop-exit", cx);
    assert_not_looping(&view, cx);
    assert!(!is_shown(window, "focus-loop-region", cx));
    assert!(
        cx.update(|cx| view.read(cx).player.is_playing()),
        "leaving the loop keeps playing"
    );

    click(window, length_id(4.0), cx);
    assert_looping(&view, (0.0, 2.0), cx);
    click(window, length_id(4.0), cx);
    assert_not_looping(&view, cx);
}

#[gpui::test]
fn l_loops_four_bars_the_arrows_halve_and_double_and_escape_leaves_first(cx: &mut TestAppContext) {
    let (window, view) = playing_in_focus(cx);
    press(window, "l", cx);
    assert_looping(&view, (0.0, 8.0), cx);
    let (halve, double) = if cfg!(target_os = "macos") {
        ("cmd-left", "cmd-right")
    } else {
        ("ctrl-left", "ctrl-right")
    };
    press(window, halve, cx);
    assert_looping(&view, (0.0, 4.0), cx);
    press(window, double, cx);
    press(window, double, cx);
    assert_looping(&view, (0.0, 16.0), cx);
    press(window, "l", cx);
    assert_not_looping(&view, cx);

    press(window, "l", cx);
    assert_looping(&view, (0.0, 8.0), cx);
    press(window, "escape", cx);
    assert_not_looping(&view, cx);
    assert!(
        cx.update(|cx| view.read(cx).focus.is_open),
        "the first Esc leaves the loop, not the view"
    );
    press(window, "escape", cx);
    assert!(!cx.update(|cx| view.read(cx).focus.is_open));
}

#[gpui::test]
fn a_loop_ends_when_the_focus_view_closes_or_playing_jumps_out_of_it(cx: &mut TestAppContext) {
    let (window, view) = playing_in_focus(cx);
    press(window, "l", cx);
    assert_looping(&view, (0.0, 8.0), cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let request = app.player.seek_to_fraction(0.5);
            app.carry_out(request, cx);
        })
    });
    assert_not_looping(&view, cx);

    press(window, "l", cx);
    assert!(loops(&view, cx).0.is_some());
    click(window, "focus-back", cx);
    assert_not_looping(&view, cx);
}

/// Drags across the detail from `from` to `to`, fractions of its width,
/// with `modifiers` held.
fn drag_on_detail(
    window: AnyWindowHandle,
    from: f32,
    to: f32,
    modifiers: Modifiers,
    cx: &mut TestAppContext,
) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let detail = window.find("focus-detail").bounds();
        let at = |fraction: f32| {
            point(
                detail.left() + detail.size.width * fraction,
                detail.top() + detail.size.height * 0.75,
            )
        };
        window.hold_modifiers(modifiers, cx);
        window.drag(at(from), at(to), cx);
        window.hold_modifiers(Modifiers::default(), cx);
    })
    .unwrap();
    cx.run_until_parked();
}

/// Drags the grip of the loop's `id` end to `to`, a fraction of the
/// detail's width.
fn drag_edge(window: AnyWindowHandle, id: &'static str, to: f32, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let grip = window.find(id).bounds().center();
        let detail = window.find("focus-detail").bounds();
        let released = point(detail.left() + detail.size.width * to, grip.y);
        window.drag(grip, released, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

trait HoldModifiers {
    fn hold_modifiers(&mut self, modifiers: Modifiers, cx: &mut gpui::App);
}

impl HoldModifiers for gpui::Window {
    fn hold_modifiers(&mut self, modifiers: Modifiers, cx: &mut gpui::App) {
        let capslock = self.capslock();
        self.dispatch_event(
            gpui::InputEvent::to_platform_input(gpui::ModifiersChangedEvent {
                modifiers,
                capslock,
            }),
            cx,
        );
        self.render_frame(cx);
    }
}

#[gpui::test]
fn a_loop_drawn_on_the_waveform_snaps_to_beats_and_its_ends_move(cx: &mut TestAppContext) {
    let (window, view) = playing_in_focus(cx);
    let secondary = cx.update(|cx| Modifiers::secondary_key(cx));
    let span = cx.update(|cx| view.read(cx).focus.viewport.span);
    assert_eq!(span, 30.0);

    drag_on_detail(window, 0.6, 0.7, Modifiers::default(), cx);
    assert_not_looping(&view, cx);
    assert!(
        (cx.update(|cx| view.read(cx).player.position_seconds()) - 18.0).abs() < 0.1,
        "a plain press seeks as before"
    );

    drag_on_detail(window, 0.302, 0.098, secondary, cx);
    assert_looping(&view, (3.0, 9.0), cx);
    assert!(
        (cx.update(|cx| view.read(cx).player.position_seconds()) - 3.0).abs() < 1e-3,
        "the playhead was outside, so playing jumps to the loop"
    );
    assert!(is_shown(window, "focus-loop-in", cx));
    assert!(is_shown(window, "focus-loop-out", cx));

    drag_edge(window, "focus-loop-out", 0.405, cx);
    assert_looping(&view, (3.0, 12.0), cx);
    drag_edge(window, "focus-loop-in", 0.152, cx);
    assert_looping(&view, (4.5, 12.0), cx);

    let free = Modifiers {
        alt: true,
        ..secondary
    };
    drag_on_detail(window, 0.5, 0.61, free, cx);
    let (kept, _) = loops(&view, cx);
    let (start, end) = kept.unwrap();
    assert!(
        (start - 15.0).abs() < 0.05 && (end - 18.3).abs() < 0.05,
        "Option keeps the loop off the beats: {start}..{end}"
    );
    assert!((start * 2.0).fract() != 0.0 || (end * 2.0).fract() != 0.0);
}

#[gpui::test]
fn a_saved_loop_is_listed_with_the_cues_and_plays_again_with_a_click(cx: &mut TestAppContext) {
    let (window, view) = playing_in_focus(cx);
    click(window, length_id(32.0), cx);
    assert_looping(&view, (0.0, 16.0), cx);
    click(window, "focus-loop-save", cx);
    let saved = cx.update(|cx| view.read(cx).selected_track().unwrap().saved_loops.clone());
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].name, "Loop 1");
    assert_eq!((saved[0].start_seconds, saved[0].end_seconds), (0.0, 16.0));
    assert!(
        !cx.update(|cx| view.read(cx).selected_track().unwrap().cues().is_edited()),
        "the automatic cues stay automatic"
    );
    click(window, "focus-loop-save", cx);
    assert_eq!(
        cx.update(|cx| view.read(cx).selected_track().unwrap().saved_loops.len()),
        1,
        "the same loop is kept once"
    );

    click(window, "focus-loop-exit", cx);
    assert_not_looping(&view, cx);
    click(window, ("focus-plan-tab", PlanTab::Cues as usize), cx);
    assert!(is_shown(window, "saved-loops", cx));
    click(window, saved_loop_id(0), cx);
    assert_looping(&view, (0.0, 16.0), cx);

    click(window, ("saved-loop-delete", 0_usize), cx);
    assert!(cx.update(|cx| {
        view.read(cx)
            .selected_track()
            .unwrap()
            .saved_loops
            .is_empty()
    }));
    assert!(!is_shown(window, "saved-loops", cx));
}

#[gpui::test]
fn the_loop_buttons_wait_for_the_track_to_play(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    click(window, "focus-open", cx);
    settle(window, cx);
    click(window, length_id(16.0), cx);
    assert_not_looping(&view, cx);
    press(window, "l", cx);
    assert_not_looping(&view, cx);
}
