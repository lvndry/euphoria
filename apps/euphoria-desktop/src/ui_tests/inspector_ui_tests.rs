// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// 12 tests: the inspector sections and the statistic rows.

#[gpui::test]
fn statistic_labels_and_notes_stay_whole_in_every_theme_at_the_narrowest_inspector(
    cx: &mut TestAppContext,
) {
    let mut measured = 0;
    let mut measured_notes = 0;
    for theme in ThemeName::ALL {
        let panels = Panels {
            inspector_width: 0,
            ..Panels::default()
        };
        let (window, view) = open_folders(
            &["/Demo/reasons"],
            tracks_with_reasons(),
            Settings {
                theme,
                panels,
                ..Settings::default()
            },
            None,
            cx,
        );
        select_track(&view, library_index_of(&view, "unsure key.mp3", cx), cx);
        for label in ["Key", "BPM", "Energy"] {
            for prefix in ["statistic-label", "statistic-note"] {
                let id = gpui::SharedString::from(format!("{prefix}-{label}"));
                let height = cx
                    .update_window(window, |_, window, cx| {
                        window.render_frame(cx);
                        window.try_find(id).map(|found| found.bounds().size.height)
                    })
                    .unwrap();
                if let Some(height) = height {
                    if prefix == "statistic-label" {
                        measured += 1;
                    } else {
                        measured_notes += 1;
                    }
                    assert!(
                        height < px(20.0),
                        "{prefix} {label} wraps in {theme:?}: {height:?} high"
                    );
                }
            }
        }
    }
    assert!(
        measured >= 12,
        "the statistics were not shown in every theme"
    );
    assert!(
        measured_notes >= 4,
        "the notes were not shown in every theme"
    );
}

#[gpui::test]
fn the_bpm_and_the_energy_read_on_one_line_in_aligned_boxes(cx: &mut TestAppContext) {
    for theme in ThemeName::ALL {
        let panels = Panels {
            inspector_width: 0,
            ..Panels::default()
        };
        let mut tracks = tracks_with_reasons();
        for demo in &mut tracks {
            if demo.path.ends_with("sure.mp3")
                && let Ok(analysis) = &mut demo.outcome
            {
                analysis.pitch_class_profile =
                    Some([0.9, 0.1, 0.5, 0.2, 0.9, 0.4, 0.1, 0.5, 0.2, 1.0, 0.2, 0.3]);
            }
        }
        let (window, view) = open_folders(
            &["/Demo/reasons"],
            tracks,
            Settings {
                theme,
                panels,
                ..Settings::default()
            },
            None,
            cx,
        );
        for (name, has_alternative) in [("sure.mp3", false), ("unsure tempo.mp3", true)] {
            select_track(&view, library_index_of(&view, name, cx), cx);
            cx.update_window(window, |_, window, cx| {
                window.render_frame(cx);
                let inspector = window.find("inspector").bounds();
                let tempo_unit = window.find("tempo-unit").bounds();
                let energy_unit = window.find("energy-unit").bounds();
                let key_label = window.find("statistic-label-Key").bounds();
                let tempo_label = window.find("statistic-label-BPM").bounds();
                let energy_label = window.find("statistic-label-Energy").bounds();
                for (unit, bounds) in [("BPM", tempo_unit), ("out of 10", energy_unit)] {
                    assert!(
                        bounds.size.height < px(20.0),
                        "{unit} wraps in {theme:?} for {name}: {:?}",
                        bounds.size.height
                    );
                    assert!(
                        bounds.right() <= inspector.right(),
                        "{unit} runs past the inspector in {theme:?} for {name}"
                    );
                }
                assert!(window.try_find("statistic-note-Energy").is_none());
                assert_eq!(
                    window.try_find("statistic-note-BPM").is_some(),
                    has_alternative,
                    "the BPM box has a second line only for the other tempo, {theme:?} {name}"
                );
                if has_alternative {
                    let note = window.find("statistic-note-BPM").bounds();
                    assert!(note.size.height < px(20.0), "{theme:?} {name}");
                }
                let notes_id = if has_alternative {
                    "notes-not-measured"
                } else {
                    "note-strip"
                };
                let notes = window.try_find(notes_id).map(|found| found.bounds());
                assert!(
                    notes.is_some(),
                    "{notes_id} is shown in {theme:?} for {name}"
                );
                if let Some(notes) = notes {
                    if theme == ThemeName::Deck {
                        assert!(
                            notes.left() >= energy_unit.right(),
                            "the notes column overlaps the boxes in {theme:?} for {name}"
                        );
                    } else {
                        assert!(
                            notes.top() >= energy_unit.bottom(),
                            "the notes overlap the boxes in {theme:?} for {name}"
                        );
                        assert!(
                            notes.right() <= inspector.right(),
                            "the notes run past the inspector in {theme:?} for {name}"
                        );
                    }
                }
                if theme == ThemeName::Deck {
                    assert_eq!(key_label.top(), tempo_label.top(), "{theme:?} {name}");
                    assert_eq!(key_label.top(), energy_label.top(), "{theme:?} {name}");
                    assert_eq!(tempo_unit.top(), energy_unit.top(), "{theme:?} {name}");
                    assert!(
                        tempo_unit.right() <= energy_label.left(),
                        "BPM stays in its box in {theme:?} for {name}"
                    );
                } else if !has_alternative {
                    assert_eq!(
                        tempo_unit.top() - tempo_label.top(),
                        energy_unit.top() - energy_label.top(),
                        "the BPM and Energy lines keep one shape in {theme:?} for {name}"
                    );
                }
            })
            .unwrap();
        }
    }
}

#[gpui::test]
fn each_statistic_row_centres_its_label_and_its_value_on_one_line(cx: &mut TestAppContext) {
    let mut rows_measured = 0;
    for theme in ThemeName::ALL {
        if theme == ThemeName::Deck {
            continue;
        }
        let panels = Panels {
            inspector_width: 0,
            ..Panels::default()
        };
        let (window, view) = open_folders(
            &["/Demo/reasons"],
            tracks_with_reasons(),
            Settings {
                theme,
                panels,
                ..Settings::default()
            },
            None,
            cx,
        );
        let mut assert_rows_centred = |situation: &str, cx: &mut TestAppContext| {
            cx.update_window(window, |_, window, cx| {
                window.render_frame(cx);
                let inspector = window.find("inspector").bounds();
                let mut paddings = Vec::new();
                for label in ["Key", "BPM", "Energy"] {
                    let row = window
                        .find(gpui::SharedString::from(format!("statistic-row-{label}")))
                        .bounds();
                    let value = window
                        .find(gpui::SharedString::from(format!("statistic-value-{label}")))
                        .bounds();
                    let label_bounds = window
                        .find(gpui::SharedString::from(format!("statistic-label-{label}")))
                        .bounds();
                    let centre = row.center().y;
                    assert!(
                        (value.center().y - centre).abs() <= px(1.0),
                        "the {label} value sits off the row's centre in {theme:?}, {situation}: {value:?} in {row:?}"
                    );
                    assert!(
                        (label_bounds.center().y - centre).abs() <= px(1.0),
                        "the {label} label sits off the row's centre in {theme:?}, {situation}: {label_bounds:?} in {row:?}"
                    );
                    let line_above = if label == "Key" { px(0.0) } else { px(1.0) };
                    paddings.push((
                        value.top() - row.top() - line_above,
                        row.bottom() - value.bottom(),
                    ));
                    rows_measured += 1;
                }
                for (top, bottom) in &paddings {
                    assert_eq!(top, bottom, "{theme:?}, {situation}");
                    assert_eq!(*top, paddings[0].0, "one padding for every row in {theme:?}, {situation}");
                }
                let code = window.find("key-code").bounds();
                let name = window.find("key-name").bounds();
                assert!(
                    name.size.height < px(20.0) && name.left() >= code.right(),
                    "the key reads on one line in {theme:?}, {situation}"
                );
                assert!(
                    name.right() <= inspector.right(),
                    "the key name runs past the inspector in {theme:?}, {situation}"
                );
            })
            .unwrap();
        };

        for name in ["sure.mp3", "unsure tempo.mp3", "unsure key.mp3"] {
            select_track(&view, library_index_of(&view, name, cx), cx);
            assert_rows_centred(name, cx);
        }
        assert!(is_shown(window, "runner-up-key", cx));
        click(window, "runner-up-key", cx);
        assert!(is_shown(window, "key-set-by-you", cx));
        assert!(is_shown(window, "key-reset", cx));
        assert_rows_centred("a key set by the user", cx);
    }
    assert!(
        rows_measured >= 12,
        "the rows were not measured in every theme"
    );
}

#[gpui::test]
fn the_key_reads_on_one_line_in_the_chosen_notation(cx: &mut TestAppContext) {
    for theme in ThemeName::ALL {
        let panels = Panels {
            inspector_width: 0,
            ..Panels::default()
        };
        let (window, view) = open_folders(
            &["/Demo/reasons"],
            tracks_with_reasons(),
            Settings {
                theme,
                panels,
                ..Settings::default()
            },
            None,
            cx,
        );
        select_track(&view, library_index_of(&view, "sure.mp3", cx), cx);
        let key = cx.update(|cx| view.read(cx).selected_track().unwrap().key().unwrap());
        for notation in NotationChoice::ALL {
            cx.update(|cx| view.update(cx, |app, cx| app.set_key_notation(notation, cx)));
            let (expected_first, expected_second) = match notation {
                NotationChoice::Musical => (key.to_string(), format!("/ {}", key.camelot())),
                _ => (notation.spell(&key), format!("/ {key}")),
            };
            cx.update_window(window, |_, window, cx| {
                window.render_frame(cx);
                let inspector = window.find("inspector").bounds();
                let code = window.find("key-code").bounds();
                let name = window.find("key-name").bounds();
                let (first, second) = match notation {
                    NotationChoice::Musical => (name, code),
                    _ => (code, name),
                };
                assert!(
                    second.left() >= first.right(),
                    "{expected_first} {expected_second} reads on one line in {theme:?}"
                );
                for (part, bounds) in [("code", code), ("name", name)] {
                    assert!(
                        bounds.size.height < px(32.0) && bounds.right() <= inspector.right(),
                        "the key {part} wraps or runs past the inspector in {theme:?} with {notation:?}"
                    );
                }
                assert!(
                    code.size.width > px(0.0),
                    "the key code is never cut away in {theme:?} with {notation:?}"
                );
            })
            .unwrap();
        }
    }
}

#[gpui::test]
fn the_deck_strip_gives_the_title_a_column_of_at_least_220_pixels(cx: &mut TestAppContext) {
    for window_size in [USUAL_WINDOW, SMALLEST_WINDOW] {
        let (window, _) = open_arranged(ThemeName::Deck, Panels::default(), window_size, cx);
        let (width, height) = cx
            .update_window(window, |_, window, cx| {
                window.render_frame(cx);
                let found = window
                    .try_find("inspector-title")
                    .expect("the title is shown");
                (found.bounds().size.width, found.bounds().size.height)
            })
            .unwrap();
        assert!(
            width >= px(220.0),
            "the title is {width:?} wide at {window_size:?}"
        );
        assert!(height < px(24.0), "the title wraps: {height:?} high");
    }
}

fn library_index_of(view: &Entity<EuphoriaApp>, file_name: &str, cx: &mut TestAppContext) -> usize {
    cx.update(|cx| {
        let app = view.read(cx);
        app.session
            .library()
            .tracks()
            .iter()
            .position(|track| track.file_name == file_name)
            .unwrap_or_else(|| panic!("{file_name} is not in the library"))
    })
}

fn section_body_id(kind: crate::inspector_sections::SectionKind) -> (&'static str, usize) {
    ("section-body", kind.index())
}

#[gpui::test]
fn the_sections_open_as_they_do_by_default(cx: &mut TestAppContext) {
    use crate::inspector_sections::SectionKind;
    let (window, view) = open_tracks_with_reasons(cx);
    select_track(&view, library_index_of(&view, "sure.mp3", cx), cx);
    for kind in [
        SectionKind::Mixes,
        SectionKind::ByEar,
        SectionKind::KeyShift,
        SectionKind::Stems,
    ] {
        assert!(
            is_shown(window, crate::views::inspector_section::toggle_id(kind), cx),
            "{kind:?} has no heading"
        );
    }
    assert!(!is_shown(window, section_body_id(SectionKind::ByEar), cx));
    assert!(!is_shown(window, section_body_id(SectionKind::Stems), cx));
    assert!(!is_shown(
        window,
        section_body_id(SectionKind::Location),
        cx
    ));
    assert!(is_shown(window, section_body_id(SectionKind::Mixes), cx));
}

#[gpui::test]
fn toggling_a_section_opens_it_and_is_written_to_the_settings(cx: &mut TestAppContext) {
    use crate::inspector_sections::SectionKind;
    let folder = ScratchFolder::new("section-toggle");
    let settings_path = folder.0.join("settings.json");
    let (window, view) = open_folders(
        &["/Demo/reasons"],
        tracks_with_reasons(),
        Settings::default(),
        Some(settings_path.clone()),
        cx,
    );
    select_track(&view, library_index_of(&view, "sure.mp3", cx), cx);
    click(
        window,
        crate::views::inspector_section::toggle_id(SectionKind::Mixes),
        cx,
    );
    click(
        window,
        crate::views::inspector_section::toggle_id(SectionKind::Stems),
        cx,
    );
    assert!(is_shown(window, section_body_id(SectionKind::Stems), cx));
    assert!(!is_shown(window, section_body_id(SectionKind::Mixes), cx));

    let saved = Settings::load(&settings_path).unwrap().inspector_sections;
    assert_eq!(saved.stems, Some(true));
    assert_eq!(saved.mixes, Some(false));
    assert_eq!(saved.by_ear, None);

    let (reopened, reopened_view) = open_folders(
        &["/Demo/reasons"],
        tracks_with_reasons(),
        Settings::load(&settings_path).unwrap(),
        None,
        cx,
    );
    select_track(
        &reopened_view,
        library_index_of(&reopened_view, "sure.mp3", cx),
        cx,
    );
    assert!(is_shown(reopened, section_body_id(SectionKind::Stems), cx));
    assert!(!is_shown(reopened, section_body_id(SectionKind::Mixes), cx));
}

#[gpui::test]
fn check_by_ear_opens_for_a_track_whose_key_is_in_doubt_and_not_for_one_that_is_sure(
    cx: &mut TestAppContext,
) {
    use crate::inspector_sections::SectionKind;
    let (window, view) = open_tracks_with_reasons(cx);
    let by_ear = section_body_id(SectionKind::ByEar);
    select_track(&view, 0, cx);
    assert!(is_shown(window, by_ear, cx), "key unsure");
    select_track(&view, 1, cx);
    assert!(!is_shown(window, by_ear, cx), "tempo unsure only");
    select_track(&view, 2, cx);
    assert!(is_shown(window, by_ear, cx), "tags conflict");
    select_track(&view, 5, cx);
    assert!(!is_shown(window, by_ear, cx), "sure");
}

#[gpui::test]
fn a_section_the_user_closed_stays_closed_for_a_track_that_would_open_it(cx: &mut TestAppContext) {
    use crate::inspector_sections::SectionKind;
    let (window, view) = open_tracks_with_reasons(cx);
    let by_ear = section_body_id(SectionKind::ByEar);
    select_track(&view, library_index_of(&view, "unsure key.mp3", cx), cx);
    assert!(is_shown(window, by_ear, cx), "opened for a doubtful key");

    choose_section(&view, SectionKind::ByEar, false, cx);
    assert!(!is_shown(window, by_ear, cx));
    select_track(&view, library_index_of(&view, "everything.mp3", cx), cx);
    assert!(!is_shown(window, by_ear, cx), "the user's choice wins");

    choose_section(&view, SectionKind::ByEar, true, cx);
    select_track(&view, library_index_of(&view, "unsure tempo.mp3", cx), cx);
    assert!(is_shown(window, by_ear, cx), "kept open for every track");
}

fn track_with_quality(name: &str, verdict: euphoria_core::quality::QualityVerdict) -> DemoTrack {
    DemoTrack {
        path: PathBuf::from(format!("/Demo/quality/{name}.mp3")),
        outcome: Ok(TrackAnalysis {
            quality: Some(euphoria_core::quality::QualityReport {
                codec: "mp3".to_string(),
                lossless: false,
                bitrate_kbps: Some(128),
                sample_rate: 44_100,
                bit_depth: None,
                channels: Some(2),
                cutoff_hz: Some(16_000.0),
                clipped_share: 0.0,
                integrated_loudness_lufs: None,
                true_peak_dbtp: None,
                damage: Default::default(),
                cancelled_share: 0.0,
                verdict,
            }),
            ..rated_analysis(0.02, 124.0, 0.6)
        }),
    }
}

#[gpui::test]
fn sound_quality_opens_for_weak_and_re_encoded_files_only(cx: &mut TestAppContext) {
    use crate::inspector_sections::SectionKind;
    use euphoria_core::quality::QualityVerdict;
    let (window, view) = open_folders(
        &["/Demo/quality"],
        vec![
            track_with_quality("weak", QualityVerdict::WeakOnABigSystem),
            track_with_quality("re-encoded", QualityVerdict::LikelyReencoded),
            track_with_quality("good", QualityVerdict::Good),
            track_with_quality("lossless", QualityVerdict::Lossless),
        ],
        Settings::default(),
        None,
        cx,
    );
    let quality = section_body_id(SectionKind::Quality);
    for (name, opens) in [
        ("weak.mp3", true),
        ("re-encoded.mp3", true),
        ("good.mp3", false),
        ("lossless.mp3", false),
    ] {
        select_track(&view, library_index_of(&view, name, cx), cx);
        assert_eq!(is_shown(window, quality, cx), opens, "{name}");
    }
}

#[gpui::test]
fn the_deck_strip_shows_one_section_at_a_time_from_its_tabs(cx: &mut TestAppContext) {
    use crate::inspector_sections::SectionKind;
    let (window, view) = open_folders(
        &["/Demo/reasons"],
        tracks_with_reasons(),
        Settings {
            theme: ThemeName::Deck,
            ..Settings::default()
        },
        None,
        cx,
    );
    select_track(&view, library_index_of(&view, "sure.mp3", cx), cx);
    for kind in SectionKind::ALL {
        assert!(is_shown(
            window,
            crate::views::inspector_section::tab_id(kind),
            cx
        ));
        assert!(!is_shown(
            window,
            crate::views::inspector_section::toggle_id(kind),
            cx
        ));
    }
    let shown = |cx: &mut TestAppContext| cx.update(|cx| view.read(cx).strip_section_shown());
    assert_eq!(shown(cx), SectionKind::Mixes);
    click(
        window,
        crate::views::inspector_section::tab_id(SectionKind::Stems),
        cx,
    );
    assert_eq!(shown(cx), SectionKind::Stems);
    select_track(&view, library_index_of(&view, "unsure key.mp3", cx), cx);
    assert_eq!(shown(cx), SectionKind::Stems, "a tab the user picked stays");
}

#[gpui::test]
fn the_track_player_hides_while_the_decks_are_open(cx: &mut TestAppContext) {
    let output = RenderedDecks::default();
    let (window, view) = open_window_on(deck_tracks(), Box::new(output), cx);
    assert!(is_shown(window, "player-bar", cx));
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.toggle_decks(cx);
            app.toggle_playback(cx);
        })
    });
    cx.run_until_parked();
    assert!(!is_shown(window, "player-bar", cx));
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.notice.is_empty(), "no notice asks to close the decks");
        assert!(!app.player.offers_pause(), "the player stays silent");
    });
    cx.update(|cx| view.update(cx, |app, cx| app.toggle_decks(cx)));
    cx.run_until_parked();
    assert!(is_shown(window, "player-bar", cx));
}
