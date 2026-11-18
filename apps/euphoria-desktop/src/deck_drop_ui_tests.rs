// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Dragging rows of the track table onto the decks through the real root
//! view in a headless window: a row dropped on a deck loads on that deck.

use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext as _, Entity, Modifiers, TestAppContext};

use crate::app::EuphoriaApp;
use crate::deck_audio::DeckId;
use crate::playback::testing::SilentOutput;
use crate::ui_tests::{click, deck_tracks, index_of_track, open_window_on, track};
use crate::views::decks::deck_part;

fn open_decks(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let (window, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    cx.update(|cx| view.update(cx, |app, cx| app.toggle_decks(cx)));
    cx.run_until_parked();
    (window, view)
}

fn drag_row_onto_deck(
    window: AnyWindowHandle,
    track_index: usize,
    deck: DeckId,
    cx: &mut TestAppContext,
) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.drag_to(track(track_index), deck_part("deck", deck), cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn track_on(view: &Entity<EuphoriaApp>, deck: DeckId, cx: &mut TestAppContext) -> Option<usize> {
    cx.update(|cx| view.read(cx).decks.view(deck).track_index)
}

#[gpui::test]
fn a_row_dropped_on_a_deck_loads_on_that_deck(cx: &mut TestAppContext) {
    let (window, view) = open_decks(cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let wav = index_of_track(&view, "sine-440hz-stereo.wav", cx);

    drag_row_onto_deck(window, flac, DeckId::A, cx);
    assert_eq!(track_on(&view, DeckId::A, cx), Some(flac));
    assert_eq!(track_on(&view, DeckId::B, cx), None);

    drag_row_onto_deck(window, wav, DeckId::B, cx);
    assert_eq!(track_on(&view, DeckId::B, cx), Some(wav));
    assert_eq!(track_on(&view, DeckId::A, cx), Some(flac));
}

#[gpui::test]
fn the_row_grabbed_loads_when_several_are_selected(cx: &mut TestAppContext) {
    let (window, view) = open_decks(cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    click(window, track(flac), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click_with_modifiers(track(mp3), Modifiers::secondary_key(cx), cx);
    })
    .unwrap();
    assert_eq!(
        cx.update(|cx| view.read(cx).marked.len()),
        2,
        "both rows are selected"
    );

    drag_row_onto_deck(window, mp3, DeckId::A, cx);

    assert_eq!(track_on(&view, DeckId::A, cx), Some(mp3));
}
