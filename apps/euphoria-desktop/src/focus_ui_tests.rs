// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests that open the focus view in the real root view, in a headless
//! window, with clicks, keys and the pointer as a person would.

use gpui::test::{ClickOptions, TestEventExt, TestWindowExt};
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext, point, px};

use crate::app::EuphoriaApp;
use crate::demo;
use crate::focus::Lane;
use crate::playback::testing::SilentOutput;
use crate::settings::Settings;
use crate::tag_writing::testing::ScratchFolder;
use crate::ui_tests::{
    click, cued_tracks, is_shown, open_cued_library, open_library, open_window_with, selected_cues,
    track, write_stems_with_vocals_for,
};

fn is_open(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> bool {
    cx.update(|cx| view.read(cx).focus.is_open)
}

fn selected(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Option<usize> {
    cx.update(|cx| view.read(cx).selected)
}

fn press(window: AnyWindowHandle, key: &str, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press(key, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn settle(window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn open_focus(window: AnyWindowHandle, cx: &mut TestAppContext) {
    click(window, "focus-open", cx);
    settle(window, cx);
}

fn move_pointer_over(
    window: AnyWindowHandle,
    id: &'static str,
    fraction: f32,
    cx: &mut TestAppContext,
) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let bounds = window.find(id).bounds();
        let target = point(
            bounds.left() + bounds.size.width * fraction,
            bounds.top() + bounds.size.height / 2.0,
        );
        window.pointer_move(target, None, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn readout(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Option<String> {
    cx.update(|cx| view.read(cx).focus_readout())
}

#[gpui::test]
fn the_focus_button_opens_the_view_and_the_library_button_returns_with_the_selection(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_library(10, cx);
    click(window, track(3), cx);
    assert!(!is_open(&view, cx));

    open_focus(window, cx);
    assert!(is_open(&view, cx));
    assert!(is_shown(window, "focus-view", cx));
    assert!(!is_shown(window, track(3), cx), "the table is replaced");
    assert!(!is_shown(window, "focus-open", cx), "so is the inspector");

    click(window, "focus-back", cx);
    assert!(!is_open(&view, cx));
    assert_eq!(selected(&view, cx), Some(3));
    assert!(is_shown(window, track(3), cx));
}

#[gpui::test]
fn the_focus_view_shows_the_wheel_of_the_selected_tracks_key_in_every_theme(
    cx: &mut TestAppContext,
) {
    for theme in crate::theme::ThemeName::ALL {
        let (window, _) = open_window_with(
            demo::tracks(10),
            Box::new(SilentOutput),
            Settings {
                theme,
                ..Settings::default()
            },
            None,
            gpui::size(px(1280.0), px(800.0)),
            cx,
        );
        click(window, track(3), cx);
        open_focus(window, cx);
        cx.update_window(window, |_, window, _| {
            let focus = window.find("focus-view").bounds();
            let wheel = window.find("camelot-wheel").bounds();
            assert!(focus.contains(&wheel.center()), "{theme:?}");
            assert_eq!(wheel.size.width, wheel.size.height, "{theme:?}");
            assert!(wheel.size.width >= px(76.0), "{theme:?}");
        })
        .unwrap();
    }
}

#[gpui::test]
fn a_double_click_on_a_row_plays_it_and_does_not_open_the_view(cx: &mut TestAppContext) {
    let (window, view) = open_library(10, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.double_click(track(2), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert!(!is_open(&view, cx));
    assert_eq!(selected(&view, cx), Some(2));
}

#[gpui::test]
fn enter_opens_the_view_escape_closes_it_and_f_still_stars(cx: &mut TestAppContext) {
    let (window, view) = open_library(10, cx);
    click(window, track(4), cx);
    press(window, "enter", cx);
    assert!(is_open(&view, cx));

    press(window, "f", cx);
    assert!(is_open(&view, cx), "F stars and does not leave the view");
    let starred = cx.update(|cx| view.read(cx).session.favorites().tracks().len());
    assert_eq!(starred, 1);

    press(window, "escape", cx);
    assert!(!is_open(&view, cx));
    assert_eq!(selected(&view, cx), Some(4));
}

#[gpui::test]
fn enter_leaves_the_set_builder_and_open_settings_alone(cx: &mut TestAppContext) {
    let (window, view) = open_library(10, cx);
    click(window, track(4), cx);
    view.update(cx, |app, cx| {
        app.set_builder.is_open = true;
        cx.notify();
    });
    press(window, "enter", cx);
    assert!(!is_open(&view, cx), "Enter does not cover the set builder");

    view.update(cx, |app, cx| {
        app.set_builder.is_open = false;
        app.settings_open = true;
        cx.notify();
    });
    press(window, "enter", cx);
    assert!(
        !is_open(&view, cx),
        "Enter does not open the view behind settings"
    );
}

#[gpui::test]
fn escape_closes_the_lanes_menu_before_the_view(cx: &mut TestAppContext) {
    let (window, view) = open_library(10, cx);
    click(window, track(4), cx);
    press(window, "enter", cx);
    view.update(cx, |app, cx| {
        app.focus.lanes_menu_open = true;
        cx.notify();
    });
    press(window, "escape", cx);
    assert!(is_open(&view, cx), "the first Escape closes the menu only");
    assert!(!view.read_with(cx, |app, _| app.focus.lanes_menu_open));
    press(window, "escape", cx);
    assert!(!is_open(&view, cx));
}

#[gpui::test]
fn every_lane_is_drawn_on_one_axis_with_the_sections_of_the_cues(cx: &mut TestAppContext) {
    let (window, _view) = open_cued_library(cx);
    open_focus(window, cx);
    for id in [
        "focus-overview",
        "focus-sections",
        "focus-detail",
        "focus-energy-slot",
        "focus-stems-empty",
        "focus-stems-separate",
        "focus-chip-key",
        "focus-chip-bpm",
        "focus-chip-energy",
        "focus-transition-card",
    ] {
        assert!(is_shown(window, id, cx), "{id}");
    }
    assert!(
        !is_shown(window, "focus-chip-grid", cx),
        "a detected grid needs no chip"
    );
    assert!(is_shown(window, ("focus-section", 0_usize), cx));
    assert!(is_shown(window, ("focus-section", 1_usize), cx));
    assert!(!is_shown(window, ("focus-section", 2_usize), cx));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let sections = window.find("focus-sections").bounds();
        let detail = window.find("focus-detail").bounds();
        let slot = window.find("focus-energy-slot").bounds();
        assert_eq!(sections.left(), detail.left());
        assert_eq!(sections.size.width, detail.size.width);
        assert_eq!(slot.left(), detail.left());
        assert!(sections.bottom() <= detail.top());
        assert!(detail.bottom() <= slot.top());
    })
    .unwrap();
}

#[gpui::test]
fn the_cursor_reads_out_the_section_and_the_stems_under_it_across_every_lane(
    cx: &mut TestAppContext,
) {
    let scratch = ScratchFolder::new("focus-readout");
    let (window, view) = open_cued_library(cx);
    let files = write_stems_with_vocals_for(&scratch, 16, 6);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let path = app.selected_track().unwrap().path.clone();
            app.stems.mark_ready(&path, files.clone());
            app.load_focus_stems(cx);
        })
    });
    cx.run_until_parked();
    open_focus(window, cx);
    assert!(is_shown(window, "focus-stems", cx));
    assert!(is_shown(window, ("focus-stem-row", 3_usize), cx));
    assert!(!is_shown(window, "focus-stems-separate", cx));
    assert!(!is_shown(window, "focus-cursor", cx));

    move_pointer_over(window, "focus-detail", 0.15, cx);
    assert_eq!(
        readout(&view, cx).as_deref(),
        Some("0:04 \u{b7} Intro \u{b7} vocals \u{b7} drums out \u{b7} bass out \u{b7} other out")
    );
    assert!(is_shown(window, "focus-cursor", cx));

    move_pointer_over(window, "focus-sections", 0.67, cx);
    assert_eq!(readout(&view, cx).as_deref(), Some("0:20 \u{b7} Drop"));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let cursor = window.find("focus-cursor").bounds();
        let detail = window.find("focus-detail").bounds();
        let stems = window.find("focus-stems").bounds();
        assert!(cursor.top() <= detail.top() && cursor.bottom() >= stems.bottom());
    })
    .unwrap();

    cx.update_window(window, |_, window, cx| {
        window.pointer_move(point(px(2.0), px(2.0)), None, cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(readout(&view, cx), None);
}

#[gpui::test]
fn the_readout_omits_the_lanes_that_have_no_data(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    open_focus(window, cx);
    move_pointer_over(window, "focus-detail", 0.15, cx);
    assert_eq!(readout(&view, cx).as_deref(), Some("0:04 \u{b7} Intro"));
}

#[gpui::test]
fn hiding_a_lane_removes_it_and_the_choice_is_kept_for_the_next_launch(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("focus-lanes-setting");
    let settings_path = scratch.0.join("settings.json");
    let (window, view) = open_window_with(
        cued_tracks(),
        Box::new(SilentOutput),
        Settings::default(),
        Some(settings_path.clone()),
        gpui::size(px(1440.0), px(900.0)),
        cx,
    );
    open_focus(window, cx);
    assert!(is_shown(window, "focus-stems", cx) || is_shown(window, "focus-stems-separate", cx));

    click(window, "focus-lanes", cx);
    click(window, ("focus-lane-toggle", Lane::Stems.number()), cx);
    settle(window, cx);
    assert!(!is_shown(window, "focus-stems", cx));
    assert!(!is_shown(window, "focus-stems-separate", cx));
    assert!(is_shown(window, "focus-detail", cx), "the others stay");
    cx.update(|cx| assert!(!view.read(cx).focus.lanes.stems));

    let kept = Settings::load(&settings_path).unwrap();
    assert!(!kept.focus_lanes.stems);
    assert!(kept.focus_lanes.detail);

    let (window, _view) = open_window_with(
        cued_tracks(),
        Box::new(SilentOutput),
        kept,
        None,
        gpui::size(px(1440.0), px(900.0)),
        cx,
    );
    open_focus(window, cx);
    assert!(is_shown(window, "focus-detail", cx));
    assert!(!is_shown(window, "focus-stems", cx));
    assert!(!is_shown(window, "focus-stems-separate", cx));
}

#[gpui::test]
fn the_mixes_into_list_shows_what_the_inspector_lists_in_the_same_order(cx: &mut TestAppContext) {
    let (window, view) = open_library(40, cx);
    let mut chosen = None;
    for index in 0..40 {
        click(window, track(index), cx);
        if cx.update(|cx| !view.read(cx).mixes.is_empty()) {
            chosen = Some(index);
            break;
        }
    }
    assert!(chosen.is_some(), "some demo track has matches");
    let mixes: Vec<usize> = cx.update(|cx| {
        view.read(cx)
            .mixes
            .iter()
            .map(|mix| mix.track_index)
            .collect()
    });

    let order = |prefix: &'static str, window: AnyWindowHandle, cx: &mut TestAppContext| {
        cx.update_window(window, |_, window, cx| {
            window.render_frame(cx);
            let mut found: Vec<(usize, gpui::Pixels)> = mixes
                .iter()
                .filter_map(|index| {
                    window
                        .try_find((prefix, *index))
                        .map(|element| (*index, element.bounds().top()))
                })
                .collect();
            found.sort_by(|first, second| first.1.partial_cmp(&second.1).unwrap());
            found
                .into_iter()
                .map(|(index, _)| index)
                .collect::<Vec<_>>()
        })
        .unwrap()
    };
    open_focus(window, cx);
    let in_focus = order("focus-mix", window, cx);
    assert_eq!(in_focus, mixes, "the list is app.mixes, in order");
    assert!(is_shown(window, ("focus-mix-clash", mixes[0]), cx));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        for index in &mixes {
            let row = window.find(("focus-mix", *index)).bounds();
            assert_eq!(row.size.height, px(32.0), "row {index}");
        }
    })
    .unwrap();

    click(window, ("focus-mix-star", mixes[0]), cx);
    cx.update(|cx| {
        let app = view.read(cx);
        let from = &app.selected_track().unwrap().path;
        let to = &app.session.library().track(mixes[0]).unwrap().path;
        assert!(app.session.favorites().transition(from, to).is_some());
    });

    click(window, ("focus-mix", mixes[1]), cx);
    cx.update(|cx| assert_eq!(view.read(cx).focus.selected_match, Some(mixes[1])));
}

#[gpui::test]
fn dragging_a_cue_flag_in_the_detail_moves_the_cue_onto_a_beat(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    open_focus(window, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let pressed_at = window
            .find(crate::views::cues::marker_id(1))
            .bounds()
            .center();
        let detail = window.find("focus-detail").bounds();
        let released_at = point(detail.left() + detail.size.width * 0.3, pressed_at.y);
        window.drag(pressed_at, released_at, cx);
    })
    .unwrap();
    cx.run_until_parked();
    let (cues, is_edited) = selected_cues(&view, cx);
    assert!(is_edited);
    assert_eq!(cues[1], (9.0, "Drop".to_string(), Some('B')));
    assert_eq!(cues[0].0, 0.0);
    assert!(is_open(&view, cx));
}

#[gpui::test]
fn the_overview_moves_the_window_and_the_zoom_buttons_resize_it(cx: &mut TestAppContext) {
    let (window, view) = open_library(5, cx);
    open_focus(window, cx);
    let duration = cx.update(|cx| {
        f64::from(
            view.read(cx)
                .selected_track()
                .unwrap()
                .duration_seconds()
                .unwrap(),
        )
    });
    let span_before = cx.update(|cx| view.read(cx).focus.viewport.span);
    assert!(span_before < duration);

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let overview = window.find("focus-overview").bounds();
        window.click_with_options(
            "focus-overview",
            ClickOptions::new().with_offset(point(overview.size.width * 0.97, px(10.0))),
            cx,
        );
    })
    .unwrap();
    cx.update(|cx| {
        let viewport = view.read(cx).focus.viewport;
        assert!((viewport.end() - duration).abs() < 1e-6, "{viewport:?}");
        assert_eq!(viewport.span, span_before);
    });

    click(window, "focus-zoom-in", cx);
    let zoomed = cx.update(|cx| view.read(cx).focus.viewport.span);
    assert!(zoomed < span_before);
    click(window, "focus-zoom-out", cx);
    click(window, "focus-zoom-out", cx);
    click(window, "focus-zoom-out", cx);
    cx.update(|cx| assert_eq!(view.read(cx).focus.viewport.span, duration));
}

#[gpui::test]
fn the_plan_tabs_show_the_transition_card_the_mashup_link_and_the_cues(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    open_focus(window, cx);
    assert!(is_shown(window, "focus-transition-card", cx));

    click(window, ("focus-plan-tab", 2_usize), cx);
    assert!(is_shown(window, ("cue-row", 0_usize), cx));
    assert!(!is_shown(window, "focus-transition-card", cx));

    click(window, ("focus-plan-tab", 1_usize), cx);
    assert!(is_shown(window, "focus-open-mashup", cx));
    click(window, "focus-open-mashup", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(!app.focus.is_open);
        assert!(app.decks.is_open);
        assert_eq!(app.decks.mode, crate::decks::DeckMode::Mashup);
    });
}

#[gpui::test]
fn without_stems_one_card_replaces_the_vocals_and_stems_lanes(cx: &mut TestAppContext) {
    let (window, _view) = open_cued_library(cx);
    open_focus(window, cx);
    assert!(is_shown(window, "focus-stems-empty", cx));
    assert!(
        is_shown(window, "focus-stems-separate", cx),
        "with its button"
    );
    assert!(!is_shown(window, "focus-vocals-slot", cx));
    assert!(
        !is_shown(window, "focus-stems", cx),
        "no label column or prompt"
    );
}

fn open_focus_with_stems(
    name: &str,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>, ScratchFolder) {
    let scratch = ScratchFolder::new(name);
    let (window, view) = open_cued_library(cx);
    let files = write_stems_with_vocals_for(&scratch, 16, 6);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let path = app.selected_track().unwrap().path.clone();
            app.stems.mark_ready(&path, files.clone());
            app.load_focus_stems(cx);
        })
    });
    cx.run_until_parked();
    open_focus(window, cx);
    (window, view, scratch)
}

fn gains(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> [f32; 4] {
    cx.update(|cx| {
        let mix = view.read(cx).stems.mix();
        euphoria_core::stems::Stem::ALL.map(|stem| mix.gain(stem as usize))
    })
}

const DRUMS: usize = euphoria_core::stems::Stem::Drums as usize;
const BASS: usize = euphoria_core::stems::Stem::Bass as usize;
const VOCALS: usize = euphoria_core::stems::Stem::Vocals as usize;

#[gpui::test]
fn every_stem_lane_is_labelled_and_the_vocals_lane_is_the_vocals_stem(cx: &mut TestAppContext) {
    let (window, view, _scratch) = open_focus_with_stems("focus-stem-labels", cx);
    for stem in euphoria_core::stems::Stem::ALL {
        assert!(
            is_shown(window, ("focus-stem-label", stem as usize), cx),
            "{stem:?}"
        );
    }
    assert!(!is_shown(window, "vocals-lane", cx), "one Vocals lane");
    assert!(!is_shown(window, "vocals-label", cx));

    let runs = cx.update(|cx| {
        let app = view.read(cx);
        let path = app.selected_track().unwrap().path.clone();
        match app.focus.stems_of(&path) {
            Some(crate::focus::StemsLane::Ready(activity)) => euphoria_core::stems::Stem::ALL
                .map(|stem| crate::views::focus::runs_of(activity.windows_of(stem)).len()),
            other => panic!("{other:?}"),
        }
    });
    assert_eq!(runs[VOCALS], 1);
    assert_eq!(&runs[..VOCALS], &[0, 0, 0]);
    assert!(is_shown(window, ("focus-stem-run", VOCALS * 1000), cx));
    assert!(!is_shown(window, ("focus-stem-run", DRUMS * 1000), cx));
    cx.update_window(window, |_, window, _| {
        let row = window.find(("focus-stem-row", VOCALS)).bounds();
        let run = window.find(("focus-stem-run", VOCALS * 1000)).bounds();
        assert_eq!(run.left(), row.left());
        assert!(run.size.width < row.size.width);
    })
    .unwrap();
}

#[gpui::test]
fn the_energy_lane_sits_on_the_axis_its_tooltip_reads_the_pointer_against(cx: &mut TestAppContext) {
    let (window, view) = open_cued_library(cx);
    let steps: std::sync::Arc<[crate::track_overview::EnergyStep]> =
        std::sync::Arc::from(vec![crate::track_overview::EnergyStep {
            start: 0.0,
            end: 1.0,
            rating: Some(5),
        }]);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.show_waveform(crate::waveforms::Waveform::Ready(
                Vec::new().into(),
                Some(crate::track_overview::EnergyCurve::Steps(steps)),
            ));
            cx.notify();
        })
    });
    open_focus(window, cx);
    assert!(is_shown(window, "energy-curve", cx));
    let axis = cx
        .update(|cx| view.read(cx).focus.axis_bounds.get())
        .expect("the axis was drawn");
    cx.update_window(window, |_, window, _| {
        let energy = window.find("energy-curve").bounds();
        assert_eq!(energy.left(), axis.left());
        assert_eq!(energy.size.width, axis.size.width);
    })
    .unwrap();
}

#[gpui::test]
fn clicking_a_stem_label_solos_it_and_a_second_click_restores_the_full_track(
    cx: &mut TestAppContext,
) {
    let (window, view, _scratch) = open_focus_with_stems("focus-stem-solo", cx);
    assert_eq!(gains(&view, cx), [1.0; 4]);
    assert!(is_shown(window, "focus-stem-control", cx));

    click(window, ("focus-stem-label", BASS), cx);
    let soloed = gains(&view, cx);
    assert!(
        soloed
            .iter()
            .enumerate()
            .all(|(index, gain)| (*gain > 0.99) == (index == BASS))
    );
    cx.update(|cx| {
        assert!(view.read(cx).player.offers_stems() || view.read(cx).stems.picked().is_some())
    });

    click(window, ("focus-stem-label", DRUMS), cx);
    let moved = gains(&view, cx);
    assert!(
        moved
            .iter()
            .enumerate()
            .all(|(index, gain)| (*gain > 0.99) == (index == DRUMS))
    );

    click(window, ("focus-stem-label", DRUMS), cx);
    assert_eq!(gains(&view, cx), [1.0; 4]);
    cx.update(|cx| assert_eq!(view.read(cx).stems.picked(), None));

    click(window, ("focus-stem-label", VOCALS), cx);
    click(window, "focus-stem-full-track", cx);
    assert_eq!(gains(&view, cx), [1.0; 4]);
}

fn secondary_click(
    window: AnyWindowHandle,
    id: impl Into<gpui::ElementId>,
    cx: &mut TestAppContext,
) {
    let id = id.into();
    cx.update_window(window, |_, window, cx| {
        let secondary = gpui::Modifiers::secondary_key(cx);
        window.render_frame(cx);
        window.click_with_modifiers(id, secondary, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn audible(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> [bool; 4] {
    cx.update(|cx| {
        let stems = view.read(cx).stems.audible();
        euphoria_core::stems::Stem::ALL.map(|stem| stems.contains(stem))
    })
}

fn heard(gains: [f32; 4]) -> [bool; 4] {
    gains.map(|gain| gain > 0.99)
}

#[gpui::test]
fn command_clicking_stem_labels_adds_and_removes_them_from_what_plays(cx: &mut TestAppContext) {
    let (window, view, _scratch) = open_focus_with_stems("focus-stem-multi", cx);

    click(window, ("focus-stem-label", VOCALS), cx);
    secondary_click(window, ("focus-stem-label", DRUMS), cx);
    let pair = [true, false, false, true];
    assert_eq!(audible(&view, cx), pair);
    assert_eq!(heard(gains(&view, cx)), pair, "the mix follows the names");
    cx.update(|cx| {
        assert_eq!(
            view.read(cx)
                .stems
                .audible()
                .summary(&[
                    euphoria_core::stems::Stem::Vocals,
                    euphoria_core::stems::Stem::Drums,
                    euphoria_core::stems::Stem::Bass,
                    euphoria_core::stems::Stem::Other,
                ])
                .as_deref(),
            Some("Solo: Vocals + Drums")
        )
    });

    secondary_click(window, ("focus-stem-label", BASS), cx);
    secondary_click(window, ("focus-stem-label", VOCALS), cx);
    let drums_and_bass = [true, true, false, false];
    assert_eq!(audible(&view, cx), drums_and_bass);
    assert_eq!(heard(gains(&view, cx)), drums_and_bass);

    secondary_click(window, ("focus-stem-label", DRUMS), cx);
    secondary_click(window, ("focus-stem-label", BASS), cx);
    assert_eq!(audible(&view, cx), [true; 4], "removing the last plays all");
    assert_eq!(gains(&view, cx), [1.0; 4]);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.stems.picked(), None);
        assert_eq!(app.stems.mode(), crate::stems::StemMode::Full);
    });
}

#[gpui::test]
fn command_clicking_a_stem_while_all_play_mutes_only_it(cx: &mut TestAppContext) {
    let (window, view, _scratch) = open_focus_with_stems("focus-stem-mute", cx);
    secondary_click(window, ("focus-stem-label", DRUMS), cx);
    let without_drums = [false, true, true, true];
    assert_eq!(audible(&view, cx), without_drums);
    assert_eq!(heard(gains(&view, cx)), without_drums);

    secondary_click(window, ("focus-stem-label", VOCALS), cx);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).stems.audible(),
            crate::stems::StemMode::Instrumental
                .stems()
                .toggled(euphoria_core::stems::Stem::Drums)
        )
    });

    secondary_click(window, ("focus-stem-label", DRUMS), cx);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).stems.audible(),
            crate::stems::StemMode::Instrumental.stems(),
            "drums, bass and other is the instrumental choice"
        )
    });

    secondary_click(window, ("focus-stem-label", VOCALS), cx);
    assert_eq!(gains(&view, cx), [1.0; 4]);
}

#[gpui::test]
fn the_quick_choices_show_on_the_stem_names(cx: &mut TestAppContext) {
    let (window, view, _scratch) = open_focus_with_stems("focus-stem-quick", cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.set_stem_mode(crate::stems::StemMode::Acapella, cx)
        })
    });
    cx.run_until_parked();
    assert_eq!(audible(&view, cx), [false, false, false, true]);

    secondary_click(window, ("focus-stem-label", BASS), cx);
    let vocals_and_bass = [false, true, false, true];
    assert_eq!(audible(&view, cx), vocals_and_bass);
    assert_eq!(heard(gains(&view, cx)), vocals_and_bass);

    click(window, ("focus-stem-label", VOCALS), cx);
    assert_eq!(audible(&view, cx), [false, false, false, true]);
    click(window, ("focus-stem-label", VOCALS), cx);
    assert_eq!(gains(&view, cx), [1.0; 4]);
}

#[gpui::test]
fn changing_the_track_returns_to_the_full_track(cx: &mut TestAppContext) {
    let (window, view, _scratch) = open_focus_with_stems("focus-stem-reset", cx);
    click(window, ("focus-stem-label", VOCALS), cx);
    assert!(gains(&view, cx).iter().any(|gain| *gain < 0.01));

    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let other = if app.selected == Some(0) { 1 } else { 0 };
            app.select(other, cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(gains(&view, cx), [1.0; 4]);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.stems.picked(), None);
        assert_eq!(app.stems.mode(), crate::stems::StemMode::Full);
    });
}

fn lanes_menu_is_open(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> bool {
    cx.update(|cx| view.read(cx).focus.lanes_menu_open)
}

fn open_lanes_menu(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let (window, view) = open_library(10, cx);
    click(window, track(3), cx);
    open_focus(window, cx);
    click(window, "focus-lanes", cx);
    settle(window, cx);
    assert!(lanes_menu_is_open(&view, cx));
    (window, view)
}

#[gpui::test]
fn the_layers_menu_is_drawn_over_the_key_wheel_and_the_key_lock(cx: &mut TestAppContext) {
    let (window, view) = open_lanes_menu(cx);
    let lane = Lane::ALL[1];
    let row_id = ("focus-lane-toggle", lane.number());
    let offset = cx
        .update_window(window, |_, window, _| {
            let menu = window.find("focus-lanes-menu").bounds();
            let wheel = window.find("camelot-wheel").bounds();
            let key_lock = window.find("focus-key-lock").bounds();
            assert!(
                menu.intersects(&wheel),
                "the menu opens over the wheel at this size"
            );
            let row = window.find(row_id).bounds();
            let overlap = row.intersect(&key_lock);
            assert!(
                overlap.size.width > px(4.0) && overlap.size.height > px(4.0),
                "a row of the menu lies over the key lock toggle"
            );
            overlap.center() - row.origin
        })
        .unwrap();
    let (key_lock_before, lane_before) = cx.update(|cx| {
        let app = view.read(cx);
        (app.tempo.key_lock, app.focus.lanes.is_shown(lane))
    });

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click_at(row_id, offset, cx);
    })
    .unwrap();
    cx.run_until_parked();

    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(
            app.tempo.key_lock, key_lock_before,
            "the key lock under the menu gets no click"
        );
        assert_ne!(
            app.focus.lanes.is_shown(lane),
            lane_before,
            "the menu row on top takes the click"
        );
        assert!(
            app.focus.lanes_menu_open,
            "toggling a layer keeps the menu open"
        );
    });
}

#[gpui::test]
fn the_layers_menu_closes_on_a_click_outside_and_on_its_button(cx: &mut TestAppContext) {
    let (window, view) = open_lanes_menu(cx);
    click(window, "focus-chip-key", cx);
    assert!(
        !lanes_menu_is_open(&view, cx),
        "a click elsewhere closes it"
    );

    click(window, "focus-lanes", cx);
    assert!(lanes_menu_is_open(&view, cx));
    click(window, "focus-lanes", cx);
    assert!(
        !lanes_menu_is_open(&view, cx),
        "the button closes it instead of closing and opening again"
    );
}

#[gpui::test]
fn the_layers_menu_stays_inside_a_short_window(cx: &mut TestAppContext) {
    let (window, _view) = open_lanes_menu(cx);
    cx.simulate_window_resize(window, gpui::size(px(900.0), px(220.0)));
    settle(window, cx);
    cx.update_window(window, |_, window, _| {
        let viewport = window.viewport_size();
        let menu = window.find("focus-lanes-menu").bounds();
        assert!(menu.left() >= px(0.0) && menu.top() >= px(0.0), "{menu:?}");
        assert!(
            menu.right() <= viewport.width && menu.bottom() <= viewport.height,
            "{menu:?} in {viewport:?}"
        );
    })
    .unwrap();
}

#[gpui::test]
fn opening_the_decks_from_the_focus_view_leaves_it_and_shows_them(cx: &mut TestAppContext) {
    let (window, view) = open_library(10, cx);
    click(window, track(3), cx);
    open_focus(window, cx);
    assert!(is_open(&view, cx));

    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(crate::actions::ToggleDecks), cx);
    })
    .unwrap();
    settle(window, cx);

    assert!(!is_open(&view, cx));
    assert!(cx.update(|cx| view.read(cx).decks.is_open));
    assert!(!is_shown(window, "focus-view", cx));
    assert!(is_shown(
        window,
        crate::views::decks::deck_part("deck-load", crate::deck_audio::DeckId::A),
        cx
    ));
}

#[gpui::test]
fn the_focus_view_stays_when_the_decks_cannot_open(cx: &mut TestAppContext) {
    let (window, view) = open_window_with(
        demo::tracks(10),
        Box::new(crate::playback::testing::NoOutput),
        Settings::default(),
        None,
        gpui::size(px(1280.0), px(800.0)),
        cx,
    );
    click(window, track(3), cx);
    open_focus(window, cx);
    assert!(is_open(&view, cx));

    cx.update_window(window, |_, window, cx| {
        window.dispatch_action(Box::new(crate::actions::ToggleDecks), cx);
    })
    .unwrap();
    settle(window, cx);

    assert!(!cx.update(|cx| view.read(cx).decks.is_open));
    assert!(is_open(&view, cx));
    assert!(is_shown(window, "focus-view", cx));
    assert!(cx.update(|cx| view.read(cx).notice.is_some()));
}

#[gpui::test]
fn the_toolbar_decks_button_leaves_the_focus_view_and_shows_the_decks(cx: &mut TestAppContext) {
    let (window, view) = open_library(10, cx);
    click(window, track(3), cx);
    open_focus(window, cx);
    assert!(is_open(&view, cx));

    click(window, "toggle-decks", cx);
    settle(window, cx);

    assert!(!is_open(&view, cx));
    assert!(cx.update(|cx| view.read(cx).decks.is_open));
    assert!(!is_shown(window, "focus-view", cx));
    assert!(is_shown(
        window,
        crate::views::decks::deck_part("deck-load", crate::deck_audio::DeckId::A),
        cx
    ));
}
