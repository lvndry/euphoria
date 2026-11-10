// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The parts of the window. Each view reads the root view's state and turns
//! it into elements, taking every colour from the theme.

pub(crate) mod empty_state;
pub(crate) mod inspector;
pub(crate) mod key_chip;
pub(crate) mod notice;
pub(crate) mod sidebar;
pub(crate) mod toolbar;
pub(crate) mod track_table;
pub(crate) mod waveform;
pub(crate) mod wheel;

use gpui::prelude::*;
use gpui::{Div, FontWeight, Pixels, SharedString, div, px};

use crate::theme::Palette;

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
