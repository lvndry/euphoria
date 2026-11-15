// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The player bar and the sound, used through the real root view in a
//! headless window: the bar shows the track that is heard, never the one
//! selected or the one still opening.
//!
//! The three fixture tracks get lengths of their own here, 7:01, 5:00 and
//! 3:20, so the length the bar shows tells which track it belongs to.

use std::path::{Path, PathBuf};

use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::playback::testing::SlowOpeningOutput;
use crate::player::PlayerStatus;
use crate::ui_tests::{click, deck_tracks, index_of_track, open_window_on};
use crate::views::next_rail::{play_id, row_id};
use crate::views::player::{BarContent, BarDetail, content};
use crate::waveforms::Waveform;

struct Fixture {
    window: AnyWindowHandle,
    view: Entity<EuphoriaApp>,
    output: SlowOpeningOutput,
    flac: usize,
    mp3: usize,
    wav: usize,
}

fn open(cx: &mut TestAppContext) -> Fixture {
    let mut tracks = deck_tracks();
    for (track, seconds) in tracks.iter_mut().zip([421.0, 300.0, 200.0]) {
        if let Ok(analysis) = &mut track.outcome {
            analysis.duration_seconds = seconds;
        }
    }
    let output = SlowOpeningOutput::default();
    let (window, view) = open_window_on(tracks, Box::new(output.clone()), cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    let wav = index_of_track(&view, "sine-440hz-stereo.wav", cx);
    let fixture = Fixture {
        window,
        view,
        output,
        flac,
        mp3,
        wav,
    };
    for index in [fixture.wav, fixture.mp3, fixture.flac] {
        cx.update(|cx| {
            fixture.view.update(cx, |app, cx| {
                app.select(index, cx);
                app.show_waveform(Waveform::Ready(vec![Default::default(); 64].into(), None));
            })
        });
    }
    settle(&fixture, cx);
    fixture
}

fn settle(fixture: &Fixture, cx: &mut TestAppContext) {
    cx.run_until_parked();
    cx.update_window(fixture.window, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

fn path_of(fixture: &Fixture, index: usize, cx: &mut TestAppContext) -> PathBuf {
    cx.update(|cx| {
        fixture.view.read(cx).session.library().tracks()[index]
            .path
            .clone()
    })
}

fn title_of(fixture: &Fixture, index: usize, cx: &mut TestAppContext) -> String {
    cx.update(|cx| {
        fixture.view.read(cx).session.library().tracks()[index]
            .title()
            .to_string()
    })
}

/// The play button of the Next row of the track at `index`, shown on hover.
fn press_next_play(fixture: &Fixture, index: usize, cx: &mut TestAppContext) {
    cx.update_window(fixture.window, |_, window, cx| {
        window.render_frame(cx);
        window.hover(row_id(index), cx);
        window.render_frame(cx);
        window.click(play_id(index), cx);
    })
    .unwrap();
    cx.run_until_parked();
    let path = path_of(fixture, index, cx);
    assert!(
        fixture.output.waiting().contains(&path),
        "the play button of the row asks for its track"
    );
}

fn bar(fixture: &Fixture, cx: &mut TestAppContext) -> BarContent {
    cx.update(|cx| content(fixture.view.read(cx)))
}

fn held(fixture: &Fixture, cx: &mut TestAppContext) -> Option<PathBuf> {
    cx.update(|cx| fixture.view.read(cx).player.track().map(Path::to_path_buf))
}

fn status(fixture: &Fixture, cx: &mut TestAppContext) -> PlayerStatus {
    cx.update(|cx| fixture.view.read(cx).player.status().clone())
}

/// Checks that everything the bar shows is the track at `index`, `length`
/// long, with its own waveform.
fn bar_is_all_of(fixture: &Fixture, index: usize, length: &str, cx: &mut TestAppContext) {
    let path = path_of(fixture, index, cx);
    let title = title_of(fixture, index, cx);
    let shown = bar(fixture, cx);
    assert_eq!(shown.track.as_deref(), Some(path.as_path()));
    assert_eq!(shown.title.as_deref(), Some(title.as_str()));
    assert_eq!(shown.length, length);
    assert_eq!(shown.waveform_of.as_deref(), Some(path.as_path()));
}

/// Plays the FLAC through the player and lets it open, as the first track
/// of each test.
fn play_the_flac(fixture: &Fixture, cx: &mut TestAppContext) {
    cx.update(|cx| {
        fixture.view.update(cx, |app, cx| {
            app.select(fixture.flac, cx);
            app.play_selection(cx);
        })
    });
    settle(fixture, cx);
    fixture.output.finish_opening();
    settle(fixture, cx);
    assert_eq!(status(fixture, cx), PlayerStatus::Playing);
    let flac = path_of(fixture, fixture.flac, cx);
    assert_eq!(fixture.output.sounding(), [flac]);
}

#[gpui::test]
fn a_click_on_a_next_row_selects_it_and_leaves_the_bar_and_the_sound_alone(
    cx: &mut TestAppContext,
) {
    let fixture = open(cx);
    play_the_flac(&fixture, cx);
    let flac = path_of(&fixture, fixture.flac, cx);

    click(fixture.window, row_id(fixture.mp3), cx);
    settle(&fixture, cx);

    cx.update(|cx| assert_eq!(fixture.view.read(cx).selected, Some(fixture.mp3)));
    assert!(
        fixture.output.waiting().is_empty(),
        "a single click asks for no sound"
    );
    assert_eq!(held(&fixture, cx), Some(flac.clone()));
    assert_eq!(status(&fixture, cx), PlayerStatus::Playing);
    assert_eq!(fixture.output.sounding(), [flac]);
    bar_is_all_of(&fixture, fixture.flac, "7:01", cx);
    assert!(matches!(
        bar(&fixture, cx).detail,
        Some(BarDetail::Artist(_))
    ));
}

#[gpui::test]
fn a_slow_track_leaves_the_bar_on_the_one_heard_until_both_switch_together(
    cx: &mut TestAppContext,
) {
    let fixture = open(cx);
    play_the_flac(&fixture, cx);
    let flac = path_of(&fixture, fixture.flac, cx);
    let mp3 = path_of(&fixture, fixture.mp3, cx);
    cx.update(|cx| {
        fixture.view.update(cx, |app, _| {
            if let Some(active) = &app.voice {
                active
                    .state
                    .set_playhead_frames(222 * u64::from(active.state.sample_rate()));
            }
        })
    });
    cx.update(|cx| fixture.view.update(cx, |app, cx| app.read_playhead(cx)));

    cx.update_window(fixture.window, |_, window, cx| {
        window.render_frame(cx);
        window.double_click(row_id(fixture.mp3), cx);
    })
    .unwrap();
    settle(&fixture, cx);

    assert_eq!(fixture.output.waiting(), std::slice::from_ref(&mp3));
    assert_eq!(held(&fixture, cx), Some(flac.clone()));
    assert_eq!(status(&fixture, cx), PlayerStatus::Playing);
    assert_eq!(fixture.output.sounding(), std::slice::from_ref(&flac));
    bar_is_all_of(&fixture, fixture.flac, "7:01", cx);
    assert_eq!(bar(&fixture, cx).position, "3:42");
    let mp3_title = title_of(&fixture, fixture.mp3, cx);
    assert_eq!(
        bar(&fixture, cx).detail,
        Some(BarDetail::Loading(format!("Loading {mp3_title}\u{2026}")))
    );

    fixture.output.finish_opening();
    settle(&fixture, cx);

    assert_eq!(fixture.output.sounding(), std::slice::from_ref(&mp3));
    assert_eq!(held(&fixture, cx), Some(mp3));
    assert_eq!(status(&fixture, cx), PlayerStatus::Playing);
    bar_is_all_of(&fixture, fixture.mp3, "5:00", cx);
    assert_eq!(bar(&fixture, cx).position, "0:00");
    assert!(matches!(
        bar(&fixture, cx).detail,
        Some(BarDetail::Artist(_))
    ));
}

#[gpui::test]
fn the_bar_never_takes_the_length_or_waveform_of_a_track_not_heard(cx: &mut TestAppContext) {
    let fixture = open(cx);
    play_the_flac(&fixture, cx);
    let wav = path_of(&fixture, fixture.wav, cx);

    cx.update(|cx| {
        fixture
            .view
            .update(cx, |app, cx| app.select_and_reveal(fixture.wav, cx))
    });
    settle(&fixture, cx);
    bar_is_all_of(&fixture, fixture.flac, "7:01", cx);

    press_next_play(&fixture, fixture.mp3, cx);
    settle(&fixture, cx);
    bar_is_all_of(&fixture, fixture.flac, "7:01", cx);

    cx.update(|cx| {
        fixture.view.update(cx, |app, cx| {
            app.select(fixture.wav, cx);
            app.play_selection(cx);
        })
    });
    settle(&fixture, cx);
    bar_is_all_of(&fixture, fixture.flac, "7:01", cx);

    fixture.output.finish_opening();
    settle(&fixture, cx);
    assert_eq!(
        held(&fixture, cx),
        Some(path_of(&fixture, fixture.flac, cx)),
        "the answer for a track no longer asked for is turned away"
    );
    bar_is_all_of(&fixture, fixture.flac, "7:01", cx);

    fixture.output.finish_opening();
    settle(&fixture, cx);
    assert_eq!(fixture.output.sounding(), [wav]);
    bar_is_all_of(&fixture, fixture.wav, "3:20", cx);
}

#[gpui::test]
fn pausing_while_the_next_track_opens_keeps_the_one_heard(cx: &mut TestAppContext) {
    let fixture = open(cx);
    play_the_flac(&fixture, cx);
    let flac = path_of(&fixture, fixture.flac, cx);

    cx.update(|cx| {
        fixture.view.update(cx, |app, cx| {
            app.play_next_track(fixture.mp3, cx);
            app.toggle_playback(cx);
        })
    });
    settle(&fixture, cx);
    assert_eq!(status(&fixture, cx), PlayerStatus::Paused);

    fixture.output.finish_opening();
    settle(&fixture, cx);
    assert_eq!(held(&fixture, cx), Some(flac.clone()));
    assert_eq!(status(&fixture, cx), PlayerStatus::Paused);
    assert_eq!(fixture.output.sounding(), [flac]);
    bar_is_all_of(&fixture, fixture.flac, "7:01", cx);
}
