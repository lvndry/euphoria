// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// 15 tests: the waveform, the energy curve, the key timeline, the vocals lane and the wheel.

/// One fixture track that changes key: A minor for its first 40 seconds,
/// silence, then E minor from 50 seconds to its end at 100.
fn track_changing_key() -> DemoTrack {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/euphoria-core/tests/fixtures");
    DemoTrack {
        path: fixtures.join("sine-440hz-mono.flac"),
        outcome: Ok(TrackAnalysis {
            duration_seconds: 100.0,
            quality: None,
            key: Some(euphoria_core::Key::new(9, euphoria_core::Mode::Minor)),
            bpm: Some(124.0),
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
            key_timeline: vec![
                euphoria_core::KeySection {
                    start_seconds: 0.0,
                    end_seconds: 40.0,
                    key: euphoria_core::Key::new(9, euphoria_core::Mode::Minor),
                },
                euphoria_core::KeySection {
                    start_seconds: 50.0,
                    end_seconds: 100.0,
                    key: euphoria_core::Key::new(4, euphoria_core::Mode::Minor),
                },
            ],
        }),
    }
}

/// A waveform with an energy curve that dips in the middle.
pub(crate) fn waveform_with_energy() -> Waveform {
    let columns: Vec<crate::peaks::BandPeaks> = (0..200)
        .map(|_| crate::peaks::BandPeaks {
            low: 0.5,
            mid: 0.4,
            high: 0.2,
        })
        .collect();
    let steps = vec![
        EnergyStep {
            start: 0.0,
            end: 0.4,
            rating: Some(9),
        },
        EnergyStep {
            start: 0.4,
            end: 0.6,
            rating: None,
        },
        EnergyStep {
            start: 0.6,
            end: 1.0,
            rating: Some(10),
        },
    ];
    Waveform::Ready(columns.into(), Some(EnergyCurve::Steps(steps.into())))
}

fn open_track_changing_key(
    theme: ThemeName,
    panels: Panels,
    window_size: gpui::Size<gpui::Pixels>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let (window, view) = open_window_with(
        vec![track_changing_key()],
        Box::new(SilentOutput),
        Settings {
            theme,
            panels,
            ..Settings::default()
        },
        None,
        window_size,
        cx,
    );
    cx.update(|cx| view.update(cx, |app, _| app.show_waveform(waveform_with_energy())));
    (window, view)
}

#[gpui::test]
fn the_energy_curve_and_the_key_timeline_line_up_under_the_waveform_in_both_layouts(
    cx: &mut TestAppContext,
) {
    for theme in [ThemeName::Slate, ThemeName::Deck] {
        let (window, _) = open_track_changing_key(theme, Panels::default(), USUAL_WINDOW, cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            let waveform = window.find("waveform").bounds();
            let energy = window.find("energy-curve").bounds();
            let timeline = window.find("key-timeline").bounds();
            for under in [energy, timeline] {
                assert_eq!(under.left(), waveform.left(), "{theme:?}");
                assert_eq!(under.right(), waveform.right(), "{theme:?}");
            }
            assert!(energy.top() >= waveform.bottom(), "{theme:?}");
            assert!(timeline.top() >= energy.bottom(), "{theme:?}");

            let first = window.find(("key-stretch", 0_usize)).bounds();
            let second = window.find(("key-stretch", 1_usize)).bounds();
            let at = |fraction: f32| waveform.left() + waveform.size.width * fraction;
            assert!((first.left() - at(0.0)).abs() < px(1.0), "{theme:?}");
            assert!((first.right() - at(0.4)).abs() < px(1.0), "{theme:?}");
            assert!((second.left() - at(0.5)).abs() < px(1.0), "{theme:?}");
            assert!(second.left() > first.right(), "the silence stays a gap");
            if theme == ThemeName::Deck {
                let strip = window.find("inspector").bounds();
                assert!(
                    timeline.bottom() <= strip.bottom(),
                    "{timeline:?} in {strip:?}"
                );
            }
        })
        .unwrap();
    }
}

fn vocal_windows_active_between(
    first_second: f32,
    last_second: f32,
) -> crate::track_overview::VocalLane {
    let windows: Vec<euphoria_core::stems::VocalWindow> = (0..24)
        .map(|index| {
            let start_seconds = index as f32 * 4.0;
            euphoria_core::stems::VocalWindow {
                start_seconds,
                duration_seconds: 8.0,
                active: start_seconds >= first_second && start_seconds < last_second,
            }
        })
        .collect();
    crate::track_overview::VocalLane::Ready(windows.into())
}

fn show_vocal_lane_of_selected(
    view: &Entity<EuphoriaApp>,
    lane: crate::track_overview::VocalLane,
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let path = app.selected_track().unwrap().path.clone();
            app.show_vocal_lane(path, lane);
            cx.notify();
        })
    });
}

fn fails_to_separate(
    _mode: euphoria_core::stems::SeparationMode,
) -> Result<Box<dyn crate::stems::StemSeparation>, euphoria_core::EuphoriaError> {
    Err(euphoria_core::EuphoriaError::NoDataDirectory)
}

#[gpui::test]
fn the_energy_strip_and_the_vocals_lane_are_labelled_and_stack_under_the_waveform_in_both_layouts(
    cx: &mut TestAppContext,
) {
    for theme in [ThemeName::Slate, ThemeName::Deck] {
        let (window, view) = open_track_changing_key(theme, Panels::default(), USUAL_WINDOW, cx);
        show_vocal_lane_of_selected(&view, vocal_windows_active_between(40.0, 80.0), cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            let waveform = window.find("waveform").bounds();
            let energy = window.find("energy-curve").bounds();
            let vocals = window.find("vocals-lane").bounds();
            let timeline = window.find("key-timeline").bounds();
            assert_eq!(vocals.left(), waveform.left(), "{theme:?}");
            assert_eq!(vocals.right(), waveform.right(), "{theme:?}");
            assert!(vocals.top() >= energy.bottom(), "{theme:?}");
            assert!(timeline.top() >= vocals.bottom(), "{theme:?}");
            assert!(vocals.size.height < energy.size.height, "{theme:?}");

            let energy_label = window.find("energy-label").bounds();
            let vocals_label = window.find("vocals-label").bounds();
            assert_eq!(energy_label.left(), energy.left(), "{theme:?}");
            assert_eq!(vocals_label.left(), vocals.left(), "{theme:?}");
            assert!(energy_label.bottom() <= energy.bottom(), "{theme:?}");
            assert!(vocals_label.bottom() <= vocals.bottom(), "{theme:?}");
            if theme == ThemeName::Deck {
                let strip = window.find("inspector").bounds();
                assert!(timeline.bottom() <= strip.bottom(), "{theme:?}");
            }
        })
        .unwrap();
    }
}

#[gpui::test]
fn the_vocal_stretches_sit_under_the_part_of_the_waveform_they_belong_to(cx: &mut TestAppContext) {
    let (window, view) =
        open_track_changing_key(ThemeName::Slate, Panels::default(), USUAL_WINDOW, cx);
    show_vocal_lane_of_selected(&view, vocal_windows_active_between(40.0, 80.0), cx);
    let expected = match vocal_windows_active_between(40.0, 80.0) {
        crate::track_overview::VocalLane::Ready(windows) => {
            crate::track_overview::vocal_stretches(&windows, 100.0)
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(expected.len(), 1);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let waveform = window.find("waveform").bounds();
        let stretch = window.find(("vocal-stretch", 0_usize)).bounds();
        let at = |fraction: f32| waveform.left() + waveform.size.width * fraction;
        assert!((stretch.left() - at(expected[0].start)).abs() < px(1.0));
        assert!((stretch.right() - at(expected[0].end)).abs() < px(1.0));
        assert!(window.try_find(("vocal-stretch", 1_usize)).is_none());
        assert!(window.try_find("vocals-separate").is_none());
    })
    .unwrap();
}

#[gpui::test]
fn a_track_without_stems_has_an_empty_vocals_lane_that_offers_to_separate_them(
    cx: &mut TestAppContext,
) {
    let (window, view) =
        open_track_changing_key(ThemeName::Slate, Panels::default(), USUAL_WINDOW, cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.stems = crate::stems::StemSession::new(fails_to_separate);
        })
    });
    show_vocal_lane_of_selected(&view, crate::track_overview::VocalLane::NoStems, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(window.try_find("vocals-lane").is_some());
        assert!(window.try_find(("vocal-stretch", 0_usize)).is_none());
        let prompt = window.find("vocals-separate").bounds();
        let lane = window.find("vocals-lane").bounds();
        assert!(prompt.left() >= lane.left() && prompt.right() <= lane.right());
    })
    .unwrap();
    cx.update(|cx| {
        let app = view.read(cx);
        let path = app.selected_track().unwrap().path.clone();
        assert_eq!(app.stems.status(&path), None, "nothing starts by itself");
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("vocals-separate", cx);
    })
    .unwrap();
    cx.update(|cx| {
        let app = view.read(cx);
        let path = app.selected_track().unwrap().path.clone();
        assert_eq!(
            app.stems.status(&path),
            Some(&crate::stems::StemStatus::Separating)
        );
    });
    assert!(!is_shown(window, "vocals-separate", cx));
    assert!(is_shown(window, "vocals-separating", cx));
    assert!(is_shown(window, "vocals-lane", cx));
}

pub(crate) fn write_stems_with_vocals_for(
    folder: &ScratchFolder,
    seconds: usize,
    sounding_seconds: usize,
) -> euphoria_core::stems::StemFiles {
    let rate = euphoria_core::stems::STEMS_SAMPLE_RATE as usize;
    let files = euphoria_core::stems::StemFiles::in_directory(folder.0.join("stems"));
    std::fs::create_dir_all(files.directory()).unwrap();
    for stem in euphoria_core::stems::Stem::ALL {
        let frames = seconds * rate;
        let data_bytes = frames * 4;
        let mut bytes = Vec::with_capacity(44 + data_bytes);
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_bytes as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&(rate as u32).to_le_bytes());
        bytes.extend_from_slice(&(rate as u32 * 4).to_le_bytes());
        bytes.extend_from_slice(&4_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data_bytes as u32).to_le_bytes());
        for frame in 0..frames {
            let sounds =
                stem == euphoria_core::stems::Stem::Vocals && frame < sounding_seconds * rate;
            let sample = if sounds {
                let time = frame as f32 / rate as f32;
                ((std::f32::consts::TAU * 220.0 * time).sin() * 9_000.0) as i16
            } else {
                0
            };
            bytes.extend_from_slice(&sample.to_le_bytes());
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(files.path(stem), bytes).unwrap();
    }
    files
}

#[gpui::test]
fn the_lane_fills_in_once_the_stems_exist(cx: &mut TestAppContext) {
    let folder = ScratchFolder::new("vocal-lane-after-stems");
    let (window, view) = open_fixture_library(cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.show_waveform(waveform_with_energy());
        })
    });
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let path = app.selected_track().unwrap().path.clone();
            app.load_vocal_lane(path, cx);
        })
    });
    cx.run_until_parked();
    assert!(is_shown(window, "vocals-separate", cx));
    assert!(!is_shown(window, ("vocal-stretch", 0_usize), cx));

    let files = write_stems_with_vocals_for(&folder, 16, 6);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let path = app.selected_track().unwrap().path.clone();
            app.stems.mark_ready(&path, files.clone());
            app.load_vocal_lane(path, cx);
        })
    });
    cx.run_until_parked();
    assert!(is_shown(window, ("vocal-stretch", 0_usize), cx));
    assert!(!is_shown(window, "vocals-separate", cx));
    let kept = folder
        .0
        .join("stems")
        .join(euphoria_core::stems::VOCAL_ACTIVITY_FILE_NAME);
    assert!(kept.is_file(), "measured once and kept with the stems");

    cx.update(|cx| view.update(cx, |app, cx| app.remove_stems(cx)));
    assert!(!is_shown(window, ("vocal-stretch", 0_usize), cx));
    assert!(is_shown(window, "vocals-separate", cx));
}

#[gpui::test]
fn a_vocals_stem_that_cannot_be_read_is_said_quietly(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.show_waveform(waveform_with_energy());
        })
    });
    show_vocal_lane_of_selected(
        &view,
        crate::track_overview::VocalLane::Unavailable("the stem is gone".to_string()),
        cx,
    );
    assert!(is_shown(window, "vocals-lane", cx));
    assert!(!is_shown(window, "vocals-separate", cx));
}

#[gpui::test]
fn hovering_the_strips_shows_their_tooltips_without_disturbing_the_layout(cx: &mut TestAppContext) {
    let (window, view) =
        open_track_changing_key(ThemeName::Slate, Panels::default(), USUAL_WINDOW, cx);
    show_vocal_lane_of_selected(&view, vocal_windows_active_between(40.0, 80.0), cx);
    for id in [
        "energy-curve",
        "vocals-lane",
        "energy-label",
        "vocals-label",
    ] {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let before = window.find("energy-curve").bounds();
            window.hover(id, cx);
            window.render_frame(cx);
            assert_eq!(window.find("energy-curve").bounds(), before, "{id}");
        })
        .unwrap();
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(2));
        cx.update_window(window, |_, window, cx| window.render_frame(cx))
            .unwrap();
    }
}

#[gpui::test]
fn the_decks_draw_the_energy_strip_and_the_vocals_lane_under_each_waveform(
    cx: &mut TestAppContext,
) {
    use crate::deck_audio::DeckId;
    use crate::views::decks::deck_part;
    let (window, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    click(window, "toggle-decks", cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    click(window, track(flac), cx);
    click(window, deck_part("deck-load", DeckId::A), cx);
    cx.update(|cx| view.update(cx, |app, _| app.show_waveform(waveform_with_energy())));
    show_vocal_lane_of_selected(&view, vocal_windows_active_between(0.0, 8.0), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let deck_bounds = window.find(deck_part("deck", DeckId::A)).bounds();
        let deck = window.within(deck_part("deck", DeckId::A));
        let waveform = deck.find(deck_part("deck-waveform", DeckId::A)).bounds();
        let energy = deck.find("energy-curve").bounds();
        let vocals = deck.find("vocals-lane").bounds();
        assert!(deck.try_find("energy-label").is_some());
        assert!(deck.try_find("vocals-label").is_some());
        assert!(energy.top() >= waveform.bottom());
        assert!(vocals.top() >= energy.bottom());
        assert_eq!(vocals.left(), waveform.left());
        assert!(vocals.bottom() <= deck_bounds.bottom());
    })
    .unwrap();
}

#[gpui::test]
fn a_track_in_one_key_has_no_timeline_and_no_curve_shows_before_it_is_worked_out(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_fixture_library(cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.show_waveform(Waveform::Ready(
                vec![crate::peaks::BandPeaks {
                    low: 0.5,
                    mid: 0.5,
                    high: 0.5,
                }]
                .into(),
                None,
            ));
        });
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("waveform").is_some());
        assert!(window.try_find("energy-curve").is_none());
        assert!(window.try_find("key-timeline").is_none());
    })
    .unwrap();
}

#[gpui::test]
fn a_curve_the_core_could_not_work_out_says_so_quietly_and_an_empty_one_shows_nothing(
    cx: &mut TestAppContext,
) {
    let columns = || -> std::sync::Arc<[crate::peaks::BandPeaks]> {
        vec![crate::peaks::BandPeaks {
            low: 0.5,
            mid: 0.5,
            high: 0.5,
        }]
        .into()
    };
    let (window, view) = open_fixture_library(cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.show_waveform(Waveform::Ready(
                columns(),
                Some(EnergyCurve::Unavailable(
                    "a sample rate of 4000 Hz is not supported".to_string(),
                )),
            ));
        });
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let waveform = window.find("waveform").bounds();
        let unavailable = window.find("energy-unavailable").bounds();
        assert!(unavailable.top() >= waveform.bottom());
        assert!(window.try_find("energy-curve").is_none());
    })
    .unwrap();

    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.show_waveform(Waveform::Ready(
                columns(),
                Some(EnergyCurve::Steps(Vec::new().into())),
            ));
        });
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("energy-unavailable").is_none());
        assert!(window.try_find("energy-curve").is_none());
    })
    .unwrap();
}

#[gpui::test]
fn a_timeline_that_contradicts_the_key_of_the_track_is_not_shown(cx: &mut TestAppContext) {
    let mut track = track_changing_key();
    if let Ok(analysis) = &mut track.outcome {
        analysis.key = Some(euphoria_core::Key::new(3, euphoria_core::Mode::Minor));
    }
    let (window, view) = open_window_on(vec![track], Box::new(SilentOutput), cx);
    cx.update(|cx| view.update(cx, |app, _| app.show_waveform(waveform_with_energy())));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("energy-curve").is_some());
        assert!(window.try_find("key-timeline").is_none());
    })
    .unwrap();
}

#[gpui::test]
fn the_strip_at_its_shortest_still_holds_the_waveform_the_curve_and_the_timeline(
    cx: &mut TestAppContext,
) {
    let shortest = Panels {
        strip_height: 0,
        ..Panels::default()
    };
    for window_size in [USUAL_WINDOW, SMALLEST_WINDOW] {
        let (window, _) = open_track_changing_key(ThemeName::Deck, shortest, window_size, cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            let strip = window.find("inspector").bounds();
            assert_eq!(strip.size.height, px(142.0));
            let inside_bottom = strip.bottom() - px(16.0);
            for part in ["waveform", "energy-curve", "key-timeline", "play-button"] {
                let bounds = window.find(part).bounds();
                assert!(
                    bounds.top() >= strip.top(),
                    "{part}: {bounds:?} in {strip:?}"
                );
                assert!(
                    bounds.bottom() <= inside_bottom + px(0.5),
                    "{part}: {bounds:?} in {strip:?}"
                );
            }
        })
        .unwrap();
    }
}

#[gpui::test]
fn the_curve_is_worked_out_from_the_decoded_audio_after_the_waveform(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
    for _ in 0..20 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(200));
        cx.run_until_parked();
    }
    cx.update(|cx| match view.read(cx).selected_waveform() {
        Some(Waveform::Ready(columns, Some(_))) => assert!(!columns.is_empty()),
        other => panic!("{other:?}"),
    });
}

#[gpui::test]
fn every_theme_shows_the_wheel_in_the_inspector_without_scrolling(cx: &mut TestAppContext) {
    for theme in ThemeName::ALL {
        let (window, _) = open_track_changing_key(theme, Panels::default(), USUAL_WINDOW, cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            let inspector = window.find("inspector").bounds();
            let wheel = window.find("camelot-wheel").bounds();
            assert!(inspector.contains(&wheel.origin), "{theme:?}");
            assert!(inspector.contains(&wheel.bottom_right()), "{theme:?}");
            assert_eq!(wheel.size.height, wheel.size.width, "{theme:?}");
            if AppTheme::named(theme).layout == Layout::InspectorRight {
                assert_eq!(
                    wheel.size.width,
                    px(crate::views::wheel::COLUMN_WHEEL_SIZE),
                    "{theme:?}"
                );
                assert!(
                    wheel.bottom() <= window.find("waveform").bounds().top(),
                    "{theme:?}"
                );
            }
        })
        .unwrap();
    }
}

#[gpui::test]
fn the_deck_strip_holds_a_square_wheel_as_tall_as_its_inside_next_to_the_key(
    cx: &mut TestAppContext,
) {
    use crate::views::wheel::{LARGEST_WHEEL_SIZE, SMALLEST_WHEEL_SIZE};
    for strip_height in [0_u32, 180, 420] {
        let panels = Panels {
            strip_height,
            ..Panels::default()
        };
        let (window, _) = open_track_changing_key(ThemeName::Deck, panels, USUAL_WINDOW, cx);
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            let strip = window.find("inspector").bounds();
            let wheel = window.find("camelot-wheel").bounds();
            let waveform = window.find("waveform").bounds();
            let inner_height = strip.size.height / px(1.0) - 33.0;
            let expected = inner_height.clamp(SMALLEST_WHEEL_SIZE, LARGEST_WHEEL_SIZE);
            assert_eq!(wheel.size.width, px(expected), "{strip_height}");
            assert_eq!(wheel.size.height, wheel.size.width, "{strip_height}");
            assert!(wheel.top() >= strip.top(), "{strip_height}");
            assert!(
                wheel.bottom() <= strip.bottom() - px(16.0) + px(0.5),
                "{wheel:?} in {strip:?}"
            );
            assert!(wheel.right() <= waveform.left() + px(0.5), "{strip_height}");
            assert!(waveform.size.width >= px(250.0), "{waveform:?}");
        })
        .unwrap();
    }
}
