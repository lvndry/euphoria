// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Writing key and tempo after analysis, and the tag editor's suggestions
//! from the analysis, in a headless window.

use std::path::PathBuf;

use euphoria_core::tags::{TagField, read_editable_tags};
use euphoria_core::{Key, Mode, TrackAnalysis};
use gpui::{AnyWindowHandle, Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::settings::Settings;
use crate::tag_flow::TagPanel;
use crate::tag_settings::{NotationChoice, TagSettings};
use crate::tag_writing::testing::{self as tag_testing, ScratchFolder};
use crate::ui_tests::{click, is_shown, open_taggable_files, open_taggable_files_analysed};

fn turn_on(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) {
    cx.update(|cx| view.update(cx, |app, _| app.tag_settings.write_after_analysis = true));
}

fn analysed(view: &Entity<EuphoriaApp>, paths: &[PathBuf], cx: &mut TestAppContext) -> Vec<String> {
    let paths = paths.to_vec();
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.queue_auto_tags(paths, cx);
            app.busy_work()
        })
    })
}

fn field(path: &std::path::Path, field: TagField) -> Option<String> {
    read_editable_tags(path)
        .unwrap()
        .get(field)
        .map(str::to_string)
}

fn set_fields(path: &std::path::Path, values: &[(TagField, &str)]) {
    use euphoria_core::tags::{FieldEdit, TextChange, apply_plan, plan_edit};
    let edits: Vec<FieldEdit> = values
        .iter()
        .map(|(field, text)| FieldEdit {
            field: *field,
            change: TextChange::Set(text.to_string()),
        })
        .collect();
    apply_plan(&plan_edit(path, &edits).unwrap()).unwrap();
}

fn low_confidence() -> TrackAnalysis {
    TrackAnalysis {
        key_runner_up: Some(Key::new(4, Mode::Minor)),
        key_score_margin: Some(0.0),
        tempo_estimate: Some(75.0),
        tempo_score_margin: Some(0.01),
        ..tag_testing::analysis()
    }
}

#[gpui::test]
fn with_the_setting_off_nothing_is_written_after_analysis(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("auto-off");
    let files = scratch.copy_fixtures();
    let before = tag_testing::contents(&files);
    let (_, view) = open_taggable_files(&scratch, &files, None, cx);
    cx.update(|cx| view.update(cx, |app, _| app.tag_settings.write_after_analysis = false));

    analysed(&view, &files, cx);
    cx.run_until_parked();
    assert_eq!(tag_testing::contents(&files), before);
    assert!(!scratch.log_directory().exists());
    cx.update(|cx| assert_eq!(view.read(cx).auto_tags.pending_count(), 0));
}

#[gpui::test]
fn with_the_setting_on_empty_fields_are_filled_and_existing_values_kept(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("auto-on");
    let files = scratch.copy_fixtures();
    set_fields(&files[0], &[(TagField::Key, "5A"), (TagField::Bpm, "100")]);
    let (_, view) = open_taggable_files(&scratch, &files, None, cx);
    turn_on(&view, cx);

    analysed(&view, &files, cx);
    cx.run_until_parked();

    assert_eq!(field(&files[0], TagField::Key).as_deref(), Some("5A"));
    assert_eq!(field(&files[0], TagField::Bpm).as_deref(), Some("100"));
    for path in &files[1..] {
        assert!(field(path, TagField::Key).is_some(), "{path:?}");
        assert_eq!(field(path, TagField::Bpm).as_deref(), Some("124"));
    }
    cx.update(|cx| assert!(!view.read(cx).is_busy()));
}

#[gpui::test]
fn low_confidence_values_are_skipped_unless_the_setting_includes_them(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("auto-low");
    let files = scratch.copy_fixtures();
    let before = tag_testing::contents(&files);
    let (_, view) = open_taggable_files_analysed(&scratch, &files, low_confidence(), None, cx);
    turn_on(&view, cx);
    analysed(&view, &files, cx);
    cx.run_until_parked();
    assert_eq!(tag_testing::contents(&files), before);

    cx.update(|cx| view.update(cx, |app, _| app.tag_settings.include_low_confidence = true));
    analysed(&view, &files, cx);
    cx.run_until_parked();
    assert!(field(&files[1], TagField::Bpm).is_some());
    assert!(field(&files[1], TagField::Key).is_some());
}

#[gpui::test]
fn analysis_loaded_from_the_cache_queues_nothing(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("auto-cached");
    let files = scratch.copy_fixtures();
    let before = tag_testing::contents(&files);
    let (_, view) = open_taggable_files(&scratch, &files, None, cx);
    turn_on(&view, cx);
    cx.update(|cx| view.update(cx, |app, cx| app.queue_fresh_analyses(cx)));
    cx.run_until_parked();
    assert_eq!(tag_testing::contents(&files), before);
    assert!(!scratch.log_directory().exists());
}

#[gpui::test]
fn a_running_dj_program_holds_the_writes_until_write_now(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("auto-held");
    let files = scratch.copy_fixtures();
    let before = tag_testing::contents(&files);
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    turn_on(&view, cx);
    cx.update(|cx| view.update(cx, |app, _| app.program_checks.rekordbox = || Some(true)));

    analysed(&view, &files, cx);
    cx.run_until_parked();
    assert_eq!(tag_testing::contents(&files), before, "nothing is written");
    assert!(is_shown(window, "held-tags", cx));
    cx.update(|cx| {
        let tags = &view.read(cx).auto_tags;
        assert_eq!(tags.held_by(), ["rekordbox"]);
        assert_eq!(
            crate::auto_tags::held_sentence(tags.pending_count(), tags.held_by()),
            "3 tracks have a key and tempo to write into the file. Waiting because rekordbox is \
             open: close it and choose Write tags, or write now."
        );
    });

    click(window, "write-held-tags", cx);
    assert_ne!(tag_testing::contents(&files), before);
    assert!(!is_shown(window, "held-tags", cx));
    cx.update(|cx| assert_eq!(view.read(cx).auto_tags.pending_count(), 0));
}

#[gpui::test]
fn files_that_cannot_be_written_are_reported_in_one_notice(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("auto-errors");
    let mut files = scratch.copy_fixtures();
    files.push(scratch.0.join("music/gone-one.mp3"));
    files.push(scratch.0.join("music/gone-two.mp3"));
    let (_, view) = open_taggable_files(&scratch, &files, None, cx);
    turn_on(&view, cx);

    analysed(&view, &files, cx);
    cx.run_until_parked();

    cx.update(|cx| {
        let notice = view.read(cx).notice.clone().expect("a notice");
        assert!(notice.contains("2 tracks"), "{notice}");
        assert_eq!(
            notice.matches("could not be written").count(),
            1,
            "{notice}"
        );
    });
    assert!(
        field(&files[0], TagField::Key).is_some(),
        "the rest is written"
    );
}

#[gpui::test]
fn an_automatic_write_can_be_undone(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("auto-undo");
    let files = scratch.copy_fixtures();
    let before = tag_testing::contents(&files);
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    turn_on(&view, cx);
    analysed(&view, &files, cx);
    cx.run_until_parked();
    assert_ne!(tag_testing::contents(&files), before);

    cx.update(|cx| view.update(cx, |app, cx| app.open_tag_undo(cx)));
    click(window, "confirm-tag-undo", cx);
    assert_eq!(tag_testing::contents(&files), before);
}

#[gpui::test]
fn the_logo_is_busy_while_tags_are_written_after_analysis(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("auto-busy");
    let files = scratch.copy_fixtures();
    let (_, view) = open_taggable_files(&scratch, &files, None, cx);
    turn_on(&view, cx);

    let during = analysed(&view, &files, cx);
    assert_eq!(during, ["Writing tags"]);
    cx.run_until_parked();
    cx.update(|cx| assert!(!view.read(cx).is_busy()));
}

#[gpui::test]
fn the_setting_is_a_button_in_the_settings_and_is_remembered(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("auto-setting");
    let files = scratch.copy_fixtures();
    let settings_path = scratch.0.join("settings.json");
    let (_window, view) = open_taggable_files(&scratch, &files, Some(settings_path.clone()), cx);
    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    cx.update(|cx| view.update(cx, |app, cx| app.toggle_write_after_analysis(cx)));
    cx.update(|cx| assert!(!view.read(cx).tag_settings.write_after_analysis));
    assert!(
        !Settings::load(&settings_path)
            .unwrap()
            .tags
            .write_after_analysis
    );
    cx.update(|cx| view.update(cx, |app, cx| app.toggle_write_after_analysis(cx)));
    assert_eq!(
        Settings::load(&settings_path).unwrap().tags,
        TagSettings::default()
    );
}

fn open_editor(
    name: &str,
    notation: NotationChoice,
    prepare: impl FnOnce(&[PathBuf]),
    cx: &mut TestAppContext,
) -> (
    ScratchFolder,
    Vec<PathBuf>,
    AnyWindowHandle,
    Entity<EuphoriaApp>,
) {
    let scratch = ScratchFolder::new(name);
    let files = scratch.copy_fixtures();
    prepare(&files);
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.tag_settings.key_notation = notation;
            app.select_all_shown(cx);
        })
    });
    click(window, "edit-tags", cx);
    (scratch, files, window, view)
}

fn typed(view: &Entity<EuphoriaApp>, field: TagField, cx: &mut TestAppContext) -> String {
    cx.update(|cx| match &view.read(cx).tag_panel {
        Some(TagPanel::Editing(editor)) => {
            editor.input(field).unwrap().read(cx).value().to_string()
        }
        other => panic!("{other:?}"),
    })
}

#[gpui::test]
fn the_editor_suggests_the_analysis_and_fill_from_analysis_types_it_without_saving(
    cx: &mut TestAppContext,
) {
    let (_scratch, files, window, view) =
        open_editor("auto-suggest", NotationChoice::Musical, |_| {}, cx);
    let before = tag_testing::contents(&files);

    cx.update(|cx| match &view.read(cx).tag_panel {
        Some(TagPanel::Editing(editor)) => {
            assert_eq!(editor.suggestion(TagField::Key), Some("Am"));
            assert_eq!(editor.suggestion(TagField::Bpm), Some("124"));
        }
        other => panic!("{other:?}"),
    });
    assert_eq!(
        typed(&view, TagField::Bpm, cx),
        "",
        "a placeholder, not text"
    );
    assert!(is_shown(window, "fill-from-analysis", cx));

    click(window, "fill-from-analysis", cx);
    assert_eq!(typed(&view, TagField::Key, cx), "Am");
    assert_eq!(typed(&view, TagField::Bpm, cx), "124");
    assert_eq!(tag_testing::contents(&files), before, "nothing is saved");
    assert!(!is_shown(window, "fill-from-analysis", cx));

    click(window, "preview-tag-edits", cx);
    cx.update(|cx| match &view.read(cx).tag_panel {
        Some(TagPanel::Preview(preview)) => assert_eq!(preview.plans.len(), 3),
        other => panic!("{other:?}"),
    });
    assert_eq!(tag_testing::contents(&files), before, "only a preview");
}

#[gpui::test]
fn a_field_the_files_already_have_gets_no_suggestion_and_one_use_fills_one_field(
    cx: &mut TestAppContext,
) {
    let (_scratch, files, window, view) = open_editor(
        "auto-suggest-partial",
        NotationChoice::Camelot,
        |files| {
            for path in files {
                set_fields(path, &[(TagField::Key, "5A")]);
            }
        },
        cx,
    );
    cx.update(|cx| match &view.read(cx).tag_panel {
        Some(TagPanel::Editing(editor)) => {
            assert_eq!(editor.suggestion(TagField::Key), None);
            assert_eq!(editor.suggestion(TagField::Bpm), Some("124"));
        }
        other => panic!("{other:?}"),
    });
    assert_eq!(typed(&view, TagField::Key, cx), "5A");

    click(
        window,
        crate::views::tag_editor::use_button_id(TagField::Bpm),
        cx,
    );
    assert_eq!(typed(&view, TagField::Bpm, cx), "124");
    assert_eq!(field(&files[0], TagField::Bpm), None);
}
