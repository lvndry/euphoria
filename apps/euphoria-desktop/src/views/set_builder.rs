// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The set builder page: the curve to draw, the pool, the length, and the
//! set built from them with how each track meets the curve.

use euphoria_core::CurvePreset;
use gpui::component::tooltip::Tooltip;
use gpui::component::{Icon, IconName};
use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, Context, Div, FontWeight, Hsla, MouseButton, MouseDownEvent,
    MouseMoveEvent, PathBuilder, Pixels, SharedString, TestSupportExt, canvas, div, fill, point,
    px, relative, size,
};

use crate::app::EuphoriaApp;
use crate::format;
use crate::journey::{
    self, BAR_SHARE, Bar, CHART_PAD, ChipTone, MAX_BAR_WIDTH, Phase, SummaryChip, bar_height,
};
use crate::set_builder::{FEWEST_TRACKS, MOST_TRACKS, PlannedSet, SetPool};
use crate::theme::Palette;
use crate::views::key_chip::key_chip;
use crate::views::player::{GLYPH_SIZE, paint_pause_glyph, paint_play_glyph};
use crate::views::tag_panel::choice_button;
use crate::views::{
    LABEL_TEXT_SIZE, SMALL_TEXT_SIZE, primary_button, secondary_button, section_label,
};

const SET_ROW_GROUP: &str = "set-row";
const CURVE_HEIGHT: f32 = 176.0;
const MIN_GAP_HEIGHT: f32 = 24.0;
const ENERGY_GRID: [f32; 4] = [1.0, 4.0, 7.0, 10.0];

pub(crate) fn render(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> AnyElement {
    let palette = app.theme.palette;
    let builder = &app.set_builder;
    div()
        .id("set-builder")
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .overflow_y_scroll()
        .track_focus(&app.table_focus)
        .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
            if matches!(event.keystroke.key.as_str(), "delete" | "backspace") {
                this.remove_selected_set_row(cx);
                cx.stop_propagation();
            }
        }))
        .bg(palette.background)
        .px(px(24.0))
        .py(px(20.0))
        .gap(px(16.0))
        .child(heading(&palette))
        .child(controls(app, cx))
        .child(pool_choices(app, cx))
        .child(curve_card(app, cx))
        .when_some(builder.problem.clone(), |page, problem| {
            page.child(
                div()
                    .id("set-problem")
                    .text_color(palette.danger)
                    .child(problem)
                    .test_support(),
            )
        })
        .when_some(builder.planned.as_ref(), |page, planned| {
            page.child(summary(app, planned, cx))
                .child(set_rows(app, planned, cx))
        })
        .test_support()
        .into_any_element()
}

fn heading(palette: &Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .text_size(px(18.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Set builder"),
        )
        .child(
            div()
                .text_color(palette.text_secondary)
                .child("Draw the energy you want over the set, choose the tracks to pick from, and Euphoria orders a set that follows the curve with every mix in key and small tempo steps."),
        )
}

fn controls(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let builder = &app.set_builder;
    let any_locked = builder
        .planned
        .as_ref()
        .is_some_and(|planned| planned.locked.iter().any(|locked| *locked));
    let build_label = if builder.is_building {
        "Building…"
    } else if any_locked {
        "Build around locked tracks"
    } else if builder.planned.is_some() {
        "Build again"
    } else {
        "Build set"
    };
    let mut presets = div().flex().items_center().gap(px(6.0));
    for preset in CurvePreset::ALL {
        presets = presets.child(
            choice_button(
                ("set-preset", preset as usize),
                preset.label(),
                builder.preset == Some(preset),
                &palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.choose_set_preset(preset, cx)))
            .test_support(),
        );
    }
    if builder.preset.is_none() {
        presets = presets.child(
            div()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child("Drawn by hand"),
        );
    }
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(16.0))
        .child(labelled("Curve", presets, &palette))
        .child(labelled(
            "Length",
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(
                    secondary_button("−", &palette)
                        .id("set-shorter")
                        .when(builder.track_count <= FEWEST_TRACKS, |button| {
                            button.opacity(0.4)
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.change_set_length(-1, cx)))
                        .test_support(),
                )
                .child(
                    div()
                        .id("set-length")
                        .min_w(px(72.0))
                        .flex()
                        .justify_center()
                        .child(format!("{} tracks", builder.track_count))
                        .test_support(),
                )
                .child(
                    secondary_button("+", &palette)
                        .id("set-longer")
                        .when(builder.track_count >= MOST_TRACKS, |button| {
                            button.opacity(0.4)
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.change_set_length(1, cx)))
                        .test_support(),
                ),
            &palette,
        ))
        .child(
            primary_button(build_label, &palette)
                .id("build-set")
                .when(builder.is_building, |button| button.opacity(0.6))
                .on_click(cx.listener(|this, _, _, cx| {
                    if !this.set_builder.is_building {
                        this.build_set(cx);
                    }
                }))
                .test_support(),
        )
}

fn labelled(label: &'static str, control: Div, palette: &Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(section_label(label, palette))
        .child(control)
}

fn pool_choices(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let current = &app.set_builder.pool;
    let mut choices: Vec<(SetPool, String)> = vec![(
        SetPool::AllTracks,
        format!("All tracks ({})", app.session.library().tracks().len()),
    )];
    for folder in app.session.library().folders() {
        let name = folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| folder.display().to_string());
        choices.push((SetPool::Folder(folder.clone()), name));
    }
    if let SetPool::Folder(folder) = current
        && !choices.iter().any(|(pool, _)| pool == current)
    {
        let name = folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        choices.push((current.clone(), name));
    }
    for playlist in app.session.playlists().all() {
        choices.push((
            SetPool::Playlist(playlist.id),
            format!("{} ({})", playlist.name, playlist.tracks.len()),
        ));
    }
    let mut row = div().flex().flex_wrap().items_center().gap(px(6.0));
    for (index, (pool, label)) in choices.into_iter().enumerate() {
        let is_chosen = &pool == current;
        row = row.child(
            choice_button(("set-pool", index), label, is_chosen, &palette)
                .on_click(cx.listener(move |this, _, _, cx| this.choose_set_pool(pool.clone(), cx)))
                .test_support(),
        );
    }
    labelled(crate::harmony_words::PICK_FROM_LABEL, row, &palette)
}

/// The curve to draw on, and once a set is built, a bar for each track of
/// it standing at its energy.
fn curve_card(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let builder = &app.set_builder;
    let curve = builder.curve.clone();
    let bars = app.journey_bars();
    let count = (!bars.is_empty()).then_some(bars.len());
    let seen_bounds = builder.curve_bounds.clone();
    let colours = CurveColours {
        grid: palette.line,
        curve: palette.accent,
    };
    let mut drawing = div()
        .id("set-curve")
        .relative()
        .h(px(CURVE_HEIGHT))
        .w_full()
        .cursor_crosshair()
        .child(
            canvas(
                move |bounds, _, _| seen_bounds.set(Some(bounds)),
                move |bounds, (), window, _| {
                    paint_curve(window, bounds, &curve, count, colours);
                },
            )
            .size_full(),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                this.draw_curve_to(event.position, true, cx);
                cx.stop_propagation();
            }),
        )
        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                if this.set_builder.stroke.is_some() {
                    this.draw_curve_to(event.position, false, cx);
                }
            } else {
                this.finish_curve_stroke(cx);
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.finish_curve_stroke(cx)),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.finish_curve_stroke(cx)),
        );
    if !bars.is_empty() {
        drawing = drawing.child(journey_bars(&bars, &palette, cx));
    }
    let drawing = drawing.test_support();

    let legend = div()
        .flex()
        .items_center()
        .gap(px(14.0))
        .text_size(LABEL_TEXT_SIZE)
        .text_color(palette.text_tertiary)
        .child("Drag across the chart to draw the curve. Higher is more energy, from 1 to 10.")
        .child(div().flex_1())
        .child(legend_swatch("Curve", palette.accent))
        .when(builder.planned.is_some(), |legend| {
            legend
                .child(legend_swatch("Track", palette.waveform))
                .child(legend_swatch("Off the curve", palette.warning))
        });

    let mut axis = div()
        .flex()
        .flex_col()
        .justify_between()
        .h(px(CURVE_HEIGHT + 12.0))
        .mt(px(-6.0))
        .w(px(16.0))
        .text_size(LABEL_TEXT_SIZE)
        .text_color(palette.text_tertiary);
    for energy in ENERGY_GRID.iter().rev() {
        axis = axis.child(
            div()
                .h(px(12.0))
                .flex()
                .items_center()
                .child(format!("{energy:.0}")),
        );
    }
    let mut chart = div().flex_1().min_w_0().flex().flex_col().child(drawing);
    if let Some(planned) = builder.planned.as_ref() {
        let curve_phases = journey::phases(&builder.curve);
        if !curve_phases.is_empty() {
            chart = chart.child(phase_row(&curve_phases, planned.paths.len(), &palette));
        }
        if let Some(axis) = time_axis(app, planned, &palette) {
            chart = chart.child(axis);
        }
    }
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .p(px(12.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(palette.line)
        .bg(palette.panel)
        .child(div().flex().gap(px(8.0)).child(axis).child(chart))
        .child(legend)
}

/// The bars over the chart, a column for each track so that the gaps between
/// them can be pointed at.
fn journey_bars(bars: &[Bar], palette: &Palette, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let mut row = div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(px(CHART_PAD))
        .right(px(CHART_PAD))
        .flex();
    let mut previous_height = 0.0;
    for bar in bars {
        let height = bar_height(bar.energy, CURVE_HEIGHT);
        row = row.child(bar_column(bar, height, previous_height, palette, cx));
        previous_height = height;
    }
    row
}

fn bar_column(
    bar: &Bar,
    height: f32,
    previous_height: f32,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Div {
    let position = bar.position;
    let colour = if bar.is_playing {
        palette.accent
    } else if bar.off_curve {
        palette.warning
    } else {
        palette.waveform
    };
    let tooltip: SharedString = bar.tooltip.clone().into();
    let selection_colour = palette.text;
    let mut column = div()
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .h_full()
        .justify_end()
        .items_center();
    if bar.is_locked {
        column = column.child(
            div()
                .mb(px(2.0))
                .text_color(if bar.is_playing {
                    palette.accent
                } else {
                    palette.text_secondary
                })
                .child(
                    Icon::default()
                        .path(crate::assets::LOCK_ICON_PATH)
                        .size(px(10.0)),
                )
                .id(("journey-lock", position))
                .test_support(),
        );
    }
    column = column.child(
        div()
            .id(("journey-bar", position))
            .w(relative(BAR_SHARE))
            .max_w(px(MAX_BAR_WIDTH))
            .min_w(px(3.0))
            .h(px(height))
            .rounded_t(px(2.0))
            .bg(colour)
            .cursor_pointer()
            .when(bar.is_selected, move |element| {
                element.border_1().border_color(selection_colour)
            })
            .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    this.table_focus.focus(window, cx);
                    this.select_set_row(position, cx);
                    if event.click_count() == 2 {
                        this.play_set_track(position, 0.0, cx);
                    }
                }),
            )
            .test_support(),
    );
    if position > 0 {
        column = column.child(
            div()
                .id(("journey-gap", position))
                .absolute()
                .left(relative(-0.5 * (1.0 - BAR_SHARE)))
                .bottom_0()
                .w(relative(1.0 - BAR_SHARE))
                .h(px(height.max(previous_height).max(MIN_GAP_HEIGHT)))
                .cursor_pointer()
                .tooltip(|window, cx| {
                    Tooltip::new("Play the transition: the end of the track before, then this one.")
                        .build(window, cx)
                })
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select_set_row(position, cx);
                    this.play_set_transition(position, cx);
                }))
                .test_support(),
        );
    }
    column
}

/// The names of the stretches of the curve, under the chart.
fn phase_row(phases: &[Phase], count: usize, palette: &Palette) -> Div {
    let mut row = div()
        .relative()
        .h(px(18.0))
        .mx(px(CHART_PAD))
        .text_size(LABEL_TEXT_SIZE)
        .text_color(palette.text_tertiary);
    for phase in phases {
        let left = if phase.from <= 0.0 {
            0.0
        } else {
            journey::share_at_curve(phase.from, count)
        };
        let right = if phase.to >= 1.0 {
            1.0
        } else {
            journey::share_at_curve(phase.to, count)
        };
        row = row.child(
            div()
                .absolute()
                .top_0()
                .left(relative(left))
                .w(relative(right - left))
                .px(px(4.0))
                .pt(px(2.0))
                .border_t_1()
                .border_color(palette.line)
                .truncate()
                .child(phase.label),
        );
    }
    row
}

/// How far into the set each quarter of it starts, under the chart.
fn time_axis(app: &EuphoriaApp, planned: &PlannedSet, palette: &Palette) -> Option<Div> {
    let seconds = app.set_track_seconds(planned);
    let total: f32 = seconds.iter().sum();
    let count = seconds.len();
    if total <= 0.0 || count < 2 {
        return None;
    }
    let mut elapsed = vec![0.0];
    for track_seconds in &seconds {
        elapsed.push(elapsed.last().copied().unwrap_or(0.0) + track_seconds);
    }
    let mut boundaries: Vec<usize> = (0..4)
        .map(|quarter| (count as f32 * quarter as f32 / 4.0).round() as usize)
        .collect();
    boundaries.dedup();
    let mut axis = div()
        .relative()
        .h(px(16.0))
        .mx(px(CHART_PAD))
        .text_size(LABEL_TEXT_SIZE)
        .text_color(palette.text_tertiary);
    for boundary in boundaries {
        axis = axis.child(
            div()
                .absolute()
                .top_0()
                .left(relative(boundary as f32 / count as f32))
                .child(format::long_duration(elapsed[boundary])),
        );
    }
    Some(
        axis.child(
            div()
                .absolute()
                .top_0()
                .right_0()
                .child(format::long_duration(total)),
        ),
    )
}

fn legend_swatch(label: &'static str, colour: Hsla) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(5.0))
        .child(div().w(px(12.0)).h(px(3.0)).rounded(px(2.0)).bg(colour))
        .child(label)
}

#[derive(Clone, Copy)]
struct CurveColours {
    grid: Hsla,
    curve: Hsla,
}

fn paint_curve(
    window: &mut gpui::Window,
    bounds: Bounds<Pixels>,
    curve: &[f32],
    count: Option<usize>,
    colours: CurveColours,
) {
    let (start, span) = journey::curve_span(f32::from(bounds.size.width), count);
    let to_point = |along: f32, energy: f32| {
        point(
            bounds.origin.x + px(start + span * along.clamp(0.0, 1.0)),
            bounds.origin.y + bounds.size.height * (1.0 - journey::level_fraction(energy)),
        )
    };
    for energy in ENERGY_GRID {
        let at = to_point(0.0, energy);
        window.paint_quad(fill(
            Bounds::new(
                point(bounds.origin.x, at.y),
                size(bounds.size.width, px(1.0)),
            ),
            colours.grid,
        ));
    }
    if curve.len() >= 2 {
        let last = (curve.len() - 1) as f32;
        let mut area = PathBuilder::fill();
        area.move_to(point(
            to_point(0.0, 1.0).x,
            bounds.origin.y + bounds.size.height,
        ));
        for (index, level) in curve.iter().enumerate() {
            area.line_to(to_point(index as f32 / last, *level));
        }
        area.line_to(point(
            to_point(1.0, 1.0).x,
            bounds.origin.y + bounds.size.height,
        ));
        area.close();
        if let Ok(path) = area.build() {
            window.paint_path(path, colours.curve.opacity(0.08));
        }
        let mut line = PathBuilder::stroke(px(2.0));
        for (index, level) in curve.iter().enumerate() {
            let at = to_point(index as f32 / last, *level);
            if index == 0 {
                line.move_to(at);
            } else {
                line.line_to(at);
            }
        }
        if let Ok(path) = line.build() {
            window.paint_path(path, colours.curve);
        }
    }
}

fn chip(summary_chip: &SummaryChip, index: usize, palette: &Palette) -> AnyElement {
    let (background, text) = match summary_chip.tone {
        ChipTone::Neutral => (palette.raised, palette.text_secondary),
        ChipTone::Warm => (palette.warning.opacity(0.16), palette.warning),
    };
    div()
        .id(("set-chip", index))
        .px(px(10.0))
        .py(px(3.0))
        .rounded(px(12.0))
        .bg(background)
        .text_color(text)
        .text_size(SMALL_TEXT_SIZE)
        .child(summary_chip.text.clone())
        .test_support()
        .into_any_element()
}

/// The set in a row of chips, the buttons that keep it, and under them the
/// list of where it falls short when that is open.
fn summary(
    app: &EuphoriaApp,
    planned: &PlannedSet,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let palette = app.theme.palette;
    let seconds: f32 = app.set_track_seconds(planned).iter().sum();
    let plan = &planned.plan;
    let is_open = app.set_builder.details_open;
    let mut chips = div().flex().flex_wrap().items_center().gap(px(6.0));
    for (index, summary_chip) in journey::summary_chips(plan, seconds).iter().enumerate() {
        chips = chips.child(chip(summary_chip, index, &palette));
    }
    let toggle = div()
        .id("set-details-toggle")
        .flex()
        .items_center()
        .gap(px(4.0))
        .px(px(6.0))
        .py(px(2.0))
        .rounded(px(4.0))
        .cursor_pointer()
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .hover({
            let raised = palette.raised;
            move |style| style.bg(raised)
        })
        .child(
            Icon::new(if is_open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .size(px(12.0)),
        )
        .child(if is_open {
            "Hide details"
        } else {
            "Show details"
        })
        .on_click(cx.listener(|this, _, _, cx| this.toggle_set_details(cx)))
        .test_support();
    let mut summary = div()
        .id("set-summary")
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(10.0))
                .child(chips)
                .child(toggle)
                .child(div().flex_1())
                .child(set_actions(app, cx)),
        );
    if is_open {
        let mut details = div()
            .id("set-details")
            .flex()
            .flex_col()
            .gap(px(2.0))
            .p(px(10.0))
            .rounded(px(8.0))
            .bg(palette.panel)
            .border_1()
            .border_color(palette.line)
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_secondary);
        for (index, line) in journey::detail_lines(plan).into_iter().enumerate() {
            details = details.child(
                div()
                    .id(("set-detail", index))
                    .child(line.message)
                    .when(!line.reason.is_empty(), |element| {
                        element.child(
                            div()
                                .text_color(palette.text_tertiary)
                                .child(format!(" {}", line.reason)),
                        )
                    })
                    .flex()
                    .flex_wrap()
                    .test_support(),
            );
        }
        summary = summary.child(details.test_support());
    }
    summary.test_support().into_any_element()
}

fn set_rows(app: &EuphoriaApp, planned: &PlannedSet, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let library = app.session.library();
    let plan = &planned.plan;
    let count = planned.paths.len();
    let raised = palette.raised;
    let line = palette.line;
    let mut rows = div()
        .flex()
        .flex_col()
        .rounded(px(10.0))
        .border_1()
        .border_color(palette.line);
    for (position, path) in planned.paths.iter().enumerate() {
        let track = library
            .index_of(path)
            .and_then(|index| library.track(index));
        let planned_track = plan.tracks.get(position);
        let transition = position
            .checked_sub(1)
            .and_then(|index| plan.transitions.get(index));
        let is_locked = planned.locked.get(position).copied().unwrap_or(false);
        let off_target = journey::is_off_target(plan, position);
        let rule_breaks = journey::rule_breaks_into(plan, position);
        let is_flagged = journey::is_flagged(plan, position);
        let rule_tooltip: Option<SharedString> =
            (!rule_breaks.is_empty()).then(|| rule_breaks.join("\n").into());
        let title: SharedString = track
            .map(|track| track.title().to_string())
            .unwrap_or_else(|| path.display().to_string())
            .into();
        let artist: SharedString = track
            .and_then(|track| track.artist.clone())
            .unwrap_or_default()
            .into();
        let with_rule_tooltip = |cell: Div, id: &'static str| -> AnyElement {
            let cell = cell.id((id, position));
            match rule_tooltip.clone() {
                Some(text) => cell
                    .tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx))
                    .test_support()
                    .into_any_element(),
                None => cell.test_support().into_any_element(),
            }
        };
        let step = transition.map(|transition| {
            let colour = if transition.tempo_allowed {
                palette.text_secondary
            } else {
                palette.warning
            };
            with_rule_tooltip(
                div()
                    .text_color(colour)
                    .child(format!("{:+.1}", transition.bpm_change)),
                "set-row-tempo",
            )
        });
        let movement = transition.map(|transition| {
            let colour = if transition.key_allowed {
                palette.text_secondary
            } else {
                palette.warning
            };
            with_rule_tooltip(
                div().text_color(colour).child(
                    transition
                        .relation
                        .map_or("Key clash", |relation| relation.label()),
                ),
                "set-row-key",
            )
        });
        let energy = planned_track.map(|planned_track| {
            div()
                .id(("set-energy", position))
                .flex()
                .px(px(6.0))
                .rounded(px(4.0))
                .when(off_target, |cell| {
                    cell.bg(palette.warning.opacity(0.16))
                        .text_color(palette.warning)
                })
                .when(!off_target, |cell| cell.text_color(palette.text_tertiary))
                .font_weight(FontWeight::MEDIUM)
                .child(journey::energy_text(
                    planned_track.energy,
                    planned_track.target_energy,
                    off_target,
                ))
                .test_support()
        });
        let is_sounding = app.is_set_row_sounding(position);
        let is_current = app.set_playing_position() == Some(position);
        let is_selected = app.set_builder.selected_row == Some(position);
        let neighbour_locked = |other: Option<usize>| {
            other
                .and_then(|other| planned.locked.get(other))
                .copied()
                .unwrap_or(false)
        };
        let can_move_earlier =
            position > 0 && !is_locked && !neighbour_locked(position.checked_sub(1));
        let can_move_later =
            position + 1 < count && !is_locked && !neighbour_locked(Some(position + 1));
        let play_colour = if is_current {
            palette.accent
        } else {
            palette.text_secondary
        };
        let row = div()
            .id(("set-row", position))
            .group(SET_ROW_GROUP)
            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                this.table_focus.focus(window, cx);
                this.select_set_row(position, cx);
                if event.click_count() == 2 {
                    this.play_set_track(position, 0.0, cx);
                }
            }))
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(12.0))
            .py(px(6.0))
            .text_size(SMALL_TEXT_SIZE)
            .when(position > 0, |row| {
                row.border_t_1().border_color(palette.line)
            })
            .when(is_selected, |row| row.bg(palette.raised))
            .when(is_locked, |row| row.bg(palette.selection.opacity(0.5)))
            .relative()
            .when(is_flagged, |row| {
                row.child(
                    div()
                        .id(("set-row-flag", position))
                        .absolute()
                        .left_0()
                        .top_0()
                        .bottom_0()
                        .w(px(3.0))
                        .bg(palette.warning)
                        .test_support(),
                )
            })
            .child(
                div()
                    .w(px(22.0))
                    .text_color(if is_current {
                        palette.accent
                    } else {
                        palette.text_tertiary
                    })
                    .child((position + 1).to_string()),
            )
            .child(
                div()
                    .id(("set-play", position))
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .size(px(crate::views::row_actions::ICON_SIZE))
                    .rounded(px(5.0))
                    .bg(if is_selected { line } else { raised })
                    .cursor_pointer()
                    .hover(move |style| style.bg(line))
                    .when(!is_current, |button| {
                        button
                            .invisible()
                            .group_hover(SET_ROW_GROUP, |style| style.visible())
                    })
                    .tooltip(move |window, cx| {
                        Tooltip::new(if is_sounding { "Pause" } else { "Play from the start" })
                            .build(window, cx)
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_set_row(position, cx);
                        cx.stop_propagation();
                    }))
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds: Bounds<Pixels>, _, window, _| {
                                if is_sounding {
                                    paint_pause_glyph(bounds, play_colour, window);
                                } else {
                                    paint_play_glyph(bounds, play_colour, window);
                                }
                            },
                        )
                        .size(px(GLYPH_SIZE)),
                    )
                    .test_support(),
            )
            .child(
                choice_button(
                    ("set-lock", position),
                    if is_locked { "Locked" } else { "Lock" },
                    is_locked,
                    &palette,
                )
                .tooltip(|window, cx| {
                    Tooltip::new("Keep this track here when the set is built again.")
                        .build(window, cx)
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.toggle_set_lock(position, cx);
                    cx.stop_propagation();
                }))
                .test_support(),
            )
            .child(
                div()
                    .w(px(40.0))
                    .children(planned_track.map(|planned_track| {
                        key_chip(&planned_track.key, &app.theme, app.display.key_notation)
                    })),
            )
            .child(
                div().w(px(48.0)).child(
                    planned_track
                        .map(|planned_track| format!("{:.1}", planned_track.bpm))
                        .unwrap_or_default(),
                ),
            )
            .child(div().w(px(44.0)).children(step))
            .child(div().w(px(96.0)).children(movement))
            .child(div().w(px(120.0)).children(energy))
            .child(div().w(px(104.0)).when(position > 0, |cell| {
                cell.child(
                    div()
                        .id(("set-transition", position))
                        .px(px(6.0))
                        .rounded(px(4.0))
                        .cursor_pointer()
                        .text_color(palette.text_secondary)
                        .hover(move |style| style.bg(raised))
                        .tooltip(|window, cx| {
                            Tooltip::new(
                                "Hear the last 20 seconds of the track before, then this track from its start.",
                            )
                            .build(window, cx)
                        })
                        .child("Play transition")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.play_set_transition(position, cx);
                            cx.stop_propagation();
                        }))
                        .test_support(),
                )
            }))
            .child(with_rule_tooltip(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().child(title))
                    .when(!artist.is_empty(), |cell| {
                        cell.child(
                            div()
                                .truncate()
                                .text_color(palette.text_tertiary)
                                .child(artist),
                        )
                    }),
                "set-row-title",
            ))
            .child(row_button(
                ("set-earlier", position),
                IconName::ArrowUp,
                can_move_earlier,
                &palette,
                cx.listener(move |this, _, _, cx| {
                    this.move_set_track(position, false, cx);
                    cx.stop_propagation();
                }),
            ))
            .child(row_button(
                ("set-later", position),
                IconName::ArrowDown,
                can_move_later,
                &palette,
                cx.listener(move |this, _, _, cx| {
                    this.move_set_track(position, true, cx);
                    cx.stop_propagation();
                }),
            ))
            .child(
                div()
                    .id(("set-remove", position))
                    .px(px(6.0))
                    .rounded(px(4.0))
                    .text_color(palette.text_tertiary)
                    .map(|button| {
                        if is_locked {
                            button
                                .opacity(0.4)
                                .tooltip(|window, cx| {
                                    Tooltip::new("Unlock to remove").build(window, cx)
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_set_track(position, cx);
                                    cx.stop_propagation();
                                }))
                        } else {
                            button
                                .cursor_pointer()
                                .hover(move |style| style.bg(raised))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_set_track(position, cx);
                                    cx.stop_propagation();
                                }))
                        }
                    })
                    .child("Remove")
                    .test_support(),
            )
            .test_support();
        rows = rows.child(row);
    }
    rows
}

fn row_button(
    id: (&'static str, usize),
    icon: IconName,
    is_enabled: bool,
    palette: &Palette,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let hover_background = palette.raised;
    div()
        .id(id)
        .p(px(4.0))
        .rounded(px(4.0))
        .text_color(palette.text_secondary)
        .child(Icon::new(icon).size(px(12.0)))
        .map(|button| {
            if is_enabled {
                button
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover_background))
                    .on_click(on_click)
            } else {
                button.opacity(0.3)
            }
        })
        .test_support()
        .into_any_element()
}

fn set_actions(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let saved = app
        .set_builder
        .saved_as
        .and_then(|id| app.session.playlists().get(id))
        .map(|playlist| playlist.name.clone());
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            secondary_button(
                if saved.is_some() {
                    "Saved as a playlist"
                } else {
                    "Save as playlist"
                },
                &palette,
            )
            .id("save-set")
            .on_click(cx.listener(|this, _, _, cx| {
                this.save_set_as_playlist(cx);
            }))
            .test_support(),
        )
        .child(
            secondary_button("Export M3U8…", &palette)
                .id("export-set")
                .on_click(cx.listener(|this, _, _, cx| this.export_set(cx)))
                .test_support(),
        )
        .when_some(saved, |actions, name| {
            actions.child(
                div()
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_tertiary)
                    .child(name),
            )
        })
}
