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
mod artwork;
#[cfg(test)]
mod artwork_ui_tests;
mod assets;
mod auto_tags;
#[cfg(test)]
mod auto_tags_ui_tests;
mod busy;
#[cfg(test)]
mod busy_ui_tests;
mod camelot;
mod columns;
mod command_palette;
#[cfg(test)]
mod command_palette_ui_tests;
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
mod first_run;
#[cfg(test)]
mod first_run_ui_tests;
mod focus;
#[cfg(test)]
mod focus_ui_tests;
mod folder_tree;
mod format;
mod inspector_sections;
mod journey;
mod key_by_ear;
mod key_shift;
mod label_preview;
mod library;
mod library_export;
#[cfg(test)]
mod library_export_ui_tests;
mod live_app;
mod live_model;
mod live_settings;
#[cfg(test)]
mod live_show_tests;
#[cfg(test)]
mod live_ui_tests;
mod logging;
mod login_item;
mod logo_motion;
mod mashup;
#[cfg(test)]
mod mashup_ui_tests;
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
mod media_keys;
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]
mod media_keys_flow;
#[cfg(target_os = "macos")]
mod media_keys_macos;
#[cfg(test)]
mod media_keys_ui_tests;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod menu_bar;
#[cfg(target_os = "macos")]
mod menu_bar_macos;
mod metronome;
mod metronome_controls;
#[cfg(test)]
mod metronome_ui_tests;
#[cfg(test)]
mod missing_ui_tests;
mod mixes;
mod motion;
#[cfg(test)]
mod motion_ui_tests;
mod next_rail;
#[cfg(test)]
mod next_rail_ui_tests;
#[cfg(test)]
mod note_strip_ui_tests;
mod output_stream;
mod palette_flow;
mod panel_frame;
mod panels;
mod peaks;
mod planner_candidates;
mod planner_playback;
#[cfg(test)]
mod planner_ui_tests;
mod playback;
mod player;
#[cfg(test)]
mod player_bar_ui_tests;
mod player_wave;
#[cfg(test)]
mod player_wave_ui_tests;
mod playlist_flow;
mod playlists;
mod prepare_files;
#[cfg(test)]
mod prepare_files_ui_tests;
mod progress;
mod reanalyze;
#[cfg(test)]
mod reanalyze_ui_tests;
mod reference_sound;
mod relink_flow;
mod reveal;
#[cfg(test)]
mod row_actions_ui_tests;
mod set_builder;
#[cfg(test)]
mod set_builder_ui_tests;
#[cfg(test)]
mod set_stage_ui_tests;
mod settings;
mod show_in_library;
mod sidebar_slide;
mod state;
mod stem_export;
#[cfg(test)]
mod stem_export_ui_tests;
mod stems;
#[cfg(test)]
mod suggestion_filter_ui_tests;
mod surface_motion;
#[cfg(test)]
mod tag_clarity_ui_tests;
mod tag_editor;
mod tag_flow;
mod tag_reads;
mod tag_settings;
mod tag_writing;
mod tempo_controls;
#[cfg(test)]
mod tempo_ui_tests;
mod theme;
mod toast;
mod toast_flow;
#[cfg(test)]
mod toast_ui_tests;
mod track_filter_flow;
#[cfg(test)]
mod track_filter_ui_tests;
mod track_filters;
mod track_overview;
mod transition_plan;
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
    AddCueAtPlayhead, AddFolder, AddToSet, AnalyzeAgain, BuildSet, CloseFocusView, EditTags,
    ExportLibrary, FocusSearch, OpenCommandPalette, OpenEuphoria, OpenFocusView, OpenLive,
    OpenSetBuilder, OpenSettings, PlanTransition, PlayNextRow, PlayPaletteTrack, Quit,
    ResetColumnWidths, RevealTrack, SelectAllTracks, SeparateStems, StopAnalysis, ToggleDecks,
    ToggleFavorite, ToggleLiveKeepRunning, ToggleLiveListening, ToggleLiveOpenAtLogin,
    ToggleMetronome, TogglePlayback, ToggleSidebar, UndoCueEdit, UndoTagWrite, UseDeckTheme,
    UseGraphiteTheme, UsePaperTheme, UseSlateTheme, WriteTags,
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

/// Everywhere but the command palette.
const NOT_IN_PALETTE_CONTEXT: &str = "!CommandPalette";

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
    logging::init();
    let live_only = std::env::args().any(|argument| argument == "--live");
    gpui::application()
        .with_assets(assets::AppAssets)
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx| {
            gpui::init(cx);
            dock_icon::show();
            cx.on_window_closed(|cx, _| quit_unless_live_stays(cx))
                .detach();
            register_global_commands(cx);
            let opened_at_login = login_item::launched_as_login_item()
                && live_settings::live_settings_path()
                    .map(|path| live_settings::LiveSettings::load(&path))
                    .unwrap_or_default()
                    .open_at_login;
            if live_only || opened_at_login {
                install_menus(ThemeName::default(), false, cx);
                cx.set_activation_policy(gpui::ActivationPolicy::Accessory);
                live_app::open_panel(cx);
            } else if let Err(problem) = open_main_window(cx) {
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

    cx.set_global(MainWindow(Some(window)));
    live_app::remember_main_view(&view, cx);
    cx.set_activation_policy(gpui::ActivationPolicy::Regular);
    register_commands(window, &view, cx);
    install_menus(theme, sidebar_folded, cx);
    #[cfg(target_os = "macos")]
    {
        let (surface, commands) = media_keys_macos::connect();
        media_keys::install(&view, Box::new(surface), commands, cx);
    }
    cx.activate(true);
    Ok(())
}

/// The commands that do not need the window of the library, registered once
/// so they work while only Live runs.
fn register_global_commands(cx: &mut App) {
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.on_action(|_: &OpenLive, cx| live_app::open_panel(cx));
    cx.on_action(|_: &ToggleLiveListening, cx| live_app::toggle_listening(cx));
    cx.on_action(|_: &ToggleLiveKeepRunning, cx| live_app::toggle_keep_running(cx));
    cx.on_action(|_: &ToggleLiveOpenAtLogin, cx| live_app::toggle_open_at_login(cx));
    cx.on_action(|_: &OpenEuphoria, cx| open_euphoria(cx));
}

/// The window of the library, while it is open.
struct MainWindow(Option<AnyWindowHandle>);

impl gpui::Global for MainWindow {}

fn main_window_is_open(cx: &App) -> bool {
    cx.try_global::<MainWindow>()
        .and_then(|main| main.0)
        .is_some_and(|window| cx.windows().contains(&window))
}

/// Quits when the window of the library is closed, unless Live runs on in the
/// menu bar. The Live panel alone never keeps the app open.
fn quit_unless_live_stays(cx: &mut App) {
    if main_window_is_open(cx) {
        return;
    }
    if live_app::keeps_running(cx) {
        cx.set_activation_policy(gpui::ActivationPolicy::Accessory);
    } else {
        cx.quit();
    }
}

/// Shows the window of the library: the one open, or a new one, as the menu
/// bar item of Live asks. Nothing of the library is kept while only Live runs,
/// so a new window reads it again.
pub(crate) fn open_euphoria(cx: &mut App) {
    if main_window_is_open(cx) {
        if let Some(window) = cx.global::<MainWindow>().0 {
            window
                .update(cx, |_, window, _| window.activate_window())
                .ok();
        }
        cx.activate(true);
        return;
    }
    if let Err(problem) = open_main_window(cx) {
        eprintln!("error: {problem}");
    }
}

/// Connects menu items and keyboard shortcuts to the root view. The commands
/// are registered on the app, so they work wherever the keyboard focus is.
pub(crate) fn register_commands(window: AnyWindowHandle, view: &Entity<EuphoriaApp>, cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-o", AddFolder, None),
        KeyBinding::new("cmd-f", FocusSearch, None),
        KeyBinding::new("cmd-k", OpenCommandPalette, None),
        KeyBinding::new("cmd-p", OpenCommandPalette, None),
        KeyBinding::new(
            "cmd-enter",
            PlayPaletteTrack,
            Some(views::PALETTE_KEY_CONTEXT),
        ),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new(
            "ctrl-enter",
            PlayPaletteTrack,
            Some(views::PALETTE_KEY_CONTEXT),
        ),
        // Control Command S is what macOS apps use to show and hide a sidebar.
        #[cfg(target_os = "macos")]
        KeyBinding::new("ctrl-cmd-s", ToggleSidebar, None),
        KeyBinding::new("cmd-shift-d", ToggleDecks, None),
        KeyBinding::new("cmd-shift-l", OpenLive, None),
        KeyBinding::new("cmd-shift-b", OpenSetBuilder, None),
        // Command Enter plays a track in the palette, which has its own binding.
        KeyBinding::new("cmd-enter", BuildSet, Some(NOT_IN_PALETTE_CONTEXT)),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-b", OpenSetBuilder, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-enter", BuildSet, Some(NOT_IN_PALETTE_CONTEXT)),
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
        KeyBinding::new("s", ToggleFavorite, Some(SPACE_BAR_CONTEXT)),
        KeyBinding::new("a", AddToSet, Some(SPACE_BAR_CONTEXT)),
        KeyBinding::new("m", ToggleMetronome, Some(SPACE_BAR_CONTEXT)),
        // F stars, so Enter opens the focus view.
        KeyBinding::new("enter", OpenFocusView, Some(SPACE_BAR_CONTEXT)),
        // In the list of tracks to play next, Enter plays the one selected.
        KeyBinding::new("enter", PlayNextRow, Some(views::NEXT_RAIL_KEY_CONTEXT)),
        KeyBinding::new("cmd-shift-r", RevealTrack, Some(SPACE_BAR_CONTEXT)),
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
        KeyBinding::new("ctrl-k", OpenCommandPalette, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-p", OpenCommandPalette, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-q", Quit, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-o", AddFolder, None),
        #[cfg(not(target_os = "macos"))]
        KeyBinding::new("ctrl-shift-r", RevealTrack, Some(SPACE_BAR_CONTEXT)),
        KeyBinding::new("ctrl-f", FocusSearch, None),
    ]);

    let weak_view = view.downgrade();
    cx.on_action(move |_: &TogglePlayback, cx| {
        weak_view
            .update(cx, |view, cx| view.play_or_pause_selection(cx))
            .ok();
    });

    let weak_view = view.downgrade();
    cx.on_action(move |_: &ToggleSidebar, cx| {
        weak_view
            .update(cx, |view, cx| view.toggle_sidebar(cx))
            .ok();
    });

    register_view_command::<PlayNextRow>(view, cx, |view, cx| view.play_next_row(cx));
    register_view_command::<ExportLibrary>(view, cx, |view, cx| view.open_library_export(cx));
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
    register_view_command::<UndoCueEdit>(view, cx, |view, cx| view.undo_latest(cx));
    register_view_command::<ToggleMetronome>(view, cx, |view, cx| view.toggle_metronome(cx));
    register_view_command::<ToggleFavorite>(view, cx, |view, cx| {
        view.toggle_star_on_selection(cx);
    });
    register_view_command::<AddToSet>(view, cx, |view, cx| view.add_selection_to_set(cx));
    register_view_command::<CloseFocusView>(view, cx, |view, cx| view.close_focus_view(cx));
    register_view_command::<ResetColumnWidths>(view, cx, |view, cx| {
        view.reset_column_widths(cx);
    });
    register_view_command::<RevealTrack>(view, cx, |view, _| {
        view.reveal_selected_track();
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

    register_view_command::<AnalyzeAgain>(view, cx, |view, cx| view.analyze_selection_again(cx));
    register_view_command::<StopAnalysis>(view, cx, |view, cx| view.stop_analysis(cx));
    register_view_command::<SeparateStems>(view, cx, |view, cx| view.separate_selected_stems(cx));

    register_theme_command::<UseSlateTheme>(ThemeName::Slate, view, cx);
    register_theme_command::<UsePaperTheme>(ThemeName::Paper, view, cx);
    register_theme_command::<UseDeckTheme>(ThemeName::Deck, view, cx);
    register_theme_command::<UseGraphiteTheme>(ThemeName::Graphite, view, cx);
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
    cx.set_menus(build_menus(current_theme, sidebar_folded));
}

/// The menus: what the command palette lists must be what they carry.
pub(crate) fn build_menus(current_theme: ThemeName, sidebar_folded: bool) -> Vec<Menu> {
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
        };
        item.checked(theme == current_theme)
    });
    vec![
        Menu::new(WINDOW_TITLE).items([
            MenuItem::action("Settings\u{2026}", OpenSettings),
            MenuItem::separator(),
            MenuItem::action("Quit Euphoria", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("Add Folder\u{2026}", AddFolder),
            MenuItem::separator(),
            MenuItem::action("Export Library\u{2026}", ExportLibrary),
        ]),
        Menu::new("View").items([
            MenuItem::action(sidebar_item, ToggleSidebar),
            MenuItem::action("Decks", ToggleDecks),
            MenuItem::action("Live", OpenLive),
            MenuItem::action("Set Builder", OpenSetBuilder),
            MenuItem::action("Focus View", OpenFocusView),
            MenuItem::separator(),
            MenuItem::action("Command Palette\u{2026}", OpenCommandPalette),
            MenuItem::action("Search", FocusSearch),
            MenuItem::action("Select All Tracks", SelectAllTracks),
            MenuItem::action("Star or Unstar Selection", ToggleFavorite),
            MenuItem::action(reveal::reveal_label(), RevealTrack),
            MenuItem::separator(),
            MenuItem::action("Reset Column Widths", ResetColumnWidths),
            MenuItem::separator(),
            MenuItem::submenu(Menu::new("Theme").items(theme_items)),
        ]),
        Menu::new("Tracks").items([
            MenuItem::action("Analyze Selected Tracks Again", AnalyzeAgain),
            MenuItem::action("Stop Analysis", StopAnalysis),
        ]),
        Menu::new("Tags").items([
            MenuItem::action("Edit Tags\u{2026}", EditTags),
            MenuItem::action("Write Tags\u{2026}", WriteTags),
            MenuItem::action("Undo Last Tag Write\u{2026}", UndoTagWrite),
        ]),
        Menu::new("Playback").items([
            MenuItem::action("Play or Pause", TogglePlayback),
            MenuItem::action("Metronome", ToggleMetronome),
            MenuItem::action("Plan Transition from Playing Track", PlanTransition),
            MenuItem::action("Separate Stems", SeparateStems),
            MenuItem::separator(),
            MenuItem::action("Add Cue at Playhead", AddCueAtPlayhead),
            MenuItem::action("Undo Cue Edit", UndoCueEdit),
        ]),
    ]
}
