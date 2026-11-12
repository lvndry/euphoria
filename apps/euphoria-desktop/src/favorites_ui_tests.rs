// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests that star tracks and mark transitions in the real root view, in a
//! headless window, with clicks and keys as a person would.

use std::path::PathBuf;

use gpui::test::TestWindowExt;
use gpui::{AppContext, TestAppContext};

use crate::deck_audio::DeckId;
use crate::playback::testing::SilentOutput;
use crate::playlist_flow::PlaylistFormat;
use crate::tag_writing::testing::ScratchFolder;
use crate::ui_tests::{
    click, deck_tracks, index_of_track, is_shown, open_library, open_window_on, open_window_with,
    open_with_playlist_store, track,
};

/// Rests the pointer on the row of `index`, where an empty star only
/// shows on hover, then clicks its star.
fn click_star(window: gpui::AnyWindowHandle, index: usize, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.hover(track(index), cx);
        window.render_frame(cx);
        window.click(("star", index), cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn starred(view: &gpui::Entity<crate::app::EuphoriaApp>, cx: &mut TestAppContext) -> Vec<PathBuf> {
    cx.update(|cx| view.read(cx).session.favorites().tracks().to_vec())
}

fn path_of(
    view: &gpui::Entity<crate::app::EuphoriaApp>,
    index: usize,
    cx: &mut TestAppContext,
) -> PathBuf {
    cx.update(|cx| {
        view.read(cx)
            .session
            .library()
            .track(index)
            .unwrap()
            .path
            .clone()
    })
}

#[gpui::test]
fn the_f_key_stars_the_selection_and_the_sidebar_shows_only_the_starred(cx: &mut TestAppContext) {
    let (window, view) = open_library(10, cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(track(3), cx);
        window.press("f", cx);
        window.click(track(7), cx);
        window.press("f", cx);
    })
    .unwrap();
    let third = path_of(&view, 3, cx);
    let seventh = path_of(&view, 7, cx);
    assert_eq!(starred(&view, cx), [third.clone(), seventh.clone()]);

    click(window, "favorites", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.showing_favorites);
        assert_eq!(app.visible, [3, 7]);
    });
    assert!(is_shown(window, "export-favorites-m3u8", cx));

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click(track(3), cx);
        window.press("f", cx);
    })
    .unwrap();
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).visible,
            [7],
            "the unstarred track leaves the list"
        )
    });
    assert_eq!(starred(&view, cx), [seventh]);

    click(window, "all-tracks", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(!app.showing_favorites);
        assert_eq!(app.visible.len(), 10);
    });
    assert!(!is_shown(window, "export-favorites-m3u8", cx));
}

#[gpui::test]
fn the_star_of_a_row_stars_that_track_and_leaves_the_selection(cx: &mut TestAppContext) {
    let (window, view) = open_library(10, cx);
    click(window, track(2), cx);
    click_star(window, 5, cx);
    let fifth = path_of(&view, 5, cx);
    assert_eq!(starred(&view, cx), [fifth]);
    cx.update(|cx| assert_eq!(view.read(cx).selected, Some(2)));

    click(window, "inspector-star", cx);
    let second = path_of(&view, 2, cx);
    assert!(
        starred(&view, cx).contains(&second),
        "the inspector stars the selected track"
    );
    click(window, "inspector-star", cx);
    assert!(!starred(&view, cx).contains(&second));
}

#[gpui::test]
fn favorites_are_kept_and_found_again_on_the_next_launch(cx: &mut TestAppContext) {
    let (_scratch, files, store_path, window, view) =
        open_with_playlist_store("ui-favorites-store", cx);
    let index_of = |path: &PathBuf, cx: &mut TestAppContext| {
        cx.update(|cx| view.read(cx).session.library().index_of(path).unwrap())
    };
    let first = index_of(&files[0], cx);
    let third = index_of(&files[2], cx);
    click_star(window, third, cx);
    click_star(window, first, cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.select(first, cx);
            app.toggle_transition_from_selected(third, cx);
        })
    });
    cx.run_until_parked();

    let store = euphoria_core::PlaylistStore::open(&store_path).unwrap();
    let kept = store.load_favorites().unwrap();
    let kept_paths: Vec<PathBuf> = kept.tracks.iter().map(|entry| entry.path.clone()).collect();
    assert_eq!(kept_paths, [files[2].clone(), files[0].clone()]);
    assert_eq!(kept.transitions.len(), 1);
    assert_eq!(kept.transitions[0].from.path, files[0]);
    assert_eq!(kept.transitions[0].to.path, files[2]);
    assert_eq!(
        kept.tracks[0].content,
        euphoria_core::ContentHash::of_file(&files[2]).unwrap(),
        "a favorite is kept by the content of its file"
    );

    let mut next_launch = crate::state::Session::start(None).unwrap();
    next_launch.use_playlist_store(store).unwrap();
    assert_eq!(
        next_launch.favorites().tracks(),
        [files[2].clone(), files[0].clone()]
    );
    assert!(
        next_launch
            .favorites()
            .transition(&files[0], &files[2])
            .is_some()
    );
}

#[gpui::test]
fn a_transition_marked_on_the_decks_comes_first_and_keeps_its_note(cx: &mut TestAppContext) {
    let (window, view) = open_window_on(deck_tracks(), Box::new(SilentOutput), cx);
    click(window, "toggle-decks", cx);
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    let wav = index_of_track(&view, "sine-440hz-stereo.wav", cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.load_on_deck(DeckId::A, flac, cx);
            app.load_on_deck(DeckId::B, mp3, cx);
        })
    });
    cx.run_until_parked();
    let first = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            let suggestion = view.read(cx).decks.suggestions[0];
            (suggestion.track_index, suggestion.is_favorite)
        })
    };
    assert_eq!(
        first(cx),
        (wav, false),
        "the relative key at a closer tempo leads"
    );

    click(window, "deck-transition", cx);
    assert_eq!(first(cx), (mp3, true), "the marked transition moves up");

    click(window, "transition-note", cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |app, cx| {
            app.transition_note_input.update(cx, |input, cx| {
                input.set_value("  bring the bass in on the drop ", window, cx);
            });
            app.finish_editing_transition_note(cx);
        })
    })
    .unwrap();
    let (from, to) = cx.update(|cx| view.read(cx).deck_pair().unwrap());
    cx.update(|cx| {
        let favorites = view.read(cx).session.favorites();
        assert_eq!(
            favorites.transition(&from, &to).unwrap().note.as_deref(),
            Some("bring the bass in on the drop")
        );
    });

    click(window, ("deck-star", 1_usize), cx);
    assert_eq!(starred(&view, cx), std::slice::from_ref(&to));

    click(window, "deck-transition", cx);
    assert_eq!(
        first(cx),
        (wav, false),
        "taking the mark off puts the order back"
    );
}

#[gpui::test]
fn a_transition_marked_in_mixes_well_with_comes_first_there(cx: &mut TestAppContext) {
    let tall_enough_for_the_whole_inspector = gpui::size(gpui::px(1440.0), gpui::px(2400.0));
    let (window, view) = open_window_with(
        deck_tracks(),
        Box::new(SilentOutput),
        crate::settings::Settings::default(),
        None,
        tall_enough_for_the_whole_inspector,
        cx,
    );
    let flac = index_of_track(&view, "sine-440hz-mono.flac", cx);
    let mp3 = index_of_track(&view, "sine-440hz-mono.mp3", cx);
    click(window, track(flac), cx);
    let mixes = |cx: &mut TestAppContext| {
        cx.update(|cx| {
            view.read(cx)
                .mixes
                .iter()
                .map(|mix| (mix.track_index, mix.is_favorite))
                .collect::<Vec<_>>()
        })
    };
    assert!(mixes(cx).contains(&(mp3, false)));
    click(window, ("mix-star", mp3), cx);
    assert_eq!(mixes(cx)[0], (mp3, true));
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).selected,
            Some(flac),
            "the star does not select"
        )
    });
}

#[gpui::test]
fn the_starred_tracks_export_in_their_order_as_m3u8_and_rekordbox(cx: &mut TestAppContext) {
    let (window, view) = open_library(6, cx);
    click_star(window, 4, cx);
    click_star(window, 1, cx);
    let scratch = ScratchFolder::new("ui-favorites-export");
    let write = |format, file: &str, cx: &mut TestAppContext| {
        let job = cx.update(|cx| view.read(cx).favorites_export_job(format));
        let destination = scratch.0.join(file);
        let message = job.write(&destination).unwrap();
        (std::fs::read_to_string(&destination).unwrap(), message)
    };
    let fourth = path_of(&view, 4, cx);
    let first = path_of(&view, 1, cx);

    let (m3u8, _) = write(PlaylistFormat::M3u8, "favorites.m3u8", cx);
    assert!(m3u8.starts_with("#EXTM3U\n#PLAYLIST:Favorites\n"), "{m3u8}");
    let fourth_at = m3u8.find(&*fourth.to_string_lossy()).unwrap();
    let first_at = m3u8.find(&*first.to_string_lossy()).unwrap();
    assert!(
        fourth_at < first_at,
        "in the order they were starred: {m3u8}"
    );

    let (xml, message) = write(PlaylistFormat::Rekordbox, "favorites.xml", cx);
    assert!(xml.contains("Name=\"Favorites\""), "{xml}");
    assert_eq!(xml.matches("<TRACK TrackID").count(), 2, "{xml}");
    assert!(message.contains("rekordbox"), "{message}");
}
