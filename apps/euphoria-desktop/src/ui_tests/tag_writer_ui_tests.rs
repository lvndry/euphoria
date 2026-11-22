// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// 17 tests: writing tags, the programs, the cache and the column widths.

/// Index of the Format column among the table's column headers.
const FORMAT_COLUMN: usize = 11;

/// A window on scratch copies of the core's test files, each analyzed with
/// a key, a tempo and an energy rating, writing its tag backups into the
/// scratch folder.
pub(crate) fn open_taggable_files(
    scratch: &ScratchFolder,
    files: &[PathBuf],
    settings_path: Option<PathBuf>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_taggable_files_analysed(scratch, files, tag_testing::analysis(), settings_path, cx)
}

pub(crate) fn open_taggable_files_analysed(
    scratch: &ScratchFolder,
    files: &[PathBuf],
    analysis: TrackAnalysis,
    settings_path: Option<PathBuf>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_taggable_files_sized(scratch, files, analysis, settings_path, USUAL_WINDOW, cx)
}

pub(crate) fn open_taggable_files_sized(
    scratch: &ScratchFolder,
    files: &[PathBuf],
    analysis: TrackAnalysis,
    settings_path: Option<PathBuf>,
    window_size: gpui::Size<gpui::Pixels>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let tracks = files
        .iter()
        .map(|path| DemoTrack {
            path: path.clone(),
            outcome: Ok(analysis.clone()),
        })
        .collect();
    let (window, view) = open_window_with(
        tracks,
        Box::new(SilentOutput),
        Settings::default(),
        settings_path,
        window_size,
        cx,
    );
    let log_directory = scratch.log_directory();
    cx.update(|cx| {
        view.update(cx, |app, _| app.write_log_directory = Some(log_directory));
    });
    (window, view)
}

pub(crate) fn click(
    window: AnyWindowHandle,
    id: impl Into<gpui::ElementId>,
    cx: &mut TestAppContext,
) {
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

    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.tag_panel.is_none(), "a clean write leaves no sheet");
        assert!(app.toasts.offers_undo());
    });
    click(window, "toast-action", cx);
    assert_eq!(tag_testing::contents(&files), before);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.tag_panel.is_none());
        assert_eq!(
            app.toasts.current().map(|toast| toast.message.as_str()),
            Some("Put the old tags back in 3 files")
        );
    });
}

#[gpui::test]
fn a_running_dj_program_makes_the_preview_ask_before_it_writes(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("ui-program-running");
    let files = scratch.copy_fixtures();
    let before = tag_testing::contents(&files);
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.program_checks.rekordbox = || Some(true);
        })
    });

    click(window, "write-tags", cx);
    click(window, "scope-shown", cx);
    click(window, "confirm-tag-write", cx);
    assert!(is_shown(window, "program-running-strip", cx));
    assert!(is_shown(window, "confirm-tag-write-anyway", cx));
    assert_eq!(
        tag_testing::contents(&files),
        before,
        "the first confirmation only asks"
    );
    cx.update(|cx| {
        assert!(matches!(
            &view.read(cx).tag_panel,
            Some(TagPanel::Preview(preview))
                if preview.program_warning.as_deref() == Some(&["rekordbox"][..])
        ));
    });

    click(window, "confirm-tag-write-anyway", cx);
    let written = cx.update(|cx| view.read(cx).written_files());
    assert_eq!(written, Some(files.clone()));
    assert_ne!(tag_testing::contents(&files), before);
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
            edited_cues: None,
            saved_loops: Vec::new(),
        })
        .collect();
    let planned = tag_writing::plan_writes(&targets, &TagSettings::default());
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

/// Scrolls the settings until the element `id` is at the top of the body.
pub(crate) fn scroll_settings_to(
    window: AnyWindowHandle,
    id: impl Into<gpui::ElementId>,
    cx: &mut TestAppContext,
) {
    let id = id.into();
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let below_top =
            window.find(id).bounds().top() - window.find("settings-body").bounds().top();
        window.scroll(
            "settings-body",
            gpui::ScrollDelta::Pixels(gpui::point(px(0.0), -below_top)),
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui::test]
fn the_settings_choose_the_program_the_tags_are_written_for_and_are_remembered(
    cx: &mut TestAppContext,
) {
    let scratch = ScratchFolder::new("ui-settings");
    let files = scratch.copy_fixtures();
    let settings_path = scratch.0.join("settings.json");
    let (window, view) = open_taggable_files(&scratch, &files, Some(settings_path.clone()), cx);

    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    cx.update(|cx| assert!(view.read(cx).settings_open));
    click(window, ("notation", 2_usize), cx);
    scroll_settings_to(window, ("program", 0_usize), cx);
    click(window, ("program", 1_usize), cx);
    click(window, ("label-place", 2_usize), cx);
    cx.update(|cx| {
        let settings = view.read(cx).tag_settings;
        assert_eq!(settings.program, ProgramChoice::Serato);
        assert!(!settings.write_tempo, "Serato starts without the tempo");
    });
    scroll_settings_to(window, "write-tempo", cx);
    click(window, "write-tempo", cx);
    click(window, "close-settings", cx);
    cx.update(|cx| assert!(!view.read(cx).settings_open));

    let saved = Settings::load(&settings_path).unwrap().tags;
    assert_eq!(
        saved,
        TagSettings {
            program: ProgramChoice::Serato,
            key_notation: NotationChoice::OpenKey,
            label: Some(crate::tag_settings::LabelChoice::Grouping),
            write_tempo: true,
            write_cue_markers: false,
            colour_cues_by_energy: true,
            ..TagSettings::default()
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
        assert!(matches!(app.tag_panel, Some(TagPanel::Preview(_))));
    });
}

#[gpui::test]
fn the_settings_preview_follows_the_label_place_as_it_is_chosen(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("ui-label-preview");
    let files = scratch.copy_fixtures();
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    cx.run_until_parked();
    scroll_settings_to(window, ("program", 0_usize), cx);
    click(window, ("program", 0_usize), cx);
    click(window, ("label-place", 0_usize), cx);
    cx.run_until_parked();
    let comment_of = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            let preview = view.read(cx).label_preview.clone().expect("a preview");
            let comment = preview
                .fields
                .into_iter()
                .find(|field| field.name == "Comment")
                .expect("a comment");
            (preview.source, comment.value(), comment.label)
        })
    };
    let (source, value, label) = comment_of(cx);
    assert_eq!(source, crate::label_preview::PreviewSource::SelectedTrack);
    assert!(value.starts_with("8A - Energy 7"), "{value}");
    assert_eq!(label, "8A - Energy 7");

    click(window, ("label-place", 1_usize), cx);
    cx.run_until_parked();
    let moved = comment_of(cx).1;
    assert!(moved.ends_with("8A - Energy 7"), "{moved}");
}

#[gpui::test]
fn the_cue_markers_setting_writes_serato_cues_with_a_preview_and_undo_restores_every_byte(
    cx: &mut TestAppContext,
) {
    let scratch = ScratchFolder::new("ui-cue-markers");
    let files = scratch.copy_fixtures();
    let before = tag_testing::contents(&files);
    let mut analysis = tag_testing::analysis();
    analysis.beat_grid = Some(euphoria_core::detect::BeatGrid {
        first_downbeat_seconds: 0.1,
        bpm: 124.0,
        salience: 10.0,
        drift_seconds: None,
    });
    analysis.cues = vec![euphoria_core::detect::SectionCue {
        seconds: 0.1,
        section: euphoria_core::detect::CueSection::Start,
        energy: None,
    }];
    let tracks = files
        .iter()
        .map(|path| DemoTrack {
            path: path.clone(),
            outcome: Ok(analysis.clone()),
        })
        .collect();
    let settings_path = scratch.0.join("settings.json");
    let (window, view) = open_window_with(
        tracks,
        Box::new(SilentOutput),
        Settings::default(),
        Some(settings_path.clone()),
        USUAL_WINDOW,
        cx,
    );
    let log_directory = scratch.log_directory();
    let mp3 = files[0].clone();
    let wav = files[2].clone();
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.write_log_directory = Some(log_directory);
            app.session.set_edited_cues(
                &mp3,
                Some(vec![
                    euphoria_core::Cue {
                        seconds: 0.5,
                        name: "Mine".to_string(),
                        hot_cue: euphoria_core::track_cues::HotCue::from_pad(3),
                        energy: None,
                    },
                    euphoria_core::Cue {
                        seconds: 0.25,
                        name: "Memory".to_string(),
                        hot_cue: None,
                        energy: None,
                    },
                ]),
            );
        })
    });

    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.scroll(
            "settings-body",
            gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(-100_000.0))),
            cx,
        );
    })
    .unwrap();
    click(window, "write-cue-markers", cx);
    click(window, "close-settings", cx);
    assert!(
        Settings::load(&settings_path)
            .unwrap()
            .tags
            .write_cue_markers
    );

    click(window, "write-tags", cx);
    click(window, "scope-shown", cx);
    let described = cx.update(|cx| match &view.read(cx).tag_panel {
        Some(TagPanel::Preview(preview)) => {
            assert!(preview.settings.write_cue_markers);
            assert!(
                crate::views::tag_panel::rules(&preview.settings)
                    .contains(&"Cue markers are written as Serato cues".to_string())
            );
            assert_eq!(preview.serato_notes.len(), 1, "{:?}", preview.serato_notes);
            assert_eq!(preview.serato_notes[0].path, wav);
            assert!(preview.serato_notes[0].reason.contains("WAV"));
            preview
                .plans
                .iter()
                .find(|plan| plan.path() == mp3)
                .unwrap()
                .changes()
                .iter()
                .filter(|change| euphoria_core::tags::serato_field_name(&change.field).is_some())
                .map(|change| {
                    crate::views::tag_panel::shown_value(&change.field, change.new.as_deref())
                })
                .collect::<Vec<_>>()
        }
        other => panic!("{other:?}"),
    });
    assert!(
        described.contains(&"cue points 4 Mine at 0:00.500".to_string()),
        "{described:?}"
    );
    assert!(
        described
            .iter()
            .any(|value| value.starts_with("beat grid from 0.100 s at 124")),
        "{described:?}"
    );
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("serato-notes-section").is_some());
    })
    .unwrap();
    assert_eq!(
        tag_testing::contents(&files),
        before,
        "a preview writes nothing"
    );

    click(window, "confirm-tag-write", cx);
    let written = cx.update(|cx| view.read(cx).written_files());
    assert_eq!(written, Some(files.clone()));
    assert_ne!(tag_testing::contents(&files), before);

    click(window, "write-tags", cx);
    click(window, "scope-shown", cx);
    assert_eq!(
        preview_file_count(&view, cx),
        Some(0),
        "the markers are already there"
    );
    click(window, "cancel-tag-write", cx);

    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    click(window, "settings-undo", cx);
    click(window, "confirm-tag-undo", cx);
    cx.update(|cx| match &view.read(cx).tag_panel {
        Some(TagPanel::Undone(Ok(Some(report)))) => {
            assert_eq!(report.restored.len(), files.len());
            assert!(report.failures.is_empty(), "{:?}", report.failures);
        }
        other => panic!("{other:?}"),
    });
    assert_eq!(tag_testing::contents(&files), before);
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
    cx.run_until_parked();
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
    cx.run_until_parked();
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
/// hand: `cargo test -p euphoria-desktop -- --ignored bench_clips`, with
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
    click(window, "toast-action", cx);
    assert_eq!(tag_testing::contents(&files), before);
}

#[gpui::test]
fn titles_and_artists_are_read_from_the_tags_off_the_main_thread(cx: &mut TestAppContext) {
    let tagged = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/euphoria-core/tests/tagged-fixtures");
    let untagged =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/euphoria-core/tests/fixtures");
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
                    vec![],
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

    click(window, "toast-action", cx);
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
