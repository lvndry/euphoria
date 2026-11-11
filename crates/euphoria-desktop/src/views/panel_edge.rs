// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The edge between the track table and the track detail. Dragging it
//! resizes the detail, and a double click returns it to its usual size. The
//! edge takes no room of its own: it lies over the hairline between the two,
//! on top of both.

use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, CursorStyle, MouseButton, TestSupportExt, Window, div, px,
};

use crate::app::EuphoriaApp;
use crate::panels::PanelEdge;
use crate::theme::Layout;

/// How far across the edge it can be grabbed.
const GRIP: f32 = 8.0;

/// The edge for the current layout: upright on the left of the inspector
/// column, or flat along the top of the strip under the table. It is placed
/// against the area the table and the detail share, so it must be drawn
/// inside that area and after both.
pub(crate) fn render(
    app: &EuphoriaApp,
    window: &Window,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let palette = app.theme.palette;
    let (edge, cursor) = match app.theme.layout {
        Layout::InspectorRight => (PanelEdge::InspectorLeft, CursorStyle::ResizeLeftRight),
        Layout::DeckBelow => (PanelEdge::StripTop, CursorStyle::ResizeUpDown),
    };
    let is_held = app.resizing == Some(edge);
    let highlight = palette.accent.opacity(0.5);
    let half_grip = px(GRIP / 2.0);

    div()
        .id("panel-edge")
        .absolute()
        .map(|grip| match edge {
            PanelEdge::InspectorLeft => grip
                .top_0()
                .bottom_0()
                .right(app.inspector_width(window) - half_grip)
                .w(px(GRIP)),
            PanelEdge::StripTop => grip
                .left_0()
                .right_0()
                .bottom(app.strip_height(window) - half_grip)
                .h(px(GRIP)),
        })
        .cursor(cursor)
        .when(is_held, |grip| grip.bg(highlight))
        .hover(move |style| style.bg(highlight))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| this.start_resizing(edge, cx)),
        )
        .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
            if event.click_count() >= 2 {
                this.reset_size(edge, cx);
            }
        }))
        .test_support()
        .into_any_element()
}
