// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests that the Write tags preview follows the tag settings before
//! anything is written, in the real root view in a headless window.

use std::time::Duration;

use gpui::{AnyWindowHandle, Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::tag_flow::TagPanel;
use crate::tag_settings::NotationChoice;
use crate::tag_writing::PreviewRow;
use crate::tag_writing::testing::{self as tag_testing, ScratchFolder};
use crate::ui_tests::{click, is_shown, open_taggable_files};

const SLOW_PLAN: Duration = Duration::from_secs(5);

fn open_preview(
    name: &str,
    cx: &mut TestAppContext,
) -> (
    ScratchFolder,
    Vec<std::path::PathBuf>,
    AnyWindowHandle,
    Entity<EuphoriaApp>,
) {
    let scratch = ScratchFolder::new(name);
    let files = scratch.copy_fixtures();
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    click(window, "write-tags", cx);
    click(window, "scope-shown", cx);
    (scratch, files, window, view)
}

fn spelled(notation: NotationChoice) -> String {
    notation.spell(&tag_testing::analysis().key.unwrap())
}

/// The new key of each file in the preview, and whether it is being
/// worked out again.
fn shown_keys(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> (Vec<String>, bool) {
    cx.update(|cx| match &view.read(cx).tag_panel {
        Some(TagPanel::Preview(preview)) => (
            preview
                .rows
                .iter()
                .filter_map(|row| match row {
                    PreviewRow::Change(change, Some("Key")) => change.new.clone(),
                    _ => None,
                })
                .collect(),
            preview.updating,
        ),
        other => panic!("{other:?}"),
    })
}

fn keys_in_files(files: &[std::path::PathBuf]) -> Vec<Option<String>> {
    files
        .iter()
        .map(|path| euphoria_core::tags::read_tags(path).unwrap().key)
        .collect()
}

fn set_notation(view: &Entity<EuphoriaApp>, notation: NotationChoice, cx: &mut TestAppContext) {
    cx.update(|cx| view.update(cx, |app, cx| app.set_key_notation(notation, cx)));
}

fn delay_next_plan(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) {
    cx.update(|cx| view.update(cx, |app, _| app.tag_plan_delays.push_back(SLOW_PLAN)));
}

#[gpui::test]
fn changing_the_notation_from_the_preview_updates_its_rows_before_the_write(
    cx: &mut TestAppContext,
) {
    let (_scratch, files, window, view) = open_preview("preview-follows-notation", cx);
    let camelot = spelled(NotationChoice::Camelot);
    let musical = spelled(NotationChoice::Musical);
    assert_ne!(camelot, musical);
    assert_eq!(shown_keys(&view, cx), (vec![camelot; files.len()], false));

    click(window, "change-tag-settings", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.settings_open);
        assert!(
            matches!(app.tag_panel, Some(TagPanel::Preview(_))),
            "the preview stays under the settings"
        );
    });
    click(window, ("notation", 1_usize), cx);
    assert_eq!(
        shown_keys(&view, cx),
        (vec![musical.clone(); files.len()], false)
    );
    click(window, "close-settings", cx);
    cx.update(|cx| assert!(!view.read(cx).settings_open));
    assert!(is_shown(window, "confirm-tag-write", cx));

    click(window, "confirm-tag-write", cx);
    assert_eq!(keys_in_files(&files), vec![Some(musical); files.len()]);
}

#[gpui::test]
fn a_preview_being_worked_out_again_writes_nothing_and_then_writes_what_it_shows(
    cx: &mut TestAppContext,
) {
    let (scratch, files, window, view) = open_preview("preview-pending-confirm", cx);
    let before = tag_testing::contents(&files);

    delay_next_plan(&view, cx);
    set_notation(&view, NotationChoice::OpenKey, cx);
    cx.run_until_parked();
    assert!(shown_keys(&view, cx).1, "the preview says it is updating");

    click(window, "confirm-tag-write", cx);
    assert_eq!(tag_testing::contents(&files), before, "nothing is written");
    assert!(!scratch.log_directory().exists());
    cx.update(|cx| {
        view.update(cx, |app, cx| app.write_confirmed_tags(cx));
        assert!(matches!(
            view.read(cx).tag_panel,
            Some(TagPanel::Preview(_))
        ));
    });
    assert_eq!(tag_testing::contents(&files), before);

    cx.executor().advance_clock(SLOW_PLAN);
    cx.run_until_parked();
    let open_key = spelled(NotationChoice::OpenKey);
    assert_eq!(
        shown_keys(&view, cx),
        (vec![open_key.clone(); files.len()], false)
    );

    click(window, "confirm-tag-write", cx);
    assert_eq!(keys_in_files(&files), vec![Some(open_key); files.len()]);
}

#[gpui::test]
fn a_slow_plan_for_older_settings_arriving_late_is_ignored(cx: &mut TestAppContext) {
    let (_scratch, files, _window, view) = open_preview("preview-late-plan", cx);

    delay_next_plan(&view, cx);
    set_notation(&view, NotationChoice::Musical, cx);
    set_notation(&view, NotationChoice::OpenKey, cx);
    cx.run_until_parked();
    let open_key = spelled(NotationChoice::OpenKey);
    assert_eq!(
        shown_keys(&view, cx),
        (vec![open_key.clone(); files.len()], false)
    );

    cx.executor().advance_clock(SLOW_PLAN);
    cx.run_until_parked();
    assert_eq!(
        shown_keys(&view, cx),
        (vec![open_key; files.len()], false),
        "the plan for Musical came last and was dropped"
    );
}

#[gpui::test]
fn a_preview_whose_settings_changed_unseen_is_worked_out_again_instead_of_written(
    cx: &mut TestAppContext,
) {
    let (_scratch, files, window, view) = open_preview("preview-settings-guard", cx);
    let before = tag_testing::contents(&files);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.tag_settings.key_notation = NotationChoice::Musical;
        })
    });

    click(window, "confirm-tag-write", cx);
    assert_eq!(tag_testing::contents(&files), before);
    let musical = spelled(NotationChoice::Musical);
    assert_eq!(
        shown_keys(&view, cx),
        (vec![musical.clone(); files.len()], false)
    );

    click(window, "confirm-tag-write", cx);
    assert_eq!(keys_in_files(&files), vec![Some(musical); files.len()]);
}
