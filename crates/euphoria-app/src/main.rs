// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Euphoria desktop app: add folders of music, watch each track get its
//! key and tempo, then sort and search the results.
//!
//! All analysis is done by `euphoria-core`. This crate turns what the user
//! does into core calls and core results into views.

mod actions;
mod analysis;
mod app;
mod assets;
mod camelot;
mod demo;
mod dock_icon;
mod format;
mod library;
mod mixes;
mod panels;
mod peaks;
mod playback;
mod player;
mod progress;
mod settings;
mod state;
mod theme;
#[cfg(test)]
mod ui_tests;
mod views;
mod waveforms;

use std::sync::Arc;

use euphoria_core::{Cache, CachedAnalyzer, EuphoriaError};
use gpui::prelude::*;
use gpui::{
    AnyWindowHandle, App, Bounds, Entity, KeyBinding, Menu, MenuItem, QuitMode, TitlebarOptions,
    WindowBounds, WindowOptions, point, px, size,
};

use crate::actions::{
    AddFolder, FocusSearch, Quit, TogglePlayback, ToggleSidebar, UseDeckTheme, UseGraphiteTheme,
    UseKeyFirstTheme, UsePaperTheme, UseSlateTheme,
};
use crate::app::EuphoriaApp;
use crate::playback::SystemOutput;
use crate::settings::Settings;
use crate::state::Session;
use crate::theme::ThemeName;

const WINDOW_TITLE: &str = "Euphoria";

/// Where the space bar belongs to the player: anywhere in the window but a
/// text field.
pub(crate) const SPACE_BAR_CONTEXT: &str = "Library && !Input";

/// Opens the analysis cache the command line tool also uses and reads what
/// it holds.
fn open_cache() -> Result<CachedAnalyzer, EuphoriaError> {
    let cache = Cache::open(&euphoria_core::default_cache_path()?)?;
    CachedAnalyzer::new(cache)
}

fn main() {
    gpui::application()
        .with_assets(assets::AppAssets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(|cx| {
            gpui::init(cx);
            dock_icon::show();
            if let Err(problem) = open_main_window(cx) {
                eprintln!("error: {problem}");
                cx.quit();
            }
        });
}

fn open_main_window(cx: &mut App) -> Result<(), String> {
    let settings_path = settings::settings_path();
    let (settings, settings_problem) = match settings_path.as_deref().map(Settings::load) {
        Some(Ok(settings)) => (settings, None),
        Some(Err(error)) => (
            Settings::default(),
            Some(format!("Could not read settings: {error}.")),
        ),
        None => (Settings::default(), None),
    };
    let (cache, cache_problem) = match open_cache() {
        Ok(cache) => (Some(Arc::new(cache)), None),
        Err(error) => (
            None,
            Some(format!(
                "Analyzing without the cache, so results will not be remembered: {}.",
                error.chain()
            )),
        ),
    };
    let problems: Vec<String> = settings_problem.into_iter().chain(cache_problem).collect();
    let notice = (!problems.is_empty()).then(|| problems.join(" "));

    let mut session = Session::start(cache)
        .map_err(|error| format!("cannot start the analysis workers: {error}"))?;
    if let Some(count) = demo::requested_track_count() {
        session.add_demo_tracks(demo::tracks(count));
    }

    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(1440.0), px(900.0)),
            cx,
        ))),
        window_min_size: Some(size(px(960.0), px(560.0))),
        titlebar: Some(TitlebarOptions {
            title: Some(WINDOW_TITLE.into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(14.0), px(15.0))),
        }),
        // The toolbar is the title bar: it moves the window itself.
        app_owns_titlebar_drag: true,
        ..Default::default()
    };
    let theme = settings.theme;
    let sidebar_folded = settings.panels.sidebar_folded;
    let (window, view) = gpui::open_window(options, cx, |window, cx| {
        cx.new(|cx| {
            let output = Box::new(SystemOutput::default());
            EuphoriaApp::new(session, settings, settings_path, notice, output, window, cx)
        })
    })
    .map_err(|error| format!("cannot open the window: {error:#}"))?;

    register_commands(window, &view, cx);
    install_menus(theme, sidebar_folded, cx);
    cx.activate(true);
    Ok(())
}

/// Connects menu items and keyboard shortcuts to the root view. The commands
/// are registered on the app, so they work wherever the keyboard focus is.
pub(crate) fn register_commands(window: AnyWindowHandle, view: &Entity<EuphoriaApp>, cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-o", AddFolder, None),
        KeyBinding::new("cmd-f", FocusSearch, None),
        KeyBinding::new("cmd-k", FocusSearch, None),
        // Control Command S is what macOS apps use to show and hide a sidebar.
        #[cfg(target_os = "macos")]
        KeyBinding::new("ctrl-cmd-s", ToggleSidebar, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-alt-s", ToggleSidebar, None),
        // Space plays and pauses, except in a text field, where it types.
        KeyBinding::new("space", TogglePlayback, Some(SPACE_BAR_CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-q", Quit, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-o", AddFolder, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-f", FocusSearch, None),
    ]);

    cx.on_action(|_: &Quit, cx| cx.quit());

    let weak_view = view.downgrade();
    cx.on_action(move |_: &TogglePlayback, cx| {
        weak_view
            .update(cx, |view, cx| view.toggle_playback(cx))
            .ok();
    });

    let weak_view = view.downgrade();
    cx.on_action(move |_: &ToggleSidebar, cx| {
        weak_view
            .update(cx, |view, cx| view.toggle_sidebar(cx))
            .ok();
    });

    let weak_view = view.downgrade();
    cx.on_action(move |_: &AddFolder, cx| {
        weak_view.update(cx, |view, cx| view.choose_folder(cx)).ok();
    });

    let weak_view = view.downgrade();
    cx.on_action(move |_: &FocusSearch, cx| {
        let weak_view = weak_view.clone();
        window
            .update(cx, |_, window, cx| {
                weak_view
                    .update(cx, |view, cx| view.focus_search(window, cx))
                    .ok();
            })
            .ok();
    });

    register_theme_command::<UseSlateTheme>(ThemeName::Slate, view, cx);
    register_theme_command::<UsePaperTheme>(ThemeName::Paper, view, cx);
    register_theme_command::<UseDeckTheme>(ThemeName::Deck, view, cx);
    register_theme_command::<UseGraphiteTheme>(ThemeName::Graphite, view, cx);
    register_theme_command::<UseKeyFirstTheme>(ThemeName::KeyFirst, view, cx);
}

fn register_theme_command<Command: gpui::Action>(
    theme: ThemeName,
    view: &Entity<EuphoriaApp>,
    cx: &mut App,
) {
    let weak_view = view.downgrade();
    cx.on_action(move |_: &Command, cx| {
        weak_view
            .update(cx, |view, cx| view.set_theme(theme, cx))
            .ok();
    });
}

/// Sets the menu bar, with a tick beside the theme in use and the sidebar
/// item worded for what it will do.
pub(crate) fn install_menus(current_theme: ThemeName, sidebar_folded: bool, cx: &mut App) {
    let sidebar_item = if sidebar_folded {
        "Show Sidebar"
    } else {
        "Hide Sidebar"
    };
    let theme_items = ThemeName::ALL.map(|theme| {
        let label = theme.label();
        let item = match theme {
            ThemeName::Slate => MenuItem::action(label, UseSlateTheme),
            ThemeName::Paper => MenuItem::action(label, UsePaperTheme),
            ThemeName::Deck => MenuItem::action(label, UseDeckTheme),
            ThemeName::Graphite => MenuItem::action(label, UseGraphiteTheme),
            ThemeName::KeyFirst => MenuItem::action(label, UseKeyFirstTheme),
        };
        item.checked(theme == current_theme)
    });
    cx.set_menus([
        Menu::new(WINDOW_TITLE).items([MenuItem::action("Quit Euphoria", Quit)]),
        Menu::new("File").items([MenuItem::action("Add Folder\u{2026}", AddFolder)]),
        Menu::new("View").items([
            MenuItem::action(sidebar_item, ToggleSidebar),
            MenuItem::separator(),
            MenuItem::action("Search", FocusSearch),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Theme").items(theme_items)),
        ]),
        Menu::new("Playback").items([MenuItem::action("Play or Pause", TogglePlayback)]),
    ]);
}
