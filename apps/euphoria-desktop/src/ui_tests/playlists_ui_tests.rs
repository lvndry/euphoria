// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// 10 tests: playlists in the sidebar and their export.

/// Three made up tracks for a set: A minor at 122, E minor at 124 and
/// F sharp major at 128, with energies 4, 6 and 8.
fn set_tracks() -> Vec<DemoTrack> {
    [
        ("opener", 9, euphoria_core::Mode::Minor, 122.0, 4),
        ("builder", 4, euphoria_core::Mode::Minor, 124.0, 6),
        ("peak", 6, euphoria_core::Mode::Major, 128.0, 8),
    ]
    .into_iter()
    .map(|(name, tonic, mode, bpm, energy)| DemoTrack {
        path: PathBuf::from(format!("/Demo/set/{name}.mp3")),
        outcome: Ok(TrackAnalysis {
            duration_seconds: 300.0,
            key: Some(euphoria_core::Key::new(tonic, mode)),
            bpm: Some(bpm),
            energy: Some(energy),
            tuning_cents: None,
            key_runner_up: None,
            key_score_margin: None,
            key_source: None,
            loudness_range_lu: None,
            pitch_class_profile: None,
            tempo_estimate: None,
            tempo_score_margin: None,
            tempo_octave_evidence: None,
            key_timeline: Vec::new(),
            quality: None,
            beat_grid: None,
            cues: Vec::new(),
        }),
    })
    .collect()
}

fn open_set(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_window_on(set_tracks(), Box::new(SilentOutput), cx)
}

/// Makes a playlist named `name` holding the set's tracks named `tracks`,
/// in that order.
fn make_playlist(
    window: AnyWindowHandle,
    view: &Entity<EuphoriaApp>,
    name: &str,
    tracks: &[&str],
    cx: &mut TestAppContext,
) -> crate::playlists::PlaylistId {
    let paths: Vec<PathBuf> = tracks
        .iter()
        .map(|track| PathBuf::from(format!("/Demo/set/{track}.mp3")))
        .collect();
    let id = cx
        .update_window(window, |_, window, cx| {
            view.update(cx, |app, cx| app.create_playlist(window, cx))
        })
        .unwrap();
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.renaming_playlist = None;
            app.session.playlists_mut().rename(id, name);
            app.add_to_playlist(id, &paths, cx);
        })
    });
    cx.run_until_parked();
    id
}

fn playlist_names(
    view: &Entity<EuphoriaApp>,
    id: crate::playlists::PlaylistId,
    cx: &mut TestAppContext,
) -> Vec<String> {
    cx.update(|cx| {
        view.read(cx)
            .session
            .playlists()
            .get(id)
            .unwrap()
            .tracks
            .iter()
            .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
            .collect()
    })
}

fn shown_stems(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<String> {
    shown_titles(view, cx)
        .into_iter()
        .map(|title| title.trim_end_matches(".mp3").to_string())
        .collect()
}

#[gpui::test]
fn a_new_playlist_is_named_in_the_sidebar_and_shown(cx: &mut TestAppContext) {
    let (window, view) = open_set(cx);
    click(window, "new-playlist", cx);
    assert!(is_shown(window, "playlist-name", cx));
    cx.update_window(window, |_, window, cx| {
        window.press(
            if cfg!(target_os = "macos") {
                "cmd-a"
            } else {
                "ctrl-a"
            },
            cx,
        );
        window.input("Opening set", cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    let id = cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.renaming_playlist, None);
        let playlist = &app.session.playlists().all()[0];
        assert_eq!(playlist.name, "Opening set");
        assert_eq!(app.playlist, Some(playlist.id));
        assert!(app.visible.is_empty());
        playlist.id
    });
    assert!(!is_shown(window, "playlist-name", cx));
    assert!(is_shown(
        window,
        crate::views::playlists::playlist_row_id(id),
        cx
    ));

    click(window, "all-tracks", cx);
    cx.update(|cx| assert_eq!(view.read(cx).playlist, None));
    click(window, crate::views::playlists::playlist_row_id(id), cx);
    cx.update(|cx| assert_eq!(view.read(cx).playlist, Some(id)));
}

#[gpui::test]
fn tracks_dragged_onto_a_playlist_join_it_once_in_table_order(cx: &mut TestAppContext) {
    let (window, view) = open_set(cx);
    let id = make_playlist(window, &view, "Set", &[], cx);
    click(window, "all-tracks", cx);
    let index_of = |name: &str, cx: &mut TestAppContext| {
        cx.update(|cx| {
            view.read(cx)
                .session
                .library()
                .index_of(Path::new(&format!("/Demo/set/{name}.mp3")))
                .unwrap()
        })
    };
    let peak = index_of("peak", cx);
    let opener = index_of("opener", cx);
    let row = crate::views::playlists::playlist_row_id(id);

    click(window, track(opener), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click_with_modifiers(track(peak), Modifiers::secondary_key(cx), cx);
        window.render_frame(cx);
        window.drag_to(track(peak), row, cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(playlist_names(&view, id, cx), ["opener", "peak"]);

    let builder = index_of("builder", cx);
    click(window, track(builder), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.drag_to(track(builder), row, cx);
        window.render_frame(cx);
        window.drag_to(track(builder), row, cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(playlist_names(&view, id, cx), ["opener", "peak", "builder"]);
}

#[gpui::test]
fn a_playlist_numbers_its_tracks_in_order_and_sorting_is_only_a_view(cx: &mut TestAppContext) {
    let (window, view) = open_set(cx);
    let id = make_playlist(window, &view, "Set", &["peak", "opener", "builder"], cx);
    assert_eq!(shown_stems(&view, cx), ["peak", "opener", "builder"]);
    let numbers = cx.update(|cx| {
        let app = view.read(cx);
        app.visible
            .iter()
            .map(|index| app.playlist_positions[index])
            .collect::<Vec<_>>()
    });
    assert_eq!(numbers, [1, 2, 3]);

    click(window, ("column", BPM_COLUMN), cx);
    assert_eq!(shown_stems(&view, cx), ["opener", "builder", "peak"]);
    assert_eq!(playlist_names(&view, id, cx), ["peak", "opener", "builder"]);
    click(window, header_id(ColumnId::Number), cx);
    assert_eq!(shown_stems(&view, cx), ["peak", "opener", "builder"]);

    let (builder, peak) = cx.update(|cx| {
        let app = view.read(cx);
        (app.visible[2], app.visible[0])
    });
    click(window, track(builder), cx);
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.drag_to(track(builder), track(peak), cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(playlist_names(&view, id, cx), ["builder", "peak", "opener"]);
    assert_eq!(shown_stems(&view, cx), ["builder", "peak", "opener"]);

    cx.update(|cx| view.update(cx, |app, cx| app.remove_selection_from_playlist(cx)));
    assert_eq!(playlist_names(&view, id, cx), ["peak", "opener"]);
}

#[gpui::test]
fn the_set_line_shows_the_key_path_the_tempos_the_energy_and_export(cx: &mut TestAppContext) {
    let (window, view) = open_set(cx);
    make_playlist(window, &view, "Set", &["opener", "builder", "peak"], cx);
    for id in [
        "set-key-path",
        "set-tempo-range",
        "set-energy",
        "export-rekordbox",
        "export-m3u8",
    ] {
        assert!(is_shown(window, id, cx), "{id}");
    }
    click(window, "all-tracks", cx);
    assert!(!is_shown(window, "set-key-path", cx));
}

#[gpui::test]
fn a_playlist_exports_in_its_order_to_rekordbox_and_m3u8(cx: &mut TestAppContext) {
    let (window, view) = open_set(cx);
    let id = make_playlist(window, &view, "Peak time", &["peak", "opener"], cx);
    let scratch = ScratchFolder::new("ui-playlist-export");
    let write = |format, file: &str, cx: &mut TestAppContext| {
        let job = cx.update(|cx| view.read(cx).playlist_export_job(id, format).unwrap());
        let destination = scratch.0.join(file);
        let message = job.write(&destination).unwrap().as_text();
        (std::fs::read_to_string(&destination).unwrap(), message)
    };

    let (m3u8, _) = write(crate::playlist_flow::PlaylistFormat::M3u8, "set.m3u8", cx);
    assert_eq!(
        m3u8,
        "#EXTM3U\n#PLAYLIST:Peak time\n#EXTINF:300,peak.mp3\n/Demo/set/peak.mp3\n\
         #EXTINF:300,opener.mp3\n/Demo/set/opener.mp3\n"
    );

    let (xml, message) = write(
        crate::playlist_flow::PlaylistFormat::Rekordbox,
        "set.xml",
        cx,
    );
    assert!(xml.contains("Name=\"Peak time\""), "{xml}");
    assert_eq!(xml.matches("<TRACK TrackID").count(), 2);
    assert!(
        xml.find("peak.mp3").unwrap() < xml.find("opener.mp3").unwrap(),
        "{xml}"
    );
    assert!(message.contains("rekordbox"), "{message}");
}

#[gpui::test]
fn a_playlist_export_uses_the_key_and_tempo_corrected_by_the_user(cx: &mut TestAppContext) {
    let (window, view) = open_set(cx);
    let id = make_playlist(window, &view, "Corrected set", &["peak"], cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let index = app
                .session
                .library()
                .index_of(Path::new("/Demo/set/peak.mp3"))
                .unwrap();
            app.select(index, cx);
            app.accept_key(euphoria_core::Key::new(4, euphoria_core::Mode::Minor), cx);
            app.accept_bpm(87.0, cx);
        })
    });
    let scratch = ScratchFolder::new("ui-playlist-correction");
    let destination = scratch.0.join("set.xml");
    let job = cx.update(|cx| {
        view.read(cx)
            .playlist_export_job(id, crate::playlist_flow::PlaylistFormat::Rekordbox)
            .unwrap()
    });
    job.write(&destination).unwrap();
    let xml = std::fs::read_to_string(destination).unwrap();
    assert!(xml.contains("Tonality=\"9A\""), "{xml}");
    assert!(xml.contains("AverageBpm=\"87.00\""), "{xml}");
}

#[gpui::test]
fn deleting_the_playlist_on_show_goes_back_to_all_tracks(cx: &mut TestAppContext) {
    let (window, view) = open_set(cx);
    let id = make_playlist(window, &view, "Set", &["peak"], cx);
    cx.update(|cx| view.update(cx, |app, cx| app.delete_playlist(id, cx)));
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.playlist, None);
        assert!(app.session.playlists().all().is_empty());
        assert_eq!(app.visible.len(), 3);
    });
}

/// Scratch copies of the test files in a window that keeps its playlists
/// in a real store in the scratch folder.
pub(crate) fn open_with_playlist_store(
    name: &str,
    cx: &mut TestAppContext,
) -> (
    ScratchFolder,
    Vec<PathBuf>,
    PathBuf,
    AnyWindowHandle,
    Entity<EuphoriaApp>,
) {
    let scratch = ScratchFolder::new(name);
    let files = scratch.copy_fixtures();
    let store_path = scratch.0.join("playlists.sqlite3");
    let tracks: Vec<DemoTrack> = fixture_tracks()
        .into_iter()
        .zip(&files)
        .map(|(track, file)| DemoTrack {
            path: file.clone(),
            outcome: track.outcome,
        })
        .collect();
    let (window, view) = cx.update(|cx| {
        gpui::init(cx);
        let (window, view) = gpui::open_window(WindowOptions::default(), cx, |window, cx| {
            cx.new(|cx| {
                let mut session = Session::start(None).unwrap();
                session.add_demo_tracks(tracks);
                session
                    .use_playlist_store(euphoria_core::PlaylistStore::open(&store_path).unwrap())
                    .unwrap();
                EuphoriaApp::new(
                    session,
                    Settings::default(),
                    None,
                    vec![],
                    Box::new(SilentOutput),
                    window,
                    cx,
                )
            })
        })
        .unwrap();
        crate::register_commands(window, &view, cx);
        (window, view)
    });
    (scratch, files, store_path, window, view)
}

#[gpui::test]
fn playlists_are_kept_and_found_again_on_the_next_launch(cx: &mut TestAppContext) {
    let (_scratch, files, store_path, window, view) =
        open_with_playlist_store("ui-playlist-store", cx);
    let id = cx
        .update_window(window, |_, window, cx| {
            view.update(cx, |app, cx| app.create_playlist(window, cx))
        })
        .unwrap();
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.renaming_playlist = None;
            app.session.playlists_mut().rename(id, "Kept");
            app.set_playlist_label(id, Some(crate::playlists::LABELS[2]), cx);
            app.add_to_playlist(id, &[files[2].clone(), files[0].clone()], cx);
        })
    });
    cx.run_until_parked();

    let kept = euphoria_core::PlaylistStore::open(&store_path)
        .unwrap()
        .load()
        .unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].name, "Kept");
    assert_eq!(kept[0].label.as_deref(), Some(crate::playlists::LABELS[2]));
    let kept_paths: Vec<&Path> = kept[0]
        .entries
        .iter()
        .map(|entry| entry.path.as_path())
        .collect();
    assert_eq!(kept_paths, [files[2].as_path(), files[0].as_path()]);
}

#[gpui::test]
fn edits_after_a_playlist_was_saved_are_all_kept(cx: &mut TestAppContext) {
    let (_scratch, files, store_path, window, view) =
        open_with_playlist_store("ui-playlist-later-edits", cx);
    let id = cx
        .update_window(window, |_, window, cx| {
            view.update(cx, |app, cx| {
                let id = app.create_playlist(window, cx);
                app.renaming_playlist = None;
                id
            })
        })
        .unwrap();
    cx.run_until_parked();
    let load = || {
        euphoria_core::PlaylistStore::open(&store_path)
            .unwrap()
            .load()
            .unwrap()
            .remove(0)
    };
    assert!(load().entries.is_empty());
    cx.update(|cx| view.update(cx, |app, cx| app.add_to_playlist(id, &files[..2], cx)));
    cx.run_until_parked();
    assert_eq!(load().entries.len(), 2);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.session.playlists_mut().rename(id, "Renamed after save");
            app.save_playlists(cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(load().name, "Renamed after save");
    cx.update(|cx| view.update(cx, |app, cx| app.set_playlist_label(id, Some("🔥"), cx)));
    cx.run_until_parked();
    assert_eq!(load().label.as_deref(), Some("🔥"));
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.session
                .playlists_mut()
                .move_tracks(id, &[files[1].clone()], 0);
            app.save_playlists(cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(load().entries[0].path, files[1]);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.session
                .playlists_mut()
                .remove_tracks(id, &[files[0].clone()]);
            app.save_playlists(cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(load().entries.len(), 1);
    assert_eq!(load().entries[0].path, files[1]);
}
