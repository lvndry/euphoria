// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The set builder page: the curve to draw, the pool, the length, and the
//! set built from them with how each track meets the curve.

use euphoria_core::harmony::set::DEFAULT_MAX_BPM_STEP;
use euphoria_core::{CurvePreset, Deviation, SetPlan};
use gpui::component::tooltip::Tooltip;
use gpui::component::{Icon, IconName};
use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, Context, Div, FontWeight, Hsla, MouseButton, MouseDownEvent,
    MouseMoveEvent, PathBuilder, Pixels, SharedString, TestSupportExt, canvas, div, fill, point,
    px, size,
};

use crate::app::EuphoriaApp;
use crate::format;
use crate::set_builder::{FEWEST_TRACKS, MOST_TRACKS, PlannedSet, SetPool};
use crate::theme::Palette;
use crate::views::key_chip::key_chip;
use crate::views::tag_panel::choice_button;
use crate::views::{
    LABEL_TEXT_SIZE, SMALL_TEXT_SIZE, primary_button, secondary_button, section_label,
};

const CURVE_HEIGHT: f32 = 200.0;
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
            page.child(summary(app, planned))
                .child(set_actions(app, cx))
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
    labelled("Pool", row, &palette)
}

/// The curve to draw on, and once a set is built, its energy over it.
fn curve_card(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let builder = &app.set_builder;
    let curve = builder.curve.clone();
    let set_points: Vec<(f32, f32, bool)> = builder
        .planned
        .as_ref()
        .map(|planned| {
            planned
                .plan
                .tracks
                .iter()
                .enumerate()
                .map(|(position, track)| {
                    (
                        track.position,
                        track.energy,
                        is_off_target(&planned.plan, position),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let seen_bounds = builder.curve_bounds.clone();
    let colours = CurveColours {
        grid: palette.line,
        curve: palette.accent,
        set: palette.text,
        miss: palette.danger,
    };
    let drawing = div()
        .id("set-curve")
        .relative()
        .h(px(CURVE_HEIGHT))
        .w_full()
        .cursor_crosshair()
        .child(
            canvas(
                move |bounds, _, _| seen_bounds.set(Some(bounds)),
                move |bounds, (), window, _| {
                    paint_curve(window, bounds, &curve, &set_points, colours);
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
        )
        .test_support();

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
                .child(legend_swatch("Set", palette.text))
                .child(legend_swatch("Off the curve", palette.danger))
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
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .p(px(12.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(palette.line)
        .bg(palette.panel)
        .child(
            div()
                .flex()
                .gap(px(8.0))
                .child(axis)
                .child(div().flex_1().min_w_0().child(drawing)),
        )
        .child(legend)
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
    set: Hsla,
    miss: Hsla,
}

fn paint_curve(
    window: &mut gpui::Window,
    bounds: Bounds<Pixels>,
    curve: &[f32],
    set_points: &[(f32, f32, bool)],
    colours: CurveColours,
) {
    let to_point = |along: f32, energy: f32| {
        let up = ((energy - 1.0) / 9.0).clamp(0.0, 1.0);
        point(
            bounds.origin.x + bounds.size.width * along.clamp(0.0, 1.0),
            bounds.origin.y + bounds.size.height * (1.0 - up),
        )
    };
    for energy in ENERGY_GRID {
        let at = to_point(0.0, energy);
        window.paint_quad(fill(
            Bounds::new(point(at.x, at.y), size(bounds.size.width, px(1.0))),
            colours.grid,
        ));
    }
    if curve.len() >= 2 {
        let last = (curve.len() - 1) as f32;
        let mut area = PathBuilder::fill();
        area.move_to(point(bounds.origin.x, bounds.origin.y + bounds.size.height));
        for (index, level) in curve.iter().enumerate() {
            area.line_to(to_point(index as f32 / last, *level));
        }
        area.line_to(point(
            bounds.origin.x + bounds.size.width,
            bounds.origin.y + bounds.size.height,
        ));
        area.close();
        if let Ok(path) = area.build() {
            window.paint_path(path, colours.curve.opacity(0.12));
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
    if set_points.len() >= 2 {
        let mut line = PathBuilder::stroke(px(1.5));
        for (index, (along, energy, _)) in set_points.iter().enumerate() {
            let at = to_point(*along, *energy);
            if index == 0 {
                line.move_to(at);
            } else {
                line.line_to(at);
            }
        }
        if let Ok(path) = line.build() {
            window.paint_path(path, colours.set.opacity(0.7));
        }
    }
    for (along, energy, missed) in set_points {
        let at = to_point(*along, *energy);
        let radius = if *missed { px(4.0) } else { px(3.0) };
        window.paint_quad(
            fill(
                Bounds::new(
                    point(at.x - radius, at.y - radius),
                    size(radius * 2.0, radius * 2.0),
                ),
                if *missed { colours.miss } else { colours.set },
            )
            .corner_radii(radius),
        );
    }
}

fn is_off_target(plan: &SetPlan, position: usize) -> bool {
    plan.deviations.iter().any(|deviation| {
        matches!(deviation, Deviation::EnergyOffTarget { position: off, .. } if *off == position)
    })
}

fn summary(app: &EuphoriaApp, planned: &PlannedSet) -> AnyElement {
    let palette = app.theme.palette;
    let library = app.session.library();
    let seconds: f32 = planned
        .paths
        .iter()
        .filter_map(|path| {
            library
                .index_of(path)
                .and_then(|index| library.track(index))
        })
        .filter_map(|track| track.duration_seconds())
        .sum();
    let plan = &planned.plan;
    let rules = if plan.every_transition_is_clean() {
        format!("every mix in key and within {DEFAULT_MAX_BPM_STEP:.0} BPM")
    } else {
        "some mixes break a rule".to_string()
    };
    let headline = format!(
        "{} tracks, {}. Energy within {:.1} of the curve on average; {rules}.",
        planned.paths.len(),
        format::long_duration(seconds),
        plan.mean_energy_error(),
    );
    let mut notes = div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.danger);
    for message in deviation_messages(plan) {
        notes = notes.child(message);
    }
    div()
        .id("set-summary")
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(div().font_weight(FontWeight::MEDIUM).child(headline))
        .child(notes)
        .test_support()
        .into_any_element()
}

/// One sentence per place the set falls short, positions counted from 1.
pub(crate) fn deviation_messages(plan: &SetPlan) -> Vec<String> {
    plan.deviations
        .iter()
        .map(|deviation| match *deviation {
            Deviation::ShortPool {
                requested,
                available,
            } => format!(
                "The pool has {available} usable tracks, so the set has {available} instead of {requested}."
            ),
            Deviation::EnergyOffTarget {
                position,
                target,
                actual,
            } => format!(
                "Track {} is at energy {actual:.0} where the curve asks for {target:.1}.",
                position + 1
            ),
            Deviation::KeyClash { position, relation } => format!(
                "Track {} to {} is {}, not an allowed key move.",
                position,
                position + 1,
                relation.map_or("a key clash", |relation| relation.label())
            ),
            Deviation::TempoJump {
                position,
                bpm_change,
            } => format!(
                "Track {} to {} changes tempo by {bpm_change:+.1} BPM.",
                position,
                position + 1
            ),
            Deviation::RepeatedArtist { position } => format!(
                "Tracks {} and {} are by the same artist.",
                position,
                position + 1
            ),
        })
        .collect()
}

fn set_rows(app: &EuphoriaApp, planned: &PlannedSet, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let library = app.session.library();
    let plan = &planned.plan;
    let count = planned.paths.len();
    let raised = palette.raised;
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
        let off_target = is_off_target(plan, position);
        let title: SharedString = track
            .map(|track| track.title().to_string())
            .unwrap_or_else(|| path.display().to_string())
            .into();
        let artist: SharedString = track
            .and_then(|track| track.artist.clone())
            .unwrap_or_default()
            .into();
        let step = transition.map(|transition| {
            let colour = if transition.tempo_allowed {
                palette.text_secondary
            } else {
                palette.danger
            };
            div()
                .text_color(colour)
                .child(format!("{:+.1}", transition.bpm_change))
        });
        let movement = transition.map(|transition| {
            let colour = if transition.key_allowed {
                palette.text_secondary
            } else {
                palette.danger
            };
            div().text_color(colour).child(
                transition
                    .relation
                    .map_or("Key clash", |relation| relation.label()),
            )
        });
        let energy = planned_track.map(|planned_track| {
            div()
                .flex()
                .gap(px(4.0))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(if off_target {
                            palette.danger
                        } else {
                            palette.text
                        })
                        .child(format!("{:.0}", planned_track.energy)),
                )
                .child(
                    div()
                        .text_color(palette.text_tertiary)
                        .child(format!("of {:.1}", planned_track.target_energy)),
                )
        });
        let row = div()
            .id(("set-row", position))
            .flex()
            .items_center()
            .gap(px(10.0))
            .px(px(12.0))
            .py(px(6.0))
            .text_size(SMALL_TEXT_SIZE)
            .when(position > 0, |row| {
                row.border_t_1().border_color(palette.line)
            })
            .when(is_locked, |row| row.bg(palette.selection.opacity(0.5)))
            .child(
                div()
                    .w(px(22.0))
                    .text_color(palette.text_tertiary)
                    .child((position + 1).to_string()),
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
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_set_lock(position, cx)))
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
            .child(div().w(px(64.0)).children(energy))
            .child(
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
            )
            .child(row_button(
                ("set-earlier", position),
                IconName::ArrowUp,
                position > 0,
                &palette,
                cx.listener(move |this, _, _, cx| this.move_set_track(position, false, cx)),
            ))
            .child(row_button(
                ("set-later", position),
                IconName::ArrowDown,
                position + 1 < count,
                &palette,
                cx.listener(move |this, _, _, cx| this.move_set_track(position, true, cx)),
            ))
            .child(
                div()
                    .id(("set-remove", position))
                    .px(px(6.0))
                    .rounded(px(4.0))
                    .cursor_pointer()
                    .text_color(palette.text_tertiary)
                    .hover(move |style| style.bg(raised))
                    .child("Remove")
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.remove_set_track(position, cx)),
                    )
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
