// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tracks to play next, used through the real root view in a headless
//! window.
//!
//! The FLAC fixture is 8A, 120 BPM, energy 6. The MP3 is 9A, 125 BPM,
//! energy 7, and the WAV is 8B, 118 BPM, energy 5.

use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, px, size};

use crate::app::EuphoriaApp;
use crate::deck_audio::DeckId;
use crate::next_rail::EnergyChoice;
use crate::playback::testing::SilentOutput;
use crate::settings::Settings;
use crate::track_filters::TrackFilters;
use crate::ui_tests::{
    click, deck_tracks, index_of_track, is_shown, open_window_on, open_window_with,
};
use crate::views::next_rail::{energy_id, row_id};

struct Fixture {
    window: AnyWindowHandle,
    view: Entity<EuphoriaApp>,
    flac: usize,
    mp3: usize,
    wav: usize,
}

fn open_with(settings: Settings, cx: &mut TestAppContext) -> Fixture {
    let (window, view) = open_window_with(
        deck_tracks(),
        Box::new(SilentOutput),
        settings,
        None,
        size(px(1440.0), px(900.0)),
        cx,
    );
    fixture(window, view, cx)
}

fn fixture(window: AnyWindowHandle, view: Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Fixture {
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    let wav = index_of_track(&view, "sine-440hz-stereo.wav", cx);
    Fixture {
        window,
        view,
        flac,
        mp3,
        wav,
    }
}

fn open(cx: &mut TestAppContext) -> Fixture {
    let (window, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    fixture(window, view, cx)
}

fn settle(fixture: &Fixture, cx: &mut TestAppContext) {
    cx.run_until_parked();
    cx.update_window(fixture.window, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

fn play_selected(fixture: &Fixture, index: usize, cx: &mut TestAppContext) {
    cx.update(|cx| {
        fixture.view.update(cx, |app, cx| {
            app.select(index, cx);
            app.play_selection(cx);
        })
    });
    settle(fixture, cx);
}

fn listed(fixture: &Fixture, cx: &mut TestAppContext) -> Vec<usize> {
    cx.update(|cx| {
        fixture
            .view
            .read(cx)
            .next_rows
            .iter()
            .map(|row| row.track_index)
            .collect()
    })
}

fn title(fixture: &Fixture, cx: &mut TestAppContext) -> String {
    cx.update(|cx| fixture.view.read(cx).mixes_title())
}

#[gpui::test]
fn the_list_follows_the_playing_track_and_falls_back_to_the_selected_one(cx: &mut TestAppContext) {
    let fixture = open(cx);
    cx.update(|cx| {
        fixture
            .view
            .update(cx, |app, cx| app.select(fixture.flac, cx))
    });
    settle(&fixture, cx);
    assert_eq!(title(&fixture, cx), "Mixes well with");
    assert!(listed(&fixture, cx).is_empty());
    assert!(!is_shown(fixture.window, energy_id(EnergyChoice::Keep), cx));

    play_selected(&fixture, fixture.flac, cx);
    assert!(title(&fixture, cx).starts_with("Next after "));
    let mut rows = listed(&fixture, cx);
    rows.sort_unstable();
    let mut expected = vec![fixture.mp3, fixture.wav];
    expected.sort_unstable();
    assert_eq!(rows, expected);
    assert!(is_shown(fixture.window, row_id(fixture.mp3), cx));
    assert!(is_shown(fixture.window, energy_id(EnergyChoice::Lift), cx));

    cx.update(|cx| fixture.view.update(cx, |app, cx| app.toggle_playback(cx)));
    settle(&fixture, cx);
    assert_eq!(title(&fixture, cx), "Mixes well with");
    assert!(listed(&fixture, cx).is_empty());
}

#[gpui::test]
fn keep_lift_and_drop_reorder_the_list(cx: &mut TestAppContext) {
    let fixture = open(cx);
    play_selected(&fixture, fixture.flac, cx);
    click(fixture.window, energy_id(EnergyChoice::Lift), cx);
    assert_eq!(listed(&fixture, cx), [fixture.mp3, fixture.wav]);
    click(fixture.window, energy_id(EnergyChoice::Drop), cx);
    assert_eq!(listed(&fixture, cx), [fixture.wav, fixture.mp3]);
    click(fixture.window, energy_id(EnergyChoice::Keep), cx);
    cx.update(|cx| assert_eq!(fixture.view.read(cx).next_energy, EnergyChoice::Keep));
}

#[gpui::test]
fn the_rows_carry_the_pills_and_the_tempo(cx: &mut TestAppContext) {
    let fixture = open(cx);
    play_selected(&fixture, fixture.flac, cx);
    click(fixture.window, energy_id(EnergyChoice::Lift), cx);
    let pills: Vec<(usize, Vec<String>)> = cx.update(|cx| {
        fixture
            .view
            .read(cx)
            .next_rows
            .iter()
            .map(|row| (row.track_index, row.pills()))
            .collect()
    });
    assert_eq!(
        pills,
        [
            (
                fixture.mp3,
                vec!["one step".to_string(), "energy +1".to_string()]
            ),
            (
                fixture.wav,
                vec!["relative".to_string(), "energy \u{2212}1".to_string()]
            ),
        ]
    );
}

#[gpui::test]
fn the_filter_bar_keeps_tracks_out_of_the_list(cx: &mut TestAppContext) {
    let fixture = open_with(
        Settings {
            track_filters: TrackFilters {
                bpm_min: Some(119.0),
                bpm_max: Some(130.0),
                ..TrackFilters::default()
            },
            ..Settings::default()
        },
        cx,
    );
    play_selected(&fixture, fixture.flac, cx);
    assert_eq!(
        listed(&fixture, cx),
        [fixture.mp3],
        "118 BPM is filtered out"
    );
}

#[gpui::test]
fn tracks_played_this_session_are_not_suggested_again(cx: &mut TestAppContext) {
    let fixture = open(cx);
    play_selected(&fixture, fixture.flac, cx);
    cx.update(|cx| {
        fixture
            .view
            .update(cx, |app, cx| app.play_next_track(fixture.mp3, cx))
    });
    settle(&fixture, cx);
    cx.update(|cx| assert_eq!(fixture.view.read(cx).rail_anchor, Some(fixture.mp3)));
    assert!(
        !listed(&fixture, cx).contains(&fixture.flac),
        "the first track played already, though 9A mixes into 8A"
    );
    cx.update(|cx| assert_eq!(fixture.view.read(cx).played_this_session.len(), 2));
}

#[gpui::test]
fn enter_plays_the_selected_row_in_the_player(cx: &mut TestAppContext) {
    let fixture = open(cx);
    play_selected(&fixture, fixture.flac, cx);
    click(fixture.window, row_id(fixture.wav), cx);
    cx.update(|cx| {
        let app = fixture.view.read(cx);
        assert_eq!(app.selected, Some(fixture.wav), "a click selects");
        assert_eq!(
            app.rail_anchor,
            Some(fixture.flac),
            "and the playing track stays"
        );
        assert!(app.player.is_playing());
    });
    cx.update_window(fixture.window, |_, window, cx| window.press("enter", cx))
        .unwrap();
    settle(&fixture, cx);
    cx.update(|cx| {
        let app = fixture.view.read(cx);
        assert_eq!(app.rail_anchor, Some(fixture.wav));
        assert!(app.player.is_playing());
        assert!(!app.focus.is_open, "Enter in the list does not open focus");
    });
}

#[gpui::test]
fn enter_loads_deck_b_when_the_decks_are_open(cx: &mut TestAppContext) {
    let fixture = open(cx);
    click(fixture.window, "toggle-decks", cx);
    cx.update(|cx| {
        fixture.view.update(cx, |app, cx| {
            app.load_on_deck(DeckId::A, fixture.flac, cx);
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        fixture
            .view
            .update(cx, |app, cx| app.toggle_deck_play(DeckId::A, cx))
    });
    settle(&fixture, cx);
    cx.update(|cx| assert_eq!(fixture.view.read(cx).rail_anchor, Some(fixture.flac)));
    cx.update(|cx| {
        fixture.view.update(cx, |app, cx| {
            app.selected = Some(fixture.mp3);
            app.play_next_row(cx);
        })
    });
    cx.update(|cx| {
        let app = fixture.view.read(cx);
        assert_eq!(app.decks.view(DeckId::B).track_index, Some(fixture.mp3));
        assert!(!app.player.offers_pause(), "the player stays out of it");
    });
}
