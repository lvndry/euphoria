// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Camelot code of a key, coloured by its number.

use euphoria_core::Key;
use gpui::prelude::*;
use gpui::{Div, FontWeight, div, px};

use crate::camelot::CamelotPosition;
use crate::theme::AppTheme;
use crate::views::SMALL_TEXT_SIZE;

/// The chip for `key`: its Camelot code in the colours of its number.
pub(crate) fn key_chip(key: &Key, theme: &AppTheme) -> Div {
    let colours = theme.key_colours(CamelotPosition::of(key));
    let chip = div()
        .flex()
        .flex_shrink_0()
        .justify_center()
        .py(px(1.0))
        .rounded(px(4.0))
        .text_size(SMALL_TEXT_SIZE)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(colours.foreground)
        .child(key.camelot());
    match colours.background {
        Some(background) => chip.min_w(px(30.0)).px(px(6.0)).bg(background),
        None => chip,
    }
}
