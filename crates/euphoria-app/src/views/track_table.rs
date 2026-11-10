// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The track table: a heading, sortable column headers and one row per
//! track. Only the rows in view are built, so the table stays quick however
//! long the library is.

use gpui::component::scroll::Scrollbar;
use gpui::component::{Icon, IconName};
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, FontWeight, SharedString, TestSupportExt, div, px, uniform_list,
};

use crate::app::EuphoriaApp;
use crate::format;
use crate::library::{LibraryFilter, SortColumn, Track, TrackStatus};
use crate::theme::Palette;
use crate::views::key_chip::key_chip;
use crate::views::{LABEL_TEXT_SIZE, SMALL_TEXT_SIZE};

const HEADER_HEIGHT: f32 = 28.0;
const COLUMN_GAP: f32 = 12.0;
const SIDE_PADDING: f32 = 14.0;

/// How wide a column is.
#[derive(Clone, Copy)]
enum ColumnWidth {
    /// A fixed number of pixels.
    Fixed(f32),
    /// A share of the space the fixed columns leave.
    Share(f32),
}

/// One column of the table.
#[derive(Clone, Copy)]
struct Column {
    label: &'static str,
    width: ColumnWidth,
    is_right_aligned: bool,
    sorts_by: Option<SortColumn>,
}

const NUMBER: Column = Column {
    label: "#",
    width: ColumnWidth::Fixed(34.0),
    is_right_aligned: true,
    sorts_by: None,
};
const TITLE: Column = Column {
    label: "Title",
    width: ColumnWidth::Share(2.2),
    is_right_aligned: false,
    sorts_by: Some(SortColumn::Title),
};
const KEY: Column = Column {
    label: "Key",
    width: ColumnWidth::Fixed(58.0),
    is_right_aligned: false,
    sorts_by: Some(SortColumn::Key),
};
const MUSICAL_KEY: Column = Column {
    label: "Musical key",
    width: ColumnWidth::Fixed(84.0),
    is_right_aligned: false,
    sorts_by: Some(SortColumn::Key),
};
const BPM: Column = Column {
    label: "BPM",
    width: ColumnWidth::Fixed(60.0),
    is_right_aligned: true,
    sorts_by: Some(SortColumn::Bpm),
};
const TIME: Column = Column {
    label: "Time",
    width: ColumnWidth::Fixed(48.0),
    is_right_aligned: true,
    sorts_by: Some(SortColumn::Duration),
};
const FORMAT: Column = Column {
    label: "Format",
    width: ColumnWidth::Fixed(56.0),
    is_right_aligned: false,
    sorts_by: Some(SortColumn::Format),
};
const FOLDER: Column = Column {
    label: "Folder",
    width: ColumnWidth::Share(1.1),
    is_right_aligned: false,
    sorts_by: Some(SortColumn::Folder),
};

const COLUMNS: [Column; 8] = [NUMBER, TITLE, KEY, MUSICAL_KEY, BPM, TIME, FORMAT, FOLDER];

/// The table with its heading.
pub(crate) fn render(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;

    let rows = if app.visible.is_empty() {
        div()
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .text_color(palette.text_tertiary)
            .child(nothing_to_show(app))
    } else {
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .track_focus(&app.table_focus)
            .on_key_down(cx.listener(|this, event, _, cx| this.table_key_down(event, cx)))
            .child(
                uniform_list(
                    "tracks",
                    app.visible.len(),
                    cx.processor(
                        |this: &mut EuphoriaApp, range: std::ops::Range<usize>, _, cx| {
                            range
                                .filter_map(|row_index| row(this, row_index, cx))
                                .collect::<Vec<_>>()
                        },
                    ),
                )
                .track_scroll(&app.table_scroll)
                .size_full(),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .bottom_0()
                    .w(Scrollbar::width())
                    .child(Scrollbar::vertical(&app.table_scroll).viewport_from_layout()),
            )
    };

    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .child(heading(app, &palette))
        .child(header(app, &palette, cx))
        .child(rows)
}

fn nothing_to_show(app: &EuphoriaApp) -> &'static str {
    if app.scans_in_progress > 0 {
        "Looking for audio files"
    } else if app.session.library().tracks().is_empty() {
        "No tracks yet"
    } else if app.filter == LibraryFilter::NeedsReview {
        "No tracks need review"
    } else {
        "No tracks match"
    }
}

/// The name of the part of the library on show and how many tracks it has.
fn heading(app: &EuphoriaApp, palette: &Palette) -> Div {
    let title: SharedString = match &app.filter {
        LibraryFilter::AllTracks => "All tracks".into(),
        LibraryFilter::NeedsReview => "Needs review".into(),
        LibraryFilter::Folder(folder) => folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| folder.to_string_lossy().into_owned())
            .into(),
    };
    let count = format::track_summary(
        app.visible.len(),
        app.session.library().counts().total,
        app.session.cached_count(),
    );

    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_between()
        .gap(px(8.0))
        .px(px(SIDE_PADDING))
        .py(px(8.0))
        .border_b_1()
        .border_color(palette.line)
        .child(
            div()
                .min_w_0()
                .truncate()
                .text_size(px(14.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child(count),
        )
}

fn header(app: &EuphoriaApp, palette: &Palette, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let cells = COLUMNS.iter().enumerate().map(|(column_index, column)| {
        let is_sorted = app
            .sort_order
            .filter(|order| column.sorts_by == Some(order.column));
        let hover_colour = palette.text;
        cell(column)
            .id(("column", column_index))
            .gap(px(4.0))
            .items_center()
            .when(is_sorted.is_some(), |cell| cell.text_color(palette.text))
            .child(column.label)
            .children(is_sorted.map(|order| {
                Icon::new(if order.descending {
                    IconName::ArrowDown
                } else {
                    IconName::ArrowUp
                })
                .size(px(10.0))
            }))
            .when_some(column.sorts_by, |cell, sort_column| {
                cell.cursor_pointer()
                    .hover(move |style| style.text_color(hover_colour))
                    .on_click(cx.listener(move |this, _, _, cx| this.sort_by(sort_column, cx)))
            })
            .test_support()
    });

    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(COLUMN_GAP))
        .h(px(HEADER_HEIGHT))
        .px(px(SIDE_PADDING))
        .border_b_1()
        .border_color(palette.line)
        .text_size(LABEL_TEXT_SIZE)
        .font_weight(FontWeight::MEDIUM)
        .text_color(palette.text_tertiary)
        .children(cells)
}

/// An empty cell sized and aligned for `column`.
fn cell(column: &Column) -> Div {
    let mut cell = div().flex().min_w_0().overflow_hidden();
    match column.width {
        ColumnWidth::Fixed(width) => cell = cell.w(px(width)).flex_shrink_0(),
        ColumnWidth::Share(share) => {
            cell = cell.flex_basis(px(0.0));
            cell.style().flex_grow = Some(share);
        }
    }
    if column.is_right_aligned {
        cell = cell.justify_end();
    }
    cell
}

/// The row at `row_index` of the visible tracks.
fn row(
    app: &EuphoriaApp,
    row_index: usize,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<AnyElement> {
    let theme = &app.theme;
    let palette = theme.palette;
    let track_index = *app.visible.get(row_index)?;
    let track = app.session.library().track(track_index)?;
    let is_selected = app.selected == Some(track_index);
    let is_pending = track.status == TrackStatus::Pending;

    let key_cell = match (&track.status, track.key()) {
        (_, Some(key)) => cell(&KEY).child(key_chip(&key, theme)),
        (TrackStatus::Pending, _) => cell(&KEY)
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_tertiary)
            .child("Pending"),
        (TrackStatus::Failed(_), _) => cell(&KEY)
            .text_size(SMALL_TEXT_SIZE)
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette.danger)
            .child("Failed"),
        (TrackStatus::Analyzed(_), None) => cell(&KEY).child(missing_value(&palette)),
    };

    Some(
        div()
            .id(("track", track_index))
            .flex()
            .w_full()
            .items_center()
            .gap(px(COLUMN_GAP))
            .h(theme.row_height)
            .px(px(SIDE_PADDING))
            .map(|row| {
                if is_selected {
                    row.bg(palette.selection)
                } else {
                    row.hover(move |style| style.bg(palette.raised.opacity(0.5)))
                }
            })
            .on_click(cx.listener(move |this, _, window, cx| {
                this.table_focus.focus(window, cx);
                this.select(track_index, cx);
            }))
            .child(
                cell(&NUMBER)
                    .text_color(palette.text_tertiary)
                    .child(format::count(row_index + 1)),
            )
            .child(
                cell(&TITLE).child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .when(is_pending, |title| title.text_color(palette.text_secondary))
                        .child(track.file_name.clone()),
                ),
            )
            .child(key_cell)
            .child(
                cell(&MUSICAL_KEY)
                    .text_color(palette.text_secondary)
                    .children(track.key().map(|key| key.to_string())),
            )
            .child(cell(&BPM).child(optional_text(track, track.bpm().map(format::bpm), &palette)))
            .child(
                cell(&TIME)
                    .text_color(palette.text_secondary)
                    .children(track.duration_seconds().map(format::duration)),
            )
            .child(
                cell(&FORMAT)
                    .text_color(palette.text_tertiary)
                    .child(track.format.clone()),
            )
            .child(
                cell(&FOLDER).child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(palette.text_tertiary)
                        .child(track.folder.clone()),
                ),
            )
            .test_support()
            .into_any_element(),
    )
}

/// The value of an analyzed track, a dash when analysis found none, and
/// nothing while the track is pending or failed.
fn optional_text(track: &Track, value: Option<String>, palette: &Palette) -> Div {
    match value {
        Some(text) => div().child(text),
        None if track.analysis().is_some() => missing_value(palette),
        None => div(),
    }
}

fn missing_value(palette: &Palette) -> Div {
    div().text_color(palette.text_tertiary).child("\u{2013}")
}
