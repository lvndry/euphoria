// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The controls that narrow the decks' suggestions, used through the real
//! root view in a headless window.
//!
//! On deck A is the FLAC fixture: 8A, 120 BPM, energy 6. The MP3 is 9A,
//! 125 BPM, energy 7, and the WAV is 8B, 118 BPM, energy 5.

use euphoria_core::harmony::Relation;
use gpui::{AnyWindowHandle, Entity, TestAppContext, px, size};

use crate::app::EuphoriaApp;
use crate::deck_audio::DeckId;
use crate::decks::{DeckMode, SuggestionFilter};
use crate::playback::testing::SilentOutput;
use crate::settings::Settings;
use crate::tag_writing::testing::ScratchFolder;
use crate::ui_tests::{
    click, deck_tracks, index_of_track, is_shown, open_window_on, open_window_with, type_into,
};

struct Fixture {
    window: AnyWindowHandle,
    view: Entity<EuphoriaApp>,
    mp3: usize,
    wav: usize,
}

fn decks_with_flac_on_a(
    window: AnyWindowHandle,
    view: Entity<EuphoriaApp>,
    cx: &mut TestAppContext,
) -> Fixture {
    click(window, "toggle-decks", cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    let wav = index_of_track(&view, "sine-440hz-stereo.wav", cx);
    cx.update(|cx| view.update(cx, |app, cx| app.load_on_deck(DeckId::A, flac, cx)));
    Fixture {
        window,
        view,
        mp3,
        wav,
    }
}

fn open_decks(cx: &mut TestAppContext) -> Fixture {
    let (window, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    decks_with_flac_on_a(window, view, cx)
}

fn suggested(fixture: &Fixture, cx: &mut TestAppContext) -> Vec<usize> {
    cx.update(|cx| {
        let mut indexes: Vec<usize> = fixture
            .view
            .read(cx)
            .decks
            .suggestions
            .iter()
            .map(|suggestion| suggestion.track_index)
            .collect();
        indexes.sort_unstable();
        indexes
    })
}

fn both(fixture: &Fixture) -> Vec<usize> {
    let mut indexes = vec![fixture.mp3, fixture.wav];
    indexes.sort_unstable();
    indexes
}

fn relation_button(relation: Relation) -> (&'static str, usize) {
    let position = Relation::ALL
        .iter()
        .position(|candidate| *candidate == relation)
        .unwrap();
    ("suggestion-relation", position)
}

#[gpui::test]
fn a_typed_tempo_range_narrows_the_suggestions(cx: &mut TestAppContext) {
    let fixture = open_decks(cx);
    assert_eq!(suggested(&fixture, cx), both(&fixture));
    type_into(fixture.window, "suggestion-tempo-from", "119", cx);
    assert!(
        is_shown(fixture.window, "suggestion-filter-problem", cx),
        "one end of a range on its own says so"
    );
    assert_eq!(suggested(&fixture, cx), both(&fixture), "and stays open");
    type_into(fixture.window, "suggestion-tempo-to", "130", cx);
    assert!(!is_shown(fixture.window, "suggestion-filter-problem", cx));
    assert_eq!(suggested(&fixture, cx), [fixture.mp3], "118 BPM is out");
}

#[gpui::test]
fn a_typed_energy_range_narrows_the_suggestions(cx: &mut TestAppContext) {
    let fixture = open_decks(cx);
    type_into(fixture.window, "suggestion-energy-from", "6", cx);
    type_into(fixture.window, "suggestion-energy-to", "7", cx);
    assert_eq!(suggested(&fixture, cx), [fixture.mp3], "energy 5 is out");
    type_into(fixture.window, "suggestion-energy-to", "11", cx);
    assert!(is_shown(fixture.window, "suggestion-filter-problem", cx));
    assert_eq!(
        suggested(&fixture, cx),
        both(&fixture),
        "a range that does not read leaves the energy open"
    );
}

#[gpui::test]
fn a_narrow_tempo_window_drops_the_far_tempo(cx: &mut TestAppContext) {
    let fixture = open_decks(cx);
    type_into(fixture.window, "suggestion-window", "2", cx);
    assert_eq!(
        suggested(&fixture, cx),
        [fixture.wav],
        "125 BPM is 4 percent from 120"
    );
}

#[gpui::test]
fn key_move_buttons_choose_the_relations_that_count(cx: &mut TestAppContext) {
    let fixture = open_decks(cx);
    click(fixture.window, relation_button(Relation::Relative), cx);
    assert_eq!(suggested(&fixture, cx), [fixture.mp3], "8B is the relative");
    click(fixture.window, relation_button(Relation::Adjacent), cx);
    assert!(suggested(&fixture, cx).is_empty(), "9A is adjacent");
    click(fixture.window, relation_button(Relation::Same), cx);
    cx.update(|cx| {
        assert_eq!(
            fixture.view.read(cx).decks.filter.relations,
            Some(vec![Relation::Same]),
            "the last key move stays counted"
        );
    });
    click(fixture.window, relation_button(Relation::Relative), cx);
    assert_eq!(suggested(&fixture, cx), [fixture.wav]);
}

#[gpui::test]
fn a_typed_genre_keeps_the_tracks_whose_genre_holds_it(cx: &mut TestAppContext) {
    let fixture = open_decks(cx);
    cx.update(|cx| {
        fixture.view.update(cx, |app, _| {
            let library = app.session.library();
            let path_of = |index: usize| library.track(index).unwrap().path.clone();
            let names = vec![
                (
                    path_of(fixture.mp3),
                    euphoria_core::TrackName {
                        title: None,
                        artist: None,
                        genre: Some("Deep House".to_string()),
                    },
                ),
                (
                    path_of(fixture.wav),
                    euphoria_core::TrackName {
                        title: None,
                        artist: None,
                        genre: Some("Techno".to_string()),
                    },
                ),
            ];
            app.session.set_track_names(names);
        })
    });
    type_into(fixture.window, "suggestion-genre", "house", cx);
    assert_eq!(suggested(&fixture, cx), [fixture.mp3]);
}

#[gpui::test]
fn favorites_can_be_the_pool(cx: &mut TestAppContext) {
    let fixture = open_decks(cx);
    cx.update(|cx| {
        fixture.view.update(cx, |app, _| {
            let path = app
                .session
                .library()
                .track(fixture.wav)
                .unwrap()
                .path
                .clone();
            app.session.favorites_mut().set_starred(&path, true);
        })
    });
    click(fixture.window, ("suggestion-scope", 2_usize), cx);
    assert_eq!(suggested(&fixture, cx), [fixture.wav]);
}

#[gpui::test]
fn the_filter_narrows_mashup_ideas_too(cx: &mut TestAppContext) {
    let fixture = open_decks(cx);
    let ideas = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            let decks = &fixture.view.read(cx).decks;
            assert_eq!(decks.mode, DeckMode::Mashup);
            decks
                .ideas
                .iter()
                .map(|idea| idea.track_index)
                .collect::<Vec<_>>()
        })
    };
    click(fixture.window, ("deck-mode", 1_usize), cx);
    assert!(ideas(cx).contains(&fixture.wav));
    type_into(fixture.window, "suggestion-tempo-from", "119", cx);
    type_into(fixture.window, "suggestion-tempo-to", "130", cx);
    let narrowed = ideas(cx);
    assert!(!narrowed.contains(&fixture.wav), "118 BPM is out");
    assert!(narrowed.contains(&fixture.mp3));
}

#[gpui::test]
fn the_filter_is_saved_and_shown_on_the_next_launch(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("ui-suggestion-filter");
    let settings_path = scratch.0.join("settings.json");
    let (window, view) = open_window_with(
        deck_tracks(),
        Box::new(SilentOutput),
        Settings::default(),
        Some(settings_path.clone()),
        size(px(1440.0), px(900.0)),
        cx,
    );
    let fixture = decks_with_flac_on_a(window, view, cx);
    type_into(fixture.window, "suggestion-tempo-from", "119", cx);
    type_into(fixture.window, "suggestion-tempo-to", "130", cx);
    click(fixture.window, relation_button(Relation::EnergyBoost), cx);
    let saved = Settings::load(&settings_path).unwrap().suggestion_filter;
    assert_eq!(
        saved,
        SuggestionFilter {
            bpm_min: Some(119.0),
            bpm_max: Some(130.0),
            relations: Some(vec![
                Relation::Same,
                Relation::Adjacent,
                Relation::Relative,
                Relation::EnergyBoost,
            ]),
            ..SuggestionFilter::default()
        }
    );
    let settings = Settings {
        suggestion_filter: saved,
        ..Settings::default()
    };
    let (window, view) = open_window_with(
        deck_tracks(),
        Box::new(SilentOutput),
        settings,
        None,
        size(px(1440.0), px(900.0)),
        cx,
    );
    let fixture = decks_with_flac_on_a(window, view, cx);
    cx.update(|cx| {
        let app = fixture.view.read(cx);
        assert_eq!(app.filter_bpm_from_input.read(cx).value(), "119");
        assert_eq!(app.filter_bpm_to_input.read(cx).value(), "130");
    });
    assert_eq!(suggested(&fixture, cx), [fixture.mp3]);
}

fn mixes_with_flac_selected(cx: &mut TestAppContext) -> Fixture {
    let fixture = open_decks(cx);
    let flac = index_of_track(&fixture.view, "sine-440hz-mono.flac", cx);
    cx.update(|cx| fixture.view.update(cx, |app, cx| app.select(flac, cx)));
    fixture
}

fn inspector_mixes(fixture: &Fixture, cx: &mut TestAppContext) -> Vec<usize> {
    cx.update(|cx| {
        fixture
            .view
            .read(cx)
            .mixes
            .iter()
            .map(|mix| mix.track_index)
            .collect()
    })
}

fn sorted(mut indexes: Vec<usize>) -> Vec<usize> {
    indexes.sort_unstable();
    indexes
}

#[gpui::test]
fn the_inspector_mixes_follow_the_tempo_range_window_and_key_moves(cx: &mut TestAppContext) {
    let fixture = mixes_with_flac_selected(cx);
    assert_eq!(sorted(inspector_mixes(&fixture, cx)), both(&fixture));
    type_into(fixture.window, "suggestion-tempo-from", "119", cx);
    type_into(fixture.window, "suggestion-tempo-to", "130", cx);
    assert_eq!(
        inspector_mixes(&fixture, cx),
        [fixture.mp3],
        "118 BPM is out"
    );
    let fixture = mixes_with_flac_selected(cx);
    assert_eq!(sorted(inspector_mixes(&fixture, cx)), both(&fixture));
    type_into(fixture.window, "suggestion-window", "2", cx);
    assert_eq!(
        inspector_mixes(&fixture, cx),
        [fixture.wav],
        "125 is 4 percent away"
    );
    let fixture = mixes_with_flac_selected(cx);
    click(fixture.window, relation_button(Relation::Relative), cx);
    assert_eq!(inspector_mixes(&fixture, cx), [fixture.mp3], "8B is out");
}

#[gpui::test]
fn the_inspector_mixes_follow_the_energy_range_the_genre_and_the_pool(cx: &mut TestAppContext) {
    let fixture = mixes_with_flac_selected(cx);
    type_into(fixture.window, "suggestion-energy-from", "6", cx);
    type_into(fixture.window, "suggestion-energy-to", "7", cx);
    assert_eq!(
        inspector_mixes(&fixture, cx),
        [fixture.mp3],
        "energy 5 is out"
    );
    let fixture = mixes_with_flac_selected(cx);
    cx.update(|cx| {
        fixture.view.update(cx, |app, _| {
            let library = app.session.library();
            let path_of = |index: usize| library.track(index).unwrap().path.clone();
            let names = vec![
                (
                    path_of(fixture.mp3),
                    euphoria_core::TrackName {
                        title: None,
                        artist: None,
                        genre: Some("Deep House".to_string()),
                    },
                ),
                (
                    path_of(fixture.wav),
                    euphoria_core::TrackName {
                        title: None,
                        artist: None,
                        genre: Some("Techno".to_string()),
                    },
                ),
            ];
            app.session.set_track_names(names);
        })
    });
    type_into(fixture.window, "suggestion-genre", "techno", cx);
    assert_eq!(inspector_mixes(&fixture, cx), [fixture.wav]);
    let fixture = mixes_with_flac_selected(cx);
    cx.update(|cx| {
        fixture.view.update(cx, |app, _| {
            let path = app
                .session
                .library()
                .track(fixture.mp3)
                .unwrap()
                .path
                .clone();
            app.session.favorites_mut().set_starred(&path, true);
        })
    });
    click(fixture.window, ("suggestion-scope", 2_usize), cx);
    assert_eq!(inspector_mixes(&fixture, cx), [fixture.mp3]);
}

#[gpui::test]
fn marked_transitions_stay_first_and_an_emptied_list_says_why(cx: &mut TestAppContext) {
    let fixture = mixes_with_flac_selected(cx);
    assert!(!is_shown(fixture.window, "mixes-empty", cx));
    cx.update(|cx| {
        fixture.view.update(cx, |app, cx| {
            app.toggle_transition_from_selected(fixture.wav, cx)
        })
    });
    type_into(fixture.window, "suggestion-tempo-from", "123", cx);
    type_into(fixture.window, "suggestion-tempo-to", "130", cx);
    assert_eq!(
        inspector_mixes(&fixture, cx),
        [fixture.wav, fixture.mp3],
        "the marked 118 BPM transition stays, first"
    );
    click(fixture.window, relation_button(Relation::Relative), cx);
    click(fixture.window, relation_button(Relation::Adjacent), cx);
    assert_eq!(inspector_mixes(&fixture, cx), [fixture.wav]);
    cx.update(|cx| {
        fixture.view.update(cx, |app, cx| {
            app.toggle_transition_from_selected(fixture.wav, cx)
        })
    });
    assert!(inspector_mixes(&fixture, cx).is_empty());
    assert!(is_shown(fixture.window, "mixes-empty", cx));
}

#[gpui::test]
fn a_saved_filter_is_named_in_the_inspector_and_opens_the_decks(cx: &mut TestAppContext) {
    let settings = Settings {
        suggestion_filter: SuggestionFilter {
            bpm_min: Some(100.0),
            bpm_max: Some(110.0),
            ..SuggestionFilter::default()
        },
        ..Settings::default()
    };
    let (window, view) = open_window_with(
        deck_tracks(),
        Box::new(SilentOutput),
        settings,
        None,
        size(px(1440.0), px(2400.0)),
        cx,
    );
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    cx.update(|cx| view.update(cx, |app, cx| app.select(flac, cx)));
    cx.update(|cx| assert!(!view.read(cx).decks.is_open));
    assert!(is_shown(window, "mixes-filter", cx));
    assert!(is_shown(window, "mixes-empty", cx));
    click(window, "mixes-filter", cx);
    cx.update(|cx| assert!(view.read(cx).decks.is_open));
    assert!(is_shown(window, "suggestion-tempo-from", cx));
}

#[gpui::test]
fn an_open_filter_shows_no_filtered_line_in_the_inspector(cx: &mut TestAppContext) {
    let fixture = mixes_with_flac_selected(cx);
    assert!(!is_shown(fixture.window, "mixes-filter", cx));
}
