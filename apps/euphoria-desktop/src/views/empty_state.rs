// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the window shows before any music has been added.

use gpui::prelude::*;
use gpui::{Context, Div, div, px};

use crate::app::EuphoriaApp;
use crate::views::SMALL_TEXT_SIZE;
use crate::views::first_run;
use crate::views::logo::logo;

/// The first run screen: the three steps, with the first one current.
pub(crate) fn render(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;

    let command_line = div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .pt(px(20.0))
        .border_t_1()
        .border_color(palette.line)
        .child(
            div()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child("Prefer the terminal? The same analysis runs from the command line."),
        )
        .child(
            div()
                .px(px(12.0))
                .py(px(8.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(palette.line)
                .bg(palette.raised)
                .font_family(
                    gpui::component::ActiveTheme::theme(&**cx)
                        .mono_font_family
                        .clone(),
                )
                .text_size(SMALL_TEXT_SIZE)
                .child("euphoria analyze ~/Music/DJ\\ tracks"),
        );

    div()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .justify_center()
        .px(px(24.0))
        .py(px(40.0))
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap(px(20.0))
                .w_full()
                .max_w(px(760.0))
                .child(logo(px(56.0), palette.text))
                .child(first_run::card(app, cx).w_full())
                .child(command_line.w_full()),
        )
}
