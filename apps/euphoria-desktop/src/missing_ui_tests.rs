// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Missing tracks in a window without a screen: a playlist and a favorite
//! whose files moved or were deleted show as missing, one is located by
//! content, and one is removed with its playlist entry and its star.

use std::path::{Path, PathBuf};

use euphoria_core::{PlaylistEntry, PlaylistStore, StoredFavorites, StoredPlaylist};
use gpui::{AppContext, Entity, TestAppContext, WindowOptions};

use crate::app::EuphoriaApp;
use crate::library::LibraryFilter;
use crate::playback::testing::SilentOutput;
use crate::settings::Settings;
use crate::state::Session;
use crate::tag_writing::testing::ScratchFolder;

struct MissingLibrary {
    _scratch: ScratchFolder,
    store_path: PathBuf,
    moved_from: PathBuf,
    moved_to: PathBuf,
    deleted: PathBuf,
    other_audio: PathBuf,
    view: Entity<EuphoriaApp>,
}

/// A playlist of three tracks, the second starred, where the first was
/// moved to another folder and the second deleted, opened in a window.
fn open_with_missing_files(name: &str, cx: &mut TestAppContext) -> MissingLibrary {
    let scratch = ScratchFolder::new(name);
    let files = scratch.copy_fixtures();
    let store_path = scratch.0.join("playlists.sqlite3");
    let entries: Vec<PlaylistEntry> = files
        .iter()
        .map(|file| PlaylistEntry::for_file(file, None).unwrap())
        .collect();
    let mut store = PlaylistStore::open(&store_path).unwrap();
    store
        .save(&[StoredPlaylist {
            name: "Peak time".to_string(),
            label: None,
            entries: entries.clone(),
        }])
        .unwrap();
    store
        .save_favorites(&StoredFavorites {
            tracks: vec![entries[1].clone()],
            transitions: Vec::new(),
        })
        .unwrap();
    drop(store);

    let moved_folder = scratch.0.join("moved");
    std::fs::create_dir_all(&moved_folder).unwrap();
    let moved_to = moved_folder.join(files[0].file_name().unwrap());
    std::fs::rename(&files[0], &moved_to).unwrap();
    std::fs::remove_file(&files[1]).unwrap();

    let session_store_path = store_path.clone();
    let view = cx.update(|cx| {
        gpui::init(cx);
        let (_, view) = gpui::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| {
                let mut session = Session::start(None).unwrap();
                session
                    .use_playlist_store(PlaylistStore::open(&session_store_path).unwrap())
                    .unwrap();
                EuphoriaApp::new(
                    session,
                    Settings::default(),
                    None,
                    None,
                    Box::new(SilentOutput),
                    window,
                    cx,
                )
            })
        })
        .unwrap();
        view
    });
    MissingLibrary {
        moved_from: files[0].clone(),
        deleted: files[1].clone(),
        other_audio: files[2].clone(),
        moved_to,
        store_path,
        view,
        _scratch: scratch,
    }
}

fn missing_paths(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<PathBuf> {
    cx.update(|cx| {
        view.read(cx)
            .session
            .library()
            .tracks()
            .iter()
            .filter(|track| track.is_missing())
            .map(|track| track.path.clone())
            .collect()
    })
}

fn kept_playlist_paths(store_path: &Path) -> Vec<PathBuf> {
    PlaylistStore::open(store_path).unwrap().load().unwrap()[0]
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect()
}

#[gpui::test]
fn files_of_a_playlist_that_are_gone_show_as_missing(cx: &mut TestAppContext) {
    let library = open_with_missing_files("ui-missing-shown", cx);
    assert_eq!(
        missing_paths(&library.view, cx),
        [library.moved_from.clone(), library.deleted.clone()]
    );
    cx.update(|cx| {
        library.view.update(cx, |app, cx| {
            assert_eq!(app.sidebar_counts().missing, 2);
            assert_eq!(app.sidebar_counts().needs_review, 0);
            app.show_missing_tracks(cx);
            assert_eq!(app.filter, LibraryFilter::Missing);
            assert_eq!(app.visible.len(), 2);
        })
    });
    // Nothing is dropped from the saved playlist while its files are gone.
    assert_eq!(kept_playlist_paths(&library.store_path).len(), 3);
}

#[gpui::test]
fn a_moved_file_is_located_by_content_and_other_audio_is_refused(cx: &mut TestAppContext) {
    let library = open_with_missing_files("ui-missing-locate", cx);
    let view = library.view.clone();

    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.locate_track(library.moved_from.clone(), library.other_audio.clone(), cx)
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(
            app.notice
                .as_deref()
                .is_some_and(|notice| notice.contains("holds other audio")),
            "{:?}",
            app.notice
        );
    });
    assert!(missing_paths(&view, cx).contains(&library.moved_from));

    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.notice = None;
            app.locate_track(library.moved_from.clone(), library.moved_to.clone(), cx)
        })
    });
    cx.run_until_parked();
    assert_eq!(
        missing_paths(&view, cx),
        std::slice::from_ref(&library.deleted)
    );
    cx.update(|cx| {
        let library_state = view.read(cx).session.library();
        assert_eq!(library_state.index_of(&library.moved_from), None);
        let relinked = library_state.track_by_path(&library.moved_to).unwrap();
        assert!(!relinked.is_missing());
    });
    let kept = kept_playlist_paths(&library.store_path);
    assert_eq!(kept[0], library.moved_to);
    assert_eq!(kept.len(), 3);
}

#[gpui::test]
fn removing_a_missing_track_takes_it_out_of_its_playlist_and_favorites(cx: &mut TestAppContext) {
    let library = open_with_missing_files("ui-missing-remove", cx);
    let view = library.view.clone();
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.selected = app.session.library().index_of(&library.deleted);
            app.remove_missing_tracks_of_selection(cx);
        })
    });
    cx.run_until_parked();

    assert_eq!(
        missing_paths(&view, cx),
        std::slice::from_ref(&library.moved_from)
    );
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.session.library().index_of(&library.deleted), None);
        assert!(!app.session.favorites().is_starred(&library.deleted));
    });
    let kept = kept_playlist_paths(&library.store_path);
    assert!(!kept.contains(&library.deleted));
    assert_eq!(kept.len(), 2);
    let kept_favorites = PlaylistStore::open(&library.store_path)
        .unwrap()
        .load_favorites()
        .unwrap();
    assert!(kept_favorites.tracks.is_empty());
}

#[gpui::test]
fn removing_leaves_a_track_whose_file_is_there(cx: &mut TestAppContext) {
    let library = open_with_missing_files("ui-missing-remove-present", cx);
    let view = library.view.clone();
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let before = app.session.library().tracks().len();
            app.session.add_demo_tracks(vec![crate::demo::DemoTrack {
                path: library.other_audio.clone(),
                outcome: Err("not analyzed in this test".to_string()),
            }]);
            app.selected = app.session.library().index_of(&library.other_audio);
            app.remove_missing_tracks_of_selection(cx);
            assert_eq!(app.session.library().tracks().len(), before + 1);
        })
    });
}

#[gpui::test]
fn removing_a_track_keeps_each_deck_on_its_own_track(cx: &mut TestAppContext) {
    let library = open_with_missing_files("ui-missing-decks", cx);
    let view = library.view.clone();
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.session.add_demo_tracks(vec![crate::demo::DemoTrack {
                path: library.other_audio.clone(),
                outcome: Err("not analyzed in this test".to_string()),
            }]);
            let on_deck = app.session.library().index_of(&library.other_audio);
            assert_eq!(on_deck, Some(2));
            let deck = app.decks.view_mut(crate::deck_audio::DeckId::A);
            deck.track_index = on_deck;
            deck.path = Some(library.other_audio.clone());

            app.selected = app.session.library().index_of(&library.deleted);
            app.remove_missing_tracks_of_selection(cx);

            let deck = app.decks.view(crate::deck_audio::DeckId::A);
            let index = deck.track_index.expect("the deck keeps its track");
            assert_eq!(index, 1);
            assert_eq!(
                app.session.library().track(index).unwrap().path,
                library.other_audio
            );
            let track_count = app.session.library().tracks().len();
            assert!(app.mixes.iter().all(|mix| mix.track_index < track_count));
            assert!(
                app.decks
                    .suggestions
                    .iter()
                    .all(|suggestion| suggestion.track_index < track_count)
            );
        })
    });
}

#[gpui::test]
fn a_file_that_cannot_be_checked_is_linked_only_once_confirmed(cx: &mut TestAppContext) {
    let library = open_with_missing_files("ui-missing-confirm", cx);
    let view = library.view.clone();
    let unrecorded = library.moved_to.with_file_name("never-recorded.mp3");
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.session.library_mut().add_missing([unrecorded.clone()]);
            app.locate_track(unrecorded.clone(), library.other_audio.clone(), cx);
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let app = view.read(cx);
        let pending = app.pending_link.as_ref().expect("the link waits for a yes");
        assert_eq!(pending.picked, library.other_audio);
        assert!(
            app.session
                .library()
                .track_by_path(&unrecorded)
                .unwrap()
                .is_missing()
        );
    });

    cx.update(|cx| view.update(cx, |app, cx| app.cancel_pending_link(cx)));
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.pending_link.is_none());
        assert!(
            app.session
                .library()
                .track_by_path(&unrecorded)
                .unwrap()
                .is_missing()
        );
    });

    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.locate_track(unrecorded.clone(), library.other_audio.clone(), cx)
        })
    });
    cx.run_until_parked();
    cx.update(|cx| view.update(cx, |app, cx| app.confirm_pending_link(cx)));
    cx.run_until_parked();
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.pending_link.is_none());
        assert_eq!(app.session.library().index_of(&unrecorded), None);
        assert!(
            app.session
                .library()
                .index_of(&library.other_audio)
                .is_some()
        );
    });
}

#[gpui::test]
fn a_link_against_recorded_audio_moves_nothing(cx: &mut TestAppContext) {
    let library = open_with_missing_files("ui-missing-conflict", cx);
    let view = library.view.clone();
    let other_content = euphoria_core::ContentHash::of_file(&library.other_audio).unwrap();
    cx.update(|cx| {
        view.update(cx, |app, _| {
            let relinked =
                app.session
                    .relink(&library.moved_from, &library.other_audio, other_content);
            assert_eq!(relinked, crate::state::Relinked::OtherAudioRecorded);
            assert!(
                app.session
                    .library()
                    .track_by_path(&library.moved_from)
                    .unwrap()
                    .is_missing()
            );
            assert!(
                app.session
                    .playlists()
                    .all()
                    .iter()
                    .any(|playlist| playlist.tracks.contains(&library.moved_from))
            );
        })
    });
}

#[gpui::test]
fn only_a_remembered_folder_stays_remembered_while_it_is_gone(cx: &mut TestAppContext) {
    let library = open_with_missing_files("ui-missing-unavailable", cx);
    let view = library.view.clone();
    let dropped = library.moved_to.with_file_name("vanished-before-the-scan");
    cx.update(|cx| view.update(cx, |app, cx| app.add_paths(vec![dropped.clone()], true, cx)));
    cx.run_until_parked();
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.unavailable_folders.is_empty());
        assert!(
            app.notice
                .as_deref()
                .is_some_and(|notice| notice.contains("is missing"))
        );
    });
}

#[gpui::test]
fn a_remembered_folder_that_is_gone_is_kept_for_the_next_launch(cx: &mut TestAppContext) {
    let gone =
        std::env::temp_dir().join(format!("euphoria-unplugged-drive-{}", std::process::id()));
    let settings = Settings {
        folders: vec![gone.clone()],
        ..Settings::default()
    };
    let view = cx.update(|cx| {
        gpui::init(cx);
        let (_, view) = gpui::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| {
                EuphoriaApp::new(
                    Session::start(None).unwrap(),
                    settings,
                    None,
                    None,
                    Box::new(SilentOutput),
                    window,
                    cx,
                )
            })
        })
        .unwrap();
        view
    });
    cx.run_until_parked();
    cx.update(|cx| assert_eq!(view.read(cx).unavailable_folders, vec![gone.clone()]));
}
