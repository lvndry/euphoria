// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The stems mode setting through the real root view in a headless window:
//! the choice in the settings, what it saves, and the offer to separate a
//! track again in Advanced mode.

use std::path::PathBuf;

use euphoria_core::stems::{SeparationMode, StemFiles};
use gpui::{AnyWindowHandle, Entity, TestAppContext, px, size};

use crate::app::EuphoriaApp;
use crate::playback::testing::SilentOutput;
use crate::settings::Settings;
use crate::stem_settings::StemSettings;
use crate::tag_writing::testing::ScratchFolder;
use crate::ui_tests::{click, fixture_tracks, is_shown, open_sections, open_window_with};

fn open_with(
    stems: StemSettings,
    settings_path: Option<PathBuf>,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    let settings = Settings {
        stems,
        ..Settings::default()
    };
    open_window_with(
        fixture_tracks(),
        Box::new(SilentOutput),
        settings,
        settings_path,
        size(px(1440.0), px(2600.0)),
        cx,
    )
}

fn stems_of(mode: SeparationMode, folder: &ScratchFolder) -> StemFiles {
    StemFiles::in_directory(
        folder
            .0
            .join(format!("{}-{}", mode.model_id(), "ab".repeat(32))),
    )
}

fn open_with_stems_made_by(
    setting: SeparationMode,
    made_by: SeparationMode,
    folder: &ScratchFolder,
    cx: &mut TestAppContext,
) -> AnyWindowHandle {
    let (window, view) = open_with(StemSettings::for_mode(setting), None, cx);
    cx.update(|cx| view.update(cx, |app, cx| app.select(0, cx)));
    open_sections(&view, &[crate::inspector_sections::SectionKind::Stems], cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.stems = crate::stems::StemSession::new(|_| {
                Err(euphoria_core::EuphoriaError::NoDataDirectory)
            });
            app.stems.set_separation_mode(setting);
            let path = app.selected_track().unwrap().path.clone();
            app.stems.mark_ready(&path, stems_of(made_by, folder));
            cx.notify();
        })
    });
    window
}

#[gpui::test]
fn the_settings_offer_the_two_modes_and_show_the_one_in_use(cx: &mut TestAppContext) {
    let (window, view) = open_with(StemSettings::default(), None, cx);
    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));
    assert!(is_shown(window, ("separation-mode", 0_usize), cx));
    assert!(is_shown(window, ("separation-mode", 1_usize), cx));
    assert!(!is_shown(window, ("separation-mode", 2_usize), cx));
    assert!(is_shown(window, "hint-info-separation-mode", cx));
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).stems.separation_mode(),
            SeparationMode::Default
        )
    });
}

#[gpui::test]
fn choosing_advanced_mode_is_saved_and_the_session_follows(cx: &mut TestAppContext) {
    let folder = ScratchFolder::new("stems-setting-saved");
    let path = folder.0.join("settings.json");
    let (window, view) = open_with(StemSettings::default(), Some(path.clone()), cx);
    cx.update(|cx| view.update(cx, |app, cx| app.open_settings(cx)));

    click(window, ("separation-mode", 1_usize), cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.stem_settings.mode(), SeparationMode::Advanced);
        assert_eq!(app.stems.separation_mode(), SeparationMode::Advanced);
    });
    let saved = Settings::load(&path).unwrap();
    assert_eq!(saved.stems.mode(), SeparationMode::Advanced);

    click(window, ("separation-mode", 0_usize), cx);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).stems.separation_mode(),
            SeparationMode::Default
        )
    });
    let saved = Settings::load(&path).unwrap();
    assert_eq!(saved.stems.mode(), SeparationMode::Default);
    assert!(
        !std::fs::read_to_string(&path)
            .unwrap()
            .contains(r#""advanced""#)
    );
}

#[gpui::test]
fn a_remembered_advanced_mode_is_in_use_after_a_restart(cx: &mut TestAppContext) {
    let (_, view) = open_with(StemSettings::for_mode(SeparationMode::Advanced), None, cx);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).stems.separation_mode(),
            SeparationMode::Advanced
        )
    });
}

#[gpui::test]
fn default_stems_with_advanced_mode_on_offer_to_separate_again(cx: &mut TestAppContext) {
    let folder = ScratchFolder::new("stems-upgrade-offered");
    let window = open_with_stems_made_by(
        SeparationMode::Advanced,
        SeparationMode::Default,
        &folder,
        cx,
    );
    assert!(is_shown(window, "stems-advanced", cx));
    assert!(is_shown(window, "stems-remove", cx));
}

#[gpui::test]
fn no_offer_for_advanced_stems_or_in_default_mode(cx: &mut TestAppContext) {
    let folder = ScratchFolder::new("stems-upgrade-not-offered");
    let window = open_with_stems_made_by(
        SeparationMode::Advanced,
        SeparationMode::Advanced,
        &folder,
        cx,
    );
    assert!(is_shown(window, "stems-remove", cx));
    assert!(!is_shown(window, "stems-advanced", cx));

    let window = open_with_stems_made_by(
        SeparationMode::Default,
        SeparationMode::Default,
        &folder,
        cx,
    );
    assert!(is_shown(window, "stems-remove", cx));
    assert!(!is_shown(window, "stems-advanced", cx));
}

#[gpui::test]
fn separating_again_in_advanced_mode_keeps_the_default_stems_on_disk(cx: &mut TestAppContext) {
    let folder = ScratchFolder::new("stems-upgrade-keeps");
    let default = stems_of(SeparationMode::Default, &folder);
    std::fs::create_dir_all(default.directory()).unwrap();
    let window = open_with_stems_made_by(
        SeparationMode::Advanced,
        SeparationMode::Default,
        &folder,
        cx,
    );
    click(window, "stems-advanced", cx);
    assert!(default.directory().is_dir());
}
