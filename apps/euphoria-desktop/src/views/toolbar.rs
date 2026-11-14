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
use crate::views::logo::busy_logo;
use crate::views::tag_panel::choice_button;
use crate::views::{CONTROL_HEIGHT, CONTROL_RADIUS, SMALL_TEXT_SIZE, secondary_button};

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
        .rounded(px(5.0))
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
    let has_tracks = !app.is_empty();

    div()
        .id("toolbar")
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(12.0))
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
                .gap(px(6.0))
                .w(px(APP_NAME_WIDTH))
                .flex_shrink_0()
                .font_weight(FontWeight::SEMIBOLD)
                .child(sidebar_toggle(app, &palette, cx))
                .child(logo_mark(app, &palette))
                .child("Euphoria"),
        )
        .when(has_tracks, |toolbar| {
            toolbar.child(search_field(app, &palette))
        })
        .child(
            div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .gap(px(10.0))
                .ml_auto()
                .children(progress(app, &palette))
                .child(metronome(app, &palette, cx))
                .when(has_tracks, |commands| {
                    commands.child(
                        choice_button("toggle-decks", "Decks", app.decks.is_open, &palette)
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_decks(cx)))
                            .test_support(),
                    )
                })
                .when(has_tracks, |commands| {
                    commands.child(write_tags(app, &palette, cx))
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
        .rounded(px(5.0))
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
            .gap(px(4.0))
            .relative()
            .pr(px(METRONOME_STEPPER_WIDTH))
            .w(px(METRONOME_FIELD_WIDTH))
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
            .tooltip(|window, cx| Tooltip::new(TEMPO_RULE).build(window, cx))
            .child(crate::format::bpm(bpm))
            .child(div().text_color(palette.text_tertiary).child("BPM"))
            .child(stepper(app, palette, cx))
            .test_support()
            .into_any_element()
    };
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(4.0))
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

/// Write tags, with the DJ program they are written for beside it, so the
/// fields of the wrong program are not written by mistake. Clicking the
/// program opens the settings.
fn write_tags(app: &EuphoriaApp, palette: &Palette, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let hover_colour = palette.text;
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(6.0))
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
        .child(
            div()
                .id("tag-program")
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .cursor_pointer()
                .hover(move |style| style.text_color(hover_colour))
                .on_click(cx.listener(|this, _, _, cx| this.open_settings(cx)))
                .child(format!(
                    "for {}",
                    app.tag_settings.program.label_in_sentence()
                ))
                .test_support(),
        )
}

fn search_field(app: &EuphoriaApp, palette: &Palette) -> Div {
    div()
        .flex()
        .flex_1()
        .items_center()
        .gap(px(8.0))
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
}

/// "12 of 340 done" with a bar, or "Looking for audio files" while
/// folders are being searched. Nothing when idle.
fn progress(app: &EuphoriaApp, palette: &Palette) -> Option<Div> {
    let progress = app.session.progress();
    let label = match progress.label() {
        Some(label) => label,
        None if app.scans_in_progress > 0 => "Looking for audio files".to_string(),
        None => return None,
    };
    Some(
        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(8.0))
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_secondary)
            .child(label)
            .when(progress.is_active(), |row| {
                row.child(
                    div()
                        .w(px(72.0))
                        .h(px(4.0))
                        .rounded(px(2.0))
                        .bg(palette.raised)
                        .child(
                            div()
                                .h_full()
                                .w(relative(progress.fraction()))
                                .rounded(px(2.0))
                                .bg(palette.accent),
                        ),
                )
            }),
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
