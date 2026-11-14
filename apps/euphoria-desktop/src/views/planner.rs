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
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, FontWeight, Hsla, SharedString, TestSupportExt, Window, div, px,
    relative,
};

use crate::app::EuphoriaApp;
use crate::focus::{SectionKind, clock};
use crate::format;
use crate::theme::Palette;
use crate::transition_plan::{
    BARS_AFTER, BARS_BEFORE, ClashVerdict, PlanState, PlannedTransition, clash_verdict,
    compressed_strip, failure_text, ruler_marks, step_line, window_detail, window_title,
};
use crate::views::focus::section_colour;
use crate::views::tag_panel::choice_button;
use crate::views::{LABEL_TEXT_SIZE, SMALL_TEXT_SIZE, primary_button, secondary_button};

type LanePick = fn(&BarLane) -> Presence;

const GUTTER_WIDTH: f32 = 96.0;
const SECTION_HEIGHT: f32 = 22.0;
const LANE_HEIGHT: f32 = 10.0;
const CLASH_HEIGHT: f32 = 18.0;
const COMPACT_ROW_HEIGHT: f32 = 12.0;
const STRIP_CELLS: usize = 16;

fn amber() -> Hsla {
    gpui::hsla(0.095, 0.85, 0.58, 1.0)
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
            .rounded(px(3.0));
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
    div().flex().gap(px(0.0)).w_full().children(pieces)
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
                .rounded(px(2.0));
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
        .rounded(px(2.0))
        .bg(palette.background.opacity(0.6))
        .children(pieces)
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
                .rounded(px(1.0))
                .bg(clash_colour(*clash, palette))
                .into_any_element()
        })
        .collect();
    div()
        .relative()
        .w_full()
        .h(px(height))
        .flex()
        .gap(px(1.0))
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
                .rounded(px(5.0)),
        )
}

fn legend_item(swatch: Div, text: &'static str, palette: &Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .child(swatch.size(px(12.0)).rounded(px(2.0)))
        .child(text)
}

fn legend(palette: &Palette) -> impl IntoElement {
    let solid = |clash: Clash| div().bg(clash_colour(Some(clash), palette));
    div()
        .id("planner-legend")
        .flex()
        .flex_wrap()
        .gap_x(px(16.0))
        .gap_y(px(4.0))
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
            "Suggested window",
            palette,
        ))
        .test_support()
}

fn ruler(
    planned: &PlannedTransition,
    window: &CandidateWindow,
    palette: &Palette,
) -> impl IntoElement {
    let a_start = planned
        .plan
        .lanes_a
        .bars
        .get(window.a_start_bar)
        .map_or(0.0, |bar| bar.start_seconds);
    let before_seconds = (a_start - BARS_BEFORE as f64 * planned.plan.lanes_a.bar_seconds).max(0.0);
    let marks = ruler_marks(window.bars);
    let last = marks.len().saturating_sub(1);
    let labels: Vec<AnyElement> = marks
        .into_iter()
        .enumerate()
        .map(|(position, (label, fraction))| {
            let mark = div().absolute().top_0().child(label);
            if position == last {
                mark.right(relative(1.0 - fraction)).into_any_element()
            } else {
                mark.left(relative(fraction)).into_any_element()
            }
        })
        .collect();
    let ends = div()
        .absolute()
        .top_0()
        .left(relative(
            (BARS_BEFORE + window.bars) as f32 / (BARS_BEFORE + window.bars + BARS_AFTER) as f32,
        ))
        .pl(px(6.0))
        .child("A ends");
    div()
        .id("planner-ruler")
        .flex()
        .items_center()
        .gap(px(12.0))
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_tertiary)
        .child(
            div()
                .w(px(GUTTER_WIDTH))
                .flex_shrink_0()
                .child(format!("A {}", clock(before_seconds))),
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
        .gap(px(4.0))
        .py(px(8.0))
        .border_t_1()
        .border_color(palette.line)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.0))
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
        .children(lane_rows.map(|(label, colour, pick)| {
            div()
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .w(px(GUTTER_WIDTH))
                        .flex_shrink_0()
                        .text_size(SMALL_TEXT_SIZE)
                        .text_color(palette.text_secondary)
                        .child(label),
                )
                .child(div().flex_1().child(lane_row(
                    lanes,
                    columns,
                    pick,
                    colour,
                    palette,
                    LANE_HEIGHT,
                )))
        }))
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
            .gap(px(8.0))
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
        .gap(px(4.0))
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
        .gap(px(1.0))
        .w(px(64.0))
        .h(px(10.0))
        .flex_shrink_0()
        .children(cells.into_iter().map(|clash| {
            div()
                .h_full()
                .flex_1()
                .rounded(px(1.0))
                .bg(clash_colour(Some(clash), palette))
        }))
}

fn stems_line(
    app: &EuphoriaApp,
    planned: &PlannedTransition,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<impl IntoElement> {
    let palette = app.theme.palette;
    let (a_known, b_known) = planned.stems_known();
    if a_known && b_known {
        return None;
    }
    let link =
        |id: &'static str, side: Side, text: &'static str, path: Option<std::path::PathBuf>| {
            let is_separating = path.as_deref().is_some_and(|path| {
                matches!(
                    app.stems.status(path),
                    Some(crate::stems::StemStatus::Separating)
                )
            });
            let hover_colour = palette.text;
            if is_separating {
                div()
                    .id(id)
                    .text_color(palette.text_tertiary)
                    .child(format!("{text} (separating)"))
                    .test_support()
            } else {
                div()
                    .id(id)
                    .cursor_pointer()
                    .text_color(palette.accent)
                    .hover(move |style| style.text_color(hover_colour))
                    .child(text)
                    .on_click(cx.listener(move |this, _, _, cx| this.separate_plan_stems(side, cx)))
                    .test_support()
            }
        };
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
        .gap(px(4.0))
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_tertiary)
        .child("Separate the stems of");
    if !a_known {
        line = line.child(link("planner-separate-a", Side::A, "A", a_path));
    }
    if !a_known && !b_known {
        line = line.child("and");
    }
    if !b_known {
        line = line.child(link("planner-separate-b", Side::B, "B", b_path));
    }
    Some(line.child("to check vocal and bass clashes").test_support())
}

fn card(palette: &Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .p(px(16.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(palette.line)
        .bg(palette.panel)
}

fn heading(text: impl Into<SharedString>) -> Div {
    div()
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(15.0))
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
                .gap(px(12.0))
                .px(px(12.0))
                .py(px(10.0))
                .rounded(px(8.0))
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
                        .rounded(px(14.0))
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
                                .child(window_detail(window)),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(verdict_colour(verdict, &palette))
                        .child(verdict.label()),
                )
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
            .gap(px(8.0))
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
                .gap(px(6.0))
                .children(steps)
                .test_support(),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(8.0))
                .mt(px(6.0))
                .child(
                    primary_button("Set the cues on A and B", &palette)
                        .id("planner-set-cues")
                        .on_click(cx.listener(|this, _, _, cx| {
                            let index = this.planner.window;
                            this.set_plan_cues(index, cx);
                        }))
                        .test_support(),
                )
                .child(
                    secondary_button("Play it on the decks", &palette)
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
                            "Update the favorite transition"
                        } else {
                            "Save as a favorite transition"
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
    let notation = app.display.key_notation;
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
        .gap(px(6.0))
        .h(px(28.0))
        .px(px(10.0))
        .rounded(px(6.0))
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
        _ => None,
    };
    let top = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(14.0))
        .child(back)
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(px(20.0))
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
        .gap(px(14.0))
        .p(px(18.0))
        .bg(palette.background)
        .overflow_y_scroll()
        .child(top);
    let body: AnyElement = match state {
        Some(PlanState::Failed(error)) => card(&palette)
            .id("planner-failure")
            .text_color(palette.danger)
            .child(failure_message(app, *error))
            .test_support()
            .into_any_element(),
        Some(PlanState::Ready(planned)) => match app.planner_window() {
            Some((_, window)) => planned_body(app, planned, window, cx).into_any_element(),
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
    let timelines = card(&palette)
        .id("planner-timelines")
        .child(ruler(planned, window, &palette))
        .child(track_block(
            format!("A \u{b7} {}", title_of(app.selected)),
            &planned.plan.lanes_a,
            &columns_a,
            &palette,
            "planner-track-a",
        ))
        .child(track_block(
            format!("B \u{b7} {}", title_of(app.focus.selected_match)),
            &planned.plan.lanes_b,
            &columns_b,
            &palette,
            "planner-track-b",
        ))
        .child(
            div()
                .id("planner-clash")
                .flex()
                .items_center()
                .gap(px(12.0))
                .pt(px(10.0))
                .border_t_1()
                .border_color(palette.line)
                .child(
                    div()
                        .w(px(GUTTER_WIDTH))
                        .flex_shrink_0()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Clash"),
                )
                .child(
                    div()
                        .flex_1()
                        .py(px(3.0))
                        .child(clash_row(window, &palette, CLASH_HEIGHT)),
                )
                .test_support(),
        )
        .child(legend(&palette))
        .children(stems_line(app, planned, cx));
    div()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .child(timelines)
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(14.0))
                .child(window_rows(app, planned, cx))
                .child(steps_card(app, planned, window, cx)),
        )
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
            .gap(px(8.0))
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
            palette.danger,
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
                shell()
                    .child(
                        div()
                            .id("focus-transition-summary")
                            .text_size(SMALL_TEXT_SIZE)
                            .text_color(palette.text_secondary)
                            .child(summary)
                            .test_support(),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap(px(6.0))
                            .child(
                                choice_button(
                                    "plan-set-cues",
                                    "Set cues on both tracks",
                                    true,
                                    &palette,
                                )
                                .on_click(cx.listener(|this, _, _, cx| this.set_plan_cues(0, cx)))
                                .test_support(),
                            )
                            .child(
                                choice_button(
                                    "plan-preview",
                                    "Preview on the decks",
                                    false,
                                    &palette,
                                )
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.preview_on_decks(0, cx)),
                                )
                                .test_support(),
                            )
                            .child(
                                choice_button("plan-open", "Open the planner", false, &palette)
                                    .on_click(cx.listener(|this, _, _, cx| this.open_planner(cx)))
                                    .test_support(),
                            ),
                    )
                    .child(compact_strip(planned, window, &palette))
                    .child(
                        div()
                            .id("focus-transition-verdict")
                            .text_size(LABEL_TEXT_SIZE)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(verdict_colour(verdict, &palette))
                            .child(verdict.label())
                            .test_support(),
                    )
                    .test_support()
                    .into_any_element()
            }
        },
    }
}
