// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The cues of the selected track: a marker for each above the waveform,
//! lined up with it, and the list where cues are added, renamed, put on a
//! hot cue pad, deleted, undone and reset to the automatic ones.

use euphoria_core::{Cue, HotCue};
use gpui::component::input::Input;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, FontWeight, MouseButton, MouseDownEvent, Pixels, SharedString,
    TestSupportExt, div, px, relative,
};

use crate::app::EuphoriaApp;
use crate::format;
use crate::library::Track;
use crate::theme::Palette;
use crate::views::{LABEL_TEXT_SIZE, SMALL_TEXT_SIZE, section_label};

/// Height of the row of markers above the waveform.
pub(crate) const CUE_LANE_HEIGHT: f32 = 18.0;

/// Past this far along the waveform a marker's label sits left of its
/// line, so it stays inside the waveform's width.
const LABEL_FLIP_FRACTION: f32 = 0.8;

/// Height of a row of the list of cues.
const CUE_ROW_HEIGHT: f32 = 26.0;

/// The id of the marker of cue `index` above the waveform.
pub(crate) fn marker_id(index: usize) -> (&'static str, usize) {
    ("cue-marker", index)
}

/// The cues of `track` as they are drawn: a cue being dragged shows where
/// it would land.
pub(crate) fn shown_cues(track: &Track, app: &EuphoriaApp) -> Vec<Cue> {
    let mut cues = track.cues().cues;
    if let Some(drag) = &app.cue_editor.drag
        && drag.track == track.path
        && let (Some(seconds), Some(cue)) = (drag.seconds, cues.get_mut(drag.index))
    {
        cue.seconds = seconds;
    }
    cues
}

/// How far along the track each of `cues` is, from 0.0 to 1.0.
pub(crate) fn fractions(cues: &[Cue], duration_seconds: f32) -> Vec<f32> {
    if duration_seconds <= 0.0 {
        return Vec::new();
    }
    cues.iter()
        .map(|cue| (cue.seconds as f32 / duration_seconds).clamp(0.0, 1.0))
        .collect()
}

/// Room each character of a marker's label takes, a little more than the
/// average at the label size so that labels are not cut short.
const LABEL_CHARACTER_WIDTH: f32 = 6.5;

/// Room around a marker's label: its padding and its line.
const LABEL_PADDING: f32 = 10.0;

/// The widest a marker's label is drawn.
const WIDEST_LABEL: f32 = 140.0;

/// How wide a marker is with only its letter.
const LETTER_LABEL: f32 = 16.0;

/// How wide a marker is with no label at all.
const TICK_WIDTH: f32 = 4.0;

/// Space kept between two markers on one row.
const MARKER_GAP: f32 = 2.0;

/// How many rows of markers the lane has at most.
const LANE_ROWS: usize = 2;

/// What a marker shows, from most to least room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MarkerLabel {
    /// The hot cue letter and the name, cut short when the next marker on
    /// its row is close.
    Full,
    /// The hot cue letter, or a dot for a memory cue.
    Letter,
    /// Nothing: no row had room even for a letter. The marker can still be
    /// clicked and dragged, and hovering names its cue.
    Tick,
}

/// Where one marker is drawn in the lane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MarkerPlace {
    /// The row, from 0 at the top.
    pub(crate) row: usize,
    /// What it shows.
    pub(crate) label: MarkerLabel,
    /// How wide it is drawn.
    pub(crate) width: f32,
}

/// The text of a marker showing `label`.
fn marker_text(cue: &Cue, label: MarkerLabel) -> String {
    match (label, cue.hot_cue) {
        (MarkerLabel::Tick, _) => String::new(),
        (MarkerLabel::Full, Some(hot_cue)) => {
            format!("{hot_cue} {}", cue.name).trim_end().to_string()
        }
        (MarkerLabel::Full, None) if !cue.name.is_empty() => cue.name.clone(),
        (MarkerLabel::Letter, Some(hot_cue)) => hot_cue.to_string(),
        (MarkerLabel::Full | MarkerLabel::Letter, None) => "\u{2022}".to_string(),
    }
}

/// How wide a marker showing `text` in full is drawn.
fn text_width(text: &str) -> f32 {
    let width = LABEL_PADDING + text.chars().count() as f32 * LABEL_CHARACTER_WIDTH;
    width.clamp(LETTER_LABEL, WIDEST_LABEL)
}

/// Places the markers of `cues`, at `fractions` along a lane `lane_width`
/// wide, so that their labels never overlap.
///
/// Each marker takes the first row where the labels before it, in full,
/// leave room for its own in full, or else the row with room for its
/// letter whose last marker ends earliest. A label then runs as far as its
/// full text needs, up to the next marker on its row: cut short, or down
/// to its letter. A marker with no row free even for a letter, a cue a few
/// pixels from others, is drawn as a bare tick on the last row. A label past
/// [`LABEL_FLIP_FRACTION`] runs left of its line. Without a known width
/// every marker gets its full label on the first row.
pub(crate) fn marker_places(
    cues: &[Cue],
    fractions: &[f32],
    lane_width: Option<f32>,
) -> Vec<MarkerPlace> {
    let full_widths: Vec<f32> = cues
        .iter()
        .map(|cue| text_width(&marker_text(cue, MarkerLabel::Full)))
        .collect();
    let Some(lane_width) = lane_width else {
        return full_widths
            .into_iter()
            .map(|width| MarkerPlace {
                row: 0,
                label: MarkerLabel::Full,
                width,
            })
            .collect();
    };
    let lines: Vec<f32> = fractions
        .iter()
        .map(|fraction| fraction * lane_width)
        .collect();
    let is_flipped = |index: usize| fractions[index] > LABEL_FLIP_FRACTION;
    // Where each marker's letter would start and end.
    let letter_span = |index: usize| {
        if is_flipped(index) {
            (lines[index] - LETTER_LABEL, lines[index])
        } else {
            (lines[index], lines[index] + LETTER_LABEL)
        }
    };

    let full_span = |index: usize| {
        if is_flipped(index) {
            (lines[index] - full_widths[index], lines[index])
        } else {
            (lines[index], lines[index] + full_widths[index])
        }
    };

    // Where the markers placed so far on each row end, with their letters
    // and with their full labels.
    let mut letter_ends = [f32::NEG_INFINITY; LANE_ROWS];
    let mut full_ends = [f32::NEG_INFINITY; LANE_ROWS];
    let mut rows: Vec<Option<usize>> = Vec::with_capacity(cues.len());
    for index in 0..cues.len() {
        let (letter_start, letter_end) = letter_span(index);
        let (full_start, full_end) = full_span(index);
        let row_for_full_label =
            (0..LANE_ROWS).find(|row| full_ends[*row] + MARKER_GAP <= full_start);
        let row = row_for_full_label.or_else(|| {
            (0..LANE_ROWS)
                .filter(|row| letter_ends[*row] + MARKER_GAP <= letter_start)
                .min_by(|first, second| letter_ends[*first].total_cmp(&letter_ends[*second]))
        });
        if let Some(row) = row {
            letter_ends[row] = letter_end;
            full_ends[row] = full_end;
        }
        rows.push(row);
    }

    let mut places: Vec<MarkerPlace> = Vec::with_capacity(cues.len());
    for index in 0..cues.len() {
        let Some(row) = rows[index] else {
            places.push(MarkerPlace {
                row: LANE_ROWS - 1,
                label: MarkerLabel::Tick,
                width: TICK_WIDTH,
            });
            continue;
        };
        let room = if is_flipped(index) {
            let previous_end = (0..index)
                .rev()
                .find(|other| rows[*other] == Some(row))
                .map(|other| {
                    let place = places[other];
                    if is_flipped(other) {
                        lines[other]
                    } else {
                        lines[other] + place.width
                    }
                })
                .unwrap_or(0.0);
            lines[index] - previous_end - MARKER_GAP
        } else {
            let next_start = (index + 1..cues.len())
                .find(|other| rows[*other] == Some(row))
                .map(|other| letter_span(other).0)
                .unwrap_or(lane_width);
            next_start - lines[index] - MARKER_GAP
        };
        let width = full_widths[index].min(room).max(LETTER_LABEL);
        let label = if width >= LETTER_LABEL + 2.0 * LABEL_CHARACTER_WIDTH {
            MarkerLabel::Full
        } else {
            MarkerLabel::Letter
        };
        let width = match label {
            MarkerLabel::Letter => LETTER_LABEL,
            MarkerLabel::Full | MarkerLabel::Tick => width,
        };
        places.push(MarkerPlace { row, label, width });
    }
    places
}

/// How tall the lane is for `places`: one row, or two when a marker needed
/// the second.
pub(crate) fn lane_height(places: &[MarkerPlace]) -> f32 {
    let rows = places.iter().map(|place| place.row + 1).max().unwrap_or(1);
    rows as f32 * CUE_LANE_HEIGHT
}

/// The markers above the waveform, as wide as it, on as many rows as
/// `places` asks: a press on a marker plays from its cue, and a drag moves
/// it. Every marker names its cue on hover, whatever its label shows.
pub(crate) fn lane(
    cues: &[Cue],
    duration_seconds: f32,
    places: &[MarkerPlace],
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let markers = cues
        .iter()
        .zip(fractions(cues, duration_seconds))
        .zip(places)
        .enumerate()
        .map(|(index, ((cue, fraction), place))| {
            let text = marker_text(cue, place.label);
            let explanation: SharedString = format!(
                "{} at {}. Click to play from here, drag to move it: it lands on a beat, or on a bar with Shift held.",
                cue_title(cue),
                format::cue_time(cue.seconds)
            )
            .into();
            let marker = div()
                .id(marker_id(index))
                .absolute()
                .top(px(place.row as f32 * CUE_LANE_HEIGHT))
                .h(px(CUE_LANE_HEIGHT))
                .w(px(place.width))
                .flex()
                .items_center()
                .map(|marker| match place.label {
                    MarkerLabel::Full => marker.px(px(4.0)).bg(palette.raised),
                    MarkerLabel::Letter => marker.justify_center().bg(palette.raised),
                    MarkerLabel::Tick => marker.bg(palette.accent.opacity(0.5)),
                })
                .text_size(LABEL_TEXT_SIZE)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.text)
                .overflow_hidden()
                .cursor_pointer()
                .child(div().min_w_0().truncate().child(text))
                .tooltip(move |window, cx| Tooltip::new(explanation.clone()).build(window, cx))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        this.press_cue(index, event.position.x);
                        cx.stop_propagation();
                    }),
                );
            let placed = if fraction > LABEL_FLIP_FRACTION {
                marker
                    .right(relative(1.0 - fraction))
                    .border_r_2()
                    .border_color(palette.accent)
            } else {
                marker
                    .left(relative(fraction))
                    .border_l_2()
                    .border_color(palette.accent)
            }
            .test_support();
            (place.label == MarkerLabel::Tick, placed)
        });
    // A tick is never placed clear of labels, so it is drawn first and any
    // label it runs into covers it.
    let mut markers: Vec<_> = markers.collect();
    markers.sort_by_key(|(is_tick, _)| !is_tick);
    let markers = markers.into_iter().map(|(_, marker)| marker);
    div()
        .id("cue-lane")
        .relative()
        .w_full()
        .h(px(lane_height(places)))
        .overflow_hidden()
        .children(markers)
        .test_support()
        .into_any_element()
}

/// The name of a cue as the interface says it.
fn cue_title(cue: &Cue) -> String {
    match (cue.name.is_empty(), cue.hot_cue) {
        (false, _) => cue.name.clone(),
        (true, Some(hot_cue)) => format!("Hot cue {hot_cue}"),
        (true, None) => "Memory cue".to_string(),
    }
}

/// The list of the selected track's cues with the commands that change
/// them. `list_height` limits the height of the list, in the strip under
/// the table.
pub(crate) fn section(
    track: &Track,
    app: &EuphoriaApp,
    list_height: Option<Pixels>,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Div {
    let palette = app.theme.palette;
    let track_cues = track.cues();
    let can_undo = app.cue_editor.history.can_undo(&track.path);
    let command = |id: &'static str,
                   label: &'static str,
                   explanation: &'static str,
                   cx: &mut Context<'_, EuphoriaApp>,
                   action: fn(&mut EuphoriaApp, &mut Context<'_, EuphoriaApp>)| {
        let hover_colour = palette.text;
        div()
            .id(id)
            .whitespace_nowrap()
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_secondary)
            .cursor_pointer()
            .hover(move |style| style.text_color(hover_colour))
            .child(label)
            .tooltip(move |window, cx| Tooltip::new(explanation).build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| action(this, cx)))
            .test_support()
    };

    let heading = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(10.0))
        .child(section_label("Cues", &palette))
        .when(track_cues.is_edited(), |heading| {
            heading.child(
                div()
                    .id("cues-set-by-you")
                    .text_size(LABEL_TEXT_SIZE)
                    .text_color(palette.accent)
                    .child("by you")
                    .tooltip(|window, cx| {
                        Tooltip::new(
                            "You edited these cues. They are used everywhere in place of the automatic ones, and kept when the track is analyzed again.",
                        )
                        .build(window, cx)
                    })
                    .test_support(),
            )
        })
        .child(div().flex_1())
        .child(command(
            "add-cue",
            "Add at playhead",
            "Add a cue where playing is, on the nearest beat. Shortcut: C. A double click on the waveform adds one there.",
            cx,
            EuphoriaApp::add_cue_at_playhead,
        ))
        .when(can_undo, |heading| {
            heading.child(command(
                "undo-cue-edit",
                "Undo",
                "Undo the last change to these cues.",
                cx,
                EuphoriaApp::undo_cue_edit,
            ))
        })
        .when(track_cues.is_edited(), |heading| {
            heading.child(command(
                "reset-cues",
                "Reset to automatic cues",
                "Forget your cues and use the ones analysis placed.",
                cx,
                EuphoriaApp::reset_cues,
            ))
        });

    let mut rows = Vec::with_capacity(track_cues.cues.len());
    for (index, cue) in track_cues.cues.iter().enumerate() {
        rows.push(cue_row(index, cue, app, &palette, cx).into_any_element());
        if app.cue_editor.choosing_hot_cue == Some(index) {
            rows.push(hot_cue_choice(index, &track_cues.cues, &palette, cx).into_any_element());
        }
    }
    let note = if track_cues.cues.is_empty() {
        Some("No cues. Double click the waveform, or add one at the playhead.")
    } else if !track_cues.is_edited() {
        Some("Placed by analysis on the bars of the beat grid. Any change makes them yours.")
    } else {
        None
    };
    let list = div()
        .id("cue-list")
        .flex()
        .flex_col()
        .gap(px(2.0))
        .when_some(list_height, |list, height| {
            list.max_h(height).overflow_y_scroll()
        })
        .children(note.map(|note| {
            div()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child(note)
        }))
        .children(rows);

    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(heading)
        .child(list)
}

/// One cue: its hot cue pad, its name, its time and a way to delete it.
fn cue_row(
    index: usize,
    cue: &Cue,
    app: &EuphoriaApp,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> impl IntoElement {
    let hover_background = palette.raised;
    let hover_colour = palette.text;
    let pad = div()
        .id(("cue-hot-cue", index))
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .w(px(22.0))
        .h(px(20.0))
        .rounded(px(4.0))
        .text_size(LABEL_TEXT_SIZE)
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .map(|pad| match cue.hot_cue {
            Some(hot_cue) => pad
                .bg(palette.accent)
                .text_color(palette.accent_ink)
                .child(hot_cue.to_string()),
            None => pad
                .border_1()
                .border_color(palette.line)
                .text_color(palette.text_tertiary)
                .child("\u{2013}"),
        })
        .tooltip(|window, cx| {
            Tooltip::new(
                "The hot cue pad, A to H. Click to choose another, or none for a memory cue only.",
            )
            .build(window, cx)
        })
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_hot_cue_choice(index, cx)))
        .test_support();

    let name = if app.cue_editor.renaming == Some(index) {
        div()
            .flex_1()
            .min_w_0()
            .child(Input::new(&app.cue_name_input).id("cue-name-input"))
            .into_any_element()
    } else {
        div()
            .id(("cue-name", index))
            .flex_1()
            .min_w_0()
            .truncate()
            .cursor_pointer()
            .rounded(px(4.0))
            .px(px(2.0))
            .hover(move |style| style.bg(hover_background))
            .when(cue.name.is_empty(), |name| {
                name.text_color(palette.text_tertiary)
            })
            .child(if cue.name.is_empty() {
                cue_title(cue)
            } else {
                cue.name.clone()
            })
            .tooltip(|window, cx| Tooltip::new("Click to rename").build(window, cx))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.start_renaming_cue(index, window, cx);
            }))
            .test_support()
            .into_any_element()
    };

    div()
        .id(("cue-row", index))
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(8.0))
        .h(px(CUE_ROW_HEIGHT))
        .child(pad)
        .child(name)
        .child(
            div()
                .id(("cue-time", index))
                .flex_shrink_0()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_secondary)
                .cursor_pointer()
                .hover(move |style| style.text_color(hover_colour))
                .child(format::cue_time(cue.seconds))
                .tooltip(|window, cx| Tooltip::new("Play from here").build(window, cx))
                .on_click(cx.listener(move |this, _, _, cx| this.play_from_cue(index, cx)))
                .test_support(),
        )
        .child(
            div()
                .id(("cue-delete", index))
                .flex_shrink_0()
                .px(px(4.0))
                .text_color(palette.text_tertiary)
                .cursor_pointer()
                .hover(move |style| style.text_color(hover_colour))
                .child("\u{00d7}")
                .tooltip(|window, cx| Tooltip::new("Delete this cue").build(window, cx))
                .on_click(cx.listener(move |this, _, _, cx| this.delete_cue(index, cx)))
                .test_support(),
        )
        .test_support()
}

/// The pads to put cue `index` on, A to H, and none. A pad another cue
/// holds is marked; taking it swaps the two cues' pads.
fn hot_cue_choice(
    index: usize,
    cues: &[Cue],
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> impl IntoElement {
    let current = cues.get(index).and_then(|cue| cue.hot_cue);
    let hover_background = palette.line;
    let choice = |id: (&'static str, usize), label: String, is_current: bool, is_taken: bool| {
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .min_w(px(22.0))
            .h(px(20.0))
            .px(px(4.0))
            .rounded(px(4.0))
            .text_size(LABEL_TEXT_SIZE)
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .border_1()
            .border_color(palette.line)
            .hover(move |style| style.bg(hover_background))
            .map(|choice| {
                if is_current {
                    choice.bg(palette.accent).text_color(palette.accent_ink)
                } else if is_taken {
                    choice.text_color(palette.text_tertiary)
                } else {
                    choice.text_color(palette.text)
                }
            })
            .child(label)
    };
    let pads = HotCue::ALL.map(|hot_cue| {
        let is_taken = cues.iter().any(|cue| cue.hot_cue == Some(hot_cue));
        choice(
            ("cue-hot-cue-choice", usize::from(hot_cue.pad())),
            hot_cue.to_string(),
            current == Some(hot_cue),
            is_taken,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.set_cue_hot_cue(index, Some(hot_cue), cx);
        }))
        .test_support()
    });
    div()
        .id("cue-hot-cue-choices")
        .flex()
        .flex_wrap()
        .gap(px(4.0))
        .pl(px(30.0))
        .pb(px(4.0))
        .children(pads)
        .child(
            choice(
                ("cue-hot-cue-none", index),
                "None".to_string(),
                current.is_none(),
                false,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_cue_hot_cue(index, None, cx)))
            .test_support(),
        )
        .test_support()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(seconds: f64, name: &str, letter: Option<char>) -> Cue {
        Cue {
            seconds,
            name: name.to_string(),
            hot_cue: letter.and_then(HotCue::from_letter),
        }
    }

    const LANE: f32 = 300.0;

    /// Eight hot cues named Breakdown, `step` of the lane apart from its
    /// start, and their places.
    fn crowded(step: f32) -> (Vec<f32>, Vec<MarkerPlace>) {
        let cues: Vec<Cue> = HotCue::ALL
            .iter()
            .map(|pad| cue(f64::from(pad.pad()), "Breakdown", Some(pad.letter())))
            .collect();
        let fractions: Vec<f32> = (0..cues.len()).map(|index| index as f32 * step).collect();
        let places = marker_places(&cues, &fractions, Some(LANE));
        (fractions, places)
    }

    /// Fails when two labelled markers overlap or touch.
    fn assert_no_labels_overlap(fractions: &[f32], places: &[MarkerPlace]) {
        let spans: Vec<(usize, f32, f32)> = places
            .iter()
            .zip(fractions)
            .filter(|(place, _)| place.label != MarkerLabel::Tick)
            .map(|(place, fraction)| {
                let line = fraction * LANE;
                if *fraction > LABEL_FLIP_FRACTION {
                    (place.row, line - place.width, line)
                } else {
                    (place.row, line, line + place.width)
                }
            })
            .collect();
        for (index, first) in spans.iter().enumerate() {
            for second in &spans[index + 1..] {
                let overlaps = first.0 == second.0 && first.1 < second.2 && second.1 < first.2;
                assert!(!overlaps, "{first:?} and {second:?} overlap: {places:?}");
            }
        }
    }

    #[test]
    fn markers_with_room_keep_their_full_label_on_one_row() {
        let cues = [cue(0.0, "Start", Some('A')), cue(150.0, "Drop", Some('B'))];
        let places = marker_places(&cues, &[0.0, 0.5], Some(LANE));
        assert_eq!(
            places,
            vec![
                MarkerPlace {
                    row: 0,
                    label: MarkerLabel::Full,
                    width: text_width("A Start"),
                },
                MarkerPlace {
                    row: 0,
                    label: MarkerLabel::Full,
                    width: text_width("B Drop"),
                },
            ]
        );
        assert_eq!(lane_height(&places), CUE_LANE_HEIGHT);
    }

    #[test]
    fn close_markers_alternate_rows_and_cut_their_labels_short() {
        let (fractions, places) = crowded(0.06);
        let rows: Vec<usize> = places.iter().map(|place| place.row).collect();
        assert_eq!(rows, [0, 1, 0, 1, 0, 1, 0, 1]);
        assert!(
            places[..6]
                .iter()
                .all(|place| place.label == MarkerLabel::Full && place.width < 40.0),
            "{places:?}"
        );
        assert_eq!(places[7].width, text_width("H Breakdown"));
        assert_eq!(lane_height(&places), 2.0 * CUE_LANE_HEIGHT);
        assert_no_labels_overlap(&fractions, &places);
    }

    #[test]
    fn closer_markers_shrink_to_their_letter_and_the_closest_to_a_tick() {
        let (fractions, places) = crowded(0.04);
        assert!(
            places[..6]
                .iter()
                .all(|place| place.label == MarkerLabel::Letter),
            "{places:?}"
        );
        assert_no_labels_overlap(&fractions, &places);

        let (fractions, places) = crowded(0.01);
        assert_eq!(places[0].label, MarkerLabel::Letter);
        assert_eq!(places[1].label, MarkerLabel::Letter);
        assert_eq!(places[2].label, MarkerLabel::Tick);
        assert_no_labels_overlap(&fractions, &places);
    }

    #[test]
    fn markers_near_the_end_run_left_and_stay_apart() {
        let cues = [
            cue(0.0, "Breakdown", Some('A')),
            cue(1.0, "Breakdown", Some('B')),
            cue(2.0, "Breakdown", Some('C')),
        ];
        let fractions = [0.9, 0.95, 1.0];
        let places = marker_places(&cues, &fractions, Some(LANE));
        assert_no_labels_overlap(&fractions, &places);
        assert!(places.iter().all(|place| place.label != MarkerLabel::Tick));
    }

    #[test]
    fn a_marker_shows_its_letter_or_a_dot_for_a_memory_cue() {
        assert_eq!(
            marker_text(&cue(1.0, "Drop", Some('C')), MarkerLabel::Letter),
            "C"
        );
        assert_eq!(
            marker_text(&cue(1.0, "Drop", None), MarkerLabel::Letter),
            "\u{2022}"
        );
        assert_eq!(
            marker_text(&cue(1.0, "", None), MarkerLabel::Full),
            "\u{2022}"
        );
        assert_eq!(
            marker_text(&cue(1.0, "", Some('D')), MarkerLabel::Full),
            "D"
        );
        assert_eq!(
            marker_text(&cue(1.0, "Drop", Some('D')), MarkerLabel::Tick),
            ""
        );
    }

    #[test]
    fn without_a_width_every_marker_is_full_on_the_first_row() {
        let cues = [cue(0.0, "Start", Some('A')), cue(0.1, "Main", Some('B'))];
        let places = marker_places(&cues, &[0.0, 0.001], None);
        assert!(
            places
                .iter()
                .all(|place| place.row == 0 && place.label == MarkerLabel::Full)
        );
    }
    #[test]
    fn dragging_a_marker_across_others_keeps_labels_apart_without_sorting_the_preview() {
        let cues: Vec<Cue> = HotCue::ALL
            .iter()
            .map(|pad| cue(f64::from(pad.pad()), "Breakdown", Some(pad.letter())))
            .collect();
        for moved in 0..cues.len() {
            for target in [0.0, 0.01, 0.79, 0.81, 0.95, 1.0] {
                let mut fractions: Vec<f32> =
                    (0..cues.len()).map(|index| index as f32 * 0.12).collect();
                fractions[moved] = target;
                let places = marker_places(&cues, &fractions, Some(LANE));
                assert_no_labels_overlap(&fractions, &places);
            }
        }
    }
}
