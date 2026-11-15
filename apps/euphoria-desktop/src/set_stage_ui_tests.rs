// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The set stage in the window: the Stage column, the inspector line and the
//! Stage chips of the filter bar, in a headless window with the made up demo
//! library.

use euphoria_core::SetStage;
use gpui::{AnyWindowHandle, Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::columns::ColumnId;
use crate::demo;
use crate::playback::testing::SilentOutput;
use crate::ui_tests::{click, is_shown, open_window_on};

const TRACKS: usize = 60;

fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_window_on(demo::tracks(TRACKS), Box::new(SilentOutput), cx)
}

fn stages_shown(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<Option<SetStage>> {
    cx.update(|cx| {
        let app = view.read(cx);
        app.visible
            .iter()
            .map(|index| app.session.library().stage_at(*index))
            .collect()
    })
}

#[gpui::test]
fn the_demo_library_has_all_three_stages(cx: &mut TestAppContext) {
    let (_, view) = open(cx);
    let stages = stages_shown(&view, cx);
    for stage in SetStage::ALL {
        assert!(stages.contains(&Some(stage)), "{stage:?} in {stages:?}");
    }
}

#[gpui::test]
fn the_stage_column_is_off_until_the_settings_turn_it_on(cx: &mut TestAppContext) {
    let (window, view) = open(cx);
    assert!(is_shown(window, "filter-button", cx));
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(!app.table_layout.shows(ColumnId::Stage));
        assert!(app.column_texts(ColumnId::Stage).len() <= 3);
    });
    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    click(window, "show-stage-column", cx);
    click(window, "close-settings", cx);
    assert!(is_shown(window, "filter-button", cx));
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.display.show_stage_column);
        assert!(app.table_layout.shows(ColumnId::Stage));
        let texts = app.column_texts(ColumnId::Stage);
        assert!(
            texts.contains("Peak") && texts.contains("Warm-up"),
            "{texts:?}"
        );
    });
}

#[gpui::test]
fn the_inspector_says_the_stage_of_the_selected_track(cx: &mut TestAppContext) {
    let (window, view) = open(cx);
    assert!(is_shown(window, "track-stage", cx));
    let selected_stage = cx.update(|cx| {
        let app = view.read(cx);
        let track = app.selected_track().expect("a track is selected");
        let index = app.session.library().index_of(&track.path).unwrap();
        app.session.library().stage_at(index)
    });
    assert!(selected_stage.is_some());
}

#[gpui::test]
fn a_stage_chip_narrows_the_list_to_that_stage_and_a_second_click_widens_it(
    cx: &mut TestAppContext,
) {
    let (window, view) = open(cx);
    click(window, "filter-button", cx);
    assert!(is_shown(window, ("filter-stage", 2_usize), cx));
    click(window, ("filter-stage", 2_usize), cx);
    let peak = stages_shown(&view, cx);
    assert!(!peak.is_empty() && peak.len() < TRACKS);
    assert!(peak.iter().all(|stage| *stage == Some(SetStage::Peak)));

    click(window, ("filter-stage", 0_usize), cx);
    let two = stages_shown(&view, cx);
    assert!(two.len() > peak.len());
    assert!(
        two.iter()
            .all(|stage| matches!(stage, Some(SetStage::Peak | SetStage::WarmUp)))
    );

    click(window, ("filter-stage", 2_usize), cx);
    click(window, ("filter-stage", 0_usize), cx);
    assert_eq!(stages_shown(&view, cx).len(), TRACKS);
    cx.update(|cx| assert!(!view.read(cx).track_filters.is_active()));
}

#[gpui::test]
fn clearing_the_filters_clears_the_chosen_stages(cx: &mut TestAppContext) {
    let (window, view) = open(cx);
    click(window, "filter-button", cx);
    click(window, ("filter-stage", 1_usize), cx);
    assert!(stages_shown(&view, cx).len() < TRACKS);
    click(window, "filter-clear", cx);
    assert_eq!(stages_shown(&view, cx).len(), TRACKS);
    cx.update(|cx| assert!(view.read(cx).track_filters.stages.is_empty()));
}

#[gpui::test]
fn a_corrected_tempo_moves_a_track_to_another_stage(cx: &mut TestAppContext) {
    let (_, view) = open(cx);
    let (path, before) = cx.update(|cx| {
        let app = view.read(cx);
        let library = app.session.library();
        let index = (0..TRACKS)
            .find(|index| library.stage_at(*index) == Some(SetStage::Peak))
            .expect("a peak track");
        (
            library.tracks()[index].path.clone(),
            library.stage_at(index),
        )
    });
    assert_eq!(before, Some(SetStage::Peak));
    let after = cx.update(|cx| {
        view.update(cx, |app, _| {
            let correction = euphoria_core::Correction {
                bpm: Some(60.0),
                ..euphoria_core::Correction::default()
            };
            app.session.set_correction(&path, correction);
        });
        let app = view.read(cx);
        let library = app.session.library();
        library.stage_at(library.index_of(&path).unwrap())
    });
    assert_eq!(after, Some(SetStage::WarmUp));
}
