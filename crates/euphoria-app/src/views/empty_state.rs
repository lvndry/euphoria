// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the window shows before any music has been added.

use gpui::prelude::*;
use gpui::{Context, Div, FontWeight, div, px};

use crate::app::EuphoriaApp;
use crate::theme::Palette;
use crate::views::logo::logo;
use crate::views::{SMALL_TEXT_SIZE, primary_button};

/// The first run screen: a drop zone, a folder button and three facts.
pub(crate) fn render(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;

    let drop_zone = div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(12.0))
        .px(px(32.0))
        .py(px(44.0))
        .rounded(px(12.0))
        .border_1()
        .border_dashed()
        .border_color(palette.text_tertiary)
        .bg(palette.panel)
        .child(logo(px(56.0), palette.text))
        .child(
            div()
                .text_size(px(18.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Drop a folder of music here"),
        )
        .child(
            div()
                .max_w(px(400.0))
                .text_center()
                .text_color(palette.text_secondary)
                .child(
                    "Euphoria reads each track and finds its key and tempo. \
                     Your files are never changed.",
                ),
        )
        .child(
            primary_button("Choose folder", &palette)
                .id("choose-folder")
                .mt(px(6.0))
                .on_click(cx.listener(|this, _, _, cx| this.choose_folder(cx))),
        );

    let facts = div()
        .flex()
        .gap(px(16.0))
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .child(fact(
            "No account",
            "No sign in, no activation and no internet connection, ever.",
            &palette,
        ))
        .child(fact("Formats", "MP3, M4A, FLAC, WAV and AIFF.", &palette))
        .child(fact(
            "Whole folders",
            "Folders are searched all the way down, and each track is analyzed once.",
            &palette,
        ));

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
                .gap(px(24.0))
                .w_full()
                .max_w(px(560.0))
                .child(drop_zone)
                .child(facts)
                .child(command_line),
        )
}

fn fact(title: &'static str, detail: &'static str, palette: &Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(px(4.0))
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.text)
                .child(title),
        )
        .child(detail)
}
