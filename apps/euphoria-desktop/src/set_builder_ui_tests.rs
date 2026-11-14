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

fn playable_tracks() -> Vec<crate::demo::DemoTrack> {
    crate::ui_tests::deck_tracks()
        .into_iter()
        .enumerate()
        .map(|(number, mut track)| {
            if let Ok(analysis) = &mut track.outcome {
                analysis.duration_seconds = 20.5;
                analysis.key = Some(euphoria_core::Key::new(9, euphoria_core::Mode::Minor));
                analysis.bpm = Some(120.0);
                analysis.energy = Some(5 + number as u8);
            }
            track
        })
        .collect()
}

fn open_playable_set(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let (window, view) = crate::ui_tests::open_window_on(
        playable_tracks(),
        Box::new(crate::playback::testing::SilentOutput),
        cx,
    );
    click(window, "set-builder-entry", cx);
    click(window, "build-set", cx);
    assert_eq!(planned_paths(&view, cx).len(), 3);
    (window, view)
}

fn click_play(window: AnyWindowHandle, position: usize, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.hover(("set-row", position), cx);
        window.render_frame(cx);
        window.click(("set-play", position), cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn held_track(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Option<std::path::PathBuf> {
    cx.update(|cx| {
        view.read(cx)
            .player
            .track()
            .map(std::path::Path::to_path_buf)
    })
}

fn sound_position(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> f32 {
    cx.update(|cx| {
        view.read(cx)
            .voice
            .as_ref()
            .expect("a track is sounding")
            .state
            .position_seconds()
    })
}

fn is_sounding(view: &Entity<EuphoriaApp>, position: usize, cx: &mut TestAppContext) -> bool {
    cx.update(|cx| view.read(cx).is_set_row_sounding(position))
}

#[gpui::test]
fn a_rows_play_button_plays_its_track_from_the_start(cx: &mut TestAppContext) {
    let (window, view) = open_playable_set(cx);
    let paths = planned_paths(&view, cx);
    click_play(window, 1, cx);
    assert_eq!(held_track(&view, cx), Some(paths[1].clone()));
    assert!(is_sounding(&view, 1, cx));
    assert!(!is_sounding(&view, 0, cx));
    assert_eq!(sound_position(&view, cx), 0.0);

    click_play(window, 2, cx);
    assert_eq!(held_track(&view, cx), Some(paths[2].clone()));
    assert!(is_sounding(&view, 2, cx));
    assert!(!is_sounding(&view, 1, cx));
}

#[gpui::test]
fn double_clicking_a_row_plays_it_and_space_plays_the_selected_row(cx: &mut TestAppContext) {
    let (window, view) = open_playable_set(cx);
    let paths = planned_paths(&view, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.double_click(("set-row-title", 2usize), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(held_track(&view, cx), Some(paths[2].clone()));
    assert!(is_sounding(&view, 2, cx));

    cx.update(|cx| view.update(cx, |app, cx| app.select_set_row(0, cx)));
    assert_eq!(held_track(&view, cx), Some(paths[2].clone()));
    cx.update(|cx| view.update(cx, |app, cx| app.toggle_playback(cx)));
    cx.run_until_parked();
    assert_eq!(held_track(&view, cx), Some(paths[0].clone()));
    assert!(is_sounding(&view, 0, cx));

    cx.update(|cx| view.update(cx, |app, cx| app.toggle_playback(cx)));
    assert!(!is_sounding(&view, 0, cx));
    assert_eq!(held_track(&view, cx), Some(paths[0].clone()));
}

#[gpui::test]
fn a_row_and_the_bottom_bar_pause_and_resume_the_same_track(cx: &mut TestAppContext) {
    let (window, view) = open_playable_set(cx);
    click_play(window, 0, cx);
    assert!(is_sounding(&view, 0, cx));

    click_play(window, 0, cx);
    assert!(!is_sounding(&view, 0, cx));
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).player.status(),
            &crate::player::PlayerStatus::Paused
        )
    });

    click(window, "play-button", cx);
    assert!(is_sounding(&view, 0, cx));
    click(window, "play-button", cx);
    assert!(!is_sounding(&view, 0, cx));
    click_play(window, 0, cx);
    assert!(is_sounding(&view, 0, cx));
}

#[gpui::test]
fn a_track_that_ends_goes_on_with_the_next_track_of_the_set(cx: &mut TestAppContext) {
    let (window, view) = open_playable_set(cx);
    click(window, ("set-later", 0usize), cx);
    let paths = planned_paths(&view, cx);
    click_play(window, 0, cx);
    for (next, path) in paths.iter().enumerate().skip(1) {
        cx.update(|cx| view.update(cx, |app, cx| app.track_finished(cx)));
        cx.run_until_parked();
        assert_eq!(held_track(&view, cx), Some(path.clone()));
        assert!(is_sounding(&view, next, cx));
        assert_eq!(sound_position(&view, cx), 0.0);
    }
    cx.update(|cx| view.update(cx, |app, cx| app.track_finished(cx)));
    cx.run_until_parked();
    assert_eq!(held_track(&view, cx), Some(paths[2].clone()));
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(!app.player.offers_pause());
        assert_eq!(app.set_builder.playing, None);
    });
}

#[gpui::test]
fn playing_a_transition_starts_the_track_before_twenty_seconds_from_its_end(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_playable_set(cx);
    let paths = planned_paths(&view, cx);
    assert!(!is_shown(window, ("set-transition", 0usize), cx));
    click(window, ("set-transition", 1usize), cx);
    assert_eq!(held_track(&view, cx), Some(paths[0].clone()));
    assert!(is_sounding(&view, 0, cx));
    let position = sound_position(&view, &mut *cx);
    assert!((position - 0.5).abs() < 0.05, "{position}");

    cx.update(|cx| view.update(cx, |app, cx| app.track_finished(cx)));
    cx.run_until_parked();
    assert_eq!(held_track(&view, cx), Some(paths[1].clone()));
    assert_eq!(sound_position(&view, cx), 0.0);
}

#[gpui::test]
fn the_decks_keep_the_player_from_a_set_row(cx: &mut TestAppContext) {
    let (window, view) = open_playable_set(cx);
    cx.update(|cx| view.update(cx, |app, cx| app.toggle_decks(cx)));
    let held_before = held_track(&view, cx);
    click_play(window, 0, cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(
            app.notice.as_deref(),
            Some(crate::decks::PLAYER_HELD_BY_DECKS)
        );
        assert!(!app.player.offers_pause());
        assert!(app.set_builder.playing.is_none());
    });
    assert_eq!(held_track(&view, cx), held_before);
    click(window, ("set-transition", 1usize), cx);
    cx.update(|cx| assert!(!view.read(cx).player.offers_pause()));
}

fn locked_set_count(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> usize {
    cx.update(|cx| {
        view.read(cx)
            .set_builder
            .planned
            .as_ref()
            .map_or(0, |planned| planned.locked.iter().filter(|l| **l).count())
    })
}

#[gpui::test]
fn a_locked_track_cannot_be_removed_until_it_is_unlocked(cx: &mut TestAppContext) {
    let (window, view) = open_playable_set(cx);
    let before = planned_paths(&view, cx);
    click(window, ("set-lock", 1usize), cx);
    click(window, ("set-remove", 1usize), cx);
    assert_eq!(planned_paths(&view, cx), before);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).notice.as_deref(),
            Some(crate::set_builder::UNLOCK_TO_REMOVE)
        )
    });

    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.select_set_row(1, cx);
            app.remove_selected_set_row(cx);
        })
    });
    assert_eq!(planned_paths(&view, cx), before);

    click(window, ("set-lock", 1usize), cx);
    assert_eq!(locked_set_count(&view, cx), 0);
    click(window, ("set-remove", 1usize), cx);
    assert_eq!(planned_paths(&view, cx).len(), 2);
}

#[gpui::test]
fn a_locked_track_and_its_neighbours_cannot_swap_places(cx: &mut TestAppContext) {
    let (window, view) = open_playable_set(cx);
    let before = planned_paths(&view, cx);
    click(window, ("set-lock", 1usize), cx);
    for (id, position) in [
        ("set-earlier", 1usize),
        ("set-later", 1),
        ("set-later", 0),
        ("set-earlier", 2),
    ] {
        click(window, (id, position), cx);
        assert_eq!(planned_paths(&view, cx), before, "{id} {position}");
    }
    click(window, ("set-lock", 1usize), cx);
    click(window, ("set-later", 0usize), cx);
    assert_eq!(planned_paths(&view, cx)[1], before[0]);
}

#[gpui::test]
fn building_again_keeps_locked_tracks_in_place_and_locked(cx: &mut TestAppContext) {
    let (window, view) = open_playable_set(cx);
    let before = planned_paths(&view, cx);
    click(window, ("set-lock", 0usize), cx);
    click(window, "build-set", cx);
    assert_eq!(planned_paths(&view, cx)[0], before[0]);
    assert_eq!(locked_set_count(&view, cx), 1);
}

fn open_flat_set(
    level: f32,
    adjust: impl Fn(usize, &mut euphoria_core::TrackAnalysis),
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let tracks: Vec<_> = playable_tracks()
        .into_iter()
        .enumerate()
        .map(|(number, mut track)| {
            if let Ok(analysis) = &mut track.outcome {
                adjust(number, analysis);
            }
            track
        })
        .collect();
    let (window, view) = crate::ui_tests::open_window_on(
        tracks,
        Box::new(crate::playback::testing::SilentOutput),
        cx,
    );
    click(window, "set-builder-entry", cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.set_builder.curve = vec![level; crate::set_builder::CURVE_SAMPLES];
        })
    });
    click(window, "build-set", cx);
    assert_eq!(planned_paths(&view, cx).len(), 3);
    (window, view)
}

fn bars(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<crate::journey::Bar> {
    cx.update(|cx| view.read(cx).journey_bars())
}

fn bar_bounds(
    window: AnyWindowHandle,
    id: (&'static str, usize),
    cx: &mut TestAppContext,
) -> gpui::Bounds<gpui::Pixels> {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.find(id).bounds()
    })
    .unwrap()
}

#[gpui::test]
fn each_track_is_a_bar_as_high_as_its_energy_and_off_the_curve_past_the_tolerance(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_flat_set(6.0, |_, _| {}, cx);
    let set_bars = bars(&view, cx);
    assert_eq!(set_bars.len(), 3);
    let chart = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            window.find("set-curve").bounds()
        })
        .unwrap();
    for bar in &set_bars {
        let bounds = bar_bounds(window, ("journey-bar", bar.position), cx);
        let expected = crate::journey::bar_height(bar.energy, f32::from(chart.size.height));
        assert!(
            (f32::from(bounds.size.height) - expected).abs() < 0.5,
            "{} against {expected}",
            f32::from(bounds.size.height)
        );
        assert_eq!(bounds.bottom(), chart.bottom());
        assert_eq!(bar.target, 6.0);
        assert!(bar.tooltip.contains("wants 6.0"), "{}", bar.tooltip);
    }
    let off_curve: Vec<(f32, bool)> = set_bars
        .iter()
        .map(|bar| (bar.energy, bar.off_curve))
        .collect();
    assert!(off_curve.contains(&(5.0, false)), "{off_curve:?}");
    assert!(off_curve.contains(&(6.0, false)), "{off_curve:?}");
    assert!(off_curve.contains(&(7.0, false)), "{off_curve:?}");

    let (_, view) = open_flat_set(5.0, |_, _| {}, cx);
    let off_curve: Vec<(f32, bool)> = bars(&view, cx)
        .iter()
        .map(|bar| (bar.energy, bar.off_curve))
        .collect();
    assert!(off_curve.contains(&(5.0, false)), "{off_curve:?}");
    assert!(
        off_curve.contains(&(6.0, false)),
        "exactly the tolerance away"
    );
    assert!(off_curve.contains(&(7.0, true)), "past the tolerance");
}

#[gpui::test]
fn a_bar_shows_when_its_track_is_locked_playing_or_selected(cx: &mut TestAppContext) {
    let (window, view) = open_playable_set(cx);
    assert!(
        bars(&view, cx)
            .iter()
            .all(|bar| !bar.is_locked && !bar.is_playing && !bar.is_selected)
    );
    for position in 0..3usize {
        assert!(!is_shown(window, ("journey-lock", position), cx));
    }

    click(window, ("set-lock", 1usize), cx);
    assert!(is_shown(window, ("journey-lock", 1usize), cx));
    assert!(!is_shown(window, ("journey-lock", 0usize), cx));
    assert!(!is_shown(window, ("journey-lock", 2usize), cx));
    let locked: Vec<bool> = bars(&view, cx).iter().map(|bar| bar.is_locked).collect();
    assert_eq!(locked, [false, true, false]);

    click_play(window, 2, cx);
    let playing: Vec<bool> = bars(&view, cx).iter().map(|bar| bar.is_playing).collect();
    assert_eq!(playing, [false, false, true]);
    let selected: Vec<bool> = bars(&view, cx).iter().map(|bar| bar.is_selected).collect();
    assert_eq!(selected, [false, false, true]);
}

#[gpui::test]
fn clicking_a_bar_selects_its_row(cx: &mut TestAppContext) {
    let (window, view) = open_playable_set(cx);
    click(window, ("journey-bar", 2usize), cx);
    cx.update(|cx| assert_eq!(view.read(cx).set_builder.selected_row, Some(2)));
    click(window, ("journey-bar", 0usize), cx);
    cx.update(|cx| assert_eq!(view.read(cx).set_builder.selected_row, Some(0)));
    cx.update(|cx| assert!(view.read(cx).set_builder.planned.is_some()));
    cx.update(|cx| assert!(view.read(cx).set_builder.preset.is_some()));
}

#[gpui::test]
fn clicking_between_two_bars_plays_that_transition(cx: &mut TestAppContext) {
    let (window, view) = open_playable_set(cx);
    let paths = planned_paths(&view, cx);
    assert!(!is_shown(window, ("journey-gap", 0usize), cx));
    click(window, ("journey-gap", 1usize), cx);
    assert_eq!(held_track(&view, cx), Some(paths[0].clone()));
    assert!(is_sounding(&view, 0, cx));
    let position = sound_position(&view, &mut *cx);
    assert!((position - 0.5).abs() < 0.05, "{position}");

    cx.update(|cx| view.update(cx, |app, cx| app.track_finished(cx)));
    cx.run_until_parked();
    assert_eq!(held_track(&view, cx), Some(paths[1].clone()));
    assert_eq!(sound_position(&view, cx), 0.0);
}

#[gpui::test]
fn the_summary_is_chips_and_the_details_open_on_demand(cx: &mut TestAppContext) {
    let (window, view) = open_flat_set(5.0, |_, _| {}, cx);
    for index in 0..3usize {
        assert!(is_shown(window, ("set-chip", index), cx));
    }
    assert!(!is_shown(window, ("set-chip", 3usize), cx));
    let chips = cx.update(|cx| {
        let app = view.read(cx);
        let planned = app.set_builder.planned.as_ref().unwrap();
        let seconds: f32 = app.set_track_seconds(planned).iter().sum();
        crate::journey::summary_chips(&planned.plan, seconds)
    });
    let texts: Vec<&str> = chips.iter().map(|chip| chip.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "Energy within 2.0 of the curve",
            "No rule breaks",
            "1 min · 3 tracks"
        ]
    );

    assert!(!is_shown(window, "set-details", cx));
    click(window, "set-details-toggle", cx);
    assert!(is_shown(window, "set-details", cx));
    let lines = cx.update(|cx| {
        crate::journey::detail_lines(&view.read(cx).set_builder.planned.as_ref().unwrap().plan)
    });
    for index in 0..lines.len() {
        assert!(is_shown(window, ("set-detail", index), cx), "{index}");
    }
    assert!(!is_shown(window, ("set-detail", lines.len()), cx));
    click(window, "set-details-toggle", cx);
    assert!(!is_shown(window, "set-details", cx));
}

#[gpui::test]
fn rows_off_the_curve_or_breaking_a_rule_are_flagged(cx: &mut TestAppContext) {
    let (window, view) = open_flat_set(5.0, |_, _| {}, cx);
    let flagged: Vec<(f32, bool)> = bars(&view, cx)
        .iter()
        .map(|bar| (bar.energy, is_shown_sync(window, bar.position, cx)))
        .collect();
    assert!(flagged.contains(&(7.0, true)), "{flagged:?}");
    assert!(flagged.contains(&(5.0, false)), "{flagged:?}");
    assert!(flagged.contains(&(6.0, false)), "{flagged:?}");
    for bar in bars(&view, cx) {
        assert!(is_shown(window, ("set-energy", bar.position), cx));
        let text = crate::journey::energy_text(bar.energy, bar.target, bar.off_curve);
        if bar.off_curve {
            assert_eq!(text, "7 (wants 5.0)");
        } else {
            assert!(text.contains(" of 5.0"), "{text}");
        }
        cx.update(|cx| {
            let planned = view.read(cx).set_builder.planned.clone().unwrap();
            assert!(crate::journey::rule_breaks_into(&planned.plan, bar.position).is_empty());
        });
    }

    let (window, view) = open_flat_set(
        6.0,
        |number, analysis| analysis.bpm = Some(100.0 + 30.0 * number as f32),
        cx,
    );
    let broken = cx.update(|cx| {
        let planned = view.read(cx).set_builder.planned.clone().unwrap();
        (0..3)
            .map(|position| crate::journey::rule_breaks_into(&planned.plan, position).len())
            .collect::<Vec<_>>()
    });
    assert_eq!(broken[0], 0);
    assert_eq!(broken[1], 1);
    assert_eq!(broken[2], 1);
    assert!(!is_shown(window, ("set-row-flag", 0usize), cx));
    for position in 1..3usize {
        assert!(is_shown(window, ("set-row-flag", position), cx));
        assert!(is_shown(window, ("set-row-tempo", position), cx));
        assert!(is_shown(window, ("set-row-title", position), cx));
    }
    assert!(!is_shown(window, ("set-row-tempo", 0usize), cx));
    let chips = cx.update(|cx| {
        let app = view.read(cx);
        let planned = app.set_builder.planned.as_ref().unwrap();
        crate::journey::summary_chips(&planned.plan, 0.0)
    });
    assert_eq!(chips[1].text, "2 rule breaks");
}

fn is_shown_sync(window: AnyWindowHandle, position: usize, cx: &mut TestAppContext) -> bool {
    is_shown(window, ("set-row-flag", position), cx)
}

#[gpui::test]
fn the_chart_the_summary_and_six_rows_fit_without_scrolling(cx: &mut TestAppContext) {
    let (window, _view) = open_builder(cx);
    click(window, "build-set", cx);
    let (page, chart, summary, sixth_row) = cx
        .update_window(window, |_, window, cx| {
            window.render_frame(cx);
            (
                window.find("set-builder").bounds(),
                window.find("set-curve").bounds(),
                window.find("set-summary").bounds(),
                window.find(("set-row", 5usize)).bounds(),
            )
        })
        .unwrap();
    assert!(chart.bottom() <= summary.top());
    assert!(summary.bottom() <= sixth_row.top());
    assert!(
        sixth_row.bottom() <= page.bottom(),
        "sixth row ends at {:?}, page at {:?}",
        sixth_row.bottom(),
        page.bottom()
    );
}
