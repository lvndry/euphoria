// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The toast that ends a write, above the player bar at the bottom centre.

use std::time::Instant;

use gpui::prelude::*;
use gpui::{BoxShadow, Context, Div, FontWeight, SharedString, TestSupportExt, div, point, px};

use crate::app::EuphoriaApp;
use crate::toast::{SLIDE_DISTANCE, ToastAction};
use crate::views::SMALL_TEXT_SIZE;

/// Space between the toast and what lies below it.
const GAP_ABOVE_BAR: f32 = 14.0;

/// An anchor with no height that sits above the player bar and lets the
/// toast float over the end of the table. The strips under the bar push the
/// anchor up with the bar, so a toast and a strip never overlap.
pub(crate) fn render(app: &EuphoriaApp, now: Instant, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let anchor = div().relative().flex_shrink_0().w_full().h(px(0.0));
    let Some(toast) = app.toasts.current() else {
        return anchor;
    };
    let palette = app.theme.palette;
    let progress = app.toasts.progress(now);
    let message: SharedString = toast.message.clone().into();
    let action = toast.action.clone();
    let shadow_alpha = if palette.is_dark { 0.35 } else { 0.18 };
    let mut body = div()
        .id("toast")
        .flex()
        .items_center()
        .gap(px(14.0))
        .px(px(12.0))
        .py(px(8.0))
        .max_w(gpui::relative(0.9))
        .rounded(px(10.0))
        .border_1()
        .border_color(palette.line)
        .bg(palette.raised)
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text)
        .shadow(vec![BoxShadow {
            color: gpui::black().opacity(shadow_alpha).into(),
            offset: point(px(0.0), px(8.0)),
            blur_radius: px(24.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .occlude()
        .on_hover(cx.listener(|this, is_hovered: &bool, _, cx| {
            this.set_toast_hovered(*is_hovered, cx);
        }))
        .child(div().min_w_0().child(message));
    if let Some(action) = action {
        let hint = matches!(action, ToastAction::UndoTagWrite);
        body = body.child(
            div()
                .id("toast-action")
                .flex_shrink_0()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.accent)
                .cursor_pointer()
                .hover(move |style| style.opacity(0.8))
                .on_click(cx.listener(|this, _, _, cx| this.run_toast_action(cx)))
                .child(action.label())
                .test_support(),
        );
        if hint {
            body = body.child(
                div()
                    .flex_shrink_0()
                    .text_color(palette.text_tertiary)
                    .child(if cfg!(target_os = "macos") {
                        "\u{2318}Z"
                    } else {
                        "Ctrl+Z"
                    }),
            );
        }
    }
    anchor.child(
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(GAP_ABOVE_BAR - SLIDE_DISTANCE * (1.0 - progress)))
            .flex()
            .justify_center()
            .opacity(progress)
            .child(body.test_support()),
    )
}
