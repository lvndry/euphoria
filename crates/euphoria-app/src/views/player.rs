// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The player controls: the play and pause button, the position and length,
//! and the bar along the bottom of the window. In the layout with a strip
//! under the table, the controls sit in the strip beside the wide waveform
//! and there is no separate bar.

use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, Context, Div, FontWeight, Hsla, MouseButton, MouseDownEvent,
    MouseMoveEvent, PathBuilder, Pixels, TestSupportExt, canvas, div, fill, point, px, relative,
    size,
};

use crate::app::EuphoriaApp;
use crate::views::SMALL_TEXT_SIZE;
use crate::views::waveform::SeenBounds;

const BUTTON_SIZE: f32 = 30.0;
const GLYPH_SIZE: f32 = 10.0;

/// The round play and pause button. It is faint and does nothing while no
/// track is selected.
pub(crate) fn play_button(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> AnyElement {
    let palette = app.theme.palette;
    let shows_pause = app.player.offers_pause();
    let has_track = app.player.track().is_some();
    let glyph_colour = palette.background;

    div()
        .id("play-button")
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(BUTTON_SIZE))
        .rounded(px(BUTTON_SIZE / 2.0))
        .bg(palette.text)
        .map(|button| {
            if has_track {
                button
                    .cursor_pointer()
                    .hover(move |style| style.bg(palette.text_secondary))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_playback(cx)))
            } else {
                button.opacity(0.35)
            }
        })
        .child(
            canvas(
                |_, _, _| (),
                move |bounds: Bounds<Pixels>, _, window, _| {
                    if shows_pause {
                        paint_pause_glyph(bounds, glyph_colour, window);
                    } else {
                        paint_play_glyph(bounds, glyph_colour, window);
                    }
                },
            )
            .size(px(GLYPH_SIZE)),
        )
        .test_support()
        .into_any_element()
}

/// Two upright bars.
fn paint_pause_glyph(bounds: Bounds<Pixels>, colour: Hsla, window: &mut gpui::Window) {
    let bar_width = GLYPH_SIZE * 0.3;
    for left in [GLYPH_SIZE * 0.12, GLYPH_SIZE * 0.58] {
        let bar = Bounds::new(
            bounds.origin + point(px(left), px(GLYPH_SIZE * 0.08)),
            size(px(bar_width), px(GLYPH_SIZE * 0.84)),
        );
        window.paint_quad(fill(bar, colour));
    }
}

/// A triangle pointing right, nudged right so that it looks centred.
fn paint_play_glyph(bounds: Bounds<Pixels>, colour: Hsla, window: &mut gpui::Window) {
    let corner = |across: f32, down: f32| {
        bounds.origin + point(px(GLYPH_SIZE * across), px(GLYPH_SIZE * down))
    };
    let mut builder = PathBuilder::fill();
    builder.move_to(corner(0.22, 0.04));
    builder.line_to(corner(0.98, 0.5));
    builder.line_to(corner(0.22, 0.96));
    builder.close();
    if let Ok(path) = builder.build() {
        window.paint_path(path, colour);
    }
}

/// Makes `strip` a place to seek: a press jumps to that point of the track,
/// and dragging with the button held follows the pointer. `seen_bounds` is
/// where the strip was last drawn.
pub(crate) fn seek_on_press_and_drag(
    strip: gpui::Stateful<Div>,
    seen_bounds: &SeenBounds,
    cx: &mut Context<'_, EuphoriaApp>,
) -> gpui::Stateful<Div> {
    let bounds_on_press = seen_bounds.clone();
    let bounds_on_drag = seen_bounds.clone();
    strip
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.seek_to_pointer(event.position.x, &bounds_on_press, cx);
                cx.stop_propagation();
            }),
        )
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            if event.pressed_button == Some(MouseButton::Left) {
                this.seek_to_pointer(event.position.x, &bounds_on_drag, cx);
            }
        }))
}

/// The bar along the bottom of the window: the button, the track, the
/// position, a thin progress line that can be clicked to seek, and the
/// length.
pub(crate) fn bar(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> AnyElement {
    let palette = app.theme.palette;
    let track = app.selected_track();
    let name = track.map(|track| track.file_name.clone());
    let detail = match (app.player.failure(), track) {
        (Some(problem), _) => Some((problem.to_string(), palette.danger)),
        (None, Some(track)) => Some((track.folder.clone(), palette.text_secondary)),
        (None, None) => None,
    };
    let progress = app.player.progress().unwrap_or(0.0);
    let position = crate::format::duration(app.player.position_seconds());
    let length = app
        .player
        .duration_seconds()
        .map(crate::format::duration)
        .unwrap_or_else(|| "\u{2013}".to_string());

    let seen_bounds = app.progress_line_bounds.clone();
    let progress_line = div()
        .id("progress-line")
        .flex()
        .flex_1()
        .items_center()
        .min_w(px(80.0))
        .h(px(20.0))
        .child(
            div()
                .relative()
                .w_full()
                .h(px(4.0))
                .rounded(px(2.0))
                .bg(palette.raised)
                .child(
                    canvas(
                        move |bounds: Bounds<Pixels>, _, _| seen_bounds.set(Some(bounds)),
                        |_, _, _, _| (),
                    )
                    .absolute()
                    .size_full(),
                )
                .child(
                    div()
                        .h_full()
                        .w(relative(progress))
                        .rounded(px(2.0))
                        .bg(palette.text_secondary),
                ),
        );

    div()
        .id("player-bar")
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(14.0))
        .px(px(14.0))
        .py(px(8.0))
        .border_t_1()
        .border_color(palette.line)
        .bg(palette.sidebar)
        .child(play_button(app, cx))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_shrink_0()
                .w(px(220.0))
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .when(name.is_none(), |title| {
                            title.text_color(palette.text_tertiary)
                        })
                        .child(name.unwrap_or_else(|| "No track selected".to_string())),
                )
                .children(detail.map(|(text, colour)| {
                    div()
                        .truncate()
                        .text_size(SMALL_TEXT_SIZE)
                        .text_color(colour)
                        .child(text)
                })),
        )
        .child(
            div()
                .flex()
                .flex_shrink_0()
                .justify_end()
                .w(px(36.0))
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_secondary)
                .child(position),
        )
        .child(seek_on_press_and_drag(progress_line, &app.progress_line_bounds, cx).test_support())
        .child(
            div()
                .flex_shrink_0()
                .w(px(36.0))
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_secondary)
                .child(length),
        )
        .test_support()
        .into_any_element()
}
