// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// 17 tests: the tag editor and correcting keys, tempos and energies.

/// Types `text` into the field `id`, replacing what it holds.
pub(crate) fn type_into(
    window: AnyWindowHandle,
    id: impl Into<gpui::ElementId>,
    text: &str,
    cx: &mut TestAppContext,
) {
    let id = id.into();
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

/// Scratch copies of the test files, each given a title, an artist and a
/// genre with the core's own editing, opened with every track selected and
/// the tag editor open on them.
fn open_editor_on_tagged_files(
    name: &str,
    cx: &mut TestAppContext,
) -> (
    ScratchFolder,
    Vec<PathBuf>,
    Vec<Vec<u8>>,
    AnyWindowHandle,
    Entity<EuphoriaApp>,
) {
    use euphoria_core::tags::{FieldEdit, TagField, TextChange, apply_plan, plan_edit};
    let scratch = ScratchFolder::new(name);
    let files = scratch.copy_fixtures();
    for (number, path) in files.iter().enumerate() {
        let set = |field, text: String| FieldEdit {
            field,
            change: TextChange::Set(text),
        };
        let plan = plan_edit(
            path,
            &[
                set(TagField::Title, format!("Song {number}")),
                set(TagField::Artist, "Ana Sol".to_string()),
                set(TagField::Genre, "deep house".to_string()),
            ],
        )
        .unwrap();
        apply_plan(&plan).unwrap();
    }
    let originals: Vec<Vec<u8>> = files
        .iter()
        .map(|path| std::fs::read(path).unwrap())
        .collect();
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    cx.update(|cx| view.update(cx, |app, cx| app.select_all_shown(cx)));
    click(window, "edit-tags", cx);
    (scratch, files, originals, window, view)
}

fn editor_value(
    view: &Entity<EuphoriaApp>,
    field: euphoria_core::tags::TagField,
    cx: &mut TestAppContext,
) -> Option<crate::tag_editor::FieldValue> {
    cx.update(|cx| match &view.read(cx).tag_panel {
        Some(TagPanel::Editing(editor)) => editor.shown.as_ref().and_then(|shown| {
            shown
                .iter()
                .find(|(candidate, _)| *candidate == field)
                .map(|(_, value)| value.clone())
        }),
        _ => None,
    })
}

#[gpui::test]
fn the_editor_shows_shared_values_and_writes_a_field_to_every_selected_track(
    cx: &mut TestAppContext,
) {
    use crate::tag_editor::FieldValue;
    use euphoria_core::tags::{TagField, read_editable_tags};
    let (_scratch, files, originals, window, view) =
        open_editor_on_tagged_files("ui-tag-editor", cx);

    assert_eq!(
        editor_value(&view, TagField::Genre, cx),
        Some(FieldValue::Same(Some("deep house".to_string())))
    );
    assert_eq!(
        editor_value(&view, TagField::Title, cx),
        Some(FieldValue::Mixed)
    );
    assert_eq!(
        editor_value(&view, TagField::Album, cx),
        Some(FieldValue::Same(None))
    );

    type_into(
        window,
        crate::views::tag_editor::field_input_id(TagField::Genre),
        "Deep House",
        cx,
    );
    click(window, "preview-tag-edits", cx);
    cx.update(|cx| match &view.read(cx).tag_panel {
        Some(TagPanel::Preview(preview)) => {
            assert_eq!(preview.plans.len(), files.len());
            assert!(matches!(
                preview.kind,
                crate::tag_flow::PreviewKind::Edit { .. }
            ));
        }
        other => panic!("{other:?}"),
    });
    assert!(is_shown(window, "edit-summary", cx));
    click(window, "confirm-tag-write", cx);
    cx.update(|cx| {
        assert!(view.read(cx).tag_panel.is_none());
        assert_eq!(
            view.read(cx)
                .toasts
                .current()
                .map(|toast| toast.message.as_str()),
            Some("Saved the tag changes to 3 files")
        );
    });
    for path in &files {
        let tags = read_editable_tags(path).unwrap();
        assert_eq!(tags.get(TagField::Genre), Some("Deep House"));
        assert_eq!(tags.get(TagField::Artist), Some("Ana Sol"), "untouched");
    }

    click(window, "toast-action", cx);
    for (path, original) in files.iter().zip(&originals) {
        assert_eq!(
            &std::fs::read(path).unwrap(),
            original,
            "{} is byte identical",
            path.display()
        );
    }
}

#[gpui::test]
fn the_text_tool_and_remove_change_every_track_and_the_table_follows(cx: &mut TestAppContext) {
    use crate::tag_editor::TextTool;
    use euphoria_core::tags::{TagField, read_editable_tags};
    let (_scratch, files, _, window, view) = open_editor_on_tagged_files("ui-tag-tool", cx);

    let suffix = TextTool::ALL
        .iter()
        .position(|tool| *tool == TextTool::AddSuffix)
        .unwrap();
    click(window, ("text-tool", suffix), cx);
    click(window, ("tool-field", 0_usize), cx);
    type_into(window, "tool-text", " (Edit)", cx);
    click(
        window,
        crate::views::tag_editor::clear_button_id(TagField::Artist),
        cx,
    );
    click(window, "preview-tag-edits", cx);
    click(window, "confirm-tag-write", cx);

    for (number, path) in files.iter().enumerate() {
        let tags = read_editable_tags(path).unwrap();
        assert_eq!(
            tags.get(TagField::Title).map(str::to_string),
            Some(format!("Song {number} (Edit)"))
        );
        assert_eq!(tags.get(TagField::Artist), None);
    }
    let titles = cx.update(|cx| {
        let app = view.read(cx);
        app.session
            .library()
            .tracks()
            .iter()
            .map(|track| track.title().to_string())
            .collect::<Vec<_>>()
    });
    assert!(
        titles.iter().all(|title| title.ends_with(" (Edit)")),
        "{titles:?}"
    );
    click(window, "toast-action", cx);
    let titles = cx.update(|cx| {
        view.read(cx)
            .session
            .library()
            .tracks()
            .iter()
            .map(|track| track.title().to_string())
            .collect::<Vec<_>>()
    });
    assert!(
        titles.iter().all(|title| !title.ends_with(" (Edit)")),
        "{titles:?}"
    );
}

#[gpui::test]
fn playlist_identity_follows_tag_edits_and_undo_in_the_same_window(cx: &mut TestAppContext) {
    use euphoria_core::tags::TagField;
    let (scratch, files, store_path, window, view) =
        open_with_playlist_store("ui-playlist-retag", cx);
    let original = std::fs::read(&files[0]).unwrap();
    let id = cx
        .update_window(window, |_, window, cx| {
            view.update(cx, |app, cx| {
                app.write_log_directory = Some(scratch.log_directory());
                let id = app.create_playlist(window, cx);
                app.renaming_playlist = None;
                app.add_to_playlist(id, &files[..1], cx);
                let index = app.session.library().index_of(&files[0]).unwrap();
                app.select(index, cx);
                id
            })
        })
        .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |app, cx| app.open_tag_editor(window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    type_into(
        window,
        crate::views::tag_editor::field_input_id(TagField::Genre),
        "Deep House",
        cx,
    );
    click(window, "preview-tag-edits", cx);
    click(window, "confirm-tag-write", cx);
    let check = |name: &str, cx: &mut TestAppContext| {
        let stored = euphoria_core::PlaylistStore::open(&store_path)
            .unwrap()
            .load()
            .unwrap();
        assert_eq!(
            stored[0].entries[0].content,
            euphoria_core::ContentHash::of_file(&files[0]).unwrap()
        );
        let job = cx.update(|cx| {
            view.read(cx)
                .playlist_export_job(id, crate::playlist_flow::PlaylistFormat::M3u8)
                .unwrap()
        });
        let destination = scratch.0.join(name);
        let message = job.write(&destination).unwrap().as_text();
        assert!(!message.contains("other audio"), "{message}");
        assert!(
            std::fs::read_to_string(destination)
                .unwrap()
                .contains(files[0].to_str().unwrap())
        );
    };
    check("after-edit.m3u8", cx);
    click(window, "toast-action", cx);
    check("after-undo.m3u8", cx);
    assert_eq!(std::fs::read(&files[0]).unwrap(), original);
}

#[gpui::test]
fn the_editor_asks_for_a_change_and_for_a_selection(cx: &mut TestAppContext) {
    let (_scratch, _files, _, window, view) = open_editor_on_tagged_files("ui-tag-nothing", cx);
    click(window, "preview-tag-edits", cx);
    assert!(is_shown(window, "edit-problem", cx));
    click(window, "cancel-tag-edit", cx);
    cx.update(|cx| assert!(view.read(cx).tag_panel.is_none()));

    let (window, view) = open_tracks_to_review(None, cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.selected = None;
            app.marked.clear();
        })
    });
    click(window, "edit-tags", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.tag_panel.is_none());
        assert!(!app.notice.is_empty());
    });
}

/// The tracks to review, with an output that notes every reference sound.
/// Opens the sections of the track detail as if the user had, so that
/// their contents are drawn whichever track is selected.
pub(crate) fn open_sections(
    view: &Entity<EuphoriaApp>,
    kinds: &[crate::inspector_sections::SectionKind],
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            for kind in kinds {
                app.inspector_sections.choices.set(*kind, true);
            }
            cx.notify();
        })
    });
}

/// Opens or closes a section as if the user had.
fn choose_section(
    view: &Entity<EuphoriaApp>,
    kind: crate::inspector_sections::SectionKind,
    is_open: bool,
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.inspector_sections.choices.set(kind, is_open);
            cx.notify();
        })
    });
}

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
    open_sections(&view, &[crate::inspector_sections::SectionKind::ByEar], cx);
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

pub(crate) fn is_shown(
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
    let message = job.write(&destination).unwrap().as_text();
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
    assert!(!is_shown(window, "key-set-by-you", cx));
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

fn selected_energy_and_detected(
    view: &Entity<EuphoriaApp>,
    cx: &mut TestAppContext,
) -> (Option<u8>, Option<u8>) {
    cx.update(|cx| {
        let track = view.read(cx).selected_track().unwrap();
        (track.energy(), track.detected_energy())
    })
}

#[gpui::test]
fn the_energy_is_picked_marked_by_you_and_reset_to_the_detected_one(cx: &mut TestAppContext) {
    let (window, view, _) = open_tracks_to_check_by_ear(cx);
    click(window, track(1), cx);
    assert_eq!(selected_energy_and_detected(&view, cx), (Some(6), Some(6)));
    assert!(!is_shown(window, ("pick-energy", 1_usize), cx));
    assert!(!is_shown(window, "energy-set-by-you", cx));

    click(window, "edit-values", cx);
    assert!(is_shown(window, ("pick-energy", 10_usize), cx));
    assert!(!is_shown(window, ("pick-energy", 11_usize), cx));
    assert!(!is_shown(window, "reset-energy", cx));

    click(window, ("pick-energy", 9_usize), cx);
    assert_eq!(selected_energy_and_detected(&view, cx), (Some(9), Some(6)));
    assert!(is_shown(window, "energy-set-by-you", cx));
    assert!(is_shown(window, "energy-reset", cx));
    assert!(is_shown(window, "reset-energy", cx));
    cx.update(|cx| {
        let correction = view.read(cx).selected_track().unwrap().correction;
        assert_eq!(correction.energy, Some(9));
        assert_eq!(correction.key, None);
        assert_eq!(correction.bpm, None);
    });
    assert_eq!(selected_key_and_bpm(&view, cx).0.as_deref(), Some("8A"));
    assert_eq!(selected_key_and_bpm(&view, cx).1, Some(124.0));

    click(window, "reset-energy", cx);
    assert_eq!(selected_energy_and_detected(&view, cx), (Some(6), Some(6)));
    assert!(!is_shown(window, "energy-set-by-you", cx));
    assert!(!is_shown(window, "reset-energy", cx));

    click(window, ("pick-energy", 2_usize), cx);
    assert_eq!(selected_energy_and_detected(&view, cx).0, Some(2));
    click(window, "energy-reset", cx);
    assert_eq!(selected_energy_and_detected(&view, cx).0, Some(6));
    assert!(!is_shown(window, "energy-set-by-you", cx));
}

#[gpui::test]
fn a_corrected_energy_is_a_correction_that_is_kept_for_that_track_only(cx: &mut TestAppContext) {
    let (window, view, _) = open_tracks_to_check_by_ear(cx);
    click(window, track(1), cx);
    click(window, "edit-values", cx);
    click(window, ("pick-energy", 3_usize), cx);
    click(window, track(0), cx);
    assert!(!is_shown(window, "energy-set-by-you", cx));
    cx.update(|cx| {
        let library = view.read(cx).session.library();
        assert_eq!(library.track(0).unwrap().energy(), Some(6));
        assert_eq!(library.track(1).unwrap().energy(), Some(3));
        assert!(!library.track(1).unwrap().correction.is_empty());
    });
}

#[gpui::test]
fn selecting_another_track_discards_the_previous_tracks_tempo_draft(cx: &mut TestAppContext) {
    let (window, view, _) = open_tracks_to_check_by_ear(cx);
    click(window, "edit-values", cx);
    retype(window, "bpm-field", "145", cx);
    click(window, track(2), cx);
    assert!(!is_shown(window, "bpm-field", cx));
    cx.update(|cx| view.update(cx, |app, cx| app.accept_typed_bpm(cx)));
    assert_eq!(selected_key_and_bpm(&view, cx).1, Some(174.0));
    click(window, "edit-values", cx);
    let typed = cx.update(|cx| view.read(cx).bpm_input.read(cx).value().to_string());
    assert_eq!(typed, "174.0");
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
            edited_cues: track.edited_cues.clone(),
            saved_loops: track.saved_loops.clone(),
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
