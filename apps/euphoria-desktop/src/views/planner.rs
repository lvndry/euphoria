// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The planner: a sub-view of the focus view that lays the selected track
//! and the match on one grid of bars, shows where their vocals, bass and
//! drums clash, and ranks the windows where the mix can happen.

use std::sync::Arc;

use euphoria_core::harmony::transition::{
    BarLane, CandidateWindow, Clash, PlanError, Presence, SectionLabel, Side, TrackLanes,
};
use gpui::component::Icon;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, Context, Div, FontWeight, Hsla, MouseButton, MouseDownEvent, Pixels,
    SharedString, TestSupportExt, Window, canvas, div, px, relative,
};

use crate::app::EuphoriaApp;
use crate::focus::{SectionKind, clock};
use crate::format;
use crate::planner_playback::PlanTiming;
use crate::stems::StemStatus;
use crate::theme::Palette;
use crate::transition_plan::{
    BARS_AFTER, BARS_BEFORE, ClashVerdict, PLAY_ACTION, PlanState, PlannedTransition,
    SAVE_FAVORITE_ACTION, SET_CUES_ACTION, UPDATE_FAVORITE_ACTION, clash_verdict, compressed_strip,
    failure_text, ruler_marks, step_line, window_detail, window_title,
};
use crate::views::focus::section_colour;
use crate::views::{
    GAP_LARGE, GAP_MEDIUM, GAP_SMALL, GAP_TIGHT, RADIUS_LARGE, RADIUS_MEDIUM, RADIUS_SMALL,
    TITLE_TEXT_SIZE,
};
use crate::views::{LABEL_TEXT_SIZE, SMALL_TEXT_SIZE, primary_button, secondary_button};
use euphoria_core::harmony::words::{OVERLAP_LABEL, SEPARATE_STEMS_LINK};

type LanePick = fn(&BarLane) -> Presence;

pub(crate) const OVERLAP_HELP: &str =
    "Where both tracks have vocals or basslines at once. Pick a window with none.";
pub(crate) const STEMS_NOT_CHECKED: &str = "Vocals and bass overlap isn't checked yet.";
pub(crate) const NOT_SEPARATED: &str = "Not separated yet";
pub(crate) const BEST_PLACE_TO_MIX: &str = "Best place to mix";
pub(crate) const MIX_STARTS: &str = "Mix starts";
pub(crate) const BARS_HINT: &str = "Bars counted from where B comes in";

const GUTTER_WIDTH: f32 = 96.0;
const SECTION_HEIGHT: f32 = 22.0;
const LANE_HEIGHT: f32 = 10.0;
const CLASH_HEIGHT: f32 = 18.0;
const COMPACT_ROW_HEIGHT: f32 = 12.0;
const STRIP_CELLS: usize = 16;

pub(crate) fn amber() -> Hsla {
    gpui::hsla(0.095, 0.85, 0.58, 1.0)
}

fn success_green() -> Hsla {
    gpui::hsla(0.38, 0.55, 0.5, 1.0)
}

pub(crate) fn tone_colour(tone: euphoria_core::harmony::words::Tone, palette: &Palette) -> Hsla {
    use euphoria_core::harmony::words::Tone;
    match tone {
        Tone::Success => success_green(),
        Tone::Neutral => palette.text_secondary,
        Tone::Warning => amber(),
    }
}

pub(crate) fn clash_colour(clash: Option<Clash>, palette: &Palette) -> Hsla {
    match clash {
        Some(Clash::Vocals) => palette.danger,
        Some(Clash::Bass) => amber(),
        Some(Clash::Clear) => palette.accent.opacity(0.28),
        Some(Clash::Unknown) => palette.text_tertiary.opacity(0.14),
        None => palette.raised.opacity(0.4),
    }
}

pub(crate) fn verdict_colour(verdict: ClashVerdict, palette: &Palette) -> Hsla {
    match verdict {
        ClashVerdict::Clean => palette.accent,
        ClashVerdict::VocalClash => palette.danger,
        ClashVerdict::BassClash => amber(),
        ClashVerdict::NotChecked => palette.text_tertiary,
    }
}

fn kind_of(label: SectionLabel) -> SectionKind {
    match label {
        SectionLabel::Start => SectionKind::Intro,
        SectionLabel::Main => SectionKind::Main,
        SectionLabel::Breakdown => SectionKind::Breakdown,
        SectionLabel::Drop => SectionKind::Drop,
        SectionLabel::Outro => SectionKind::Outro,
        SectionLabel::Section => SectionKind::Section,
    }
}

/// `cells` as runs of equal neighbours with their lengths.
fn runs<T: PartialEq + Copy>(cells: &[T]) -> Vec<(T, usize)> {
    let mut runs: Vec<(T, usize)> = Vec::new();
    for cell in cells {
        match runs.last_mut() {
            Some((value, length)) if value == cell => *length += 1,
            _ => runs.push((*cell, 1)),
        }
    }
    runs
}

/// The bars of a track on the planner's grid: a bar for each column from
/// [`BARS_BEFORE`] before the window to [`BARS_AFTER`] after it, or `None`
/// where the track is not playing.
fn track_columns(
    lanes: &TrackLanes,
    start_bar: usize,
    bars: usize,
    is_a: bool,
) -> Vec<Option<usize>> {
    (0..BARS_BEFORE + bars + BARS_AFTER)
        .map(|column| {
            let offset = column as i64 - BARS_BEFORE as i64;
            let playing = if is_a {
                offset < bars as i64
            } else {
                offset >= 0
            };
            let bar = start_bar as i64 + offset;
            (playing && bar >= 0 && (bar as usize) < lanes.bars.len()).then_some(bar as usize)
        })
        .collect()
}

fn sections_row(
    lanes: &TrackLanes,
    columns: &[Option<usize>],
    palette: &Palette,
    height: f32,
    show_names: bool,
) -> Div {
    let total = columns.len() as f32;
    let cells: Vec<Option<SectionLabel>> = columns
        .iter()
        .map(|bar| bar.and_then(|bar| lanes.bars[bar].section))
        .collect();
    let playing: Vec<bool> = columns.iter().map(Option::is_some).collect();
    let mut pieces: Vec<AnyElement> = Vec::new();
    for (label, length) in runs(
        &cells
            .iter()
            .zip(&playing)
            .map(|(label, playing)| (*label, *playing))
            .collect::<Vec<_>>(),
    ) {
        let (label, is_playing) = label;
        let width = length as f32 / total;
        let piece = div()
            .h(px(height))
            .w(relative(width))
            .flex_shrink_0()
            .overflow_hidden()
            .rounded(RADIUS_SMALL);
        let piece = match (label, is_playing) {
            (Some(label), _) => {
                let (fill, ink) = section_colour(kind_of(label), palette);
                piece
                    .bg(fill)
                    .px(px(6.0))
                    .flex()
                    .items_center()
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(ink)
                    .font_weight(FontWeight::MEDIUM)
                    .when(show_names && length >= 4, |piece| {
                        piece.child(crate::transition_plan::section_name(label))
                    })
            }
            (None, true) => piece.bg(palette.raised),
            (None, false) => piece,
        };
        pieces.push(piece.into_any_element());
    }
    div().flex().gap(GAP_TIGHT).w_full().children(pieces)
}

fn lane_row(
    lanes: &TrackLanes,
    columns: &[Option<usize>],
    pick: LanePick,
    colour: Hsla,
    palette: &Palette,
    height: f32,
) -> Div {
    let total = columns.len() as f32;
    let cells: Vec<Option<Presence>> = columns
        .iter()
        .map(|bar| bar.map(|bar| pick(&lanes.bars[bar])))
        .collect();
    let pieces: Vec<AnyElement> = runs(&cells)
        .into_iter()
        .map(|(presence, length)| {
            let piece = div()
                .h_full()
                .w(relative(length as f32 / total))
                .flex_shrink_0()
                .rounded(RADIUS_SMALL);
            match presence {
                Some(Presence::Present) => piece.bg(colour),
                Some(Presence::Unknown) => piece
                    .bg(palette.text_tertiary.opacity(0.14))
                    .border_1()
                    .border_color(palette.text_tertiary.opacity(0.3)),
                Some(Presence::Absent) | None => piece,
            }
            .into_any_element()
        })
        .collect();
    div()
        .flex()
        .w_full()
        .h(px(height))
        .rounded(RADIUS_SMALL)
        .bg(palette.background.opacity(0.6))
        .children(pieces)
}

const UNKNOWN_LANE_HEIGHT: f32 = 16.0;

fn unknown_lane(palette: &Palette) -> Div {
    div()
        .flex()
        .items_center()
        .w_full()
        .h(px(UNKNOWN_LANE_HEIGHT))
        .px(px(6.0))
        .rounded(RADIUS_SMALL)
        .bg(palette.text_tertiary.opacity(0.1))
        .border_1()
        .border_color(palette.text_tertiary.opacity(0.2))
        .text_size(LABEL_TEXT_SIZE)
        .text_color(palette.text_tertiary)
        .child(NOT_SEPARATED)
}

fn clash_cells(window: &CandidateWindow) -> Vec<Option<Clash>> {
    let mut cells = vec![None; BARS_BEFORE];
    cells.extend(window.clash_strip.iter().copied().map(Some));
    cells.extend(vec![None; BARS_AFTER]);
    cells
}

fn clash_row(window: &CandidateWindow, palette: &Palette, height: f32) -> Div {
    let cells = clash_cells(window);
    let total = cells.len() as f32;
    let pieces: Vec<AnyElement> = cells
        .iter()
        .map(|clash| {
            div()
                .h_full()
                .flex_1()
                .rounded(RADIUS_SMALL)
                .bg(clash_colour(*clash, palette))
                .into_any_element()
        })
        .collect();
    div()
        .relative()
        .w_full()
        .h(px(height))
        .flex()
        .gap(GAP_TIGHT)
        .children(pieces)
        .child(
            div()
                .absolute()
                .top(px(-3.0))
                .bottom(px(-3.0))
                .left(relative(BARS_BEFORE as f32 / total))
                .w(relative(window.bars as f32 / total))
                .border_2()
                .border_color(palette.accent)
                .rounded(RADIUS_MEDIUM),
        )
}

fn legend_item(swatch: Div, text: &'static str, palette: &Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap(GAP_SMALL)
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .child(swatch.size(px(12.0)).rounded(RADIUS_SMALL))
        .child(text)
}

fn legend(palette: &Palette) -> impl IntoElement {
    let solid = |clash: Clash| div().bg(clash_colour(Some(clash), palette));
    div()
        .id("planner-legend")
        .flex()
        .flex_wrap()
        .gap_x(GAP_LARGE)
        .gap_y(GAP_TIGHT)
        .pl(px(GUTTER_WIDTH + 12.0))
        .child(legend_item(solid(Clash::Clear), "Fine", palette))
        .child(legend_item(solid(Clash::Bass), "Both basslines", palette))
        .child(legend_item(solid(Clash::Vocals), "Both vocals", palette))
        .child(legend_item(
            div()
                .bg(palette.text_tertiary.opacity(0.14))
                .border_1()
                .border_color(palette.text_tertiary.opacity(0.3)),
            "Unknown",
            palette,
        ))
        .child(legend_item(
            div().border_2().border_color(palette.accent),
            BEST_PLACE_TO_MIX,
            palette,
        ))
        .test_support()
}

/// "A at 5:09": where in A the playhead is, or playing would start.
pub(crate) fn a_clock_label(
    app: &EuphoriaApp,
    planned: &PlannedTransition,
    window: &CandidateWindow,
) -> String {
    PlanTiming::of(planned, window).map_or(String::new(), |timing| {
        format!(
            "A at {}",
            clock(timing.a_seconds_at(app.plan_playhead_bar()))
        )
    })
}

fn ruler(
    app: &EuphoriaApp,
    planned: &PlannedTransition,
    window: &CandidateWindow,
    palette: &Palette,
) -> impl IntoElement {
    let total = (BARS_BEFORE + window.bars + BARS_AFTER) as f32;
    let drop_bar = planned
        .plan
        .lanes_b
        .bars
        .iter()
        .skip(window.b_start_bar)
        .position(|bar| bar.section == Some(SectionLabel::Drop))
        .filter(|offset| *offset < window.bars + BARS_AFTER);
    let drop_fraction = drop_bar.map(|offset| (BARS_BEFORE + offset) as f32 / total);
    let marks = ruler_marks(window.bars);
    let last = marks.len().saturating_sub(1);
    let mut labels: Vec<AnyElement> = marks
        .into_iter()
        .enumerate()
        .filter(|(position, (label, fraction))| {
            *position == last
                || label == "bar 1"
                || drop_fraction.is_none_or(|drop| (fraction - drop).abs() > 3.0 / total)
        })
        .map(|(position, (label, fraction))| {
            let label = if label == "bar 1" {
                match drop_bar {
                    Some(0) => format!("{MIX_STARTS}, B's drop"),
                    _ => MIX_STARTS.to_string(),
                }
            } else {
                label
            };
            let mark = div().absolute().top_0().child(label);
            if position == last {
                mark.right(relative(1.0 - fraction)).into_any_element()
            } else {
                mark.left(relative(fraction)).into_any_element()
            }
        })
        .collect();
    if let Some((offset, fraction)) = drop_bar.zip(drop_fraction)
        && offset > 0
    {
        labels.push(
            div()
                .absolute()
                .top_0()
                .left(relative(fraction))
                .text_color(palette.text_secondary)
                .child("B's drop")
                .into_any_element(),
        );
    }
    let ends = div()
        .absolute()
        .top_0()
        .left(relative(
            (BARS_BEFORE + window.bars) as f32 / (BARS_BEFORE + window.bars + BARS_AFTER) as f32,
        ))
        .pl(px(6.0))
        .child("A ends");
    let a_label = a_clock_label(app, planned, window);
    div()
        .id("planner-ruler")
        .flex()
        .items_center()
        .gap(GAP_MEDIUM)
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_tertiary)
        .tooltip(|window, cx| Tooltip::new(BARS_HINT).build(window, cx))
        .child(
            div()
                .id("planner-a-clock")
                .w(px(GUTTER_WIDTH))
                .flex_shrink_0()
                .child(a_label)
                .test_support(),
        )
        .child(
            div()
                .relative()
                .flex_1()
                .h(px(16.0))
                .children(labels)
                .child(ends),
        )
        .test_support()
}

fn track_block(
    name: String,
    lanes: &TrackLanes,
    columns: &[Option<usize>],
    palette: &Palette,
    id: &'static str,
    unknown_id: &'static str,
    stems_known: bool,
) -> impl IntoElement {
    let lane_rows = [
        ("Vocals", amber(), (|bar: &BarLane| bar.vocals) as LanePick),
        (
            "Bass",
            palette.accent,
            (|bar: &BarLane| bar.bass) as LanePick,
        ),
        (
            "Drums",
            palette.text_secondary,
            (|bar: &BarLane| bar.drums) as LanePick,
        ),
    ];
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(GAP_TIGHT)
        .py(px(8.0))
        .border_t_1()
        .border_color(palette.line)
        .child(
            div()
                .flex()
                .items_center()
                .gap(GAP_MEDIUM)
                .child(
                    div()
                        .w(px(GUTTER_WIDTH))
                        .flex_shrink_0()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(name),
                )
                .child(div().flex_1().child(sections_row(
                    lanes,
                    columns,
                    palette,
                    SECTION_HEIGHT,
                    true,
                ))),
        )
        .children(
            lane_rows
                .into_iter()
                .enumerate()
                .map(|(position, (label, colour, pick))| {
                    div()
                        .flex()
                        .items_center()
                        .gap(GAP_MEDIUM)
                        .child(
                            div()
                                .w(px(GUTTER_WIDTH))
                                .flex_shrink_0()
                                .text_size(SMALL_TEXT_SIZE)
                                .text_color(palette.text_secondary)
                                .child(label),
                        )
                        .child(
                            div()
                                .id((id, position))
                                .flex_1()
                                .child(if stems_known {
                                    lane_row(lanes, columns, pick, colour, palette, LANE_HEIGHT)
                                        .into_any_element()
                                } else {
                                    unknown_lane(palette)
                                        .id((unknown_id, position))
                                        .test_support()
                                        .into_any_element()
                                })
                                .test_support(),
                        )
                }),
        )
        .test_support()
}

/// A and B as bands of sections with the clash under them, small enough
/// for the transition tab.
pub(crate) fn compact_strip(
    planned: &PlannedTransition,
    window: &CandidateWindow,
    palette: &Palette,
) -> impl IntoElement {
    let columns_a = track_columns(&planned.plan.lanes_a, window.a_start_bar, window.bars, true);
    let columns_b = track_columns(
        &planned.plan.lanes_b,
        window.b_start_bar,
        window.bars,
        false,
    );
    let named = |name: &'static str, row: Div| {
        div()
            .flex()
            .items_center()
            .gap(GAP_SMALL)
            .child(
                div()
                    .w(px(14.0))
                    .flex_shrink_0()
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_tertiary)
                    .child(name),
            )
            .child(div().flex_1().child(row))
    };
    div()
        .id("plan-strip")
        .flex()
        .flex_col()
        .gap(GAP_TIGHT)
        .child(named(
            "A",
            sections_row(
                &planned.plan.lanes_a,
                &columns_a,
                palette,
                COMPACT_ROW_HEIGHT,
                false,
            ),
        ))
        .child(named(
            "B",
            sections_row(
                &planned.plan.lanes_b,
                &columns_b,
                palette,
                COMPACT_ROW_HEIGHT,
                false,
            ),
        ))
        .child(
            div()
                .id("plan-clash")
                .py(px(3.0))
                .child(named("", clash_row(window, palette, COMPACT_ROW_HEIGHT)))
                .test_support(),
        )
        .test_support()
}

/// The strip in the clash column of a row of the mixes list.
pub(crate) fn row_strip(window: &CandidateWindow, palette: &Palette) -> Div {
    let cells = compressed_strip(&window.clash_strip, STRIP_CELLS);
    div()
        .flex()
        .gap(GAP_TIGHT)
        .w(px(64.0))
        .h(px(10.0))
        .flex_shrink_0()
        .children(cells.into_iter().map(|clash| {
            div()
                .h_full()
                .flex_1()
                .rounded(RADIUS_SMALL)
                .bg(clash_colour(Some(clash), palette))
        }))
}

/// The one line that says the overlap is not checked, with a button to
/// separate the stems of each track that has none, and the progress and the
/// way out of a separation that runs.
fn stems_line(
    app: &EuphoriaApp,
    planned: &PlannedTransition,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<AnyElement> {
    let palette = app.theme.palette;
    let (a_known, b_known) = planned.stems_known();
    if a_known && b_known {
        return None;
    }
    let a_path = app.selected_track().map(|track| track.path.clone());
    let b_path = app
        .focus
        .selected_match
        .and_then(|index| app.session.library().track(index))
        .map(|track| track.path.clone());
    let mut line = div()
        .id("planner-stems-line")
        .flex()
        .flex_wrap()
        .items_center()
        .gap(GAP_SMALL)
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .child(STEMS_NOT_CHECKED);
    let sides = [
        (
            a_known,
            Side::A,
            "A",
            "planner-separate-a",
            "planner-cancel-a",
            a_path,
        ),
        (
            b_known,
            Side::B,
            "B",
            "planner-separate-b",
            "planner-cancel-b",
            b_path,
        ),
    ];
    for (known, side, letter, separate_id, cancel_id, path) in sides {
        if known {
            continue;
        }
        let Some(path) = path else {
            continue;
        };
        if let Some(phase) = app.stems.phase_of(&path) {
            let cancelled_path = path.clone();
            line = line.child(
                div()
                    .id(("planner-progress", usize::from(side == Side::B)))
                    .flex()
                    .items_center()
                    .gap(GAP_SMALL)
                    .child(format!("{letter}: {}", phase.description()))
                    .child(
                        secondary_button("Cancel", &palette)
                            .id(cancel_id)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.cancel_stems_of(&cancelled_path, cx)
                            }))
                            .test_support(),
                    )
                    .test_support(),
            );
            continue;
        }
        if let Some(StemStatus::Failed(problem)) = app.stems.status(&path) {
            line = line.child(
                div()
                    .id(("planner-stems-problem", usize::from(side == Side::B)))
                    .text_color(palette.warning)
                    .child(format!("{letter}: {problem}"))
                    .test_support(),
            );
        }
        line = line.child(
            secondary_button(format!("Separate stems of {letter}"), &palette)
                .id(separate_id)
                .on_click(cx.listener(move |this, _, _, cx| this.separate_plan_stems(side, cx)))
                .test_support(),
        );
    }
    Some(line.test_support().into_any_element())
}

fn card(palette: &Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(GAP_MEDIUM)
        .p(px(16.0))
        .rounded(RADIUS_LARGE)
        .border_1()
        .border_color(palette.line)
        .bg(palette.panel)
}

fn heading(text: impl Into<SharedString>) -> Div {
    div()
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(TITLE_TEXT_SIZE)
        .child(text.into())
}

fn window_rows(
    app: &EuphoriaApp,
    planned: &Arc<PlannedTransition>,
    cx: &mut Context<'_, EuphoriaApp>,
) -> impl IntoElement {
    let palette = app.theme.palette;
    let rows = planned
        .plan
        .windows
        .iter()
        .enumerate()
        .map(|(index, window)| {
            let is_chosen = app.planner.window == index;
            let verdict = if planned.stems_known() == (true, true) {
                clash_verdict(window)
            } else {
                ClashVerdict::NotChecked
            };
            div()
                .id(("planner-window", index))
                .flex()
                .items_center()
                .gap(GAP_MEDIUM)
                .px(px(12.0))
                .py(px(10.0))
                .rounded(RADIUS_LARGE)
                .border_1()
                .cursor_pointer()
                .map(|row| {
                    if is_chosen {
                        row.border_color(palette.accent)
                            .bg(palette.accent.opacity(0.1))
                    } else {
                        let hover = palette.raised;
                        row.border_color(palette.line)
                            .hover(move |style| style.bg(hover))
                    }
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(28.0))
                        .flex_shrink_0()
                        .rounded(RADIUS_LARGE)
                        .bg(palette.raised)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("{}", window.rank)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(window_title(planned, window)),
                        )
                        .child(
                            div()
                                .text_color(palette.text_secondary)
                                .child(window_detail(
                                    window,
                                    planned.stems_known() == (true, true),
                                )),
                        ),
                )
                .children((verdict != ClashVerdict::NotChecked).then(|| {
                    div()
                        .id(("planner-window-verdict", index))
                        .flex_shrink_0()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(verdict_colour(verdict, &palette))
                        .child(verdict.label())
                }))
                .on_click(cx.listener(move |this, _, _, cx| this.select_plan_window(index, cx)))
                .test_support()
        });
    card(&palette)
        .id("planner-windows")
        .flex_1()
        .min_w(px(360.0))
        .child(heading("Suggested windows"))
        .children(rows)
        .test_support()
}

fn steps_card(
    app: &EuphoriaApp,
    planned: &Arc<PlannedTransition>,
    window: &CandidateWindow,
    cx: &mut Context<'_, EuphoriaApp>,
) -> impl IntoElement {
    let palette = app.theme.palette;
    let steps = window.steps.iter().enumerate().map(|(position, step)| {
        div()
            .id(("planner-step", position))
            .flex()
            .gap(GAP_SMALL)
            .child(
                div()
                    .w(px(18.0))
                    .flex_shrink_0()
                    .text_color(palette.text_tertiary)
                    .child(format!("{}.", position + 1)),
            )
            .child(
                div()
                    .flex_1()
                    .text_color(palette.text_secondary)
                    .child(step_line(step, planned, window)),
            )
            .test_support()
    });
    let is_favorite = app
        .selected_track()
        .zip(
            app.focus
                .selected_match
                .and_then(|index| app.session.library().track(index)),
        )
        .is_some_and(|(from, to)| {
            app.session
                .favorites()
                .transition(&from.path, &to.path)
                .is_some()
        });
    card(&palette)
        .id("planner-steps")
        .flex_1()
        .min_w(px(320.0))
        .child(heading(format!("Window {}, step by step", window.rank)))
        .child(
            div()
                .id("planner-steps-list")
                .flex()
                .flex_col()
                .gap(GAP_SMALL)
                .children(steps)
                .test_support(),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(GAP_SMALL)
                .mt(px(6.0))
                .child(
                    primary_button(SET_CUES_ACTION, &palette)
                        .id("planner-set-cues")
                        .on_click(cx.listener(|this, _, _, cx| {
                            let index = this.planner.window;
                            this.set_plan_cues(index, cx);
                        }))
                        .test_support(),
                )
                .child(
                    secondary_button(PLAY_ACTION, &palette)
                        .id("planner-play")
                        .on_click(cx.listener(|this, _, _, cx| {
                            let index = this.planner.window;
                            this.preview_on_decks(index, cx);
                        }))
                        .test_support(),
                )
                .child(
                    secondary_button(
                        if is_favorite {
                            UPDATE_FAVORITE_ACTION
                        } else {
                            SAVE_FAVORITE_ACTION
                        },
                        &palette,
                    )
                    .id("planner-save-favorite")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let index = this.planner.window;
                        this.save_plan_as_favorite(index, cx);
                    }))
                    .test_support(),
                ),
        )
        .test_support()
}

fn header_line(app: &EuphoriaApp, planned: Option<&Arc<PlannedTransition>>) -> String {
    let notation = app.tag_settings.key_notation;
    let track_line = |track: &crate::library::Track| {
        let mut parts = vec![track.title().to_string()];
        if let Some(key) = track.key() {
            parts.push(notation.spell(&key));
        }
        if let Some(bpm) = track.bpm() {
            parts.push(format!("{} BPM", format::bpm(bpm)));
        }
        parts.join(" \u{b7} ")
    };
    let Some(a) = app.selected_track() else {
        return String::new();
    };
    let Some(b) = app
        .focus
        .selected_match
        .and_then(|index| app.session.library().track(index))
    else {
        return track_line(a);
    };
    let tempo = planned.map_or(String::new(), |planned| {
        let alignment = &planned.plan.alignment;
        format!(
            " \u{2192} {} BPM ({})",
            format::bpm(alignment.bpm_a as f32),
            format::signed_percent(alignment.tempo_change_percent as f32)
        )
    });
    format!("A {} \u{2192} B {}{tempo}", track_line(a), track_line(b))
}

/// The failure of a plan, in words.
pub(crate) fn failure_message(app: &EuphoriaApp, error: PlanError) -> String {
    let title = |index: Option<usize>| {
        index
            .and_then(|index| app.session.library().track(index))
            .map_or("the track".to_string(), |track| track.title().to_string())
    };
    failure_text(
        error,
        &title(app.selected),
        &title(app.focus.selected_match),
    )
}

pub(crate) fn render(
    app: &EuphoriaApp,
    _window: &Window,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let palette = app.theme.palette;
    let hover_background = palette.line;
    let back = div()
        .id("planner-back")
        .flex()
        .items_center()
        .gap(GAP_SMALL)
        .h(px(28.0))
        .px(px(10.0))
        .rounded(RADIUS_MEDIUM)
        .border_1()
        .border_color(palette.line)
        .bg(palette.raised)
        .cursor_pointer()
        .hover(move |style| style.bg(hover_background))
        .child(
            Icon::default()
                .path("icons/arrow-left.svg")
                .size(px(13.0))
                .text_color(palette.text_secondary),
        )
        .child("Focus view")
        .on_click(cx.listener(|this, _, _, cx| this.close_planner(cx)))
        .test_support();
    let state = app
        .focus
        .selected_match
        .and_then(|index| app.plan_state(index));
    let planned = match state {
        Some(PlanState::Ready(planned)) => Some(planned),
        Some(PlanState::Computing) | None => app
            .selected_track()
            .zip(
                app.focus
                    .selected_match
                    .and_then(|index| app.session.library().track(index)),
            )
            .and_then(|(from, to)| app.planner.last_ready(&from.path, &to.path)),
        Some(PlanState::Failed(_)) => None,
    };
    let top = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(GAP_MEDIUM)
        .child(back)
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(TITLE_TEXT_SIZE)
                .child("Plan a transition"),
        )
        .child(
            div()
                .id("planner-subtitle")
                .text_color(palette.text_secondary)
                .child(header_line(app, planned))
                .test_support(),
        );
    let page = div()
        .id("planner-view")
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .gap(GAP_MEDIUM)
        .p(px(18.0))
        .bg(palette.background)
        .overflow_y_scroll()
        .track_scroll(&app.planner.scroll)
        .child(top);
    let body: AnyElement = match (state, planned) {
        (Some(PlanState::Failed(error)), _) => card(&palette)
            .id("planner-failure")
            .text_color(palette.warning)
            .child(failure_message(app, *error))
            .test_support()
            .into_any_element(),
        (_, Some(planned)) => match planned.window(app.planner.window) {
            Some(window) => planned_body(app, planned, window, cx).into_any_element(),
            None => card(&palette)
                .id("planner-no-window")
                .text_color(palette.text_secondary)
                .child("No window of 16, 32 or 64 bars fits these two tracks.")
                .test_support()
                .into_any_element(),
        },
        _ => card(&palette)
            .id("planner-computing")
            .text_color(palette.text_tertiary)
            .child("Working out the transition\u{2026}")
            .test_support()
            .into_any_element(),
    };
    page.child(body).test_support().into_any_element()
}

fn planned_body(
    app: &EuphoriaApp,
    planned: &Arc<PlannedTransition>,
    window: &CandidateWindow,
    cx: &mut Context<'_, EuphoriaApp>,
) -> impl IntoElement {
    let palette = app.theme.palette;
    let title_of = |index: Option<usize>| {
        index
            .and_then(|index| app.session.library().track(index))
            .map_or(String::new(), |track| track.title().to_string())
    };
    let columns_a = track_columns(&planned.plan.lanes_a, window.a_start_bar, window.bars, true);
    let columns_b = track_columns(
        &planned.plan.lanes_b,
        window.b_start_bar,
        window.bars,
        false,
    );
    let total = (BARS_BEFORE + window.bars + BARS_AFTER) as f32;
    let playhead_fraction =
        ((app.plan_playhead_bar() as f32 + BARS_BEFORE as f32) / total).clamp(0.0, 1.0);
    let timeline_bounds = app.planner.timeline_bounds.clone();
    let seek_bounds = timeline_bounds.clone();
    let overlay = div()
        .id("planner-timeline")
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .left(px(GUTTER_WIDTH + 12.0))
        .cursor_pointer()
        .child(
            canvas(
                move |bounds: Bounds<Pixels>, _, _| {
                    timeline_bounds.replace(Some(bounds));
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .child(
            div()
                .id("planner-playhead")
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative(playhead_fraction))
                .w(px(2.0))
                .bg(palette.text)
                .test_support(),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                if let Some(bounds) = seek_bounds.get() {
                    let fraction = crate::player::fraction_along(
                        event.position.x / px(1.0),
                        bounds.left() / px(1.0),
                        bounds.size.width / px(1.0),
                    );
                    this.seek_plan_timeline(fraction, cx);
                }
            }),
        )
        .test_support();
    let grid = div()
        .id("planner-grid")
        .relative()
        .flex()
        .flex_col()
        .child(ruler(app, planned, window, &palette))
        .child(track_block(
            format!("A \u{b7} {}", title_of(app.selected)),
            &planned.plan.lanes_a,
            &columns_a,
            &palette,
            "planner-track-a",
            "planner-unknown-lane-a",
            planned.plan.lanes_a.stems_known,
        ))
        .child(track_block(
            format!("B \u{b7} {}", title_of(app.focus.selected_match)),
            &planned.plan.lanes_b,
            &columns_b,
            &palette,
            "planner-track-b",
            "planner-unknown-lane-b",
            planned.plan.lanes_b.stems_known,
        ))
        .child(
            div()
                .id("planner-clash")
                .flex()
                .flex_col()
                .gap(GAP_TIGHT)
                .pt(px(10.0))
                .border_t_1()
                .border_color(palette.line)
                .child(
                    div()
                        .id("planner-overlap-help")
                        .pl(px(GUTTER_WIDTH + 12.0))
                        .text_size(SMALL_TEXT_SIZE)
                        .text_color(palette.text_secondary)
                        .child(OVERLAP_HELP)
                        .test_support(),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(GAP_MEDIUM)
                        .child(
                            div()
                                .w(px(GUTTER_WIDTH))
                                .flex_shrink_0()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(OVERLAP_LABEL),
                        )
                        .child(div().flex_1().py(px(3.0)).child(clash_row(
                            window,
                            &palette,
                            CLASH_HEIGHT,
                        ))),
                )
                .test_support(),
        )
        .child(overlay);
    let timelines = card(&palette)
        .id("planner-timelines")
        .child(grid)
        .child(legend(&palette));
    div()
        .flex()
        .flex_col()
        .gap(GAP_MEDIUM)
        .child(transport(app, &palette, cx))
        .child(timelines)
        .children(stems_line(app, planned, cx))
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(GAP_MEDIUM)
                .child(window_rows(app, planned, cx))
                .child(steps_card(app, planned, window, cx)),
        )
}

/// Play the transition, A or B alone, and stop.
fn transport(
    app: &EuphoriaApp,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> impl IntoElement {
    use crate::planner_playback::PlanPlay;
    let playing = app
        .planner
        .playback
        .as_ref()
        .map(|playback| playback.kind());
    let mark = |kind: PlanPlay, button: Div| {
        if playing == Some(kind) {
            button.border_color(palette.accent)
        } else {
            button
        }
    };
    div()
        .id("planner-transport")
        .flex()
        .flex_wrap()
        .items_center()
        .gap(GAP_SMALL)
        .child(
            primary_button("Play the transition", palette)
                .id("planner-play-transition")
                .on_click(
                    cx.listener(|this, _, _, cx| this.play_plan(PlanPlay::Transition, None, cx)),
                )
                .test_support(),
        )
        .child(
            mark(PlanPlay::A, secondary_button("Play A", palette))
                .id("planner-play-a")
                .on_click(cx.listener(|this, _, _, cx| this.play_plan(PlanPlay::A, None, cx)))
                .test_support(),
        )
        .child(
            mark(PlanPlay::B, secondary_button("Play B", palette))
                .id("planner-play-b")
                .on_click(cx.listener(|this, _, _, cx| this.play_plan(PlanPlay::B, None, cx)))
                .test_support(),
        )
        .child(
            secondary_button("Stop", palette)
                .id("planner-stop")
                .on_click(cx.listener(|this, _, _, cx| this.stop_plan_playback(cx)))
                .test_support(),
        )
        .child(
            div()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child("Click the timeline to start from there. Space plays and stops."),
        )
        .test_support()
}

/// The line under the strip: whether vocals and bass overlap, or the link
/// that separates the stems to find out.
fn overlap_line(
    app: &EuphoriaApp,
    verdict: ClashVerdict,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> impl IntoElement {
    let is_separating = [
        app.selected_track(),
        app.focus
            .selected_match
            .and_then(|index| app.session.library().track(index)),
    ]
    .into_iter()
    .flatten()
    .any(|track| matches!(app.stems.status(&track.path), Some(StemStatus::Separating)));
    let answer = if verdict != ClashVerdict::NotChecked {
        div()
            .id("focus-transition-verdict")
            .text_color(verdict_colour(verdict, palette))
            .child(verdict.label())
            .test_support()
    } else if is_separating {
        div()
            .id("focus-transition-verdict")
            .text_color(palette.text_tertiary)
            .child(format!("{SEPARATE_STEMS_LINK} (separating)"))
            .test_support()
    } else {
        let hover_colour = palette.text;
        div()
            .id("focus-transition-verdict")
            .cursor_pointer()
            .text_color(palette.accent)
            .hover(move |style| style.text_color(hover_colour))
            .child(SEPARATE_STEMS_LINK)
            .on_click(cx.listener(|this, _, _, cx| this.separate_missing_plan_stems(cx)))
            .test_support()
    };
    div()
        .id("focus-transition-overlap")
        .flex()
        .flex_wrap()
        .gap(GAP_TIGHT)
        .text_size(LABEL_TEXT_SIZE)
        .font_weight(FontWeight::MEDIUM)
        .child(
            div()
                .text_color(palette.text_secondary)
                .child(format!("{OVERLAP_LABEL}:")),
        )
        .child(answer)
        .test_support()
}

/// The transition tab: the best window in a sentence, the strip, the
/// verdict and the three buttons.
pub(crate) fn transition_tab(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> AnyElement {
    let palette = app.theme.palette;
    let shell = || {
        div()
            .id("focus-transition-card")
            .flex()
            .flex_col()
            .gap(GAP_SMALL)
            .size_full()
            .overflow_y_scroll()
    };
    let Some(index) = app.focus.selected_match else {
        return shell()
            .child(
                div()
                    .id("focus-transition-match")
                    .text_color(palette.text_secondary)
                    .child("Pick a track in Mixes into to plan the transition.")
                    .test_support(),
            )
            .test_support()
            .into_any_element();
    };
    let note = |id: &'static str, text: String, colour: Hsla| {
        shell()
            .child(div().id(id).text_color(colour).child(text).test_support())
            .test_support()
            .into_any_element()
    };
    match app.plan_state(index) {
        None | Some(PlanState::Computing) => note(
            "focus-transition-working",
            "Working out the transition\u{2026}".to_string(),
            palette.text_tertiary,
        ),
        Some(PlanState::Failed(error)) => note(
            "focus-transition-failure",
            failure_message(app, *error),
            palette.warning,
        ),
        Some(PlanState::Ready(planned)) => match planned.window(0) {
            None => note(
                "focus-transition-none",
                "No window of 16, 32 or 64 bars fits these two tracks.".to_string(),
                palette.text_secondary,
            ),
            Some(window) => {
                let verdict = if planned.stems_known() == (true, true) {
                    clash_verdict(window)
                } else {
                    ClashVerdict::NotChecked
                };
                let summary = app.window_summary_text(0).unwrap_or_default();
                let is_favorite = app
                    .selected_track()
                    .zip(
                        app.focus
                            .selected_match
                            .and_then(|index| app.session.library().track(index)),
                    )
                    .is_some_and(|(from, to)| {
                        app.session
                            .favorites()
                            .transition(&from.path, &to.path)
                            .is_some()
                    });
                shell()
                    .child(
                        div()
                            .id("focus-transition-summary")
                            .text_size(SMALL_TEXT_SIZE)
                            .text_color(palette.text_secondary)
                            .child(summary)
                            .test_support(),
                    )
                    .child(overlap_line(app, verdict, &palette, cx))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(GAP_SMALL)
                            .child(
                                primary_button(SET_CUES_ACTION, &palette)
                                    .id("plan-set-cues")
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.set_plan_cues(0, cx)),
                                    )
                                    .test_support(),
                            )
                            .child(
                                secondary_button(PLAY_ACTION, &palette)
                                    .id("plan-preview")
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.preview_on_decks(0, cx)),
                                    )
                                    .test_support(),
                            )
                            .child(
                                secondary_button(
                                    if is_favorite {
                                        UPDATE_FAVORITE_ACTION
                                    } else {
                                        SAVE_FAVORITE_ACTION
                                    },
                                    &palette,
                                )
                                .id("plan-save-favorite")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.save_plan_as_favorite(0, cx)),
                                )
                                .test_support(),
                            ),
                    )
                    .child(compact_strip(planned, window, &palette))
                    .test_support()
                    .into_any_element()
            }
        },
    }
}
