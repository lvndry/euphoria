// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests that plan a transition in the real root view, in a headless window:
//! the clash column of the focus view, the transition tab and the planner,
//! with clicks and keys as a person would, on fixture tracks and stems
//! generated the way the other stem tests make them.

use std::path::{Path, PathBuf};

use euphoria_core::harmony::transition::{PlanError, Side};
use euphoria_core::{BeatGrid, CueSection, EuphoriaError, SectionCue, TrackAnalysis};
use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext, px};

use crate::app::EuphoriaApp;
use crate::demo::DemoTrack;
use crate::playback::testing::SilentOutput;
use crate::settings::Settings;
use crate::stems::StemSession;
use crate::tag_writing::testing::ScratchFolder;
use crate::track_overview::VocalLane;
use crate::transition_plan::PlanState;
use crate::ui_tests::{click, is_shown, open_window_with, write_stems_with_vocals_for};

const TRACK_SECONDS: f32 = 64.0;

fn plan_track(name: &str, key: &str, bpm: f32) -> DemoTrack {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/euphoria-core/tests/fixtures");
    let section_cue = |seconds, section| SectionCue { seconds, section };
    DemoTrack {
        path: fixtures.join(name),
        outcome: Ok(TrackAnalysis {
            duration_seconds: TRACK_SECONDS,
            key: Some(key.parse().unwrap()),
            bpm: Some(bpm),
            energy: None,
            tuning_cents: None,
            key_runner_up: None,
            key_score_margin: None,
            key_source: None,
            tempo_estimate: Some(bpm),
            tempo_score_margin: None,
            beat_grid: Some(BeatGrid {
                first_downbeat_seconds: 0.0,
                bpm: f64::from(bpm),
                salience: 10.0,
                drift_seconds: Some(0.001),
            }),
            cues: vec![
                section_cue(0.0, CueSection::Start),
                section_cue(16.0, CueSection::Main),
                section_cue(32.0, CueSection::Breakdown),
                section_cue(48.0, CueSection::Outro),
            ],
            key_timeline: Vec::new(),
            quality: None,
        }),
    }
}

fn plan_tracks() -> Vec<DemoTrack> {
    vec![
        plan_track("sine-440hz-mono.wav", "A minor", 120.0),
        plan_track("sine-440hz-stereo.wav", "E minor", 118.0),
        plan_track("sine-440hz-mono.mp3", "A minor", 119.0),
    ]
}

struct Setup {
    window: AnyWindowHandle,
    view: Entity<EuphoriaApp>,
    scratch: ScratchFolder,
    stem_folders: Vec<ScratchFolder>,
    a: PathBuf,
    b: PathBuf,
    c: PathBuf,
}

fn path_of(view: &Entity<EuphoriaApp>, index: usize, cx: &mut TestAppContext) -> PathBuf {
    cx.update(|cx| {
        view.read(cx)
            .session
            .library()
            .track(index)
            .unwrap()
            .path
            .clone()
    })
}

fn index_of(view: &Entity<EuphoriaApp>, path: &Path, cx: &mut TestAppContext) -> usize {
    cx.update(|cx| view.read(cx).session.library().index_of(path).unwrap())
}

fn open_plans(name: &str, stems_for: &[usize], cx: &mut TestAppContext) -> Setup {
    let scratch = ScratchFolder::new(name);
    let (window, view) = open_window_with(
        plan_tracks(),
        Box::new(SilentOutput),
        Settings::default(),
        None,
        gpui::size(px(1440.0), px(900.0)),
        cx,
    );
    let paths: Vec<PathBuf> = (0..3).map(|index| path_of(&view, index, cx)).collect();
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.planner.signals_directory = Some(scratch.0.join("signals"));
        })
    });
    let mut stem_folders = Vec::new();
    for index in stems_for {
        let folder = ScratchFolder::new(&format!("{name}-stems-{index}"));
        let files = write_stems_with_vocals_for(&folder, 64, 6);
        cx.update(|cx| {
            view.update(cx, |app, _| {
                app.stems.mark_ready(&paths[*index], files.clone());
            })
        });
        stem_folders.push(folder);
    }
    cx.update(|cx| view.update(cx, |app, cx| app.select(0, cx)));
    cx.run_until_parked();
    Setup {
        window,
        view,
        scratch,
        stem_folders,
        a: paths[0].clone(),
        b: paths[1].clone(),
        c: paths[2].clone(),
    }
}

fn settle(window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn press(window: AnyWindowHandle, key: &str, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press(key, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn open_focus(setup: &Setup, cx: &mut TestAppContext) {
    click(setup.window, "focus-open", cx);
    cx.run_until_parked();
    settle(setup.window, cx);
}

fn open_focus_with_plans(setup: &Setup, cx: &mut TestAppContext) {
    open_focus(setup, cx);
    for _ in 0..10 {
        cx.run_until_parked();
    }
    settle(setup.window, cx);
}

fn pick(setup: &Setup, path: &Path, cx: &mut TestAppContext) -> usize {
    let index = index_of(&setup.view, path, cx);
    click(setup.window, ("focus-mix", index), cx);
    cx.run_until_parked();
    index
}

fn clash_label(setup: &Setup, index: usize, cx: &mut TestAppContext) -> &'static str {
    cx.update(|cx| setup.view.read(cx).clash_label(index))
}

fn summary(setup: &Setup, cx: &mut TestAppContext) -> Option<String> {
    cx.update(|cx| setup.view.read(cx).window_summary_text(0))
}

fn cue_names(setup: &Setup, path: &Path, cx: &mut TestAppContext) -> (Vec<(String, u32)>, bool) {
    cx.update(|cx| {
        let cues = setup
            .view
            .read(cx)
            .session
            .library()
            .track_by_path(path)
            .unwrap()
            .cues();
        let listed = cues
            .cues
            .iter()
            .map(|cue| (cue.name.clone(), (cue.seconds * 1000.0).round() as u32))
            .collect();
        (listed, cues.is_edited())
    })
}

#[gpui::test]
fn the_clash_column_fills_for_the_matches_and_says_stems_are_needed_without_stems(
    cx: &mut TestAppContext,
) {
    let setup = open_plans("plan-clash-column", &[0, 1], cx);
    open_focus_with_plans(&setup, cx);
    let b_index = index_of(&setup.view, &setup.b, cx);
    let c_index = index_of(&setup.view, &setup.c, cx);

    assert!(is_shown(setup.window, ("focus-mix-clash", b_index), cx));
    assert!(is_shown(
        setup.window,
        ("focus-mix-clash-verdict", b_index),
        cx
    ));
    assert_eq!(clash_label(&setup, b_index, cx), "Clean");
    assert_eq!(
        clash_label(&setup, c_index, cx),
        "Stems needed",
        "a match without stems is not checked"
    );
    cx.update(|cx| {
        let state = setup.view.read(cx).plan_state(b_index);
        assert!(matches!(state, Some(PlanState::Ready(_))));
    });
}

#[gpui::test]
fn removing_stems_recomputes_the_plan_without_cached_stem_levels(cx: &mut TestAppContext) {
    let setup = open_plans("plan-removed-stems", &[0, 1], cx);
    open_focus_with_plans(&setup, cx);
    let b_index = index_of(&setup.view, &setup.b, cx);
    assert_eq!(clash_label(&setup, b_index, cx), "Clean");

    cx.update(|cx| setup.view.update(cx, |app, cx| app.remove_stems(cx)));
    for _ in 0..10 {
        cx.run_until_parked();
    }
    assert_eq!(clash_label(&setup, b_index, cx), "Stems needed");
    cx.update(|cx| {
        let app = setup.view.read(cx);
        assert!(app.stems.ready_files(&setup.a).is_none());
        assert!(app.decks.stem_library.files(&setup.a).is_none());
    });
}

#[gpui::test]
fn a_failed_stem_removal_keeps_the_vocal_lane_and_stem_state(cx: &mut TestAppContext) {
    let setup = open_plans("plan-failed-stem-removal", &[], cx);
    let file = setup.scratch.0.join("not-a-directory");
    std::fs::write(&file, b"stem directory cannot be a file").unwrap();
    let files = euphoria_core::stems::StemFiles::in_directory(file);
    cx.update(|cx| {
        setup.view.update(cx, |app, cx| {
            app.stems.mark_ready(&setup.a, files.clone());
            app.show_vocal_lane(setup.a.clone(), VocalLane::Checking);
            app.remove_stems(cx);
        })
    });
    cx.update(|cx| {
        let app = setup.view.read(cx);
        assert_eq!(app.stems.ready_files(&setup.a), Some(files));
        assert_eq!(app.vocal_lane_of(&setup.a), &VocalLane::Checking);
    });
}

#[gpui::test]
fn the_transition_tab_gives_the_best_window_in_a_sentence(cx: &mut TestAppContext) {
    let setup = open_plans("plan-summary", &[0, 1], cx);
    open_focus_with_plans(&setup, cx);
    pick(&setup, &setup.b.clone(), cx);
    settle(setup.window, cx);
    assert!(is_shown(setup.window, "focus-transition-summary", cx));
    assert!(is_shown(setup.window, "plan-strip", cx));
    assert!(is_shown(setup.window, "plan-clash", cx));
    for id in [
        "plan-set-cues",
        "plan-preview",
        "plan-save-favorite",
        "plan-open",
    ] {
        assert!(is_shown(setup.window, id, cx), "{id}");
    }
    let text = summary(&setup, cx).unwrap();
    assert_eq!(
        text,
        "Mix out of this track's Main (0:16) into sine-440hz-stereo.wav's Intro. 16 bars, \
         vocals on B only, no bass overlap. sine-440hz-stereo.wav plays at 120.0 BPM (+1.7%), \
         key 9A, one step up the wheel."
    );
}

#[gpui::test]
fn setting_the_cues_writes_them_to_both_tracks_and_one_undo_restores_both(cx: &mut TestAppContext) {
    let setup = open_plans("plan-set-cues", &[0, 1], cx);
    open_focus_with_plans(&setup, cx);
    pick(&setup, &setup.b.clone(), cx);
    let (a_before, a_edited_before) = cue_names(&setup, &setup.a.clone(), cx);
    let (b_before, b_edited_before) = cue_names(&setup, &setup.b.clone(), cx);
    assert!(!a_edited_before && !b_edited_before);

    click(setup.window, "plan-set-cues", cx);
    let (a_after, a_edited) = cue_names(&setup, &setup.a.clone(), cx);
    let (b_after, b_edited) = cue_names(&setup, &setup.b.clone(), cx);
    assert!(a_edited && b_edited, "the plan's cues are the user's own");
    let names = |cues: &[(String, u32)]| cues.iter().map(|cue| cue.0.clone()).collect::<Vec<_>>();
    assert!(names(&a_after).contains(&"Mix out".to_string()));
    assert!(names(&a_after).contains(&"Mix out end".to_string()));
    assert!(names(&b_after).contains(&"Mix in".to_string()));
    assert!(
        names(&a_after).contains(&"Start".to_string()),
        "the sections stay"
    );
    assert!(a_after.len() > a_before.len() && b_after.len() > b_before.len());

    click(setup.window, "plan-set-cues", cx);
    let (a_again, _) = cue_names(&setup, &setup.a.clone(), cx);
    assert_eq!(
        a_again, a_after,
        "setting them again does not pile up copies"
    );

    cx.update(|cx| view_update_undo(&setup, cx));
    cx.run_until_parked();
    cx.update(|cx| view_update_undo(&setup, cx));
    cx.run_until_parked();
    let (a_back, a_edited_back) = cue_names(&setup, &setup.a.clone(), cx);
    let (b_back, b_edited_back) = cue_names(&setup, &setup.b.clone(), cx);
    assert_eq!((a_back, a_edited_back), (a_before, false));
    assert_eq!((b_back, b_edited_back), (b_before, false));
}

fn view_update_undo(setup: &Setup, cx: &mut gpui::App) {
    setup.view.update(cx, |app, cx| app.undo_cue_edit(cx));
}

#[gpui::test]
fn the_planner_opens_from_the_tab_and_escape_goes_back_to_the_focus_view(cx: &mut TestAppContext) {
    let setup = open_plans("plan-open-close", &[0, 1], cx);
    open_focus_with_plans(&setup, cx);
    pick(&setup, &setup.b.clone(), cx);
    assert!(!is_shown(setup.window, "planner-view", cx));

    click(setup.window, "plan-open", cx);
    settle(setup.window, cx);
    assert!(is_shown(setup.window, "planner-view", cx));
    assert!(!is_shown(setup.window, "focus-view", cx));
    for id in [
        "planner-ruler",
        "planner-track-a",
        "planner-track-b",
        "planner-clash",
        "planner-legend",
        "planner-windows",
        "planner-steps",
    ] {
        assert!(is_shown(setup.window, id, cx), "{id}");
    }

    press(setup.window, "escape", cx);
    settle(setup.window, cx);
    assert!(!is_shown(setup.window, "planner-view", cx));
    assert!(is_shown(setup.window, "focus-view", cx));
    cx.update(|cx| assert!(setup.view.read(cx).focus.is_open));

    click(setup.window, "plan-open", cx);
    click(setup.window, "planner-back", cx);
    settle(setup.window, cx);
    assert!(is_shown(setup.window, "focus-view", cx));
}

#[gpui::test]
fn clicking_a_window_moves_the_outline_and_changes_the_steps(cx: &mut TestAppContext) {
    let setup = open_plans("plan-windows", &[0, 1], cx);
    open_focus_with_plans(&setup, cx);
    pick(&setup, &setup.b.clone(), cx);
    click(setup.window, "plan-open", cx);
    settle(setup.window, cx);

    let steps_of = |cx: &mut TestAppContext| -> Vec<String> {
        cx.update(|cx| {
            let app = setup.view.read(cx);
            let (planned, window) = app.planner_window().unwrap();
            window
                .steps
                .iter()
                .map(|step| crate::transition_plan::step_line(step, planned, window))
                .collect()
        })
    };
    let listed = cx.update(|cx| {
        setup
            .view
            .read(cx)
            .picked_plan()
            .unwrap()
            .plan
            .windows
            .len()
    });
    assert!((3..=5).contains(&listed), "{listed} windows");
    for index in 0..listed {
        assert!(is_shown(setup.window, ("planner-window", index), cx));
    }
    let first = steps_of(cx);
    assert!(is_shown(
        setup.window,
        ("planner-step", first.len() - 1),
        cx
    ));

    click(setup.window, ("planner-window", 1_usize), cx);
    settle(setup.window, cx);
    cx.update(|cx| assert_eq!(setup.view.read(cx).planner.window, 1));
    let second = steps_of(cx);
    assert_ne!(first, second, "the step list follows the chosen window");
}

#[gpui::test]
fn saving_the_favorite_transition_keeps_the_summary_as_its_note(cx: &mut TestAppContext) {
    let setup = open_plans("plan-favorite", &[0, 1], cx);
    open_focus_with_plans(&setup, cx);
    pick(&setup, &setup.b.clone(), cx);
    click(setup.window, "plan-open", cx);
    settle(setup.window, cx);
    cx.update(|cx| {
        assert!(
            setup
                .view
                .read(cx)
                .session
                .favorites()
                .transition(&setup.a, &setup.b)
                .is_none()
        );
    });

    click(setup.window, "planner-save-favorite", cx);
    let expected = summary(&setup, cx).unwrap();
    cx.update(|cx| {
        let favorites = setup.view.read(cx).session.favorites();
        let transition = favorites.transition(&setup.a, &setup.b).unwrap();
        assert_eq!(transition.note.as_deref(), Some(expected.as_str()));
    });
}

#[gpui::test]
fn missing_stems_say_stems_are_needed_and_links_that_start_separation(cx: &mut TestAppContext) {
    let setup = open_plans("plan-missing-stems", &[], cx);
    cx.update(|cx| {
        setup.view.update(cx, |app, _| {
            app.stems = StemSession::new(|| Err(EuphoriaError::NoDataDirectory));
        })
    });
    open_focus_with_plans(&setup, cx);
    let b_index = pick(&setup, &setup.b.clone(), cx);
    assert_eq!(clash_label(&setup, b_index, cx), "Stems needed");
    cx.update(|cx| {
        let text = setup.view.read(cx).window_summary_text(0).unwrap();
        assert!(
            text.contains("no stems on either track, so vocals and bass are not checked"),
            "{text}"
        );
    });

    click(setup.window, "plan-open", cx);
    settle(setup.window, cx);
    assert!(is_shown(setup.window, "planner-stems-line", cx));
    assert!(is_shown(setup.window, "planner-separate-a", cx));
    assert!(is_shown(setup.window, "planner-separate-b", cx));

    click(setup.window, "planner-separate-a", cx);
    cx.update(|cx| {
        assert!(setup.view.read(cx).stems.status(&setup.a).is_some());
    });
    drop(setup.stem_folders);
    drop(setup.scratch);
}

#[gpui::test]
fn a_plan_that_fails_says_why_in_words(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("plan-failure");
    let mut tracks = plan_tracks();
    if let Ok(analysis) = &mut tracks[1].outcome {
        analysis.beat_grid = None;
    }
    let (window, view) = open_window_with(
        tracks,
        Box::new(SilentOutput),
        Settings::default(),
        None,
        gpui::size(px(1440.0), px(900.0)),
        cx,
    );
    let a = path_of(&view, 0, cx);
    let b = path_of(&view, 1, cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.planner.signals_directory = Some(scratch.0.join("signals"));
            app.select(0, cx);
        })
    });
    click(window, "focus-open", cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let b_index = app.session.library().index_of(&b).unwrap();
            app.pick_match(b_index, cx);
        })
    });
    for _ in 0..10 {
        cx.run_until_parked();
    }
    cx.update(|cx| {
        let app = view.read(cx);
        let b_index = app.session.library().index_of(&b).unwrap();
        assert!(matches!(
            app.plan_state(b_index),
            Some(PlanState::Failed(PlanError::NoBeatGrid(Side::B)))
        ));
        assert_eq!(app.clash_label(b_index), "");
        view.update(cx, |app, cx| app.open_planner(cx));
    });
    settle(window, cx);
    assert!(is_shown(window, "planner-failure", cx));
    let failure_text = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            let app = view.read(cx);
            let Some(PlanState::Failed(error)) =
                app.plan_state(app.focus.selected_match.unwrap()).cloned()
            else {
                panic!("the plan should have failed");
            };
            crate::views::planner::failure_message(app, error)
        })
    };
    let text = failure_text(cx);
    assert!(text.starts_with("No beat grid for "), "{text}");

    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.planner.force_state(
                &a,
                &b,
                PlanState::Failed(PlanError::TempoOutOfReach { rate: 2.4 }),
            );
        })
    });
    settle(window, cx);
    assert!(is_shown(window, "planner-failure", cx));
    let text = failure_text(cx);
    assert!(
        text.starts_with("Tempo out of reach: ") && text.contains("2.40 times its speed"),
        "{text}"
    );
}

#[gpui::test]
fn previewing_loads_both_tracks_on_the_decks_with_sync_on(cx: &mut TestAppContext) {
    let setup = open_plans("plan-preview", &[0, 1], cx);
    open_focus_with_plans(&setup, cx);
    let b_index = pick(&setup, &setup.b.clone(), cx);
    click(setup.window, "plan-preview", cx);
    cx.update(|cx| {
        let app = setup.view.read(cx);
        assert!(app.decks.is_open);
        assert!(app.decks.sync);
        assert_eq!(
            app.decks.view(crate::deck_audio::DeckId::A).track_index,
            Some(0)
        );
        assert_eq!(
            app.decks.view(crate::deck_audio::DeckId::B).track_index,
            Some(b_index)
        );
        assert!(!app.focus.is_open, "the decks are in the library layout");
    });
}

#[gpui::test]
fn the_overlap_link_on_the_plan_card_starts_separating_the_first_track_without_stems(
    cx: &mut TestAppContext,
) {
    let setup = open_plans("plan-overlap-link", &[], cx);
    cx.update(|cx| {
        setup.view.update(cx, |app, _| {
            app.stems = StemSession::new(|| Err(EuphoriaError::NoDataDirectory));
        })
    });
    open_focus_with_plans(&setup, cx);
    pick(&setup, &setup.b.clone(), cx);
    settle(setup.window, cx);
    assert!(is_shown(setup.window, "focus-transition-overlap", cx));
    cx.update(|cx| {
        let app = setup.view.read(cx);
        assert!(app.stems.status(&setup.a).is_none());
        assert!(app.stems.status(&setup.b).is_none());
    });
    click(setup.window, "focus-transition-verdict", cx);
    cx.update(|cx| {
        let app = setup.view.read(cx);
        assert!(app.stems.status(&setup.a).is_some(), "A starts separating");
        assert!(
            app.stems.status(&setup.b).is_none(),
            "B waits, one separation runs at a time"
        );
    });
    drop(setup.stem_folders);
    drop(setup.scratch);
}

#[gpui::test]
fn the_plan_card_opens_the_planner_from_a_link_in_its_header(cx: &mut TestAppContext) {
    let setup = open_plans("plan-open-link", &[0, 1], cx);
    open_focus_with_plans(&setup, cx);
    pick(&setup, &setup.b.clone(), cx);
    settle(setup.window, cx);
    cx.update_window(setup.window, |_, window, cx| {
        window.render_frame(cx);
        let link = window.find("plan-open").bounds();
        let card = window.find("focus-plan-body").bounds();
        assert!(link.bottom() <= card.top(), "the link sits in the header");
        assert!(link.right() <= window.viewport_size().width);
    })
    .unwrap();
    click(setup.window, "plan-open", cx);
    cx.update(|cx| assert!(setup.view.read(cx).planner.is_open));
}

#[test]
fn the_surfaces_use_the_dj_words_and_the_old_words_are_gone() {
    let surfaces = [
        ("views/planner.rs", include_str!("views/planner.rs")),
        ("views/focus.rs", include_str!("views/focus.rs")),
        ("views/decks.rs", include_str!("views/decks.rs")),
        ("views/set_builder.rs", include_str!("views/set_builder.rs")),
        ("transition_plan.rs", include_str!("transition_plan.rs")),
        ("decks.rs", include_str!("decks.rs")),
    ];
    let old_words = [
        "\"Stretch\"",
        "\"Workable\"",
        "\"Smooth\"",
        "\"Verdict\"",
        "\"Adjacent\"",
        "\"Diagonal\"",
        "\"Pool\"",
        "\"Lanes\"",
        "\"Clash\"",
        "\"Not checked\"",
        "\"Vocal clash\"",
        "\"Bass clash\"",
        "\"Set the cues on A and B\"",
        "\"Set cues on both tracks\"",
        "\"Play it on the decks\"",
        "\"Preview on the decks\"",
        "\"Open the planner\"",
        "\"Save as a favorite transition\"",
        "dropped frames; ",
    ];
    for (name, source) in surfaces {
        let production = source.split("#[cfg(test)]").next().unwrap();
        for word in old_words {
            assert!(!production.contains(word), "{name} still says {word}");
        }
    }
    let words = include_str!("../../../crates/euphoria-core/src/harmony/words.rs");
    for word in [
        "One step on the wheel",
        "Wheel diagonal",
        "One step and a major/minor switch",
        "Vocals and bass overlap",
        "Separate stems to check",
        "Pick from",
        "Layers",
        "Pitch move",
        "Big jump",
    ] {
        assert!(words.contains(word), "{word} is in the shared words");
    }
    let plan = include_str!("transition_plan.rs");
    for word in [
        "Set cues on A and B",
        "Open on the decks",
        "Save as favorite transition",
        "Open planner \\u{2192}",
    ] {
        assert!(plan.contains(word), "{word} is in the shared plan copy");
    }
}

#[gpui::test]
fn escape_goes_up_one_level_from_the_planner_to_the_focus_view_to_the_library(
    cx: &mut TestAppContext,
) {
    let setup = open_plans("plan-escape-chain", &[0, 1], cx);
    open_focus_with_plans(&setup, cx);
    pick(&setup, &setup.b.clone(), cx);
    click(setup.window, "plan-open", cx);
    settle(setup.window, cx);
    assert!(is_shown(setup.window, "planner-view", cx));

    press(setup.window, "escape", cx);
    settle(setup.window, cx);
    assert!(is_shown(setup.window, "focus-view", cx));
    assert!(!is_shown(setup.window, "planner-view", cx));
    cx.update(|cx| assert!(setup.view.read(cx).focus.is_open));

    press(setup.window, "escape", cx);
    settle(setup.window, cx);
    assert!(!is_shown(setup.window, "focus-view", cx));
    assert!(
        is_shown(setup.window, "all-tracks", cx),
        "the library is back"
    );
    cx.update(|cx| assert!(!setup.view.read(cx).focus.is_open));
}

fn open_planner_view(setup: &Setup, cx: &mut TestAppContext) -> usize {
    open_focus_with_plans(setup, cx);
    let b_index = pick(setup, &setup.b.clone(), cx);
    click(setup.window, "plan-open", cx);
    settle(setup.window, cx);
    assert!(is_shown(setup.window, "planner-view", cx));
    b_index
}

fn timing_of(setup: &Setup, cx: &mut TestAppContext) -> crate::planner_playback::PlanTiming {
    cx.update(|cx| {
        let app = setup.view.read(cx);
        let (planned, window) = app.planner_window().unwrap();
        crate::planner_playback::PlanTiming::of(planned, window).unwrap()
    })
}

fn tick(setup: &Setup, a_seconds: f64, b_seconds: f64, cx: &mut TestAppContext) -> bool {
    cx.update(|cx| {
        setup.view.update(cx, |app, cx| {
            let generation = app.planner.playback.as_ref().unwrap().generation();
            app.step_plan_playback(generation, a_seconds, b_seconds, cx)
        })
    })
}

fn sent(setup: &Setup, cx: &mut TestAppContext) -> Vec<crate::decks::SentCommand> {
    cx.update(|cx| setup.view.read(cx).decks.sent.borrow().clone())
}

fn opened(setup: &Setup, cx: &mut TestAppContext) -> Vec<(crate::deck_audio::DeckId, f64)> {
    cx.update(|cx| setup.view.read(cx).decks.opened.borrow().clone())
}

#[gpui::test]
fn the_transport_plays_a_then_both_on_the_planned_bars_with_b_stretched(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    use crate::decks::SentCommand;
    let setup = open_plans("plan-transport", &[0, 1], cx);
    open_planner_view(&setup, cx);
    let timing = timing_of(&setup, cx);
    click(setup.window, "planner-play-transition", cx);

    let opens = opened(&setup, cx);
    let a_open = opens
        .iter()
        .rev()
        .find(|(deck, _)| *deck == DeckId::A)
        .unwrap();
    let b_open = opens
        .iter()
        .rev()
        .find(|(deck, _)| *deck == DeckId::B)
        .unwrap();
    assert!(
        (a_open.1 - timing.a_seconds_at(-8.0)).abs() < 1e-9,
        "A opens 8 bars before the mix"
    );
    assert!(
        (b_open.1 - timing.b_seconds_at(0.0)).abs() < 1e-9,
        "B waits at the mix start"
    );
    cx.update(|cx| {
        let app = setup.view.read(cx);
        assert!(!app.decks.sync);
        assert_eq!(app.decks.view(DeckId::B).rate, timing.b_rate);
        assert_eq!(app.decks.view(DeckId::A).rate, 1.0);
        assert!(app.decks.view(DeckId::B).keylock);
    });
    assert!(sent(&setup, cx).contains(&SentCommand::SetRate {
        deck: DeckId::B,
        rate: timing.b_rate
    }));

    let before = sent(&setup, cx).len();
    assert!(tick(&setup, 0.0, 0.0, cx));
    let after: Vec<_> = sent(&setup, cx)[before..].to_vec();
    assert_eq!(
        after,
        vec![SentCommand::Play {
            deck: DeckId::A,
            quantized: false
        }],
        "A plays alone first"
    );
    cx.update(|cx| assert_eq!(setup.view.read(cx).decks.gains_set.get(), [1.0, 0.0]));

    let before = sent(&setup, cx).len();
    assert!(tick(&setup, timing.a_seconds_at(-3.0), 0.0, cx));
    assert_eq!(sent(&setup, cx).len(), before, "B is not due yet");
    assert!(tick(&setup, timing.a_seconds_at(-0.3), 0.0, cx));
    let after: Vec<_> = sent(&setup, cx)[before..].to_vec();
    assert_eq!(
        after,
        vec![SentCommand::Play {
            deck: DeckId::B,
            quantized: true
        }],
        "B comes in on A's downbeat"
    );

    let half = timing.window_bars as f64 / 2.0;
    assert!(tick(
        &setup,
        timing.a_seconds_at(half),
        timing.b_seconds_at(half),
        cx
    ));
    cx.update(|cx| {
        let [a, b] = setup.view.read(cx).decks.gains_set.get();
        assert!((a - b).abs() < 1e-5 && (a * a + b * b - 1.0).abs() < 1e-4);
    });

    click(setup.window, "planner-stop", cx);
    cx.update(|cx| {
        let app = setup.view.read(cx);
        assert!(app.planner.playback.is_none());
        assert!(!app.decks.view(DeckId::A).playing && !app.decks.view(DeckId::B).playing);
    });
}

#[gpui::test]
fn play_a_and_play_b_play_one_track_at_the_planned_place(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    use crate::decks::SentCommand;
    let setup = open_plans("plan-play-one", &[0, 1], cx);
    open_planner_view(&setup, cx);
    let timing = timing_of(&setup, cx);

    click(setup.window, "planner-play-a", cx);
    let a_open = *opened(&setup, cx)
        .iter()
        .rev()
        .find(|(deck, _)| *deck == DeckId::A)
        .unwrap();
    assert!((a_open.1 - timing.a_seconds_at(-8.0)).abs() < 1e-9);
    let before = sent(&setup, cx).len();
    assert!(tick(&setup, 0.0, 0.0, cx));
    assert_eq!(
        sent(&setup, cx)[before..].to_vec(),
        vec![SentCommand::Play {
            deck: DeckId::A,
            quantized: false
        }]
    );

    click(setup.window, "planner-play-b", cx);
    let b_open = *opened(&setup, cx)
        .iter()
        .rev()
        .find(|(deck, _)| *deck == DeckId::B)
        .unwrap();
    assert!((b_open.1 - timing.b_seconds_at(0.0)).abs() < 1e-9);
    cx.update(|cx| {
        assert_eq!(
            setup.view.read(cx).decks.view(DeckId::B).rate,
            timing.b_rate
        );
    });
    let before = sent(&setup, cx).len();
    assert!(tick(&setup, 0.0, 0.0, cx));
    let after = sent(&setup, cx)[before..].to_vec();
    assert!(after.contains(&SentCommand::Play {
        deck: DeckId::B,
        quantized: false
    }));
    assert!(!after.contains(&SentCommand::Play {
        deck: DeckId::A,
        quantized: false
    }));
    cx.update(|cx| assert_eq!(setup.view.read(cx).decks.gains_set.get(), [0.0, 1.0]));
}

#[gpui::test]
fn clicking_the_timeline_seeks_and_space_plays_and_stops(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    let setup = open_plans("plan-seek", &[0, 1], cx);
    open_planner_view(&setup, cx);
    let timing = timing_of(&setup, cx);
    assert!(is_shown(setup.window, "planner-playhead", cx));

    click(setup.window, "planner-timeline", cx);
    let expected = timing.bar_at_fraction(0.5).round();
    cx.update(|cx| {
        let app = setup.view.read(cx);
        assert_eq!(
            app.planner.start_bar,
            Some(expected),
            "a click while stopped moves the playhead"
        );
        assert!(app.planner.playback.is_none());
    });

    press(setup.window, "space", cx);
    cx.update(|cx| assert!(setup.view.read(cx).plan_is_playing(), "space plays"));
    let a_open = *opened(&setup, cx)
        .iter()
        .rev()
        .find(|(deck, _)| *deck == DeckId::A)
        .unwrap();
    assert!(
        (a_open.1 - timing.a_seconds_at(expected)).abs() < 1e-9,
        "from the clicked bar"
    );

    cx.update(|cx| {
        setup
            .view
            .update(cx, |app, cx| app.seek_plan_timeline(0.25, cx))
    });
    let bar = timing.bar_at_fraction(0.25).round();
    let a_open = *opened(&setup, cx)
        .iter()
        .rev()
        .find(|(deck, _)| *deck == DeckId::A)
        .unwrap();
    assert!(
        (a_open.1 - timing.a_seconds_at(bar)).abs() < 1e-9,
        "a click while playing restarts there"
    );

    press(setup.window, "space", cx);
    cx.update(|cx| assert!(!setup.view.read(cx).plan_is_playing(), "space stops"));
}

#[gpui::test]
fn the_axis_says_where_the_mix_starts_and_where_a_ends(cx: &mut TestAppContext) {
    use crate::views::planner;
    let setup = open_plans("plan-axis", &[0, 1], cx);
    open_planner_view(&setup, cx);
    for id in [
        "planner-ruler",
        "planner-a-clock",
        "planner-overlap-help",
        "planner-legend",
    ] {
        assert!(is_shown(setup.window, id, cx), "{id}");
    }
    cx.update(|cx| {
        let app = setup.view.read(cx);
        let (planned, window) = app.planner_window().unwrap();
        let label = planner::a_clock_label(app, planned, window);
        assert!(label.starts_with("A at "), "{label}");
        assert!(!label.contains("A 0"), "{label}");
    });
    assert_eq!(planner::MIX_STARTS, "Mix starts");
    assert_eq!(planner::BARS_HINT, "Bars counted from where B comes in");
    assert_eq!(
        planner::OVERLAP_HELP,
        "Where both tracks have vocals or basslines at once. Pick a window with none."
    );
    assert_eq!(planner::BEST_PLACE_TO_MIX, "Best place to mix");
    let source = include_str!("views/planner.rs");
    let production = source.split("#[cfg(test)]").next().unwrap();
    assert!(!production.contains("\"Suggested window\""));
    assert!(!production.contains("\"bar 1\" =>"));
}

#[gpui::test]
fn lanes_without_stems_say_not_separated_yet_and_known_ones_do_not(cx: &mut TestAppContext) {
    let setup = open_plans("plan-unknown-lanes", &[0], cx);
    open_planner_view(&setup, cx);
    assert_eq!(crate::views::planner::NOT_SEPARATED, "Not separated yet");
    for position in 0..3_usize {
        assert!(is_shown(
            setup.window,
            ("planner-unknown-lane-b", position),
            cx
        ));
        assert!(!is_shown(
            setup.window,
            ("planner-unknown-lane-a", position),
            cx
        ));
    }
}

struct StubStems;

static STUB_STEMS: std::sync::Mutex<Option<euphoria_core::stems::StemFiles>> =
    std::sync::Mutex::new(None);

impl crate::stems::StemSeparation for StubStems {
    fn stems(
        &mut self,
        _: &Path,
        _: &dyn euphoria_core::stems::SeparationObserver,
    ) -> Result<euphoria_core::stems::StemFiles, EuphoriaError> {
        std::thread::sleep(std::time::Duration::from_millis(30));
        Ok(STUB_STEMS.lock().unwrap().clone().unwrap())
    }
}

#[gpui::test]
fn the_separate_button_separates_b_in_place_and_the_lanes_fill_when_it_finishes(
    cx: &mut TestAppContext,
) {
    let setup = open_plans("plan-separate-button", &[0], cx);
    let folder = ScratchFolder::new("plan-separate-button-stub");
    *STUB_STEMS.lock().unwrap() = Some(write_stems_with_vocals_for(&folder, 64, 6));
    let folder_a = ScratchFolder::new("plan-separate-button-a");
    let files_a = write_stems_with_vocals_for(&folder_a, 64, 6);
    cx.update(|cx| {
        setup.view.update(cx, |app, _| {
            app.stems = StemSession::new(|| Ok(Box::new(StubStems)));
            app.stems.mark_ready(&setup.a, files_a.clone());
        })
    });
    open_planner_view(&setup, cx);
    assert!(is_shown(setup.window, "planner-stems-line", cx));
    assert!(
        !is_shown(setup.window, "planner-separate-a", cx),
        "A has stems"
    );
    assert!(is_shown(setup.window, "planner-separate-b", cx));
    assert_eq!(
        crate::views::planner::STEMS_NOT_CHECKED,
        "Vocals and bass overlap isn't checked yet."
    );
    cx.update(|cx| {
        assert!(
            !setup
                .view
                .read(cx)
                .planner_window()
                .unwrap()
                .0
                .stems_known()
                .1
        )
    });

    click(setup.window, "planner-separate-b", cx);
    settle(setup.window, cx);
    assert!(
        is_shown(setup.window, ("planner-progress", 1_usize), cx),
        "progress shows in place"
    );
    assert!(is_shown(setup.window, "planner-cancel-b", cx));
    assert!(!is_shown(setup.window, "planner-separate-b", cx));

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(300));
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let known = cx.update(|cx| {
            setup
                .view
                .read(cx)
                .planner_window()
                .is_some_and(|(planned, _)| planned.stems_known() == (true, true))
        });
        if known {
            break;
        }
    }
    settle(setup.window, cx);
    cx.update(|cx| {
        let app = setup.view.read(cx);
        let (planned, _) = app.planner_window().expect("the plan is back");
        assert_eq!(planned.stems_known(), (true, true));
        assert!(app.planner.is_open, "no navigation was needed");
    });
    assert!(
        !is_shown(setup.window, "planner-stems-line", cx),
        "the line goes when both are known"
    );
    assert!(!is_shown(
        setup.window,
        ("planner-unknown-lane-b", 0_usize),
        cx
    ));
}

#[gpui::test]
fn the_stems_notice_is_said_once_and_the_rows_say_only_what_differs(cx: &mut TestAppContext) {
    let setup = open_plans("plan-single-notice", &[], cx);
    open_planner_view(&setup, cx);
    assert!(is_shown(setup.window, "planner-stems-line", cx));
    assert!(is_shown(setup.window, "planner-separate-a", cx));
    assert!(is_shown(setup.window, "planner-separate-b", cx));
    let rows = cx.update(|cx| {
        setup
            .view
            .read(cx)
            .planner_window()
            .unwrap()
            .0
            .plan
            .windows
            .len()
    });
    assert!(rows >= 2);
    for index in 0..rows {
        assert!(is_shown(setup.window, ("planner-window", index), cx));
        assert!(
            !is_shown(setup.window, ("planner-window-verdict", index), cx),
            "no Stems needed on row {index}"
        );
    }
    cx.update(|cx| {
        let app = setup.view.read(cx);
        let (planned, _) = app.planner_window().unwrap();
        for window in &planned.plan.windows {
            let detail = crate::transition_plan::window_detail(window, false);
            assert!(!detail.to_lowercase().contains("stems"), "{detail}");
            assert!(!detail.contains("not checked"), "{detail}");
        }
    });
}

#[gpui::test]
fn the_step_lines_read_naturally(cx: &mut TestAppContext) {
    let setup = open_plans("plan-steps-words", &[0, 1], cx);
    open_planner_view(&setup, cx);
    let steps: Vec<String> = cx.update(|cx| {
        let app = setup.view.read(cx);
        let (planned, window) = app.planner_window().unwrap();
        window
            .steps
            .iter()
            .map(|step| crate::transition_plan::step_line(step, planned, window))
            .collect()
    });
    assert!(
        steps[0].starts_with("At ") && steps[0].contains(", start B from "),
        "{steps:?}"
    );
    assert!(steps[0].ends_with("on the downbeat."), "{steps:?}");
    let last = steps.last().unwrap();
    assert!(
        last.starts_with("A ends at ") && last.ends_with(" bars later."),
        "{steps:?}"
    );
    assert!(
        steps.iter().all(|step| !step.starts_with("Bar ")),
        "{steps:?}"
    );
}

#[gpui::test]
fn back_to_plan_returns_to_the_same_window_and_escape_does_too(cx: &mut TestAppContext) {
    use crate::deck_audio::DeckId;
    let setup = open_plans("plan-back", &[0, 1], cx);
    let b_index = open_planner_view(&setup, cx);
    click(setup.window, ("planner-window", 1_usize), cx);
    cx.update(|cx| {
        setup.view.update(cx, |app, _| {
            app.planner
                .scroll
                .set_offset(gpui::point(px(0.0), px(-40.0)))
        })
    });
    let scroll_before = cx.update(|cx| setup.view.read(cx).planner.scroll.offset());

    click(setup.window, "planner-play", cx);
    settle(setup.window, cx);
    cx.update(|cx| {
        let app = setup.view.read(cx);
        assert!(app.decks.is_open && !app.focus.is_open && app.planner.return_to.is_some());
    });
    assert!(is_shown(setup.window, "decks-back-to-plan", cx));

    click(setup.window, "decks-back-to-plan", cx);
    settle(setup.window, cx);
    for _ in 0..10 {
        cx.run_until_parked();
    }
    settle(setup.window, cx);
    cx.update(|cx| {
        let app = setup.view.read(cx);
        assert!(app.focus.is_open && app.planner.is_open);
        assert_eq!(app.selected, Some(0));
        assert_eq!(app.focus.selected_match, Some(b_index));
        assert_eq!(app.planner.window, 1);
        assert!(app.planner.return_to.is_none());
        assert_eq!(app.planner.scroll.offset(), scroll_before);
        assert!(!app.decks.view(DeckId::A).playing && !app.decks.view(DeckId::B).playing);
    });
    assert!(is_shown(setup.window, "planner-view", cx));

    click(setup.window, "planner-play", cx);
    settle(setup.window, cx);
    press(setup.window, "escape", cx);
    for _ in 0..10 {
        cx.run_until_parked();
    }
    settle(setup.window, cx);
    cx.update(|cx| {
        let app = setup.view.read(cx);
        assert!(
            app.planner.is_open && app.focus.is_open,
            "Esc goes back to the plan"
        );
        assert_eq!(app.planner.window, 1);
    });
}
