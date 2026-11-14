// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The logo in the toolbar as the activity indicator: what counts as busy,
//! the words in its tooltip, and which mark is on screen.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use gpui::{Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::library_export::{
    ExportProgram, ExportScope, ExportSetup, ExportStage, LibraryExportPanel,
};
use crate::tag_flow::TagPanel;
use crate::ui_tests::{is_shown, open_library};

fn fails_to_separate() -> Result<Box<dyn crate::stems::StemSeparation>, euphoria_core::EuphoriaError>
{
    Err(euphoria_core::EuphoriaError::NoDataDirectory)
}

fn work(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| view.read(cx).busy_work())
}

fn change(
    view: &Entity<EuphoriaApp>,
    cx: &mut TestAppContext,
    change: impl FnOnce(&mut EuphoriaApp, &mut gpui::Context<'_, EuphoriaApp>),
) {
    cx.update(|cx| view.update(cx, change));
}

#[gpui::test]
fn an_app_with_nothing_running_is_idle(cx: &mut TestAppContext) {
    let (_, view) = open_library(5, cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(!app.is_busy());
        assert_eq!(app.busy_description(), None);
    });
}

#[gpui::test]
fn analysis_counts_its_tracks(cx: &mut TestAppContext) {
    let (_, view) = open_library(5, cx);
    change(&view, cx, |app, _| {
        app.session.pretend_queued(575);
        for _ in 0..120 {
            app.session.pretend_finished();
        }
    });
    assert_eq!(work(&view, cx), ["Analyzing 120 of 575 tracks"]);
}

#[gpui::test]
fn each_kind_of_work_makes_the_app_busy(cx: &mut TestAppContext) {
    let (_, view) = open_library(5, cx);

    change(&view, cx, |app, _| app.scans_in_progress = 1);
    assert_eq!(work(&view, cx), ["Looking for audio files"]);
    change(&view, cx, |app, _| app.scans_in_progress = 0);

    change(&view, cx, |app, _| {
        app.tag_panel = Some(TagPanel::Writing {
            done: Arc::new(AtomicUsize::new(0)),
            total: 3,
        });
    });
    assert_eq!(work(&view, cx), ["Writing tags"]);
    change(&view, cx, |app, _| app.tag_panel = Some(TagPanel::Undoing));
    assert_eq!(work(&view, cx), ["Undoing the tag write"]);
    change(&view, cx, |app, _| {
        app.tag_panel = Some(TagPanel::ConfirmUndo)
    });
    assert!(work(&view, cx).is_empty());
    change(&view, cx, |app, _| app.tag_panel = None);

    let export_panel = |stage| LibraryExportPanel {
        setup: ExportSetup::new(ExportProgram::Rekordbox, ExportScope::Library),
        stage,
        problem: None,
    };
    change(&view, cx, |app, _| {
        app.library_export = Some(export_panel(ExportStage::Working { writing: true }));
    });
    assert_eq!(work(&view, cx), ["Exporting to rekordbox"]);
    change(&view, cx, |app, _| {
        app.library_export = Some(export_panel(ExportStage::Setup));
    });
    assert!(work(&view, cx).is_empty());
    change(&view, cx, |app, _| app.library_export = None);

    change(&view, cx, |app, _| {
        app.stems = crate::stems::StemSession::new(fails_to_separate);
        assert!(app.stems.start(Path::new("/music/track.mp3")));
    });
    assert_eq!(work(&view, cx), ["Separating stems"]);
    change(&view, cx, |app, _| {
        app.stems = crate::stems::StemSession::new(fails_to_separate);
    });

    change(&view, cx, |app, _| {
        app.decks.stem_library.pretend_scanning()
    });
    assert_eq!(work(&view, cx), ["Looking for stems"]);
    change(&view, cx, |app, _| {
        app.decks.stem_library.pretend_done_scanning();
    });

    change(&view, cx, |app, _| app.set_builder.is_building = true);
    assert_eq!(work(&view, cx), ["Building the set"]);
    change(&view, cx, |app, _| app.set_builder.is_building = false);

    assert!(work(&view, cx).is_empty());
}

#[gpui::test]
fn background_jobs_count_until_they_finish(cx: &mut TestAppContext) {
    let (_, view) = open_library(5, cx);
    let mut stems = 0;
    let mut playlist = 0;
    change(&view, cx, |app, cx| {
        stems = app.begin_background_job("Exporting stems", cx);
        playlist = app.begin_background_job("Exporting a playlist", cx);
    });
    assert_eq!(work(&view, cx), ["Exporting stems", "Exporting a playlist"]);
    change(&view, cx, |app, _| app.background_jobs.finish(stems));
    assert_eq!(work(&view, cx), ["Exporting a playlist"]);
    change(&view, cx, |app, _| app.background_jobs.finish(playlist));
    assert!(work(&view, cx).is_empty());
}

#[gpui::test]
fn the_logo_is_the_activity_indicator(cx: &mut TestAppContext) {
    let (window, view) = open_library(5, cx);
    assert!(is_shown(window, "logo-idle", cx));
    assert!(!is_shown(window, "logo-busy", cx));

    change(&view, cx, |app, cx| {
        app.session.pretend_queued(575);
        for _ in 0..120 {
            app.session.pretend_finished();
        }
        cx.notify();
    });
    assert!(is_shown(window, "logo-busy", cx));
    assert!(!is_shown(window, "logo-idle", cx));
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).busy_description().as_deref(),
            Some("Analyzing 120 of 575 tracks")
        );
    });

    change(&view, cx, |app, cx| {
        for _ in 120..575 {
            app.session.pretend_finished();
        }
        cx.notify();
    });
    assert!(is_shown(window, "logo-idle", cx));
}
