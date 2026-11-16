// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The playback tempo control of the focus view, used through the real root
//! view in a headless window, with the one second files the core is tested
//! with standing in for tracks of 120 beats per minute.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use euphoria_core::{BeatGrid, Cache, CachedAnalyzer, TrackAnalysis};
use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Entity, Modifiers, TestAppContext, WindowOptions};

use crate::app::EuphoriaApp;
use crate::demo::DemoTrack;
use crate::playback::testing::SilentOutput;
use crate::playback::{AudioOutput, TrackSource};
use crate::player::{PlayerRequest, PlayerStatus};
use crate::settings::Settings;
use crate::state::Session;
use crate::ui_tests::{click, is_shown, open_window_on, retype, track};

const ORIGINAL_BPM: f32 = 120.0;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/euphoria-core/tests/fixtures")
}

fn tracks_at_120() -> Vec<DemoTrack> {
    ["sine-440hz-mono.flac", "sine-440hz-mono.mp3"]
        .into_iter()
        .map(|name| DemoTrack {
            path: fixtures().join(name),
            outcome: Ok(TrackAnalysis {
                duration_seconds: 1.0,
                key: None,
                bpm: Some(ORIGINAL_BPM),
                energy: None,
                tuning_cents: None,
                key_runner_up: None,
                key_score_margin: None,
                key_source: None,
                loudness_range_lu: None,
                pitch_class_profile: None,
                tempo_estimate: None,
                tempo_score_margin: None,
                tempo_octave_evidence: None,
                beat_grid: Some(BeatGrid {
                    first_downbeat_seconds: 0.25,
                    bpm: f64::from(ORIGINAL_BPM),
                    salience: 5.0,
                    drift_seconds: None,
                }),
                cues: Vec::new(),
                key_timeline: Vec::new(),
                quality: None,
            }),
        })
        .collect()
}

fn open_focus_on_track(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let (window, view) = open_window_on(tracks_at_120(), Box::new(SilentOutput), cx);
    click(window, track(0), cx);
    click(window, "focus-open", cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
    (window, view)
}

fn start_playing(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) {
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let Some(PlayerRequest::Start { attempt, path, .. }) = app.player.toggle() else {
                panic!("expected the track to start");
            };
            let source = TrackSource::open(&path, 0.0);
            app.finish_starting(attempt, source, cx);
            assert_eq!(app.player.status(), &PlayerStatus::Playing);
        });
    });
}

fn type_target(window: AnyWindowHandle, text: &str, cx: &mut TestAppContext) {
    click(window, "focus-tempo-bpm", cx);
    retype(window, "focus-tempo-field", text, cx);
    cx.update_window(window, |_, window, cx| window.press("enter", cx))
        .unwrap();
    cx.run_until_parked();
}

fn select_track(view: &Entity<EuphoriaApp>, index: usize, cx: &mut TestAppContext) {
    cx.update(|cx| view.update(cx, |app, cx| app.select(index, cx)));
    cx.run_until_parked();
}

fn rate(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> f64 {
    cx.update(|cx| view.read(cx).tempo_rate())
}

fn target(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Option<f32> {
    cx.update(|cx| view.read(cx).tempo_target_bpm())
}

#[gpui::test]
fn the_header_shows_the_control_with_key_lock_on_and_the_tempo_as_recorded(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_focus_on_track(cx);
    for id in [
        "focus-tempo-bpm",
        "focus-tempo-percent",
        "focus-tempo-reset",
        "focus-key-lock",
        "focus-tempo-up",
        "focus-tempo-down",
    ] {
        assert!(is_shown(window, id, cx), "{id}");
    }
    assert_eq!(rate(&view, cx), 1.0);
    assert_eq!(target(&view, cx), Some(ORIGINAL_BPM));
    cx.update(|cx| assert!(view.read(cx).tempo.key_lock));
    assert!(!is_shown(window, "focus-bpm-original", cx));
}

#[gpui::test]
fn a_typed_tempo_with_a_comma_sets_the_target_and_the_chip_shows_the_original(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_focus_on_track(cx);
    type_target(window, "124,5", cx);
    assert_eq!(target(&view, cx), Some(124.5));
    assert!((rate(&view, cx) - 124.5 / 120.0).abs() < 1e-9);
    assert!(is_shown(window, "focus-bpm-target", cx));
    assert!(is_shown(window, "focus-bpm-original", cx));
}

#[gpui::test]
fn the_tempo_is_clamped_to_sixteen_percent_either_way(cx: &mut TestAppContext) {
    let (window, view) = open_focus_on_track(cx);
    type_target(window, "200", cx);
    assert_eq!(target(&view, cx), Some(139.2));
    assert!((rate(&view, cx) - 1.16).abs() < 1e-9);
    type_target(window, "40", cx);
    assert_eq!(target(&view, cx), Some(100.8));
    assert!((rate(&view, cx) - 0.84).abs() < 1e-9);
    click(window, "focus-tempo-down", cx);
    assert_eq!(
        target(&view, cx),
        Some(100.8),
        "the arrow stops at the limit"
    );
}

#[gpui::test]
fn the_arrows_step_a_beat_and_alt_a_tenth(cx: &mut TestAppContext) {
    let (window, view) = open_focus_on_track(cx);
    click(window, "focus-tempo-up", cx);
    assert_eq!(target(&view, cx), Some(121.0));
    click(window, "focus-tempo-down", cx);
    click(window, "focus-tempo-down", cx);
    assert_eq!(target(&view, cx), Some(119.0));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click_with_modifiers("focus-tempo-up", Modifiers::alt(), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(target(&view, cx), Some(119.1));
}

#[gpui::test]
fn reset_returns_to_the_tracks_own_tempo(cx: &mut TestAppContext) {
    let (window, view) = open_focus_on_track(cx);
    type_target(window, "130", cx);
    assert_ne!(rate(&view, cx), 1.0);
    click(window, "focus-tempo-reset", cx);
    assert_eq!(rate(&view, cx), 1.0);
    assert_eq!(target(&view, cx), Some(ORIGINAL_BPM));
    assert!(!is_shown(window, "focus-bpm-original", cx));
}

#[gpui::test]
fn the_tempo_goes_back_when_another_track_is_selected(cx: &mut TestAppContext) {
    let (window, view) = open_focus_on_track(cx);
    type_target(window, "130", cx);
    assert_ne!(rate(&view, cx), 1.0);
    select_track(&view, 1, cx);
    assert_eq!(rate(&view, cx), 1.0);
    select_track(&view, 0, cx);
    assert_eq!(
        rate(&view, cx),
        1.0,
        "the first track is no longer remembered"
    );
}

#[gpui::test]
fn the_rate_reaches_the_playing_voice_live_and_key_lock_follows(cx: &mut TestAppContext) {
    let (window, view) = open_focus_on_track(cx);
    start_playing(&view, cx);
    let state = cx.update(|cx| view.read(cx).voice.as_ref().unwrap().state.clone());
    assert_eq!(state.tempo().rate, 1.0);
    for (typed, expected) in [("108", 0.9), ("124", 124.0 / 120.0), ("139.2", 1.16)] {
        type_target(window, typed, cx);
        assert!((state.tempo().rate - expected).abs() < 1e-6, "{typed}");
        assert!(state.tempo().key_lock);
    }
    click(window, "focus-key-lock", cx);
    assert!(!state.tempo().key_lock);
    assert!((state.tempo().rate - 1.16).abs() < 1e-6);
    click(window, "focus-key-lock", cx);
    assert!(state.tempo().key_lock);
    click(window, "focus-tempo-reset", cx);
    assert_eq!(state.tempo().rate, 1.0);
}

#[gpui::test]
fn now_playing_counts_at_the_playback_tempo_in_seconds_of_the_recording(cx: &mut TestAppContext) {
    let (window, view) = open_focus_on_track(cx);
    start_playing(&view, cx);
    let shown = cx.update(|cx| view.read(cx).now_playing()).unwrap();
    assert_eq!(shown.rate, 1.0);

    type_target(window, "132", cx);
    let shown = cx.update(|cx| view.read(cx).now_playing()).unwrap();
    assert!((shown.rate - 1.1).abs() < 1e-6, "{}", shown.rate);
    assert_eq!(shown.duration_seconds, Some(1.0));
    assert!(shown.playing);

    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let request = app.player.toggle();
            app.carry_out(request, cx);
        })
    });
    let paused = cx.update(|cx| view.read(cx).now_playing()).unwrap();
    assert!(!paused.playing);
    assert_eq!(paused.rate, 0.0);
}

#[gpui::test]
fn a_track_started_after_the_change_of_selection_plays_at_its_own_tempo(cx: &mut TestAppContext) {
    let (window, view) = open_focus_on_track(cx);
    start_playing(&view, cx);
    type_target(window, "130", cx);
    select_track(&view, 1, cx);
    cx.update(|cx| {
        let app = view.read(cx);
        let playing = app.voice.as_ref().unwrap().state.tempo().rate;
        assert!(
            (playing - 130.0 / 120.0).abs() < 1e-6,
            "selecting never changes the sound"
        );
        assert_eq!(app.tempo_rate(), 1.0);
    });
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.play_selection(cx);
        });
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let app = view.read(cx);
        if let Some(active) = &app.voice {
            assert_eq!(active.state.tempo().rate, 1.0);
        }
        assert_eq!(app.tempo_rate(), 1.0);
    });
}

#[gpui::test]
fn the_player_bar_says_original_and_target_while_the_tempo_differs(cx: &mut TestAppContext) {
    let (window, view) = open_focus_on_track(cx);
    start_playing(&view, cx);
    cx.update(|cx| assert_eq!(view.read(cx).playing_tempo_text(), None));
    assert!(!is_shown(window, "player-tempo", cx));
    type_target(window, "124", cx);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).playing_tempo_text().as_deref(),
            Some("120.0 \u{2192} 124.0 BPM")
        );
    });
    assert!(is_shown(window, "player-tempo", cx));
    click(window, "focus-tempo-reset", cx);
    assert!(!is_shown(window, "player-tempo", cx));
}

#[gpui::test]
fn the_playhead_and_time_labels_stay_in_track_time(cx: &mut TestAppContext) {
    let (window, view) = open_focus_on_track(cx);
    start_playing(&view, cx);
    type_target(window, "139.2", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        let voice = &app.voice.as_ref().unwrap().state;
        voice.seek_to(0.5);
        assert!((voice.position_seconds() - 0.5).abs() < 1e-6);
        assert!((app.player.duration_seconds().unwrap() - 1.0).abs() < 0.05);
    });
}

#[gpui::test]
fn the_metronome_follows_the_target_tempo_of_the_selected_track(cx: &mut TestAppContext) {
    let (window, view) = open_focus_on_track(cx);
    let metronome = |cx: &mut TestAppContext| cx.update(|cx| view.read(cx).metronome_bpm());
    assert_eq!(metronome(cx), ORIGINAL_BPM);
    type_target(window, "124", cx);
    assert_eq!(metronome(cx), 124.0);
    click(window, "focus-tempo-reset", cx);
    assert_eq!(metronome(cx), ORIGINAL_BPM);
}

#[gpui::test]
fn a_running_metronome_locks_to_the_playing_track_at_the_target_tempo(cx: &mut TestAppContext) {
    let (window, view) = open_focus_on_track(cx);
    click(window, "metronome-toggle", cx);
    let locked = |cx: &mut TestAppContext| cx.update(|cx| view.read(cx).metronome_locked_bpm());
    assert_eq!(locked(cx), None, "nothing plays, so it clicks freely");
    assert!(!is_shown(window, "metronome-locked", cx));

    start_playing(&view, cx);
    type_target(window, "124", cx);
    assert_eq!(locked(cx), Some(124.0));
    assert!(is_shown(window, "metronome-locked", cx));

    cx.update(|cx| view.update(cx, |app, cx| app.toggle_playback(cx)));
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    assert_eq!(locked(cx), None, "paused, it clicks freely again");
}

#[gpui::test]
fn the_metronome_stays_free_for_a_track_without_a_grid(cx: &mut TestAppContext) {
    let mut tracks = tracks_at_120();
    for track in &mut tracks {
        if let Ok(analysis) = &mut track.outcome {
            analysis.beat_grid = None;
        }
    }
    let (window, view) = open_window_on(tracks, Box::new(SilentOutput), cx);
    click(window, track(0), cx);
    click(window, "metronome-toggle", cx);
    start_playing(&view, cx);
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.update(|cx| assert_eq!(view.read(cx).metronome_locked_bpm(), None));
}

fn snapshot(directory: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read(&path).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

#[gpui::test]
fn changing_the_tempo_writes_nothing_to_the_file_or_the_cache(cx: &mut TestAppContext) {
    let scratch = std::env::temp_dir().join(format!("euphoria-tempo-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    let cache = Arc::new(
        CachedAnalyzer::new(Cache::open(&scratch.join("analysis-cache.sqlite3")).unwrap()).unwrap(),
    );
    let (window, view) = cx.update(|cx| {
        gpui::init(cx);
        let (window, view) = gpui::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| {
                let mut session = Session::start(Some(cache.clone())).unwrap();
                session.add_demo_tracks(tracks_at_120());
                let settings = Settings {
                    first_run: Some(crate::first_run::FirstRun::already_done()),
                    ..Settings::default()
                };
                let output: Box<dyn AudioOutput> = Box::new(SilentOutput);
                EuphoriaApp::new(session, settings, None, None, output, window, cx)
            })
        })
        .unwrap();
        crate::register_commands(window, &view, cx);
        (window, view)
    });
    click(window, track(0), cx);
    click(window, "focus-open", cx);
    let file = fixtures().join("sine-440hz-mono.flac");
    let file_before = std::fs::read(&file).unwrap();
    let cache_before = snapshot(&scratch);

    start_playing(&view, cx);
    type_target(window, "130", cx);
    click(window, "focus-key-lock", cx);
    click(window, "focus-tempo-up", cx);
    click(window, "focus-tempo-reset", cx);
    type_target(window, "110", cx);

    assert_eq!(std::fs::read(&file).unwrap(), file_before);
    assert_eq!(snapshot(&scratch), cache_before);
    cx.update(|cx| {
        let track = view.read(cx).selected_track().unwrap();
        assert!(track.correction.bpm.is_none());
        assert_eq!(track.bpm(), Some(ORIGINAL_BPM));
    });
    let _ = std::fs::remove_dir_all(&scratch);
}
