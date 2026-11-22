// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

// 2 tests: playing and removing the separated stems.

/// The selected track of the fixture library, marked as separated into
/// four copies of a stereo test file in `folder`.
fn separate_selected_track(
    view: &Entity<EuphoriaApp>,
    folder: &ScratchFolder,
    cx: &mut TestAppContext,
) -> euphoria_core::stems::StemFiles {
    open_sections(view, &[crate::inspector_sections::SectionKind::Stems], cx);
    let stereo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/euphoria-core/tests/fixtures/sine-440hz-stereo.wav");
    let files = euphoria_core::stems::StemFiles::in_directory(folder.0.join("stems"));
    std::fs::create_dir_all(files.directory()).unwrap();
    for stem in euphoria_core::stems::Stem::ALL {
        std::fs::copy(&stereo, files.path(stem)).unwrap();
    }
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let path = app.selected_track().unwrap().path.clone();
            app.stems.mark_ready(&path, files.clone());
            app.stems_changed(cx);
        })
    });
    files
}

#[gpui::test]
fn the_full_track_plays_the_file_and_acapella_or_instrumental_the_stems(cx: &mut TestAppContext) {
    let folder = ScratchFolder::new("stem-modes");
    let (_, view) = open_fixture_library(cx);
    separate_selected_track(&view, &folder, cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            assert_eq!(app.stems.mode(), crate::stems::StemMode::Full);
            assert!(!app.player.offers_stems());
            match app.player.toggle() {
                Some(PlayerRequest::Start {
                    with_stems: false, ..
                }) => {}
                other => panic!("the full track should play the file, got {other:?}"),
            }

            app.player.advance_to(0.4);
            app.set_stem_mode(crate::stems::StemMode::Acapella, cx);
            assert!(app.player.offers_stems());
            assert_eq!(app.player.status(), &PlayerStatus::Starting);

            app.set_stem_mode(crate::stems::StemMode::Instrumental, cx);
            assert!(app.player.offers_stems());
            assert_eq!(app.player.status(), &PlayerStatus::Starting);

            app.set_stem_mode(crate::stems::StemMode::Full, cx);
            assert!(!app.player.offers_stems());
            assert_eq!(app.player.status(), &PlayerStatus::Starting);
        })
    });
}

#[gpui::test]
fn remove_stems_deletes_them_and_offers_to_separate_again(cx: &mut TestAppContext) {
    let folder = ScratchFolder::new("stem-remove");
    let (window, view) = open_fixture_library(cx);
    let files = separate_selected_track(&view, &folder, cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.set_stem_mode(crate::stems::StemMode::Acapella, cx)
        })
    });
    assert!(is_shown(window, "stems-remove", cx));
    assert!(!is_shown(window, "stems-separate", cx));

    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.click("stems-remove", cx);
    })
    .unwrap();

    assert!(!files.directory().exists());
    cx.update(|cx| {
        let app = view.read(cx);
        let path = app.selected_track().unwrap().path.clone();
        assert_eq!(app.stems.status(&path), None);
        assert!(!app.player.offers_stems());
    });
    assert!(is_shown(window, "stems-separate", cx));
    assert!(!is_shown(window, "stems-remove", cx));
}
