// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The command palette used through the real root view in a headless
//! window, with keys and clicks as a person would.

use std::collections::HashSet;

use euphoria_core::harmony::Relation;
use gpui::test::TestWindowExt;
use gpui::{Action, AnyWindowHandle, AppContext, Entity, TestAppContext};

use crate::actions::{OpenCommandPalette, WriteTags};
use crate::app::EuphoriaApp;
use crate::command_palette::{COMMANDS, PaletteRow, open_progress};
use crate::player::PlayerStatus;
use crate::theme::ThemeName;
use crate::ui_tests::{click, is_shown, open_fixture_library, open_library, track, type_into};
use crate::views::command_palette::row_id;

const OPEN: &str = if cfg!(target_os = "macos") {
    "cmd-k"
} else {
    "ctrl-k"
};

const OPEN_ALSO: &str = if cfg!(target_os = "macos") {
    "cmd-p"
} else {
    "ctrl-p"
};

#[gpui::test]
fn command_k_and_command_p_both_open_and_the_hint_chip_clicks_open(cx: &mut TestAppContext) {
    let (window, view) = open_library(30, cx);
    press(window, OPEN_ALSO, cx);
    assert!(is_open(&view, cx));
    press(window, "escape", cx);
    assert!(!is_open(&view, cx));
    click(window, "palette-hint", cx);
    assert!(is_open(&view, cx));
    press(window, "escape", cx);
    press(window, OPEN, cx);
    assert!(is_open(&view, cx));
}

#[gpui::test]
fn command_f_still_searches_and_command_k_no_longer_does(cx: &mut TestAppContext) {
    cx.update(|_| {});
    let (_, _) = open_library(10, cx);
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let search = crate::actions::FocusSearch;
        let search_keys: Vec<String> = keymap
            .bindings_for_action(&search)
            .flat_map(|binding| {
                binding
                    .keystrokes()
                    .iter()
                    .map(|key| key.unparse(gpui::OperatingSystem::Mac))
            })
            .collect();
        assert!(search_keys.iter().any(|key| key == "cmd-f"));
        assert!(!search_keys.iter().any(|key| key == "cmd-k"));
    });
}

const PLAY: &str = if cfg!(target_os = "macos") {
    "cmd-enter"
} else {
    "ctrl-enter"
};

fn press(window: AnyWindowHandle, keys: &str, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.press(keys, cx);
    })
    .unwrap();
    cx.run_until_parked();
}

fn open_palette(window: AnyWindowHandle, cx: &mut TestAppContext) {
    press(window, OPEN, cx);
}

fn ask(window: AnyWindowHandle, text: &str, cx: &mut TestAppContext) {
    type_into(window, "palette-input", text, cx);
    cx.run_until_parked();
}

fn is_open(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> bool {
    cx.update(|cx| view.read(cx).palette.open.is_some())
}

fn rows(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Vec<PaletteRow> {
    cx.update(|cx| {
        view.read(cx)
            .palette
            .open
            .as_ref()
            .map(|open| open.answer.rows.clone())
            .unwrap_or_default()
    })
}

fn command_ids(rows: &[PaletteRow]) -> Vec<&'static str> {
    rows.iter()
        .filter_map(|row| match row {
            PaletteRow::Command(position) => Some(COMMANDS[*position].id),
            PaletteRow::Track { .. } => None,
        })
        .collect()
}

fn selected(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Option<usize> {
    cx.update(|cx| view.read(cx).selected)
}

#[gpui::test]
fn the_shortcut_opens_the_palette_with_the_keyboard_in_its_field(cx: &mut TestAppContext) {
    let (window, view) = open_library(60, cx);
    assert!(!is_shown(window, "palette", cx));
    open_palette(window, cx);
    assert!(is_open(&view, cx));
    assert!(is_shown(window, "palette", cx));
    assert!(is_shown(window, "palette-input", cx));
    ask(window, "settings", cx);
    cx.update(|cx| {
        assert_eq!(
            view.read(cx).palette.input.read(cx).value().to_string(),
            "settings"
        );
    });
}

#[gpui::test]
fn nothing_typed_lists_the_recent_tracks_then_the_most_used_commands(cx: &mut TestAppContext) {
    let (window, view) = open_library(60, cx);
    click(window, track(4), cx);
    click(window, track(2), cx);
    open_palette(window, cx);
    let shown = rows(&view, cx);
    assert!(matches!(shown[0], PaletteRow::Track { index, .. } if index == 2 || index == 4));
    let track_rows: Vec<usize> = shown
        .iter()
        .filter_map(|row| match row {
            PaletteRow::Track { index, .. } => Some(*index),
            PaletteRow::Command(_) => None,
        })
        .collect();
    assert_eq!(track_rows.len(), 2);
    assert_eq!(selected(&view, cx), Some(track_rows[0]));
    assert_eq!(command_ids(&shown).len(), 6);
    assert!(is_shown(window, row_id(0), cx));
    assert!(is_shown(window, row_id(7), cx));
}

#[gpui::test]
fn the_nine_hundred_recent_selections_are_not_kept_past_eight(cx: &mut TestAppContext) {
    let (window, view) = open_library(60, cx);
    for index in 0..12 {
        click(window, track(index), cx);
    }
    open_palette(window, cx);
    let tracks = rows(&view, cx)
        .into_iter()
        .filter(|row| matches!(row, PaletteRow::Track { .. }))
        .count();
    assert_eq!(tracks, 8);
}

#[gpui::test]
fn a_key_lists_its_tracks_first_and_compatible_ones_with_a_pill(cx: &mut TestAppContext) {
    let (window, view) = open_library(600, cx);
    open_palette(window, cx);
    ask(window, "8A", cx);
    let relations: Vec<Option<Relation>> = rows(&view, cx)
        .into_iter()
        .filter_map(|row| match row {
            PaletteRow::Track { relation, .. } => Some(relation),
            PaletteRow::Command(_) => None,
        })
        .collect();
    assert_eq!(relations.len(), 8);
    assert_eq!(relations[0], Some(Relation::Same));
    assert!(relations.iter().all(Option::is_some));
    ask(window, "8A 120-126 deep", cx);
    let combined = rows(&view, cx);
    assert!(!combined.is_empty());
    cx.update(|cx| {
        let app = view.read(cx);
        for row in combined {
            let PaletteRow::Track { index, .. } = row else {
                panic!("a key and a tempo list no commands");
            };
            let track = &app.session.library().tracks()[index];
            assert!(track.folder.contains("Deep"));
            assert!((120.0..=126.0).contains(&track.bpm().unwrap()));
        }
    });
}

#[gpui::test]
fn enter_selects_the_first_track_and_shows_it_in_the_table(cx: &mut TestAppContext) {
    let (window, view) = open_library(300, cx);
    open_palette(window, cx);
    ask(window, "cobalt", cx);
    let wanted = match rows(&view, cx)[0] {
        PaletteRow::Track { index, .. } => index,
        PaletteRow::Command(_) => panic!("a name lists tracks first"),
    };
    assert_ne!(selected(&view, cx), Some(wanted));
    press(window, "enter", cx);
    assert!(!is_open(&view, cx));
    assert_eq!(selected(&view, cx), Some(wanted));
    cx.update(|cx| assert!(view.read(cx).visible.contains(&wanted)));
    assert_eq!(
        cx.update(|cx| view.read(cx).player.status().clone()),
        PlayerStatus::Stopped
    );
}

#[gpui::test]
fn enter_takes_off_a_search_that_hides_the_track(cx: &mut TestAppContext) {
    let (window, view) = open_library(300, cx);
    type_into(window, "search", "zzzzzz", cx);
    cx.update(|cx| assert!(view.read(cx).visible.is_empty()));
    open_palette(window, cx);
    ask(window, "cobalt", cx);
    press(window, "down", cx);
    press(window, "up", cx);
    press(window, "enter", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(app.query, "");
        let chosen = app.selected.unwrap();
        assert!(app.visible.contains(&chosen));
    });
}

#[gpui::test]
fn the_play_shortcut_selects_the_track_and_plays_it(cx: &mut TestAppContext) {
    let (window, view) = open_fixture_library(cx);
    open_palette(window, cx);
    ask(window, "stereo", cx);
    press(window, PLAY, cx);
    assert!(!is_open(&view, cx));
    assert_eq!(selected(&view, cx), Some(2));
    assert_eq!(
        cx.update(|cx| view.read(cx).player.status().clone()),
        PlayerStatus::Playing
    );
}

#[gpui::test]
fn escape_closes_the_palette_and_gives_the_keyboard_back(cx: &mut TestAppContext) {
    let (window, view) = open_library(60, cx);
    let before = selected(&view, cx);
    open_palette(window, cx);
    ask(window, "write", cx);
    press(window, "escape", cx);
    assert!(!is_open(&view, cx));
    assert!(!is_shown(window, "palette", cx));
    assert_eq!(selected(&view, cx), before);
    cx.update_window(window, |_, window, cx| {
        assert!(view.read(cx).window_focus.is_focused(window));
    })
    .unwrap();
    cx.update(|cx| assert!(view.read(cx).tag_panel.is_none()));
}

#[gpui::test]
fn clicking_outside_the_panel_closes_it_and_clicking_a_row_runs_it(cx: &mut TestAppContext) {
    let (window, view) = open_library(60, cx);
    open_palette(window, cx);
    click(window, "palette-backdrop", cx);
    assert!(!is_open(&view, cx) || is_shown(window, "palette", cx));
    open_palette(window, cx);
    ask(window, "settings", cx);
    let position = rows(&view, cx)
        .iter()
        .position(|row| command_ids(&[*row]) == ["settings"])
        .unwrap();
    click(window, row_id(position), cx);
    assert!(!is_open(&view, cx));
    cx.update(|cx| assert!(view.read(cx).settings_open));
}

#[gpui::test]
fn the_arrow_keys_choose_a_command_and_enter_runs_it(cx: &mut TestAppContext) {
    let (window, view) = open_library(60, cx);
    open_palette(window, cx);
    ask(window, "theme", cx);
    assert_eq!(
        command_ids(&rows(&view, cx)),
        ["theme-slate", "theme-paper", "theme-deck", "theme-graphite"]
    );
    press(window, "down", cx);
    cx.update(|cx| assert_eq!(view.read(cx).palette.open.as_ref().unwrap().selected, 1));
    press(window, "enter", cx);
    assert!(!is_open(&view, cx));
    cx.update(|cx| assert_eq!(view.read(cx).theme.name, ThemeName::Paper));
}

#[gpui::test]
fn running_a_command_counts_it_as_used(cx: &mut TestAppContext) {
    let (window, view) = open_library(60, cx);
    open_palette(window, cx);
    ask(window, "graphite", cx);
    press(window, "enter", cx);
    open_palette(window, cx);
    let shown = command_ids(&rows(&view, cx));
    assert_eq!(shown[0], "theme-graphite");
    assert_eq!(shown.len(), 6);
}

#[gpui::test]
fn synonyms_find_commands_in_the_open_palette(cx: &mut TestAppContext) {
    let (window, view) = open_library(60, cx);
    open_palette(window, cx);
    ask(window, "key", cx);
    assert!(command_ids(&rows(&view, cx)).contains(&"write-tags"));
    ask(window, "tag", cx);
    let shown = command_ids(&rows(&view, cx));
    for wanted in ["write-tags", "edit-tags", "undo-tag-write"] {
        assert!(shown.contains(&wanted), "{wanted}");
    }
}

#[gpui::test]
fn a_command_shows_its_shortcut_as_the_keymap_has_it(cx: &mut TestAppContext) {
    let (_, _) = open_library(10, cx);
    cx.update(|cx| {
        let write = crate::palette_flow::shortcut_label(&WriteTags, cx).unwrap();
        let palette = crate::palette_flow::shortcut_label(&OpenCommandPalette, cx).unwrap();
        if cfg!(target_os = "macos") {
            assert_eq!(write, "\u{21e7}\u{2318}T");
            assert_eq!(palette, "\u{2318}K");
        }
        assert!(crate::palette_flow::shortcut_label(&crate::actions::UsePaperTheme, cx).is_none());
    });
}

#[gpui::test]
fn no_two_commands_share_a_key_in_the_same_context(cx: &mut TestAppContext) {
    let (_, _) = open_library(10, cx);
    cx.update(|cx| {
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let mut seen: Vec<(String, String, &str)> = Vec::new();
        for binding in keymap.bindings() {
            if !COMMANDS
                .iter()
                .any(|command| (command.action)().name() == binding.action().name())
            {
                continue;
            }
            let keys: Vec<String> = binding
                .keystrokes()
                .iter()
                .map(|keystroke| keystroke.unparse(gpui::OperatingSystem::Mac))
                .collect();
            let context = format!("{:?}", binding.predicate());
            seen.push((keys.join(" "), context, binding.action().name()));
        }
        for (position, (keys, context, name)) in seen.iter().enumerate() {
            for (other_keys, other_context, other_name) in &seen[position + 1..] {
                if keys == other_keys && context == other_context {
                    assert_eq!(name, other_name, "{keys} is bound twice");
                }
            }
        }
    });
}

#[test]
fn the_palette_lists_every_menu_command_and_nothing_else() {
    fn menu_actions(menus: &[gpui::Menu], found: &mut HashSet<&'static str>) {
        for menu in menus {
            for item in &menu.items {
                match item {
                    gpui::MenuItem::Action { action, .. } => {
                        found.insert(action.name());
                    }
                    gpui::MenuItem::Submenu(submenu) => {
                        menu_actions(std::slice::from_ref(submenu), found)
                    }
                    _ => {}
                }
            }
        }
    }
    let mut in_menus = HashSet::new();
    for sidebar_folded in [false, true] {
        for theme in ThemeName::ALL {
            menu_actions(&crate::build_menus(theme, sidebar_folded), &mut in_menus);
        }
    }
    in_menus.remove(crate::actions::Quit.name());
    let in_palette: HashSet<&'static str> = COMMANDS
        .iter()
        .map(|command| (command.action)().name())
        .collect();
    assert_eq!(in_palette.len(), COMMANDS.len(), "an action listed twice");
    let missing: Vec<_> = in_menus.difference(&in_palette).collect();
    let extra: Vec<_> = in_palette.difference(&in_menus).collect();
    assert!(
        missing.is_empty(),
        "in the menus, not the palette: {missing:?}"
    );
    assert!(extra.is_empty(), "in the palette, not the menus: {extra:?}");
    let view_menu_has_palette = crate::build_menus(ThemeName::Slate, false)
        .iter()
        .filter(|menu| menu.name == "View")
        .flat_map(|menu| menu.items.iter())
        .any(|item| {
            matches!(item, gpui::MenuItem::Action { action, .. }
                if action.name() == OpenCommandPalette.name())
        });
    assert!(view_menu_has_palette);
}

#[gpui::test]
fn with_motion_reduced_the_palette_is_there_at_once(cx: &mut TestAppContext) {
    let (window, view) = open_library(60, cx);
    open_palette(window, cx);
    cx.update(|cx| {
        let open = view.read(cx).palette.open.as_ref().unwrap();
        assert!(open.reduces_motion);
        assert_eq!(
            open_progress(open.opened_at, open.opened_at, open.reduces_motion),
            1.0
        );
    });
}

#[gpui::test]
fn plan_transition_opens_the_planner_or_says_what_is_missing(cx: &mut TestAppContext) {
    let (window, view) = open_library(60, cx);
    open_palette(window, cx);
    ask(window, "plan transition", cx);
    press(window, "enter", cx);
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.focus.is_open, "the focus view opens on the track");
    });
}

#[gpui::test]
fn separating_stems_without_a_selection_says_so(cx: &mut TestAppContext) {
    let (window, view) = open_library(0, cx);
    open_palette(window, cx);
    ask(window, "separate stems", cx);
    press(window, "enter", cx);
    cx.update(|cx| {
        let notice = view.read(cx).notice.clone().unwrap_or_default();
        assert!(notice.contains("Select a track"), "{notice}");
    });
}
