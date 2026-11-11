// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests that open the real root view in a headless window and use it with
//! clicks and keys, as a person would.

use std::path::{Path, PathBuf};

use euphoria_core::TrackAnalysis;
use gpui::test::TestWindowExt;
use gpui::{
    AnyWindowHandle, AppContext, Bounds, Entity, Point, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};

use crate::app::EuphoriaApp;
use crate::demo::{self, DemoTrack};
use crate::library::{SortColumn, SortOrder};
use crate::panels::Panels;
use crate::playback::testing::{NoOutput, SilentOutput};
use crate::playback::{AudioOutput, PlaybackProblem};
use crate::player::{PlayerRequest, PlayerStatus};
use crate::settings::{FolderTreeMemory, Settings};
use crate::state::Session;
use crate::theme::{AppTheme, Layout, ThemeName};
use crate::views::sidebar::{disclosure_id, folder_row_id};

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
            tuning_cents: None,
            key_timeline: Vec::new(),
        }),
    })
    .collect()
}

fn open_window_on(
    tracks: Vec<DemoTrack>,
    output: Box<dyn AudioOutput>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let usual_size = size(px(1440.0), px(900.0));
    open_window_with(tracks, output, Settings::default(), None, usual_size, cx)
}

/// The fixture library in a window of `window_size`, opened with `settings`
/// as if they had been remembered, and saving to `settings_path` when given.
fn open_window_with(
    tracks: Vec<DemoTrack>,
    output: Box<dyn AudioOutput>,
    settings: Settings,
    settings_path: Option<PathBuf>,
    window_size: gpui::Size<gpui::Pixels>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    cx.update(|cx| {
        gpui::init(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: window_size,
            })),
            ..Default::default()
        };
        let (window, view) = gpui::open_window(options, cx, |window, cx| {
            cx.new(|cx| {
                let mut session = Session::start(None).unwrap();
                session.add_demo_tracks(tracks);
                EuphoriaApp::new(session, settings, settings_path, None, output, window, cx)
            })
        })
        .unwrap();
        crate::register_commands(window, &view, cx);
        (window, view)
    })
}

/// The fixture library opened in `theme` with a remembered arrangement.
fn open_arranged(
    theme: ThemeName,
    panels: Panels,
    window_size: gpui::Size<gpui::Pixels>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let settings = Settings {
        theme,
        panels,
        ..Settings::default()
    };
    open_window_with(
        fixture_tracks(),
        Box::new(SilentOutput),
        settings,
        None,
        window_size,
        cx,
    )
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

const USUAL_WINDOW: gpui::Size<gpui::Pixels> = gpui::Size {
    width: px(1440.0),
    height: px(900.0),
};

/// The smallest window the app allows.
const SMALLEST_WINDOW: gpui::Size<gpui::Pixels> = gpui::Size {
    width: px(960.0),
    height: px(560.0),
};

/// Folds the sidebar with `fold`, checks the table took the freed width and
/// the way back is still there, then unfolds it with `unfold`.
fn fold_and_unfold(
    theme: ThemeName,
    fold: impl FnOnce(&mut gpui::Window, &mut gpui::App),
    unfold: impl FnOnce(&mut gpui::Window, &mut gpui::App),
    cx: &mut TestAppContext,
) {
    let (window, view) = open_arranged(theme, Panels::default(), USUAL_WINDOW, cx);
    let sidebar_width = px(220.0);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("all-tracks").is_some());
        assert_eq!(window.find(track(0)).bounds().left(), sidebar_width);
        let search_left = window.find("search").bounds().left();
        let toggle = window.find("sidebar-toggle").bounds();

        fold(window, cx);
        window.render_frame(cx);
        assert!(window.try_find("all-tracks").is_none());
        assert!(window.try_find("needs-review").is_none());
        assert_eq!(window.find(track(0)).bounds().left(), px(0.0));
        assert_eq!(window.find("search").bounds().left(), search_left);
        assert_eq!(window.find("sidebar-toggle").bounds(), toggle);
        let inspector = window.find("inspector").bounds();
        match AppTheme::named(theme).layout {
            Layout::InspectorRight => assert_eq!(inspector.size.width, px(320.0)),
            Layout::DeckBelow => assert_eq!(inspector.left(), px(0.0)),
        }
    })
    .unwrap();
    cx.update(|cx| assert!(view.read(cx).panels.sidebar_folded));

    cx.update_window(window, |_, window, cx| {
        unfold(window, cx);
        window.render_frame(cx);
        assert!(window.try_find("all-tracks").is_some());
        assert_eq!(window.find(track(0)).bounds().left(), sidebar_width);
    })
    .unwrap();
    cx.update(|cx| assert!(!view.read(cx).panels.sidebar_folded));
}

#[gpui::test]
fn the_toolbar_button_folds_the_sidebar_and_brings_it_back(cx: &mut TestAppContext) {
    fold_and_unfold(
        ThemeName::Slate,
        |window, cx| window.click("sidebar-toggle", cx),
        |window, cx| window.click("sidebar-toggle", cx),
        cx,
    );
}

#[gpui::test]
fn the_toolbar_button_folds_the_sidebar_in_the_strip_layout_too(cx: &mut TestAppContext) {
    fold_and_unfold(
        ThemeName::Deck,
        |window, cx| window.click("sidebar-toggle", cx),
        |window, cx| window.click("sidebar-toggle", cx),
        cx,
    );
}

/// The shortcut that folds the sidebar on the platform the tests run on.
const SIDEBAR_SHORTCUT: &str = if cfg!(target_os = "macos") {
    "ctrl-cmd-s"
} else {
    "ctrl-alt-s"
};

#[gpui::test]
fn the_keyboard_shortcut_folds_the_sidebar_and_brings_it_back(cx: &mut TestAppContext) {
    fold_and_unfold(
        ThemeName::Graphite,
        |window, cx| window.press(SIDEBAR_SHORTCUT, cx),
        |window, cx| window.press(SIDEBAR_SHORTCUT, cx),
        cx,
    );
}

#[gpui::test]
fn the_empty_first_run_window_has_no_track_detail_to_resize(cx: &mut TestAppContext) {
    for theme in [ThemeName::Slate, ThemeName::Deck] {
        let (window, _) = open_window_with(
            Vec::new(),
            Box::new(SilentOutput),
            Settings {
                theme,
                ..Settings::default()
            },
            None,
            USUAL_WINDOW,
            cx,
        );
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("all-tracks").is_some(), "{theme:?}");
            assert!(window.try_find("inspector").is_none(), "{theme:?}");
            assert!(window.try_find("panel-edge").is_none(), "{theme:?}");
        })
        .unwrap();
    }
}

#[gpui::test]
fn the_inspector_edge_lands_under_the_pointer_with_the_sidebar_shown_or_folded(
    cx: &mut TestAppContext,
) {
    for sidebar_folded in [false, true] {
        let panels = Panels {
            sidebar_folded,
            ..Panels::default()
        };
        let (window, view) = open_arranged(ThemeName::Slate, panels, USUAL_WINDOW, cx);
        cx.update_window(window, |_, window, cx| {
            for pointer_x in [1_000.0, 1_100.0, 900.0] {
                drag_edge_to(window, gpui::point(px(pointer_x), px(400.0)), cx);
                let inspector = window.find("inspector").bounds();
                let edge = window.find("panel-edge").bounds();
                assert_eq!(inspector.left(), px(pointer_x), "folded {sidebar_folded}");
                assert_eq!(edge.center().x, px(pointer_x), "folded {sidebar_folded}");
                assert_eq!(window.find(track(0)).bounds().right(), px(pointer_x));
            }
        })
        .unwrap();
        cx.update(|cx| assert_eq!(view.read(cx).panels.inspector_width, 540));
    }
}

#[gpui::test]
fn everything_in_the_strip_fits_at_its_shortest(cx: &mut TestAppContext) {
    let shortest = Panels {
        strip_height: 0,
        ..Panels::default()
    };
    for window_size in [USUAL_WINDOW, SMALLEST_WINDOW] {
        let (window, _) = open_arranged(ThemeName::Deck, shortest, window_size, cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            let strip = window.find("inspector").bounds();
            assert_eq!(strip.size.height, px(124.0));
            let inside_bottom = strip.bottom() - px(16.0);
            for part in ["waveform", "play-button"] {
                let bounds = window.find(part).bounds();
                assert!(
                    bounds.top() >= strip.top(),
                    "{part}: {bounds:?} in {strip:?}"
                );
                assert!(
                    bounds.bottom() <= inside_bottom,
                    "{part}: {bounds:?} in {strip:?}"
                );
            }
            assert!(window.find("waveform").bounds().size.height >= px(64.0));
        })
        .unwrap();
    }
}

#[gpui::test]
fn the_sidebar_folds_on_the_empty_first_run_window_as_well(cx: &mut TestAppContext) {
    let (window, _) = open_library(0, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("all-tracks").is_some());
        window.click("sidebar-toggle", cx);
        window.render_frame(cx);
        assert!(window.try_find("all-tracks").is_none());
        window.click("sidebar-toggle", cx);
        window.render_frame(cx);
        assert!(window.try_find("all-tracks").is_some());
    })
    .unwrap();
}

/// Presses on the middle of the edge of the track detail and releases at
/// `released_at`.
fn drag_edge_to(window: &mut gpui::Window, released_at: Point<gpui::Pixels>, cx: &mut gpui::App) {
    window.render_frame(cx);
    let pressed_at = window.find("panel-edge").bounds().center();
    window.drag(pressed_at, released_at, cx);
    window.render_frame(cx);
}

#[gpui::test]
fn dragging_the_edge_of_the_inspector_resizes_it_between_its_limits(cx: &mut TestAppContext) {
    let (window, view) = open_arranged(ThemeName::Slate, Panels::default(), USUAL_WINDOW, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("inspector").bounds().size.width, px(320.0));

        drag_edge_to(window, gpui::point(px(900.0), px(400.0)), cx);
        let inspector = window.find("inspector").bounds();
        assert_eq!(inspector.size.width, px(540.0));
        assert_eq!(inspector.left(), px(900.0));
        assert_eq!(window.find(track(0)).bounds().right(), px(900.0));
        let waveform = window.find("waveform").bounds();
        assert_eq!(waveform.size.width, px(540.0 - 33.0));

        drag_edge_to(window, gpui::point(px(1_400.0), px(400.0)), cx);
        assert_eq!(window.find("inspector").bounds().size.width, px(288.0));
        assert_eq!(
            window.find("waveform").bounds().size.width,
            px(288.0 - 33.0)
        );

        drag_edge_to(window, gpui::point(px(30.0), px(400.0)), cx);
        assert_eq!(window.find("inspector").bounds().size.width, px(640.0));
        let table_width = window.find(track(0)).bounds().size.width;
        assert_eq!(table_width, px(1440.0 - 220.0 - 640.0));
    })
    .unwrap();
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.panels.inspector_width, 640);
        assert_eq!(app.resizing, None);
    });
}

#[gpui::test]
fn a_double_click_on_the_edge_returns_the_inspector_to_its_usual_width(cx: &mut TestAppContext) {
    let wide = Panels {
        inspector_width: 500,
        ..Panels::default()
    };
    let (window, view) = open_arranged(ThemeName::KeyFirst, wide, USUAL_WINDOW, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(window.find("inspector").bounds().size.width, px(500.0));
        window.double_click("panel-edge", cx);
        window.render_frame(cx);
        assert_eq!(window.find("inspector").bounds().size.width, px(320.0));
    })
    .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).panels, Panels::default()));
}

#[gpui::test]
fn dragging_the_edge_of_the_strip_resizes_it_and_the_waveform_grows_with_it(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_arranged(ThemeName::Deck, Panels::default(), USUAL_WINDOW, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let usual = window.find("inspector").bounds();
        assert_eq!(usual.size.height, px(180.0));
        assert_eq!(usual.bottom(), px(900.0));
        let usual_waveform_height = window.find("waveform").bounds().size.height;

        drag_edge_to(window, gpui::point(px(700.0), px(560.0)), cx);
        let taller = window.find("inspector").bounds();
        assert_eq!(taller.size.height, px(340.0));
        assert_eq!(taller.top(), px(560.0));
        let taller_waveform_height = window.find("waveform").bounds().size.height;
        assert_eq!(taller_waveform_height - usual_waveform_height, px(160.0));

        drag_edge_to(window, gpui::point(px(700.0), px(890.0)), cx);
        assert_eq!(window.find("inspector").bounds().size.height, px(124.0));

        drag_edge_to(window, gpui::point(px(700.0), px(10.0)), cx);
        assert_eq!(window.find("inspector").bounds().size.height, px(420.0));
        assert!(window.find(track(0)).bounds().bottom() <= px(900.0 - 420.0));

        window.double_click("panel-edge", cx);
        window.render_frame(cx);
        assert_eq!(window.find("inspector").bounds().size.height, px(180.0));
    })
    .unwrap();
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.panels, Panels::default());
        assert_eq!(app.resizing, None);
    });
}

#[gpui::test]
fn the_smallest_window_keeps_the_table_whatever_width_was_remembered(cx: &mut TestAppContext) {
    let remembered = Panels {
        inspector_width: 640,
        ..Panels::default()
    };
    let (window, view) = open_arranged(ThemeName::Slate, remembered, SMALLEST_WINDOW, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let inspector = window.find("inspector").bounds();
        let row = window.find(track(0)).bounds();
        assert_eq!(inspector.size.width, px(340.0));
        assert_eq!(inspector.right(), px(960.0));
        assert_eq!(row.size.width, px(400.0));
        assert!(window.find("player-bar").bounds().bottom() <= px(560.0));

        window.click("sidebar-toggle", cx);
        window.render_frame(cx);
        assert_eq!(window.find("inspector").bounds().size.width, px(560.0));
        assert_eq!(window.find(track(0)).bounds().size.width, px(400.0));
    })
    .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).panels.inspector_width, 640));
}

#[gpui::test]
fn the_smallest_window_keeps_table_rows_whatever_strip_height_was_remembered(
    cx: &mut TestAppContext,
) {
    let remembered = Panels {
        strip_height: 420,
        sidebar_folded: true,
        ..Panels::default()
    };
    let (window, _) = open_arranged(ThemeName::Deck, remembered, SMALLEST_WINDOW, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let inspector = window.find("inspector").bounds();
        let first_row = window.find(track(0)).bounds();
        assert_eq!(inspector.bottom(), px(560.0));
        assert!(inspector.size.height >= px(124.0));
        assert!(
            inspector.top() - first_row.top() >= px(200.0 - 70.0),
            "{:?} of table above a strip of {:?}",
            inspector.top() - first_row.top(),
            inspector.size.height
        );
        assert!(first_row.bottom() <= inspector.top());
    })
    .unwrap();
}

#[gpui::test]
fn the_arrangement_is_written_to_the_settings_file_and_comes_back(cx: &mut TestAppContext) {
    let folder = std::env::temp_dir().join(format!("euphoria-ui-panels-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let settings_path = folder.join("settings.json");
    let (window, _) = open_window_with(
        fixture_tracks(),
        Box::new(SilentOutput),
        Settings::default(),
        Some(settings_path.clone()),
        USUAL_WINDOW,
        cx,
    );
    cx.update_window(window, |_, window, cx| {
        drag_edge_to(window, gpui::point(px(1_000.0), px(400.0)), cx);
        window.click("sidebar-toggle", cx);
    })
    .unwrap();

    let saved = Settings::load(&settings_path);
    std::fs::remove_dir_all(&folder).unwrap();
    let saved = saved.unwrap();
    assert_eq!(saved.panels.inspector_width, 440);
    assert!(saved.panels.sidebar_folded);
    assert_eq!(saved.panels.strip_height, 180);
}

const DJ_TRACKS: &str = "/music/DJ tracks";

/// Index of the Folder column among the table's column headers.
const FOLDER_COLUMN: usize = 8;

/// A made up DJ folder: two tracks in afro_house/warmup, one that failed in
/// afro_house/peak, one in deep and one directly in the folder.
fn dj_tracks() -> Vec<DemoTrack> {
    [
        ("afro_house/warmup/one.mp3", true),
        ("afro_house/warmup/two.mp3", true),
        ("afro_house/peak/three.mp3", false),
        ("deep/four.mp3", true),
        ("loose.mp3", true),
    ]
    .into_iter()
    .map(|(relative, is_analyzed)| DemoTrack {
        path: Path::new(DJ_TRACKS).join(relative),
        outcome: if is_analyzed {
            Ok(TrackAnalysis {
                duration_seconds: 300.0,
                key: None,
                bpm: Some(124.0),
                energy: None,
            })
        } else {
            Err("cannot decode".to_string())
        },
    })
    .collect()
}

/// The made up DJ folder added as one folder, opened with `settings`.
fn open_dj_folder(
    settings: Settings,
    settings_path: Option<PathBuf>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_folders(&[DJ_TRACKS], dj_tracks(), settings, settings_path, cx)
}

/// Made up `tracks` in the made up `folders`, as if the folders had been
/// added, opened with `settings`.
fn open_folders(
    folders: &[&str],
    tracks: Vec<DemoTrack>,
    settings: Settings,
    settings_path: Option<PathBuf>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let folders: Vec<PathBuf> = folders.iter().map(PathBuf::from).collect();
    let settings = Settings {
        folder_tree: FolderTreeMemory {
            expanded: settings
                .folder_tree
                .expanded
                .or_else(|| Some(folders.clone())),
            selected: settings.folder_tree.selected,
        },
        ..settings
    };
    cx.update(|cx| {
        gpui::init(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: USUAL_WINDOW,
            })),
            ..Default::default()
        };
        let (window, view) = gpui::open_window(options, cx, |window, cx| {
            cx.new(|cx| {
                let mut session = Session::start(None).unwrap();
                for folder in folders {
                    session.add_demo_folder(folder);
                }
                session.add_demo_tracks(tracks);
                EuphoriaApp::new(
                    session,
                    settings,
                    settings_path,
                    None,
                    Box::new(SilentOutput),
                    window,
                    cx,
                )
            })
        })
        .unwrap();
        crate::register_commands(window, &view, cx);
        (window, view)
    })
}

/// The index in the folder tree of the folder at `relative` inside the DJ
/// folder, or of the DJ folder itself for "".
fn folder_index(view: &Entity<EuphoriaApp>, relative: &str, cx: &mut TestAppContext) -> usize {
    let path = if relative.is_empty() {
        PathBuf::from(DJ_TRACKS)
    } else {
        Path::new(DJ_TRACKS).join(relative)
    };
    cx.update(|cx| view.read(cx).folder_tree.index_of(&path).unwrap())
}

/// The names of the listed folders, indented two spaces a level.
fn listed_folders(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| {
        let app = view.read(cx);
        app.folder_rows
            .iter()
            .map(|index| {
                let node = app.folder_tree.node(*index).unwrap();
                format!("{}{}", "  ".repeat(node.depth), node.name)
            })
            .collect()
    })
}

fn shown_titles(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| {
        let app = view.read(cx);
        app.visible
            .iter()
            .map(|index| app.session.library().tracks()[*index].file_name.clone())
            .collect()
    })
}

fn shown_folder(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Option<String> {
    cx.update(|cx| {
        view.read(cx)
            .folder
            .as_ref()
            .map(|folder| folder.to_string_lossy().into_owned())
    })
}

fn folder_text(view: &Entity<EuphoriaApp>, file_name: &str, cx: &mut TestAppContext) -> String {
    cx.update(|cx| {
        let app = view.read(cx);
        let index = app
            .session
            .library()
            .tracks()
            .iter()
            .position(|track| track.file_name == file_name)
            .unwrap();
        app.folder_column.label(index).unwrap().text.clone()
    })
}

fn joined(names: &[&str]) -> String {
    names.join(std::path::MAIN_SEPARATOR_STR)
}

#[gpui::test]
fn the_triangle_unfolds_a_folder_and_folds_it_again(cx: &mut TestAppContext) {
    let (window, view) = open_dj_folder(Settings::default(), None, cx);
    assert_eq!(
        listed_folders(&view, cx),
        ["DJ tracks", "  afro_house", "  deep"]
    );
    let afro_house = folder_index(&view, "afro_house", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(disclosure_id(afro_house), cx);
    })
    .unwrap();
    assert_eq!(
        listed_folders(&view, cx),
        [
            "DJ tracks",
            "  afro_house",
            "    peak",
            "    warmup",
            "  deep"
        ]
    );
    assert_eq!(shown_folder(&view, cx), None);

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(disclosure_id(afro_house), cx);
    })
    .unwrap();
    assert_eq!(
        listed_folders(&view, cx),
        ["DJ tracks", "  afro_house", "  deep"]
    );
}

#[gpui::test]
fn choosing_a_folder_shows_the_tracks_under_it_and_all_tracks_shows_everything(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_dj_folder(Settings::default(), None, cx);
    let afro_house = folder_index(&view, "afro_house", cx);
    assert_eq!(shown_titles(&view, cx).len(), 5);

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(folder_row_id(afro_house), cx);
    })
    .unwrap();
    assert_eq!(shown_titles(&view, cx), ["one.mp3", "two.mp3", "three.mp3"]);
    assert_eq!(
        shown_folder(&view, cx).as_deref(),
        Some("/music/DJ tracks/afro_house")
    );

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("needs-review", cx);
    })
    .unwrap();
    assert_eq!(shown_titles(&view, cx), ["three.mp3"]);

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(folder_row_id(afro_house), cx);
        window.render_frame(cx);
        window.click("search", cx);
        window.input("two", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(shown_titles(&view, cx), ["two.mp3"]);

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("all-tracks", cx);
    })
    .unwrap();
    assert_eq!(shown_titles(&view, cx), ["two.mp3"]);
    assert_eq!(shown_folder(&view, cx), None);
}

#[gpui::test]
fn sorting_inside_a_folder_groups_the_tracks_by_subfolder(cx: &mut TestAppContext) {
    let (window, view) = open_dj_folder(Settings::default(), None, cx);
    let root = folder_index(&view, "", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(folder_row_id(root), cx);
        window.render_frame(cx);
        window.click(("column", FOLDER_COLUMN), cx);
    })
    .unwrap();
    assert_eq!(
        shown_titles(&view, cx),
        ["loose.mp3", "three.mp3", "one.mp3", "two.mp3", "four.mp3"]
    );
}

#[gpui::test]
fn the_folder_column_is_relative_to_the_folder_on_show(cx: &mut TestAppContext) {
    let (window, view) = open_dj_folder(Settings::default(), None, cx);
    assert_eq!(
        folder_text(&view, "one.mp3", cx),
        joined(&["afro_house", "warmup"])
    );
    assert_eq!(folder_text(&view, "loose.mp3", cx), "");

    let afro_house = folder_index(&view, "afro_house", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(folder_row_id(afro_house), cx);
        window.render_frame(cx);
        assert!(window.try_find(("folder-cell", 0_usize)).is_some());
    })
    .unwrap();
    assert_eq!(folder_text(&view, "one.mp3", cx), "warmup");
    assert_eq!(folder_text(&view, "three.mp3", cx), "peak");

    cx.update_window(window, |_, window, cx| {
        window.click(disclosure_id(afro_house), cx);
        window.render_frame(cx);
    })
    .unwrap();
    let warmup = folder_index(&view, "afro_house/warmup", cx);
    cx.update_window(window, |_, window, cx| {
        window.click(folder_row_id(warmup), cx);
        window.render_frame(cx);
        assert!(window.try_find(("folder-cell", 0_usize)).is_none());
    })
    .unwrap();
    assert_eq!(folder_text(&view, "one.mp3", cx), "");
    assert_eq!(shown_titles(&view, cx), ["one.mp3", "two.mp3"]);
}

#[gpui::test]
fn with_two_folders_added_all_tracks_names_the_folder_first(cx: &mut TestAppContext) {
    let mut tracks = dj_tracks();
    tracks.push(DemoTrack {
        path: PathBuf::from("/promos/label/new.mp3"),
        outcome: Err("cannot decode".to_string()),
    });
    let (_, view) = open_folders(
        &[DJ_TRACKS, "/promos"],
        tracks,
        Settings::default(),
        None,
        cx,
    );
    assert_eq!(
        folder_text(&view, "one.mp3", cx),
        joined(&["DJ tracks", "afro_house", "warmup"])
    );
    assert_eq!(
        folder_text(&view, "new.mp3", cx),
        joined(&["promos", "label"])
    );
    assert_eq!(folder_text(&view, "loose.mp3", cx), "DJ tracks");
}

#[gpui::test]
fn the_arrow_keys_move_through_the_folders_and_fold_and_unfold_them(cx: &mut TestAppContext) {
    let (window, view) = open_dj_folder(Settings::default(), None, cx);
    let root = folder_index(&view, "", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(folder_row_id(root), cx);
        window.press("down", cx);
    })
    .unwrap();
    assert_eq!(
        shown_folder(&view, cx).as_deref(),
        Some("/music/DJ tracks/afro_house")
    );

    cx.update_window(window, |_, window, cx| window.press("right", cx))
        .unwrap();
    assert_eq!(
        listed_folders(&view, cx),
        [
            "DJ tracks",
            "  afro_house",
            "    peak",
            "    warmup",
            "  deep"
        ]
    );
    cx.update_window(window, |_, window, cx| window.press("right", cx))
        .unwrap();
    assert_eq!(
        shown_folder(&view, cx).as_deref(),
        Some("/music/DJ tracks/afro_house/peak")
    );
    assert_eq!(shown_titles(&view, cx), ["three.mp3"]);

    cx.update_window(window, |_, window, cx| {
        window.press("down", cx);
        window.press("left", cx);
    })
    .unwrap();
    assert_eq!(
        shown_folder(&view, cx).as_deref(),
        Some("/music/DJ tracks/afro_house")
    );
    cx.update_window(window, |_, window, cx| window.press("left", cx))
        .unwrap();
    assert_eq!(
        listed_folders(&view, cx),
        ["DJ tracks", "  afro_house", "  deep"]
    );
    cx.update_window(window, |_, window, cx| window.press("end", cx))
        .unwrap();
    assert_eq!(
        shown_folder(&view, cx).as_deref(),
        Some("/music/DJ tracks/deep")
    );
    cx.update_window(window, |_, window, cx| {
        window.press("up", cx);
        window.press("up", cx);
    })
    .unwrap();
    assert_eq!(shown_folder(&view, cx).as_deref(), Some("/music/DJ tracks"));
    assert_eq!(shown_titles(&view, cx).len(), 5);
}

#[gpui::test]
fn the_tree_and_the_folder_on_show_come_back_and_gone_folders_are_tolerated(
    cx: &mut TestAppContext,
) {
    let folder = std::env::temp_dir().join(format!("euphoria-ui-tree-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let settings_path = folder.join("settings.json");
    let (window, view) = open_dj_folder(Settings::default(), Some(settings_path.clone()), cx);
    let afro_house = folder_index(&view, "afro_house", cx);
    let deep = folder_index(&view, "deep", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(disclosure_id(afro_house), cx);
        window.render_frame(cx);
        window.click(folder_row_id(deep), cx);
    })
    .unwrap();
    let saved = Settings::load(&settings_path);
    std::fs::remove_dir_all(&folder).unwrap();
    let saved = saved.unwrap();
    assert_eq!(
        saved.folder_tree.selected,
        Some(Path::new(DJ_TRACKS).join("deep"))
    );

    let (_, reopened) = open_dj_folder(saved.clone(), None, cx);
    assert_eq!(
        listed_folders(&reopened, cx),
        [
            "DJ tracks",
            "  afro_house",
            "    peak",
            "    warmup",
            "  deep"
        ]
    );
    assert_eq!(shown_titles(&reopened, cx), ["four.mp3"]);

    let gone = Settings {
        folder_tree: FolderTreeMemory {
            expanded: Some(vec![PathBuf::from("/not/here"), PathBuf::from(DJ_TRACKS)]),
            selected: Some(PathBuf::from("/not/here/either")),
        },
        ..saved
    };
    let (window, after_loss) = open_dj_folder(gone, None, cx);
    cx.run_until_parked();
    assert_eq!(
        listed_folders(&after_loss, cx),
        ["DJ tracks", "  afro_house", "  deep"]
    );
    assert_eq!(shown_folder(&after_loss, cx), None);
    assert_eq!(shown_titles(&after_loss, cx).len(), 5);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("all-tracks").is_some());
    })
    .unwrap();
}

#[gpui::test]
fn a_tree_of_hundreds_of_folders_scrolls_and_builds_only_the_rows_in_view(cx: &mut TestAppContext) {
    let tracks: Vec<DemoTrack> = (0..3_000)
        .map(|number| DemoTrack {
            path: PathBuf::from(format!(
                "/music/DJ tracks/label {:03}/release {}/track {number}.mp3",
                number % 300,
                number % 3
            )),
            outcome: Err("cannot decode".to_string()),
        })
        .collect();
    let (window, view) = open_folders(&[DJ_TRACKS], tracks, Settings::default(), None, cx);
    assert_eq!(listed_folders(&view, cx).len(), 301);
    let last = cx.update(|cx| *view.read(cx).folder_rows.last().unwrap());
    let root = folder_index(&view, "", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let tree = window.find("folder-tree").bounds();
        assert!(tree.bottom() <= px(900.0), "{tree:?}");
        assert!(window.try_find(folder_row_id(last)).is_none());
        window.click(folder_row_id(root), cx);
        window.press("end", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(window.try_find(folder_row_id(last)).is_some());
    })
    .unwrap();
}

#[gpui::test]
fn all_tracks_counts_the_whole_library_while_a_folder_is_on_show(cx: &mut TestAppContext) {
    let mut tracks = dj_tracks();
    tracks.push(DemoTrack {
        path: PathBuf::from("/promos/label/new.mp3"),
        outcome: Err("cannot decode".to_string()),
    });
    let (window, view) = open_folders(
        &[DJ_TRACKS, "/promos"],
        tracks,
        Settings::default(),
        None,
        cx,
    );
    let counts = |view: &Entity<EuphoriaApp>, cx: &mut TestAppContext| {
        cx.update(|cx| {
            let counts = view.read(cx).sidebar_counts();
            (counts.total, counts.failed)
        })
    };
    assert_eq!(counts(&view, cx), (6, 2));

    let root = folder_index(&view, "", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(folder_row_id(root), cx);
    })
    .unwrap();
    assert_eq!(shown_titles(&view, cx).len(), 5);
    assert_eq!(counts(&view, cx), (6, 1));

    let deep = folder_index(&view, "deep", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(folder_row_id(deep), cx);
    })
    .unwrap();
    assert_eq!(counts(&view, cx), (6, 0));
}

/// How wide `text` is in the table's text size.
fn text_width(window: &mut gpui::Window, text: &str) -> gpui::Pixels {
    let run = window.text_style().to_run(text.len());
    window
        .text_system()
        .shape_line(
            text.to_string().into(),
            crate::views::TEXT_SIZE,
            &[run],
            None,
        )
        .width
}

#[gpui::test]
fn two_level_folder_paths_show_in_full_at_the_usual_window_size(cx: &mut TestAppContext) {
    let (window, view) = open_dj_folder(Settings::default(), None, cx);
    let root = folder_index(&view, "", cx);
    let one = cx.update(|cx| {
        view.read(cx)
            .session
            .library()
            .tracks()
            .iter()
            .position(|track| track.file_name == "one.mp3")
            .unwrap()
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(folder_row_id(root), cx);
        window.render_frame(cx);
        let path = joined(&["afro_house", "warmup"]);
        let needed = text_width(window, &path);
        let folder_cell = window.find(("folder-cell", one)).bounds();
        let title = window.find(("column", 1_usize)).bounds();
        assert!(
            folder_cell.size.width >= needed,
            "{folder_cell:?} for {needed:?}"
        );
        assert!(title.size.width >= px(160.0), "{title:?}");
    })
    .unwrap();
}

#[gpui::test]
fn the_smallest_window_leaves_columns_out_before_squeezing_the_title(cx: &mut TestAppContext) {
    for sidebar_folded in [false, true] {
        let panels = Panels {
            sidebar_folded,
            ..Panels::default()
        };
        let (window, _) = open_arranged(ThemeName::Slate, panels, SMALLEST_WINDOW, cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let title = window.find(("column", 1_usize)).bounds();
            let energy = window.find(("column", ENERGY_COLUMN)).bounds();
            let row = window.find(track(0)).bounds();
            assert!(
                title.size.width >= px(150.0),
                "folded {sidebar_folded}: {title:?}"
            );
            assert!(energy.right() <= row.right(), "folded {sidebar_folded}");
            assert!(window.try_find(("column", BPM_COLUMN)).is_some());
            assert!(window.try_find(("column", FORMAT_COLUMN)).is_none());
        })
        .unwrap();
    }
}

/// Index of the Format column among the table's column headers.
const FORMAT_COLUMN: usize = 7;
