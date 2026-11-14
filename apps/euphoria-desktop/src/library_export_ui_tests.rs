// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of the export sheet in the real root view, in a headless window.

use std::path::PathBuf;

use gpui::{Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::library_export::{ExportProgram, ExportSetup, ExportStage, RunningChecks};
use crate::tag_writing::testing::ScratchFolder;
use crate::ui_tests::{click, is_shown, open_fixture_library};

fn change(
    view: &Entity<EuphoriaApp>,
    change: impl FnOnce(&mut ExportSetup),
    cx: &mut TestAppContext,
) {
    cx.update(|cx| view.update(cx, |app, cx| app.change_export_setup(change, cx)));
}

fn open_sheet(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) {
    cx.update(|cx| view.update(cx, |app, cx| app.open_library_export(cx)));
}

fn problem(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Option<String> {
    cx.update(|cx| {
        view.read(cx)
            .library_export
            .as_ref()
            .and_then(|panel| panel.problem.clone())
    })
}

#[gpui::test]
fn the_export_sheet_opens_lists_the_programs_and_closes(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    assert!(!is_shown(window, "library-export-card", cx));
    open_sheet(&view, cx);
    assert!(is_shown(window, "library-export-card", cx));
    for program in ExportProgram::ALL {
        assert!(is_shown(
            window,
            format!("export-program-{}", program.id()),
            cx
        ));
    }
    click(window, "export-program-engine", cx);
    cx.update(|cx| {
        let panel = view.read(cx).library_export.as_ref().unwrap();
        assert_eq!(panel.setup.program, ExportProgram::EngineDj);
    });
    assert!(is_shown(window, "export-choose-target", cx));
    click(window, "export-cancel", cx);
    assert!(!is_shown(window, "library-export-card", cx));
}

#[gpui::test]
fn the_rekordbox_export_of_the_fixture_library_writes_a_valid_file(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("library-export-rekordbox");
    let output = scratch.0.join("library.xml");
    let (window, view) = open_fixture_library(cx);
    open_sheet(&view, cx);
    let destination = output.clone();
    change(&view, move |setup| setup.output = Some(destination), cx);

    click(window, "export-review", cx);
    assert!(!output.exists(), "the review writes nothing");
    cx.update(|cx| {
        let panel = view.read(cx).library_export.as_ref().unwrap();
        let ExportStage::Review(report) = &panel.stage else {
            panic!(
                "expected a review, got {:?} {:?}",
                panel.stage, panel.problem
            );
        };
        assert_eq!(report.track_count, 3);
    });

    click(window, "export-confirm", cx);
    let text = std::fs::read_to_string(&output).unwrap();
    let document = roxmltree::Document::parse(&text).unwrap();
    let root = document.root_element();
    assert!(root.has_tag_name("DJ_PLAYLISTS"));
    let tracks = document
        .descendants()
        .filter(|node| node.has_tag_name("TRACK") && node.attribute("Location").is_some())
        .count();
    assert_eq!(tracks, 3);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(matches!(
            app.library_export.as_ref().unwrap().stage,
            ExportStage::Done(_)
        ));
        assert!(app.notice.as_deref().unwrap().contains("Exported 3 tracks"));
    });
}

fn engine_library_with_version(scratch: &ScratchFolder, major: i64) -> PathBuf {
    let library = scratch.0.join("Engine Library");
    std::fs::create_dir_all(library.join("Database2")).unwrap();
    let connection = rusqlite::Connection::open(library.join("Database2/m.db")).unwrap();
    connection
        .execute_batch(&format!(
            "CREATE TABLE Information (id INTEGER PRIMARY KEY, uuid TEXT, schemaVersionMajor INTEGER, \
             schemaVersionMinor INTEGER, schemaVersionPatch INTEGER, currentPlayedIndicator INTEGER, \
             lastRekordBoxLibraryImportReadCounter INTEGER);
             INSERT INTO Information VALUES (1, 'x', {major}, 0, 0, 0, 0);"
        ))
        .unwrap();
    library
}

#[gpui::test]
fn an_engine_export_of_a_database_with_an_unknown_version_is_refused(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("library-export-engine");
    let library = engine_library_with_version(&scratch, 99);
    let database = library.join("Database2/m.db");
    let before = std::fs::read(&database).unwrap();
    let (window, view) = open_fixture_library(cx);
    open_sheet(&view, cx);
    change(
        &view,
        move |setup| {
            setup.choose_program(ExportProgram::EngineDj);
            setup.target = Some(library);
        },
        cx,
    );

    click(window, "export-review", cx);
    let message = problem(&view, cx).expect("the review refuses");
    assert!(message.contains("schema version 99.0.0"), "{message}");
    assert!(!is_shown(window, "export-confirm", cx));
    assert_eq!(std::fs::read(&database).unwrap(), before);
    let backups: Vec<_> = std::fs::read_dir(&scratch.0)
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().contains("backup"))
        .collect();
    assert!(backups.is_empty());
}

#[gpui::test]
fn an_engine_export_is_refused_while_engine_dj_runs(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("library-export-engine-running");
    let library = engine_library_with_version(&scratch, 99);
    let (_window, view) = open_fixture_library(cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.running_checks = RunningChecks {
                engine: || Some(true),
                virtualdj: || Some(false),
            }
        })
    });
    let (window, view) = (_window, view);
    open_sheet(&view, cx);
    change(
        &view,
        move |setup| {
            setup.choose_program(ExportProgram::EngineDj);
            setup.target = Some(library);
        },
        cx,
    );
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.library_export.as_mut().unwrap().stage = ExportStage::Review(Default::default());
            app.confirm_library_export(cx);
        })
    });
    cx.run_until_parked();
    let message = problem(&view, cx).expect("the export refuses");
    assert!(message.contains("Engine DJ is running"), "{message}");
    assert!(is_shown(window, "export-problem", cx));
}

#[gpui::test]
fn the_collection_is_protected_even_when_cues_are_off(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("library-export-protected-collection");
    let collection = scratch.0.join("collection.xml");
    std::fs::write(&collection, "my own collection").unwrap();
    let (window, view) = open_fixture_library(cx);
    open_sheet(&view, cx);
    let chosen = collection.clone();
    change(
        &view,
        move |setup| {
            setup.beat_grid = true;
            setup.collection = Some(chosen.clone());
            setup.output = Some(chosen);
        },
        cx,
    );
    cx.update(|cx| {
        let setup = &view.read(cx).library_export.as_ref().unwrap().setup;
        assert!(!setup.cues);
        assert!(setup.collection.is_some());
    });

    click(window, "export-review", cx);
    let message = problem(&view, cx).expect("the review refuses");
    assert!(message.contains("collection"), "{message}");
    assert_eq!(
        std::fs::read_to_string(&collection).unwrap(),
        "my own collection"
    );

    change(&view, |setup| setup.beat_grid = false, cx);
    cx.update(|cx| {
        let setup = &view.read(cx).library_export.as_ref().unwrap().setup;
        assert!(setup.collection.is_none(), "a hidden collection is dropped");
    });
}
