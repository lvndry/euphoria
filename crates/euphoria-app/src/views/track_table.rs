// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The track table: a heading, sortable column headers and one row per
//! track. Only the rows in view are built, so the table stays quick however
//! long the library is.

use std::collections::HashSet;

use gpui::component::scroll::Scrollbar;
use gpui::component::tooltip::Tooltip;
use gpui::component::{Icon, IconName};
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, Font, FontWeight, SharedString, TestSupportExt, div, px, relative,
    uniform_list,
};

use crate::app::EuphoriaApp;
use crate::folder_tree::folder_name;
use crate::format;
use crate::library::{LibraryFilter, NOT_FOUND, SortColumn, Track, TrackStatus};
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
    /// As wide as the folder paths shown need, within limits; see
    /// [`TableLayout`].
    FolderPaths,
}

/// The widest visible folder label at [`super::TEXT_SIZE`]. Paths shared by
/// several tracks are measured once, and playback frames reuse the result.
#[derive(Default)]
pub(crate) struct FolderPathWidths {
    paths: HashSet<String>,
    font: Option<Font>,
    widest: Option<f32>,
}

impl FolderPathWidths {
    /// Updates the labels after filtering or changing folders. Sorting and
    /// analysis results that leave the same labels shown keep their width.
    pub(crate) fn set_paths<'path>(&mut self, paths: impl Iterator<Item = &'path str>) {
        let paths: HashSet<&str> = paths.filter(|path| !path.is_empty()).collect();
        if paths.len() != self.paths.len() || paths.iter().any(|path| !self.paths.contains(*path)) {
            self.paths = paths.into_iter().map(str::to_string).collect();
            self.widest = None;
        }
    }

    /// Measures every distinct path in the font actually used by the table.
    /// Character count cannot order widths in a proportional font.
    pub(crate) fn widest(&mut self, font: Font, mut measure: impl FnMut(&str) -> f32) -> f32 {
        if self.font.as_ref() != Some(&font) {
            self.font = Some(font);
            self.widest = None;
        }
        *self.widest.get_or_insert_with(|| {
            self.paths
                .iter()
                .map(|path| measure(path))
                .fold(0.0, f32::max)
        })
    }
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
const ENERGY: Column = Column {
    label: "Energy",
    width: ColumnWidth::Fixed(86.0),
    is_right_aligned: false,
    sorts_by: Some(SortColumn::Energy),
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
    width: ColumnWidth::FolderPaths,
    is_right_aligned: false,
    sorts_by: Some(SortColumn::Folder),
};

const COLUMNS: [Column; 9] = [
    NUMBER,
    TITLE,
    KEY,
    MUSICAL_KEY,
    BPM,
    ENERGY,
    TIME,
    FORMAT,
    FOLDER,
];

/// The narrowest the Title column is made to give the other columns room.
const TITLE_KEEPS: f32 = 160.0;

/// The narrowest the Folder column is drawn, so that its header stays
/// readable. With less room than this it is left out.
const NARROWEST_FOLDER: f32 = 56.0;

/// The columns left out when the table is too narrow for all of them,
/// first to last. The title, the key, the tempo and the energy always stay.
const LEFT_OUT_FIRST: [Column; 5] = [FORMAT, FOLDER, TIME, MUSICAL_KEY, NUMBER];

/// Which columns the table shows and how wide the Folder column is, for the
/// width the table has.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct TableLayout {
    left_out: Vec<&'static str>,
    /// The width of the Folder column.
    pub(crate) folder_width: f32,
}

impl TableLayout {
    /// The layout of a table `table_width` wide whose longest folder path is
    /// `longest_path` wide.
    ///
    /// The Title and Folder columns share what the other columns leave. The
    /// Folder column takes what its longest path needs, so paths show in
    /// full when there is room, but the Title column keeps at least
    /// [`TITLE_KEEPS`]; a path that does not fit is cut short. When even
    /// that is not possible, columns are left out in the order of
    /// [`LEFT_OUT_FIRST`] until the Title column has its room.
    pub(crate) fn for_width(table_width: f32, longest_path: f32) -> Self {
        let wanted_folder = longest_path.ceil().max(NARROWEST_FOLDER);
        let mut left_out: Vec<&'static str> = Vec::new();
        let mut candidates = LEFT_OUT_FIRST.iter();
        loop {
            let shown: Vec<&Column> = COLUMNS
                .iter()
                .filter(|column| !left_out.contains(&column.label))
                .collect();
            let fixed: f32 = shown
                .iter()
                .map(|column| match column.width {
                    ColumnWidth::Fixed(width) => width,
                    ColumnWidth::Share(_) | ColumnWidth::FolderPaths => 0.0,
                })
                .sum();
            let gaps = COLUMN_GAP * shown.len().saturating_sub(1) as f32;
            let shared = table_width - 2.0 * SIDE_PADDING - gaps - fixed;
            let shows_folder = !left_out.contains(&FOLDER.label);
            let title_room = if shows_folder {
                shared - NARROWEST_FOLDER
            } else {
                shared
            };
            let Some(next) = candidates.next().filter(|_| title_room < TITLE_KEEPS) else {
                let folder_width = if shows_folder {
                    wanted_folder.min(shared - TITLE_KEEPS).max(0.0)
                } else {
                    0.0
                };
                return Self {
                    left_out,
                    folder_width,
                };
            };
            left_out.push(next.label);
        }
    }

    fn shows(&self, column: &Column) -> bool {
        !self.left_out.contains(&column.label)
    }
}

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
    } else if app.filter == LibraryFilter::NeedsReview && app.folder.is_some() {
        "No tracks in this folder need review"
    } else if app.filter == LibraryFilter::NeedsReview {
        "No tracks need review"
    } else {
        "No tracks match"
    }
}

/// The name of the part of the library on show and how many tracks it has.
fn heading(app: &EuphoriaApp, palette: &Palette) -> Div {
    let folder = app.folder.as_deref().map(folder_name);
    let title: SharedString = match (app.filter, folder) {
        (LibraryFilter::AllTracks, None) => "All tracks".into(),
        (LibraryFilter::NeedsReview, None) => "Needs review".into(),
        (LibraryFilter::AllTracks, Some(folder)) => folder.into(),
        (LibraryFilter::NeedsReview, Some(folder)) => format!("Needs review in {folder}").into(),
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
    let layout = &app.table_layout;
    let cells = COLUMNS
        .iter()
        .enumerate()
        .filter(|(_, column)| layout.shows(column))
        .map(|(column_index, column)| {
            let is_sorted = app
                .sort_order
                .filter(|order| column.sorts_by == Some(order.column));
            let hover_colour = palette.text;
            cell(column, layout.folder_width)
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

/// An empty cell sized and aligned for `column`, with the Folder column
/// `folder_width` wide.
fn cell(column: &Column, folder_width: f32) -> Div {
    let mut cell = div().flex().min_w_0().overflow_hidden();
    match column.width {
        ColumnWidth::Fixed(width) => cell = cell.w(px(width)).flex_shrink_0(),
        ColumnWidth::FolderPaths => cell = cell.w(px(folder_width)).flex_shrink_0(),
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
    let is_unfinished = track.is_unfinished();
    let layout = &app.table_layout;

    let key_cell = match (&track.status, track.key()) {
        (_, Some(key)) => cell(&KEY, 0.0).child(key_chip(&key, theme)),
        (TrackStatus::Queued, _) => cell(&KEY, 0.0)
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_tertiary)
            .child("Queued"),
        (TrackStatus::Analyzing, _) => cell(&KEY, 0.0)
            .text_size(SMALL_TEXT_SIZE)
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette.accent)
            .child("Analyzing"),
        (TrackStatus::Failed(_), _) => cell(&KEY, 0.0)
            .text_size(SMALL_TEXT_SIZE)
            .font_weight(FontWeight::MEDIUM)
            .text_color(palette.danger)
            .child("Failed"),
        (TrackStatus::Analyzed(_), None) => cell(&KEY, 0.0).child(missing_value(&palette)),
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
            .when(layout.shows(&NUMBER), |row| {
                row.child(
                    cell(&NUMBER, 0.0)
                        .text_color(palette.text_tertiary)
                        .child(format::count(row_index + 1)),
                )
            })
            .child(
                cell(&TITLE, 0.0).child(
                    div()
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .when(is_unfinished, |title| {
                            title.text_color(palette.text_secondary)
                        })
                        .child(track.file_name.clone()),
                ),
            )
            .child(key_cell)
            .when(layout.shows(&MUSICAL_KEY), |row| {
                row.child(
                    cell(&MUSICAL_KEY, 0.0)
                        .text_color(palette.text_secondary)
                        .children(track.key().map(|key| key.to_string())),
                )
            })
            .child(cell(&BPM, 0.0).child(optional_text(
                track,
                track.bpm().map(format::bpm),
                &palette,
            )))
            .child(cell(&ENERGY, 0.0).child(energy(track, &palette)))
            .when(layout.shows(&TIME), |row| {
                row.child(
                    cell(&TIME, 0.0)
                        .text_color(palette.text_secondary)
                        .children(track.duration_seconds().map(format::duration)),
                )
            })
            .when(layout.shows(&FORMAT), |row| {
                row.child(
                    cell(&FORMAT, 0.0)
                        .text_color(palette.text_tertiary)
                        .child(track.format.clone()),
                )
            })
            .when(layout.shows(&FOLDER), |row| {
                row.child(cell(&FOLDER, layout.folder_width).child(folder_cell(
                    app,
                    track_index,
                    &palette,
                )))
            })
            .test_support()
            .into_any_element(),
    )
}

/// Where the track's folder is, from the folder on show: a path that keeps
/// its last folder names in view when it is too long, with the whole path in
/// a tooltip, or a quiet dash for a track directly in the folder on show.
fn folder_cell(app: &EuphoriaApp, track_index: usize, palette: &Palette) -> AnyElement {
    let Some(label) = app.folder_column.label(track_index) else {
        return div().into_any_element();
    };
    if label.text.is_empty() {
        return missing_value(palette).into_any_element();
    }
    let full_path: SharedString = label.full_path.to_string_lossy().into_owned().into();
    div()
        .id(("folder-cell", track_index))
        .min_w_0()
        .truncate()
        .text_ellipsis_start()
        .text_color(palette.text_secondary)
        .tooltip(move |window, cx| Tooltip::new(full_path.clone()).build(window, cx))
        .child(label.text.clone())
        .test_support()
        .into_any_element()
}

/// The value of an analyzed track, a dash when analysis found none, and
/// nothing while the track is waiting, being analyzed or failed.
fn optional_text(track: &Track, value: Option<String>, palette: &Palette) -> Div {
    match track.value_or_dash(value) {
        Some(text) if text == NOT_FOUND => missing_value(palette),
        Some(text) => div().child(text),
        None => div(),
    }
}

fn missing_value(palette: &Palette) -> Div {
    div().text_color(palette.text_tertiary).child(NOT_FOUND)
}

/// The energy rating as a number beside a bar that fills with it, or a dash
/// when the analyzed track has none.
fn energy(track: &Track, palette: &Palette) -> Div {
    let Some(rating) = track.energy() else {
        return optional_text(track, None, palette);
    };
    div()
        .flex()
        .flex_1()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .flex_shrink_0()
                .justify_end()
                .w(px(16.0))
                .text_color(palette.text_secondary)
                .child(rating.to_string()),
        )
        .child(
            div()
                .flex_1()
                .h(px(4.0))
                .rounded(px(2.0))
                .bg(palette.raised)
                .child(
                    div()
                        .h_full()
                        .w(relative(f32::from(rating) / 10.0))
                        .rounded(px(2.0))
                        .bg(palette.text_tertiary),
                ),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table in a 1440 wide window beside the sidebar and an inspector
    /// of the usual width.
    const USUAL_TABLE: f32 = 1440.0 - 220.0 - 320.0;

    fn left_out(layout: &TableLayout) -> Vec<&'static str> {
        layout.left_out.clone()
    }

    #[test]
    fn a_shorter_but_wider_path_decides_the_folder_width() {
        let mut paths = FolderPathWidths::default();
        paths.set_paths(["iiiiiiiiiiiiiiiiiiii", "WWWWWWWWWW"].into_iter());
        // Proportional widths are injected because the headless renderer
        // gives every character the same width.
        let width = paths.widest(Font::default(), |path| match path {
            "iiiiiiiiiiiiiiiiiiii" => 63.0,
            "WWWWWWWWWW" => 125.0,
            _ => unreachable!(),
        });
        assert_eq!(width, 125.0);
        assert_eq!(
            TableLayout::for_width(USUAL_TABLE, width).folder_width,
            125.0
        );
    }

    #[test]
    fn repeated_paths_and_frames_only_measure_each_path_once() {
        let mut paths = FolderPathWidths::default();
        paths.set_paths(["deep", "house/warmup", "deep"].into_iter());
        let mut measured = Vec::new();
        let width = paths.widest(Font::default(), |path| {
            measured.push(path.to_string());
            path.len() as f32
        });
        measured.sort();
        assert_eq!(measured, ["deep", "house/warmup"]);

        paths.set_paths(["house/warmup", "deep", "house/warmup"].into_iter());
        assert_eq!(
            paths.widest(Font::default(), |_| panic!(
                "unchanged paths were remeasured"
            )),
            width
        );
    }

    #[test]
    fn filtering_or_changing_folders_remeasures_the_paths_left_on_show() {
        let mut paths = FolderPathWidths::default();
        paths.set_paths(["deep", "house/warmup"].into_iter());
        assert_eq!(
            paths.widest(Font::default(), |path| path.len() as f32),
            12.0
        );

        paths.set_paths(["deep"].into_iter());
        assert_eq!(paths.widest(Font::default(), |path| path.len() as f32), 4.0);
        paths.set_paths(["another/folder"].into_iter());
        assert_eq!(
            paths.widest(Font::default(), |path| path.len() as f32),
            14.0
        );

        paths.set_paths([""].into_iter());
        assert_eq!(
            paths.widest(Font::default(), |_| panic!("an empty label was measured")),
            0.0
        );
    }

    #[test]
    fn changing_the_table_font_remeasures_the_same_paths() {
        let mut paths = FolderPathWidths::default();
        paths.set_paths(["deep"].into_iter());
        assert_eq!(paths.widest(Font::default(), |_| 28.0), 28.0);
        let bold = Font {
            weight: FontWeight::BOLD,
            ..Font::default()
        };
        assert_eq!(paths.widest(bold, |_| 32.0), 32.0);
    }

    #[test]
    fn a_path_that_fits_shows_in_full_with_every_column() {
        let layout = TableLayout::for_width(USUAL_TABLE, 133.0);
        assert!(left_out(&layout).is_empty());
        assert_eq!(layout.folder_width, 133.0);
    }

    #[test]
    fn a_short_path_keeps_the_header_readable() {
        assert_eq!(TableLayout::for_width(USUAL_TABLE, 10.0).folder_width, 56.0);
        assert_eq!(TableLayout::for_width(USUAL_TABLE, 0.0).folder_width, 56.0);
    }

    #[test]
    fn a_long_path_is_cut_short_where_the_title_needs_its_room() {
        let layout = TableLayout::for_width(USUAL_TABLE, 400.0);
        assert!(left_out(&layout).is_empty());
        assert_eq!(layout.folder_width, 350.0 - TITLE_KEEPS);
    }

    #[test]
    fn a_narrow_table_leaves_out_format_then_folder_then_more() {
        let without_sidebar = TableLayout::for_width(960.0 - 320.0, 133.0);
        assert_eq!(left_out(&without_sidebar), ["Format", "Folder"]);
        assert_eq!(without_sidebar.folder_width, 0.0);

        let beside_the_sidebar = TableLayout::for_width(960.0 - 220.0 - 320.0, 133.0);
        assert_eq!(
            left_out(&beside_the_sidebar),
            ["Format", "Folder", "Time", "Musical key", "#"]
        );

        let strip_layout = TableLayout::for_width(960.0 - 220.0, 133.0);
        assert_eq!(left_out(&strip_layout), ["Format"]);
        assert_eq!(strip_layout.folder_width, 98.0);
    }

    #[test]
    fn a_table_too_narrow_for_anything_keeps_the_four_columns_that_matter() {
        let layout = TableLayout::for_width(0.0, 133.0);
        for column in [TITLE, KEY, BPM, ENERGY] {
            assert!(layout.shows(&column), "{}", column.label);
        }
        assert_eq!(layout.folder_width, 0.0);
    }
}
