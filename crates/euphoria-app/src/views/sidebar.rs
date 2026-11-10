// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The column on the left: parts of the library and the folders added.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, FontWeight, SharedString, TestSupportExt, div, px};

use crate::app::EuphoriaApp;
use crate::format;
use crate::library::LibraryFilter;
use crate::theme::Palette;
use crate::views::{LABEL_TEXT_SIZE, section_label};

const SIDEBAR_WIDTH: f32 = 220.0;

const PRIVACY_NOTE: &str = if cfg!(target_os = "macos") {
    "Works offline. Nothing leaves this Mac."
} else {
    "Works offline. Nothing leaves this computer."
};

/// The sidebar.
pub(crate) fn render(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let library = app.session.library();
    let counts = library.counts();

    let folder_rows: Vec<AnyElement> = library
        .folders()
        .iter()
        .enumerate()
        .map(|(folder_index, folder)| {
            let name = folder
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| folder.to_string_lossy().into_owned());
            let filter = LibraryFilter::Folder(folder.clone());
            filter_row(
                ("folder", folder_index),
                name,
                None,
                filter,
                app,
                &palette,
                cx,
            )
        })
        .collect();

    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .w(px(SIDEBAR_WIDTH))
        .px(px(8.0))
        .py(px(12.0))
        .gap(px(1.0))
        .bg(palette.sidebar)
        .border_r_1()
        .border_color(palette.line)
        .child(section_label("Library", &palette).px(px(8.0)).py(px(4.0)))
        .child(filter_row(
            "all-tracks",
            "All tracks",
            Some(counts.total),
            LibraryFilter::AllTracks,
            app,
            &palette,
            cx,
        ))
        .child(filter_row(
            "needs-review",
            "Needs review",
            Some(counts.failed),
            LibraryFilter::NeedsReview,
            app,
            &palette,
            cx,
        ))
        .child(
            row("Missing files", Some(0), false, &palette)
                .id("missing-files")
                .opacity(0.6),
        )
        .child(
            section_label("Folders", &palette)
                .px(px(8.0))
                .pt(px(16.0))
                .pb(px(4.0)),
        )
        .when(folder_rows.is_empty(), |sidebar| {
            sidebar.child(
                div()
                    .px(px(8.0))
                    .py(px(4.0))
                    .text_color(palette.text_tertiary)
                    .child("No folders yet"),
            )
        })
        .children(folder_rows)
        .child(
            div()
                .mt_auto()
                .p(px(8.0))
                .text_size(LABEL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child(PRIVACY_NOTE),
        )
}

/// A row that shows one part of the library when clicked.
fn filter_row(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    count: Option<usize>,
    filter: LibraryFilter,
    app: &EuphoriaApp,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let is_current = app.filter == filter;
    let hover_background = palette.raised;
    row(label, count, is_current, palette)
        .id(id)
        .cursor_pointer()
        .when(!is_current, |row| {
            row.hover(move |style| style.bg(hover_background.opacity(0.6)))
        })
        .on_click(cx.listener(move |this, _, _, cx| this.set_filter(filter.clone(), cx)))
        .test_support()
        .into_any_element()
}

fn row(
    label: impl Into<SharedString>,
    count: Option<usize>,
    is_current: bool,
    palette: &Palette,
) -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .h(px(28.0))
        .px(px(8.0))
        .rounded(px(6.0))
        .map(|row| {
            if is_current {
                row.bg(palette.raised)
                    .text_color(palette.text)
                    .font_weight(FontWeight::MEDIUM)
            } else {
                row.text_color(palette.text_secondary)
            }
        })
        .child(div().min_w_0().truncate().child(label.into()))
        .children(count.map(|count| {
            div()
                .flex_shrink_0()
                .font_weight(FontWeight::NORMAL)
                .text_color(palette.text_tertiary)
                .child(format::count(count))
        }))
}
