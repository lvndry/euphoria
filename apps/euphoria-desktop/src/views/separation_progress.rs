// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The bar every place shows while a track's stems are separated: a fill in
//! the accent tint growing from left to right, with the caption over it.

use gpui::prelude::*;
use gpui::{AnyElement, Pixels, TestSupportExt, div, relative};

use crate::separation_fill::SeparationShown;
use crate::theme::Palette;
use crate::views::{GAP_SMALL, LABEL_TEXT_SIZE, RADIUS_SMALL};

/// The ids a bar is found by: the bar itself, and its fill.
#[derive(Clone, Copy)]
pub(crate) struct BarIds {
    pub(crate) bar: &'static str,
    pub(crate) fill: &'static str,
}

/// The bar, as wide and as tall as where it is put. The caption starts
/// `inset` from the left, clear of a lane's label.
pub(crate) fn separation_bar(
    ids: BarIds,
    shown: &SeparationShown,
    inset: Pixels,
    palette: &Palette,
) -> AnyElement {
    let fill = shown.fill.map(|fill| {
        div()
            .id(ids.fill)
            .absolute()
            .top_0()
            .bottom_0()
            .left_0()
            .w(relative(fill.clamp(0.0, 1.0)))
            .bg(palette.accent.opacity(0.3))
            .test_support()
    });
    div()
        .id(ids.bar)
        .relative()
        .size_full()
        .overflow_hidden()
        .rounded(RADIUS_SMALL)
        .children(fill)
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(inset)
                .right(GAP_SMALL)
                .flex()
                .items_center()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(LABEL_TEXT_SIZE)
                .text_color(palette.text_secondary)
                .child(shown.caption.clone()),
        )
        .test_support()
        .into_any_element()
}
