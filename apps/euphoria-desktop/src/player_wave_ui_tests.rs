// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of the waveform in the player bar, in the real root view in a
//! headless window.

use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext, px, size};

use crate::app::EuphoriaApp;
use crate::peaks::BandPeaks;
use crate::player::PlayerStatus;
use crate::ui_tests::{click, open_cued_library, open_fixture_library};
use crate::waveforms::Waveform;

fn ready_waveform() -> Waveform {
    let columns: Vec<BandPeaks> = (0..300)
        .map(|index| BandPeaks {
            low: 0.2 + 0.6 * ((index % 20) as f32 / 20.0),
            mid: 0.3,
            high: 0.1,
        })
        .collect();
    Waveform::Ready(columns.into(), None)
}

fn show_waveform(view: &Entity<EuphoriaApp>, waveform: Waveform, cx: &mut TestAppContext) {
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.show_waveform(waveform);
            cx.notify();
        })
    });
}

fn render(window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

fn click_wave_at(window: AnyWindowHandle, fraction: f32, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let bounds = window.find("player-wave").bounds();
        window.click_at(
            "player-wave",
            gpui::point(bounds.size.width * fraction, px(30.0)),
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
}

fn status(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> PlayerStatus {
    cx.update(|cx| view.read(cx).player.status().clone())
}

fn position(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> f32 {
    cx.update(|cx| view.read(cx).player.position_seconds())
}

fn computations(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> usize {
    cx.update(|cx| view.read(cx).player_wave_peaks.borrow().computations())
}

#[gpui::test]
fn the_bar_shows_a_thin_line_until_the_waveform_is_ready(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update(|cx| view.update(cx, |app, _| app.show_waveform(Waveform::Reading)));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("progress-line").is_some());
        assert!(window.try_find("player-wave").is_none());
    })
    .unwrap();

    show_waveform(&view, Waveform::Unreadable("no".to_string()), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("progress-line").is_some());
        assert!(window.try_find("player-wave").is_none());
    })
    .unwrap();

    show_waveform(&view, ready_waveform(), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("progress-line").is_none());
        assert!(window.try_find("player-wave").is_some());
    })
    .unwrap();
}

#[gpui::test]
fn the_peaks_are_computed_once_and_again_when_the_bar_is_resized(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    show_waveform(&view, ready_waveform(), cx);
    for _ in 0..5 {
        render(window, cx);
    }
    assert_eq!(computations(&view, cx), 1);

    click_wave_at(window, 0.3, cx);
    click_wave_at(window, 0.6, cx);
    render(window, cx);
    assert_eq!(
        computations(&view, cx),
        1,
        "playing moves, the peaks do not"
    );

    cx.simulate_window_resize(window, size(px(1500.0), px(900.0)));
    render(window, cx);
    render(window, cx);
    assert_eq!(computations(&view, cx), 2);
}

#[gpui::test]
fn a_click_on_the_bar_seeks_without_starting_while_stopped_or_paused(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    show_waveform(&view, ready_waveform(), cx);
    click_wave_at(window, 0.5, cx);
    assert!((position(&view, cx) - 0.5).abs() < 0.02);
    assert_eq!(status(&view, cx), PlayerStatus::Stopped);

    click(window, "play-button", cx);
    assert_eq!(status(&view, cx), PlayerStatus::Playing);
    click_wave_at(window, 0.8, cx);
    assert!((position(&view, cx) - 0.8).abs() < 0.02);
    assert_eq!(status(&view, cx), PlayerStatus::Playing);

    click(window, "play-button", cx);
    assert_eq!(status(&view, cx), PlayerStatus::Paused);
    click_wave_at(window, 0.2, cx);
    assert!((position(&view, cx) - 0.2).abs() < 0.02);
    assert_eq!(status(&view, cx), PlayerStatus::Paused);
}

#[gpui::test]
fn a_click_on_the_inspector_waveform_seeks_without_starting_while_stopped_or_paused(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_fixture_library(cx);
    show_waveform(&view, ready_waveform(), cx);
    let click_waveform = |fraction: f32, cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let bounds = window.find("waveform").bounds();
            window.click_at(
                "waveform",
                gpui::point(bounds.size.width * fraction, px(20.0)),
                cx,
            );
        })
        .unwrap();
        cx.run_until_parked();
    };
    click_waveform(0.5, cx);
    assert!((position(&view, cx) - 0.5).abs() < 0.02);
    assert_eq!(status(&view, cx), PlayerStatus::Stopped);

    click(window, "play-button", cx);
    click_waveform(0.7, cx);
    assert_eq!(status(&view, cx), PlayerStatus::Playing);

    click(window, "play-button", cx);
    click_waveform(0.1, cx);
    assert!((position(&view, cx) - 0.1).abs() < 0.02);
    assert_eq!(status(&view, cx), PlayerStatus::Paused);
}

#[gpui::test]
fn a_click_on_a_cue_letter_jumps_to_the_cue_and_leaves_play_alone(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    show_waveform(&view, ready_waveform(), cx);
    click(window, ("player-cue", 1_usize), cx);
    assert_eq!(position(&view, cx), 16.0);
    assert_eq!(status(&view, cx), PlayerStatus::Stopped);

    click(window, "play-button", cx);
    click(window, ("player-cue", 0_usize), cx);
    assert_eq!(position(&view, cx), 0.0);
    assert_eq!(status(&view, cx), PlayerStatus::Playing);

    click(window, "play-button", cx);
    click(window, ("player-cue", 1_usize), cx);
    assert_eq!(position(&view, cx), 16.0);
    assert_eq!(status(&view, cx), PlayerStatus::Paused);
}

#[gpui::test]
fn section_boundaries_get_a_tick_and_the_start_does_not(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    show_waveform(&view, ready_waveform(), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let wave = window.find("player-wave").bounds();
        let tick = window.find(("player-section", 0_usize)).bounds();
        assert!(window.try_find(("player-section", 1_usize)).is_none());
        let middle = wave.left() + wave.size.width * 0.5;
        assert!(
            (tick.center().x - middle).abs() < px(2.0),
            "{tick:?} {wave:?}"
        );
    })
    .unwrap();
}
