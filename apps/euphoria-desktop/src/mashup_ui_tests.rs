// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Full, Vocals and Instrumental buttons under the decks, used through
//! the real root view in a headless window: setting one deck to its vocals
//! sets the other to its instrumental, and the other way round, but never a
//! deck the user set by hand.

use euphoria_core::harmony::{INSTRUMENTAL_ONLY, VOCALS_ONLY};
use euphoria_core::stems::StemFiles;
use gpui::{AnyWindowHandle, Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::deck_audio::DeckId;
use crate::playback::testing::SilentOutput;
use crate::ui_tests::{click, deck_tracks, index_of_track, is_shown, open_window_on};

/// The id of a deck's part button: Full 0, Vocals 1, Instrumental 2.
fn part_button(deck: DeckId, position: usize) -> (&'static str, usize) {
    let deck_number = match deck {
        DeckId::A => 0,
        DeckId::B => 1,
    };
    ("deck-stem-preset", deck_number * 10 + position)
}

const FULL: usize = 0;
const VOCALS: usize = 1;
const INSTRUMENTAL: usize = 2;

/// What `deck` plays: `None` for the whole track, its stem levels
/// otherwise.
fn playing(view: &Entity<EuphoriaApp>, deck: DeckId, cx: &mut TestAppContext) -> Option<[f32; 4]> {
    cx.update(|cx| {
        let view = view.read(cx).decks.view(deck);
        view.use_stems.then_some(view.stem_levels)
    })
}

/// Puts the track at `index` on `deck`, with stems as if it had been
/// separated.
fn load_separated(
    window: AnyWindowHandle,
    view: &Entity<EuphoriaApp>,
    deck: DeckId,
    index: usize,
    cx: &mut TestAppContext,
) {
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.load_on_deck(deck, index, cx);
            app.decks.view_mut(deck).stem_files =
                Some(StemFiles::in_directory(std::env::temp_dir().join("stems")));
            cx.notify();
        })
    });
    cx.run_until_parked();
    assert!(is_shown(window, part_button(deck, FULL), cx));
}

#[gpui::test]
fn setting_one_deck_to_vocals_sets_the_other_to_its_instrumental(cx: &mut TestAppContext) {
    let (window, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    click(window, "toggle-decks", cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let wav = index_of_track(&view, "sine-440hz-stereo.wav", cx);
    load_separated(window, &view, DeckId::A, flac, cx);
    load_separated(window, &view, DeckId::B, wav, cx);
    click(window, ("deck-mode", 1_usize), cx);

    click(window, part_button(DeckId::A, VOCALS), cx);
    assert_eq!(playing(&view, DeckId::A, cx), Some(VOCALS_ONLY));
    assert_eq!(playing(&view, DeckId::B, cx), Some(INSTRUMENTAL_ONLY));

    click(window, part_button(DeckId::A, INSTRUMENTAL), cx);
    assert_eq!(playing(&view, DeckId::A, cx), Some(INSTRUMENTAL_ONLY));
    assert_eq!(
        playing(&view, DeckId::B, cx),
        Some(VOCALS_ONLY),
        "the other way round"
    );

    click(window, part_button(DeckId::A, FULL), cx);
    assert_eq!(playing(&view, DeckId::A, cx), None);
    assert_eq!(
        playing(&view, DeckId::B, cx),
        Some(VOCALS_ONLY),
        "the whole track asks nothing of the other deck"
    );
}

#[gpui::test]
fn the_automatic_switch_never_overrides_a_choice_made_on_the_other_deck(cx: &mut TestAppContext) {
    let (window, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    click(window, "toggle-decks", cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    let wav = index_of_track(&view, "sine-440hz-stereo.wav", cx);
    load_separated(window, &view, DeckId::A, flac, cx);
    load_separated(window, &view, DeckId::B, wav, cx);

    click(window, part_button(DeckId::A, VOCALS), cx);
    assert_eq!(playing(&view, DeckId::B, cx), Some(INSTRUMENTAL_ONLY));
    click(window, part_button(DeckId::B, FULL), cx);
    assert_eq!(playing(&view, DeckId::B, cx), None, "B overridden by hand");
    assert_eq!(playing(&view, DeckId::A, cx), Some(VOCALS_ONLY));

    click(window, part_button(DeckId::A, INSTRUMENTAL), cx);
    assert_eq!(playing(&view, DeckId::A, cx), Some(INSTRUMENTAL_ONLY));
    assert_eq!(
        playing(&view, DeckId::B, cx),
        None,
        "B keeps the part it was given by hand"
    );
    click(window, part_button(DeckId::B, VOCALS), cx);
    assert_eq!(
        playing(&view, DeckId::A, cx),
        Some(INSTRUMENTAL_ONLY),
        "A was set by hand too, so B's choice leaves it"
    );

    load_separated(window, &view, DeckId::B, mp3, cx);
    assert_eq!(playing(&view, DeckId::B, cx), None);
    click(window, part_button(DeckId::A, VOCALS), cx);
    assert_eq!(
        playing(&view, DeckId::B, cx),
        Some(INSTRUMENTAL_ONLY),
        "a new track on B follows again"
    );
}
