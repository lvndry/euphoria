// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// 8 tests: the needs review list and the key, tempo and display settings.

/// An analysis rated by the core's confidence rules: a key margin of 0.02
/// is high, 0.005 medium and 0.001 low; a tempo margin of 0.6 is high and
/// 0.01 low.
fn rated_analysis(key_margin: f32, tempo_estimate: f32, tempo_margin: f32) -> TrackAnalysis {
    TrackAnalysis {
        duration_seconds: 300.0,
        key: Some(euphoria_core::Key::new(9, euphoria_core::Mode::Minor)),
        bpm: Some(tempo_estimate),
        energy: Some(6),
        tuning_cents: None,
        key_timeline: Vec::new(),
        quality: None,
        key_runner_up: Some(euphoria_core::Key::new(4, euphoria_core::Mode::Minor)),
        key_score_margin: Some(key_margin),
        key_source: None,
        loudness_range_lu: None,
        pitch_class_profile: None,
        tempo_estimate: Some(tempo_estimate),
        tempo_score_margin: Some(tempo_margin),
        tempo_octave_evidence: None,
        beat_grid: None,
        cues: Vec::new(),
    }
}

/// Four made up tracks: one rated high, one with a doubtful key, one with
/// a doubtful tempo counted at half time, and one that failed.
pub(crate) fn tracks_to_review() -> Vec<DemoTrack> {
    let track = |name: &str, outcome| DemoTrack {
        path: PathBuf::from(format!("/Demo/review/{name}.mp3")),
        outcome,
    };
    vec![
        track("sure", Ok(rated_analysis(0.02, 124.0, 0.6))),
        track("doubtful key", Ok(rated_analysis(0.005, 124.0, 0.6))),
        track("doubtful tempo", Ok(rated_analysis(0.02, 87.0, 0.01))),
        track("broken", Err("cannot decode".to_string())),
    ]
}

fn open_tracks_to_review(
    settings_path: Option<PathBuf>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_window_with(
        tracks_to_review(),
        Box::new(SilentOutput),
        Settings::default(),
        settings_path,
        USUAL_WINDOW,
        cx,
    )
}

#[gpui::test]
fn needs_review_lists_doubtful_keys_and_tempos_and_failed_files_and_says_why(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_tracks_to_review(None, cx);
    cx.update(|cx| assert_eq!(view.read(cx).sidebar_counts().needs_review, 2));
    click(window, "needs-review", cx);
    assert_eq!(
        shown_titles(&view, cx),
        ["doubtful tempo.mp3", "broken.mp3"],
        "the doubtful key is rated medium, which is not low"
    );
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("review-explanation").is_some());
    })
    .unwrap();
    click(window, "all-tracks", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("review-explanation").is_none());
    })
    .unwrap();
}

#[gpui::test]
fn a_key_that_differs_from_the_tags_is_marked_listed_for_review_and_explained(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_tracks_to_review(None, cx);
    assert!(!is_shown(window, ("key-conflict", 0_usize), cx));
    assert!(!is_shown(window, "tag-key", cx));
    let tag_key = euphoria_core::TrackName {
        key: euphoria_core::TagKey::from_text("9A"),
        ..euphoria_core::TrackName::default()
    };
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.session
                .set_track_names(vec![(PathBuf::from("/Demo/review/sure.mp3"), tag_key)]);
            app.refresh_visible();
            cx.notify();
        })
    });
    cx.update(|cx| assert_eq!(view.read(cx).sidebar_counts().needs_review, 3));
    assert!(is_shown(window, ("key-conflict", 0_usize), cx));
    assert!(!is_shown(window, ("key-conflict", 1_usize), cx));
    assert!(is_shown(window, "tag-key", cx), "the inspector explains it");
    click(window, "needs-review", cx);
    assert_eq!(
        shown_titles(&view, cx),
        ["sure.mp3", "doubtful tempo.mp3", "broken.mp3"]
    );
}

#[gpui::test]
fn the_key_box_offers_the_runner_up_only_when_the_key_is_not_high(cx: &mut TestAppContext) {
    let (window, view) = open_tracks_to_review(None, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("key-confidence").is_none());
        assert!(window.try_find("tempo-confidence").is_none());
        assert!(
            window.try_find("runner-up-key").is_none(),
            "a key rated high"
        );
        window.click(track(1), cx);
        window.render_frame(cx);
        assert!(
            window.try_find("runner-up-key").is_some(),
            "a key rated medium"
        );
        let key_box = window.find("runner-up-key").bounds();
        assert!(
            window
                .find("inspector")
                .bounds()
                .contains(&key_box.center())
        );
    })
    .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).selected, Some(1)));
}

#[gpui::test]
fn a_tempo_range_preset_moves_the_tempos_shown_and_is_remembered(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("ui-tempo-range");
    let settings_path = scratch.0.join("settings.json");
    let (window, view) = open_tracks_to_review(Some(settings_path.clone()), cx);
    let tempo_of_half_time = |view: &Entity<EuphoriaApp>, cx: &mut TestAppContext| {
        cx.update(|cx| view.read(cx).session.library().tracks()[2].bpm())
    };
    assert_eq!(tempo_of_half_time(&view, cx), Some(174.0));

    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    click(window, ("tempo-preset", 1_usize), cx);
    assert_eq!(tempo_of_half_time(&view, cx), Some(87.0));
    let saved = Settings::load(&settings_path).unwrap().display.tempo_range;
    assert_eq!((saved.slowest, saved.fastest), (70, 140));

    click(window, ("tempo-preset", 0_usize), cx);
    assert_eq!(tempo_of_half_time(&view, cx), Some(174.0));
}

/// Replaces the text of the field `id` with `text`.
pub(crate) fn retype(
    window: AnyWindowHandle,
    id: &'static str,
    text: &str,
    cx: &mut TestAppContext,
) {
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

#[gpui::test]
fn a_custom_tempo_range_is_used_when_valid_and_explained_when_not(cx: &mut TestAppContext) {
    let (window, view) = open_tracks_to_review(None, cx);
    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    click(window, "tempo-custom", cx);
    retype(window, "tempo-from", "80", cx);
    retype(window, "tempo-to", "160", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.session.library().tempo_range().slowest(), 80.0);
        assert_eq!(app.session.library().tempo_range().fastest(), 160.0);
        assert_eq!(app.session.library().tracks()[2].bpm(), Some(87.0));
        assert!(app.tempo_range_problem.is_none());
    });

    retype(window, "tempo-to", "200", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(
            app.tempo_range_problem
                .as_deref()
                .is_some_and(|problem| problem.contains("octave")),
            "{:?}",
            app.tempo_range_problem
        );
        assert_eq!(
            app.session.library().tempo_range().fastest(),
            160.0,
            "the last range that worked stays in use"
        );
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("tempo-range-problem").is_some());
    })
    .unwrap();
}

#[gpui::test]
fn the_key_display_setting_spells_keys_in_the_table_and_is_remembered(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("ui-key-display");
    let settings_path = scratch.0.join("settings.json");
    let (window, view) = open_tracks_to_review(Some(settings_path.clone()), cx);
    cx.update(|cx| {
        let codes = view.read(cx).column_texts(ColumnId::Key);
        assert!(codes.contains("8A"), "{codes:?}");
    });
    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    click(window, ("notation", 2_usize), cx);
    cx.update(|cx| {
        let codes = view.read(cx).column_texts(ColumnId::Key);
        assert!(codes.contains("1m"), "{codes:?}");
    });
    assert_eq!(
        Settings::load(&settings_path).unwrap().tags.key_notation,
        NotationChoice::OpenKey
    );
}

#[gpui::test]
fn custom_starts_from_the_preset_in_use_so_editing_one_field_keeps_the_other(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_tracks_to_review(None, cx);
    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    click(window, ("tempo-preset", 1_usize), cx);
    click(window, "tempo-custom", cx);
    let typed = |view: &Entity<EuphoriaApp>, cx: &mut TestAppContext| {
        cx.update(|cx| {
            let app = view.read(cx);
            (
                app.tempo_from_input.read(cx).value().to_string(),
                app.tempo_to_input.read(cx).value().to_string(),
            )
        })
    };
    assert_eq!(typed(&view, cx), ("70".to_string(), "140".to_string()));

    retype(window, "tempo-from", "75", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        let range = app.session.library().tempo_range();
        assert_eq!((range.slowest(), range.fastest()), (75.0, 140.0));
        assert!(app.tempo_range_problem.is_none());
    });

    click(window, ("tempo-preset", 2_usize), cx);
    assert_eq!(typed(&view, cx), ("120".to_string(), "200".to_string()));
}
