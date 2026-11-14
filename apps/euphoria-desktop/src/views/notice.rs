// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A strip along the bottom of the window that reports a problem.

use gpui::prelude::*;
use gpui::{Context, Div, SharedString, Stateful, div, px};

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

/// The strip that asks whether to link a missing track to a file Euphoria
/// cannot check, while one waits for an answer.
pub(crate) fn render_link_confirmation(
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<Stateful<Div>> {
    let palette = app.theme.palette;
    let link = app.pending_link.as_ref()?;
    let file_name = |path: &std::path::Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    };
    let question: SharedString = format!(
        "Euphoria can\u{2019}t check that {} is the same track as the missing {}. Link it anyway?",
        file_name(&link.picked),
        file_name(&link.old)
    )
    .into();
    let button = |id: &'static str, label: &'static str| {
        div()
            .id(id)
            .flex_shrink_0()
            .px(px(8.0))
            .py(px(2.0))
            .rounded(px(4.0))
            .text_color(palette.text_secondary)
            .cursor_pointer()
            .hover(move |style| style.bg(palette.raised).text_color(palette.text))
            .child(label)
    };
    Some(
        div()
            .id("link-confirmation")
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
                    .text_color(palette.text)
                    .child(question),
            )
            .child(
                button("link-anyway", "Link anyway")
                    .on_click(cx.listener(|this, _, _, cx| this.confirm_pending_link(cx))),
            )
            .child(
                button("cancel-link", "Cancel")
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_pending_link(cx))),
            ),
    )
}
