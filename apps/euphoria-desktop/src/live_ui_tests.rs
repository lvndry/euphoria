// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Live through its model, its panel and its menu bar item, in a headless
//! window, listening to generated audio: a C major pad under a kick pattern
//! at 126 BPM, or silence.

use std::f32::consts::TAU;
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use euphoria_core::TrackAnalysis;
use euphoria_core::live::{AudioCapture, AudioSink, CaptureError, ListenSource};
use gpui::test::TestWindowExt;
use gpui::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, Point, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};

use crate::library::Library;
use crate::live_model::{ListenState, LiveModel};
use crate::live_settings::LiveSettings;
use crate::menu_bar::MenuBarState;
use crate::menu_bar::testing::RecordingSurface;
use crate::next_rail::EnergyChoice;
use crate::settings::Settings;
use crate::ui_tests::{click, is_shown};
use crate::views::live::{
    BPM_ID, KEEP_ID, KEY_ID, LISTENING_ID, LiveView, PAUSE_ID, SOURCE_ID, WAITING_ID, energy_id,
    row_id, source_option_id,
};

const SAMPLE_RATE: u32 = 44_100;

fn programme(seconds: f32, with_music: bool) -> Vec<f32> {
    let total = (seconds * SAMPLE_RATE as f32) as usize;
    if !with_music {
        return vec![0.0; total];
    }
    let beat = (60.0 / 126.0 * SAMPLE_RATE as f32) as usize;
    (0..total)
        .map(|index| {
            let time = index as f32 / SAMPLE_RATE as f32;
            let mut value = 0.0;
            for frequency in [130.81_f32, 164.81, 196.0] {
                for harmonic in 1..=4 {
                    value +=
                        0.08 * (TAU * frequency * harmonic as f32 * time).sin() / harmonic as f32;
                }
            }
            let since_beat = (index % beat) as f32 / SAMPLE_RATE as f32;
            let frequency = 50.0 + 110.0 * (-since_beat * 30.0).exp();
            value + 0.7 * (-since_beat * 14.0).exp() * (TAU * frequency * since_beat).sin()
        })
        .collect()
}

#[derive(Default)]
struct Seen {
    started_with: Vec<ListenSource>,
    stops: usize,
}

struct FakeCapture {
    audio: Vec<f32>,
    seen: Arc<Mutex<Seen>>,
    refuse: Option<CaptureError>,
    sink: Option<AudioSink>,
    sources: Vec<ListenSource>,
}

impl AudioCapture for FakeCapture {
    fn sources(&self) -> Vec<ListenSource> {
        self.sources.clone()
    }

    fn start(
        &mut self,
        source: &ListenSource,
        mut sink: AudioSink,
    ) -> Result<NonZeroU32, CaptureError> {
        if let Some(error) = &self.refuse {
            return Err(error.clone());
        }
        self.seen.lock().unwrap().started_with.push(source.clone());
        for chunk in self.audio.chunks(441) {
            sink(chunk);
        }
        self.sink = Some(sink);
        Ok(NonZeroU32::new(SAMPLE_RATE).unwrap())
    }

    fn stop(&mut self) {
        self.sink = None;
        self.seen.lock().unwrap().stops += 1;
    }
}

struct Setup {
    audio: Vec<f32>,
    refuse: Option<CaptureError>,
    settings_file: Option<PathBuf>,
}

impl Setup {
    fn music() -> Self {
        Self {
            audio: programme(14.0, true),
            refuse: None,
            settings_file: None,
        }
    }

    fn silence() -> Self {
        Self {
            audio: programme(14.0, false),
            ..Self::music()
        }
    }
}

fn library() -> Library {
    let mut library = Library::default();
    for (name, key, bpm, energy) in [
        ("Same Key", "8B", 126.0, 6),
        ("One Step", "9B", 128.0, 7),
        ("Relative", "8A", 124.0, 5),
        ("Far Key", "3A", 126.0, 6),
        ("Too Fast", "8B", 170.0, 6),
    ] {
        let path = PathBuf::from(format!("/music/{name}.flac"));
        library.add_files(vec![path.clone()]);
        library.record_outcome(
            &path,
            Ok(TrackAnalysis {
                duration_seconds: 300.0,
                key: Some(key.parse().unwrap()),
                bpm: Some(bpm),
                energy: Some(energy),
                tuning_cents: None,
                key_runner_up: None,
                key_score_margin: None,
                key_source: None,
                loudness_range_lu: None,
                tempo_estimate: None,
                tempo_score_margin: None,
                beat_grid: None,
                cues: Vec::new(),
                key_timeline: Vec::new(),
                quality: None,
            }),
        );
    }
    library
}

fn model(setup: Setup, cx: &mut TestAppContext) -> (Entity<LiveModel>, Arc<Mutex<Seen>>) {
    let seen = Arc::new(Mutex::new(Seen::default()));
    let model = {
        let seen = Arc::clone(&seen);
        let audio = setup.audio.clone();
        let refuse = setup.refuse.clone();
        cx.update(|cx| {
            cx.new(|cx| {
                LiveModel::with_parts(
                    Box::new(move || {
                        Box::new(FakeCapture {
                            audio: audio.clone(),
                            seen: Arc::clone(&seen),
                            refuse: refuse.clone(),
                            sink: None,
                            sources: vec![ListenSource::Process {
                                process_id: 7,
                                name: "rekordbox".into(),
                            }],
                        })
                    }),
                    Box::new(library),
                    Settings::default(),
                    setup.settings_file.clone(),
                    Duration::from_millis(100),
                    cx,
                )
            })
        })
    };
    (model, seen)
}

fn poll_until(
    model: &Entity<LiveModel>,
    cx: &mut TestAppContext,
    ready: impl Fn(&LiveModel) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        cx.update(|cx| model.update(cx, |model, cx| model.poll(cx)));
        cx.run_until_parked();
        if cx.update(|cx| ready(model.read(cx))) || Instant::now() > deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn hears_a_key(model: &LiveModel) -> bool {
    model.reading.key.is_some() && !model.reading.listening
}

fn start(model: &Entity<LiveModel>, cx: &mut TestAppContext) {
    cx.update(|cx| {
        model.update(cx, |model, cx| {
            model.ensure_library(cx);
            model.start(cx);
        })
    });
    cx.run_until_parked();
}

fn open_panel(model: &Entity<LiveModel>, cx: &mut TestAppContext) -> AnyWindowHandle {
    cx.update(|cx| {
        gpui::init(cx);
        let view_model = model.clone();
        let (window, _) = gpui::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(340.0), px(620.0)),
                })),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|cx| LiveView::new(view_model, cx)),
        )
        .unwrap();
        window
    })
}

fn rows(model: &Entity<LiveModel>, cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| {
        let model = model.read(cx);
        let library = model.library.as_ref().unwrap();
        model
            .next_rows
            .iter()
            .map(|row| library.track(row.track_index).unwrap().title().to_string())
            .collect()
    })
}

#[gpui::test]
fn it_says_listening_and_shows_nothing_guessed_while_the_audio_is_silent(cx: &mut TestAppContext) {
    let (model, _) = model(Setup::silence(), cx);
    start(&model, cx);
    let window = open_panel(&model, cx);
    poll_until(&model, cx, |model| model.reading.heard_seconds == 0.0);
    std::thread::sleep(Duration::from_millis(400));
    cx.update(|cx| model.update(cx, |model, cx| model.poll(cx)));
    cx.update(|cx| {
        let model = model.read(cx);
        assert_eq!(model.state, ListenState::Listening);
        assert!(model.reading.listening);
        assert!(model.reading.key.is_none() && model.reading.tempo.is_none());
        assert!(model.next_rows.is_empty());
        assert_eq!(model.menu_bar_title(), "Live");
    });
    assert!(is_shown(window, LISTENING_ID, cx));
    assert!(is_shown(window, WAITING_ID, cx));
    assert!(!is_shown(window, KEY_ID, cx));
    assert!(!is_shown(window, BPM_ID, cx));
}

#[gpui::test]
fn it_ranks_the_library_from_the_key_and_tempo_heard_without_a_match(cx: &mut TestAppContext) {
    let (model, _) = model(Setup::music(), cx);
    start(&model, cx);
    let window = open_panel(&model, cx);
    poll_until(&model, cx, |model| {
        hears_a_key(model) && !model.next_rows.is_empty()
    });
    cx.update(|cx| {
        let model = model.read(cx);
        let key = model.reading.key.unwrap().key;
        assert_eq!(key.camelot(), "8B");
        assert!((model.reading.tempo.unwrap().bpm - 126.0).abs() <= 2.0);
        assert_eq!(model.menu_bar_title(), "8B");
    });
    let listed = rows(&model, cx);
    for wanted in ["Same Key", "One Step", "Relative"] {
        assert!(
            listed.contains(&wanted.to_string()),
            "{wanted} in {listed:?}"
        );
    }
    assert!(!listed.contains(&"Far Key".to_string()));
    assert!(!listed.contains(&"Too Fast".to_string()));
    assert!(is_shown(window, KEY_ID, cx));
    assert!(is_shown(window, BPM_ID, cx));
    assert!(!is_shown(window, WAITING_ID, cx));
    let first_row = cx.update(|cx| model.read(cx).next_rows[0].track_index);
    assert!(is_shown(window, row_id(first_row), cx));
}

#[gpui::test]
fn keep_lift_and_drop_reorder_the_same_candidates(cx: &mut TestAppContext) {
    let (model, _) = model(Setup::music(), cx);
    start(&model, cx);
    let window = open_panel(&model, cx);
    poll_until(&model, cx, |model| {
        hears_a_key(model) && !model.next_rows.is_empty()
    });
    let kept = rows(&model, cx);
    click(window, energy_id(EnergyChoice::Lift), cx);
    let lifted = rows(&model, cx);
    click(window, energy_id(EnergyChoice::Drop), cx);
    let dropped = rows(&model, cx);
    assert_eq!(kept.len(), lifted.len());
    assert_ne!(kept, lifted);
    assert_ne!(lifted, dropped);
    assert_eq!(
        lifted[0], "One Step",
        "the most energetic mix comes first on Lift"
    );
}

#[gpui::test]
fn pausing_forgets_what_was_heard_and_shows_that_nothing_is_listened_to(cx: &mut TestAppContext) {
    let (model, seen) = model(Setup::music(), cx);
    start(&model, cx);
    let window = open_panel(&model, cx);
    poll_until(&model, cx, hears_a_key);
    assert!(is_shown(window, KEY_ID, cx));
    click(window, PAUSE_ID, cx);
    cx.update(|cx| {
        let model = model.read(cx);
        assert_eq!(model.state, ListenState::Paused);
        assert!(model.reading.key.is_none());
        assert!(model.next_rows.is_empty());
        assert!(!model.is_listening());
        assert_eq!(model.menu_bar_title(), "Live paused");
    });
    assert!(seen.lock().unwrap().stops >= 1);
    assert!(!is_shown(window, KEY_ID, cx));
    assert!(is_shown(window, WAITING_ID, cx));
    click(window, PAUSE_ID, cx);
    cx.update(|cx| assert!(model.read(cx).is_listening()));
}

#[gpui::test]
fn the_source_picker_restarts_listening_on_the_chosen_application(cx: &mut TestAppContext) {
    let (model, seen) = model(Setup::music(), cx);
    start(&model, cx);
    let window = open_panel(&model, cx);
    click(window, SOURCE_ID, cx);
    assert!(is_shown(window, source_option_id(1), cx));
    click(window, source_option_id(1), cx);
    cx.update(|cx| {
        let model = model.read(cx);
        assert_eq!(model.source.label(), "rekordbox");
        assert!(model.is_listening());
    });
    let started = seen.lock().unwrap().started_with.clone();
    assert_eq!(started.first(), Some(&ListenSource::AllAudio));
    assert_eq!(started.last().map(ListenSource::label), Some("rekordbox"));
    assert!(!is_shown(window, source_option_id(1), cx));
}

#[gpui::test]
fn a_capture_that_cannot_start_says_why(cx: &mut TestAppContext) {
    let mut setup = Setup::music();
    setup.refuse = Some(CaptureError::Unsupported(
        "Listening to other applications needs macOS 14.2 or later.".into(),
    ));
    let (model, _) = model(setup, cx);
    start(&model, cx);
    let window = open_panel(&model, cx);
    cx.update(|cx| {
        let model = model.read(cx);
        assert!(matches!(model.state, ListenState::Unavailable(_)));
        assert_eq!(model.menu_bar_title(), "Live off");
    });
    assert!(is_shown(window, WAITING_ID, cx));
    assert!(!is_shown(window, KEY_ID, cx));
}

#[gpui::test]
fn keeping_live_running_is_remembered_in_a_file_of_its_own(cx: &mut TestAppContext) {
    let folder = std::env::temp_dir().join(format!("euphoria-live-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    let path = folder.join("live.json");
    let mut setup = Setup::music();
    setup.settings_file = Some(path.clone());
    let (model, _) = model(setup, cx);
    start(&model, cx);
    let window = open_panel(&model, cx);
    assert!(LiveSettings::load(&path).started_once);
    assert!(LiveSettings::load(&path).keeps_running());
    click(window, KEEP_ID, cx);
    let stored = LiveSettings::load(&path);
    assert!(!stored.keep_running_when_closed);
    assert!(!stored.keeps_running());
    click(window, energy_id(EnergyChoice::Lift), cx);
    assert_eq!(LiveSettings::load(&path).energy, EnergyChoice::Lift);
    let _ = std::fs::remove_dir_all(&folder);
}

#[gpui::test]
fn the_menu_bar_item_follows_the_model(cx: &mut TestAppContext) {
    let (model, _) = model(Setup::music(), cx);
    let surface = RecordingSurface::default();
    let shown = Rc::clone(&surface.shown);
    let (_sender, commands) = async_channel::unbounded();
    cx.update(|cx| crate::menu_bar::install(&model, Box::new(surface), commands, cx));
    assert_eq!(shown.borrow().last().unwrap().title, "Live paused");
    start(&model, cx);
    poll_until(&model, cx, hears_a_key);
    let last: MenuBarState = shown.borrow().last().unwrap().clone();
    assert_eq!(last.title, "8B");
    assert!(last.is_listening);
    assert!(last.keeps_running);
    cx.update(|cx| model.update(cx, |model, cx| model.pause(cx)));
    cx.run_until_parked();
    assert_eq!(shown.borrow().last().unwrap().title, "Live paused");
}
