// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Opening the command palette, typing into it, and running what it lists.

use std::time::Instant;

use gpui::{Action, App, Context, Modifiers, Window};

use crate::app::EuphoriaApp;
use crate::command_palette::{COMMANDS, OpenPalette, PaletteRow, answer, motion_is_reduced};

/// The text shown for the keys that run `action`: the first binding of the
/// keymap, written as the platform writes it.
pub(crate) fn shortcut_label(action: &dyn Action, cx: &App) -> Option<String> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let binding = keymap.bindings_for_action(action).find(|binding| {
        cfg!(target_os = "macos")
            || binding
                .keystrokes()
                .iter()
                .all(|keystroke| !keystroke.modifiers().platform)
    })?;
    let label: Vec<String> = binding
        .keystrokes()
        .iter()
        .map(|keystroke| write_keystroke(keystroke.modifiers(), keystroke.key()))
        .collect();
    Some(label.join(" "))
}

/// A keystroke as macOS writes it, modifiers in the order Control, Option,
/// Shift, Command, or as other systems write it, joined with plus signs.
fn write_keystroke(modifiers: &Modifiers, key: &str) -> String {
    let key_text = match key {
        "enter" => "\u{21b5}".to_string(),
        "escape" => "Esc".to_string(),
        "space" => "Space".to_string(),
        "up" => "\u{2191}".to_string(),
        "down" => "\u{2193}".to_string(),
        "left" => "\u{2190}".to_string(),
        "right" => "\u{2192}".to_string(),
        "backspace" => "\u{232b}".to_string(),
        "tab" => "\u{21e5}".to_string(),
        other => other.to_uppercase(),
    };
    if cfg!(target_os = "macos") {
        let mut written = String::new();
        for (is_held, symbol) in [
            (modifiers.control, '\u{2303}'),
            (modifiers.alt, '\u{2325}'),
            (modifiers.shift, '\u{21e7}'),
            (modifiers.platform, '\u{2318}'),
        ] {
            if is_held {
                written.push(symbol);
            }
        }
        written + &key_text
    } else {
        let mut parts: Vec<&str> = Vec::new();
        for (is_held, name) in [
            (modifiers.control, "Ctrl"),
            (modifiers.alt, "Alt"),
            (modifiers.shift, "Shift"),
            (modifiers.platform, "Super"),
        ] {
            if is_held {
                parts.push(name);
            }
        }
        parts.push(&key_text);
        parts.join("+")
    }
}

impl EuphoriaApp {
    /// Opens the palette over the window with the keyboard in its field.
    pub(crate) fn open_command_palette(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.palette
            .input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.palette.open = Some(OpenPalette {
            answer: self.palette_answer(cx),
            selected: 0,
            opened_at: Instant::now(),
            reduces_motion: motion_is_reduced(),
        });
        self.palette
            .input
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// Closes the palette and hands the keyboard back to the window.
    pub(crate) fn close_command_palette(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.palette.open.take().is_some() {
            self.window_focus.focus(window, cx);
            cx.notify();
        }
    }

    fn palette_answer(&self, cx: &App) -> crate::command_palette::PaletteAnswer {
        let text = self.palette.input.read(cx).value().to_string();
        let library = self.session.library();
        answer(
            &text,
            library.tracks(),
            |path| library.index_of(path),
            &self.palette.recent,
            &self.palette.usage,
        )
    }

    /// Lists what the text in the field asks for, with the first row
    /// selected.
    pub(crate) fn palette_query_changed(&mut self, cx: &mut Context<'_, Self>) {
        if self.palette.open.is_none() {
            return;
        }
        let answer = self.palette_answer(cx);
        if let Some(open) = self.palette.open.as_mut() {
            open.answer = answer;
            open.selected = 0;
        }
        self.palette.scroll.scroll_to_item(0);
        cx.notify();
    }

    /// Moves the selection `steps` rows and scrolls it into view.
    pub(crate) fn move_palette_selection(&mut self, steps: isize, cx: &mut Context<'_, Self>) {
        let Some(open) = self.palette.open.as_mut() else {
            return;
        };
        open.move_selection(steps);
        let position = child_position(open);
        self.palette.scroll.scroll_to_item(position);
        cx.notify();
    }

    /// Selects the row under the pointer.
    pub(crate) fn point_at_palette_row(&mut self, row: usize, cx: &mut Context<'_, Self>) {
        if let Some(open) = self.palette.open.as_mut()
            && open.selected != row
            && row < open.answer.rows.len()
        {
            open.selected = row;
            cx.notify();
        }
    }

    /// Runs the selected row: a track is selected and shown in the table,
    /// and played when `play` is set; a command is run.
    pub(crate) fn run_palette_selection(
        &mut self,
        play: bool,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(row) = self
            .palette
            .open
            .as_ref()
            .and_then(OpenPalette::selected_row)
        else {
            return;
        };
        self.run_palette_row(row, play, window, cx);
    }

    /// Runs the row at `position` in the list.
    pub(crate) fn run_palette_row_at(
        &mut self,
        position: usize,
        play: bool,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let row = self
            .palette
            .open
            .as_ref()
            .and_then(|open| open.answer.rows.get(position).copied());
        if let Some(row) = row {
            self.run_palette_row(row, play, window, cx);
        }
    }

    /// Runs `row`.
    pub(crate) fn run_palette_row(
        &mut self,
        row: PaletteRow,
        play: bool,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.close_command_palette(window, cx);
        match row {
            PaletteRow::Track { index, .. } => {
                self.reveal_track_in_table(index, window, cx);
                if play && !self.player.offers_pause() {
                    self.toggle_playback(cx);
                }
            }
            PaletteRow::Command(position) => {
                let Some(command) = COMMANDS.get(position) else {
                    return;
                };
                self.palette.usage.record(position);
                window.dispatch_action((command.action)(), cx);
            }
        }
    }

    /// Selects the track at `index` and shows it in the table. A search,
    /// filter, folder or playlist that hides it is taken off first.
    pub(crate) fn reveal_track_in_table(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.close_focus_view(cx);
        self.close_set_builder();
        if !self.visible.contains(&index) {
            self.search_input
                .update(cx, |input, cx| input.set_value("", window, cx));
            self.query.clear();
            self.clear_track_filters(window, cx);
            self.show_all_tracks(cx);
        }
        self.select_and_reveal(index, cx);
        self.table_focus.focus(window, cx);
    }

    /// Opens the focus view on the playing track, or the selected one when
    /// nothing plays, and the planner for its best match.
    pub(crate) fn plan_transition_from_playing(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let playing = self
            .player
            .track()
            .and_then(|path| self.session.library().index_of(path));
        let Some(index) = playing.or(self.selected) else {
            self.notice = Some("Play or select a track to plan a transition from.".to_string());
            cx.notify();
            return;
        };
        if self.selected != Some(index) {
            self.reveal_track_in_table(index, window, cx);
        }
        if !self.focus.is_open {
            self.open_focus_view(window, cx);
        }
        if !self.focus.is_open {
            return;
        }
        if self.focus.selected_match.is_none() {
            self.notice = Some("No track in the library mixes with this one yet.".to_string());
            cx.notify();
            return;
        }
        self.planner.open_when_ready = true;
        self.open_planner_when_ready(cx);
    }

    /// Asks the stems of the selected track to be separated.
    pub(crate) fn separate_selected_stems(&mut self, cx: &mut Context<'_, Self>) {
        if self.selected_track().is_none() {
            self.notice = Some("Select a track to separate its stems.".to_string());
            cx.notify();
            return;
        }
        self.request_stems(cx);
    }
}

/// Where the selected row is among the children of the list, which also
/// holds a heading above the tracks and one above the commands.
fn child_position(open: &OpenPalette) -> usize {
    let has_tracks = open.answer.track_rows > 0;
    let in_commands = open.selected >= open.answer.track_rows;
    let headings = usize::from(has_tracks) + usize::from(in_commands);
    open.selected + headings
}
