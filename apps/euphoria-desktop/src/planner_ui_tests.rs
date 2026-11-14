// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests that plan a transition in the real root view, in a headless window:
//! the clash column of the focus view, the transition tab and the planner,
//! with clicks and keys as a person would, on fixture tracks and stems
//! generated the way the other stem tests make them.

use std::path::{Path, PathBuf};

use euphoria_core::{BeatGrid, CueSection, EuphoriaError, SectionCue, TrackAnalysis};
use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext, px};

use crate::app::EuphoriaApp;
use crate::demo::DemoTrack;
use crate::playback::testing::SilentOutput;
use crate::settings::Settings;
use crate::stems::StemSession;
use crate::tag_writing::testing::ScratchFolder;
use crate::transition_plan::PlanState;
use crate::ui_tests::{click, is_shown, open_window_with, write_stems_with_vocals_for};

const TRACK_SECONDS: f32 = 48.0;

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
                section_cue(32.0, CueSection::Outro),
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
    for index in stems_for {
        let folder = ScratchFolder::new(&format!("{name}-stems-{index}"));
        let files = write_stems_with_vocals_for(&folder, 48, 6);
        cx.update(|cx| {
            view.update(cx, |app, _| {
                app.stems.mark_ready(&paths[*index], files.clone());
            })
        });
        std::mem::forget(folder);
    }
    cx.update(|cx| view.update(cx, |app, cx| app.select(0, cx)));
    cx.run_until_parked();
    Setup {
        window,
        view,
        scratch,
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
fn the_clash_column_fills_for_the_matches_and_says_not_checked_without_stems(
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
        "Not checked",
        "a match without stems is not checked"
    );
    cx.update(|cx| {
        let state = setup.view.read(cx).plan_state(b_index);
        assert!(matches!(state, Some(PlanState::Ready(_))));
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
    for id in ["plan-set-cues", "plan-preview", "plan-open"] {
        assert!(is_shown(setup.window, id, cx), "{id}");
    }
    let text = summary(&setup, cx).unwrap();
    let title = cx.update(|cx| {
        let app = setup.view.read(cx);
        let index = app.focus.selected_match.unwrap();
        app.session
            .library()
            .track(index)
            .unwrap()
            .title()
            .to_string()
    });
    assert!(
        text.starts_with(&format!(
            "Mix out of this track's Main (0:16) into {title}'s Intro. 16 bars, "
        )) || text.starts_with(&format!(
            "Mix out of this track's Outro (0:32) into {title}'s Intro. 16 bars, "
        )),
        "{text}"
    );
    assert!(text.contains(&format!("{title} plays at 120.0 BPM (+1.7%), key ")));
    assert!(text.ends_with("one step up the wheel."), "{text}");
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
fn missing_stems_show_not_checked_and_links_that_start_separation(cx: &mut TestAppContext) {
    let setup = open_plans("plan-missing-stems", &[], cx);
    cx.update(|cx| {
        setup.view.update(cx, |app, _| {
            app.stems = StemSession::new(|| Err(EuphoriaError::NoDataDirectory));
        })
    });
    open_focus_with_plans(&setup, cx);
    let b_index = pick(&setup, &setup.b.clone(), cx);
    assert_eq!(clash_label(&setup, b_index, cx), "Not checked");
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
    drop(setup.scratch);
}

#[gpui::test]
fn a_plan_that_fails_says_why_in_words(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("plan-failure");
    let mut tracks = plan_tracks();
    tracks[1] = plan_track("sine-440hz-stereo.wav", "E minor", 40.0);
    let (window, view) = open_window_with(
        tracks,
        Box::new(SilentOutput),
        Settings::default(),
        None,
        gpui::size(px(1440.0), px(900.0)),
        cx,
    );
    let b = path_of(&view, 1, cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.planner.signals_directory = Some(scratch.0.join("signals"));
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
            Some(PlanState::Failed(_))
        ));
        assert_eq!(app.clash_label(b_index), "No plan");
        view.update(cx, |app, cx| app.open_planner(cx));
    });
    settle(window, cx);
    assert!(is_shown(window, "planner-failure", cx));
    cx.update(|cx| {
        let app = view.read(cx);
        let PlanState::Failed(error) = app
            .plan_state(app.focus.selected_match.unwrap())
            .cloned()
            .unwrap()
        else {
            panic!("the plan should have failed");
        };
        let text = crate::views::planner::failure_message(app, error);
        assert!(text.starts_with("Tempo out of reach: "), "{text}");
    });
}
