// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The strip along the top: app name, search, analysis progress and commands.
//! It also stands in for the title bar, so dragging it moves the window.

use gpui::component::button::{Button, ButtonVariants};
use gpui::component::input::Input;
use gpui::component::menu::DropdownMenu;
use gpui::component::tooltip::Tooltip;
use gpui::component::{Icon, IconName, InteractiveElementExt};
use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, ClickEvent, Context, Div, FontWeight, KeyDownEvent, MouseButton,
    PathBuilder, Pixels, ScrollEvent, Stateful, TestSupportExt, canvas, div, point, px, relative,
};

use crate::actions;
use crate::app::EuphoriaApp;
use crate::metronome_controls::TEMPO_RULE;
use crate::theme::{Palette, ThemeName};
use crate::views::controls::pressed_button;
use crate::views::logo::busy_logo;
use crate::views::{CONTROL_HEIGHT, CONTROL_RADIUS, SMALL_TEXT_SIZE, secondary_button};
use crate::views::{GAP_MEDIUM, GAP_SMALL, GAP_TIGHT, RADIUS_MEDIUM, RADIUS_SMALL};

/// Room left for the window's close, minimize and zoom buttons on macOS.
const WINDOW_BUTTONS_WIDTH: f32 = if cfg!(target_os = "macos") { 72.0 } else { 0.0 };

/// Width of the app name, chosen so the search field starts where the
/// sidebar ends.
const APP_NAME_WIDTH: f32 = 194.0 - WINDOW_BUTTONS_WIDTH;

/// The button that folds the sidebar away and brings it back. It sits
/// beside the window's own buttons, where macOS apps put it, and stays in
/// the toolbar in both states, so there is always a way back.
fn sidebar_toggle(
    app: &EuphoriaApp,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let hover_background = palette.raised;
    let hover_colour = palette.text;
    let colour = if app.panels.sidebar_folded {
        palette.text_tertiary
    } else {
        palette.text_secondary
    };
    div()
        .id("sidebar-toggle")
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(24.0))
        .rounded(RADIUS_MEDIUM)
        .text_color(colour)
        .cursor_pointer()
        .hover(move |style| style.bg(hover_background).text_color(hover_colour))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|this, _, _, cx| this.toggle_sidebar(cx)))
        .child(Icon::new(IconName::PanelLeft).size(px(16.0)))
        .test_support()
        .into_any_element()
}

/// The logo beside the app name. It plays its trace while work runs, with
/// the work in a tooltip, and is the plain mark otherwise.
fn logo_mark(app: &EuphoriaApp, palette: &Palette) -> impl IntoElement {
    let description = app.busy_description();
    let mark = busy_logo(
        px(18.0),
        palette.text,
        palette.sidebar,
        app.logo_clock.clone(),
        description.is_some(),
    );
    div()
        .id(if description.is_some() {
            "logo-busy"
        } else {
            "logo-idle"
        })
        .flex_shrink_0()
        .when_some(description, |mark_area, description| {
            mark_area.tooltip(move |window, cx| Tooltip::new(description.clone()).build(window, cx))
        })
        .child(mark)
        .test_support()
}

/// The toolbar.
pub(crate) fn render(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Stateful<Div> {
    let palette = app.theme.palette;
    let has_tracks = !app.session.library().tracks().is_empty();
    let is_searching_only = app.scans.is_running() && app.session.progress().label().is_none();

    div()
        .id("toolbar")
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(GAP_MEDIUM)
        .pl(px(14.0 + WINDOW_BUTTONS_WIDTH))
        .pr(px(14.0))
        .py(px(8.0))
        .border_b_1()
        .border_color(palette.line)
        .bg(palette.sidebar)
        .on_double_click(|_, window, _| window.titlebar_double_click())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, _| this.toolbar_drag_armed = true),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, _| this.toolbar_drag_armed = false),
        )
        .on_mouse_down_out(cx.listener(|this, _, _, _| this.toolbar_drag_armed = false))
        .on_mouse_move(cx.listener(|this, _, window, _| {
            if this.toolbar_drag_armed {
                this.toolbar_drag_armed = false;
                window.start_window_move();
            }
        }))
        .child(
            div()
                .flex()
                .items_center()
                .gap(GAP_SMALL)
                .w(px(APP_NAME_WIDTH))
                .flex_shrink_0()
                .font_weight(FontWeight::SEMIBOLD)
                .child(sidebar_toggle(app, &palette, cx))
                .child(logo_mark(app, &palette))
                .child("Euphoria"),
        )
        .when(has_tracks, |toolbar| {
            toolbar.child(search_field(app, &palette, cx))
        })
        .child(
            div()
                .flex()
                .when(is_searching_only, |commands| commands.min_w_0())
                .when(!is_searching_only, |commands| commands.flex_shrink_0())
                .items_center()
                .gap(GAP_MEDIUM)
                .ml_auto()
                .children(progress(app, &palette, cx))
                .child(metronome(app, &palette, cx))
                .child(
                    secondary_button("Live", &palette)
                        .id("open-live")
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(|_, _, cx| crate::live_app::open_panel(cx))
                        .test_support(),
                )
                .when(has_tracks, |commands| {
                    commands.child(
                        pressed_button("toggle-decks", "Decks", app.decks.is_open, &palette)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(
                                cx.listener(|this, _, _, cx| this.toggle_decks_from_command(cx)),
                            )
                            .test_support(),
                    )
                })
                .when(has_tracks, |commands| {
                    commands.child(write_tags(&palette, cx))
                })
                .when(has_tracks, |commands| {
                    commands.child(
                        secondary_button("Add folder", &palette)
                            .id("add-folder")
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|this, _, _, cx| this.choose_folder(cx))),
                    )
                })
                .child(theme_menu(app.theme.name)),
        )
}

const METRONOME_GLYPH_SIZE: f32 = 14.0;

/// Width of the tempo field, room for "300.0 BPM".
const METRONOME_FIELD_WIDTH: f32 = 94.0;

/// Width of the tempo field while the clicks follow the playing track, room
/// for "300.0 BPM locked to track".
const METRONOME_LOCKED_FIELD_WIDTH: f32 = 188.0;

/// The tempo field's tooltip while the clicks follow the playing track.
const LOCKED_RULE: &str = "Locked to the playing track: clicks land on its beats, with the \
     accent on the bar's first beat, and follow the track's playback tempo. With no beat grid, \
     or nothing playing, it clicks freely at the tempo set.";

/// Width of the column of arrows at the field's right edge.
const METRONOME_STEPPER_WIDTH: f32 = 14.0;

/// The metronome: a toggle button that starts and stops the clicks, and
/// the tempo beside it. The tempo is typed after a click on it or moved
/// with the scroll wheel, one beat for a notch and a tenth with Alt held.
fn metronome(app: &EuphoriaApp, palette: &Palette, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let is_running = app.metronome.is_running();
    let hover_background = palette.raised;
    let hover_colour = palette.text;
    let (colour, background) = if is_running {
        (palette.accent_ink, palette.accent)
    } else {
        (palette.text_secondary, palette.sidebar)
    };
    let button = div()
        .id("metronome-toggle")
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(24.0))
        .rounded(RADIUS_MEDIUM)
        .bg(background)
        .cursor_pointer()
        .when(!is_running, |button| {
            button.hover(move |style| style.bg(hover_background).text_color(hover_colour))
        })
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener(|this, _, _, cx| this.toggle_metronome(cx)))
        .tooltip(move |window, cx| {
            Tooltip::new(if is_running {
                "Stop the metronome (M)"
            } else {
                "Start the metronome (M)"
            })
            .build(window, cx)
        })
        .child(
            canvas(
                |_, _, _| (),
                move |bounds: Bounds<Pixels>, _, window, _| {
                    paint_metronome_glyph(bounds, colour, window);
                },
            )
            .size(px(METRONOME_GLYPH_SIZE)),
        )
        .test_support();

    let bpm = app.metronome_bpm();
    let is_locked = app.metronome_locked_bpm().is_some();
    let field = if app.metronome.is_editing {
        div()
            .relative()
            .w(px(METRONOME_FIELD_WIDTH))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let is_down = match event.keystroke.key.as_str() {
                    "up" => false,
                    "down" => true,
                    _ => return,
                };
                cx.stop_propagation();
                this.step_metronome_bpm(is_down, event.keystroke.modifiers.alt, window, cx);
            }))
            .child(Input::new(&app.metronome_input).id("metronome-field"))
            .child(stepper(app, palette, cx))
            .child(
                div()
                    .absolute()
                    .right(px(METRONOME_STEPPER_WIDTH + 4.0))
                    .top_0()
                    .bottom_0()
                    .flex()
                    .items_center()
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_tertiary)
                    .child("BPM"),
            )
            .into_any_element()
    } else {
        div()
            .id("metronome-bpm")
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .gap(GAP_TIGHT)
            .relative()
            .pr(px(METRONOME_STEPPER_WIDTH))
            .w(px(if is_locked {
                METRONOME_LOCKED_FIELD_WIDTH
            } else {
                METRONOME_FIELD_WIDTH
            }))
            .h(CONTROL_HEIGHT)
            .rounded(CONTROL_RADIUS)
            .border_1()
            .border_color(palette.line)
            .bg(palette.raised)
            .text_size(SMALL_TEXT_SIZE)
            .text_color(if app.metronome.typed_bpm.is_some() {
                palette.text
            } else {
                palette.text_secondary
            })
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| this.edit_metronome_bpm(window, cx)))
            .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                let notches = event.delta.pixel_delta(px(12.0)).y / px(12.0);
                let step = if event.modifiers.alt { 0.1 } else { 1.0 };
                this.nudge_metronome_bpm(notches * step, cx);
            }))
            .tooltip(move |window, cx| {
                Tooltip::new(if is_locked { LOCKED_RULE } else { TEMPO_RULE }).build(window, cx)
            })
            .child(crate::format::bpm(bpm))
            .child(div().text_color(palette.text_tertiary).child("BPM"))
            .when(is_locked, |field| {
                field.child(
                    div()
                        .id("metronome-locked")
                        .text_color(palette.accent)
                        .child("locked to track")
                        .test_support(),
                )
            })
            .child(stepper(app, palette, cx))
            .test_support()
            .into_any_element()
    };
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(GAP_TIGHT)
        .child(button)
        .child(field)
}

/// The up and down arrows at the right edge of the tempo field. Each steps
/// the tempo by a beat, or a tenth with Alt held, and dims at its limit.
fn stepper(app: &EuphoriaApp, palette: &Palette, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let arrow = |is_down: bool, cx: &mut Context<'_, EuphoriaApp>| {
        let can_step = app.can_step_metronome_bpm(is_down);
        let hover_background = palette.selection;
        let hover_colour = palette.text;
        div()
            .id(if is_down {
                "metronome-down"
            } else {
                "metronome-up"
            })
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .w_full()
            .text_color(if can_step {
                palette.text_secondary
            } else {
                palette.text_tertiary.opacity(0.4)
            })
            .when(can_step, |arrow| {
                arrow
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover_background).text_color(hover_colour))
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                cx.stop_propagation();
                if this.can_step_metronome_bpm(is_down) {
                    this.step_metronome_bpm(is_down, event.modifiers().alt, window, cx);
                }
            }))
            .tooltip(move |window, cx| {
                Tooltip::new(if is_down {
                    "Down 1 BPM (Alt: 0.1)"
                } else {
                    "Up 1 BPM (Alt: 0.1)"
                })
                .build(window, cx)
            })
            .child(
                Icon::new(if is_down {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronUp
                })
                .size(px(10.0)),
            )
            .test_support()
    };
    div()
        .absolute()
        .right(px(1.0))
        .top(px(1.0))
        .bottom(px(1.0))
        .w(px(METRONOME_STEPPER_WIDTH))
        .flex()
        .flex_col()
        .child(arrow(false, cx))
        .child(arrow(true, cx))
}

/// A metronome: a trapezoid body on a base, with a slanted arm that rises
/// past the top and carries a small weight.
fn paint_metronome_glyph(bounds: Bounds<Pixels>, colour: gpui::Hsla, window: &mut gpui::Window) {
    let corner = |across: f32, down: f32| {
        bounds.origin
            + point(
                px(METRONOME_GLYPH_SIZE * across),
                px(METRONOME_GLYPH_SIZE * down),
            )
    };
    let mut stroke = |width: f32, points: &[(f32, f32)], is_closed: bool| {
        let mut builder = PathBuilder::stroke(px(width));
        for (index, (across, down)) in points.iter().enumerate() {
            if index == 0 {
                builder.move_to(corner(*across, *down));
            } else {
                builder.line_to(corner(*across, *down));
            }
        }
        if is_closed {
            builder.close();
        }
        if let Ok(path) = builder.build() {
            window.paint_path(path, colour);
        }
    };
    stroke(
        1.3,
        &[(0.3, 0.16), (0.7, 0.16), (0.93, 0.94), (0.07, 0.94)],
        true,
    );
    stroke(1.3, &[(0.5, 0.8), (0.76, 0.0)], false);
    stroke(3.4, &[(0.64, 0.32), (0.67, 0.24)], false);
}

/// Edit tags and Write tags. The DJ program the tags are written for is named
/// in the Write tags preview, and chosen in the settings.
fn write_tags(palette: &Palette, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(GAP_SMALL)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            secondary_button("Edit tags", palette)
                .id("edit-tags")
                .on_click(cx.listener(|this, _, window, cx| this.open_tag_editor(window, cx)))
                .test_support(),
        )
        .child(
            secondary_button("Write tags", palette)
                .id("write-tags")
                .on_click(cx.listener(|this, _, _, cx| this.open_tag_writer(cx)))
                .test_support(),
        )
}

fn search_field(app: &EuphoriaApp, palette: &Palette, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let hover_colour = palette.text;
    div()
        .flex()
        .flex_1()
        .items_center()
        .gap(GAP_SMALL)
        .max_w(px(420.0))
        .h(CONTROL_HEIGHT)
        .px(px(10.0))
        .rounded(CONTROL_RADIUS)
        .border_1()
        .border_color(palette.line)
        .bg(palette.raised)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            Icon::new(IconName::Search)
                .size(px(13.0))
                .text_color(palette.text_tertiary),
        )
        .child(
            div().flex_1().min_w_0().child(
                Input::new(&app.search_input)
                    .id("search")
                    .appearance(false)
                    .cleanable(true),
            ),
        )
        .child(
            div()
                .id("palette-hint")
                .flex()
                .flex_shrink_0()
                .items_center()
                .px(px(5.0))
                .rounded(RADIUS_SMALL)
                .border_1()
                .border_color(palette.line)
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .cursor_pointer()
                .hover(move |style| style.text_color(hover_colour))
                .child(if cfg!(target_os = "macos") {
                    "\u{2318}K"
                } else {
                    "Ctrl+K"
                })
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.palette.open.is_none() {
                        this.open_command_palette(window, cx);
                    }
                }))
                .test_support(),
        )
}

/// "12 of 340 done" with a bar, or "Looking for audio files in
/// ~/Music/DJ tracks" with the whole paths in a tooltip while folders are
/// being searched. Nothing when idle.
fn progress(
    app: &EuphoriaApp,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<AnyElement> {
    let progress = app.session.progress();
    let is_searching = progress.label().is_none();
    let label = match (progress.label(), app.scans.label_parts()) {
        (Some(label), _) => div().child(label).into_any_element(),
        (None, Some((lead, path, rest))) => div()
            .id("scan-status")
            .flex()
            .min_w_0()
            .whitespace_nowrap()
            .child(div().flex_shrink_0().child(lead))
            .when_some(path, |status, path| {
                status.child(
                    div()
                        .id("scan-status-path")
                        .min_w_0()
                        .ml(px(4.0))
                        .text_ellipsis_middle()
                        .child(path)
                        .test_support(),
                )
            })
            .when(!rest.is_empty(), |status| {
                status.child(div().flex_shrink_0().child(rest))
            })
            .when_some(app.scans.tooltip(), |status, paths| {
                status.tooltip(move |window, cx| Tooltip::new(paths.clone()).build(window, cx))
            })
            .test_support()
            .into_any_element(),
        (None, None) => return None,
    };
    Some(
        div()
            .flex()
            .id("toolbar-progress")
            .when(is_searching, |row| row.min_w_0().overflow_hidden())
            .when(!is_searching, |row| row.flex_shrink_0())
            .items_center()
            .gap(GAP_SMALL)
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_secondary)
            .child(label)
            .when_some(progress.time_left_label(), |row, time_left| {
                row.child(
                    div()
                        .id("analysis-time-left")
                        .text_color(palette.text_tertiary)
                        .child(time_left)
                        .test_support(),
                )
            })
            .when(progress.is_active(), |row| {
                row.child(
                    div()
                        .id("analysis-stop")
                        .px(px(5.0))
                        .rounded(RADIUS_SMALL)
                        .border_1()
                        .border_color(palette.line)
                        .text_color(palette.text_tertiary)
                        .cursor_pointer()
                        .child("Stop")
                        .tooltip(|window, cx| {
                            Tooltip::new(
                                "Skip the tracks still waiting. Tracks being analyzed finish.",
                            )
                            .build(window, cx)
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.stop_analysis(cx)))
                        .test_support(),
                )
                .child(
                    div()
                        .w(px(72.0))
                        .h(px(4.0))
                        .rounded(RADIUS_SMALL)
                        .bg(palette.raised)
                        .child(
                            div()
                                .h_full()
                                .w(relative(progress.fraction()))
                                .rounded(RADIUS_SMALL)
                                .bg(palette.accent),
                        ),
                )
            })
            .test_support()
            .into_any_element(),
    )
}

fn theme_menu(current: ThemeName) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(
            Button::new("theme-menu")
                .ghost()
                .icon(IconName::Palette)
                .label(current.label())
                .dropdown_caret(true)
                .dropdown_menu(move |menu, _, _| {
                    ThemeName::ALL.into_iter().fold(menu, |menu, theme| {
                        menu.menu_with_check(
                            theme.label(),
                            theme == current,
                            actions::use_theme(theme),
                        )
                    })
                }),
        )
}

/// Where the window's own controls sit, the top left of the first of the
/// three: the traffic light position the window is opened with in main.rs.
pub(crate) const WINDOW_CONTROLS_ORIGIN: (f32, f32) = (14.0, 15.0);
/// How far apart the three window controls are, centre to centre.
const WINDOW_CONTROL_SPACING: f32 = 20.0;
/// How wide each window control is.
const WINDOW_CONTROL_SIZE: f32 = 12.0;

/// A line and a fill under the window's three controls while the window is
/// not the active one: the system draws them as faint grey discs, which a
/// light toolbar swallows.
pub(crate) fn inactive_window_controls(palette: &Palette) -> Div {
    div().children((0..3).map(|index| {
        div()
            .absolute()
            .top(px(WINDOW_CONTROLS_ORIGIN.1))
            .left(px(
                WINDOW_CONTROLS_ORIGIN.0 + index as f32 * WINDOW_CONTROL_SPACING
            ))
            .size(px(WINDOW_CONTROL_SIZE))
            .rounded_full()
            .border_1()
            .border_color(palette.line)
            .bg(palette.raised)
    }))
}
