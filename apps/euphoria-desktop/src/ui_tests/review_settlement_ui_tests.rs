// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// 9 tests: settling the doubtful values under needs review.

/// reasons once its tags are read, and one that is sure.
fn tracks_with_reasons() -> Vec<DemoTrack> {
    let track = |name: &str, outcome| DemoTrack {
        path: PathBuf::from(format!("/Demo/reasons/{name}.mp3")),
        outcome,
    };
    vec![
        track("unsure key", Ok(rated_analysis(0.001, 124.0, 0.6))),
        track("unsure tempo", Ok(rated_analysis(0.02, 87.0, 0.01))),
        track("tags disagree", Ok(rated_analysis(0.02, 124.0, 0.6))),
        track("everything", Ok(rated_analysis(0.001, 87.0, 0.01))),
        track("broken", Err("cannot decode".to_string())),
        track("sure", Ok(rated_analysis(0.02, 124.0, 0.6))),
    ]
}

fn open_tracks_with_reasons(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let (window, view) = open_folders(
        &["/Demo/reasons"],
        tracks_with_reasons(),
        Settings::default(),
        None,
        cx,
    );
    let tag_key = |text: &str| euphoria_core::TrackName {
        key: euphoria_core::TagKey::from_text(text),
        ..euphoria_core::TrackName::default()
    };
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.session.set_track_names(vec![
                (
                    PathBuf::from("/Demo/reasons/tags disagree.mp3"),
                    tag_key("9A"),
                ),
                (PathBuf::from("/Demo/reasons/everything.mp3"), tag_key("9A")),
            ]);
            app.refresh_visible();
            cx.notify();
        })
    });
    (window, view)
}

fn review_counts(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> (usize, usize) {
    cx.update(|cx| {
        let app = view.read(cx);
        let folder = app
            .folder_tree
            .index_of(Path::new("/Demo/reasons"))
            .and_then(|index| app.folder_tree.node(index))
            .map(|node| node.review_count)
            .unwrap();
        (app.session.library().counts().needs_review, folder)
    })
}

fn select_track(view: &Entity<EuphoriaApp>, index: usize, cx: &mut TestAppContext) {
    cx.update(|cx| view.update(cx, |app, cx| app.select(index, cx)));
}

fn reasons_of(
    view: &Entity<EuphoriaApp>,
    index: usize,
    cx: &mut TestAppContext,
) -> Vec<crate::library::ReviewReason> {
    cx.update(|cx| view.read(cx).session.library().tracks()[index].review_reasons())
}

#[gpui::test]
fn each_row_under_needs_review_carries_a_chip_for_each_of_its_reasons(cx: &mut TestAppContext) {
    let (window, view) = open_tracks_with_reasons(cx);
    assert_eq!(review_counts(&view, cx), (5, 5));
    assert!(!is_shown(window, ("review-chip", 0_usize), cx));

    click(window, "needs-review", cx);
    assert_eq!(
        shown_titles(&view, cx),
        [
            "unsure key.mp3",
            "unsure tempo.mp3",
            "tags disagree.mp3",
            "everything.mp3",
            "broken.mp3"
        ]
    );
    let chip = |index: usize, position: usize| ("review-chip", index * 8 + position);
    assert!(is_shown(window, chip(0, 0), cx));
    assert!(!is_shown(window, chip(0, 1), cx));
    assert!(is_shown(window, chip(1, 0), cx));
    assert!(is_shown(window, chip(2, 0), cx));
    for position in 0..3 {
        assert!(is_shown(window, chip(3, position), cx), "{position}");
    }
    assert!(!is_shown(window, chip(3, 3), cx));
    assert!(is_shown(window, chip(4, 0), cx));
    assert!(!is_shown(window, chip(5, 0), cx));

    let notation = NotationChoice::default();
    let words: Vec<String> = reasons_of(&view, 3, cx)
        .iter()
        .map(|reason| crate::views::review::chip_label(reason, notation))
        .collect();
    assert_eq!(
        words,
        [
            "Key: relative or 5th?",
            "BPM: half or double?",
            "Tag says 9A"
        ]
    );
    assert_eq!(
        crate::views::review::chip_label(&reasons_of(&view, 4, cx)[0], notation),
        "Failed"
    );

    click(window, "all-tracks", cx);
    assert!(!is_shown(window, chip(0, 0), cx));
}

#[gpui::test]
fn the_inspector_box_gives_each_reason_its_line_and_its_quick_actions(cx: &mut TestAppContext) {
    let (window, view) = open_tracks_with_reasons(cx);
    let shown = |ids: &[&'static str], cx: &mut TestAppContext| -> Vec<bool> {
        ids.iter().map(|id| is_shown(window, *id, cx)).collect()
    };
    let lines = [
        "review-key-unsure",
        "review-tempo-unsure",
        "review-key-conflict",
        "review-failed",
    ];

    select_track(&view, 5, cx);
    assert!(!is_shown(window, "review-box", cx));

    select_track(&view, 0, cx);
    assert!(is_shown(window, "review-box", cx));
    assert_eq!(shown(&lines, cx), [true, false, false, false]);
    assert_eq!(
        shown(
            &[
                "review-runner-up",
                "review-key-looks-right",
                "review-correct"
            ],
            cx
        ),
        [true; 3]
    );

    select_track(&view, 1, cx);
    assert_eq!(shown(&lines, cx), [false, true, false, false]);
    assert_eq!(
        shown(
            &[
                "review-tempo-half",
                "review-tempo-double",
                "review-tempo-looks-right"
            ],
            cx
        ),
        [true, false, true],
        "174 can be counted at 87, but 348 is no tempo a DJ counts"
    );

    select_track(&view, 2, cx);
    assert_eq!(shown(&lines, cx), [false, false, true, false]);
    assert_eq!(
        shown(&["review-keep-tag-key", "review-use-detected-key"], cx),
        [true; 2]
    );

    select_track(&view, 3, cx);
    assert_eq!(shown(&lines, cx), [true, true, true, false]);

    select_track(&view, 4, cx);
    assert_eq!(shown(&lines, cx), [false, false, false, true]);
    assert!(is_shown(window, "review-analyze-again", cx));
}

fn tracks_with_tempo_doubts() -> Vec<DemoTrack> {
    let track = |name: &str, outcome| DemoTrack {
        path: PathBuf::from(format!("/Demo/tempo doubts/{name}.mp3")),
        outcome,
    };
    vec![
        track("steady house", Ok(rated_analysis(0.02, 119.8, 0.01))),
        track("half time", Ok(rated_analysis(0.02, 87.0, 0.01))),
        track("double time", Ok(rated_analysis(0.02, 95.0, 0.01))),
        track("dubstep", Ok(rated_analysis(0.02, 140.0, 0.01))),
    ]
}

#[gpui::test]
fn a_doubtful_tempo_is_listed_only_with_a_half_or_double_a_dj_could_count(cx: &mut TestAppContext) {
    let (window, view) = open_folders(
        &["/Demo/tempo doubts"],
        tracks_with_tempo_doubts(),
        Settings::default(),
        None,
        cx,
    );
    let shown = |ids: &[&'static str], cx: &mut TestAppContext| -> Vec<bool> {
        ids.iter().map(|id| is_shown(window, *id, cx)).collect()
    };
    let buttons = ["review-tempo-half", "review-tempo-double"];
    let tempo_unsure = [crate::library::ReviewReason::TempoUnsure];
    cx.update(|cx| assert_eq!(view.read(cx).sidebar_counts().needs_review, 2));

    assert!(
        reasons_of(&view, 0, cx).is_empty(),
        "59.9 and 239.6 are implausible"
    );
    select_track(&view, 0, cx);
    assert!(!is_shown(window, "review-box", cx));
    assert!(!is_shown(window, ("review-chip", 0_usize), cx));

    select_track(&view, 1, cx);
    assert_eq!(selected_key_and_bpm(&view, cx).1, Some(174.0));
    assert_eq!(shown(&buttons, cx), [true, false]);

    select_track(&view, 2, cx);
    assert_eq!(selected_key_and_bpm(&view, cx).1, Some(95.0));
    assert_eq!(shown(&buttons, cx), [false, true]);

    assert!(
        reasons_of(&view, 3, cx).is_empty(),
        "70 is below the default span"
    );
    select_track(&view, 3, cx);
    assert!(!is_shown(window, "review-box", cx));

    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.session
                .set_tempo_range(euphoria_core::TempoRange::new(70.0, 140.0).unwrap());
            app.refresh_visible();
            cx.notify();
        })
    });
    cx.update(|cx| assert_eq!(view.read(cx).sidebar_counts().needs_review, 3));
    assert_eq!(
        reasons_of(&view, 3, cx),
        tempo_unsure,
        "70 or 140 in hip hop"
    );
    select_track(&view, 3, cx);
    assert_eq!(selected_key_and_bpm(&view, cx).1, Some(70.0));
    assert_eq!(shown(&buttons, cx), [false, true]);
    click(window, "review-tempo-double", cx);
    assert_eq!(selected_key_and_bpm(&view, cx).1, Some(140.0));
    assert!(reasons_of(&view, 3, cx).is_empty());
}

#[gpui::test]
fn trying_the_runner_up_key_settles_a_doubtful_key_and_the_counts_follow(cx: &mut TestAppContext) {
    let (window, view) = open_tracks_with_reasons(cx);
    select_track(&view, 0, cx);
    click(window, "review-runner-up", cx);
    assert_eq!(selected_key_and_bpm(&view, cx).0.as_deref(), Some("9A"));
    assert!(reasons_of(&view, 0, cx).is_empty());
    assert_eq!(review_counts(&view, cx), (4, 4));
    assert!(!is_shown(window, "review-box", cx));
}

#[gpui::test]
fn looks_right_keeps_the_detected_values_and_settles_the_doubt(cx: &mut TestAppContext) {
    let (window, view) = open_tracks_with_reasons(cx);
    select_track(&view, 3, cx);
    click(window, "review-key-looks-right", cx);
    assert_eq!(selected_key_and_bpm(&view, cx).0.as_deref(), Some("8A"));
    assert!(!is_shown(window, "review-key-unsure", cx));
    assert_eq!(
        reasons_of(&view, 3, cx),
        [crate::library::ReviewReason::TempoUnsure],
        "confirming the key also settles the conflict with the tags"
    );
    assert_eq!(review_counts(&view, cx), (5, 5));

    click(window, "review-tempo-looks-right", cx);
    assert_eq!(selected_key_and_bpm(&view, cx).1, Some(174.0));
    assert!(!is_shown(window, "review-tempo-unsure", cx));
    cx.update(|cx| {
        let correction = view.read(cx).selected_track().unwrap().correction;
        assert!(correction.key.is_none() && correction.bpm.is_none());
        assert!(correction.key_confirmed && correction.bpm_confirmed);
    });

    assert!(reasons_of(&view, 3, cx).is_empty());
    assert_eq!(review_counts(&view, cx), (4, 4));
    assert!(
        !is_shown(window, "key-set-by-you", cx) && !is_shown(window, "tempo-set-by-you", cx),
        "a value checked with Looks right is still the detected one"
    );
}

#[gpui::test]
fn looks_right_stops_offering_the_runner_up_key_and_the_other_tempo(cx: &mut TestAppContext) {
    let (window, view) = open_tracks_with_reasons(cx);
    select_track(&view, 3, cx);
    assert!(is_shown(window, "runner-up-key", cx));
    assert!(is_shown(window, "tempo-alternative", cx));

    click(window, "review-key-looks-right", cx);
    assert!(
        !is_shown(window, "runner-up-key", cx),
        "a confirmed key offers no runner up"
    );
    cx.update(|cx| {
        let track = view.read(cx).selected_track().unwrap();
        let (_, runner_up) = crate::views::by_ear::heard_keys(track, 0).unwrap();
        assert_eq!(runner_up, None, "Check by ear offers no runner up either");
    });
    assert!(is_shown(window, "tempo-alternative", cx));

    click(window, "review-tempo-looks-right", cx);
    assert!(
        !is_shown(window, "tempo-alternative", cx),
        "a confirmed tempo offers no half or double"
    );
    assert!(reasons_of(&view, 3, cx).is_empty());
    assert!(!is_shown(window, "review-box", cx));
}

#[gpui::test]
fn a_doubtful_value_is_explained_under_needs_review_without_a_confidence_word(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_tracks_with_reasons(cx);
    select_track(&view, 3, cx);
    assert!(is_shown(window, "review-key-unsure", cx));
    assert!(is_shown(window, "review-tempo-unsure", cx));
    assert!(!is_shown(window, "key-confidence", cx));
    assert!(!is_shown(window, "tempo-confidence", cx));
}

#[gpui::test]
fn the_half_button_sets_the_tempo_and_leaves_needs_review(cx: &mut TestAppContext) {
    let (window, view) = open_tracks_with_reasons(cx);
    select_track(&view, 1, cx);
    click(window, "review-tempo-half", cx);
    assert_eq!(selected_key_and_bpm(&view, cx).1, Some(87.0));
    assert_eq!(review_counts(&view, cx), (4, 4));
}

#[gpui::test]
fn a_key_conflict_is_settled_by_keeping_the_tag_or_the_analysis(cx: &mut TestAppContext) {
    let (window, view) = open_tracks_with_reasons(cx);
    select_track(&view, 2, cx);
    click(window, "review-keep-tag-key", cx);
    assert_eq!(selected_key_and_bpm(&view, cx).0.as_deref(), Some("9A"));
    assert!(reasons_of(&view, 2, cx).is_empty());
    assert_eq!(review_counts(&view, cx), (4, 4));

    click(window, "key-reset", cx);
    assert_eq!(review_counts(&view, cx), (5, 5));
    click(window, "review-use-detected-key", cx);
    assert_eq!(selected_key_and_bpm(&view, cx).0.as_deref(), Some("8A"));
    assert!(reasons_of(&view, 2, cx).is_empty());
    assert_eq!(review_counts(&view, cx), (4, 4));
    assert!(!is_shown(window, "review-box", cx));
}

#[gpui::test]
fn analyze_again_takes_a_failed_track_out_of_needs_review_while_it_is_queued(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_tracks_with_reasons(cx);
    select_track(&view, 4, cx);
    click(window, "review-analyze-again", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.session.library().tracks()[4].is_unfinished());
    });
    assert_eq!(review_counts(&view, cx), (4, 4));
}
