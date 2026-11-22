// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Analyzing tracks again from the table, with the analysis stubbed so that
//! it can be counted and held back.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use euphoria_core::{Correction, TrackAnalysis};
use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::demo::{self, DemoTrack};
use crate::library::{STOPPED_REASON, TrackFailure, TrackStatus};
use crate::playback::testing::SilentOutput;
use crate::settings::Settings;
use crate::state::Session;
use crate::ui_tests::click;

const REFRESHED_DURATION: f32 = 1.0;

struct Stub {
    calls: Mutex<HashMap<PathBuf, usize>>,
    open: AtomicBool,
}

impl Stub {
    fn new(open: bool) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(HashMap::new()),
            open: AtomicBool::new(open),
        })
    }

    fn calls_for(&self, path: &Path) -> usize {
        self.calls.lock().unwrap().get(path).copied().unwrap_or(0)
    }

    fn total(&self) -> usize {
        self.calls.lock().unwrap().values().sum()
    }

    fn analysis(&self, path: &Path) -> Result<TrackAnalysis, TrackFailure> {
        while !self.open.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(2));
        }
        *self
            .calls
            .lock()
            .unwrap()
            .entry(path.to_path_buf())
            .or_default() += 1;
        let mut analysis = demo::tracks(1).remove(0).outcome.unwrap();
        analysis.duration_seconds = REFRESHED_DURATION;
        Ok(analysis)
    }
}

fn open_stubbed(
    tracks: Vec<DemoTrack>,
    stub: &Arc<Stub>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let stub = stub.clone();
    cx.update(|cx| {
        gpui::init(cx);
        let mut settings = Settings::default();
        settings
            .first_run
            .get_or_insert(crate::first_run::FirstRun::already_done());
        let (window, view) = gpui::open_window(Default::default(), cx, |window, cx| {
            cx.new(|cx| {
                let mut session =
                    Session::start_stubbed(Arc::new(move |path| stub.analysis(path))).unwrap();
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
        })
        .unwrap();
        crate::register_commands(window, &view, cx);
        (window, view)
    })
}

fn paths(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<PathBuf> {
    cx.update(|cx| {
        view.read(cx)
            .session
            .library()
            .tracks()
            .iter()
            .map(|track| track.path.clone())
            .collect()
    })
}

fn wait_for_workers(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let active = cx.update(|cx| {
            view.update(cx, |app, _| {
                app.session.collect_finished();
                app.session.progress().is_active()
            })
        });
        if !active {
            return;
        }
        assert!(Instant::now() < deadline, "the workers did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn duration_of(view: &Entity<EuphoriaApp>, path: &Path, cx: &mut TestAppContext) -> Option<f32> {
    cx.update(|cx| {
        let library = view.read(cx).session.library();
        let track = library.track(library.index_of(path)?)?;
        match &track.status {
            TrackStatus::Analyzed(analysis) => Some(analysis.duration_seconds),
            _ => None,
        }
    })
}

fn select_rows(view: &Entity<EuphoriaApp>, rows: &[usize], cx: &mut TestAppContext) {
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.select(rows[0], cx);
            app.marked = rows.iter().copied().collect::<HashSet<_>>();
        })
    });
}

#[gpui::test]
fn only_the_chosen_tracks_are_analyzed_again_and_keep_everything_the_user_owns(
    cx: &mut TestAppContext,
) {
    let stub = Stub::new(true);
    let (window, view) = open_stubbed(demo::tracks(300), &stub, cx);
    let all = paths(&view, cx);
    let chosen = [2usize, 3, 4];
    let correction = Correction::default().confirming_key();
    let cues = vec![euphoria_core::Cue {
        seconds: 5.0,
        name: "Drop".to_string(),
        hot_cue: None,
        energy: None,
    }];
    click(window, "set-builder-entry", cx);
    click(window, "build-set", cx);
    click(window, ("set-lock", 0usize), cx);
    let before = cx.update(|cx| {
        view.update(cx, |app, cx| {
            for index in chosen {
                app.session.set_correction(&all[index], correction);
                app.session.set_edited_cues(&all[index], Some(cues.clone()));
            }
            app.session.favorites_mut().toggle_stars(&all[2..4]);
            let playlist = app.session.playlists_mut().create();
            app.session.playlists_mut().add_tracks(playlist, &all[3..5]);
            let planned = app.set_builder.planned.clone().unwrap();
            cx.notify();
            (
                app.session.favorites().tracks().to_vec(),
                app.session.playlists().all().to_vec(),
                planned.paths,
                planned.locked,
            )
        })
    });
    select_rows(&view, &chosen, cx);
    cx.update(|cx| view.update(cx, |app, cx| app.analyze_row_again(3, cx)));
    wait_for_workers(&view, cx);

    for (index, path) in all.iter().enumerate() {
        let expected = usize::from(chosen.contains(&index));
        assert_eq!(stub.calls_for(path), expected, "track {index}");
        assert_eq!(
            duration_of(&view, path, cx) == Some(REFRESHED_DURATION),
            chosen.contains(&index),
            "track {index}"
        );
    }
    cx.update(|cx| {
        let app = view.read(cx);
        for index in chosen {
            let track = &app.session.library().tracks()[index];
            assert_eq!(track.correction, correction);
            assert_eq!(track.edited_cues.as_deref(), Some(cues.as_slice()));
        }
        assert_eq!(app.session.favorites().tracks(), before.0.as_slice());
        assert_eq!(app.session.playlists().all().len(), before.1.len());
        assert_eq!(app.session.playlists().all()[0].tracks, before.1[0].tracks);
        let planned = app.set_builder.planned.as_ref().unwrap();
        assert_eq!(planned.paths, before.2);
        assert_eq!(planned.locked, before.3);
        assert!(planned.locked[0]);
    });
}

#[gpui::test]
fn a_right_click_outside_the_selection_analyzes_only_that_track(cx: &mut TestAppContext) {
    let stub = Stub::new(true);
    let (_, view) = open_stubbed(demo::tracks(20), &stub, cx);
    let all = paths(&view, cx);
    select_rows(&view, &[1, 2, 3], cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.rows_to_analyze_again(2).len(), 3);
        assert_eq!(app.rows_to_analyze_again(7), vec![all[7].clone()]);
    });
    cx.update(|cx| view.update(cx, |app, cx| app.analyze_row_again(7, cx)));
    wait_for_workers(&view, cx);
    assert_eq!(stub.total(), 1);
    assert_eq!(stub.calls_for(&all[7]), 1);
    let fresh = cx.update(|cx| view.update(cx, |app, _| app.session.take_fresh_analyses()));
    assert_eq!(
        fresh,
        vec![all[7].clone()],
        "the tracks write after analysis picks up, as for a new analysis"
    );
}

#[gpui::test]
fn the_logo_and_progress_show_while_tracks_are_analyzed_again(cx: &mut TestAppContext) {
    let stub = Stub::new(false);
    let (_, view) = open_stubbed(demo::tracks(30), &stub, cx);
    select_rows(&view, &[0, 1, 2, 3, 4], cx);
    cx.update(|cx| {
        assert!(!view.read(cx).is_busy());
        view.update(cx, |app, cx| app.analyze_selection_again(cx));
        let app = view.read(cx);
        assert!(app.is_busy());
        assert_eq!(app.busy_work(), ["Analyzing 0 of 5 tracks"]);
        assert_eq!(app.session.progress().counts(), Some((0, 5)));
    });
    stub.open.store(true, Ordering::SeqCst);
    wait_for_workers(&view, cx);
    cx.update(|cx| assert!(!view.read(cx).is_busy()));
}

#[gpui::test]
fn needs_review_analyze_again_uses_the_same_queue(cx: &mut TestAppContext) {
    let stub = Stub::new(true);
    let mut tracks = demo::tracks(6);
    tracks[4].outcome = Err("could not decode".to_string());
    let (window, view) = open_stubbed(tracks, &stub, cx);
    let all = paths(&view, cx);
    cx.update(|cx| view.update(cx, |app, cx| app.select(4, cx)));
    click(window, "review-analyze-again", cx);
    wait_for_workers(&view, cx);
    assert_eq!(stub.total(), 1);
    assert_eq!(stub.calls_for(&all[4]), 1);
    assert_eq!(duration_of(&view, &all[4], cx), Some(REFRESHED_DURATION));
}

#[gpui::test]
fn the_menus_and_the_palette_carry_analyze_again(cx: &mut TestAppContext) {
    use gpui::Action;
    let in_tracks_menu = crate::build_menus(crate::theme::ThemeName::Slate, false)
        .iter()
        .filter(|menu| menu.name == "Tracks")
        .flat_map(|menu| menu.items.iter())
        .any(|item| {
            matches!(item, gpui::MenuItem::Action { action, .. }
                if action.name() == crate::actions::AnalyzeAgain.name())
        });
    assert!(in_tracks_menu);
    assert!(crate::command_palette::command_named("analyze-again").is_some());
    let stub = Stub::new(true);
    let (window, view) = open_stubbed(demo::tracks(8), &stub, cx);
    select_rows(&view, &[0, 1], cx);
    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(crate::actions::AnalyzeAgain), cx);
    })
    .unwrap();
    wait_for_workers(&view, cx);
    assert_eq!(stub.total(), 2);
}

fn select_all_with_the_keyboard(
    window: AnyWindowHandle,
    view: &Entity<EuphoriaApp>,
    cx: &mut TestAppContext,
) {
    cx.update_window(window, |_, window, cx| {
        let focus = view.read(cx).table_focus.clone();
        focus.focus(window, cx);
        window.render_frame(cx);
        window.press("cmd-a", cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui::test]
fn select_all_then_analyze_again_queues_a_whole_library(cx: &mut TestAppContext) {
    let stub = Stub::new(false);
    let (window, view) = open_stubbed(demo::tracks(2000), &stub, cx);
    let all = paths(&view, cx);
    let correction = Correction::default().confirming_bpm();
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.session.set_correction(&all[10], correction);
            app.session.favorites_mut().toggle_stars(&all[20..30]);
        })
    });
    select_all_with_the_keyboard(window, &view, cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.selected_tracks().len(), 2000);
        assert_eq!(app.rows_to_analyze_again(0).len(), 2000);
    });
    cx.update(|cx| view.update(cx, |app, cx| app.analyze_row_again(0, cx)));
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.session.progress().counts(), Some((0, 2000)));
        assert_eq!(app.busy_work(), ["Analyzing 0 of 2,000 tracks"]);
    });
    stub.open.store(true, Ordering::SeqCst);
    wait_for_workers(&view, cx);
    assert_eq!(stub.total(), 2000);
    for path in &all {
        assert_eq!(stub.calls_for(path), 1);
    }
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.session.library().tracks()[10].correction, correction);
        assert_eq!(app.session.favorites().tracks().len(), 10);
    });
}

#[gpui::test]
fn stop_skips_the_tracks_still_waiting_and_they_can_be_analyzed_again(cx: &mut TestAppContext) {
    let stub = Stub::new(false);
    let (_, view) = open_stubbed(demo::tracks(200), &stub, cx);
    let all = paths(&view, cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.select_all_shown(cx);
            app.analyze_selection_again(cx);
            app.stop_analysis(cx);
        })
    });
    stub.open.store(true, Ordering::SeqCst);
    wait_for_workers(&view, cx);
    let analyzed = stub.total();
    assert!(analyzed < 200, "{analyzed} were analyzed after the stop");
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(!app.is_busy());
        let refreshed = app
            .session
            .library()
            .tracks()
            .iter()
            .filter(|track| matches!(&track.status, TrackStatus::Analyzed(analysis) if analysis.duration_seconds == REFRESHED_DURATION))
            .count();
        assert_eq!(refreshed, analyzed);
    });
    cx.update(|cx| view.update(cx, |app, cx| app.analyze_selection_again(cx)));
    wait_for_workers(&view, cx);
    for path in &all {
        assert!(stub.calls_for(path) >= 1);
    }
}

#[gpui::test]
fn stopping_queued_first_analysis_leaves_tracks_that_can_be_analyzed_again(
    cx: &mut TestAppContext,
) {
    let stub = Stub::new(false);
    let mut tracks = demo::tracks(100);
    for track in &mut tracks {
        track.outcome = Err("not yet".to_string());
    }
    let (_, view) = open_stubbed(tracks, &stub, cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.select_all_shown(cx);
            app.analyze_selection_again(cx);
            app.stop_analysis(cx);
        })
    });
    stub.open.store(true, Ordering::SeqCst);
    wait_for_workers(&view, cx);
    cx.update(|cx| {
        let stopped = view
            .read(cx)
            .session
            .library()
            .tracks()
            .iter()
            .filter(|track| matches!(&track.status, TrackStatus::Failed(reason) if reason == STOPPED_REASON))
            .count();
        assert_eq!(stopped, 100 - stub.total());
    });
}
