// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The menu that clicking the Camelot wheel opens: the key moves the Next
//! list, "Mixes well with" and the wheel count, each a toggle, and whether
//! the wheel marks the matching keys at all. It floats above the whole
//! window and closes on a click outside it or on Escape.

use euphoria_core::harmony::Relation;
use euphoria_core::harmony::words::relation_name;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, ElementId, Stateful, TestSupportExt, anchored, deferred, div, px,
};

use crate::app::EuphoriaApp;
use crate::theme::Palette;
use crate::views::{
    GAP_SMALL, GAP_TIGHT, RADIUS_LARGE, RADIUS_SMALL, SMALL_TEXT_SIZE, section_label,
};

/// Width of the menu, room for "One step on the wheel" and its check.
const MENU_WIDTH: f32 = 230.0;
/// Height of one toggle in the menu.
const ROW_HEIGHT: f32 = 26.0;
/// The least room kept between the menu and the edges of the window.
const WINDOW_MARGIN: f32 = 8.0;

/// The menu, when it is open, drawn after everything else in the window.
pub(crate) fn wheel_menu(
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<AnyElement> {
    let position = app.wheel_menu_at?;
    let palette = app.theme.palette;
    let counted = app.decks.filter.mix_options().relations;
    let moves = Relation::ALL
        .into_iter()
        .enumerate()
        .map(|(index, relation)| {
            toggle(
                ("wheel-key-move", index),
                relation_name(relation),
                counted.contains(&relation),
                &palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_wheel_relation(relation, cx)))
            .test_support()
        });
    let show_matching = toggle(
        "wheel-show-matching-keys",
        "Show matching keys",
        app.display.show_matching_keys,
        &palette,
    )
    .on_click(cx.listener(|this, _, _, cx| this.toggle_matching_keys(cx)))
    .test_support();
    let menu = div()
        .id("wheel-menu")
        .occlude()
        .flex()
        .flex_col()
        .gap(GAP_TIGHT)
        .w(px(MENU_WIDTH))
        .p(GAP_SMALL)
        .rounded(RADIUS_LARGE)
        .border_1()
        .border_color(palette.line)
        .bg(palette.panel)
        .text_color(palette.text)
        .shadow_lg()
        .on_mouse_down_out(cx.listener(|this, _, _, cx| {
            this.close_wheel_menu(cx);
        }))
        .child(section_label("Key moves", &palette).px(GAP_SMALL))
        .children(moves)
        .child(div().h(px(1.0)).my(GAP_TIGHT).bg(palette.line))
        .child(show_matching)
        .child(
            div()
                .px(GAP_SMALL)
                .pt(GAP_TIGHT)
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child("The Next list and Mixes well with use these moves too."),
        )
        .test_support();
    Some(
        deferred(
            anchored()
                .position(position)
                .snap_to_window_with_margin(px(WINDOW_MARGIN))
                .child(menu),
        )
        .with_priority(1)
        .into_any_element(),
    )
}

/// One toggle of the menu: a check when it is on, and its label.
fn toggle(
    id: impl Into<ElementId>,
    label: &'static str,
    is_on: bool,
    palette: &Palette,
) -> Stateful<Div> {
    let hover_background = palette.raised;
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(GAP_SMALL)
        .h(px(ROW_HEIGHT))
        .px(GAP_SMALL)
        .rounded(RADIUS_SMALL)
        .cursor_pointer()
        .hover(move |style| style.bg(hover_background))
        .child(
            div()
                .w(px(12.0))
                .text_color(palette.accent)
                .child(if is_on { "\u{2713}" } else { "" }),
        )
        .child(
            div()
                .text_color(if is_on {
                    palette.text
                } else {
                    palette.text_secondary
                })
                .child(label),
        )
}
