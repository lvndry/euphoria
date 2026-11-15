// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rendering the set of the set builder as one mix, through the real root
//! view in a headless window, on the core crate's short fixture tracks
//! given the analysis of longer ones.

use std::path::Path;
use std::time::Duration;

use euphoria_core::{BeatGrid, CueSection, SectionCue, TrackAnalysis};
use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext, px};

use crate::app::EuphoriaApp;
use crate::demo::DemoTrack;
use crate::playback::testing::SilentOutput;
use crate::settings::Settings;
use crate::tag_writing::testing::ScratchFolder;
use crate::ui_tests::{click, is_shown, open_window_with};

fn track(name: &str, key: &str, bpm: f32) -> DemoTrack {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/euphoria-core/tests/fixtures");
    let section_cue = |seconds, section| SectionCue { seconds, section };
    DemoTrack {
        path: fixtures.join(name),
        outcome: Ok(TrackAnalysis {
            duration_seconds: 48.0,
            key: Some(key.parse().unwrap()),
            bpm: Some(bpm),
            energy: Some(6),
            tuning_cents: None,
            key_runner_up: None,
            key_score_margin: None,
            key_source: None,
            loudness_range_lu: None,
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

fn open_built_set(
    name: &str,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>, ScratchFolder) {
    let scratch = ScratchFolder::new(name);
    let (window, view) = open_window_with(
        vec![
            track("sine-440hz-mono.wav", "A minor", 120.0),
            track("sine-440hz-stereo.wav", "A minor", 122.0),
            track("sine-440hz-mono.flac", "A minor", 121.0),
        ],
        Box::new(SilentOutput),
        Settings::default(),
        None,
        gpui::size(px(1440.0), px(900.0)),
        cx,
    );
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.planner.signals_directory = Some(scratch.0.join("signals"));
        })
    });
    click(window, "set-builder-entry", cx);
    click(window, "build-set", cx);
    cx.update(|cx| {
        let planned = view.read(cx).set_builder.planned.clone();
        assert_eq!(planned.map(|planned| planned.paths.len()), Some(3));
    });
    (window, view, scratch)
}

fn wait_for_render(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) {
    for _ in 0..200 {
        cx.executor().advance_clock(Duration::from_millis(250));
        cx.run_until_parked();
        if cx.update(|cx| view.read(cx).set_builder.render.is_none()) {
            return;
        }
    }
    panic!("the render did not finish");
}

#[gpui::test]
fn the_set_renders_to_one_mix_with_its_cue_sheet_tracklist_and_chapters(cx: &mut TestAppContext) {
    let (window, view, scratch) = open_built_set("mix-render", cx);
    assert!(is_shown(window, "render-mix", cx));
    let output = scratch.0.join("friday.wav");
    cx.update(|cx| view.update(cx, |app, cx| app.begin_mix_render(output.clone(), cx)));
    cx.update(|cx| assert!(view.read(cx).set_builder.render.is_some()));
    wait_for_render(&view, cx);

    let header = std::fs::read(&output).expect("the mix is written");
    assert_eq!(&header[0..4], b"RIFF");
    let cue = std::fs::read_to_string(scratch.0.join("friday.cue")).unwrap();
    assert!(cue.contains("FILE \"friday.wav\" WAVE"), "{cue}");
    assert_eq!(cue.matches("TRACK ").count(), 3, "{cue}");
    let chapters = std::fs::read_to_string(scratch.0.join("friday.chapters.txt")).unwrap();
    assert!(chapters.starts_with("0:00 "), "{chapters}");
    assert!(scratch.0.join("friday.tracklist.txt").exists());
    cx.update(|cx| {
        let app = view.read(cx);
        let toast = app.toasts.current().expect("a toast says where it went");
        assert!(
            toast.message.starts_with("Saved the mix as friday.wav"),
            "{}",
            toast.message
        );
        assert!(matches!(
            &toast.action,
            Some(crate::toast::ToastAction::Reveal(path)) if *path == output
        ));
    });
    assert!(is_shown(window, "render-mix", cx));
}

#[gpui::test]
fn cancel_stops_the_render_and_writes_nothing(cx: &mut TestAppContext) {
    let (window, view, scratch) = open_built_set("mix-render-cancel", cx);
    let output = scratch.0.join("cancelled.wav");
    cx.update(|cx| view.update(cx, |app, cx| app.begin_mix_render(output.clone(), cx)));
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    assert!(is_shown(window, "render-mix-progress", cx));
    click(window, "cancel-render-mix", cx);
    wait_for_render(&view, cx);
    assert!(!output.exists());
    assert!(!scratch.0.join("cancelled.cue").exists());
    cx.update(|cx| {
        let toast = view
            .read(cx)
            .toasts
            .current()
            .map(|toast| toast.message.clone());
        assert_eq!(toast.as_deref(), Some("Rendering the mix was cancelled"));
    });
}

fn wait_for_check(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) {
    for _ in 0..200 {
        cx.run_until_parked();
        if cx.update(|cx| !view.read(cx).set_builder.is_checking) {
            return;
        }
    }
    panic!("the check did not finish");
}

#[gpui::test]
fn a_blend_where_both_sing_is_warned_and_cut_with_the_stems(cx: &mut TestAppContext) {
    let (window, view, scratch) = open_built_set("mix-vocals", cx);
    let paths = cx.update(|cx| view.read(cx).set_builder.planned.clone().unwrap().paths);
    let mut stem_folders = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let folder = ScratchFolder::new(&format!("mix-vocals-stems-{index}"));
        let files = crate::ui_tests::write_stems_with_vocals_for(&folder, 48, 48);
        cx.update(|cx| view.update(cx, |app, _| app.stems.mark_ready(path, files)));
        stem_folders.push(folder);
    }
    click(window, "set-details-toggle", cx);
    click(window, "check-set", cx);
    wait_for_check(&view, cx);
    assert!(is_shown(window, "set-transitions", cx));
    assert!(is_shown(window, ("set-vocal-warning", 0usize), cx));
    let fixes = cx.update(|cx| {
        let app = view.read(cx);
        let overlap = app.checked_transitions().unwrap().transitions[0]
            .vocal_overlap
            .clone()
            .expect("both sing throughout");
        overlap.fixes
    });
    assert_eq!(
        fixes.first(),
        Some(&euphoria_core::set_render::VocalFix::CutOutgoingVocals)
    );

    click(window, ("set-vocal-fix", 0usize), cx);
    wait_for_check(&view, cx);
    cx.update(|cx| {
        let app = view.read(cx);
        let transition = &app.checked_transitions().unwrap().transitions[0];
        assert_eq!(
            transition.vocal_fix,
            Some(euphoria_core::set_render::VocalFix::CutOutgoingVocals)
        );
    });

    let output = scratch.0.join("cut.wav");
    cx.update(|cx| view.update(cx, |app, cx| app.begin_mix_render(output.clone(), cx)));
    wait_for_render(&view, cx);
    assert!(
        output.exists(),
        "{:?}",
        cx.update(|cx| view.read(cx).notice.clone())
    );
    drop(stem_folders);
}
