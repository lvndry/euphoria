// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// 16 tests: the cues column, the markers and the cue edits.

/// Length of the tracks of [`open_cued_library`], in seconds.
pub(crate) const CUED_TRACK_SECONDS: f32 = 32.0;

/// A fixture file standing for a 32 second track at 120 BPM. With a grid,
/// beats fall every half second from 0 and bars every two seconds, and
/// analysis placed a Start cue at 0 and a Drop at 16.
fn cued_track(name: &str, has_grid: bool) -> DemoTrack {
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/euphoria-core/tests/fixtures");
    let section_cue = |seconds, section| euphoria_core::SectionCue {
        seconds,
        section,
        energy: None,
    };
    DemoTrack {
        path: fixtures.join(name),
        outcome: Ok(TrackAnalysis {
            duration_seconds: CUED_TRACK_SECONDS,
            key: None,
            bpm: Some(120.0),
            energy: None,
            tuning_cents: None,
            key_runner_up: None,
            key_score_margin: None,
            key_source: None,
            loudness_range_lu: None,
            pitch_class_profile: None,
            tempo_estimate: Some(120.0),
            tempo_score_margin: None,
            tempo_octave_evidence: None,
            beat_grid: has_grid.then_some(euphoria_core::BeatGrid {
                first_downbeat_seconds: 0.0,
                bpm: 120.0,
                salience: 10.0,
                drift_seconds: Some(0.001),
            }),
            cues: if has_grid {
                vec![
                    section_cue(0.0, euphoria_core::CueSection::Start),
                    section_cue(16.0, euphoria_core::CueSection::Drop),
                ]
            } else {
                Vec::new()
            },
            key_timeline: Vec::new(),
            quality: None,
        }),
    }
}

/// A track with a beat grid and the cues analysis placed on it, selected,
/// and a track without a grid or cues.
pub(crate) fn cued_tracks() -> Vec<DemoTrack> {
    vec![
        cued_track("sine-440hz-stereo.wav", true),
        cued_track("sine-440hz-mono.wav", false),
    ]
}

pub(crate) fn open_cued_library(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_window_on(cued_tracks(), Box::new(SilentOutput), cx)
}

/// The selected track's cues as (seconds, name, hot cue letter), and
/// whether the user set them.
pub(crate) fn selected_cues(
    view: &Entity<EuphoriaApp>,
    cx: &mut TestAppContext,
) -> (Vec<(f64, String, Option<char>)>, bool) {
    cx.update(|cx| {
        let cues = view.read(cx).selected_track().unwrap().cues();
        let listed = cues
            .cues
            .iter()
            .map(|cue| {
                (
                    cue.seconds,
                    cue.name.clone(),
                    cue.hot_cue.map(|hot_cue| hot_cue.letter()),
                )
            })
            .collect();
        (listed, cues.is_edited())
    })
}

#[gpui::test]
fn the_cues_column_counts_edited_cues_else_automatic_ones_and_sorts(cx: &mut TestAppContext) {
    let (window, view) = open_window_with(
        vec![
            cued_track("sine-440hz-stereo.wav", true),
            cued_track("sine-440hz-mono.wav", false),
        ],
        Box::new(SilentOutput),
        Settings::default(),
        None,
        size(px(2400.0), px(900.0)),
        cx,
    );
    let counts = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            let app = view.read(cx);
            app.visible
                .iter()
                .map(|index| app.session.library().track(*index).unwrap().cue_count())
                .collect::<Vec<_>>()
        })
    };
    cx.update(|cx| {
        let app = view.read(cx);
        let texts = app.column_texts(ColumnId::Cues);
        assert!(texts.contains("2") && texts.contains("0"), "{texts:?}");
    });
    click(window, header_id(ColumnId::Cues), cx);
    assert_eq!(counts(cx), [Some(0), Some(2)]);
    click(window, header_id(ColumnId::Cues), cx);
    assert_eq!(counts(cx), [Some(2), Some(0)]);

    cx.update(|cx| {
        view.update(cx, |app, _| {
            let path = app.session.library().tracks()[1].path.clone();
            let cue = |seconds: f64| euphoria_core::Cue {
                seconds,
                name: String::new(),
                hot_cue: None,
                energy: None,
            };
            app.session
                .set_edited_cues(&path, Some(vec![cue(1.0), cue(2.0), cue(3.0), cue(4.0)]));
        })
    });
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.refresh_visible();
            cx.notify();
        })
    });
    assert_eq!(counts(cx), [Some(4), Some(2)]);
}

/// Holds or lets go of Shift, as the keyboard would.
fn hold_shift(window: &mut gpui::Window, is_held: bool, cx: &mut gpui::App) {
    let modifiers = if is_held {
        Modifiers::shift()
    } else {
        Modifiers::default()
    };
    window.dispatch_event(
        gpui::InputEvent::to_platform_input(gpui::ModifiersChangedEvent {
            modifiers,
            capslock: window.capslock(),
        }),
        cx,
    );
    window.render_frame(cx);
}

/// Drags cue marker `index` to `fraction` of the way along the waveform.
fn drag_marker(
    window: AnyWindowHandle,
    index: usize,
    fraction: f32,
    with_shift: bool,
    cx: &mut TestAppContext,
) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let pressed_at = window
            .find(crate::views::cues::marker_id(index))
            .bounds()
            .center();
        let waveform = window.find("waveform").bounds();
        let released_at = gpui::point(
            waveform.left() + waveform.size.width * fraction,
            pressed_at.y,
        );
        hold_shift(window, with_shift, cx);
        window.drag(pressed_at, released_at, cx);
        hold_shift(window, false, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui::test]
fn the_cues_show_as_markers_lined_up_with_the_waveform_with_their_hot_cue_letters(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_cued_library(cx);
    assert_eq!(
        selected_cues(&view, cx),
        (
            vec![
                (0.0, "Start".to_string(), Some('A')),
                (16.0, "Drop".to_string(), Some('B')),
            ],
            false
        )
    );
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let waveform = window.find("waveform").bounds();
        let start = window.find(crate::views::cues::marker_id(0)).bounds();
        let drop = window.find(crate::views::cues::marker_id(1)).bounds();
        assert_eq!(start.left(), waveform.left());
        let middle = waveform.left() + waveform.size.width * 0.5;
        assert!((drop.left() - middle).abs() < px(1.0), "{drop:?}");
        assert!(drop.bottom() <= waveform.top());
        assert!(window.try_find(("cue-row", 1_usize)).is_some());
        assert!(window.try_find(("cue-row", 2_usize)).is_none());
        assert!(window.try_find("reset-cues").is_none());
    })
    .unwrap();
}

#[gpui::test]
fn markers_of_cues_close_together_never_overlap_and_stay_above_the_waveform(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_cued_library(cx);
    let crowded: Vec<euphoria_core::Cue> = euphoria_core::HotCue::ALL
        .iter()
        .enumerate()
        .map(|(index, pad)| euphoria_core::Cue {
            seconds: 1.0 + index as f64 * 0.5,
            name: "Breakdown".to_string(),
            hot_cue: Some(*pad),
            energy: None,
        })
        .chain(std::iter::once(euphoria_core::Cue {
            seconds: 31.5,
            name: "Last".to_string(),
            hot_cue: None,
            energy: None,
        }))
        .collect();
    cx.update(|cx| {
        view.update(cx, |app, _| {
            let path = app.selected_track().unwrap().path.clone();
            app.session.set_edited_cues(&path, Some(crowded.clone()));
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let waveform = window.find("waveform").bounds();
        let lane = window.find("cue-lane").bounds();
        let markers: Vec<Bounds<gpui::Pixels>> = (0..crowded.len())
            .map(|index| window.find(crate::views::cues::marker_id(index)).bounds())
            .collect();
        // Cues a few pixels apart get a bare tick, which carries no label.
        let labelled: Vec<&Bounds<gpui::Pixels>> = markers
            .iter()
            .filter(|marker| marker.size.width > px(4.0))
            .collect();
        assert!(labelled.len() >= 3, "{markers:?}");
        for (index, marker) in markers.iter().enumerate() {
            assert!(marker.bottom() <= waveform.top(), "{index}: {marker:?}");
            assert!(lane.left() <= marker.left() && marker.right() <= lane.right());
        }
        for (index, marker) in labelled.iter().enumerate() {
            for other in &labelled[index + 1..] {
                assert!(!marker.intersects(other), "{marker:?} overlaps {other:?}");
            }
        }
        assert!(
            markers.iter().any(|marker| marker.top() > lane.top()),
            "crowded markers use a second row"
        );
    })
    .unwrap();
}

#[gpui::test]
fn a_click_on_a_marker_plays_from_its_cue(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    click(window, crate::views::cues::marker_id(0), cx);
    assert_eq!(player_status(&view, cx), PlayerStatus::Playing);

    click(window, crate::views::cues::marker_id(1), cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.player.position_seconds(), 16.0);
        assert_eq!(app.player.status(), &PlayerStatus::Playing);
    });
    assert!(!selected_cues(&view, cx).1, "a click changes no cue");

    click(window, ("cue-time", 0_usize), cx);
    cx.update(|cx| assert_eq!(view.read(cx).player.position_seconds(), 0.0));
}

#[gpui::test]
fn dragging_a_marker_moves_its_cue_onto_a_beat_or_onto_a_bar_with_shift(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    drag_marker(window, 1, 0.3, false, cx);
    let (cues, is_edited) = selected_cues(&view, cx);
    assert!(is_edited);
    assert_eq!(
        cues[1],
        (9.5, "Drop".to_string(), Some('B')),
        "9.6 s lands on the beat at 9.5"
    );
    assert_eq!(
        cx.update(|cx| view.read(cx).player.status().clone()),
        PlayerStatus::Stopped,
        "a drag does not play"
    );

    drag_marker(window, 1, 0.6, true, cx);
    assert_eq!(
        selected_cues(&view, cx).0[1].0,
        20.0,
        "19.2 s lands on the bar at 20 with Shift held"
    );
}

#[gpui::test]
fn a_double_click_adds_a_cue_on_the_nearest_beat_or_exactly_there_without_a_grid(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_cued_library(cx);
    let double_click_at = |fraction: f32, cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let waveform = window.find("waveform").bounds();
            window.click_with_options(
                "waveform",
                gpui::test::ClickOptions::new()
                    .with_offset(gpui::point(waveform.size.width * fraction, px(10.0)))
                    .with_count(2),
                cx,
            );
        })
        .unwrap();
        cx.run_until_parked();
    };
    double_click_at(0.3, cx);
    let (cues, is_edited) = selected_cues(&view, cx);
    assert!(is_edited);
    assert_eq!(cues[1], (9.5, "Cue 3".to_string(), Some('C')));

    click(window, track(1), cx);
    assert_eq!(selected_cues(&view, cx), (Vec::new(), false));
    double_click_at(0.3, cx);
    let (cues, _) = selected_cues(&view, cx);
    assert_eq!(cues.len(), 1);
    assert!(
        (cues[0].0 - 9.6).abs() < 0.05,
        "no grid, no snapping: {cues:?}"
    );
    assert_ne!(cues[0].0, 9.5);
}

#[gpui::test]
fn a_cue_is_added_at_the_playhead_with_the_button_or_the_c_key(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let waveform = window.find("waveform").bounds();
        window.click_at(
            "waveform",
            gpui::point(waveform.size.width * 0.25, px(10.0)),
            cx,
        );
    })
    .unwrap();
    click(window, "add-cue", cx);
    let (cues, _) = selected_cues(&view, cx);
    assert_eq!(cues[1], (8.0, "Cue 3".to_string(), Some('C')));

    cx.update_window(window, |_, window, cx| window.press("c", cx))
        .unwrap();
    assert_eq!(selected_cues(&view, cx).0.len(), 4);
}

#[gpui::test]
fn a_cue_is_renamed_put_on_another_pad_and_deleted_from_the_list(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    click(window, ("cue-name", 0_usize), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("cue-name-input").is_some());
        window.input("Intro", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(selected_cues(&view, cx).0[0].1, "Intro");

    click(window, ("cue-hot-cue", 1_usize), cx);
    click(window, ("cue-hot-cue-choice", 0_usize), cx);
    let (cues, _) = selected_cues(&view, cx);
    assert_eq!(cues[0].2, Some('B'), "the cue that held A takes B");
    assert_eq!(cues[1].2, Some('A'));

    click(window, ("cue-hot-cue", 0_usize), cx);
    click(window, ("cue-hot-cue-none", 0_usize), cx);
    assert_eq!(selected_cues(&view, cx).0[0].2, None);

    click(window, ("cue-delete", 0_usize), cx);
    assert_eq!(
        selected_cues(&view, cx),
        (vec![(16.0, "Drop".to_string(), Some('A'))], true)
    );
}

#[gpui::test]
fn edits_are_undone_one_at_a_time_and_reset_brings_back_the_automatic_cues(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_cued_library(cx);
    let automatic = selected_cues(&view, cx);
    assert!(!is_shown(window, "undo-cue-edit", cx));
    click(window, ("cue-delete", 1_usize), cx);
    click(window, ("cue-delete", 0_usize), cx);
    assert_eq!(selected_cues(&view, cx), (Vec::new(), true));
    assert!(is_shown(window, "cues-set-by-you", cx));

    click(window, "undo-cue-edit", cx);
    assert_eq!(selected_cues(&view, cx).0, automatic.0[..1].to_vec());
    cx.update_window(window, |_, window, cx| window.press("cmd-z", cx))
        .unwrap();
    assert_eq!(selected_cues(&view, cx), automatic);
    assert!(!is_shown(window, "undo-cue-edit", cx));

    click(window, ("cue-delete", 1_usize), cx);
    click(window, "reset-cues", cx);
    assert_eq!(selected_cues(&view, cx), automatic);
    assert!(!is_shown(window, "reset-cues", cx));
    click(window, "undo-cue-edit", cx);
    assert_eq!(
        selected_cues(&view, cx).0.len(),
        1,
        "the reset is undone too"
    );
}

#[gpui::test]
fn a_corrected_tempo_drops_the_automatic_cues_but_keeps_the_users(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    cx.update(|cx| view.update(cx, |app, cx| app.accept_bpm(60.0, cx)));
    assert_eq!(selected_cues(&view, cx), (Vec::new(), false));
    assert!(!is_shown(window, crate::views::cues::marker_id(0), cx));

    cx.update(|cx| view.update(cx, |app, cx| app.reset_bpm(cx)));
    click(window, ("cue-delete", 0_usize), cx);
    cx.update(|cx| view.update(cx, |app, cx| app.accept_bpm(60.0, cx)));
    assert_eq!(
        selected_cues(&view, cx),
        (vec![(16.0, "Drop".to_string(), Some('B'))], true)
    );
}

#[gpui::test]
fn a_cue_name_commits_when_selecting_another_track(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    click(window, ("cue-name", 0_usize), cx);
    cx.update_window(window, |_, window, cx| window.input("Intro", cx))
        .unwrap();
    click(window, track(1), cx);
    click(window, track(0), cx);
    assert_eq!(selected_cues(&view, cx).0[0].1, "Intro");
}

#[gpui::test]
fn a_cue_command_keeps_a_name_typed_before_blurring(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    click(window, ("cue-name", 0_usize), cx);
    cx.update_window(window, |_, window, cx| window.input("Intro", cx))
        .unwrap();
    click(window, "add-cue", cx);
    assert_eq!(selected_cues(&view, cx).0[0].1, "Intro");
    click(window, "undo-cue-edit", cx);
    assert_eq!(selected_cues(&view, cx).0[0].1, "Intro");
    click(window, "undo-cue-edit", cx);
    assert_eq!(selected_cues(&view, cx).0[0].1, "Start");
}

#[gpui::test]
fn measuring_a_cue_lane_requests_a_frame_to_finish_its_layout(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    let crowded: Vec<_> = euphoria_core::HotCue::ALL
        .iter()
        .enumerate()
        .map(|(index, pad)| euphoria_core::Cue {
            seconds: 1.0 + index as f64 * 0.5,
            name: "Breakdown".to_string(),
            hot_cue: Some(*pad),
            energy: None,
        })
        .collect();
    cx.update(|cx| {
        view.update(cx, |app, _| {
            let path = app.selected_track().unwrap().path.clone();
            app.session.set_edited_cues(&path, Some(crowded));
            app.waveform_bounds.set(None);
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.simulate_next_frame(cx);
        window.render_frame(cx);
        assert!(
            window.simulate_next_frame(cx) > 0,
            "measuring the waveform must schedule layout with the measured width"
        );
        window.render_frame(cx);
        assert_eq!(
            window.simulate_next_frame(cx),
            0,
            "a stable width needs no redraw loop"
        );
        let first = window.find(crate::views::cues::marker_id(0)).bounds();
        let second = window.find(crate::views::cues::marker_id(1)).bounds();
        assert!(!first.intersects(&second));
    })
    .unwrap();
    for width in [600, 288] {
        cx.update(|cx| {
            view.update(cx, |app, cx| {
                app.panels.inspector_width = width;
                cx.notify();
            })
        });
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            assert!(
                window.simulate_next_frame(cx) > 0,
                "a resized waveform must finish its cue layout"
            );
            window.render_frame(cx);
            assert_eq!(window.simulate_next_frame(cx), 0);
            let markers: Vec<_> = (0..8)
                .map(|index| window.find(crate::views::cues::marker_id(index)).bounds())
                .collect();
            for (index, marker) in markers
                .iter()
                .enumerate()
                .filter(|(_, marker)| marker.size.width > px(4.0))
            {
                for other in markers[index + 1..]
                    .iter()
                    .filter(|other| other.size.width > px(4.0))
                {
                    assert!(!marker.intersects(other), "{marker:?} overlaps {other:?}");
                }
            }
        })
        .unwrap();
    }
}

#[gpui::test]
fn saved_cues_wait_for_analysis_duration_before_drawing_markers(cx: &mut TestAppContext) {
    let mut track = cued_track("sine-440hz-mono.wav", false);
    track.outcome = Err("Analysis is not available".to_string());
    let (window, view) = open_window_on(vec![track], Box::new(SilentOutput), cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            let path = app.selected_track().unwrap().path.clone();
            app.session.set_edited_cues(
                &path,
                Some(vec![euphoria_core::Cue {
                    seconds: 1.0,
                    name: "Saved cue".to_string(),
                    hot_cue: euphoria_core::HotCue::from_letter('A'),
                    energy: None,
                }]),
            );
        })
    });
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        assert!(window.try_find(crate::views::cues::marker_id(0)).is_none());
    })
    .unwrap();
}
