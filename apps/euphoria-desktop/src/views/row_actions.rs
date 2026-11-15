// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The small icon buttons a row shows when the pointer is on it or it is
//! selected: play, star and add to the set. A row shows only its scanned
//! data the rest of the time.

use gpui::component::tooltip::Tooltip;
use gpui::component::{Icon, IconName};
use gpui::prelude::*;
use gpui::{Bounds, Div, ElementId, Hsla, Pixels, SharedString, Stateful, canvas, div, px};

use crate::theme::{Palette, blend};
use crate::views::RADIUS_MEDIUM;
use crate::views::favorites::star_icon;
use crate::views::player::{GLYPH_SIZE, paint_pause_glyph, paint_play_glyph};

/// Side of an icon button.
pub(crate) const ICON_SIZE: f32 = 22.0;

const GROUP_PADDING: f32 = 3.0;

/// Space between icon buttons.
const ICON_GAP: f32 = 4.0;

/// The tooltip of an action: its name and the key that does the same.
pub(crate) fn tooltip_text(action: &str, key: &str) -> SharedString {
    format!("{action} ({key})").into()
}

/// How much of the end of the cell `count` buttons cover. A title leaves
/// this much room while they show, so it ends in an ellipsis before them.
pub(crate) fn reserved_width(count: usize) -> f32 {
    ICON_SIZE * count as f32
        + ICON_GAP * count.saturating_sub(1) as f32
        + 2.0 * GROUP_PADDING
        + ICON_GAP
}

/// A 22 px square button.
pub(crate) fn icon_button(id: impl Into<ElementId>, palette: &Palette) -> Stateful<Div> {
    let hover_background = palette.line;
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(ICON_SIZE))
        .rounded(RADIUS_MEDIUM)
        .bg(palette.raised)
        .cursor_pointer()
        .hover(move |style| style.bg(hover_background))
}

/// The play button's glyph, a pause glyph while the track is sounding.
pub(crate) fn play_icon(is_sounding: bool, colour: Hsla) -> gpui::Canvas<()> {
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, _, window, _| {
            if is_sounding {
                paint_pause_glyph(bounds, colour, window);
            } else {
                paint_play_glyph(bounds, colour, window);
            }
        },
    )
}

/// A play or pause button with its tooltip.
pub(crate) fn play_button(
    id: impl Into<ElementId>,
    is_sounding: bool,
    tooltip: SharedString,
    palette: &Palette,
) -> Stateful<Div> {
    icon_button(id, palette)
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .child(play_icon(is_sounding, palette.text_secondary).size(px(GLYPH_SIZE)))
}

/// A star button, filled when `is_starred`.
pub(crate) fn star_action(
    id: impl Into<ElementId>,
    is_starred: bool,
    tooltip: SharedString,
    palette: &Palette,
) -> Stateful<Div> {
    icon_button(id, palette)
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .child(star_icon(is_starred, palette))
}

/// An add button.
pub(crate) fn add_action(
    id: impl Into<ElementId>,
    tooltip: SharedString,
    palette: &Palette,
) -> Stateful<Div> {
    icon_button(id, palette)
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .child(
            Icon::new(IconName::Plus)
                .size(px(12.0))
                .text_color(palette.text_secondary),
        )
}

/// The buttons laid over the end of a row's first cell. They are laid out
/// out of the flow, so showing them moves nothing, and they stay out of
/// sight until the pointer is on the row in `group`, or `is_selected`.
pub(crate) fn overlay(group: &'static str, is_selected: bool, palette: &Palette) -> Div {
    let hovered = blend(palette.raised, palette.background, 0.5);
    let resting = if is_selected {
        palette.selection
    } else {
        hovered
    };
    div()
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .flex()
        .items_center()
        .gap(px(ICON_GAP))
        .pl(px(GROUP_PADDING))
        .pr(px(GROUP_PADDING))
        .bg(resting)
        .when(!is_selected, |overlay| {
            overlay
                .invisible()
                .group_hover(group, |style| style.visible())
        })
}
