// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The strip along the top: app name, search, analysis progress and commands.
//! It also stands in for the title bar, so dragging it moves the window.

use gpui::component::button::{Button, ButtonVariants};
use gpui::component::input::Input;
use gpui::component::menu::DropdownMenu;
use gpui::component::{Icon, IconName, InteractiveElementExt};
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, FontWeight, MouseButton, Stateful, TestSupportExt, div, px, relative,
};

use crate::actions;
use crate::app::EuphoriaApp;
use crate::theme::{Palette, ThemeName};
use crate::views::logo::logo;
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
                .child(logo(px(18.0), palette.text))
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
