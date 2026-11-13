// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The set builder page used through the real root view in a headless
//! window.

use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::set_builder::{DEFAULT_TRACK_COUNT, SetPool};
use crate::ui_tests::{click, is_shown, open_library};

fn open_builder(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let (window, view) = open_library(300, cx);
    click(window, "set-builder-entry", cx);
    (window, view)
}

fn planned_paths(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<std::path::PathBuf> {
    cx.update(|cx| {
        view.read(cx)
            .set_builder
            .planned
            .as_ref()
            .map(|planned| planned.paths.clone())
            .unwrap_or_default()
    })
}

#[gpui::test]
fn the_sidebar_opens_the_set_builder_and_the_library_closes_it(cx: &mut TestAppContext) {
    let (window, view) = open_builder(cx);
    assert!(is_shown(window, "set-builder", cx));
    assert!(is_shown(window, "set-curve", cx));
    assert!(!is_shown(window, "set-summary", cx));
    cx.update(|cx| assert_eq!(view.read(cx).set_builder.pool, SetPool::AllTracks));

    click(window, "all-tracks", cx);
    assert!(!is_shown(window, "set-builder", cx));
    cx.update(|cx| assert!(!view.read(cx).set_builder.is_open));
}

#[gpui::test]
fn building_fills_the_set_with_tracks_that_each_appear_once(cx: &mut TestAppContext) {
    let (window, view) = open_builder(cx);
    click(window, "build-set", cx);
    let paths = planned_paths(&view, cx);
    assert_eq!(paths.len(), DEFAULT_TRACK_COUNT);
    let mut unique = paths.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), paths.len());
    assert!(is_shown(window, "set-summary", cx));
    assert!(is_shown(window, ("set-row", 0usize), cx));
    cx.update(|cx| {
        let app = view.read(cx);
        let planned = app.set_builder.planned.as_ref().unwrap();
        assert_eq!(planned.plan.tracks.len(), DEFAULT_TRACK_COUNT);
        assert_eq!(planned.plan.transitions.len(), DEFAULT_TRACK_COUNT - 1);
        assert!(app.set_builder.problem.is_none());
    });
}

#[gpui::test]
fn the_length_buttons_change_the_track_count(cx: &mut TestAppContext) {
    let (window, view) = open_builder(cx);
    click(window, "set-longer", cx);
    click(window, "set-longer", cx);
    click(window, "set-shorter", cx);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).set_builder.track_count,
            DEFAULT_TRACK_COUNT + 1
        )
    });
    click(window, "build-set", cx);
    assert_eq!(planned_paths(&view, cx).len(), DEFAULT_TRACK_COUNT + 1);
}

#[gpui::test]
fn dragging_across_the_chart_draws_the_curve(cx: &mut TestAppContext) {
    let (window, view) = open_builder(cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let bounds = window.find("set-curve").bounds();
        let height = bounds.size.height;
        let from = gpui::point(
            bounds.left() + bounds.size.width * 0.1,
            bounds.top() + height * 0.05,
        );
        let to = gpui::point(
            bounds.left() + bounds.size.width * 0.9,
            bounds.top() + height * 0.05,
        );
        window.drag(from, to, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        let builder = &view.read(cx).set_builder;
        assert!(builder.preset.is_none());
        let middle = builder.curve[builder.curve.len() / 2];
        assert!(middle > 9.0, "{middle}");
        assert!(builder.stroke.is_none());
    });
}

#[gpui::test]
fn a_locked_track_stays_in_place_when_the_set_is_built_again(cx: &mut TestAppContext) {
    let (window, view) = open_builder(cx);
    click(window, "build-set", cx);
    let first = planned_paths(&view, cx);
    click(window, ("set-lock", 4usize), cx);
    click(window, ("set-preset", 0usize), cx);
    click(window, "build-set", cx);
    let second = planned_paths(&view, cx);
    assert_eq!(second[4], first[4]);
    cx.update(|cx| {
        let planned = view.read(cx).set_builder.planned.clone().unwrap();
        assert!(planned.locked[4]);
        assert!(planned.plan.tracks[4].pinned);
        assert_eq!(planned.locked.iter().filter(|locked| **locked).count(), 1);
    });
}

#[gpui::test]
fn moving_a_track_swaps_it_with_its_neighbour_and_explains_the_set_again(cx: &mut TestAppContext) {
    let (window, view) = open_builder(cx);
    click(window, "build-set", cx);
    let before = planned_paths(&view, cx);
    click(window, ("set-later", 2usize), cx);
    let after = planned_paths(&view, cx);
    assert_eq!(after[2], before[3]);
    assert_eq!(after[3], before[2]);
    cx.update(|cx| {
        let app = view.read(cx);
        let planned = app.set_builder.planned.as_ref().unwrap();
        let library = app.session.library();
        let energy_at = |position: usize| {
            let track = library
                .track(library.index_of(&planned.paths[position]).unwrap())
                .unwrap();
            track.energy().map(f32::from)
        };
        assert_eq!(Some(planned.plan.tracks[2].energy), energy_at(2));
        assert_eq!(Some(planned.plan.tracks[3].energy), energy_at(3));
    });

    click(window, ("set-remove", 0usize), cx);
    assert_eq!(planned_paths(&view, cx).len(), before.len() - 1);
}

#[gpui::test]
fn saving_makes_a_playlist_in_the_order_of_the_set(cx: &mut TestAppContext) {
    let (window, view) = open_builder(cx);
    click(window, "build-set", cx);
    let paths = planned_paths(&view, cx);
    click(window, "save-set", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        let id = app.set_builder.saved_as.unwrap();
        let playlist = app.session.playlists().get(id).unwrap();
        assert_eq!(playlist.tracks, paths);
        assert!(playlist.name.starts_with("Set: Journey"));
    });
    click(window, "save-set", cx);
    cx.update(|cx| assert_eq!(view.read(cx).session.playlists().all().len(), 1));
}
