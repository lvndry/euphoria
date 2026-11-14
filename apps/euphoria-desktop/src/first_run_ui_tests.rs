// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The first run card, used in a headless window.

use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext, px, size};

use crate::app::EuphoriaApp;
use crate::first_run::FirstRun;
use crate::playback::testing::SilentOutput;
use crate::settings::Settings;
use crate::tag_settings::ProgramChoice;
use crate::tag_writing::testing::ScratchFolder;
use crate::ui_tests::{deck_tracks, is_shown, open_window_with};

fn open_with(
    tracks: Vec<crate::demo::DemoTrack>,
    settings: Settings,
    settings_path: Option<std::path::PathBuf>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_window_with(
        tracks,
        Box::new(SilentOutput),
        settings,
        settings_path,
        size(px(1440.0), px(900.0)),
        cx,
    )
}

fn new_user() -> Settings {
    Settings {
        first_run: Some(FirstRun::default()),
        ..Settings::default()
    }
}

#[gpui::test]
fn an_empty_library_shows_the_three_steps_with_choose_folder(cx: &mut TestAppContext) {
    let (window, _view) = open_with(Vec::new(), new_user(), None, cx);
    for id in [
        "first-run",
        "first-run-add-music",
        "first-run-analyze",
        "first-run-fill-in-tags",
        "choose-folder",
    ] {
        assert!(is_shown(window, id, cx), "{id}");
    }
    assert!(!is_shown(window, "first-run-write-tags", cx));
}

#[gpui::test]
fn the_steps_advance_as_analysis_and_the_first_write_finish(cx: &mut TestAppContext) {
    let (window, view) = open_with(deck_tracks(), new_user(), None, cx);
    assert!(is_shown(window, "first-run", cx), "above the table");
    assert!(!is_shown(window, "choose-folder", cx), "music is added");
    assert!(!is_shown(window, "first-run-write-tags", cx), "not yet");
    assert!(is_shown(window, "first-run-analysis-line", cx));

    cx.update(|cx| view.update(cx, |app, _| app.note_first_run_progress()));
    cx.update(|cx| assert!(view.read(cx).first_run.analysis_done));
    assert!(is_shown(window, "first-run", cx));
    assert!(
        is_shown(window, "first-run-write-tags", cx),
        "step 3 is current"
    );
    assert!(!is_shown(window, "first-run-analysis-line", cx));

    cx.update(|cx| view.update(cx, |app, _| app.note_first_write()));
    assert!(!is_shown(window, "first-run", cx), "both steps are done");
}

#[gpui::test]
fn while_analysis_runs_the_second_step_counts_the_tracks(cx: &mut TestAppContext) {
    let (_window, view) = open_with(deck_tracks(), new_user(), None, cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.session.pretend_queued(575);
            for _ in 0..312 {
                app.session.pretend_finished();
            }
            assert_eq!(
                crate::views::first_run::analysis_line(app).as_deref(),
                Some("312 of 575"),
                "no time left before ten seconds of data"
            );
            app.note_first_run_progress();
            assert!(!app.first_run.analysis_done, "a run is going");
        })
    });
}

#[gpui::test]
fn the_third_step_names_the_chosen_dj_program(cx: &mut TestAppContext) {
    let (_window, view) = open_with(deck_tracks(), new_user(), None, cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            assert_eq!(crate::views::first_run::program_line(app), None);
            app.tag_settings.choose_program(ProgramChoice::Rekordbox);
            assert_eq!(
                crate::views::first_run::program_line(app).as_deref(),
                Some("Writing for rekordbox")
            );
        })
    });
}

#[gpui::test]
fn the_card_never_comes_back_once_both_steps_are_done(cx: &mut TestAppContext) {
    let folder = ScratchFolder::new("first-run-never-again");
    let path = folder.0.join("settings.json");
    let (_window, view) = open_with(deck_tracks(), new_user(), Some(path.clone()), cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.note_first_run_progress();
            app.note_first_write();
        })
    });
    let stored = Settings::load(&path).unwrap();
    assert_eq!(stored.first_run, Some(FirstRun::already_done()));

    let (window, view) = open_with(deck_tracks(), stored, None, cx);
    assert!(!is_shown(window, "first-run", cx));
    cx.update(|cx| assert!(!view.read(cx).shows_first_run()));
}

#[gpui::test]
fn someone_with_folders_and_an_old_settings_file_never_sees_the_card(cx: &mut TestAppContext) {
    let old: Settings = serde_json::from_str(r#"{"folders":["/music"],"theme":"slate"}"#).unwrap();
    assert_eq!(old.first_run, None);
    let (window, view) = open_with(deck_tracks(), old, None, cx);
    assert!(!is_shown(window, "first-run", cx));
    cx.update(|cx| assert!(!view.read(cx).shows_first_run()));
}
