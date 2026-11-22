// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// 23 tests: the player bar, the waveform and seeking.

fn player_status(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> PlayerStatus {
    cx.update(|cx| view.read(cx).player.status().clone())
}

#[gpui::test]
fn the_play_button_plays_the_selected_track_and_then_pauses_it(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    assert_eq!(player_status(&view, cx), PlayerStatus::Stopped);

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("play-button", cx);
    })
    .unwrap();
    assert_eq!(player_status(&view, cx), PlayerStatus::Starting);
    cx.run_until_parked();
    assert_eq!(player_status(&view, cx), PlayerStatus::Playing);

    cx.update_window(window, |_, window, cx| window.click("play-button", cx))
        .unwrap();
    assert_eq!(player_status(&view, cx), PlayerStatus::Paused);

    cx.update_window(window, |_, window, cx| window.click("play-button", cx))
        .unwrap();
    assert_eq!(player_status(&view, cx), PlayerStatus::Playing);
}

#[gpui::test]
fn the_space_bar_plays_and_pauses(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("space", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(player_status(&view, cx), PlayerStatus::Playing);

    cx.update_window(window, |_, window, cx| {
        window.click(track(0), cx);
        window.press("space", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(player_status(&view, cx), PlayerStatus::Paused);
}

#[gpui::test]
fn the_arrow_keys_move_the_selection_before_anything_is_clicked(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press("down", cx);
        window.press("end", cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).selected, Some(2)));
}

#[gpui::test]
fn the_space_bar_does_not_reach_the_player_from_the_search_field(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("search", cx);
        window.press("space", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(player_status(&view, cx), PlayerStatus::Stopped);
}

fn held_path(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Option<PathBuf> {
    cx.update(|cx| view.read(cx).player.track().map(Path::to_path_buf))
}

fn start_playing_the_first_track(
    window: AnyWindowHandle,
    view: &Entity<EuphoriaApp>,
    cx: &mut TestAppContext,
) -> PathBuf {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("play-button", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(player_status(view, cx), PlayerStatus::Playing);
    held_path(view, cx).unwrap()
}

#[gpui::test]
fn clicking_another_row_while_one_plays_only_selects_it(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    let playing = start_playing_the_first_track(window, &view, cx);

    cx.update_window(window, |_, window, cx| window.click(track(2), cx))
        .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.selected, Some(2));
        assert_eq!(app.player.status(), &PlayerStatus::Playing);
        assert_eq!(app.player.track(), Some(playing.as_path()));
    });
}

#[gpui::test]
fn the_arrow_keys_while_one_plays_do_not_switch_it(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    let playing = start_playing_the_first_track(window, &view, cx);

    cx.update_window(window, |_, window, cx| {
        window.click(track(0), cx);
        window.press("down", cx);
        window.press("down", cx);
        window.press("up", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(view.read(cx).selected, Some(1)));
    assert_eq!(held_path(&view, cx), Some(playing));
    assert_eq!(player_status(&view, cx), PlayerStatus::Playing);
}

#[gpui::test]
fn a_double_click_switches_playback_to_the_row(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    let playing = start_playing_the_first_track(window, &view, cx);

    cx.update_window(window, |_, window, cx| window.double_click(track(2), cx))
        .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.selected, Some(2));
        assert_eq!(app.player.status(), &PlayerStatus::Playing);
        assert_ne!(app.player.track(), Some(playing.as_path()));
        assert_eq!(
            app.player.track(),
            app.selected_track().map(|track| track.path.as_path())
        );
        assert_eq!(app.player.position_seconds(), 0.0);
        assert!(
            !app.focus.is_open,
            "a double click plays, Enter opens focus"
        );
    });
}

#[gpui::test]
fn space_plays_the_selected_row_when_another_one_sounds(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    let playing = start_playing_the_first_track(window, &view, cx);

    cx.update_window(window, |_, window, cx| {
        window.click(track(2), cx);
        window.press("space", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.player.status(), &PlayerStatus::Playing);
        assert_ne!(app.player.track(), Some(playing.as_path()));
        assert_eq!(
            app.player.track(),
            app.selected_track().map(|track| track.path.as_path())
        );
    });

    cx.update_window(window, |_, window, cx| window.press("space", cx))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(player_status(&view, cx), PlayerStatus::Paused);
}

#[gpui::test]
fn the_row_play_button_switches_playback_to_its_row(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    let playing = start_playing_the_first_track(window, &view, cx);

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.hover(track(2), cx);
        window.render_frame(cx);
        window.click(("row-play", 2usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.selected, Some(2));
        assert_eq!(app.player.status(), &PlayerStatus::Playing);
        assert_ne!(app.player.track(), Some(playing.as_path()));
    });
}

#[gpui::test]
fn selecting_another_track_while_stopped_does_not_start_it(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(track(1), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(player_status(&view, cx), PlayerStatus::Stopped);
}

#[gpui::test]
fn a_click_on_the_waveform_seeks_to_that_point(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("play-button", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let waveform = window.find("waveform").bounds();
        let three_quarters_along = gpui::point(waveform.size.width * 0.75, px(20.0));
        window.click_at("waveform", three_quarters_along, cx);
    })
    .unwrap();
    cx.update(|cx| {
        let app = view.read(cx);
        let position = app.player.position_seconds();
        assert!((position - 0.75).abs() < 0.02, "{position}");
        assert_eq!(app.player.status(), &PlayerStatus::Playing);
    });
}

/// The point `fraction` of the way along the element called `id`, at half
/// its height, in window coordinates.
fn point_along(window: &gpui::Window, id: &'static str, fraction: f32) -> Point<gpui::Pixels> {
    let bounds = window.find(id).bounds();
    gpui::point(
        bounds.left() + bounds.size.width * fraction,
        bounds.top() + bounds.size.height * 0.5,
    )
}

#[gpui::test]
fn dragging_along_the_waveform_moves_playing_to_where_the_drag_ends(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("play-button", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let pressed_at = point_along(window, "waveform", 0.25);
        let released_at = point_along(window, "waveform", 0.8);
        window.drag(pressed_at, released_at, cx);
    })
    .unwrap();
    cx.update(|cx| {
        let app = view.read(cx);
        let position = app.player.position_seconds();
        assert!((position - 0.8).abs() < 0.02, "{position}");
        assert_eq!(app.player.status(), &PlayerStatus::Playing);
    });
}

#[gpui::test]
fn dragging_back_along_the_waveform_while_paused_stays_paused(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("play-button", cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        window.click("play-button", cx);
        window.render_frame(cx);
        let pressed_at = point_along(window, "waveform", 0.9);
        let released_at = point_along(window, "waveform", 0.1);
        window.drag(pressed_at, released_at, cx);
    })
    .unwrap();
    cx.update(|cx| {
        let app = view.read(cx);
        let position = app.player.position_seconds();
        assert!((position - 0.1).abs() < 0.02, "{position}");
        assert_eq!(app.player.status(), &PlayerStatus::Paused);
    });
}

#[gpui::test]
fn dragging_along_the_progress_line_seeks_too(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let pressed_at = point_along(window, "progress-line", 0.2);
        let released_at = point_along(window, "progress-line", 0.6);
        window.drag(pressed_at, released_at, cx);
    })
    .unwrap();
    cx.update(|cx| {
        let position = view.read(cx).player.position_seconds();
        assert!((position - 0.6).abs() < 0.02, "{position}");
    });
}

#[gpui::test]
fn a_shift_changed_while_opening_is_applied_before_playback_starts(cx: &mut TestAppContext) {
    let (_, view) = open_fixture_library(cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let Some(PlayerRequest::Start { attempt, .. }) = app.player.toggle() else {
                panic!("expected an opening track");
            };
            let fixture = app.selected_track().unwrap().path.clone();
            let source = crate::playback::TrackSource::open_shifted(&fixture, 0.0, 0).unwrap();
            let state = source.state();
            app.set_key_shift(2, cx);
            app.finish_starting(attempt, Ok(source), cx);
            // A change must invalidate the already-decoded unshifted audio,
            // just as a seek does, instead of retaining the opener's snapshot.
            assert!(state.position_seconds() == 0.0);
            assert!(state.shift_is(2));
        });
    });
}

#[gpui::test]
fn an_answer_to_an_abandoned_start_does_not_touch_a_retry(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    let attempt_of = |request| match request {
        Some(PlayerRequest::Start { attempt, .. }) => attempt,
        other => panic!("expected a start, got {other:?}"),
    };
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let abandoned = attempt_of(app.player.toggle());
            app.player.toggle();
            let retry = attempt_of(app.player.toggle());

            app.finish_starting(abandoned, Err(PlaybackProblem::FileMissing), cx);
            assert_eq!(app.player.status(), &PlayerStatus::Starting);

            let fixture = app.selected_track().unwrap().path.clone();
            let source = crate::playback::TrackSource::open(&fixture, 0.0);
            app.finish_starting(retry, source, cx);
            assert_eq!(app.player.status(), &PlayerStatus::Playing);
        });
    });
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

#[gpui::test]
fn a_click_on_the_progress_line_of_the_player_bar_seeks_too(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let line = window.find("progress-line").bounds();
        window.click_at(
            "progress-line",
            gpui::point(line.size.width * 0.5, px(10.0)),
            cx,
        );
    })
    .unwrap();
    cx.update(|cx| {
        let app = view.read(cx);
        let position = app.player.position_seconds();
        assert!((position - 0.5).abs() < 0.02, "{position}");
        assert_eq!(app.player.status(), &PlayerStatus::Stopped);
    });
}

#[gpui::test]
fn a_track_whose_file_is_gone_says_so_and_the_app_carries_on(cx: &mut TestAppContext) {
    let (window, view) = open_library(20, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("play-button", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        player_status(&view, cx),
        PlayerStatus::Failed(PlaybackProblem::FileMissing.message().to_string())
    );

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(track(4), cx);
    })
    .unwrap();
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.selected, Some(4));
        assert_eq!(app.player.status(), &PlayerStatus::Stopped);
    });
}

#[gpui::test]
fn a_computer_with_no_audio_output_says_so_and_the_app_carries_on(cx: &mut TestAppContext) {
    let (window, view) = open_window_on(fixture_tracks(), Box::new(NoOutput), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("play-button", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        player_status(&view, cx),
        PlayerStatus::Failed(PlaybackProblem::NoOutput.message().to_string())
    );

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(track(1), cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).selected, Some(1)));
}

#[gpui::test]
fn each_theme_puts_the_player_where_its_layout_expects(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    for name in ThemeName::ALL {
        let layout = AppTheme::named(name).layout;
        cx.update(|cx| view.update(cx, |app, cx| app.set_theme(name, cx)));
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let viewport = window.viewport_size();
            let button = window.find("play-button").bounds();
            let waveform = window.find("waveform").bounds();
            let inspector = window.find("inspector").bounds();
            assert!(inspector.contains(&waveform.center()), "{name:?}");

            match layout {
                Layout::InspectorRight => {
                    let bar = window.find("player-bar").bounds();
                    assert_eq!(bar.left(), px(0.0), "{name:?}");
                    assert_eq!(bar.right(), viewport.width, "{name:?}");
                    assert_eq!(bar.bottom(), viewport.height, "{name:?}");
                    assert!(bar.contains(&button.center()), "{name:?}");
                    assert!(inspector.bottom() <= bar.top(), "{name:?}");
                    assert!(window.try_find("progress-line").is_some(), "{name:?}");
                }
                Layout::DeckBelow => {
                    assert!(window.try_find("player-bar").is_none(), "{name:?}");
                    assert!(inspector.contains(&button.center()), "{name:?}");
                    assert!(button.right() <= waveform.left(), "{name:?}");
                    // The stems section shares the strip's width, so the
                    // waveform keeps what is left of it.
                    assert!(waveform.size.width > px(240.0), "{name:?}");
                }
            }
        })
        .unwrap();
    }
}

#[gpui::test]
fn the_empty_window_has_no_player(cx: &mut TestAppContext) {
    let (window, _) = open_library(0, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("player-bar").is_none());
        assert!(window.try_find("play-button").is_none());
    })
    .unwrap();
}
