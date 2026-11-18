// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The edge between the track table and the track detail. Dragging it
//! resizes the detail, and a double click returns it to its usual size. The
//! edge takes no room of its own: it lies over the hairline between the two,
//! on top of both.

use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, CursorStyle, Div, MouseButton, Stateful, TestSupportExt,
    Window, div, px,
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
    let half_grip = px(GRIP / 2.0);
    match app.theme.layout {
        Layout::InspectorRight => grip(
            "panel-edge",
            app,
            PanelEdge::InspectorLeft,
            CursorStyle::ResizeLeftRight,
            cx,
        )
        .top_0()
        .bottom_0()
        .right(app.inspector_width(window) - half_grip)
        .w(px(GRIP))
        .test_support(),
        Layout::DeckBelow => grip(
            "panel-edge",
            app,
            PanelEdge::StripTop,
            CursorStyle::ResizeUpDown,
            cx,
        )
        .left_0()
        .right_0()
        .bottom(app.strip_height(window) - half_grip)
        .h(px(GRIP))
        .test_support(),
    }
    .into_any_element()
}

/// The edge along the top of the decks. Dragging it makes the list in the
/// decks taller or shorter, giving the table the rest, and a double click
/// returns the list to its usual height. It must be drawn inside the decks.
pub(crate) fn decks_top(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> AnyElement {
    grip(
        "decks-edge",
        app,
        PanelEdge::DecksTop,
        CursorStyle::ResizeUpDown,
        cx,
    )
    .left_0()
    .right_0()
    .top(px(-GRIP / 2.0))
    .h(px(GRIP))
    .test_support()
    .into_any_element()
}

fn grip(
    id: &'static str,
    app: &EuphoriaApp,
    edge: PanelEdge,
    cursor: CursorStyle,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Stateful<Div> {
    let palette = app.theme.palette;
    let is_held = app.resizing == Some(edge);
    let highlight = palette.accent.opacity(0.5);
    div()
        .id(id)
        .absolute()
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
}
