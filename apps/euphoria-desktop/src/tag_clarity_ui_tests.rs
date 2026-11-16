// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests of the one key notation, the Write tags preview, the "In your
//! files" box and the helpers of the writing settings, in the real root
//! view in a headless window.

use euphoria_core::tags::{TagValues, apply_plan, plan_write};
use euphoria_core::{Key, Mode, TrackAnalysis};
use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext, px};

use crate::app::EuphoriaApp;
use crate::columns::ColumnId;
use crate::key_by_ear::CorrectionWriter;
use crate::label_preview::{
    LabelPreview, LibraryChange, PreviewInput, plan_library_change, plan_preview,
};
use crate::tag_flow::{PreviewKind, TagPanel};
use crate::tag_settings::{LabelChoice, NotationChoice, ProgramChoice, TagSettings};
use crate::tag_writing::testing::{self as tag_testing, ScratchFolder};
use crate::tag_writing::{PreviewRow, WriteTarget};
use crate::ui_tests::{
    click, is_shown, open_taggable_files, open_taggable_files_sized, scroll_settings_to,
};
use crate::views::library_export_panel::keys_line;
use crate::views::tag_panel::{rules, write_button_label};

fn open_files(
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
    (scratch, files, window, view)
}

fn open_files_in_a_tall_window(
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
    let (window, view) = open_taggable_files_sized(
        &scratch,
        &files,
        tag_testing::analysis(),
        None,
        gpui::size(px(1440.0), px(3000.0)),
        cx,
    );
    (scratch, files, window, view)
}

fn settle(cx: &mut TestAppContext) {
    cx.executor()
        .advance_clock(crate::label_preview::LIBRARY_COUNT_DELAY * 2);
    cx.run_until_parked();
}

fn open_settings(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) {
    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    settle(cx);
}

fn height_of(window: AnyWindowHandle, id: &str, cx: &mut TestAppContext) -> gpui::Pixels {
    let id = gpui::ElementId::from(gpui::SharedString::from(id.to_string()));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.find(id).bounds().size.height
    })
    .unwrap()
}

#[gpui::test]
fn one_key_notation_drives_the_table_the_tags_and_the_export(cx: &mut TestAppContext) {
    let (_scratch, _files, window, view) = open_files("clarity-one-notation", cx);
    cx.update(|cx| {
        let codes = view.read(cx).column_texts(ColumnId::Key);
        assert!(codes.contains("8A"), "{codes:?}");
    });
    open_settings(&view, cx);
    assert!(is_shown(window, ("notation", 0_usize), cx));
    assert!(
        !is_shown(window, ("key-display", 0_usize), cx),
        "the table no longer has a picker of its own"
    );
    click(window, ("notation", 1_usize), cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.column_texts(ColumnId::Key).contains("Am"));
        assert_eq!(app.tag_settings.key_notation, NotationChoice::Musical);
        assert_eq!(
            app.tag_settings.write_options().key_notation,
            euphoria_core::tags::KeyNotation::Musical
        );
    });
    click(window, "close-settings", cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| app.open_library_export(cx));
        let app = view.read(cx);
        let setup = app.library_export.as_ref().unwrap().setup.clone();
        let job = app.library_export_job(&setup);
        assert_eq!(job.setup.key_notation, NotationChoice::Musical);
    });
}

#[gpui::test]
fn export_names_the_notation_read_only_and_the_link_opens_settings(cx: &mut TestAppContext) {
    let (_scratch, _files, window, view) = open_files("clarity-export-line", cx);
    cx.update(|cx| view.update(cx, |app, cx| app.open_library_export(cx)));
    assert!(is_shown(window, "export-key-notation-line", cx));
    assert!(is_shown(window, "export-key-notation", cx));
    for notation in NotationChoice::ALL {
        assert!(
            !is_shown(
                window,
                gpui::SharedString::from(format!("export-notation-{notation:?}")),
                cx
            ),
            "{notation:?}"
        );
    }
    assert_eq!(keys_line(NotationChoice::Camelot), "Keys as Camelot (8A)");
    assert_eq!(keys_line(NotationChoice::OpenKey), "Keys as Open Key (1m)");

    click(window, "export-change-key-notation", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.settings_open);
        assert!(
            app.library_export.is_some(),
            "the export sheet waits under the settings"
        );
    });
    click(window, ("notation", 2_usize), cx);
    cx.update(|cx| {
        let panel = view.read(cx).library_export.as_ref().unwrap();
        assert!(matches!(
            panel.stage,
            crate::library_export::ExportStage::Setup
        ));
        assert_eq!(
            view.read(cx).tag_settings.key_notation,
            NotationChoice::OpenKey
        );
    });
}

#[gpui::test]
fn the_write_preview_names_fields_in_words_with_the_frame_in_a_tooltip(cx: &mut TestAppContext) {
    let (_scratch, _files, window, view) = open_files("clarity-rows", cx);
    click(window, "write-tags", cx);
    click(window, "scope-shown", cx);
    let rows = cx.update(|cx| match &view.read(cx).tag_panel {
        Some(TagPanel::Preview(preview)) => preview.rows.clone(),
        other => panic!("{other:?}"),
    });
    let named: Vec<(String, Option<&'static str>)> = rows
        .iter()
        .filter_map(|row| match row {
            PreviewRow::Change(change, name) => Some((change.field.clone(), *name)),
            _ => None,
        })
        .collect();
    assert!(
        named.contains(&("TKEY".to_string(), Some("Key"))),
        "{named:?}"
    );
    assert!(
        named.contains(&("TBPM".to_string(), Some("BPM"))),
        "{named:?}"
    );
    let first_change = rows
        .iter()
        .position(|row| matches!(row, PreviewRow::Change(..)))
        .unwrap();
    assert!(is_shown(window, ("tag-preview-field", first_change), cx));
}

#[gpui::test]
fn the_write_preview_shows_each_named_value_once_per_file(cx: &mut TestAppContext) {
    let (_scratch, _files, window, view) = open_files("clarity-once", cx);
    click(window, "write-tags", cx);
    click(window, "scope-shown", cx);
    let rows = cx.update(|cx| match &view.read(cx).tag_panel {
        Some(TagPanel::Preview(preview)) => preview.rows.clone(),
        other => panic!("{other:?}"),
    });
    let mut seen: Vec<(Option<&'static str>, Option<String>, Option<String>)> = Vec::new();
    for row in &rows {
        match row {
            PreviewRow::File { change_count, .. } => {
                seen.clear();
                assert!(*change_count > 0);
            }
            PreviewRow::Change(change, Some(name)) => {
                let key = (Some(*name), change.current.clone(), change.new.clone());
                assert!(
                    !seen.contains(&key),
                    "{name} shown twice for one file: {rows:?}"
                );
                seen.push(key);
            }
            _ => {}
        }
    }
}

#[test]
fn the_rules_list_matches_the_settings() {
    let defaults = rules(&TagSettings::default());
    assert_eq!(
        defaults,
        [
            "Keys and tempos already in files are kept",
            "Low confidence values are skipped",
            "No energy label is written",
            "Keys as Camelot (8A)",
        ]
    );
    let rekordbox = TagSettings {
        program: ProgramChoice::Rekordbox,
        ..TagSettings::default()
    };
    assert_eq!(
        rules(&rekordbox)[2],
        "Energy label at the start of the comment"
    );
    let changed = TagSettings {
        program: ProgramChoice::Rekordbox,
        label: Some(LabelChoice::Grouping),
        write_tempo: false,
        replace_existing: true,
        include_low_confidence: true,
        ..TagSettings::default()
    };
    assert_eq!(
        rules(&changed),
        [
            "Keys already in files are replaced",
            "Low confidence values are written",
            "Energy label at the start of the grouping",
            "The tempo is not written",
        ]
    );
    let with_cues = TagSettings {
        write_cue_markers: true,
        colour_cues_by_energy: true,
        ..TagSettings::default()
    };
    assert_eq!(
        rules(&with_cues)[3],
        "Cue markers are written as Serato cues"
    );
    for settings in [TagSettings::default(), changed, with_cues] {
        let count = rules(&settings).len();
        assert!((3..=4).contains(&count), "{count}");
    }
}

#[gpui::test]
fn the_write_preview_shows_rules_a_settings_link_and_a_counted_button(cx: &mut TestAppContext) {
    let (_scratch, files, window, view) = open_files("clarity-panel", cx);
    click(window, "write-tags", cx);
    click(window, "scope-shown", cx);
    assert!(is_shown(window, "tag-rules", cx));
    cx.update(|cx| {
        let Some(TagPanel::Preview(preview)) = &view.read(cx).tag_panel else {
            panic!("a preview");
        };
        assert!(matches!(preview.kind, PreviewKind::Analysis { .. }));
        let shown = rules(&preview.settings).len();
        assert!(shown >= 3);
        assert_eq!(preview.plans.len(), files.len());
    });
    for index in 0..rules(&TagSettings::default()).len() {
        assert!(is_shown(window, ("tag-rule", index), cx), "{index}");
    }
    assert_eq!(write_button_label(18), "Write tags to 18 files");
    assert_eq!(write_button_label(1), "Write tags to 1 file");
    click(window, "change-tag-settings", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.settings_open);
        assert!(matches!(app.tag_panel, Some(TagPanel::Preview(_))));
    });
}

fn example_preview(settings: &TagSettings) -> LabelPreview {
    plan_preview(
        PreviewInput::choose(None).unwrap(),
        settings,
        None,
        &CorrectionWriter::default(),
        None,
    )
}

fn rekordbox() -> TagSettings {
    TagSettings {
        program: ProgramChoice::Rekordbox,
        label: Some(LabelChoice::CommentStart),
        ..TagSettings::default()
    }
}

fn row_named<'a>(
    preview: &'a LabelPreview,
    name: &str,
) -> Option<&'a crate::label_preview::PreviewField> {
    preview.fields.iter().find(|field| field.name == name)
}

#[test]
fn turning_off_the_tempo_strikes_out_the_bpm_row() {
    let on = example_preview(&rekordbox());
    assert_eq!(row_named(&on, "BPM").unwrap().struck, None);
    let off = example_preview(&TagSettings {
        write_tempo: false,
        ..rekordbox()
    });
    let bpm = row_named(&off, "BPM").expect("a struck row, not a missing one");
    assert_eq!(bpm.struck.as_deref(), Some("not written"));
    assert_eq!(bpm.value(), "128");
    assert_eq!(row_named(&off, "Key").unwrap().struck, None);
}

#[test]
fn turning_off_the_cue_markers_strikes_out_the_cue_rows() {
    let off = example_preview(&rekordbox());
    for name in ["Cue markers", "Beat grid"] {
        assert_eq!(
            row_named(&off, name).unwrap().struck.as_deref(),
            Some("not written"),
            "{name}"
        );
    }
    let on = example_preview(&TagSettings {
        write_cue_markers: true,
        colour_cues_by_energy: true,
        ..rekordbox()
    });
    for name in ["Cue markers", "Beat grid"] {
        assert_eq!(row_named(&on, name).unwrap().struck, None, "{name}");
    }
    assert!(
        row_named(&on, "Cue markers")
            .unwrap()
            .value()
            .starts_with("cue points")
    );
}

fn selected(path: &std::path::Path, analysis: TrackAnalysis) -> PreviewInput {
    PreviewInput::choose(Some(WriteTarget {
        path: path.to_path_buf(),
        analysis: Some(analysis),
        correction: Default::default(),
        edited_cues: None,
        saved_loops: Vec::new(),
    }))
    .unwrap()
}

fn preview_of(input: PreviewInput, settings: &TagSettings) -> LabelPreview {
    plan_preview(input, settings, None, &CorrectionWriter::default(), None)
}

#[test]
fn low_confidence_values_are_struck_out_until_the_option_writes_them() {
    let scratch = ScratchFolder::new("clarity-low-confidence");
    let files = scratch.copy_fixtures();
    let unsure = TrackAnalysis {
        key_runner_up: Some(Key::new(4, Mode::Minor)),
        key_score_margin: Some(0.0),
        tempo_estimate: Some(75.0),
        tempo_score_margin: Some(0.01),
        ..tag_testing::analysis()
    };
    let skipped = preview_of(selected(&files[0], unsure.clone()), &rekordbox());
    assert_eq!(
        row_named(&skipped, "Key").unwrap().struck.as_deref(),
        Some("not written, low confidence")
    );
    assert_eq!(
        row_named(&skipped, "BPM").unwrap().struck.as_deref(),
        Some("not written, low confidence")
    );
    let written = preview_of(
        selected(&files[0], unsure),
        &TagSettings {
            include_low_confidence: true,
            ..rekordbox()
        },
    );
    assert_eq!(row_named(&written, "Key").unwrap().struck, None);
    assert_eq!(row_named(&written, "BPM").unwrap().struck, None);
}

#[test]
fn a_key_and_tempo_confirmed_with_looks_right_are_not_struck_out_for_low_confidence() {
    let scratch = ScratchFolder::new("clarity-confirmed");
    let files = scratch.copy_fixtures();
    let unsure = TrackAnalysis {
        key_runner_up: Some(Key::new(4, Mode::Minor)),
        key_score_margin: Some(0.0),
        tempo_estimate: Some(75.0),
        tempo_score_margin: Some(0.01),
        ..tag_testing::analysis()
    };
    let confirmed = |correction: euphoria_core::Correction| {
        PreviewInput::choose(Some(WriteTarget {
            path: files[0].clone(),
            analysis: Some(unsure.clone()),
            correction,
            edited_cues: None,
            saved_loops: Vec::new(),
        }))
        .unwrap()
    };
    let key_only = preview_of(
        confirmed(euphoria_core::Correction::default().confirming_key()),
        &rekordbox(),
    );
    assert_eq!(row_named(&key_only, "Key").unwrap().struck, None);
    assert_eq!(
        row_named(&key_only, "BPM").unwrap().struck.as_deref(),
        Some("not written, low confidence")
    );
    let both = preview_of(
        confirmed(
            euphoria_core::Correction::default()
                .confirming_key()
                .confirming_bpm(),
        ),
        &rekordbox(),
    );
    assert_eq!(row_named(&both, "Key").unwrap().struck, None);
    assert_eq!(row_named(&both, "BPM").unwrap().struck, None);
}

fn seed_with_key(files: &[std::path::PathBuf]) {
    let seeding = TagSettings {
        replace_existing: true,
        ..rekordbox()
    };
    for path in files {
        let plan = plan_write(
            path,
            TagValues::from(&tag_testing::analysis()),
            &seeding.write_options(),
        )
        .unwrap();
        apply_plan(&plan).unwrap();
    }
}

#[test]
fn a_key_already_in_the_file_is_struck_out_until_replacing() {
    let scratch = ScratchFolder::new("clarity-replace");
    let files = scratch.copy_fixtures();
    seed_with_key(&files);
    let other = TrackAnalysis {
        key: Some(Key::new(4, Mode::Minor)),
        bpm: Some(150.0),
        ..tag_testing::analysis()
    };
    let kept = preview_of(selected(&files[0], other.clone()), &rekordbox());
    assert_eq!(
        row_named(&kept, "Key").unwrap().struck.as_deref(),
        Some("kept as it is in the file")
    );
    let replaced = preview_of(
        selected(&files[0], other),
        &TagSettings {
            replace_existing: true,
            ..rekordbox()
        },
    );
    assert_eq!(row_named(&replaced, "Key").unwrap().struck, None);
}

fn library_targets(files: &[std::path::PathBuf], analysis: TrackAnalysis) -> Vec<WriteTarget> {
    files
        .iter()
        .map(|path| WriteTarget {
            path: path.clone(),
            analysis: Some(analysis.clone()),
            correction: Default::default(),
            edited_cues: None,
            saved_loops: Vec::new(),
        })
        .collect()
}

fn library_change(
    files: &[std::path::PathBuf],
    analysis: TrackAnalysis,
    settings: &TagSettings,
) -> LibraryChange {
    plan_library_change(
        library_targets(files, analysis),
        settings,
        None,
        &CorrectionWriter::default(),
        None,
        &crate::tag_reads::TagReads::default(),
    )
}

#[test]
fn the_library_count_follows_the_options_and_reads_as_a_sentence() {
    let scratch = ScratchFolder::new("clarity-library-count");
    let files = scratch.copy_fixtures();
    let fresh = library_change(&files, tag_testing::analysis(), &rekordbox());
    assert_eq!(
        fresh,
        LibraryChange {
            would_change: 3,
            total: 3
        }
    );
    assert_eq!(
        fresh.sentence(),
        "Would change in your library: 3 of 3 files"
    );

    seed_with_key(&files);
    let other = TrackAnalysis {
        key: Some(Key::new(4, Mode::Minor)),
        bpm: Some(150.0),
        ..tag_testing::analysis()
    };
    let kept = library_change(&files, other.clone(), &rekordbox());
    assert_eq!(kept.would_change, 0, "the files keep their keys and tempos");
    let replaced = library_change(
        &files,
        other,
        &TagSettings {
            replace_existing: true,
            ..rekordbox()
        },
    );
    assert_eq!(replaced.would_change, 3);
    assert_eq!(replaced.total, 3);
}

#[gpui::test]
fn the_settings_box_counts_the_library_off_the_main_thread_and_shows_struck_rows(
    cx: &mut TestAppContext,
) {
    let (_scratch, files, window, view) = open_files_in_a_tall_window("clarity-box", cx);
    open_settings(&view, cx);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).library_change,
            Some(LibraryChange {
                would_change: files.len(),
                total: files.len()
            })
        );
    });
    assert!(is_shown(window, "label-preview-library", cx));
    click(window, ("program", 0_usize), cx);
    click(window, "write-tempo", cx);
    cx.run_until_parked();
    cx.update(|cx| {
        let app = view.read(cx);
        let preview = app.label_preview.as_ref().unwrap();
        let tempo = row_named(preview, "BPM").unwrap();
        assert_eq!(tempo.struck.as_deref(), Some("not written"));
    });
    click(window, "write-tempo", cx);
    cx.update(|cx| {
        let preview = view.read(cx).label_preview.clone().unwrap();
        assert_eq!(row_named(&preview, "BPM").unwrap().struck, None);
    });
}

#[gpui::test]
fn the_box_is_pinned_once_scrolled_past_while_writing_options_are_in_view(cx: &mut TestAppContext) {
    let (_scratch, _files, window, view) = open_files("clarity-pinned", cx);
    open_settings(&view, cx);
    let scroll = |by: f32, cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.scroll(
                "settings-body",
                gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(by))),
                cx,
            );
        })
        .unwrap();
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .unwrap();
    };
    assert!(!is_shown(window, "label-preview-pinned", cx), "at the top");
    scroll_settings_to(window, ("program", 0_usize), cx);
    scroll(0.0, cx);
    assert!(
        is_shown(window, "label-preview-pinned", cx),
        "in the writing options"
    );
    scroll(-100_000.0, cx);
    assert!(
        is_shown(window, "label-preview-pinned", cx),
        "the last writing options are still in view above the metronome"
    );
    scroll(100_000.0, cx);
    assert!(
        !is_shown(window, "label-preview-pinned", cx),
        "back at the top"
    );
}

const HINTS: [&str; 13] = [
    "key-notation",
    "tempo-range",
    "dj-program",
    "energy-label",
    "write-tempo",
    "replace-existing",
    "include-low-confidence",
    "write-cue-markers",
    "write-after-analysis",
    "metronome-sound",
    "metronome-volume",
    "metronome-accent",
    "separation-mode",
];

const WITH_INFO: [&str; 3] = ["tempo-range", "energy-label", "separation-mode"];

#[gpui::test]
fn every_helper_is_one_line_at_the_default_settings_width(cx: &mut TestAppContext) {
    let (_scratch, _files, window, view) = open_files_in_a_tall_window("clarity-helpers", cx);
    open_settings(&view, cx);
    for program in [0_usize, 1] {
        click(window, ("program", program), cx);
        for key in HINTS {
            let height = height_of(window, &format!("hint-{key}"), cx);
            assert!(
                height < px(24.0),
                "the helper of {key} wraps, {height:?} high, with program {program}"
            );
        }
    }
    for key in WITH_INFO {
        assert!(
            is_shown(
                window,
                gpui::SharedString::from(format!("hint-info-{key}")),
                cx
            ),
            "{key} keeps its longer text in a tooltip"
        );
    }
    assert!(!is_shown(window, "hint-info-dj-program", cx));
}

#[gpui::test]
fn a_second_settings_change_counts_the_library_without_reading_the_files_again(
    cx: &mut TestAppContext,
) {
    let (_scratch, files, window, view) = open_files_in_a_tall_window("clarity-reads", cx);
    open_settings(&view, cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.tag_reads.read_count(), files.len());
        assert_eq!(app.library_change.unwrap().would_change, files.len());
    });
    click(window, ("program", 0_usize), cx);
    click(window, "replace-existing", cx);
    settle(cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(
            app.tag_reads.read_count(),
            files.len(),
            "planned again from memory"
        );
        assert!(app.library_change.is_some(), "and counted again");
    });
}

#[gpui::test]
fn a_change_made_while_the_settings_rest_drops_the_older_count(cx: &mut TestAppContext) {
    let (_scratch, _files, window, view) = open_files_in_a_tall_window("clarity-stale", cx);
    open_settings(&view, cx);
    click(window, ("program", 0_usize), cx);
    click(window, "replace-existing", cx);
    cx.update(|cx| {
        assert!(
            view.read(cx).library_change.is_none(),
            "nothing is counted until the settings rest"
        );
    });
    settle(cx);
    cx.update(|cx| assert!(view.read(cx).library_change.is_some()));
}

#[gpui::test]
fn a_tag_write_makes_the_next_count_read_the_files_again(cx: &mut TestAppContext) {
    let (_scratch, files, window, view) = open_files("clarity-write-invalidates", cx);
    open_settings(&view, cx);
    click(window, "close-settings", cx);
    click(window, "write-tags", cx);
    click(window, "scope-shown", cx);
    click(window, "confirm-tag-write", cx);
    cx.update(|cx| assert_eq!(view.read(cx).written_files(), Some(files.clone())));
    open_settings(&view, cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(
            app.tag_reads.read_count(),
            files.len() * 2,
            "the written files were read again"
        );
        assert_eq!(app.library_change.unwrap().would_change, 0);
    });
}

#[gpui::test]
fn every_on_off_setting_is_a_row_with_a_32_by_18_switch_that_flips_with_a_click(
    cx: &mut TestAppContext,
) {
    let (_scratch, _files, window, view) = open_files_in_a_tall_window("switch-rows", cx);
    open_settings(&view, cx);
    for id in [
        "write-tempo",
        "replace-existing",
        "include-low-confidence",
        "write-cue-markers",
        "write-after-analysis",
        "metronome-accent",
    ] {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let switch = window.find(format!("{id}-switch")).bounds();
            assert_eq!(switch.size.width, px(32.0), "{id}");
            assert_eq!(switch.size.height, px(18.0), "{id}");
        })
        .unwrap();
    }
    let write_tempo =
        |cx: &mut TestAppContext| cx.update(|cx| view.read(cx).tag_settings.write_tempo);
    let before = write_tempo(cx);
    click(window, "write-tempo", cx);
    assert_ne!(write_tempo(cx), before);
    click(window, "write-tempo", cx);
    assert_eq!(write_tempo(cx), before);
}

#[test]
fn a_selected_track_whose_file_is_missing_falls_back_to_the_example() {
    let scratch = ScratchFolder::new("clarity-missing-file");
    let missing = scratch.0.join("gone.aiff");
    let preview = preview_of(selected(&missing, tag_testing::analysis()), &rekordbox());
    assert_eq!(
        preview.source.caption(),
        "Example (the selected track's file is missing)"
    );
    assert!(row_named(&preview, "Key").is_some());
    assert!(
        preview
            .notes
            .iter()
            .all(|note| !note.contains("No such file"))
    );
}
