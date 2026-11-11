// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests that open the real root view in a headless window and use it with
//! clicks and keys, as a person would.

use std::path::Path;

use euphoria_core::TrackAnalysis;
use gpui::test::TestWindowExt;
use gpui::{
    AnyWindowHandle, AppContext, Bounds, Entity, Point, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};

use crate::app::EuphoriaApp;
use crate::demo::{self, DemoTrack};
use crate::library::{SortColumn, SortOrder};
use crate::playback::testing::{NoOutput, SilentOutput};
use crate::playback::{AudioOutput, PlaybackProblem};
use crate::player::{PlayerRequest, PlayerStatus};
use crate::settings::Settings;
use crate::state::Session;
use crate::theme::{AppTheme, Layout, ThemeName};

/// Index of the BPM column among the table's column headers.
const BPM_COLUMN: usize = 4;

/// Index of the Energy column among the table's column headers.
const ENERGY_COLUMN: usize = 5;

/// A library of made up tracks whose files do not exist, with an audio
/// output that makes no sound.
fn open_library(
    track_count: usize,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_window_on(demo::tracks(track_count), Box::new(SilentOutput), cx)
}

/// A library of the one second audio files the core is tested with, each
/// already analyzed, with an audio output that makes no sound.
fn open_fixture_library(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_window_on(fixture_tracks(), Box::new(SilentOutput), cx)
}

fn fixture_tracks() -> Vec<DemoTrack> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../euphoria-core/tests/fixtures");
    [
        "sine-440hz-mono.flac",
        "sine-440hz-mono.mp3",
        "sine-440hz-stereo.wav",
    ]
    .into_iter()
    .map(|name| DemoTrack {
        path: fixtures.join(name),
        outcome: Ok(TrackAnalysis {
            duration_seconds: 1.0,
            key: None,
            bpm: None,
            energy: None,
        }),
    })
    .collect()
}

fn open_window_on(
    tracks: Vec<DemoTrack>,
    output: Box<dyn AudioOutput>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    cx.update(|cx| {
        gpui::init(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(1440.0), px(900.0)),
            })),
            ..Default::default()
        };
        let (window, view) = gpui::open_window(options, cx, |window, cx| {
            cx.new(|cx| {
                let mut session = Session::start(None).unwrap();
                session.add_demo_tracks(tracks);
                EuphoriaApp::new(session, Settings::default(), None, None, output, window, cx)
            })
        })
        .unwrap();
        crate::register_commands(window, &view, cx);
        (window, view)
    })
}

fn track(index: usize) -> (&'static str, usize) {
    ("track", index)
}

#[gpui::test]
fn opens_with_the_first_track_selected(cx: &mut TestAppContext) {
    let (_, view) = open_library(50, cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.visible.len(), 50);
        assert_eq!(app.selected, Some(0));
    });
}

#[gpui::test]
fn clicking_a_row_selects_its_track(cx: &mut TestAppContext) {
    let (window, view) = open_library(50, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(track(5), cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).selected, Some(5)));
}

#[gpui::test]
fn arrow_keys_move_the_selection_through_the_rows(cx: &mut TestAppContext) {
    let (window, view) = open_library(50, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(track(5), cx);
        window.press("down", cx);
        window.press("down", cx);
        window.press("up", cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).selected, Some(6)));
}

#[gpui::test]
fn clicking_a_header_sorts_and_clicking_again_reverses(cx: &mut TestAppContext) {
    let (window, view) = open_library(200, cx);
    let tempos = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            let app = view.read(cx);
            app.visible
                .iter()
                .filter_map(|index| app.session.library().track(*index)?.bpm())
                .collect::<Vec<f32>>()
        })
    };

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(("column", BPM_COLUMN), cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).sort_order,
            Some(SortOrder {
                column: SortColumn::Bpm,
                descending: false
            })
        );
    });
    let ascending = tempos(cx);
    assert!(ascending.is_sorted(), "{ascending:?}");

    cx.update_window(window, |_, window, cx| {
        window.click(("column", BPM_COLUMN), cx);
    })
    .unwrap();
    let descending = tempos(cx);
    assert!(descending.iter().rev().is_sorted(), "{descending:?}");
    assert_eq!(descending.len(), ascending.len());
}

#[gpui::test]
fn clicking_the_energy_header_sorts_by_energy(cx: &mut TestAppContext) {
    let (window, view) = open_library(200, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(("column", ENERGY_COLUMN), cx);
    })
    .unwrap();
    cx.update(|cx| {
        let app = view.read(cx);
        let ratings: Vec<u8> = app
            .visible
            .iter()
            .filter_map(|index| app.session.library().track(*index)?.energy())
            .collect();
        assert!(ratings.len() > 100);
        assert!(ratings.is_sorted(), "{ratings:?}");
        let last = app.session.library().track(app.visible[199]).unwrap();
        assert_eq!(last.energy(), None);
    });
}

#[gpui::test]
fn typing_in_the_search_field_filters_the_table(cx: &mut TestAppContext) {
    let (window, view) = open_library(500, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("search", cx);
        window.input("8a", cx);
    })
    .unwrap();
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(!app.visible.is_empty());
        assert!(app.visible.len() < 500);
        for index in &app.visible {
            let key = app.session.library().track(*index).unwrap().key().unwrap();
            assert_eq!(key.camelot(), "8A");
        }
    });
}

#[gpui::test]
fn the_sidebar_shows_the_tracks_that_need_review(cx: &mut TestAppContext) {
    let (window, view) = open_library(2_000, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("needs-review", cx);
    })
    .unwrap();
    cx.update(|cx| {
        let app = view.read(cx);
        let failed = app.session.library().counts().failed;
        assert!(failed > 0);
        assert_eq!(app.visible.len(), failed);
    });
}

#[gpui::test]
fn only_the_rows_in_view_are_built_for_ten_thousand_tracks(cx: &mut TestAppContext) {
    let (window, view) = open_library(10_000, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(track(0)).is_some());
        assert!(window.try_find(track(200)).is_none());
        assert!(window.try_find(track(9_999)).is_none());

        window.click(track(3), cx);
        window.press("end", cx);
        window.render_frame(cx);
        assert!(window.try_find(track(9_999)).is_some());
        assert!(window.try_find(track(0)).is_none());
    })
    .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).selected, Some(9_999)));
}

#[gpui::test]
fn each_theme_lays_the_window_out_as_it_should(cx: &mut TestAppContext) {
    let (window, view) = open_library(50, cx);
    let sidebar_width = px(220.0);
    for name in ThemeName::ALL {
        let expected = AppTheme::named(name);
        cx.update(|cx| view.update(cx, |app, cx| app.set_theme(name, cx)));
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let viewport = window.viewport_size();
            let inspector = window.find("inspector").bounds();
            let first_row = window.find(track(view.read(cx).visible[0])).bounds();

            match expected.layout {
                Layout::InspectorRight => {
                    assert_eq!(inspector.right(), viewport.width, "{name:?}");
                    assert_eq!(inspector.size.width, px(320.0), "{name:?}");
                    assert!(first_row.right() <= inspector.left(), "{name:?}");
                    assert!(inspector.top() < first_row.top(), "{name:?}");
                }
                Layout::DeckBelow => {
                    assert_eq!(inspector.left(), sidebar_width, "{name:?}");
                    assert_eq!(inspector.right(), viewport.width, "{name:?}");
                    assert_eq!(inspector.bottom(), viewport.height, "{name:?}");
                    assert_eq!(first_row.right(), viewport.width, "{name:?}");
                    assert!(inspector.top() > first_row.bottom(), "{name:?}");
                }
            }
            assert_eq!(first_row.size.height, expected.row_height, "{name:?}");
            assert_eq!(
                gpui::component::ActiveTheme::theme(cx).mode.is_dark(),
                expected.palette.is_dark,
                "{name:?}"
            );
        })
        .unwrap();
        cx.update(|cx| {
            let app = view.read(cx);
            assert_eq!(app.theme, expected, "{name:?}");
        });
    }
}

#[gpui::test]
fn the_expected_layout_of_each_theme_is_the_one_in_the_spec(_cx: &mut TestAppContext) {
    let layout_and_rows = |name| {
        let theme = AppTheme::named(name);
        (theme.layout, theme.row_height, theme.shows_wheel)
    };
    let column = Layout::InspectorRight;
    assert_eq!(layout_and_rows(ThemeName::Slate), (column, px(32.0), false));
    assert_eq!(layout_and_rows(ThemeName::Paper), (column, px(32.0), false));
    assert_eq!(
        layout_and_rows(ThemeName::Deck),
        (Layout::DeckBelow, px(32.0), false)
    );
    assert_eq!(
        layout_and_rows(ThemeName::Graphite),
        (column, px(26.0), false)
    );
    assert_eq!(
        layout_and_rows(ThemeName::KeyFirst),
        (column, px(32.0), true)
    );
}

#[gpui::test]
fn key_first_sorts_the_table_by_key(cx: &mut TestAppContext) {
    let (_, view) = open_library(50, cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| app.set_theme(ThemeName::KeyFirst, cx));
        assert_eq!(
            view.read(cx).sort_order.map(|order| order.column),
            Some(SortColumn::Key)
        );
    });
}

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

#[gpui::test]
fn selecting_another_track_while_one_plays_switches_to_it(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("play-button", cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update_window(window, |_, window, cx| window.click(track(2), cx))
        .unwrap();
    assert_eq!(player_status(&view, cx), PlayerStatus::Starting);
    cx.run_until_parked();
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.player.status(), &PlayerStatus::Playing);
        assert_eq!(
            app.player.track(),
            app.selected_track().map(|track| track.path.as_path())
        );
        assert!(
            app.player
                .track()
                .unwrap()
                .ends_with("sine-440hz-stereo.wav")
        );
        assert_eq!(app.player.position_seconds(), 0.0);
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
                    assert!(waveform.size.width > px(300.0), "{name:?}");
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
