// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The parts of the window. Each view reads the root view's state and turns
//! it into elements, taking every colour from the theme.

pub(crate) mod by_ear;
pub(crate) mod cues;
pub(crate) mod decks;
pub(crate) mod empty_state;
pub(crate) mod favorites;
pub(crate) mod inspector;
pub(crate) mod key_chip;
pub(crate) mod key_shift;
pub(crate) mod logo;
pub(crate) mod notice;
pub(crate) mod panel_edge;
pub(crate) mod player;
pub(crate) mod playlists;
pub(crate) mod settings_panel;
pub(crate) mod sidebar;
pub(crate) mod stems;
pub(crate) mod tag_editor;
pub(crate) mod tag_panel;
pub(crate) mod toolbar;
pub(crate) mod track_overview;
pub(crate) mod track_table;
pub(crate) mod waveform;
pub(crate) mod wheel;

use gpui::prelude::*;
use gpui::{Div, FontWeight, Pixels, SharedString, Stateful, div, px};

use crate::theme::Palette;

/// The key context of the window, which keyboard shortcuts for the library
/// and the player are bound in.
pub(crate) const LIBRARY_KEY_CONTEXT: &str = "Library";

/// Size of ordinary interface text.
pub(crate) const TEXT_SIZE: Pixels = px(13.0);
/// Size of secondary text such as counts and progress.
pub(crate) const SMALL_TEXT_SIZE: Pixels = px(12.0);
/// Size of labels above sections and columns.
pub(crate) const LABEL_TEXT_SIZE: Pixels = px(11.0);
/// Corner radius of buttons and fields.
pub(crate) const CONTROL_RADIUS: Pixels = px(6.0);
/// Height of buttons and fields in the toolbar.
pub(crate) const CONTROL_HEIGHT: Pixels = px(28.0);

/// The small bold label above a section.
pub(crate) fn section_label(text: impl Into<SharedString>, palette: &Palette) -> Div {
    div()
        .text_size(LABEL_TEXT_SIZE)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette.text_tertiary)
        .child(text.into())
}

/// A quiet bordered button, as used for secondary commands.
pub(crate) fn secondary_button(label: impl Into<SharedString>, palette: &Palette) -> Div {
    let hover_background = palette.line;
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(CONTROL_HEIGHT)
        .px(px(12.0))
        .rounded(CONTROL_RADIUS)
        .border_1()
        .border_color(palette.line)
        .bg(palette.raised)
        .text_color(palette.text)
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(move |style| style.bg(hover_background))
        .child(label.into())
}

/// A filled accent button, as used for the main command of a screen.
pub(crate) fn primary_button(label: impl Into<SharedString>, palette: &Palette) -> Div {
    let hover_background = palette.accent.opacity(0.9);
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(px(32.0))
        .px(px(16.0))
        .rounded(CONTROL_RADIUS)
        .bg(palette.accent)
        .text_color(palette.accent_ink)
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(move |style| style.bg(hover_background))
        .child(label.into())
}

/// A card in the middle of the window over a dimmed backdrop, for a panel
/// that needs an answer before the window is used again. Clicks on the
/// backdrop go nowhere.
pub(crate) fn modal(id: &'static str, width: Pixels, palette: &Palette) -> (Stateful<Div>, Div) {
    let backdrop = div()
        .id(id)
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui::black().opacity(if palette.is_dark { 0.55 } else { 0.3 }))
        .occlude();
    let card = div()
        .flex()
        .flex_col()
        .w(width)
        .max_w(gpui::relative(0.92))
        .max_h(gpui::relative(0.86))
        .gap(px(14.0))
        .p(px(20.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(palette.line)
        .bg(palette.panel)
        .shadow_lg();
    (backdrop, card)
}
