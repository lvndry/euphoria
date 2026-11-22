// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Clicking a track in the Live panel shows it in the library window, with
//! the window open, closed and a search hiding the track, and plays nothing.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use euphoria_core::live::{LiveKey, LiveReading, LiveTempo};
use euphoria_core::{Confidence, Key, Mode};
use gpui::{
    AnyWindowHandle, App, AppContext as _, Bounds, Entity, Point, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};

use crate::app::EuphoriaApp;
use crate::library::{Library, TrackFailure};
use crate::live_app::{MainWindowOpener, remember_main_view};
use crate::live_model::LiveModel;
use crate::playback::testing::SilentOutput;
use crate::settings::Settings;
use crate::state::Session;
use crate::ui_tests::{click, deck_tracks, index_of_track};
use crate::views::live::{LiveView, MATCH_ID, row_id};

fn fixture_library() -> Library {
    let mut library = Library::default();
    let tracks = deck_tracks();
    library.add_files(tracks.iter().map(|track| track.path.clone()).collect());
    for track in tracks {
        library.record_outcome(&track.path, track.outcome.map_err(TrackFailure::Failed));
    }
    library
}

fn open_main(cx: &mut App, tracks: Vec<crate::demo::DemoTrack>) -> Entity<EuphoriaApp> {
    gpui::init(cx);
    let mut settings = Settings::default();
    settings
        .first_run
        .get_or_insert(crate::first_run::FirstRun::already_done());
    let (window, view) = gpui::open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: Point::default(),
                size: size(px(1440.0), px(900.0)),
            })),
            ..Default::default()
        },
        cx,
        |window, cx| {
            cx.new(|cx| {
                let mut session = Session::start(None).unwrap();
                session.add_demo_tracks(tracks);
                EuphoriaApp::new(
                    session,
                    settings,
                    None,
                    vec![],
                    Box::new(SilentOutput),
                    window,
                    cx,
                )
            })
        },
    )
    .unwrap();
    crate::register_commands(window, &view, cx);
    remember_main_view(&view, cx);
    view
}

fn live(cx: &mut TestAppContext) -> (Entity<LiveModel>, AnyWindowHandle) {
    let model = cx.update(|cx| {
        gpui::init(cx);
        cx.new(|cx| {
            LiveModel::with_parts(
                Box::new(|| unreachable!("nothing starts listening")),
                Box::new(fixture_library),
                Box::new(|_, _, _| {
                    let mut empty = euphoria_core::fingerprint::FingerprintIndex::new();
                    empty.freeze();
                    empty
                }),
                Settings::default(),
                None,
                Duration::from_millis(100),
                cx,
            )
        })
    });
    let window = cx.update(|cx| {
        let view_model = model.clone();
        gpui::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(360.0), px(620.0)),
                })),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|cx| LiveView::new(view_model, cx)),
        )
        .unwrap()
        .0
    });
    cx.run_until_parked();
    cx.update(|cx| {
        model.update(cx, |model, cx| {
            let mut reading = LiveReading::listening();
            reading.listening = false;
            reading.key = Some(LiveKey {
                key: Key::new(9, Mode::Minor),
                confidence: Confidence::High,
                runner_up: None,
            });
            reading.tempo = Some(LiveTempo {
                bpm: 120.0,
                confidence: Confidence::High,
            });
            reading.energy = Some(6);
            model.pretend_heard(reading, cx);
        })
    });
    cx.run_until_parked();
    (model, window)
}

fn first_row_path(model: &Entity<LiveModel>, cx: &mut TestAppContext) -> (usize, PathBuf) {
    cx.update(|cx| {
        let model = model.read(cx);
        let row = model.next_rows.first().expect("a track to play next");
        let path = model.library.as_ref().unwrap().tracks()[row.track_index]
            .path
            .clone();
        (row.track_index, path)
    })
}

fn never_opens() {
    // The window is open in these tests, so the opener does nothing.
}

#[gpui::test]
fn clicking_a_next_row_selects_it_in_the_open_library_window_without_playing(
    cx: &mut TestAppContext,
) {
    let main = cx.update(|cx| {
        cx.set_global(MainWindowOpener(Rc::new(|_| never_opens())));
        open_main(cx, deck_tracks())
    });
    let (model, window) = live(cx);
    let (live_index, path) = first_row_path(&model, cx);
    click(window, row_id(live_index), cx);
    cx.run_until_parked();
    let wanted = cx.update(|cx| main.read(cx).session.library().index_of(&path).unwrap());
    cx.update(|cx| {
        let app = main.read(cx);
        assert_eq!(app.selected, Some(wanted));
        assert!(app.visible.contains(&wanted));
        assert!(!app.player.is_playing(), "nothing plays");
    });
}

#[gpui::test]
fn a_search_that_hides_the_track_is_taken_off(cx: &mut TestAppContext) {
    let main = cx.update(|cx| {
        cx.set_global(MainWindowOpener(Rc::new(|_| never_opens())));
        open_main(cx, deck_tracks())
    });
    let (model, window) = live(cx);
    let (live_index, path) = first_row_path(&model, cx);
    cx.update(|cx| {
        main.update(cx, |app, _| {
            app.query = "no track is called this".into();
            app.refresh_visible();
        })
    });
    let wanted = cx.update(|cx| main.read(cx).session.library().index_of(&path).unwrap());
    cx.update(|cx| assert!(!main.read(cx).visible.contains(&wanted)));
    click(window, row_id(live_index), cx);
    cx.run_until_parked();
    cx.update(|cx| {
        let app = main.read(cx);
        assert!(app.query.is_empty());
        assert!(app.visible.contains(&wanted));
        assert_eq!(app.selected, Some(wanted));
        assert!(!app.player.is_playing());
    });
}

#[gpui::test]
fn a_closed_library_window_is_opened_and_shows_the_track_once_it_has_read_its_folders(
    cx: &mut TestAppContext,
) {
    let opened: Rc<std::cell::RefCell<Option<Entity<EuphoriaApp>>>> = Rc::default();
    let sink = Rc::clone(&opened);
    cx.update(|cx| {
        cx.set_global(MainWindowOpener(Rc::new(move |cx| {
            *sink.borrow_mut() = Some(open_main(cx, Vec::new()));
        })));
    });
    let (model, window) = live(cx);
    let (live_index, path) = first_row_path(&model, cx);
    click(window, row_id(live_index), cx);
    cx.run_until_parked();
    let main = opened.borrow().clone().expect("the window was opened");
    cx.update(|cx| {
        assert_eq!(main.read(cx).pending_reveal.as_ref(), Some(&path));
        assert_eq!(main.read(cx).selected, None);
    });
    cx.update(|cx| {
        main.update(cx, |app, cx| {
            app.session.add_demo_tracks(deck_tracks());
            app.refresh_visible();
            app.reveal_pending(cx);
        })
    });
    cx.run_until_parked();
    let wanted = index_of_track(&main, path.file_name().unwrap().to_str().unwrap(), cx);
    cx.update(|cx| {
        let app = main.read(cx);
        assert!(app.pending_reveal.is_none());
        assert_eq!(app.selected, Some(wanted));
        assert!(app.visible.contains(&wanted));
        assert!(!app.player.is_playing());
    });
}

#[gpui::test]
fn the_in_your_library_card_opens_the_matched_track(cx: &mut TestAppContext) {
    let main = cx.update(|cx| {
        cx.set_global(MainWindowOpener(Rc::new(|_| never_opens())));
        open_main(cx, deck_tracks())
    });
    let (model, window) = live(cx);
    let (_, path) = first_row_path(&model, cx);
    let live_index = cx.update(|cx| {
        model
            .read(cx)
            .library
            .as_ref()
            .unwrap()
            .index_of(&path)
            .unwrap()
    });
    cx.update(|cx| {
        model.update(cx, |model, cx| {
            model.pretend_match(live_index, cx);
        })
    });
    cx.run_until_parked();
    click(window, MATCH_ID, cx);
    cx.run_until_parked();
    let wanted = cx.update(|cx| main.read(cx).session.library().index_of(&path).unwrap());
    cx.update(|cx| {
        assert_eq!(main.read(cx).selected, Some(wanted));
        assert!(!main.read(cx).player.is_playing());
    });
}
