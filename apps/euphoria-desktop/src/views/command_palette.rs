// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The command palette: a panel near the top of the window with one field,
//! the tracks and commands that answer it, and the keys that run them.

use std::time::Instant;

use euphoria_core::harmony::Relation;
use gpui::component::input::Input;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, FontWeight, KeyDownEvent, MouseButton, TestSupportExt, Window, div,
    px, relative,
};

use crate::app::EuphoriaApp;
use crate::command_palette::{
    COMMANDS, OpenPalette, PaletteRow, RISE_PIXELS, open_progress, relation_pill,
};
use crate::format;
use crate::library::Track;
use crate::palette_flow::shortcut_label;
use crate::theme::Palette;
use crate::views::key_chip::key_chip;
use crate::views::{LABEL_TEXT_SIZE, SMALL_TEXT_SIZE};

/// Width of the panel.
const PANEL_WIDTH: f32 = 560.0;

/// How far below the top of the window the panel starts.
const TOP_OFFSET: f32 = 72.0;

/// The tallest the list of rows grows before it scrolls.
const LIST_MAX_HEIGHT: f32 = 400.0;

/// Id of the row at `position` in the list.
pub(crate) fn row_id(position: usize) -> (&'static str, usize) {
    ("palette-row", position)
}

pub(crate) fn render(
    app: &EuphoriaApp,
    window: &mut Window,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<AnyElement> {
    let open = app.palette.open.as_ref()?;
    let palette = app.theme.palette;
    let progress = open_progress(open.opened_at, Instant::now(), open.reduces_motion);
    if progress < 1.0 {
        window.request_animation_frame();
    }
    let dim = if palette.is_dark { 0.4 } else { 0.2 } * progress;
    let rows = rows(app, open, &palette, cx);

    let field = div()
        .mx(px(10.0))
        .mt(px(10.0))
        .px(px(12.0))
        .py(px(3.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(palette.line)
        .bg(palette.raised)
        .child(
            Input::new(&app.palette.input)
                .id("palette-input")
                .appearance(false),
        );

    let list = div()
        .id("palette-list")
        .flex()
        .flex_col()
        .flex_shrink(1.0)
        .min_h_0()
        .max_h(px(LIST_MAX_HEIGHT))
        .overflow_y_scroll()
        .track_scroll(&app.palette.scroll)
        .px(px(6.0))
        .pt(px(6.0))
        .pb(px(8.0))
        .children(rows);

    let panel = div()
        .id("palette")
        .flex()
        .flex_col()
        .w(px(PANEL_WIDTH))
        .max_w(relative(0.92))
        .max_h(relative(0.86))
        .mt(px(TOP_OFFSET + RISE_PIXELS * (1.0 - progress)))
        .opacity(progress)
        .rounded(px(12.0))
        .border_1()
        .border_color(palette.line)
        .bg(palette.panel)
        .shadow_lg()
        .overflow_hidden()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
            let step = match event.keystroke.key.as_str() {
                "up" => -1,
                "down" => 1,
                "escape" => {
                    cx.stop_propagation();
                    this.close_command_palette(window, cx);
                    return;
                }
                _ => return,
            };
            cx.stop_propagation();
            this.move_palette_selection(step, cx);
        }))
        .child(field)
        .child(list)
        .child(footer(&palette))
        .test_support();

    Some(
        div()
            .id("palette-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .bg(gpui::black().opacity(dim))
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.close_command_palette(window, cx)),
            )
            .child(panel)
            .test_support()
            .into_any_element(),
    )
}

fn rows(
    app: &EuphoriaApp,
    open: &OpenPalette,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Vec<AnyElement> {
    let answer = &open.answer;
    if answer.rows.is_empty() {
        return vec![
            div()
                .px(px(8.0))
                .py(px(14.0))
                .text_color(palette.text_tertiary)
                .child("Nothing matches. Try a key like 8A, a tempo like 124, or e6.")
                .into_any_element(),
        ];
    }
    let mut elements: Vec<AnyElement> = Vec::new();
    let track_heading = match (answer.is_resting, answer.track_total) {
        (true, _) => "RECENT".to_string(),
        (false, total) => format!("TRACKS \u{b7} {} match", format::count(total)),
    };
    let command_heading = if answer.is_resting {
        "MOST USED"
    } else {
        "ACTIONS"
    };
    for (position, row) in answer.rows.iter().enumerate() {
        if position == 0 && answer.track_rows > 0 {
            elements.push(heading(track_heading.clone(), palette));
        }
        if position == answer.track_rows {
            elements.push(heading(command_heading.to_string(), palette));
        }
        let is_selected = position == open.selected;
        let content = match row {
            PaletteRow::Track { index, relation } => {
                let Some(track) = app.session.library().track(*index) else {
                    continue;
                };
                track_row(app, track, *relation, is_selected, palette)
            }
            PaletteRow::Command(command) => command_row(*command, is_selected, palette, cx),
        };
        elements.push(
            content
                .id(row_id(position))
                .on_mouse_move(cx.listener(move |this, _, _, cx| {
                    this.point_at_palette_row(position, cx);
                }))
                .on_click(
                    cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                        let play = event.modifiers().secondary(cx);
                        this.run_palette_row_at(position, play, window, cx);
                    }),
                )
                .test_support()
                .into_any_element(),
        );
    }
    elements
}

fn heading(text: String, palette: &Palette) -> AnyElement {
    div()
        .px(px(8.0))
        .pt(px(8.0))
        .pb(px(4.0))
        .text_size(LABEL_TEXT_SIZE)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette.text_tertiary)
        .child(text)
        .into_any_element()
}

fn row_frame(is_selected: bool, palette: &Palette) -> gpui::Stateful<Div> {
    let hover_background = palette.accent.opacity(0.08);
    div()
        .id("palette-row-frame")
        .flex()
        .items_center()
        .gap(px(10.0))
        .px(px(8.0))
        .py(px(6.0))
        .rounded(px(6.0))
        .cursor_pointer()
        .when(is_selected, |row| row.bg(palette.accent.opacity(0.16)))
        .when(!is_selected, |row| {
            row.hover(move |style| style.bg(hover_background))
        })
}

fn track_row(
    app: &EuphoriaApp,
    track: &Track,
    relation: Option<Relation>,
    is_selected: bool,
    palette: &Palette,
) -> gpui::Stateful<Div> {
    let key = track.key().map(|key| {
        key_chip(&key, &app.theme, app.display.key_notation)
            .w(px(36.0))
            .into_any_element()
    });
    let key_slot = div()
        .flex()
        .flex_shrink_0()
        .w(px(36.0))
        .justify_center()
        .text_color(palette.text_tertiary)
        .child(key.unwrap_or_else(|| "\u{2013}".into_any_element()));
    let mut title = div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_baseline()
        .gap(px(8.0))
        .child(
            div()
                .flex_shrink_0()
                .max_w(relative(0.7))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(palette.text)
                .child(track.title().to_string()),
        );
    if let Some(artist) = track.artist.as_deref() {
        title = title.child(
            div()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child(artist.to_string()),
        );
    }
    let pill = relation.and_then(relation_pill).map(|text| {
        div()
            .flex_shrink_0()
            .px(px(7.0))
            .rounded(px(999.0))
            .bg(palette.raised)
            .text_size(LABEL_TEXT_SIZE)
            .text_color(palette.text_secondary)
            .child(text)
    });
    let numbers = div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(10.0))
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .child(
            div()
                .w(px(40.0))
                .flex()
                .justify_end()
                .child(track.bpm().map_or("\u{2013}".to_string(), format::bpm)),
        )
        .child(
            div()
                .w(px(22.0))
                .flex()
                .justify_end()
                .text_color(palette.text_tertiary)
                .child(
                    track
                        .energy()
                        .map_or(String::new(), |energy| format!("E{energy}")),
                ),
        );
    row_frame(is_selected, palette)
        .child(key_slot)
        .child(title)
        .children(pill)
        .child(numbers)
        .children(is_selected.then(|| kbd("\u{21b5}", palette)))
}

fn command_row(
    command: usize,
    is_selected: bool,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> gpui::Stateful<Div> {
    let entry = &COMMANDS[command];
    let shortcut = shortcut_label((entry.action)().as_ref(), cx);
    row_frame(is_selected, palette)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_color(palette.text)
                .child(entry.name),
        )
        .children(shortcut.map(|label| kbd(label, palette)))
}

/// A key as a small outlined chip.
fn kbd(label: impl Into<gpui::SharedString>, palette: &Palette) -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .px(px(5.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(palette.line)
        .text_size(LABEL_TEXT_SIZE)
        .text_color(palette.text_tertiary)
        .child(label.into())
}

fn footer(palette: &Palette) -> Div {
    let hint = |keys: &'static str, what: &'static str| {
        div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(kbd(keys, palette))
            .child(what)
    };
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(16.0))
        .px(px(14.0))
        .py(px(8.0))
        .border_t_1()
        .border_color(palette.line)
        .text_size(LABEL_TEXT_SIZE)
        .text_color(palette.text_tertiary)
        .child(hint("\u{2191}\u{2193}", "move"))
        .child(hint("\u{21b5}", "select"))
        .child(hint(
            if cfg!(target_os = "macos") {
                "\u{2318}\u{21b5}"
            } else {
                "Ctrl+\u{21b5}"
            },
            "play",
        ))
        .child(hint("esc", "close"))
}
