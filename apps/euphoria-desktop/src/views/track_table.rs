// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The track table: a heading, sortable column headers whose edges can be
//! dragged, and one row per track. Only the rows in view are built, so the
//! table stays quick however long the library is.

use std::collections::HashSet;

use gpui::component::scroll::Scrollbar;
use gpui::component::tooltip::Tooltip;
use gpui::component::{Icon, IconName};
use gpui::prelude::*;
use gpui::{
    AnyElement, ClickEvent, Context, CursorStyle, Div, Font, FontWeight, MouseButton,
    MouseDownEvent, SharedString, TestSupportExt, canvas, div, px, relative, uniform_list,
};

use crate::app::EuphoriaApp;
use crate::columns::{COLUMN_GAP, ColumnId, SIDE_PADDING};
use crate::folder_tree::folder_name;
use crate::format;
use crate::library::{LibraryFilter, NOT_FOUND, Track, TrackStatus};
use crate::playlist_flow::DraggedTracks;
use crate::playlists::PlaylistId;
use crate::theme::Palette;
use crate::views::favorites::star_button;
use crate::views::key_chip::key_chip;
use crate::views::playlists::{DragLabel, track_menu};
use crate::views::{LABEL_TEXT_SIZE, SMALL_TEXT_SIZE};
use gpui::component::menu::ContextMenuExt;

const HEADER_HEIGHT: f32 = 28.0;

/// The group a row's star shows on hover in.
const TRACK_ROW_GROUP: &str = "track-row";

/// Width of the grip on the edge between two column headers.
const EDGE_GRIP_WIDTH: f32 = 8.0;

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

/// The element id of the header of `column`.
pub(crate) fn header_id(column: ColumnId) -> (&'static str, usize) {
    ("column", column.position())
}

/// The element id of the grip on the right edge of `column`'s header.
pub(crate) fn edge_id(column: ColumnId) -> (&'static str, usize) {
    ("column-edge", column.position())
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
        .child(heading(app, &palette, cx))
        .child(header(app, &palette, cx))
        .child(rows)
}

fn nothing_to_show(app: &EuphoriaApp) -> &'static str {
    if app.playlist.is_some() && app.query.trim().is_empty() {
        "This playlist is empty. Drag tracks onto it in the sidebar, or right click tracks and \
         choose Add to playlist."
    } else if app.showing_favorites && app.query.trim().is_empty() {
        "No favorites yet. Click the star beside a track, or select tracks and press F."
    } else if app.scans_in_progress > 0 {
        "Looking for audio files"
    } else if app.session.library().tracks().is_empty() {
        "No tracks yet"
    } else if app.filter == LibraryFilter::NeedsReview && app.folder.is_some() {
        "Nothing to review in this folder: every key and tempo is rated medium or high, and \
         every file was analyzed"
    } else if app.filter == LibraryFilter::NeedsReview {
        "Nothing to review: every key and tempo is rated medium or high, and every file was \
         analyzed"
    } else if app.filter == LibraryFilter::QualityToCheck {
        "No tracks with a low bit rate or likely re-encoded"
    } else {
        "No tracks match"
    }
}

/// The name of the part of the library on show and how many tracks it has.
fn heading(app: &EuphoriaApp, palette: &Palette, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let folder = app.folder.as_deref().map(folder_name);
    let playlist = app.playlist.and_then(|id| app.session.playlists().get(id));
    let title: SharedString = match (app.filter, folder) {
        _ if app.showing_favorites => "Favorites".into(),
        _ if playlist.is_some() => playlist
            .map(|playlist| match &playlist.label {
                Some(label) => format!("{label} {}", playlist.name),
                None => playlist.name.clone(),
            })
            .unwrap_or_default()
            .into(),
        (LibraryFilter::AllTracks, None) => "All tracks".into(),
        (LibraryFilter::NeedsReview, None) => "Needs review".into(),
        (LibraryFilter::AllTracks, Some(folder)) => folder.into(),
        (LibraryFilter::NeedsReview, Some(folder)) => format!("Needs review in {folder}").into(),
        (LibraryFilter::QualityToCheck, None) => "Quality to check".into(),
        (LibraryFilter::QualityToCheck, Some(folder)) => {
            format!("Quality to check in {folder}").into()
        }
    };
    let count = match playlist {
        None if app.showing_favorites => {
            let shown = app.visible.len();
            let tracks = if shown == 1 { "track" } else { "tracks" };
            format!("{} {tracks}", format::count(shown))
        }
        Some(playlist) => {
            let shown = app.visible.len();
            let not_in_library = playlist.tracks.len() - app.playlist_positions.len();
            let tracks = if shown == 1 { "track" } else { "tracks" };
            match not_in_library {
                0 => format!("{} {tracks}", format::count(shown)),
                missing => format!(
                    "{} {tracks}, {} not in the library",
                    format::count(shown),
                    format::count(missing)
                ),
            }
        }
        None => format::track_summary(
            app.visible.len(),
            app.session.library().counts().total,
            app.session.cached_count(),
        ),
    };

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
                .flex()
                .flex_col()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .children(crate::views::playlists::set_summary(app, cx))
                .children(crate::views::favorites::favorites_summary(app, cx))
                .when(
                    app.filter == LibraryFilter::NeedsReview && playlist.is_none(),
                    |heading| {
                        heading.child(
                            div()
                                .id("review-explanation")
                                .truncate()
                                .text_size(SMALL_TEXT_SIZE)
                                .text_color(palette.text_secondary)
                                .child(REVIEW_EXPLANATION)
                                .test_support(),
                        )
                    },
                ),
        )
        .child(
            div()
                .flex_shrink_0()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child(count),
        )
}

/// Why tracks are listed under Needs review, under its heading.
const REVIEW_EXPLANATION: &str =
    "A key or tempo rated low confidence, or a file that could not be analyzed. Check them by ear.";

fn header(app: &EuphoriaApp, palette: &Palette, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let layout = &app.table_layout;
    let cells = layout
        .columns()
        .iter()
        .map(|(column, width)| {
            let column = *column;
            let is_sorted = app
                .sort_order
                .filter(|order| column.sorts_by() == Some(order.column));
            let hover_colour = palette.text;
            cell(column, *width)
                .id(header_id(column))
                .gap(px(4.0))
                .items_center()
                .when(is_sorted.is_some(), |cell| cell.text_color(palette.text))
                .child(div().min_w_0().truncate().child(column.label()))
                .children(is_sorted.map(|order| {
                    Icon::new(if order.descending {
                        IconName::ArrowDown
                    } else {
                        IconName::ArrowUp
                    })
                    .size(px(10.0))
                }))
                .when_some(column.sorts_by(), |cell, sort_column| {
                    cell.cursor_pointer()
                        .hover(move |style| style.text_color(hover_colour))
                        .on_click(cx.listener(move |this, _, _, cx| this.sort_by(sort_column, cx)))
                })
                .when(
                    column == ColumnId::Number
                        && app.playlist.is_some()
                        && app.sort_order.is_some(),
                    |cell| {
                        cell.cursor_pointer()
                            .hover(move |style| style.text_color(hover_colour))
                            .tooltip(|window, cx| {
                                Tooltip::new("Back to the playlist's own order").build(window, cx)
                            })
                            .on_click(cx.listener(|this, _, _, cx| this.show_playlist_order(cx)))
                    },
                )
                .test_support()
        })
        .collect::<Vec<_>>();
    let resizing = app.column_resize.map(|resize| resize.column);
    let edges: Vec<AnyElement> = layout
        .columns()
        .iter()
        .filter_map(|(column, width)| {
            let left = layout.left_of(*column)? + width + COLUMN_GAP / 2.0;
            Some(column_edge(
                *column,
                left,
                resizing == Some(*column),
                palette,
                cx,
            ))
        })
        .collect();
    let seen_bounds = app.table_header_bounds.clone();

    div()
        .relative()
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
        .child(
            canvas(
                move |bounds, _, _| seen_bounds.set(Some(bounds)),
                |_, _, _, _| (),
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .children(cells)
        .children(edges)
}

/// The grip on the edge after `column`, centred `centre` from the table's
/// left: a thin line that a drag moves and a double click fits to the
/// column's contents, as in a macOS table.
fn column_edge(
    column: ColumnId,
    centre: f32,
    is_held: bool,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let line = palette.line;
    let highlight = palette.accent.opacity(0.6);
    div()
        .id(edge_id(column))
        .absolute()
        .top(px(6.0))
        .bottom(px(6.0))
        .left(px(centre - EDGE_GRIP_WIDTH / 2.0))
        .w(px(EDGE_GRIP_WIDTH))
        .flex()
        .justify_center()
        .cursor(CursorStyle::ResizeLeftRight)
        .child(
            div()
                .w(px(1.0))
                .h_full()
                .bg(if is_held { highlight } else { line }),
        )
        .hover(move |style| style.bg(highlight.opacity(0.25)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.start_column_resize(column, event.position.x, cx);
            }),
        )
        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
            if event.click_count() >= 2 {
                this.fit_column(column, window, cx);
            }
        }))
        .test_support()
        .into_any_element()
}

/// An empty cell `width` wide, aligned for `column`.
fn cell(column: ColumnId, width: f32) -> Div {
    let cell = div()
        .flex()
        .flex_shrink_0()
        .w(px(width))
        .min_w_0()
        .overflow_hidden();
    if column.is_right_aligned() {
        cell.justify_end()
    } else {
        cell
    }
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
    let is_selected = app.selected == Some(track_index) || app.marked.contains(&track_index);
    let drag_text: SharedString = if is_selected && app.marked.len() > 1 {
        format!("{} tracks", app.marked.len()).into()
    } else {
        track.title().to_string().into()
    };
    let can_reorder = app.can_reorder_playlist();
    let app_handle = cx.entity().downgrade();
    let playlists: Vec<(PlaylistId, String)> = app
        .session
        .playlists()
        .all()
        .iter()
        .map(|playlist| (playlist.id, playlist.name.clone()))
        .collect();
    let shown_playlist = app.playlist;
    let is_starred = app.session.favorites().is_starred(&track.path);
    let cells: Vec<AnyElement> = app
        .table_layout
        .columns()
        .iter()
        .map(|(column, width)| {
            cell(*column, *width)
                .child(cell_contents(
                    app,
                    *column,
                    row_index,
                    track_index,
                    track,
                    cx,
                ))
                .into_any_element()
        })
        .collect();

    Some(
        div()
            .id(("track", track_index))
            .group(TRACK_ROW_GROUP)
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
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.table_focus.focus(window, cx);
                this.click_track(track_index, event.modifiers(), cx);
            }))
            .on_drag(
                DraggedTracks {
                    from_track: track_index,
                },
                move |_, _, _, cx| {
                    cx.new(|_| DragLabel {
                        text: drag_text.clone(),
                        background: palette.accent,
                        foreground: palette.accent_ink,
                    })
                },
            )
            .when(can_reorder, |row| {
                row.drag_over::<DraggedTracks>(move |style, _, _, _| {
                    style.border_t_2().border_color(palette.accent)
                })
                .on_drop(cx.listener(
                    move |this, dragged: &DraggedTracks, _, cx| {
                        this.drop_tracks_on_row(*dragged, track_index, cx);
                    },
                ))
            })
            .children(cells)
            .test_support()
            .context_menu(move |menu, window, cx| {
                track_menu(
                    menu,
                    &app_handle,
                    &playlists,
                    shown_playlist,
                    is_starred,
                    window,
                    cx,
                )
            })
            .into_any_element(),
    )
}

/// What the cell of `column` shows for `track`.
fn cell_contents(
    app: &EuphoriaApp,
    column: ColumnId,
    row_index: usize,
    track_index: usize,
    track: &Track,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let theme = &app.theme;
    let palette = theme.palette;
    match column {
        ColumnId::Number => div()
            .text_color(palette.text_tertiary)
            .child(format::count(
                app.playlist_positions
                    .get(&track_index)
                    .copied()
                    .unwrap_or(row_index + 1),
            ))
            .into_any_element(),
        ColumnId::Title => {
            let title: SharedString = track.title().to_string().into();
            let tooltip = title.clone();
            let is_starred = app.session.favorites().is_starred(&track.path);
            div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .min_w_0()
                .child(
                    star_button(
                        ("star", track_index),
                        is_starred,
                        if is_starred {
                            "Take the star off (F)"
                        } else {
                            "Star (F)"
                        },
                        &palette,
                    )
                    .when(!is_starred, |star| {
                        star.invisible()
                            .group_hover(TRACK_ROW_GROUP, |style| style.visible())
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_star_on_track(track_index, cx);
                        cx.stop_propagation();
                    }))
                    .test_support(),
                )
                .child(
                    div()
                        .id(("title-cell", track_index))
                        .min_w_0()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .when(track.is_unfinished(), |title| {
                            title.text_color(palette.text_secondary)
                        })
                        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                        .child(title)
                        .test_support(),
                )
                .into_any_element()
        }
        ColumnId::Artist => div()
            .min_w_0()
            .truncate()
            .text_color(palette.text_secondary)
            .children(track.artist.clone())
            .into_any_element(),
        ColumnId::Key => match (&track.status, track.key()) {
            (_, Some(key)) => key_chip(&key, theme, app.display.key_notation).into_any_element(),
            (TrackStatus::Queued, _) => status_text("Queued", palette.text_tertiary, false),
            (TrackStatus::Analyzing, _) => status_text("Analyzing", palette.accent, true),
            (TrackStatus::Failed(_), _) => status_text("Failed", palette.danger, true),
            (TrackStatus::Analyzed(_), None) => missing_value(&palette).into_any_element(),
        },
        ColumnId::MusicalKey => div()
            .text_color(palette.text_secondary)
            .children(track.key().map(|key| key.to_string()))
            .into_any_element(),
        ColumnId::Bpm => {
            optional_text(track, track.bpm().map(format::bpm), &palette).into_any_element()
        }
        ColumnId::Energy => energy(track, &palette).into_any_element(),
        ColumnId::Quality => quality(track, &palette).into_any_element(),
        ColumnId::Time => div()
            .text_color(palette.text_secondary)
            .children(track.duration_seconds().map(format::duration))
            .into_any_element(),
        ColumnId::Format => div()
            .text_color(palette.text_tertiary)
            .child(track.format.clone())
            .into_any_element(),
        ColumnId::Folder => folder_cell(app, track_index, &palette),
    }
}

fn status_text(text: &'static str, colour: gpui::Hsla, is_bold: bool) -> AnyElement {
    div()
        .text_size(SMALL_TEXT_SIZE)
        .when(is_bold, |text| text.font_weight(FontWeight::MEDIUM))
        .text_color(colour)
        .child(text)
        .into_any_element()
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

/// The quality verdict in a word, quieter when there is nothing to check.
fn quality(track: &Track, palette: &Palette) -> Div {
    let Some(report) = track.quality() else {
        return optional_text(track, None, palette);
    };
    div()
        .min_w_0()
        .truncate()
        .text_color(if track.has_quality_to_check() {
            palette.text
        } else {
            palette.text_tertiary
        })
        .child(format::quality_verdict(report.verdict))
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
    use crate::columns::{ColumnWidths, TableLayout};

    /// The table in a 1440 wide window beside the sidebar and an inspector
    /// of the usual width.
    const USUAL_TABLE: f32 = 1440.0 - 220.0 - 320.0;

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
            TableLayout::new(USUAL_TABLE, &ColumnWidths::default(), width)
                .width_of(ColumnId::Folder),
            Some(125.0)
        );
    }

    #[test]
    fn a_folder_width_the_user_dragged_wins_over_the_measured_paths() {
        let mut chosen = ColumnWidths::default();
        chosen.set(ColumnId::Folder, 90.0);
        assert_eq!(
            TableLayout::new(USUAL_TABLE, &chosen, 125.0).width_of(ColumnId::Folder),
            Some(90.0)
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
}
