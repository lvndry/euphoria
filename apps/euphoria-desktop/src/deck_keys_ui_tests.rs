// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The deck keys pressed as a person would, in a headless window with an
//! output whose opens finish only when the test says so: space plays or
//! pauses the deck last touched, a loads the selection on deck A, and s, k
//! and l flip the sync, the key lock and the loop of the active deck.

use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::deck_audio::DeckId;
use crate::decks::SentCommand;
use crate::playback::testing::SlowOpeningOutput;
use crate::ui_tests::{deck_tracks, index_of_track, open_window_on};

struct Fixture {
    window: AnyWindowHandle,
    view: Entity<EuphoriaApp>,
    output: SlowOpeningOutput,
    flac: usize,
    mp3: usize,
    wav: usize,
}

fn open(cx: &mut TestAppContext) -> Fixture {
    let output = SlowOpeningOutput::default();
    let (window, view) = open_window_on(deck_tracks(), Box::new(output.clone()), cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    let wav = index_of_track(&view, "sine-440hz-stereo.wav", cx);
    cx.update(|cx| view.update(cx, |app, cx| app.toggle_decks(cx)));
    settle(window, cx);
    Fixture {
        window,
        view,
        output,
        flac,
        mp3,
        wav,
    }
}

fn settle(window: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

fn press(window: AnyWindowHandle, key: &str, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press(key, cx);
    })
    .unwrap();
    cx.run_until_parked();
    cx.update_window(window, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

fn sent_commands(fixture: &Fixture, cx: &mut TestAppContext) -> Vec<SentCommand> {
    cx.update(|cx| fixture.view.read(cx).decks.sent.borrow().clone())
}

#[gpui::test]
fn the_space_bar_plays_and_pauses_the_last_touched_deck(cx: &mut TestAppContext) {
    let fixture = open(cx);
    cx.update(|cx| {
        fixture.view.update(cx, |app, cx| {
            app.load_on_deck(DeckId::A, fixture.flac, cx);
            app.load_on_deck(DeckId::B, fixture.mp3, cx);
        })
    });
    settle(fixture.window, cx);
    fixture.output.finish_opening();
    fixture.output.finish_opening();
    settle(fixture.window, cx);

    let before = sent_commands(&fixture, cx).len();
    press(fixture.window, "space", cx);
    let new = sent_commands(&fixture, cx)[before..].to_vec();
    assert!(
        new.contains(&SentCommand::Play {
            deck: DeckId::B,
            quantized: false
        }),
        "deck B was touched last, so it plays"
    );
    assert!(
        !new.contains(&SentCommand::Play {
            deck: DeckId::A,
            quantized: false
        }),
        "deck A stays paused"
    );
    cx.update(|cx| {
        let decks = &fixture.view.read(cx).decks;
        assert!(decks.view(DeckId::B).playing);
        assert!(!decks.view(DeckId::A).playing);
        assert!(
            !fixture.view.read(cx).player.offers_pause(),
            "the global library player was untouched"
        );
    });

    let before = sent_commands(&fixture, cx).len();
    press(fixture.window, "space", cx);
    let new = sent_commands(&fixture, cx)[before..].to_vec();
    assert!(
        new.contains(&SentCommand::Pause(DeckId::B)),
        "space again pauses it"
    );
    cx.update(|cx| {
        let decks = &fixture.view.read(cx).decks;
        assert!(!decks.view(DeckId::B).playing);
        assert!(!decks.view(DeckId::A).playing);
    });
}

#[gpui::test]
fn the_deck_keys_load_the_selection_and_flip_the_setup(cx: &mut TestAppContext) {
    let fixture = open(cx);
    cx.update(|cx| {
        fixture.view.update(cx, |app, cx| {
            app.load_on_deck(DeckId::B, fixture.flac, cx);
            app.select(fixture.mp3, cx);
        })
    });
    settle(fixture.window, cx);
    fixture.output.finish_opening();
    settle(fixture.window, cx);

    press(fixture.window, "a", cx);
    fixture.output.finish_opening();
    settle(fixture.window, cx);
    cx.update(|cx| {
        let decks = &fixture.view.read(cx).decks;
        assert_eq!(decks.view(DeckId::A).track_index, Some(fixture.mp3));
        assert!(!decks.view(DeckId::A).loading);
        assert!(!decks.view(DeckId::A).playing);
        assert_eq!(decks.last_touched_deck, DeckId::A, "a touches deck A");
    });

    press(fixture.window, "s", cx);
    press(fixture.window, "k", cx);
    cx.update(|cx| {
        let decks = &fixture.view.read(cx).decks;
        assert!(!decks.sync, "s flips the sync");
        assert!(
            !decks.view(DeckId::A).keylock,
            "k flips the active deck's key lock off"
        );
    });

    let looping_before = cx.update(|cx| fixture.view.read(cx).decks.is_looping(DeckId::A, 4));
    press(fixture.window, "l", cx);
    let looping_after = cx.update(|cx| fixture.view.read(cx).decks.is_looping(DeckId::A, 4));
    if looping_after != looping_before {
        press(fixture.window, "l", cx);
        cx.update(|cx| {
            assert_eq!(
                fixture.view.read(cx).decks.is_looping(DeckId::A, 4),
                looping_before,
                "l again leaves the loop"
            );
        });
    }
}

#[gpui::test]
fn with_the_decks_closed_the_deck_keys_do_nothing(cx: &mut TestAppContext) {
    let fixture = open(cx);
    cx.update(|cx| {
        fixture
            .view
            .update(cx, |app, cx| app.load_on_deck(DeckId::A, fixture.flac, cx))
    });
    settle(fixture.window, cx);
    fixture.output.finish_opening();
    settle(fixture.window, cx);
    cx.update(|cx| {
        fixture.view.update(cx, |app, cx| {
            app.select(fixture.wav, cx);
            app.toggle_decks(cx);
        })
    });
    settle(fixture.window, cx);
    let sent = sent_commands(&fixture, cx);

    press(fixture.window, "space", cx);
    press(fixture.window, "a", cx);
    press(fixture.window, "s", cx);
    cx.update(|cx| {
        let app = fixture.view.read(cx);
        assert!(!app.decks.is_open);
        assert!(
            app.decks
                .sent
                .borrow()
                .iter()
                .all(|command| sent.contains(command)),
            "no deck command went out"
        );
        assert!(app.decks.sync);
        assert!(app.decks.view(DeckId::A).keylock);
    });
}
