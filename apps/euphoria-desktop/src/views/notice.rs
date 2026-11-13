// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A strip along the bottom of the window that reports a problem.

use gpui::prelude::*;
use gpui::{Context, Div, Stateful, div, px};

use crate::app::EuphoriaApp;
use crate::views::SMALL_TEXT_SIZE;

/// The notice strip, when there is a problem to report.
pub(crate) fn render(
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<Stateful<Div>> {
    let palette = app.theme.palette;
    let message = app.notice.clone()?;
    Some(
        div()
            .id("notice")
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(12.0))
            .px(px(14.0))
            .py(px(8.0))
            .border_t_1()
            .border_color(palette.line)
            .bg(palette.sidebar)
            .text_size(SMALL_TEXT_SIZE)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(palette.danger)
                    .child(message),
            )
            .child(
                div()
                    .id("dismiss-notice")
                    .flex_shrink_0()
                    .px(px(8.0))
                    .py(px(2.0))
                    .rounded(px(4.0))
                    .text_color(palette.text_secondary)
                    .cursor_pointer()
                    .hover(move |style| style.bg(palette.raised).text_color(palette.text))
                    .on_click(cx.listener(|this, _, _, cx| this.dismiss_notice(cx)))
                    .child("Dismiss"),
            ),
    )
}
