// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The filter bar of the track list, used through the real root view in a
//! headless window with the made up demo library.

use gpui::{AnyWindowHandle, Entity, TestAppContext, px, size};

use crate::app::EuphoriaApp;
use crate::demo;
use crate::playback::testing::SilentOutput;
use crate::settings::Settings;
use crate::tag_writing::testing::ScratchFolder;
use crate::track_filters::TrackFilters;
use crate::ui_tests::{click, is_shown, open_window_on, open_window_with, type_into};

const TRACKS: usize = 60;

fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_window_on(demo::tracks(TRACKS), Box::new(SilentOutput), cx)
}

fn shown_tempos(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<f32> {
    cx.update(|cx| {
        let app = view.read(cx);
        app.visible
            .iter()
            .filter_map(|index| app.session.library().tracks()[*index].bpm())
            .collect()
    })
}

fn shown_count(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> usize {
    cx.update(|cx| view.read(cx).visible.len())
}

#[gpui::test]
fn nothing_but_the_filter_button_shows_until_a_filter_is_set(cx: &mut TestAppContext) {
    let (window, _) = open(cx);
    assert!(is_shown(window, "filter-button", cx));
    assert!(!is_shown(window, "filter-bar", cx));
    click(window, "filter-button", cx);
    assert!(is_shown(window, "filter-bar", cx));
    assert!(!is_shown(window, "filter-clear", cx));
    click(window, "filter-button", cx);
    assert!(!is_shown(window, "filter-bar", cx));
}

#[gpui::test]
fn typing_a_tempo_range_narrows_the_list_as_you_type(cx: &mut TestAppContext) {
    let (window, view) = open(cx);
    let everything = shown_count(&view, cx);
    assert_eq!(everything, TRACKS);
    click(window, "filter-button", cx);

    type_into(window, ("filter-bpm", 0usize), "120", cx);
    let from_120 = shown_tempos(&view, cx);
    assert!(from_120.len() < everything);
    assert!(from_120.iter().all(|bpm| *bpm >= 120.0));

    type_into(window, ("filter-bpm", 1usize), "125,5", cx);
    let between = shown_tempos(&view, cx);
    assert!(!between.is_empty() && between.len() < from_120.len());
    assert!(between.iter().all(|bpm| (120.0..=125.5).contains(bpm)));
}

#[gpui::test]
fn the_count_says_n_of_m_while_a_filter_is_set_and_clear_resets_everything(
    cx: &mut TestAppContext,
) {
    let (window, view) = open(cx);
    click(window, "filter-button", cx);
    type_into(window, ("filter-energy", 0usize), "9", cx);
    let shown = shown_count(&view, cx);
    assert!(shown > 0 && shown < TRACKS);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(
            crate::format::track_summary_filtered(shown, TRACKS, 0),
            format!("{shown} of {TRACKS} tracks")
        );
        assert!(app.track_filters.is_active());
    });
    assert!(is_shown(window, "filter-clear", cx));

    click(window, "filter-clear", cx);
    assert_eq!(shown_count(&view, cx), TRACKS);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.track_filters, TrackFilters::default());
        assert_eq!(app.track_filter_inputs.energy_from.read(cx).value(), "");
    });
    assert!(!is_shown(window, "filter-clear", cx));
}

#[gpui::test]
fn the_half_double_chip_widens_the_tempo_range(cx: &mut TestAppContext) {
    let (window, view) = open(cx);
    click(window, "filter-button", cx);
    type_into(window, ("filter-bpm", 0usize), "56", cx);
    type_into(window, ("filter-bpm", 1usize), "62", cx);
    assert_eq!(shown_count(&view, cx), 0, "the demo tempos are 112 and up");
    click(window, "filter-half-double", cx);
    let doubled = shown_tempos(&view, cx);
    assert!(!doubled.is_empty());
    assert!(doubled.iter().all(|bpm| (112.0..=124.0).contains(bpm)));
}

#[gpui::test]
fn the_filters_are_saved_and_come_back_with_the_bar_open(cx: &mut TestAppContext) {
    let folder = ScratchFolder::new("track-filters");
    let settings_path = folder.0.join("settings.json");
    let (window, view) = open_window_with(
        demo::tracks(TRACKS),
        Box::new(SilentOutput),
        Settings::default(),
        Some(settings_path.clone()),
        size(px(1440.0), px(900.0)),
        cx,
    );
    click(window, "filter-button", cx);
    type_into(window, ("filter-bpm", 0usize), "120", cx);
    click(window, "filter-half-double", cx);
    let saved = Settings::load(&settings_path).unwrap().track_filters;
    assert_eq!(saved.bpm_min, Some(120.0));
    assert!(saved.half_or_double);
    let narrowed = shown_count(&view, cx);

    let (window, view) = open_window_with(
        demo::tracks(TRACKS),
        Box::new(SilentOutput),
        Settings {
            track_filters: saved,
            ..Settings::default()
        },
        None,
        size(px(1440.0), px(900.0)),
        cx,
    );
    assert!(is_shown(window, "filter-bar", cx));
    assert_eq!(shown_count(&view, cx), narrowed);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.track_filter_inputs.bpm_from.read(cx).value(), "120.0");
    });
}
