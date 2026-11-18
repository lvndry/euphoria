// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Loading tracks on the decks through the real root view in a headless
//! window, with an output whose opens finish only when the test says so:
//! a deck asks the output for its track at once, whatever waveform decodes
//! wait, shows it loading and paused until it opens, and takes only the
//! track asked for last.

use std::path::PathBuf;

use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::deck_audio::DeckId;
use crate::decks::SentCommand;
use crate::playback::testing::SlowOpeningOutput;
use crate::ui_tests::{deck_tracks, index_of_track, open_window_on};
use crate::waveforms::Waveform;

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

fn path_of(fixture: &Fixture, index: usize, cx: &mut TestAppContext) -> PathBuf {
    cx.update(|cx| {
        fixture.view.read(cx).session.library().tracks()[index]
            .path
            .clone()
    })
}

fn loads_sent(fixture: &Fixture, deck: DeckId, cx: &mut TestAppContext) -> usize {
    cx.update(|cx| {
        fixture
            .view
            .read(cx)
            .decks
            .sent
            .borrow()
            .iter()
            .filter(|sent| **sent == SentCommand::Load(deck))
            .count()
    })
}

fn selected_is_decoded(fixture: &Fixture, cx: &mut TestAppContext) -> bool {
    cx.update(|cx| {
        matches!(
            fixture.view.read(cx).selected_waveform(),
            Some(Waveform::Ready(..))
        )
    })
}

#[gpui::test]
fn a_deck_asks_the_output_for_its_track_at_once_and_loads_while_decodes_wait(
    cx: &mut TestAppContext,
) {
    let fixture = open(cx);
    let flac = path_of(&fixture, fixture.flac, cx);
    cx.update(|cx| {
        fixture.view.update(cx, |app, cx| {
            app.select(fixture.wav, cx);
            app.select(fixture.mp3, cx);
            app.load_on_deck(DeckId::A, fixture.flac, cx);
        })
    });

    assert_eq!(
        fixture.output.waiting(),
        std::slice::from_ref(&flac),
        "the open goes to the output before any queued work runs"
    );
    cx.update(|cx| {
        let deck = fixture.view.read(cx).decks.view(DeckId::A);
        assert!(deck.loading);
        assert!(!deck.playing);
        assert_eq!(deck.path.as_deref(), Some(flac.as_path()));
    });

    fixture.output.finish_opening();
    settle(fixture.window, cx);

    assert!(
        !selected_is_decoded(&fixture, cx),
        "the waveform decodes have not run yet"
    );
    assert_eq!(loads_sent(&fixture, DeckId::A, cx), 1);
    cx.update(|cx| {
        let deck = fixture.view.read(cx).decks.view(DeckId::A);
        assert!(!deck.loading, "the deck switches once its track opens");
        assert_eq!(deck.problem, None);
        assert!(deck.duration_seconds.is_some());
    });
}

#[gpui::test]
fn a_late_answer_for_a_track_no_longer_wanted_is_ignored(cx: &mut TestAppContext) {
    let fixture = open(cx);
    let flac = path_of(&fixture, fixture.flac, cx);
    let mp3 = path_of(&fixture, fixture.mp3, cx);
    cx.update(|cx| {
        fixture
            .view
            .update(cx, |app, cx| app.load_on_deck(DeckId::A, fixture.flac, cx))
    });
    settle(fixture.window, cx);
    cx.update(|cx| {
        fixture
            .view
            .update(cx, |app, cx| app.load_on_deck(DeckId::A, fixture.mp3, cx))
    });
    settle(fixture.window, cx);
    assert_eq!(fixture.output.waiting(), [flac.clone(), mp3.clone()]);

    fixture.output.finish_opening();
    settle(fixture.window, cx);

    assert_eq!(
        loads_sent(&fixture, DeckId::A, cx),
        0,
        "the first track opened too late to go on the deck"
    );
    cx.update(|cx| {
        let deck = fixture.view.read(cx).decks.view(DeckId::A);
        assert!(deck.loading, "the deck still waits for the track it shows");
        assert!(!deck.playing);
        assert_eq!(deck.path.as_deref(), Some(mp3.as_path()));
        assert_eq!(deck.track_index, Some(fixture.mp3));
    });

    fixture.output.finish_opening();
    settle(fixture.window, cx);

    assert_eq!(loads_sent(&fixture, DeckId::A, cx), 1);
    cx.update(|cx| {
        let deck = fixture.view.read(cx).decks.view(DeckId::A);
        assert!(!deck.loading);
        assert_eq!(deck.path.as_deref(), Some(mp3.as_path()));
        assert_eq!(deck.problem, None);
    });
}

#[gpui::test]
fn closing_the_decks_drops_an_open_still_on_its_way(cx: &mut TestAppContext) {
    let fixture = open(cx);
    cx.update(|cx| {
        fixture
            .view
            .update(cx, |app, cx| app.load_on_deck(DeckId::B, fixture.flac, cx))
    });
    settle(fixture.window, cx);
    cx.update(|cx| fixture.view.update(cx, |app, cx| app.toggle_decks(cx)));
    settle(fixture.window, cx);

    fixture.output.finish_opening();
    settle(fixture.window, cx);

    assert_eq!(loads_sent(&fixture, DeckId::B, cx), 0);
    cx.update(|cx| {
        let decks = &fixture.view.read(cx).decks;
        assert!(!decks.is_open);
        assert!(!decks.view(DeckId::B).loading);
        assert!(!decks.view(DeckId::B).playing);
    });
}

#[gpui::test]
fn move_to_a_shows_only_once_deck_b_has_its_track(cx: &mut TestAppContext) {
    let fixture = open(cx);
    cx.update(|cx| {
        fixture
            .view
            .update(cx, |app, cx| app.load_on_deck(DeckId::B, fixture.flac, cx))
    });
    settle(fixture.window, cx);
    assert!(!crate::ui_tests::is_shown(
        fixture.window,
        "deck-move-to-a",
        cx
    ));

    fixture.output.finish_opening();
    settle(fixture.window, cx);
    assert!(crate::ui_tests::is_shown(
        fixture.window,
        "deck-move-to-a",
        cx
    ));
}

#[gpui::test]
fn deck_b_plays_on_untouched_when_move_to_a_cannot_open_the_track(cx: &mut TestAppContext) {
    let fixture = open(cx);
    let mp3 = path_of(&fixture, fixture.mp3, cx);
    cx.update(|cx| {
        fixture.view.update(cx, |app, cx| {
            app.load_on_deck(DeckId::A, fixture.flac, cx);
            app.load_on_deck(DeckId::B, fixture.mp3, cx);
        })
    });
    fixture.output.finish_opening();
    fixture.output.finish_opening();
    settle(fixture.window, cx);
    cx.update(|cx| {
        fixture
            .view
            .update(cx, |app, cx| app.toggle_deck_play(DeckId::B, cx))
    });
    settle(fixture.window, cx);

    cx.update(|cx| fixture.view.update(cx, |app, cx| app.move_b_to_a(cx)));
    settle(fixture.window, cx);
    fixture.output.fail_opening();
    settle(fixture.window, cx);

    cx.update(|cx| {
        let decks = &fixture.view.read(cx).decks;
        let deck_b = decks.view(DeckId::B);
        assert_eq!(deck_b.track_index, Some(fixture.mp3));
        assert_eq!(deck_b.path.as_deref(), Some(mp3.as_path()));
        assert!(deck_b.playing, "B keeps playing");
        assert_eq!(deck_b.problem, None);
        assert!(decks.view(DeckId::A).problem.is_some());
        assert!(!decks.view(DeckId::A).playing);
    });
}
