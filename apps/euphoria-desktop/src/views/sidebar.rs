// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The column on the left: parts of the library, and the folders added as a
//! tree of the subfolders that hold tracks. Only the folder rows in view are
//! built, so a tree of hundreds of folders stays quick.

use gpui::component::scroll::Scrollbar;
use gpui::component::tooltip::Tooltip;
use gpui::component::{Icon, IconName};
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, ElementId, FontWeight, SharedString, TestSupportExt, div, px,
    uniform_list,
};

use crate::app::EuphoriaApp;
use crate::format;
use crate::library::LibraryFilter;
use crate::panels::SIDEBAR_WIDTH;
use crate::theme::Palette;
use crate::views::{LABEL_TEXT_SIZE, section_label};

const PRIVACY_NOTE: &str = if cfg!(target_os = "macos") {
    "Works offline. Nothing leaves this Mac."
} else {
    "Works offline. Nothing leaves this computer."
};

/// Height of every row of the sidebar.
const ROW_HEIGHT: f32 = 28.0;

/// How far each level of the folder tree is set in.
const INDENT_PER_LEVEL: f32 = 12.0;

/// Width of the disclosure triangle, and of the space left where a folder
/// has nothing inside to show.
const DISCLOSURE_WIDTH: f32 = 16.0;

/// The sidebar.
pub(crate) fn render(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let counts = app.sidebar_counts();
    let hover_background = palette.raised;

    let in_library = app.playlist.is_none() && !app.showing_favorites;
    let favorites_are_current = app.playlist.is_none() && app.showing_favorites;
    let all_tracks_is_current =
        in_library && app.filter == LibraryFilter::AllTracks && app.folder.is_none();
    let needs_review_is_current = in_library && app.filter == LibraryFilter::NeedsReview;
    let quality_is_current = in_library && app.filter == LibraryFilter::QualityToCheck;

    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .min_h_0()
        .w(px(SIDEBAR_WIDTH))
        .px(px(8.0))
        .py(px(12.0))
        .gap(px(1.0))
        .bg(palette.sidebar)
        .border_r_1()
        .border_color(palette.line)
        .child(section_label("Library", &palette).px(px(8.0)).py(px(4.0)))
        .child(
            row(
                "All tracks",
                Some(counts.total),
                all_tracks_is_current,
                &palette,
            )
            .id("all-tracks")
            .cursor_pointer()
            .when(!all_tracks_is_current, |row| {
                row.hover(move |style| style.bg(hover_background.opacity(0.6)))
            })
            .on_click(cx.listener(|this, _, _, cx| this.show_all_tracks(cx)))
            .test_support(),
        )
        .child(
            row(
                "Needs review",
                Some(counts.needs_review),
                needs_review_is_current,
                &palette,
            )
            .id("needs-review")
            .cursor_pointer()
            .when(!needs_review_is_current, |row| {
                row.hover(move |style| style.bg(hover_background.opacity(0.6)))
            })
            .on_click(cx.listener(|this, _, _, cx| this.show_tracks_needing_review(cx)))
            .test_support(),
        )
        .child(
            row(
                "Quality to check",
                Some(counts.quality_to_check),
                quality_is_current,
                &palette,
            )
            .id("quality-to-check")
            .cursor_pointer()
            .when(!quality_is_current, |row| {
                row.hover(move |style| style.bg(hover_background.opacity(0.6)))
            })
            .on_click(cx.listener(|this, _, _, cx| this.show_tracks_with_quality_to_check(cx)))
            .test_support(),
        )
        .child(
            row(
                "Favorites",
                Some(app.session.favorites().tracks().len()),
                favorites_are_current,
                &palette,
            )
            .id("favorites")
            .cursor_pointer()
            .when(!favorites_are_current, |row| {
                row.hover(move |style| style.bg(hover_background.opacity(0.6)))
            })
            .on_click(cx.listener(|this, _, _, cx| this.show_favorites(cx)))
            .test_support(),
        )
        .child(
            row("Missing files", Some(0), false, &palette)
                .id("missing-files")
                .opacity(0.6),
        )
        .child(
            section_label("Folders", &palette)
                .flex_shrink_0()
                .px(px(8.0))
                .pt(px(16.0))
                .pb(px(4.0)),
        )
        .child(folder_tree(app, &palette, cx))
        .child(crate::views::playlists::sidebar_section(app, cx))
        .child(
            div()
                .flex_shrink_0()
                .p(px(8.0))
                .text_size(LABEL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child(PRIVACY_NOTE),
        )
}

/// The added folders and their subfolders, scrolling when taller than the
/// room left.
fn folder_tree(
    app: &EuphoriaApp,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    if app.folder_rows.is_empty() {
        return div()
            .flex_1()
            .child(
                div()
                    .px(px(8.0))
                    .py(px(4.0))
                    .text_color(palette.text_tertiary)
                    .child("No folders yet"),
            )
            .into_any_element();
    }
    div()
        .id("folder-tree")
        .relative()
        .flex_1()
        .min_h_0()
        .track_focus(&app.tree_focus)
        .on_key_down(cx.listener(|this, event, _, cx| this.tree_key_down(event, cx)))
        .child(
            uniform_list(
                "folders",
                app.folder_rows.len(),
                cx.processor(
                    |this: &mut EuphoriaApp, range: std::ops::Range<usize>, _, cx| {
                        range
                            .filter_map(|row_index| folder_row(this, row_index, cx))
                            .collect::<Vec<_>>()
                    },
                ),
            )
            .track_scroll(&app.folder_scroll)
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .right_0()
                .bottom_0()
                .w(Scrollbar::width())
                .child(Scrollbar::vertical(&app.folder_scroll).viewport_from_layout()),
        )
        .test_support()
        .into_any_element()
}

/// The element id of the row of the folder at `node_index` in the tree.
pub(crate) fn folder_row_id(node_index: usize) -> ElementId {
    ("folder", node_index).into()
}

/// The element id of the disclosure triangle of the folder at `node_index`.
pub(crate) fn disclosure_id(node_index: usize) -> ElementId {
    ("disclosure", node_index).into()
}

/// The folder listed at `row_index`: a disclosure triangle when it holds
/// folders, its name, and how many tracks are in it and below it.
fn folder_row(
    app: &EuphoriaApp,
    row_index: usize,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<AnyElement> {
    let palette = app.theme.palette;
    let node_index = *app.folder_rows.get(row_index)?;
    let node = app.folder_tree.node(node_index)?;
    let is_current = app.folder.as_ref() == Some(&node.path);
    let is_expanded = app.expanded_folders.contains(&node.path);
    let hover_background = palette.raised;
    let full_path: SharedString = node.path.to_string_lossy().into_owned().into();
    let shown_path = node.path.clone();
    let toggled_path = node.path.clone();

    let disclosure = if node.children.is_empty() {
        div()
            .w(px(DISCLOSURE_WIDTH))
            .flex_shrink_0()
            .into_any_element()
    } else {
        div()
            .id(disclosure_id(node_index))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .size(px(DISCLOSURE_WIDTH))
            .rounded(px(4.0))
            .text_color(palette.text_tertiary)
            .hover(move |style| style.bg(hover_background))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.toggle_folder(toggled_path.clone(), cx);
            }))
            .child(
                Icon::new(if is_expanded {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .size(px(12.0)),
            )
            .test_support()
            .into_any_element()
    };

    Some(
        row_after(
            Some(disclosure),
            node.name.clone(),
            Some(node.track_count),
            is_current,
            &palette,
        )
        .id(folder_row_id(node_index))
        .w_full()
        .gap(px(4.0))
        .pl(px(2.0 + INDENT_PER_LEVEL * node.depth as f32))
        .cursor_pointer()
        .when(!is_current, |row| {
            row.hover(move |style| style.bg(hover_background.opacity(0.6)))
        })
        .tooltip(move |window, cx| Tooltip::new(full_path.clone()).build(window, cx))
        .on_click(cx.listener(move |this, _, window, cx| {
            this.tree_focus.focus(window, cx);
            this.show_folder(shown_path.clone(), cx);
        }))
        .test_support()
        .into_any_element(),
    )
}

fn row(
    label: impl Into<SharedString>,
    count: Option<usize>,
    is_current: bool,
    palette: &Palette,
) -> Div {
    row_after(None, label, count, is_current, palette)
}

/// A row with `leading` before its label.
fn row_after(
    leading: Option<AnyElement>,
    label: impl Into<SharedString>,
    count: Option<usize>,
    is_current: bool,
    palette: &Palette,
) -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(8.0))
        .h(px(ROW_HEIGHT))
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
        .children(leading)
        .child(div().flex_1().min_w_0().truncate().child(label.into()))
        .children(count.map(|count| {
            div()
                .flex_shrink_0()
                .font_weight(FontWeight::NORMAL)
                .text_color(palette.text_tertiary)
                .child(format::count(count))
        }))
}
