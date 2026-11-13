// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The columns of the track table and how wide each is drawn.
//!
//! Each column has a usual width. Title and Artist share what the others
//! leave; Folder is as wide as its longest path on show. A column the user
//! has dragged keeps the width they gave it, remembered by name in the
//! settings file. When everything does not fit, the least needed columns are
//! left out, in a fixed order, so the title, key, tempo and energy always
//! stay readable. Format and Quality go before a folder path is cut short.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::library::SortColumn;

/// Space between two columns.
pub(crate) const COLUMN_GAP: f32 = 12.0;

/// Space before the first column and after the last.
pub(crate) const SIDE_PADDING: f32 = 14.0;

/// The widest a column can be dragged or fitted.
pub(crate) const WIDEST_COLUMN: f32 = 1200.0;

/// The narrowest the Folder column is drawn before it is left out.
const NARROWEST_FOLDER: f32 = 56.0;

/// A column of the track table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ColumnId {
    /// The row number.
    Number,
    /// The title from the tags, or the file name.
    Title,
    /// The artist from the tags.
    Artist,
    /// The Camelot code.
    Key,
    /// The key by name.
    MusicalKey,
    /// The tempo.
    Bpm,
    /// The energy rating, with a bar.
    Energy,
    /// The sound quality verdict in a word.
    Quality,
    /// The length.
    Time,
    /// The file type.
    Format,
    /// Where the file is, from the folder on show.
    Folder,
}

/// How wide a column is before the user drags it.
enum UsualWidth {
    /// A fixed number of pixels.
    Fixed(f32),
    /// A share of what the other columns leave.
    Share(f32),
    /// As wide as the longest folder path on show.
    FolderPaths,
}

/// The columns left out when the table is too narrow, first to last.
const LEFT_OUT_FIRST: [ColumnId; 7] = [
    ColumnId::Format,
    ColumnId::Quality,
    ColumnId::Folder,
    ColumnId::Artist,
    ColumnId::Time,
    ColumnId::MusicalKey,
    ColumnId::Number,
];

/// The columns left out before a folder path is cut short: where a track is
/// filed is read more often than its file type or quality verdict, which the
/// inspector and the Quality to check filter also show.
const LEFT_OUT_BEFORE_PATHS_ARE_CUT: [ColumnId; 2] = [ColumnId::Format, ColumnId::Quality];

impl ColumnId {
    /// Every column, left to right.
    pub(crate) const ALL: [ColumnId; 11] = [
        ColumnId::Number,
        ColumnId::Title,
        ColumnId::Artist,
        ColumnId::Key,
        ColumnId::MusicalKey,
        ColumnId::Bpm,
        ColumnId::Energy,
        ColumnId::Quality,
        ColumnId::Time,
        ColumnId::Format,
        ColumnId::Folder,
    ];

    /// The name the column's width is remembered under. It never changes.
    pub(crate) fn name(self) -> &'static str {
        match self {
            ColumnId::Number => "number",
            ColumnId::Title => "title",
            ColumnId::Artist => "artist",
            ColumnId::Key => "key",
            ColumnId::MusicalKey => "musical-key",
            ColumnId::Bpm => "bpm",
            ColumnId::Energy => "energy",
            ColumnId::Quality => "quality",
            ColumnId::Time => "time",
            ColumnId::Format => "format",
            ColumnId::Folder => "folder",
        }
    }

    /// The column's header.
    pub(crate) fn label(self) -> &'static str {
        match self {
            ColumnId::Number => "#",
            ColumnId::Title => "Title",
            ColumnId::Artist => "Artist",
            ColumnId::Key => "Key",
            ColumnId::MusicalKey => "Musical key",
            ColumnId::Bpm => "BPM",
            ColumnId::Energy => "Energy",
            ColumnId::Quality => "Quality",
            ColumnId::Time => "Time",
            ColumnId::Format => "Format",
            ColumnId::Folder => "Folder",
        }
    }

    fn usual_width(self) -> UsualWidth {
        match self {
            ColumnId::Number => UsualWidth::Fixed(34.0),
            ColumnId::Title => UsualWidth::Share(2.2),
            ColumnId::Artist => UsualWidth::Share(1.3),
            ColumnId::Key => UsualWidth::Fixed(58.0),
            ColumnId::MusicalKey => UsualWidth::Fixed(84.0),
            ColumnId::Bpm => UsualWidth::Fixed(60.0),
            ColumnId::Energy => UsualWidth::Fixed(86.0),
            ColumnId::Quality => UsualWidth::Fixed(90.0),
            ColumnId::Time => UsualWidth::Fixed(48.0),
            ColumnId::Format => UsualWidth::Fixed(56.0),
            ColumnId::Folder => UsualWidth::FolderPaths,
        }
    }

    /// The narrowest the column is drawn or dragged: its contents stay
    /// readable, a key chip, a tempo and an energy bar whole.
    pub(crate) fn minimum(self) -> f32 {
        match self {
            ColumnId::Number => 28.0,
            ColumnId::Title => 160.0,
            ColumnId::Artist => 90.0,
            ColumnId::Key => 44.0,
            ColumnId::MusicalKey => 64.0,
            ColumnId::Bpm => 44.0,
            ColumnId::Energy => 70.0,
            ColumnId::Quality => 56.0,
            ColumnId::Time => 40.0,
            ColumnId::Format => 44.0,
            ColumnId::Folder => NARROWEST_FOLDER,
        }
    }

    /// True for a column whose contents line up on the right.
    pub(crate) fn is_right_aligned(self) -> bool {
        matches!(self, ColumnId::Number | ColumnId::Bpm | ColumnId::Time)
    }

    /// What clicking the header sorts by.
    pub(crate) fn sorts_by(self) -> Option<SortColumn> {
        match self {
            ColumnId::Number => None,
            ColumnId::Title => Some(SortColumn::Title),
            ColumnId::Artist => Some(SortColumn::Artist),
            ColumnId::Key | ColumnId::MusicalKey => Some(SortColumn::Key),
            ColumnId::Bpm => Some(SortColumn::Bpm),
            ColumnId::Energy => Some(SortColumn::Energy),
            ColumnId::Quality => Some(SortColumn::Quality),
            ColumnId::Time => Some(SortColumn::Duration),
            ColumnId::Format => Some(SortColumn::Format),
            ColumnId::Folder => Some(SortColumn::Folder),
        }
    }

    /// The position of the column among [`ColumnId::ALL`].
    pub(crate) fn position(self) -> usize {
        ColumnId::ALL
            .iter()
            .position(|column| *column == self)
            .unwrap_or_default()
    }
}

/// The widths the user gave columns by dragging or fitting them, by column
/// name. Names this version does not know are kept as they are, so a newer
/// or older version's columns survive a save.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct ColumnWidths(BTreeMap<String, u32>);

impl ColumnWidths {
    /// The width the user gave `column`, kept between its narrowest and
    /// [`WIDEST_COLUMN`].
    pub(crate) fn get(&self, column: ColumnId) -> Option<f32> {
        self.0
            .get(column.name())
            .map(|width| (*width as f32).clamp(column.minimum(), WIDEST_COLUMN))
    }

    /// Gives `column` the width `width`, kept between its narrowest and
    /// [`WIDEST_COLUMN`].
    pub(crate) fn set(&mut self, column: ColumnId, width: f32) {
        let width = width.clamp(column.minimum(), WIDEST_COLUMN).round() as u32;
        self.0.insert(column.name().to_string(), width);
    }

    /// Forgets every width the user gave.
    pub(crate) fn reset(&mut self) {
        self.0.clear();
    }

    /// True when every column has its usual width.
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The width to give `column` so that content `content_width` wide and its
/// header `header_width` wide both show in full, within its limits.
pub(crate) fn fitted_width(column: ColumnId, content_width: f32, header_width: f32) -> f32 {
    content_width
        .max(header_width)
        .ceil()
        .clamp(column.minimum(), WIDEST_COLUMN)
}

/// The widest distinct text, measured on demand when a column is fitted.
/// Character count cannot rule out shorter text in a proportional font.
pub(crate) fn widest_text(
    texts: impl IntoIterator<Item = String>,
    mut measure: impl FnMut(&str) -> f32,
) -> f32 {
    texts
        .into_iter()
        .collect::<HashSet<_>>()
        .iter()
        .map(|text| measure(text))
        .fold(0.0, f32::max)
}

/// Which columns the table shows, left to right, and how wide each is.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct TableLayout {
    columns: Vec<(ColumnId, f32)>,
}

impl TableLayout {
    /// The layout of a table `table_width` wide, with the widths the user
    /// chose and a longest folder path `longest_path` wide.
    ///
    /// Columns the user sized keep their size, and so do the fixed ones.
    /// The Folder column, unless sized, takes what its longest path needs.
    /// Title and Artist, unless sized, share the rest, each at least its
    /// narrowest. When that does not fit, Format and then Quality are left
    /// out first; then an unsized Folder column is narrowed, cutting its
    /// paths short; then
    /// columns are left out in the order of [`LEFT_OUT_FIRST`]. When even
    /// that does not fit, the sized columns go down to their narrowest.
    pub(crate) fn new(table_width: f32, chosen: &ColumnWidths, longest_path: f32) -> Self {
        let mut shown: Vec<ColumnId> = ColumnId::ALL.to_vec();
        let mut left_out_next = LEFT_OUT_FIRST.iter().peekable();
        let mut folder_cap = f32::INFINITY;
        let mut chosen_at_narrowest = false;
        loop {
            let width_of = |column: ColumnId| -> Option<f32> {
                let chosen_width = chosen.get(column).map(|width| {
                    if chosen_at_narrowest {
                        column.minimum()
                    } else {
                        width
                    }
                });
                match (chosen_width, column.usual_width()) {
                    (Some(width), _) => Some(width),
                    (None, UsualWidth::Fixed(width)) => Some(width),
                    (None, UsualWidth::FolderPaths) => Some(
                        longest_path
                            .ceil()
                            .max(NARROWEST_FOLDER)
                            .min(folder_cap.max(NARROWEST_FOLDER)),
                    ),
                    (None, UsualWidth::Share(_)) => None,
                }
            };
            let set_widths: Vec<(ColumnId, Option<f32>)> = shown
                .iter()
                .map(|column| (*column, width_of(*column)))
                .collect();
            let gaps = COLUMN_GAP * shown.len().saturating_sub(1) as f32;
            let used: f32 = 2.0 * SIDE_PADDING
                + gaps
                + set_widths
                    .iter()
                    .filter_map(|(_, width)| *width)
                    .sum::<f32>();
            let sharing_minimum: f32 = set_widths
                .iter()
                .filter(|(_, width)| width.is_none())
                .map(|(column, _)| column.minimum())
                .sum();
            let room = table_width - used;
            if room >= sharing_minimum {
                return Self::sharing(set_widths, room);
            }
            let short_by = sharing_minimum - room;

            if let Some(next) =
                left_out_next.next_if(|column| LEFT_OUT_BEFORE_PATHS_ARE_CUT.contains(column))
            {
                shown.retain(|column| column != next);
                continue;
            }
            let folder_is_narrowable = chosen.get(ColumnId::Folder).is_none()
                && shown.contains(&ColumnId::Folder)
                && width_of(ColumnId::Folder).is_some_and(|width| width > NARROWEST_FOLDER);
            if folder_is_narrowable {
                let folder_width = width_of(ColumnId::Folder).unwrap_or(NARROWEST_FOLDER);
                folder_cap = (folder_width - short_by).max(NARROWEST_FOLDER);
                continue;
            }
            if let Some(next) = left_out_next.next() {
                shown.retain(|column| column != next);
                continue;
            }
            if !chosen_at_narrowest {
                chosen_at_narrowest = true;
                continue;
            }
            return Self::sharing(set_widths, room);
        }
    }

    /// Gives the columns without a set width their narrowest plus a share
    /// of what is left of `room`, or, when `room` is less than their
    /// narrowest, a part of it in proportion.
    fn sharing(set_widths: Vec<(ColumnId, Option<f32>)>, room: f32) -> Self {
        let share_of = |column: ColumnId| match column.usual_width() {
            UsualWidth::Share(share) => share,
            UsualWidth::Fixed(_) | UsualWidth::FolderPaths => 1.0,
        };
        let sharing: Vec<ColumnId> = set_widths
            .iter()
            .filter(|(_, width)| width.is_none())
            .map(|(column, _)| *column)
            .collect();
        let minimum: f32 = sharing.iter().map(|column| column.minimum()).sum();
        let shares: f32 = sharing.iter().map(|column| share_of(*column)).sum();
        let extra = (room - minimum).max(0.0);
        let squeeze = if room < minimum && minimum > 0.0 {
            room.max(0.0) / minimum
        } else {
            1.0
        };
        Self {
            columns: set_widths
                .into_iter()
                .map(|(column, width)| {
                    let width = width.unwrap_or_else(|| {
                        (column.minimum() * squeeze + extra * share_of(column) / shares).floor()
                    });
                    (column, width)
                })
                .collect(),
        }
    }

    /// The columns shown, left to right, with their widths.
    pub(crate) fn columns(&self) -> &[(ColumnId, f32)] {
        &self.columns
    }

    /// True when `column` is shown.
    #[cfg(test)]
    pub(crate) fn shows(&self, column: ColumnId) -> bool {
        self.width_of(column).is_some()
    }

    /// How wide `column` is drawn, when it is shown.
    #[cfg(test)]
    pub(crate) fn width_of(&self, column: ColumnId) -> Option<f32> {
        self.columns
            .iter()
            .find(|(shown, _)| *shown == column)
            .map(|(_, width)| *width)
    }

    /// Where `column` starts, from the left edge of the table.
    pub(crate) fn left_of(&self, column: ColumnId) -> Option<f32> {
        let mut left = SIDE_PADDING;
        for (shown, width) in &self.columns {
            if *shown == column {
                return Some(left);
            }
            left += width + COLUMN_GAP;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table in a 1440 wide window beside the sidebar and an inspector
    /// of the usual width.
    const USUAL_TABLE: f32 = 1440.0 - 220.0 - 320.0;

    #[test]
    fn fitting_keeps_a_shorter_wider_text_beyond_forty_long_names() {
        let mut texts: Vec<String> = (0..41)
            .map(|index| format!("{} {index}", "i".repeat(80)))
            .collect();
        let wide = "W".repeat(30);
        texts.push(wide.clone());
        // The headless text renderer has uniform glyph widths, so supply
        // proportional widths to check that every candidate is measured.
        let content_width = widest_text(texts, |text| if text == wide { 375.0 } else { 260.0 });
        assert_eq!(content_width, 375.0);
        assert_eq!(fitted_width(ColumnId::Title, content_width, 50.0), 375.0);
    }

    #[test]
    fn fitting_measures_repeated_text_once_and_handles_no_text() {
        let mut measured = Vec::new();
        assert_eq!(
            widest_text(["house".to_string(), "house".to_string()], |text| {
                measured.push(text.to_string());
                45.0
            }),
            45.0
        );
        assert_eq!(measured, ["house"]);
        assert_eq!(
            widest_text(Vec::new(), |_| panic!("no text to measure")),
            0.0
        );
    }

    fn shown(layout: &TableLayout) -> Vec<ColumnId> {
        layout.columns().iter().map(|(column, _)| *column).collect()
    }

    fn total_width(layout: &TableLayout) -> f32 {
        let widths: f32 = layout.columns().iter().map(|(_, width)| width).sum();
        widths + 2.0 * SIDE_PADDING + COLUMN_GAP * (layout.columns().len() - 1) as f32
    }

    #[test]
    fn the_usual_table_leaves_out_format_and_quality_so_two_level_paths_show_in_full() {
        let layout = TableLayout::new(USUAL_TABLE, &ColumnWidths::default(), 133.0);
        assert!(!layout.shows(ColumnId::Format));
        assert!(!layout.shows(ColumnId::Quality));
        assert_eq!(layout.width_of(ColumnId::Folder), Some(133.0));
        assert!(layout.width_of(ColumnId::Title).unwrap() >= 160.0);
        assert!(layout.width_of(ColumnId::Artist).unwrap() >= 90.0);
        assert!(total_width(&layout) <= USUAL_TABLE);
    }

    #[test]
    fn a_wide_table_shows_every_column_and_gives_the_rest_to_title_and_artist() {
        let layout = TableLayout::new(1800.0, &ColumnWidths::default(), 133.0);
        assert_eq!(shown(&layout), ColumnId::ALL);
        let title = layout.width_of(ColumnId::Title).unwrap();
        let artist = layout.width_of(ColumnId::Artist).unwrap();
        assert!(title > artist && artist > 90.0, "{title} {artist}");
        assert!((total_width(&layout) - 1800.0).abs() < 2.0);
    }

    #[test]
    fn quality_comes_back_with_the_sidebar_folded_or_the_inspector_closed() {
        for table_width in [1440.0 - 320.0, 1440.0 - 220.0] {
            let layout = TableLayout::new(table_width, &ColumnWidths::default(), 133.0);
            assert_eq!(layout.width_of(ColumnId::Quality), Some(90.0));
            assert_eq!(layout.width_of(ColumnId::Folder), Some(133.0));
            assert!(total_width(&layout) <= table_width);
        }
    }

    #[test]
    fn a_long_path_is_cut_short_before_a_column_is_left_out() {
        let layout = TableLayout::new(USUAL_TABLE, &ColumnWidths::default(), 600.0);
        assert!(!layout.shows(ColumnId::Quality));
        assert!(layout.shows(ColumnId::Folder));
        assert!(layout.shows(ColumnId::Artist));
        assert!(layout.width_of(ColumnId::Folder).unwrap() < 600.0);
        assert_eq!(layout.width_of(ColumnId::Title), Some(160.0));
        assert!(total_width(&layout) <= USUAL_TABLE + 1.0);
    }

    #[test]
    fn a_narrow_table_leaves_columns_out_in_order_and_keeps_the_four_that_matter() {
        let beside_sidebar =
            TableLayout::new(960.0 - 220.0 - 320.0, &ColumnWidths::default(), 133.0);
        assert_eq!(
            shown(&beside_sidebar),
            [
                ColumnId::Title,
                ColumnId::Key,
                ColumnId::Bpm,
                ColumnId::Energy
            ]
        );
        let nothing = TableLayout::new(0.0, &ColumnWidths::default(), 133.0);
        for column in [
            ColumnId::Title,
            ColumnId::Key,
            ColumnId::Bpm,
            ColumnId::Energy,
        ] {
            assert!(nothing.shows(column), "{column:?}");
        }
    }

    #[test]
    fn a_dragged_column_keeps_its_width_and_the_others_share_what_is_left() {
        let mut chosen = ColumnWidths::default();
        chosen.set(ColumnId::Title, 420.0);
        let layout = TableLayout::new(1800.0, &chosen, 133.0);
        assert_eq!(layout.width_of(ColumnId::Title), Some(420.0));
        assert!((total_width(&layout) - 1800.0).abs() < 2.0);

        chosen.set(ColumnId::Folder, 70.0);
        let layout = TableLayout::new(1800.0, &chosen, 600.0);
        assert_eq!(layout.width_of(ColumnId::Folder), Some(70.0));
    }

    #[test]
    fn chosen_widths_that_do_not_fit_leave_columns_out_then_shrink() {
        let mut chosen = ColumnWidths::default();
        chosen.set(ColumnId::Title, 900.0);
        let layout = TableLayout::new(USUAL_TABLE, &chosen, 133.0);
        for column in [
            ColumnId::Title,
            ColumnId::Key,
            ColumnId::Bpm,
            ColumnId::Energy,
        ] {
            assert!(layout.shows(column), "{column:?}");
        }
        assert!(!layout.shows(ColumnId::Format));
        assert!(total_width(&layout) <= USUAL_TABLE + 1.0, "{layout:?}");
    }

    #[test]
    fn widths_are_kept_between_the_narrowest_and_the_widest() {
        let mut chosen = ColumnWidths::default();
        chosen.set(ColumnId::Bpm, 5.0);
        chosen.set(ColumnId::Title, 99_999.0);
        assert_eq!(chosen.get(ColumnId::Bpm), Some(44.0));
        assert_eq!(chosen.get(ColumnId::Title), Some(WIDEST_COLUMN));
        let stored: ColumnWidths = serde_json::from_str(r#"{"key":3}"#).unwrap();
        assert_eq!(stored.get(ColumnId::Key), Some(44.0));
    }

    #[test]
    fn fitting_takes_the_wider_of_content_and_header_within_the_limits() {
        assert_eq!(fitted_width(ColumnId::Title, 412.3, 30.0), 413.0);
        assert_eq!(fitted_width(ColumnId::Title, 40.0, 30.0), 160.0);
        assert_eq!(fitted_width(ColumnId::Time, 20.0, 41.2), 42.0);
        assert_eq!(fitted_width(ColumnId::Title, 5_000.0, 30.0), WIDEST_COLUMN);
    }

    #[test]
    fn widths_are_stored_by_name_unknown_names_survive_and_reset_forgets_them() {
        let mut chosen: ColumnWidths =
            serde_json::from_str(r#"{"rating":120,"title":300}"#).unwrap();
        assert_eq!(chosen.get(ColumnId::Title), Some(300.0));
        chosen.set(ColumnId::Artist, 150.0);
        assert_eq!(
            serde_json::to_string(&chosen).unwrap(),
            r#"{"artist":150,"rating":120,"title":300}"#
        );
        chosen.reset();
        assert!(chosen.is_empty());
        assert_eq!(serde_json::to_string(&chosen).unwrap(), "{}");
    }

    #[test]
    fn columns_start_where_the_ones_before_them_end() {
        let layout = TableLayout::new(1800.0, &ColumnWidths::default(), 133.0);
        assert_eq!(layout.left_of(ColumnId::Number), Some(SIDE_PADDING));
        assert_eq!(
            layout.left_of(ColumnId::Title),
            Some(SIDE_PADDING + 34.0 + COLUMN_GAP)
        );
        let narrow = TableLayout::new(400.0, &ColumnWidths::default(), 133.0);
        assert_eq!(narrow.left_of(ColumnId::Format), None);
    }

    #[test]
    fn every_column_has_its_own_stored_name() {
        let mut names: Vec<&str> = ColumnId::ALL.iter().map(|column| column.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ColumnId::ALL.len());
    }
}
