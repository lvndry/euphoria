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

use euphoria_core::Confidence;
use euphoria_core::TrackAnalysis;
use euphoria_core::live::{
    AudioCapture, AudioSink, CaptureError, ListenSource, LiveKey, LiveTempo,
};
use gpui::test::TestWindowExt;
use gpui::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, Point, TestAppContext, WindowBounds,
    WindowOptions, px, size,
};

use crate::library::Library;
use crate::live_model::{ListenState, LiveModel, Steadiness, shown_title};
use crate::live_settings::LiveSettings;
use crate::menu_bar::MenuBarState;
use crate::menu_bar::testing::RecordingSurface;
use crate::next_rail::EnergyChoice;
use crate::settings::Settings;
use crate::ui_tests::{click, is_shown};
use crate::views::live::{
    BPM_ID, EARLIER_ID, ENERGY_ID, HEADING_ID, KEY_BLOCK_ID, KEY_ID, LEARNING_ID, LISTENING_ID,
    LiveView, MATCH_ID, MenuRow, PAUSE_ID, SOURCE_ID, STATE_ID, WAITING_ID, energy_id, more_menu,
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
    many_tracks: bool,
    index: Option<euphoria_core::fingerprint::FingerprintIndex>,
    audio: Vec<f32>,
    refuse: Option<CaptureError>,
    settings_file: Option<PathBuf>,
}

impl Setup {
    fn music() -> Self {
        Self {
            many_tracks: false,
            index: None,
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
        ("Calm", "8B", 126.0, 3),
        ("Peak", "8B", 126.0, 9),
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
                pitch_class_profile: None,
                tempo_estimate: None,
                tempo_score_margin: None,
                tempo_octave_evidence: None,
                beat_grid: None,
                cues: Vec::new(),
                key_timeline: Vec::new(),
                quality: None,
            }),
        );
    }
    library
}

fn crowded_library() -> Library {
    let mut library = Library::default();
    for number in 1..=10 {
        let path = PathBuf::from(format!("/music/Crowd {number:02}.flac"));
        library.add_files(vec![path.clone()]);
        library.record_outcome(
            &path,
            Ok(TrackAnalysis {
                duration_seconds: 300.0,
                key: Some("8B".parse().unwrap()),
                bpm: Some(126.0),
                energy: Some(6),
                tuning_cents: None,
                key_runner_up: None,
                key_score_margin: None,
                key_source: None,
                loudness_range_lu: None,
                pitch_class_profile: None,
                tempo_estimate: None,
                tempo_score_margin: None,
                tempo_octave_evidence: None,
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
        let index = setup.index.clone();
        let many_tracks = setup.many_tracks;
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
                    Box::new(move || {
                        if many_tracks {
                            crowded_library()
                        } else {
                            library()
                        }
                    }),
                    Box::new(move |_, done, _| {
                        done.store(1, std::sync::atomic::Ordering::Relaxed);
                        index.clone().unwrap_or_else(|| {
                            let mut empty = euphoria_core::fingerprint::FingerprintIndex::new();
                            empty.freeze();
                            empty
                        })
                    }),
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

fn hear_c_major(model: &Entity<LiveModel>, cx: &mut TestAppContext) {
    cx.update(|cx| {
        model.update(cx, |model, cx| {
            let mut reading = model.reading;
            reading.key = Some(LiveKey {
                key: "8B".parse().unwrap(),
                confidence: Confidence::High,
                runner_up: None,
            });
            reading.tempo = Some(LiveTempo {
                bpm: 126.0,
                confidence: Confidence::High,
            });
            reading.energy = Some(6);
            model.pretend_heard(reading, cx);
        })
    });
    cx.run_until_parked();
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

#[gpui::test]
fn reopening_live_keeps_completed_learning_when_folders_are_unchanged(cx: &mut TestAppContext) {
    let (model, _) = model(Setup::music(), cx);
    start(&model, cx);
    cx.update(|cx| {
        model.update(cx, |model, cx| model.reload_inputs(Settings::default(), cx));
        assert!(matches!(
            model.read(cx).learning,
            crate::live_model::Learning::Ready { .. }
        ));
        model.update(cx, |model, cx| model.ensure_library(cx));
    });
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(matches!(
            model.read(cx).learning,
            crate::live_model::Learning::Ready { .. }
        ));
    });
}

fn rows(model: &Entity<LiveModel>, cx: &mut TestAppContext) -> Vec<String> {
    cx.update(|cx| {
        let model = model.read(cx);
        let library = model.library.as_ref().unwrap();
        model
            .next_rows
            .iter()
            .map(|row| shown_title(library.track(row.track_index).unwrap()))
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
    poll_until(&model, cx, hears_a_key);
    cx.update(|cx| {
        let model = model.read(cx);
        assert!((model.reading.tempo.unwrap().bpm - 126.0).abs() <= 2.0);
        assert!(model.reading.energy.is_some());
    });
    hear_c_major(&model, cx);
    cx.update(|cx| assert_eq!(model.read(cx).menu_bar_title(), "8B"));
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
    poll_until(&model, cx, hears_a_key);
    hear_c_major(&model, cx);
    let kept = rows(&model, cx);
    click(window, energy_id(EnergyChoice::Lift), cx);
    let lifted = rows(&model, cx);
    click(window, energy_id(EnergyChoice::Drop), cx);
    let dropped = rows(&model, cx);
    assert_eq!(kept.len(), lifted.len());
    assert_ne!(kept, lifted);
    assert_ne!(lifted, dropped);
    assert_eq!(
        lifted[0], "Peak",
        "the most energetic mix comes first on Lift"
    );
    assert_eq!(dropped[0], "Calm", "the calmest mix comes first on Drop");
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
    cx.update(|cx| model.update(cx, |model, cx| model.set_keep_running(false, cx)));
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
    let heard = cx.update(|cx| model.read(cx).reading.key.unwrap().key.camelot());
    let last: MenuBarState = shown.borrow().last().unwrap().clone();
    assert_eq!(last.title, heard);
    assert!(last.is_listening);
    assert!(last.keeps_running);
    cx.update(|cx| model.update(cx, |model, cx| model.pause(cx)));
    cx.run_until_parked();
    assert_eq!(shown.borrow().last().unwrap().title, "Live paused");
}

fn melody(seed: u32, seconds: f32) -> Vec<f32> {
    let total = (seconds * SAMPLE_RATE as f32) as usize;
    let mut samples = vec![0.0; total];
    let mut state = seed.wrapping_mul(2_654_435_761) | 1;
    let step = SAMPLE_RATE as usize / 4;
    let mut start = 0;
    while start < total {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let frequency = 220.0 * 2.0_f32.powf(((state >> 24) % 24) as f32 / 12.0);
        for offset in 0..(step * 2).min(total - start) {
            let time = offset as f32 / SAMPLE_RATE as f32;
            samples[start + offset] += 0.3
                * (-time * 5.0).exp()
                * ((TAU * frequency * time).sin() + 0.4 * (TAU * 2.0 * frequency * time).sin());
        }
        start += step;
    }
    samples
}

#[gpui::test]
fn a_recognised_track_shows_its_match_and_the_list_ranks_from_it(cx: &mut TestAppContext) {
    use euphoria_core::AudioBuffer;
    use euphoria_core::fingerprint::{FingerprintIndex, fingerprint};

    let tracks: Vec<Vec<f32>> = (1..=8).map(|seed| melody(seed, 60.0)).collect();
    let mut index = FingerprintIndex::new();
    for (id, track) in tracks.iter().enumerate() {
        let samples = AudioBuffer::new(track.clone(), NonZeroU32::new(SAMPLE_RATE).unwrap());
        index.add(id, &fingerprint(&samples));
    }
    index.freeze();
    let mut setup = Setup::music();
    setup.index = Some(index);
    let played = 1;
    setup.audio = tracks[played][SAMPLE_RATE as usize * 20..SAMPLE_RATE as usize * 34].to_vec();
    let (model, _) = model(setup, cx);
    start(&model, cx);
    let window = open_panel(&model, cx);
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(matches!(
            model.read(cx).learning,
            crate::live_model::Learning::Ready { .. }
        ))
    });
    poll_until(&model, cx, |model| model.reading.matched.is_some());
    cx.update(|cx| {
        let model = model.read(cx);
        let (track, found) = model.matched_track().expect("a matched library track");
        assert_eq!(found.track, played);
        assert_eq!(
            track.path,
            model.library.as_ref().unwrap().tracks()[played].path
        );
        assert!((found.start_seconds - 20.0).abs() < 1.5);
        assert!(!model.next_rows.is_empty());
        assert!(model.next_rows.iter().all(|row| row.track_index != played));
    });
    assert!(is_shown(window, MATCH_ID, cx));
    assert!(!is_shown(window, LEARNING_ID, cx));
}

#[gpui::test]
fn without_a_match_the_panel_does_not_claim_one(cx: &mut TestAppContext) {
    let (model, _) = model(Setup::music(), cx);
    start(&model, cx);
    let window = open_panel(&model, cx);
    poll_until(&model, cx, hears_a_key);
    assert!(!is_shown(window, MATCH_ID, cx));
    cx.update(|cx| assert!(model.read(cx).matched_track().is_none()));
}

#[test]
fn playback_words_say_how_far_and_how_the_pitch_went() {
    use euphoria_core::fingerprint::Playback;
    let words = |ratio, follows| {
        crate::views::live::playback_words(Playback {
            tempo_ratio: ratio,
            pitch_follows_tempo: follows,
        })
    };
    assert_eq!(words(1.0, false), None);
    assert_eq!(words(1.04, false).as_deref(), Some("+4.0%, key lock"));
    assert_eq!(words(0.92, true).as_deref(), Some("-8.0%, pitch follows"));
}

fn open_small_panel(
    model: &Entity<LiveModel>,
    height: f32,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<LiveView>) {
    cx.update(|cx| {
        gpui::init(cx);
        let view_model = model.clone();
        gpui::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(340.0), px(height)),
                })),
                ..Default::default()
            },
            cx,
            |_, cx| cx.new(|cx| LiveView::new(view_model, cx)),
        )
        .unwrap()
    })
}

fn last_row_is_in_view(view: &Entity<LiveView>, last: usize, cx: &mut TestAppContext) -> bool {
    cx.update(|cx| {
        let scroll = &view.read(cx).scroll;
        let row = scroll
            .bounds_for_item(last)
            .expect("the last row was laid out");
        let viewport = scroll.bounds();
        row.bottom() + scroll.offset().y <= viewport.bottom() + px(1.0)
    })
}

#[gpui::test]
fn a_next_list_taller_than_the_panel_scrolls_to_its_last_row_with_the_arrow_keys(
    cx: &mut TestAppContext,
) {
    let mut setup = Setup::music();
    setup.many_tracks = true;
    let (model, _) = model(setup, cx);
    start(&model, cx);
    let (window, view) = open_small_panel(&model, 420.0, cx);
    poll_until(&model, cx, hears_a_key);
    hear_c_major(&model, cx);
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    cx.update(|cx| {
        assert_eq!(
            model.read(cx).next_rows.len(),
            crate::live_model::NEXT_LIMIT
        )
    });
    let last = crate::live_model::NEXT_LIMIT - 1;
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    assert!(
        !last_row_is_in_view(&view, last, cx),
        "the list is taller than the room it has"
    );
    cx.update_window(window, |_, window, cx| {
        let focus = view.read(cx).focus.clone();
        focus.focus(window, cx);
    })
    .unwrap();
    for _ in 0..=last {
        cx.simulate_keystrokes(window, "down");
    }
    for _ in 0..3 {
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .unwrap();
    }
    assert!(
        last_row_is_in_view(&view, last, cx),
        "the last row scrolled into view"
    );
    cx.update(|cx| assert_eq!(view.read(cx).selected_row, Some(last)));
}

fn labels(rows: &[MenuRow]) -> Vec<(&'static str, Option<bool>)> {
    rows.iter().map(|row| (row.label, row.checked)).collect()
}

#[gpui::test]
fn the_more_menu_holds_pause_the_two_settings_with_ticks_open_and_quit(cx: &mut TestAppContext) {
    let (model, _) = model(Setup::music(), cx);
    start(&model, cx);
    let rows = cx.update(|cx| more_menu(model.read(cx), false));
    assert_eq!(
        labels(&rows),
        [
            ("Pause listening", None),
            ("Keep Live running when the window is closed", Some(true)),
            ("Open Live at login", Some(false)),
            ("Open Euphoria", None),
            ("Quit Euphoria", None),
        ]
    );
    cx.update(|cx| {
        model.update(cx, |model, cx| {
            model.set_keep_running(false, cx);
            model.pause(cx);
        })
    });
    let rows = cx.update(|cx| more_menu(model.read(cx), true));
    assert_eq!(rows[0].label, "Resume listening");
    assert_eq!(rows[1].checked, Some(false));
    assert_eq!(rows[2].checked, Some(true));
}

#[gpui::test]
fn the_state_line_says_settling_and_then_steady_and_kept_readings_become_earlier(
    cx: &mut TestAppContext,
) {
    let (model, _) = model(Setup::music(), cx);
    start(&model, cx);
    let window = open_panel(&model, cx);
    poll_until(&model, cx, hears_a_key);
    hear_c_major(&model, cx);
    cx.update(|cx| model.update(cx, |model, _| model.backdate_signature(Duration::ZERO)));
    cx.update(|cx| {
        assert_eq!(model.read(cx).steadiness(), Some(Steadiness::Settling));
        assert!(model.read(cx).earlier_readings().is_empty());
    });
    assert!(is_shown(window, STATE_ID, cx));
    assert!(!is_shown(window, EARLIER_ID, cx));
    cx.update(|cx| {
        model.update(cx, |model, cx| {
            model.backdate_signature(Duration::from_secs(7));
            let reading = model.reading;
            model.pretend_heard(reading, cx);
        })
    });
    cx.update(|cx| assert_eq!(model.read(cx).steadiness(), Some(Steadiness::Steady)));
    cx.update(|cx| {
        model.update(cx, |model, cx| {
            let mut reading = model.reading;
            reading.key = Some(LiveKey {
                key: "9B".parse().unwrap(),
                confidence: Confidence::High,
                runner_up: None,
            });
            model.pretend_heard(reading, cx);
            model.backdate_signature(Duration::from_secs(7));
            let reading = model.reading;
            model.pretend_heard(reading, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let model = model.read(cx);
        assert_eq!(model.steadiness(), Some(Steadiness::Steady));
        let earlier = model.earlier_readings();
        assert_eq!(earlier.len(), 1);
        assert_eq!(earlier[0].key.camelot(), "8B");
    });
    assert!(is_shown(window, EARLIER_ID, cx));
}

#[gpui::test]
fn the_next_heading_says_what_the_list_follows(cx: &mut TestAppContext) {
    let (model, _) = model(Setup::music(), cx);
    start(&model, cx);
    let window = open_panel(&model, cx);
    poll_until(&model, cx, hears_a_key);
    hear_c_major(&model, cx);
    cx.update(|cx| {
        assert_eq!(model.read(cx).next_heading(), "Next for 8B \u{b7} 126 BPM");
    });
    assert!(is_shown(window, HEADING_ID, cx));
    cx.update(|cx| {
        model.update(cx, |model, cx| {
            model.ensure_library(cx);
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        model.update(cx, |model, cx| model.pretend_match(0, cx));
    });
    cx.update(|cx| {
        let model = model.read(cx);
        let title = shown_title(model.matched_track().unwrap().0);
        assert_eq!(model.next_heading(), format!("Next after {title}"));
    });
}

/// Draws the frames a change asks for, reusing what was cached as the app
/// does, where a test's `render_frame` draws everything again.
fn draw_frames(window: AnyWindowHandle, cx: &mut TestAppContext) {
    for _ in 0..4 {
        cx.run_until_parked();
        cx.update_window(window, |_, window, cx| window.draw(cx).clear(cx))
            .unwrap();
    }
}

fn with_level(model: &LiveModel, rms_db: f32) -> euphoria_core::live::LiveReading {
    let mut reading = model.reading;
    reading.level = Some(euphoria_core::live::LevelReading {
        rms_db,
        peak_db: rms_db + 6.0,
    });
    reading.heard_seconds += 0.1;
    reading
}

fn list_renders(view: &Entity<LiveView>, cx: &mut TestAppContext) -> usize {
    cx.update(|cx| view.read(cx).list.read(cx).renders)
}

#[gpui::test]
fn a_change_of_the_level_alone_moves_the_dot_and_leaves_the_next_list_alone(
    cx: &mut TestAppContext,
) {
    let mut setup = Setup::music();
    setup.many_tracks = true;
    let (model, _) = model(setup, cx);
    start(&model, cx);
    let (window, view) = open_small_panel(&model, 420.0, cx);
    poll_until(&model, cx, hears_a_key);
    cx.update(|cx| {
        let level = model.read(cx).level.clone();
        level.update(cx, |level, _| level.allow_motion());
    });
    hear_c_major(&model, cx);
    draw_frames(window, cx);
    assert!(list_renders(&view, cx) > 0, "the list was drawn");
    let renders_before = list_renders(&view, cx);
    let mut positions = Vec::new();
    for step in 0..6 {
        std::thread::sleep(crate::motion::FAST + Duration::from_millis(10));
        cx.update(|cx| {
            model.update(cx, |model, cx| {
                let reading = with_level(model, -40.0 + 12.0 * (step % 3) as f32);
                model.pretend_polled(reading, cx);
            })
        });
        draw_frames(window, cx);
        positions.push(cx.update(|cx| model.read(cx).level.read(cx).position));
    }
    positions.dedup();
    assert!(
        positions.len() > 1,
        "the dot followed the level: {positions:?}"
    );
    assert_eq!(
        list_renders(&view, cx),
        renders_before,
        "the Next list was not drawn again"
    );
}

#[gpui::test]
fn scrolling_the_next_list_never_resizes_the_window_and_a_change_waits_for_it_to_settle(
    cx: &mut TestAppContext,
) {
    let mut setup = Setup::music();
    setup.many_tracks = true;
    let (model, _) = model(setup, cx);
    start(&model, cx);
    let (window, view) = open_small_panel(&model, 700.0, cx);
    cx.update(|cx| view.update(cx, |view, _| view.top_left = Some((0.0, 0.0))));
    poll_until(&model, cx, hears_a_key);
    hear_c_major(&model, cx);
    draw_frames(window, cx);
    let first = cx.update(|cx| view.update(cx, |view, _| std::mem::take(&mut view.resizes)));
    assert!(
        !first.is_empty(),
        "the window took the height of its content"
    );
    let max_offset = cx.update(|cx| view.read(cx).scroll.max_offset().y);
    assert!(max_offset > px(0.0), "the list scrolls");

    for step in 1..=12 {
        cx.update_window(window, |_, window, cx| {
            window.scroll(
                "live-next-list",
                gpui::ScrollDelta::Pixels(gpui::point(px(0.0), px(-8.0))),
                cx,
            );
        })
        .unwrap();
        if step == 6 {
            cx.update(|cx| {
                view.update(cx, |view, cx| {
                    view.picker_open = true;
                    cx.notify();
                })
            });
        }
        cx.update(|cx| {
            model.update(cx, |model, cx| {
                let reading = with_level(model, -40.0 + 4.0 * (step % 5) as f32);
                model.pretend_polled(reading, cx);
            })
        });
        draw_frames(window, cx);
    }
    let offset = cx.update(|cx| view.read(cx).scroll.offset().y);
    assert!(offset < px(0.0), "the list scrolled");
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).resizes,
            Vec::new(),
            "no resize while the list scrolls"
        )
    });

    std::thread::sleep(crate::motion::LAYOUT + Duration::from_millis(30));
    cx.executor().advance_clock(crate::motion::LAYOUT);
    draw_frames(window, cx);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).resizes.len(),
            1,
            "the opened source picker resized the window once the scroll settled"
        )
    });
}

#[gpui::test]
fn rows_that_appear_grow_the_panel(cx: &mut TestAppContext) {
    let (model, _) = model(Setup::music(), cx);
    start(&model, cx);
    let (window, view) = open_small_panel(&model, 420.0, cx);
    cx.update(|cx| view.update(cx, |view, _| view.top_left = Some((0.0, 0.0))));
    draw_frames(window, cx);
    let empty_height = cx.update(|cx| view.read(cx).sizing.window_height);
    cx.update(|cx| assert_eq!(view.read(cx).list.read(cx).renders, 0));

    poll_until(&model, cx, hears_a_key);
    hear_c_major(&model, cx);
    draw_frames(window, cx);
    let rows = cx.update(|cx| model.read(cx).next_rows.len());
    assert!(rows >= 2, "the library has tracks that mix in: {rows}");
    let full_height = cx.update(|cx| view.read(cx).sizing.window_height);
    assert!(
        full_height > empty_height + 2.0 * 51.0,
        "the panel grew from {empty_height} to {full_height} for {rows} rows"
    );
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).resizes.last().map(|(_, height)| *height),
            Some(full_height)
        )
    });
}

#[test]
fn the_panel_ignores_small_changes_of_the_same_content_and_follows_new_parts() {
    use crate::views::live::{PanelSizing, Sizing, Structure};
    let now = Instant::now();
    let shown = Structure {
        rows: 4,
        ..Structure::default()
    };
    let mut sizing = PanelSizing::new(560.0);
    assert_eq!(
        sizing.decide(400.0, shown, 700.0, now),
        Sizing::Resize(400.0)
    );
    assert_eq!(sizing.decide(405.5, shown, 700.0, now), Sizing::Keep);
    assert_eq!(sizing.decide(395.0, shown, 700.0, now), Sizing::Keep);
    let more = Structure { rows: 5, ..shown };
    assert_eq!(
        sizing.decide(455.0, more, 700.0, now),
        Sizing::Resize(455.0)
    );
    assert_eq!(
        sizing.decide(480.0, more, 700.0, now),
        Sizing::Resize(480.0)
    );
    sizing.note_scroll(-30.0, now);
    let fewer = Structure { rows: 3, ..shown };
    assert!(matches!(
        sizing.decide(345.0, fewer, 700.0, now),
        Sizing::AfterScroll(_)
    ));
    let later = now + crate::motion::LAYOUT;
    assert_eq!(
        sizing.decide(345.0, fewer, 700.0, later),
        Sizing::Resize(345.0)
    );
}

#[gpui::test]
fn the_tempo_and_energy_sit_centred_against_the_key(cx: &mut TestAppContext) {
    let (model, _) = model(Setup::music(), cx);
    start(&model, cx);
    let window = open_panel(&model, cx);
    poll_until(&model, cx, hears_a_key);
    hear_c_major(&model, cx);
    let (key, tempo, energy) = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            (
                window.find(KEY_BLOCK_ID).bounds(),
                window.find(BPM_ID).bounds(),
                window.find(ENERGY_ID).bounds(),
            )
        })
        .unwrap();
    let key_centre = key.top() + key.size.height / 2.0;
    let figures_centre = tempo.top() + (energy.bottom() - tempo.top()) / 2.0;
    assert!(
        (key_centre - figures_centre).abs() <= px(1.0),
        "key centre {key_centre:?}, tempo and energy centre {figures_centre:?}"
    );
    assert!(tempo.bottom() < energy.top(), "tempo above energy");
}
