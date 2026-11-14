// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The metronome button, its tempo field and its settings, used through
//! the real root view in a headless window.

use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::demo;
use crate::metronome::ClickSound;
use crate::playback::testing::{ListeningOutput, NoOutput};
use crate::settings::Settings;
use crate::ui_tests::{click, is_shown, open_window_on, retype, track};

fn open_listening(
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>, ListeningOutput) {
    let output = ListeningOutput::default();
    let (window, view) = open_window_on(demo::tracks(20), Box::new(output.clone()), cx);
    (window, view, output)
}

fn type_tempo(window: AnyWindowHandle, text: &str, cx: &mut TestAppContext) {
    click(window, "metronome-bpm", cx);
    retype(window, "metronome-field", text, cx);
    cx.update_window(window, |_, window, cx| window.press("enter", cx))
        .unwrap();
    cx.run_until_parked();
}

fn tempo(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> f32 {
    cx.update(|cx| view.read(cx).metronome_bpm())
}

#[gpui::test]
fn the_button_starts_and_stops_the_clicks(cx: &mut TestAppContext) {
    let (window, view, output) = open_listening(cx);
    assert!(is_shown(window, "metronome-toggle", cx));
    assert_eq!(output.metronomes(), (0, 0));

    click(window, "metronome-toggle", cx);
    assert_eq!(output.metronomes(), (1, 1));
    cx.update(|cx| assert!(view.read(cx).metronome.is_running()));

    click(window, "metronome-toggle", cx);
    assert_eq!(output.metronomes(), (1, 0));
    cx.update(|cx| assert!(!view.read(cx).metronome.is_running()));
}

#[gpui::test]
fn the_tempo_follows_the_selected_track_until_one_is_typed(cx: &mut TestAppContext) {
    let (window, view, _) = open_listening(cx);
    let track_bpm = |index: usize, cx: &mut TestAppContext| {
        cx.update(|cx| {
            view.read(cx)
                .session
                .library()
                .track(index)
                .and_then(|track| track.bpm())
        })
    };
    click(window, track(0), cx);
    let first = track_bpm(0, cx).expect("the demo tracks have tempos");
    assert_eq!(tempo(&view, cx), crate::metronome::clamp_bpm(first));

    type_tempo(window, "127,5", cx);
    assert_eq!(tempo(&view, cx), 127.5);
    click(window, track(3), cx);
    assert_eq!(tempo(&view, cx), 127.5, "a typed tempo stays");

    type_tempo(window, "98.3", cx);
    assert_eq!(tempo(&view, cx), 98.3);
}

#[gpui::test]
fn a_tempo_out_of_range_is_refused_and_the_old_one_stays(cx: &mut TestAppContext) {
    let (window, view, _) = open_listening(cx);
    type_tempo(window, "140", cx);
    type_tempo(window, "500", cx);
    assert_eq!(tempo(&view, cx), 140.0);
    cx.update(|cx| assert!(view.read(cx).notice.is_some()));
}

#[gpui::test]
fn the_audio_thread_gets_the_new_tempo_while_running(cx: &mut TestAppContext) {
    let (window, view, _) = open_listening(cx);
    click(window, "metronome-toggle", cx);
    type_tempo(window, "150", cx);
    cx.update(|cx| assert_eq!(view.read(cx).metronome.control.bpm(), 150.0));
}

#[gpui::test]
fn the_settings_choose_the_sound_and_play_a_preview(cx: &mut TestAppContext) {
    let (window, view, output) = open_listening(cx);
    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    assert!(is_shown(window, ("metronome-sound", 3_usize), cx));

    click(window, ("metronome-sound", 2_usize), cx);
    cx.update(|cx| assert_eq!(view.read(cx).metronome.settings.sound, ClickSound::HiHat));
    assert_eq!(output.metronomes().0, 1, "one click previews the sound");

    click(window, ("metronome-volume", 2_usize), cx);
    click(window, "metronome-accent", cx);
    cx.update(|cx| {
        let settings = view.read(cx).metronome.settings;
        assert_eq!(settings.volume, 0.4);
        assert!(!settings.accent);
    });
}

#[gpui::test]
fn a_computer_without_output_says_so_and_does_not_click(cx: &mut TestAppContext) {
    let (window, view) = open_window_on(demo::tracks(5), Box::new(NoOutput), cx);
    click(window, "metronome-toggle", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(!app.metronome.is_running());
        assert!(app.notice.is_some());
    });
}

#[gpui::test]
fn old_settings_start_the_metronome_on_its_defaults(cx: &mut TestAppContext) {
    let (_, view, _) = open_listening(cx);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).metronome.settings,
            Settings::default().metronome
        );
    });
}
