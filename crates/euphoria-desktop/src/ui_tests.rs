// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests that open the real root view in a headless window and use it with
//! clicks and keys, as a person would.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use euphoria_core::TrackAnalysis;
use gpui::test::TestWindowExt;
use gpui::{
    AnyWindowHandle, AppContext, Bounds, Entity, Modifiers, Point, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};

use crate::app::EuphoriaApp;
use crate::columns::ColumnId;
use crate::demo::{self, DemoTrack};
use crate::library::{SortColumn, SortOrder};
use crate::panels::Panels;
use crate::playback::testing::{ListeningOutput, NoOutput, SilentOutput};
use crate::playback::{AudioOutput, PlaybackProblem};
use crate::player::{PlayerRequest, PlayerStatus};
use crate::settings::{FolderTreeMemory, Settings};
use crate::state::Session;
use crate::tag_flow::TagPanel;
use crate::tag_settings::{ProgramChoice, TagSettings};
use crate::tag_writing;
use crate::tag_writing::testing::{self as tag_testing, ScratchFolder};
use crate::theme::{AppTheme, Layout, ThemeName};
use crate::track_overview::{EnergyCurve, EnergyStep};
use crate::views::sidebar::{disclosure_id, folder_row_id};
use crate::views::tag_panel::ProblemList;
use crate::views::track_table::{edge_id, header_id};
use crate::waveforms::Waveform;

/// Index of the BPM column among the table's column headers.
const BPM_COLUMN: usize = 5;

/// Index of the Energy column among the table's column headers.
const ENERGY_COLUMN: usize = 6;

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
pub(crate) fn open_fixture_library(
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
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
            key_runner_up: None,
            key_score_margin: None,
            tempo_estimate: None,
            tempo_score_margin: None,
            beat_grid: None,
            key_timeline: Vec::new(),
            quality: None,
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
        let failed = app.session.library().counts().needs_review;
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
const FOLDER_COLUMN: usize = 10;

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
                tuning_cents: None,
                key_runner_up: None,
                key_score_margin: None,
                tempo_estimate: None,
                tempo_score_margin: None,
                beat_grid: None,
                key_timeline: Vec::new(),
                quality: None,
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
            (counts.total, counts.needs_review)
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
const FORMAT_COLUMN: usize = 9;

/// A window on scratch copies of the core's test files, each analyzed with
/// a key, a tempo and an energy rating, writing its tag backups into the
/// scratch folder.
fn open_taggable_files(
    scratch: &ScratchFolder,
    files: &[PathBuf],
    settings_path: Option<PathBuf>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let tracks = files
        .iter()
        .map(|path| DemoTrack {
            path: path.clone(),
            outcome: Ok(tag_testing::analysis()),
        })
        .collect();
    let (window, view) = open_window_with(
        tracks,
        Box::new(SilentOutput),
        Settings::default(),
        settings_path,
        USUAL_WINDOW,
        cx,
    );
    let log_directory = scratch.log_directory();
    cx.update(|cx| {
        view.update(cx, |app, _| app.write_log_directory = Some(log_directory));
    });
    (window, view)
}

fn click(window: AnyWindowHandle, id: impl Into<gpui::ElementId>, cx: &mut TestAppContext) {
    let id = id.into();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn preview_file_count(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Option<usize> {
    cx.update(|cx| match &view.read(cx).tag_panel {
        Some(TagPanel::Preview(preview)) => Some(preview.plans.len()),
        _ => None,
    })
}

#[gpui::test]
fn writing_tags_shows_a_preview_writes_only_on_confirm_and_undo_restores_every_byte(
    cx: &mut TestAppContext,
) {
    let scratch = ScratchFolder::new("ui-write");
    let files = scratch.copy_fixtures();
    let before = tag_testing::contents(&files);
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);

    click(window, "write-tags", cx);
    assert_eq!(preview_file_count(&view, cx), Some(1), "the selected track");
    click(window, "scope-shown", cx);
    assert_eq!(preview_file_count(&view, cx), Some(3));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("tag-preview-row", 0_usize)).is_some());
        assert!(window.try_find(("tag-preview-row", 1_usize)).is_some());
    })
    .unwrap();
    assert_eq!(
        tag_testing::contents(&files),
        before,
        "a preview writes nothing"
    );

    click(window, "cancel-tag-write", cx);
    cx.update(|cx| assert!(view.read(cx).tag_panel.is_none()));
    assert_eq!(
        tag_testing::contents(&files),
        before,
        "cancel writes nothing"
    );
    assert!(!scratch.log_directory().exists());

    click(window, "write-tags", cx);
    click(window, "scope-shown", cx);
    click(window, "confirm-tag-write", cx);
    let written = cx.update(|cx| view.read(cx).written_files());
    assert_eq!(written, Some(files.clone()));
    assert_ne!(tag_testing::contents(&files), before);

    click(window, "undo-tag-write", cx);
    cx.update(|cx| {
        assert!(matches!(
            view.read(cx).tag_panel,
            Some(TagPanel::ConfirmUndo)
        ));
    });
    assert_ne!(tag_testing::contents(&files), before, "undo asks first");
    click(window, "confirm-tag-undo", cx);
    cx.update(|cx| match &view.read(cx).tag_panel {
        Some(TagPanel::Undone(Ok(Some(report)))) => {
            assert_eq!(report.restored.len(), 3);
            assert!(report.failures.is_empty());
        }
        other => panic!("{other:?}"),
    });
    assert_eq!(tag_testing::contents(&files), before);
    click(window, "close-tag-panel", cx);
    cx.update(|cx| assert!(view.read(cx).tag_panel.is_none()));
}

#[gpui::test]
fn the_app_undoes_a_write_the_command_line_made(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("ui-shared-log");
    let files = scratch.copy_fixtures();
    let before = tag_testing::contents(&files);
    let targets: Vec<tag_writing::WriteTarget> = files
        .iter()
        .map(|path| tag_writing::WriteTarget {
            path: path.clone(),
            analysis: Some(tag_testing::analysis()),
            correction: euphoria_core::Correction::default(),
        })
        .collect();
    let planned = tag_writing::plan_writes(&targets, &TagSettings::default().write_options());
    let mut log = euphoria_core::tags::WriteLog::new(&scratch.log_directory());
    for plan in &planned.plans {
        assert!(matches!(
            log.apply(plan),
            Ok(euphoria_core::tags::LoggedWrite::Recorded)
        ));
    }
    assert_ne!(tag_testing::contents(&files), before);

    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    cx.update(|cx| view.update(cx, |app, cx| app.open_tag_undo(cx)));
    click(window, "confirm-tag-undo", cx);
    assert_eq!(tag_testing::contents(&files), before);
}

#[gpui::test]
fn files_left_out_of_a_write_are_listed_with_the_reason(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("ui-left-out");
    let mut files = scratch.copy_fixtures();
    files.push(scratch.0.join("music/gone.mp3"));
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    click(window, "write-tags", cx);
    click(window, "scope-shown", cx);
    cx.update(|cx| match &view.read(cx).tag_panel {
        Some(TagPanel::Preview(preview)) => {
            assert_eq!(preview.plans.len(), 3);
            assert_eq!(preview.failures.len(), 1);
            assert_eq!(preview.failures[0].path, files[3]);
        }
        other => panic!("{other:?}"),
    });
}

#[gpui::test]
fn opening_settings_discards_a_tag_preview_still_being_planned(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("ui-settings-during-plan");
    let files = scratch.copy_fixtures();
    let before = tag_testing::contents(&files);
    let (_, view) = open_taggable_files(&scratch, &files, None, cx);

    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.plan_tag_write(crate::tag_flow::WriteScope::Shown, cx);
            app.open_settings(cx);
            assert!(app.settings_open);
            assert!(app.tag_panel.is_none());
        });
    });
    cx.run_until_parked();

    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.settings_open);
        assert!(app.tag_panel.is_none(), "a dismissed preview reopened");
    });
    assert_eq!(tag_testing::contents(&files), before);
    assert!(!scratch.log_directory().exists());
}

#[gpui::test]
fn the_settings_choose_the_program_shown_beside_write_tags_and_are_remembered(
    cx: &mut TestAppContext,
) {
    let scratch = ScratchFolder::new("ui-settings");
    let files = scratch.copy_fixtures();
    let settings_path = scratch.0.join("settings.json");
    let (window, view) = open_taggable_files(&scratch, &files, Some(settings_path.clone()), cx);

    click(window, "tag-program", cx);
    cx.update(|cx| assert!(view.read(cx).settings_open));
    click(window, ("program", 1_usize), cx);
    click(window, ("notation", 2_usize), cx);
    click(window, ("label-place", 2_usize), cx);
    cx.update(|cx| {
        let settings = view.read(cx).tag_settings;
        assert_eq!(settings.program, ProgramChoice::Serato);
        assert!(!settings.write_tempo, "Serato starts without the tempo");
    });
    click(window, "write-tempo", cx);
    click(window, "close-settings", cx);
    cx.update(|cx| assert!(!view.read(cx).settings_open));

    let saved = Settings::load(&settings_path).unwrap().tags;
    assert_eq!(
        saved,
        TagSettings {
            program: ProgramChoice::Serato,
            key_notation: crate::tag_settings::NotationChoice::OpenKey,
            label: crate::tag_settings::LabelChoice::Grouping,
            write_tempo: true,
        }
    );

    click(window, "write-tags", cx);
    cx.update(|cx| match &view.read(cx).tag_panel {
        Some(TagPanel::Preview(preview)) => {
            assert_eq!(preview.settings.program, ProgramChoice::Serato);
            let fields: Vec<&str> = preview
                .plans
                .iter()
                .flat_map(|plan| plan.changes())
                .map(|change| change.field.as_str())
                .collect();
            assert!(fields.contains(&"TBPM"), "{fields:?}");
            assert!(fields.contains(&"TIT1"), "{fields:?}");
        }
        other => panic!("{other:?}"),
    });
    click(window, "change-tag-settings", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.settings_open);
        assert!(app.tag_panel.is_none());
    });
}

#[gpui::test]
fn command_click_and_shift_click_select_several_tracks_to_write(cx: &mut TestAppContext) {
    let (window, view) = open_library(6, cx);
    let shown = cx.update(|cx| view.read(cx).visible.clone());
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(track(shown[0]), cx);
        window.click_with_modifiers(track(shown[2]), Modifiers::secondary_key(cx), cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).selected_tracks(), [shown[0], shown[2]]));

    cx.update_window(window, |_, window, cx| {
        window.click_with_modifiers(track(shown[4]), Modifiers::shift(), cx);
    })
    .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).selected_tracks(), shown[2..=4]));

    cx.update_window(window, |_, window, cx| window.click(track(shown[1]), cx))
        .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).selected_tracks(), [shown[1]]));

    cx.update(|cx| view.update(cx, |app, cx| app.select_all_shown(cx)));
    cx.update(|cx| assert_eq!(view.read(cx).selected_tracks(), shown));
}

/// Presses on the grip after `column` and lets go `by` further right.
fn drag_column_edge(window: AnyWindowHandle, column: ColumnId, by: f32, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let grip = window.find(edge_id(column)).bounds().center();
        window.drag(grip, gpui::point(grip.x + px(by), grip.y), cx);
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui::test]
fn dragging_a_header_edge_resizes_that_column_and_is_remembered(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("ui-columns");
    let settings_path = scratch.0.join("settings.json");
    let (window, view) = open_window_with(
        demo::tracks(20),
        Box::new(SilentOutput),
        Settings {
            panels: Panels {
                sidebar_folded: true,
                ..Panels::default()
            },
            ..Settings::default()
        },
        Some(settings_path.clone()),
        size(px(1800.0), px(900.0)),
        cx,
    );
    let before = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.find(header_id(ColumnId::Title)).bounds()
        })
        .unwrap();
    drag_column_edge(window, ColumnId::Title, 120.0, cx);
    let after = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.find(header_id(ColumnId::Title)).bounds()
        })
        .unwrap();
    assert_eq!(after.left(), before.left());
    assert_eq!(after.size.width, before.size.width + px(120.0));
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.column_resize, None);
        assert_eq!(
            app.column_widths.get(ColumnId::Title),
            Some(before.size.width / px(1.0) + 120.0)
        );
    });
    let saved = Settings::load(&settings_path).unwrap();
    assert_eq!(
        saved.column_widths.get(ColumnId::Title),
        Some(before.size.width / px(1.0) + 120.0)
    );

    drag_column_edge(window, ColumnId::Bpm, -500.0, cx);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).column_widths.get(ColumnId::Bpm),
            Some(ColumnId::Bpm.minimum())
        );
    });

    cx.update(|cx| view.update(cx, |app, cx| app.reset_column_widths(cx)));
    assert_eq!(
        Settings::load(&settings_path).unwrap().column_widths,
        crate::columns::ColumnWidths::default()
    );
}

#[gpui::test]
fn a_double_click_on_a_header_edge_fits_the_column_to_its_longest_text(cx: &mut TestAppContext) {
    let mut tracks = demo::tracks(5);
    tracks[3].path = PathBuf::from(
        "/Demo/Promos/An extended title that is much longer than the column (Original Mix).flac",
    );
    let (window, view) = open_window_with(
        tracks,
        Box::new(SilentOutput),
        Settings::default(),
        None,
        USUAL_WINDOW,
        cx,
    );
    let (fitted, needed) = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let needed = crate::app::text_width(
                window,
                "An extended title that is much longer than the column (Original Mix).flac",
                crate::views::TEXT_SIZE,
            );
            window.double_click(edge_id(ColumnId::Title), cx);
            window.render_frame(cx);
            (
                window.find(header_id(ColumnId::Title)).bounds().size.width,
                needed,
            )
        })
        .unwrap();
    assert!(fitted >= px(needed), "{fitted:?} for {needed}");
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).column_widths.get(ColumnId::Title),
            Some(fitted / px(1.0))
        );
    });
    cx.update_window(window, |_, window, _| {
        assert!(window.try_find(("title-cell", 3_usize)).is_some());
    })
    .unwrap();
}

#[gpui::test]
fn fitting_a_column_keeps_shorter_titles_after_many_long_names(cx: &mut TestAppContext) {
    let mut tracks = demo::tracks(42);
    for (index, track) in tracks.iter_mut().enumerate() {
        track.path = PathBuf::from(format!("/Demo/{}-{index}.mp3", "i".repeat(80)));
    }
    let wide_title = format!("{}.mp3", "W".repeat(30));
    tracks[41].path = Path::new("/Demo").join(&wide_title);
    let (_, view) = open_window_on(tracks, Box::new(SilentOutput), cx);

    cx.update(|cx| {
        let candidates = view.read(cx).column_texts(ColumnId::Title);
        assert_eq!(candidates.len(), 42);
        assert!(
            candidates.contains(&wide_title),
            "a shorter title can be wider in the table's proportional font"
        );
    });
}

/// The same check as the scratch fixture test, on copies of real two minute
/// benchmark clips. They are not in the repository, so this runs only by
/// hand: `cargo test -p euphoria -- --ignored bench_clips`, with
/// `EUPHORIA_BENCH_CLIPS` naming the folder when it is elsewhere.
#[gpui::test]
#[ignore = "needs the benchmark clips in bench/data/key/audio"]
fn writing_and_undoing_tags_on_bench_clips_restores_every_byte(cx: &mut TestAppContext) {
    let clips = std::env::var_os("EUPHORIA_BENCH_CLIPS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bench/data/key/audio")
        });
    let mut names: Vec<PathBuf> = std::fs::read_dir(&clips)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    names.sort();
    let scratch = ScratchFolder::new("ui-bench");
    let music = scratch.0.join("music");
    std::fs::create_dir_all(&music).unwrap();
    let files: Vec<PathBuf> = names
        .iter()
        .take(12)
        .map(|clip| {
            let copy = music.join(clip.file_name().unwrap());
            std::fs::copy(clip, &copy).unwrap();
            copy
        })
        .collect();
    let before = tag_testing::contents(&files);
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    click(window, "write-tags", cx);
    click(window, "scope-shown", cx);
    assert_eq!(preview_file_count(&view, cx), Some(files.len()));
    click(window, "confirm-tag-write", cx);
    assert_eq!(
        cx.update(|cx| view.read(cx).written_files())
            .map(|written| written.len()),
        Some(files.len())
    );
    assert_ne!(tag_testing::contents(&files), before);
    click(window, "undo-tag-write", cx);
    click(window, "confirm-tag-undo", cx);
    assert_eq!(tag_testing::contents(&files), before);
}

#[gpui::test]
fn titles_and_artists_are_read_from_the_tags_off_the_main_thread(cx: &mut TestAppContext) {
    let tagged =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../euphoria-core/tests/tagged-fixtures");
    let untagged = Path::new(env!("CARGO_MANIFEST_DIR")).join("../euphoria-core/tests/fixtures");
    let paths = vec![
        tagged.join("tagged.mp3"),
        tagged.join("tagged.flac"),
        untagged.join("sine-440hz-mono.wav"),
    ];
    let tracks = paths
        .iter()
        .map(|path| DemoTrack {
            path: path.clone(),
            outcome: Ok(tag_testing::analysis()),
        })
        .collect();
    let (window, view) = open_window_with(
        tracks,
        Box::new(SilentOutput),
        Settings::default(),
        None,
        USUAL_WINDOW,
        cx,
    );
    cx.update(|cx| view.update(cx, |app, cx| app.read_track_names(paths.clone(), cx)));
    cx.run_until_parked();
    cx.update(|cx| {
        let library = view.read(cx).session.library();
        let names: Vec<(&str, Option<&str>)> = library
            .tracks()
            .iter()
            .map(|track| (track.title(), track.artist.as_deref()))
            .collect();
        assert_eq!(
            names,
            [
                ("Low Tide", Some("Saffron & Co")),
                ("Low Tide", Some("Saffron & Co")),
                ("sine-440hz-mono.wav", None),
            ]
        );
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(header_id(ColumnId::Artist), cx);
    })
    .unwrap();
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(
            app.sort_order.map(|order| order.column),
            Some(SortColumn::Artist)
        );
        assert_eq!(app.visible.last(), Some(&2));
    });
}

#[gpui::test]
fn the_shortcuts_open_the_panels_and_escape_closes_them(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("ui-escape");
    let files = scratch.copy_fixtures();
    let before = tag_testing::contents(&files);
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    let (settings_key, write_key) = if cfg!(target_os = "macos") {
        ("cmd-,", "cmd-shift-t")
    } else {
        ("ctrl-,", "ctrl-shift-t")
    };
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press(settings_key, cx);
    })
    .unwrap();
    cx.update(|cx| assert!(view.read(cx).settings_open));
    cx.update_window(window, |_, window, cx| window.press("escape", cx))
        .unwrap();
    cx.update(|cx| assert!(!view.read(cx).settings_open));

    cx.update_window(window, |_, window, cx| window.press(write_key, cx))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(preview_file_count(&view, cx), Some(1));
    cx.update_window(window, |_, window, cx| window.press("escape", cx))
        .unwrap();
    cx.update(|cx| assert!(view.read(cx).tag_panel.is_none()));
    assert_eq!(tag_testing::contents(&files), before);
}

#[gpui::test]
fn files_written_from_the_window_are_served_from_the_cache_on_the_next_launch(
    cx: &mut TestAppContext,
) {
    let scratch = ScratchFolder::new("ui-cache");
    let files = scratch.copy_fixtures();
    let cache_path = scratch.0.join("analysis-cache.sqlite3");
    let launch = || {
        euphoria_core::CachedAnalyzer::new(euphoria_core::Cache::open(&cache_path).unwrap())
            .unwrap()
    };
    let first_launch = std::sync::Arc::new(launch());
    let tracks: Vec<DemoTrack> = files
        .iter()
        .map(|path| DemoTrack {
            path: path.clone(),
            outcome: Ok(first_launch.analyze_file(path).unwrap().analysis),
        })
        .collect();
    let analyses: Vec<TrackAnalysis> = tracks
        .iter()
        .map(|track| track.outcome.clone().unwrap())
        .collect();

    let log_directory = scratch.log_directory();
    let (window, view) = cx.update(|cx| {
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
                let mut session = Session::start(Some(first_launch.clone())).unwrap();
                session.add_demo_tracks(tracks);
                let mut app = EuphoriaApp::new(
                    session,
                    Settings::default(),
                    None,
                    None,
                    Box::new(SilentOutput),
                    window,
                    cx,
                );
                app.write_log_directory = Some(log_directory);
                app
            })
        })
        .unwrap();
        crate::register_commands(window, &view, cx);
        (window, view)
    });

    click(window, "write-tags", cx);
    click(window, "scope-shown", cx);
    click(window, "confirm-tag-write", cx);
    assert_eq!(
        cx.update(|cx| view.read(cx).written_files()),
        Some(files.clone())
    );
    let after_write = launch();
    for (path, analysis) in files.iter().zip(&analyses) {
        assert_eq!(after_write.stored_analysis(path).as_ref(), Some(analysis));
    }

    click(window, "undo-tag-write", cx);
    click(window, "confirm-tag-undo", cx);
    let after_undo = launch();
    for (path, analysis) in files.iter().zip(&analyses) {
        assert_eq!(after_undo.stored_analysis(path).as_ref(), Some(analysis));
    }
}

#[gpui::test]
fn a_long_list_of_files_left_out_builds_only_the_lines_in_view_and_scrolls_to_every_one(
    cx: &mut TestAppContext,
) {
    let scratch = ScratchFolder::new("ui-many-failures");
    let files: Vec<PathBuf> = (0..300)
        .map(|number| scratch.0.join(format!("music/gone {number:03}.mp3")))
        .collect();
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    click(window, "write-tags", cx);
    click(window, "scope-shown", cx);
    cx.update(|cx| match &view.read(cx).tag_panel {
        Some(panel) => assert_eq!(ProblemList::CannotBeWritten.files(panel).len(), 300),
        None => panic!("no preview"),
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(("cannot-be-written", 0_usize)).is_some());
        assert!(window.try_find(("cannot-be-written", 299_usize)).is_none());
        window.scroll(
            "cannot-be-written-section",
            gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(-100_000.0))),
            cx,
        );
        window.render_frame(cx);
        assert!(window.try_find(("cannot-be-written", 299_usize)).is_some());
    })
    .unwrap();
}

#[gpui::test]
fn opening_undo_discards_a_tag_preview_still_being_planned(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("ui-undo-during-plan");
    let files = scratch.copy_fixtures();
    let (_, view) = open_taggable_files(&scratch, &files, None, cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.plan_tag_write(crate::tag_flow::WriteScope::Shown, cx);
            app.open_tag_undo(cx);
        });
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(matches!(
            view.read(cx).tag_panel,
            Some(TagPanel::ConfirmUndo)
        ));
    });
}

#[gpui::test]
fn fitting_a_column_measures_each_distinct_text_once(cx: &mut TestAppContext) {
    let (_, view) = open_library(500, cx);
    cx.update(|cx| {
        let app = view.read(cx);
        let numbers = app.column_texts(ColumnId::Number);
        assert_eq!(numbers.len(), 1);
        assert!(numbers.contains("500"));
        let formats = app.column_texts(ColumnId::Format);
        assert!(formats.len() <= 4, "{formats:?}");
        let titles = app.column_texts(ColumnId::Title);
        let distinct_titles: HashSet<&str> = app
            .visible
            .iter()
            .map(|index| app.session.library().tracks()[*index].title())
            .collect();
        assert_eq!(titles.len(), distinct_titles.len());
    });
}

/// One fixture track that changes key: A minor for its first 40 seconds,
/// silence, then E minor from 50 seconds to its end at 100.
fn track_changing_key() -> DemoTrack {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../euphoria-core/tests/fixtures");
    DemoTrack {
        path: fixtures.join("sine-440hz-mono.flac"),
        outcome: Ok(TrackAnalysis {
            duration_seconds: 100.0,
            quality: None,
            key: Some(euphoria_core::Key::new(9, euphoria_core::Mode::Minor)),
            bpm: Some(124.0),
            energy: Some(6),
            tuning_cents: None,
            key_runner_up: None,
            key_score_margin: None,
            tempo_estimate: None,
            tempo_score_margin: None,
            beat_grid: None,
            key_timeline: vec![
                euphoria_core::KeySection {
                    start_seconds: 0.0,
                    end_seconds: 40.0,
                    key: euphoria_core::Key::new(9, euphoria_core::Mode::Minor),
                },
                euphoria_core::KeySection {
                    start_seconds: 50.0,
                    end_seconds: 100.0,
                    key: euphoria_core::Key::new(4, euphoria_core::Mode::Minor),
                },
            ],
        }),
    }
}

/// A waveform with an energy curve that dips in the middle.
fn waveform_with_energy() -> Waveform {
    let columns: Vec<crate::peaks::BandPeaks> = (0..200)
        .map(|_| crate::peaks::BandPeaks {
            low: 0.5,
            mid: 0.4,
            high: 0.2,
        })
        .collect();
    let steps = vec![
        EnergyStep {
            start: 0.0,
            end: 0.4,
            rating: Some(9),
        },
        EnergyStep {
            start: 0.4,
            end: 0.6,
            rating: None,
        },
        EnergyStep {
            start: 0.6,
            end: 1.0,
            rating: Some(10),
        },
    ];
    Waveform::Ready(columns.into(), Some(EnergyCurve::Steps(steps.into())))
}

fn open_track_changing_key(
    theme: ThemeName,
    panels: Panels,
    window_size: gpui::Size<gpui::Pixels>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let (window, view) = open_window_with(
        vec![track_changing_key()],
        Box::new(SilentOutput),
        Settings {
            theme,
            panels,
            ..Settings::default()
        },
        None,
        window_size,
        cx,
    );
    cx.update(|cx| view.update(cx, |app, _| app.show_waveform(waveform_with_energy())));
    (window, view)
}

#[gpui::test]
fn the_energy_curve_and_the_key_timeline_line_up_under_the_waveform_in_both_layouts(
    cx: &mut TestAppContext,
) {
    for theme in [ThemeName::Slate, ThemeName::Deck] {
        let (window, _) = open_track_changing_key(theme, Panels::default(), USUAL_WINDOW, cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            let waveform = window.find("waveform").bounds();
            let energy = window.find("energy-curve").bounds();
            let timeline = window.find("key-timeline").bounds();
            for under in [energy, timeline] {
                assert_eq!(under.left(), waveform.left(), "{theme:?}");
                assert_eq!(under.right(), waveform.right(), "{theme:?}");
            }
            assert!(energy.top() >= waveform.bottom(), "{theme:?}");
            assert!(timeline.top() >= energy.bottom(), "{theme:?}");

            let first = window.find(("key-stretch", 0_usize)).bounds();
            let second = window.find(("key-stretch", 1_usize)).bounds();
            let at = |fraction: f32| waveform.left() + waveform.size.width * fraction;
            assert!((first.left() - at(0.0)).abs() < px(1.0), "{theme:?}");
            assert!((first.right() - at(0.4)).abs() < px(1.0), "{theme:?}");
            assert!((second.left() - at(0.5)).abs() < px(1.0), "{theme:?}");
            assert!(second.left() > first.right(), "the silence stays a gap");
            if theme == ThemeName::Deck {
                let strip = window.find("inspector").bounds();
                assert!(
                    timeline.bottom() <= strip.bottom(),
                    "{timeline:?} in {strip:?}"
                );
            }
        })
        .unwrap();
    }
}

#[gpui::test]
fn a_track_in_one_key_has_no_timeline_and_no_curve_shows_before_it_is_worked_out(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_fixture_library(cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.show_waveform(Waveform::Ready(
                vec![crate::peaks::BandPeaks {
                    low: 0.5,
                    mid: 0.5,
                    high: 0.5,
                }]
                .into(),
                None,
            ));
        });
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("waveform").is_some());
        assert!(window.try_find("energy-curve").is_none());
        assert!(window.try_find("key-timeline").is_none());
    })
    .unwrap();
}

#[gpui::test]
fn a_curve_the_core_could_not_work_out_says_so_quietly_and_an_empty_one_shows_nothing(
    cx: &mut TestAppContext,
) {
    let columns = || -> std::sync::Arc<[crate::peaks::BandPeaks]> {
        vec![crate::peaks::BandPeaks {
            low: 0.5,
            mid: 0.5,
            high: 0.5,
        }]
        .into()
    };
    let (window, view) = open_fixture_library(cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.show_waveform(Waveform::Ready(
                columns(),
                Some(EnergyCurve::Unavailable(
                    "a sample rate of 4000 Hz is not supported".to_string(),
                )),
            ));
        });
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let waveform = window.find("waveform").bounds();
        let unavailable = window.find("energy-unavailable").bounds();
        assert!(unavailable.top() >= waveform.bottom());
        assert!(window.try_find("energy-curve").is_none());
    })
    .unwrap();

    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.show_waveform(Waveform::Ready(
                columns(),
                Some(EnergyCurve::Steps(Vec::new().into())),
            ));
        });
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("energy-unavailable").is_none());
        assert!(window.try_find("energy-curve").is_none());
    })
    .unwrap();
}

#[gpui::test]
fn a_timeline_that_contradicts_the_key_of_the_track_is_not_shown(cx: &mut TestAppContext) {
    let mut track = track_changing_key();
    if let Ok(analysis) = &mut track.outcome {
        analysis.key = Some(euphoria_core::Key::new(3, euphoria_core::Mode::Minor));
    }
    let (window, view) = open_window_on(vec![track], Box::new(SilentOutput), cx);
    cx.update(|cx| view.update(cx, |app, _| app.show_waveform(waveform_with_energy())));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("energy-curve").is_some());
        assert!(window.try_find("key-timeline").is_none());
    })
    .unwrap();
}

#[gpui::test]
fn the_strip_at_its_shortest_still_holds_the_waveform_the_curve_and_the_timeline(
    cx: &mut TestAppContext,
) {
    let shortest = Panels {
        strip_height: 0,
        ..Panels::default()
    };
    for window_size in [USUAL_WINDOW, SMALLEST_WINDOW] {
        let (window, _) = open_track_changing_key(ThemeName::Deck, shortest, window_size, cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            let strip = window.find("inspector").bounds();
            assert_eq!(strip.size.height, px(124.0));
            let inside_bottom = strip.bottom() - px(16.0);
            for part in ["waveform", "energy-curve", "key-timeline", "play-button"] {
                let bounds = window.find(part).bounds();
                assert!(
                    bounds.top() >= strip.top(),
                    "{part}: {bounds:?} in {strip:?}"
                );
                assert!(
                    bounds.bottom() <= inside_bottom + px(0.5),
                    "{part}: {bounds:?} in {strip:?}"
                );
            }
        })
        .unwrap();
    }
}

#[gpui::test]
fn the_curve_is_worked_out_from_the_decoded_audio_after_the_waveform(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    for _ in 0..20 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(200));
        cx.run_until_parked();
    }
    cx.update(|cx| match view.read(cx).selected_waveform() {
        Some(Waveform::Ready(columns, Some(_))) => assert!(!columns.is_empty()),
        other => panic!("{other:?}"),
    });
}

#[gpui::test]
fn the_key_first_wheel_shows_both_rings_around_the_selected_key(cx: &mut TestAppContext) {
    let (window, _) =
        open_track_changing_key(ThemeName::KeyFirst, Panels::default(), USUAL_WINDOW, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let wheel = window.find("camelot-wheel").bounds();
        assert_eq!(wheel.size.width, px(crate::views::wheel::WHEEL_SIZE));
        assert!(window.find("inspector").bounds().contains(&wheel.center()));
    })
    .unwrap();
}

/// An analysis rated by the core's confidence rules: a key margin of 0.02
/// is high, 0.005 medium and 0.001 low; a tempo margin of 0.6 is high and
/// 0.01 low.
fn rated_analysis(key_margin: f32, tempo_estimate: f32, tempo_margin: f32) -> TrackAnalysis {
    TrackAnalysis {
        duration_seconds: 300.0,
        key: Some(euphoria_core::Key::new(9, euphoria_core::Mode::Minor)),
        bpm: Some(tempo_estimate),
        energy: Some(6),
        tuning_cents: None,
        key_timeline: Vec::new(),
        quality: None,
        key_runner_up: Some(euphoria_core::Key::new(4, euphoria_core::Mode::Minor)),
        key_score_margin: Some(key_margin),
        tempo_estimate: Some(tempo_estimate),
        tempo_score_margin: Some(tempo_margin),
        beat_grid: None,
    }
}

/// Four made up tracks: one rated high, one with a doubtful key, one with
/// a doubtful tempo counted at half time, and one that failed.
fn tracks_to_review() -> Vec<DemoTrack> {
    let track = |name: &str, outcome| DemoTrack {
        path: PathBuf::from(format!("/Demo/review/{name}.mp3")),
        outcome,
    };
    vec![
        track("sure", Ok(rated_analysis(0.02, 124.0, 0.6))),
        track("doubtful key", Ok(rated_analysis(0.005, 124.0, 0.6))),
        track("doubtful tempo", Ok(rated_analysis(0.02, 87.0, 0.01))),
        track("broken", Err("cannot decode".to_string())),
    ]
}

fn open_tracks_to_review(
    settings_path: Option<PathBuf>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_window_with(
        tracks_to_review(),
        Box::new(SilentOutput),
        Settings::default(),
        settings_path,
        USUAL_WINDOW,
        cx,
    )
}

#[gpui::test]
fn needs_review_lists_doubtful_keys_and_tempos_and_failed_files_and_says_why(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_tracks_to_review(None, cx);
    cx.update(|cx| assert_eq!(view.read(cx).sidebar_counts().needs_review, 2));
    click(window, "needs-review", cx);
    assert_eq!(
        shown_titles(&view, cx),
        ["doubtful tempo.mp3", "broken.mp3"],
        "the doubtful key is rated medium, which is not low"
    );
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("review-explanation").is_some());
    })
    .unwrap();
    click(window, "all-tracks", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("review-explanation").is_none());
    })
    .unwrap();
}

#[gpui::test]
fn the_key_box_shows_its_confidence_and_the_runner_up_only_when_the_key_is_not_high(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_tracks_to_review(None, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("key-confidence").is_some());
        assert!(window.try_find("tempo-confidence").is_some());
        assert!(
            window.try_find("runner-up-key").is_none(),
            "a key rated high"
        );
        window.click(track(1), cx);
        window.render_frame(cx);
        assert!(
            window.try_find("runner-up-key").is_some(),
            "a key rated medium"
        );
        let key_box = window.find("key-confidence").bounds();
        assert!(
            window
                .find("inspector")
                .bounds()
                .contains(&key_box.center())
        );
    })
    .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).selected, Some(1)));
}

#[gpui::test]
fn a_tempo_range_preset_moves_the_tempos_shown_and_is_remembered(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("ui-tempo-range");
    let settings_path = scratch.0.join("settings.json");
    let (window, view) = open_tracks_to_review(Some(settings_path.clone()), cx);
    let tempo_of_half_time = |view: &Entity<EuphoriaApp>, cx: &mut TestAppContext| {
        cx.update(|cx| view.read(cx).session.library().tracks()[2].bpm())
    };
    assert_eq!(tempo_of_half_time(&view, cx), Some(174.0));

    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    click(window, ("tempo-preset", 1_usize), cx);
    assert_eq!(tempo_of_half_time(&view, cx), Some(87.0));
    let saved = Settings::load(&settings_path).unwrap().display.tempo_range;
    assert_eq!((saved.slowest, saved.fastest), (70, 140));

    click(window, ("tempo-preset", 0_usize), cx);
    assert_eq!(tempo_of_half_time(&view, cx), Some(174.0));
}

/// Replaces the text of the field `id` with `text`.
fn retype(window: AnyWindowHandle, id: &'static str, text: &str, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
        window.press(
            if cfg!(target_os = "macos") {
                "cmd-a"
            } else {
                "ctrl-a"
            },
            cx,
        );
        window.input(text, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui::test]
fn a_custom_tempo_range_is_used_when_valid_and_explained_when_not(cx: &mut TestAppContext) {
    let (window, view) = open_tracks_to_review(None, cx);
    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    click(window, "tempo-custom", cx);
    retype(window, "tempo-from", "80", cx);
    retype(window, "tempo-to", "160", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.session.library().tempo_range().slowest(), 80.0);
        assert_eq!(app.session.library().tempo_range().fastest(), 160.0);
        assert_eq!(app.session.library().tracks()[2].bpm(), Some(87.0));
        assert!(app.tempo_range_problem.is_none());
    });

    retype(window, "tempo-to", "200", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(
            app.tempo_range_problem
                .as_deref()
                .is_some_and(|problem| problem.contains("octave")),
            "{:?}",
            app.tempo_range_problem
        );
        assert_eq!(
            app.session.library().tempo_range().fastest(),
            160.0,
            "the last range that worked stays in use"
        );
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("tempo-range-problem").is_some());
    })
    .unwrap();
}

#[gpui::test]
fn the_key_display_setting_spells_keys_in_the_table_and_is_remembered(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("ui-key-display");
    let settings_path = scratch.0.join("settings.json");
    let (window, view) = open_tracks_to_review(Some(settings_path.clone()), cx);
    cx.update(|cx| {
        let codes = view.read(cx).column_texts(ColumnId::Key);
        assert!(codes.contains("8A"), "{codes:?}");
    });
    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    click(window, ("key-display", 2_usize), cx);
    cx.update(|cx| {
        let codes = view.read(cx).column_texts(ColumnId::Key);
        assert!(codes.contains("1m"), "{codes:?}");
    });
    assert_eq!(
        Settings::load(&settings_path).unwrap().display.key_notation,
        crate::tag_settings::NotationChoice::OpenKey
    );
}

#[gpui::test]
fn custom_starts_from_the_preset_in_use_so_editing_one_field_keeps_the_other(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_tracks_to_review(None, cx);
    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    click(window, ("tempo-preset", 1_usize), cx);
    click(window, "tempo-custom", cx);
    let typed = |view: &Entity<EuphoriaApp>, cx: &mut TestAppContext| {
        cx.update(|cx| {
            let app = view.read(cx);
            (
                app.tempo_from_input.read(cx).value().to_string(),
                app.tempo_to_input.read(cx).value().to_string(),
            )
        })
    };
    assert_eq!(typed(&view, cx), ("70".to_string(), "140".to_string()));

    retype(window, "tempo-from", "75", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        let range = app.session.library().tempo_range();
        assert_eq!((range.slowest(), range.fastest()), (75.0, 140.0));
        assert!(app.tempo_range_problem.is_none());
    });

    click(window, ("tempo-preset", 2_usize), cx);
    assert_eq!(typed(&view, cx), ("120".to_string(), "200".to_string()));
}

/// Three made up tracks for a set: A minor at 122, E minor at 124 and
/// F sharp major at 128, with energies 4, 6 and 8.
fn set_tracks() -> Vec<DemoTrack> {
    [
        ("opener", 9, euphoria_core::Mode::Minor, 122.0, 4),
        ("builder", 4, euphoria_core::Mode::Minor, 124.0, 6),
        ("peak", 6, euphoria_core::Mode::Major, 128.0, 8),
    ]
    .into_iter()
    .map(|(name, tonic, mode, bpm, energy)| DemoTrack {
        path: PathBuf::from(format!("/Demo/set/{name}.mp3")),
        outcome: Ok(TrackAnalysis {
            duration_seconds: 300.0,
            key: Some(euphoria_core::Key::new(tonic, mode)),
            bpm: Some(bpm),
            energy: Some(energy),
            tuning_cents: None,
            key_runner_up: None,
            key_score_margin: None,
            tempo_estimate: None,
            tempo_score_margin: None,
            key_timeline: Vec::new(),
            quality: None,
            beat_grid: None,
        }),
    })
    .collect()
}

fn open_set(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_window_on(set_tracks(), Box::new(SilentOutput), cx)
}

/// Makes a playlist named `name` holding the set's tracks named `tracks`,
/// in that order.
fn make_playlist(
    window: AnyWindowHandle,
    view: &Entity<EuphoriaApp>,
    name: &str,
    tracks: &[&str],
    cx: &mut TestAppContext,
) -> crate::playlists::PlaylistId {
    let paths: Vec<PathBuf> = tracks
        .iter()
        .map(|track| PathBuf::from(format!("/Demo/set/{track}.mp3")))
        .collect();
    let id = cx
        .update_window(window, |_, window, cx| {
            view.update(cx, |app, cx| app.create_playlist(window, cx))
        })
        .unwrap();
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.renaming_playlist = None;
            app.session.playlists_mut().rename(id, name);
            app.add_to_playlist(id, &paths, cx);
        })
    });
    cx.run_until_parked();
    id
}

fn playlist_names(
    view: &Entity<EuphoriaApp>,
    id: crate::playlists::PlaylistId,
    cx: &mut TestAppContext,
) -> Vec<String> {
    cx.update(|cx| {
        view.read(cx)
            .session
            .playlists()
            .get(id)
            .unwrap()
            .tracks
            .iter()
            .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
            .collect()
    })
}

fn shown_stems(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<String> {
    shown_titles(view, cx)
        .into_iter()
        .map(|title| title.trim_end_matches(".mp3").to_string())
        .collect()
}

#[gpui::test]
fn a_new_playlist_is_named_in_the_sidebar_and_shown(cx: &mut TestAppContext) {
    let (window, view) = open_set(cx);
    click(window, "new-playlist", cx);
    assert!(is_shown(window, "playlist-name", cx));
    cx.update_window(window, |_, window, cx| {
        window.press(
            if cfg!(target_os = "macos") {
                "cmd-a"
            } else {
                "ctrl-a"
            },
            cx,
        );
        window.input("Opening set", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let id = cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.renaming_playlist, None);
        let playlist = &app.session.playlists().all()[0];
        assert_eq!(playlist.name, "Opening set");
        assert_eq!(app.playlist, Some(playlist.id));
        assert!(app.visible.is_empty());
        playlist.id
    });
    assert!(!is_shown(window, "playlist-name", cx));
    assert!(is_shown(
        window,
        crate::views::playlists::playlist_row_id(id),
        cx
    ));

    click(window, "all-tracks", cx);
    cx.update(|cx| assert_eq!(view.read(cx).playlist, None));
    click(window, crate::views::playlists::playlist_row_id(id), cx);
    cx.update(|cx| assert_eq!(view.read(cx).playlist, Some(id)));
}

#[gpui::test]
fn tracks_dragged_onto_a_playlist_join_it_once_in_table_order(cx: &mut TestAppContext) {
    let (window, view) = open_set(cx);
    let id = make_playlist(window, &view, "Set", &[], cx);
    click(window, "all-tracks", cx);
    let index_of = |name: &str, cx: &mut TestAppContext| {
        cx.update(|cx| {
            view.read(cx)
                .session
                .library()
                .index_of(Path::new(&format!("/Demo/set/{name}.mp3")))
                .unwrap()
        })
    };
    let peak = index_of("peak", cx);
    let opener = index_of("opener", cx);
    let row = crate::views::playlists::playlist_row_id(id);

    click(window, track(opener), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click_with_modifiers(track(peak), Modifiers::secondary_key(cx), cx);
        window.render_frame(cx);
        window.drag_to(track(peak), row, cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(playlist_names(&view, id, cx), ["opener", "peak"]);

    let builder = index_of("builder", cx);
    click(window, track(builder), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.drag_to(track(builder), row, cx);
        window.render_frame(cx);
        window.drag_to(track(builder), row, cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(playlist_names(&view, id, cx), ["opener", "peak", "builder"]);
}

#[gpui::test]
fn a_playlist_numbers_its_tracks_in_order_and_sorting_is_only_a_view(cx: &mut TestAppContext) {
    let (window, view) = open_set(cx);
    let id = make_playlist(window, &view, "Set", &["peak", "opener", "builder"], cx);
    assert_eq!(shown_stems(&view, cx), ["peak", "opener", "builder"]);
    let numbers = cx.update(|cx| {
        let app = view.read(cx);
        app.visible
            .iter()
            .map(|index| app.playlist_positions[index])
            .collect::<Vec<_>>()
    });
    assert_eq!(numbers, [1, 2, 3]);

    click(window, ("column", BPM_COLUMN), cx);
    assert_eq!(shown_stems(&view, cx), ["opener", "builder", "peak"]);
    assert_eq!(playlist_names(&view, id, cx), ["peak", "opener", "builder"]);
    click(window, header_id(ColumnId::Number), cx);
    assert_eq!(shown_stems(&view, cx), ["peak", "opener", "builder"]);

    let (builder, peak) = cx.update(|cx| {
        let app = view.read(cx);
        (app.visible[2], app.visible[0])
    });
    click(window, track(builder), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.drag_to(track(builder), track(peak), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(playlist_names(&view, id, cx), ["builder", "peak", "opener"]);
    assert_eq!(shown_stems(&view, cx), ["builder", "peak", "opener"]);

    cx.update(|cx| view.update(cx, |app, cx| app.remove_selection_from_playlist(cx)));
    assert_eq!(playlist_names(&view, id, cx), ["peak", "opener"]);
}

#[gpui::test]
fn the_set_line_shows_the_key_path_the_tempos_the_energy_and_export(cx: &mut TestAppContext) {
    let (window, view) = open_set(cx);
    make_playlist(window, &view, "Set", &["opener", "builder", "peak"], cx);
    for id in [
        "set-key-path",
        "set-tempo-range",
        "set-energy",
        "export-rekordbox",
        "export-m3u8",
    ] {
        assert!(is_shown(window, id, cx), "{id}");
    }
    click(window, "all-tracks", cx);
    assert!(!is_shown(window, "set-key-path", cx));
}

#[gpui::test]
fn a_playlist_exports_in_its_order_to_rekordbox_and_m3u8(cx: &mut TestAppContext) {
    let (window, view) = open_set(cx);
    let id = make_playlist(window, &view, "Peak time", &["peak", "opener"], cx);
    let scratch = ScratchFolder::new("ui-playlist-export");
    let write = |format, file: &str, cx: &mut TestAppContext| {
        let job = cx.update(|cx| view.read(cx).playlist_export_job(id, format).unwrap());
        let destination = scratch.0.join(file);
        let message = job.write(&destination).unwrap();
        (std::fs::read_to_string(&destination).unwrap(), message)
    };

    let (m3u8, _) = write(crate::playlist_flow::PlaylistFormat::M3u8, "set.m3u8", cx);
    assert_eq!(
        m3u8,
        "#EXTM3U\n#PLAYLIST:Peak time\n#EXTINF:300,peak.mp3\n/Demo/set/peak.mp3\n\
         #EXTINF:300,opener.mp3\n/Demo/set/opener.mp3\n"
    );

    let (xml, message) = write(
        crate::playlist_flow::PlaylistFormat::Rekordbox,
        "set.xml",
        cx,
    );
    assert!(xml.contains("Name=\"Peak time\""), "{xml}");
    assert_eq!(xml.matches("<TRACK TrackID").count(), 2);
    assert!(
        xml.find("peak.mp3").unwrap() < xml.find("opener.mp3").unwrap(),
        "{xml}"
    );
    assert!(message.contains("rekordbox"), "{message}");
}

#[gpui::test]
fn deleting_the_playlist_on_show_goes_back_to_all_tracks(cx: &mut TestAppContext) {
    let (window, view) = open_set(cx);
    let id = make_playlist(window, &view, "Set", &["peak"], cx);
    cx.update(|cx| view.update(cx, |app, cx| app.delete_playlist(id, cx)));
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.playlist, None);
        assert!(app.session.playlists().all().is_empty());
        assert_eq!(app.visible.len(), 3);
    });
}

/// Scratch copies of the test files in a window that keeps its playlists
/// in a real store in the scratch folder.
fn open_with_playlist_store(
    name: &str,
    cx: &mut TestAppContext,
) -> (
    ScratchFolder,
    Vec<PathBuf>,
    PathBuf,
    AnyWindowHandle,
    Entity<EuphoriaApp>,
) {
    let scratch = ScratchFolder::new(name);
    let files = scratch.copy_fixtures();
    let store_path = scratch.0.join("playlists.sqlite3");
    let tracks: Vec<DemoTrack> = fixture_tracks()
        .into_iter()
        .zip(&files)
        .map(|(track, file)| DemoTrack {
            path: file.clone(),
            outcome: track.outcome,
        })
        .collect();
    let (window, view) = cx.update(|cx| {
        gpui::init(cx);
        let (window, view) = gpui::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| {
                let mut session = Session::start(None).unwrap();
                session.add_demo_tracks(tracks);
                session
                    .use_playlist_store(euphoria_core::PlaylistStore::open(&store_path).unwrap())
                    .unwrap();
                EuphoriaApp::new(
                    session,
                    Settings::default(),
                    None,
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
    });
    (scratch, files, store_path, window, view)
}

#[gpui::test]
fn playlists_are_kept_and_found_again_on_the_next_launch(cx: &mut TestAppContext) {
    let (_scratch, files, store_path, window, view) =
        open_with_playlist_store("ui-playlist-store", cx);
    let id = cx
        .update_window(window, |_, window, cx| {
            view.update(cx, |app, cx| app.create_playlist(window, cx))
        })
        .unwrap();
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.renaming_playlist = None;
            app.session.playlists_mut().rename(id, "Kept");
            app.set_playlist_label(id, Some(crate::playlists::LABELS[2]), cx);
            app.add_to_playlist(id, &[files[2].clone(), files[0].clone()], cx);
        })
    });
    cx.run_until_parked();

    let kept = euphoria_core::PlaylistStore::open(&store_path)
        .unwrap()
        .load()
        .unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].name, "Kept");
    assert_eq!(kept[0].label.as_deref(), Some(crate::playlists::LABELS[2]));
    let kept_paths: Vec<&Path> = kept[0]
        .entries
        .iter()
        .map(|entry| entry.path.as_path())
        .collect();
    assert_eq!(kept_paths, [files[2].as_path(), files[0].as_path()]);
}

/// The tracks to review, with an output that notes every reference sound.
fn open_tracks_to_check_by_ear(
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>, ListeningOutput) {
    let output = ListeningOutput::default();
    let (window, view) = open_window_with(
        tracks_to_review(),
        Box::new(output.clone()),
        Settings::default(),
        None,
        USUAL_WINDOW,
        cx,
    );
    (window, view, output)
}

fn selected_key_and_bpm(
    view: &Entity<EuphoriaApp>,
    cx: &mut TestAppContext,
) -> (Option<String>, Option<f32>) {
    cx.update(|cx| {
        let track = view.read(cx).selected_track().unwrap();
        (track.key().map(|key| key.camelot()), track.bpm())
    })
}

fn is_shown(
    window: AnyWindowHandle,
    id: impl Into<gpui::ElementId>,
    cx: &mut TestAppContext,
) -> bool {
    let id = id.into();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
    .unwrap()
}

#[gpui::test]
fn a_track_not_in_the_library_keeps_its_place_in_the_numbering(cx: &mut TestAppContext) {
    let (window, view) = open_set(cx);
    let id = make_playlist(window, &view, "Set", &["gone", "opener", "builder"], cx);
    let numbers = cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.playlist, Some(id));
        app.visible
            .iter()
            .map(|index| app.playlist_positions[index])
            .collect::<Vec<_>>()
    });
    assert_eq!(numbers, [2, 3], "the missing first track still counts as 1");
}

#[gpui::test]
fn a_file_replaced_by_other_audio_is_left_out_of_an_export(cx: &mut TestAppContext) {
    let (scratch, files, _store, window, view) =
        open_with_playlist_store("ui-playlist-replaced", cx);
    let id = cx
        .update_window(window, |_, window, cx| {
            view.update(cx, |app, cx| app.create_playlist(window, cx))
        })
        .unwrap();
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.renaming_playlist = None;
            app.add_to_playlist(id, &[files[0].clone(), files[1].clone()], cx);
        })
    });
    cx.run_until_parked();
    std::fs::copy(&files[2], &files[1]).unwrap();

    let job = cx.update(|cx| {
        view.read(cx)
            .playlist_export_job(id, crate::playlist_flow::PlaylistFormat::M3u8)
            .unwrap()
    });
    let destination = scratch.0.join("set.m3u8");
    let message = job.write(&destination).unwrap();
    let written = std::fs::read_to_string(&destination).unwrap();
    assert!(written.contains(files[0].to_str().unwrap()), "{written}");
    assert!(!written.contains(files[1].to_str().unwrap()), "{written}");
    assert!(message.contains("other audio"), "{message}");
}

#[gpui::test]
fn a_click_takes_the_runner_up_key_and_another_resets_it(cx: &mut TestAppContext) {
    let (window, view, _) = open_tracks_to_check_by_ear(cx);
    click(window, track(1), cx);
    assert_eq!(selected_key_and_bpm(&view, cx).0.as_deref(), Some("8A"));

    click(window, "runner-up-key", cx);
    assert_eq!(selected_key_and_bpm(&view, cx).0.as_deref(), Some("9A"));
    assert!(is_shown(window, "key-set-by-you", cx));
    assert!(!is_shown(window, "key-confidence", cx));
    assert!(
        !is_shown(window, "runner-up-key", cx),
        "the runner up is the key now"
    );
    cx.update(|cx| {
        let correction = view.read(cx).selected_track().unwrap().correction;
        assert_eq!(
            correction.key.map(|key| key.camelot()).as_deref(),
            Some("9A")
        );
        assert_eq!(correction.bpm, None);
    });

    click(window, "key-reset", cx);
    assert_eq!(selected_key_and_bpm(&view, cx).0.as_deref(), Some("8A"));
    assert!(is_shown(window, "runner-up-key", cx));
    assert!(is_shown(window, "key-confidence", cx));
}

#[gpui::test]
fn a_click_takes_the_other_tempo_and_the_track_leaves_needs_review(cx: &mut TestAppContext) {
    let (window, view, _) = open_tracks_to_check_by_ear(cx);
    let needs_review =
        |cx: &mut TestAppContext| cx.update(|cx| view.read(cx).sidebar_counts().needs_review);
    assert_eq!(needs_review(cx), 2);
    click(window, track(2), cx);
    assert_eq!(selected_key_and_bpm(&view, cx).1, Some(174.0));

    click(window, "tempo-alternative", cx);
    assert_eq!(selected_key_and_bpm(&view, cx).1, Some(87.0));
    assert_eq!(needs_review(cx), 1);
    assert!(is_shown(window, "tempo-set-by-you", cx));
    assert!(!is_shown(window, "tempo-alternative", cx));

    click(window, "needs-review", cx);
    assert_eq!(shown_titles(&view, cx), ["broken.mp3"]);
    click(window, "all-tracks", cx);
    cx.update(|cx| view.update(cx, |app, cx| app.select(2, cx)));

    click(window, "tempo-reset", cx);
    assert_eq!(selected_key_and_bpm(&view, cx).1, Some(174.0));
    assert_eq!(needs_review(cx), 2);
}

#[gpui::test]
fn the_key_is_picked_and_the_tempo_typed_by_hand(cx: &mut TestAppContext) {
    let (window, view, _) = open_tracks_to_check_by_ear(cx);
    assert!(!is_shown(window, ("pick-key", 0_usize), cx));
    click(window, "edit-values", cx);
    let typed = cx.update(|cx| view.read(cx).bpm_input.read(cx).value().to_string());
    assert_eq!(typed, "124.0", "the field starts from the tempo on show");

    click(window, ("pick-key", 13_usize), cx);
    assert_eq!(selected_key_and_bpm(&view, cx).0.as_deref(), Some("2B"));

    retype(window, "bpm-field", "128,5", cx);
    cx.update_window(window, |_, window, cx| window.press("enter", cx))
        .unwrap();
    cx.run_until_parked();
    assert_eq!(selected_key_and_bpm(&view, cx).1, Some(128.5));

    retype(window, "bpm-field", "fast", cx);
    click(window, "set-bpm", cx);
    assert!(is_shown(window, "bpm-problem", cx));
    assert_eq!(selected_key_and_bpm(&view, cx).1, Some(128.5));

    click(window, "edit-values", cx);
    assert!(!is_shown(window, ("pick-key", 0_usize), cx));
    assert!(is_shown(window, "tempo-set-by-you", cx));
}

#[gpui::test]
fn a_correction_wins_in_the_mixes_and_the_tags_to_write(cx: &mut TestAppContext) {
    let (window, view, _) = open_tracks_to_check_by_ear(cx);
    let mixed_keys = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            let app = view.read(cx);
            app.mixes
                .iter()
                .map(|mix| {
                    let track = app.session.library().track(mix.track_index).unwrap();
                    track.key().map(|key| key.camelot())
                })
                .collect::<Vec<_>>()
        })
    };
    assert!(!mixed_keys(cx).contains(&Some("9A".to_string())));
    click(window, track(1), cx);
    click(window, "runner-up-key", cx);
    click(window, track(0), cx);
    let after = mixed_keys(cx);
    assert!(after.contains(&Some("9A".to_string())), "{after:?}");

    let target = cx.update(|cx| {
        let track = view.read(cx).session.library().track(1).unwrap();
        tag_writing::WriteTarget {
            path: track.path.clone(),
            analysis: track.analysis().cloned(),
            correction: track.correction,
        }
    });
    let written = target.correction.apply(target.analysis.clone().unwrap());
    assert_eq!(written.key.map(|key| key.camelot()).as_deref(), Some("9A"));
    assert_eq!(
        target
            .analysis
            .unwrap()
            .key
            .map(|key| key.camelot())
            .as_deref(),
        Some("8A"),
        "the cache keeps what was detected"
    );
}

#[gpui::test]
fn the_tones_play_over_the_track_one_at_a_time_and_stop(cx: &mut TestAppContext) {
    let (window, _, output) = open_tracks_to_check_by_ear(cx);
    click(window, track(1), cx);
    click(window, ("tone", 0_usize), cx);
    let sounding = output.sounding();
    assert_eq!(sounding.len(), 1);
    assert!(sounding[0].is_drone);
    assert!((sounding[0].frequencies[1] - 220.0).abs() < 0.01, "A minor");

    click(window, ("tone", 1_usize), cx);
    let sounding = output.sounding();
    assert_eq!(sounding.len(), 1, "one tone at a time");
    assert!(
        (sounding[0].frequencies[1] - 164.81).abs() < 0.01,
        "the runner up, E minor"
    );

    click(window, ("tone", 1_usize), cx);
    assert!(output.sounding().is_empty(), "a second click stops it");

    click(window, ("tone", 0_usize), cx);
    assert_eq!(output.sounding().len(), 1);
    click(window, track(2), cx);
    assert!(
        output.sounding().is_empty(),
        "another track has its own key"
    );
}

#[gpui::test]
fn taking_another_key_stops_the_drone_of_the_old_one(cx: &mut TestAppContext) {
    let (window, _, output) = open_tracks_to_check_by_ear(cx);
    click(window, track(1), cx);
    click(window, ("tone", 0_usize), cx);
    assert_eq!(output.sounding().len(), 1);
    click(window, "runner-up-key", cx);
    assert!(output.sounding().is_empty(), "the drone was of the old key");

    click(window, ("tone", 0_usize), cx);
    let sounding = output.sounding();
    assert_eq!(sounding.len(), 1, "the tone button plays the new key");
    assert!(
        (sounding[0].frequencies[1] - 164.81).abs() < 0.01,
        "E minor"
    );
    click(window, "key-reset", cx);
    assert!(output.sounding().is_empty());
}

#[gpui::test]
fn the_piano_plays_its_notes_and_the_volume_has_steps(cx: &mut TestAppContext) {
    let (window, view, output) = open_tracks_to_check_by_ear(cx);
    click(window, ("piano-key", 9_usize), cx);
    click(window, ("piano-key", 1_usize), cx);
    let heard = output.heard();
    assert_eq!(heard.len(), 2);
    assert!(heard.iter().all(|sound| !sound.is_drone));
    assert!((heard[0].frequencies[0] - 440.0).abs() < 0.01);
    assert!((heard[1].frequencies[0] - 277.18).abs() < 0.01);

    click(window, ("reference-level", 1_usize), cx);
    cx.update(|cx| assert_eq!(view.read(cx).by_ear.level_step(), 1));
    click(window, ("reference-level", 5_usize), cx);
    cx.update(|cx| assert_eq!(view.read(cx).by_ear.level_step(), 5));
}

#[gpui::test]
fn a_tone_without_an_audio_output_says_so(cx: &mut TestAppContext) {
    let (window, view) = open_window_with(
        tracks_to_review(),
        Box::new(NoOutput),
        Settings::default(),
        None,
        USUAL_WINDOW,
        cx,
    );
    click(window, ("tone", 0_usize), cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.by_ear.drone_key(), None);
        assert_eq!(
            app.notice.as_deref(),
            Some(PlaybackProblem::NoOutput.message())
        );
    });
}

#[gpui::test]
fn a_corrected_key_hides_the_detected_key_timeline_until_it_is_reset(cx: &mut TestAppContext) {
    let (window, view) =
        open_track_changing_key(ThemeName::Slate, Panels::default(), USUAL_WINDOW, cx);
    assert!(is_shown(window, "key-timeline", cx));
    let detected = cx.update(|cx| {
        view.read(cx)
            .selected_track()
            .unwrap()
            .detected_key()
            .unwrap()
    });
    let other = euphoria_core::Key::new((detected.tonic() + 1) % 12, detected.mode());
    cx.update(|cx| view.update(cx, |app, cx| app.accept_key(other, cx)));
    assert!(!is_shown(window, "key-timeline", cx));
    cx.update(|cx| view.update(cx, |app, cx| app.accept_key(detected, cx)));
    assert!(
        !is_shown(window, "key-timeline", cx),
        "a correction to the detected key is still the user's"
    );
    cx.update(|cx| view.update(cx, |app, cx| app.reset_key(cx)));
    assert!(is_shown(window, "key-timeline", cx));
}
