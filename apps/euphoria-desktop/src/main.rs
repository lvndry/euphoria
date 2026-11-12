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
mod columns;
mod cue_editing;
mod deck_audio;
mod decks;
mod demo;
mod display_settings;
mod dock_icon;
mod favorite_flow;
mod favorites;
#[cfg(test)]
mod favorites_ui_tests;
mod folder_tree;
mod format;
mod key_by_ear;
mod key_shift;
mod library;
mod mashup;
mod mixes;
mod panels;
mod peaks;
mod playback;
mod player;
mod playlist_flow;
mod playlists;
mod progress;
mod reference_sound;
mod set_builder;
#[cfg(test)]
mod set_builder_ui_tests;
mod settings;
mod state;
mod stems;
mod tag_editor;
mod tag_flow;
mod tag_settings;
mod tag_writing;
mod theme;
mod track_overview;
#[cfg(test)]
mod ui_tests;
mod views;
mod waveforms;

use std::path::PathBuf;
use std::sync::Arc;

use euphoria_core::{Cache, CachedAnalyzer, EuphoriaError, PlaylistStore};
use gpui::prelude::*;
use gpui::{
    AnyWindowHandle, App, Bounds, Context, Entity, KeyBinding, Menu, MenuItem, QuitMode,
    TitlebarOptions, WindowBounds, WindowOptions, point, px, size,
};

use crate::actions::{
    AddCueAtPlayhead, AddFolder, BuildSet, EditTags, FocusSearch, OpenSetBuilder, OpenSettings,
    Quit, ResetColumnWidths, SelectAllTracks, ToggleDecks, ToggleFavorite, TogglePlayback,
    ToggleSidebar, UndoCueEdit, UndoTagWrite, UseDeckTheme, UseGraphiteTheme, UseKeyFirstTheme,
    UsePaperTheme, UseSlateTheme, WriteTags,
};
use crate::app::EuphoriaApp;
use crate::playback::SystemOutput;
use crate::settings::Settings;
use crate::state::Session;
use crate::theme::ThemeName;

const WINDOW_TITLE: &str = "Euphoria";

/// The name Linux desktops match against `euphoria.desktop`, the launcher
/// entry the AppImage installs, to show the window with its name and icon.
const APP_ID: &str = "euphoria";

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
    let mut session = Session::start(cache)
        .map_err(|error| format!("cannot start the analysis workers: {error}"))?;
    let playlist_problem = euphoria_core::default_playlists_path()
        .and_then(|path| PlaylistStore::open(&path))
        .and_then(|store| session.use_playlist_store(store))
        .err()
        .map(|error| {
            format!(
                "Playlists cannot be kept, so changes to them are lost on quitting: {}.",
                error.chain()
            )
        });
    let problems: Vec<String> = settings_problem
        .into_iter()
        .chain(cache_problem)
        .chain(playlist_problem)
        .collect();
    let notice = (!problems.is_empty()).then(|| problems.join(" "));
    if let Some(count) = demo::requested_track_count() {
        session.add_demo_folder(PathBuf::from(demo::DEMO_FOLDER));
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
        app_id: Some(APP_ID.into()),
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
        KeyBinding::new("cmd-shift-d", ToggleDecks, None),
        KeyBinding::new("cmd-shift-b", OpenSetBuilder, None),
        KeyBinding::new("cmd-enter", BuildSet, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-b", OpenSetBuilder, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-enter", BuildSet, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-alt-s", ToggleSidebar, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-d", ToggleDecks, None),
        // Space plays and pauses, except in a text field, where it types.
        KeyBinding::new("space", TogglePlayback, Some(SPACE_BAR_CONTEXT)),
        KeyBinding::new("cmd-a", SelectAllTracks, Some(SPACE_BAR_CONTEXT)),
        // Outside a text field, where Command Z undoes typing.
        KeyBinding::new("cmd-z", UndoCueEdit, Some(SPACE_BAR_CONTEXT)),
        KeyBinding::new("c", AddCueAtPlayhead, Some(SPACE_BAR_CONTEXT)),
        KeyBinding::new("f", ToggleFavorite, Some(SPACE_BAR_CONTEXT)),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("cmd-shift-t", WriteTags, None),
        KeyBinding::new("cmd-i", EditTags, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-a", SelectAllTracks, Some(SPACE_BAR_CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-z", UndoCueEdit, Some(SPACE_BAR_CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-,", OpenSettings, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-t", WriteTags, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-i", EditTags, None),
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

    register_view_command::<WriteTags>(view, cx, |view, cx| view.open_tag_writer(cx));
    register_view_command::<ToggleDecks>(view, cx, |view, cx| view.toggle_decks(cx));
    register_view_command::<OpenSetBuilder>(view, cx, |view, cx| view.open_set_builder(cx));
    register_view_command::<BuildSet>(view, cx, |view, cx| {
        if view.set_builder.is_open && !view.set_builder.is_building {
            view.build_set(cx);
        }
    });
    register_view_command::<UndoTagWrite>(view, cx, |view, cx| view.open_tag_undo(cx));
    register_view_command::<OpenSettings>(view, cx, |view, cx| view.open_settings(cx));
    register_view_command::<SelectAllTracks>(view, cx, |view, cx| view.select_all_shown(cx));
    register_view_command::<AddCueAtPlayhead>(view, cx, |view, cx| view.add_cue_at_playhead(cx));
    register_view_command::<UndoCueEdit>(view, cx, |view, cx| view.undo_cue_edit(cx));
    register_view_command::<ToggleFavorite>(view, cx, |view, cx| {
        view.toggle_star_on_selection(cx);
    });
    register_view_command::<ResetColumnWidths>(view, cx, |view, cx| {
        view.reset_column_widths(cx);
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

    let weak_view = view.downgrade();
    cx.on_action(move |_: &EditTags, cx| {
        let weak_view = weak_view.clone();
        window
            .update(cx, |_, window, cx| {
                weak_view
                    .update(cx, |view, cx| view.open_tag_editor(window, cx))
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

/// Runs `command` on the root view when the action `Command` is triggered.
fn register_view_command<Command: gpui::Action>(
    view: &Entity<EuphoriaApp>,
    cx: &mut App,
    command: impl Fn(&mut EuphoriaApp, &mut Context<'_, EuphoriaApp>) + 'static,
) {
    let weak_view = view.downgrade();
    cx.on_action(move |_: &Command, cx| {
        weak_view.update(cx, |view, cx| command(view, cx)).ok();
    });
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
        Menu::new(WINDOW_TITLE).items([
            MenuItem::action("Settings\u{2026}", OpenSettings),
            MenuItem::separator(),
            MenuItem::action("Quit Euphoria", Quit),
        ]),
        Menu::new("File").items([MenuItem::action("Add Folder\u{2026}", AddFolder)]),
        Menu::new("View").items([
            MenuItem::action(sidebar_item, ToggleSidebar),
            MenuItem::action("Decks", ToggleDecks),
            MenuItem::action("Set Builder", OpenSetBuilder),
            MenuItem::separator(),
            MenuItem::action("Search", FocusSearch),
            MenuItem::action("Select All Tracks", SelectAllTracks),
            MenuItem::action("Star or Unstar Selection", ToggleFavorite),
            MenuItem::separator(),
            MenuItem::action("Reset Column Widths", ResetColumnWidths),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Theme").items(theme_items)),
        ]),
        Menu::new("Tags").items([
            MenuItem::action("Edit Tags\u{2026}", EditTags),
            MenuItem::action("Write Tags\u{2026}", WriteTags),
            MenuItem::action("Undo Last Tag Write\u{2026}", UndoTagWrite),
        ]),
        Menu::new("Playback").items([
            MenuItem::action("Play or Pause", TogglePlayback),
            MenuItem::separator(),
            MenuItem::action("Add Cue at Playhead", AddCueAtPlayhead),
            MenuItem::action("Undo Cue Edit", UndoCueEdit),
        ]),
    ]);
}
