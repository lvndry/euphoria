// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The playback tempo control in the focus view header: the target tempo in
//! a field with arrows, the change in percent, a reset button and the key
//! lock switch.

use gpui::component::input::Input;
use gpui::component::tooltip::Tooltip;
use gpui::component::{Icon, IconName};
use gpui::prelude::*;
use gpui::{
    ClickEvent, Context, Div, FontWeight, KeyDownEvent, MouseButton, ScrollEvent, TestSupportExt,
    div, px,
};

use crate::app::EuphoriaApp;
use crate::format;
use crate::tempo_controls::{
    KEY_LOCK_RULE, TEMPO_FIELD_RULE, TEMPO_RESET_RULE, is_original, percent_text,
};
use crate::theme::Palette;
use crate::views::controls::toggle_row;
use crate::views::{
    CONTROL_HEIGHT, CONTROL_RADIUS, GAP_SMALL, GAP_TIGHT, SMALL_TEXT_SIZE, secondary_button,
};

const FIELD_WIDTH: f32 = 104.0;
const STEPPER_WIDTH: f32 = 14.0;
const PERCENT_WIDTH: f32 = 48.0;

/// The tempo control for the selected track, or nothing while it has no
/// tempo to change.
pub(crate) fn tempo_control(
    app: &EuphoriaApp,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<Div> {
    let target = app.tempo_target_bpm()?;
    let rate = app.tempo_rate();
    let changed = !is_original(rate);
    let field = if app.tempo.is_editing {
        div()
            .relative()
            .w(px(FIELD_WIDTH))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let is_down = match event.keystroke.key.as_str() {
                    "up" => false,
                    "down" => true,
                    _ => return,
                };
                cx.stop_propagation();
                this.step_tempo(is_down, event.keystroke.modifiers.alt, window, cx);
            }))
            .child(Input::new(&app.tempo_input).id("focus-tempo-field"))
            .child(stepper(app, palette, cx))
            .into_any_element()
    } else {
        div()
            .id("focus-tempo-bpm")
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .gap(GAP_TIGHT)
            .relative()
            .pr(px(STEPPER_WIDTH))
            .w(px(FIELD_WIDTH))
            .h(CONTROL_HEIGHT)
            .rounded(CONTROL_RADIUS)
            .border_1()
            .border_color(palette.line)
            .bg(palette.raised)
            .text_size(SMALL_TEXT_SIZE)
            .text_color(if changed {
                palette.text
            } else {
                palette.text_secondary
            })
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(|this, _, window, cx| this.edit_tempo(window, cx)))
            .on_scroll(cx.listener(|this, event: &ScrollEvent, window, cx| {
                let notches = event.delta.pixel_delta(px(12.0)).y / px(12.0);
                if notches != 0.0 {
                    this.step_tempo(notches < 0.0, event.modifiers.alt, window, cx);
                }
            }))
            .tooltip(|window, cx| Tooltip::new(TEMPO_FIELD_RULE).build(window, cx))
            .child(format::bpm(target))
            .child(div().text_color(palette.text_tertiary).child("BPM"))
            .child(stepper(app, palette, cx))
            .test_support()
            .into_any_element()
    };
    let percent = div()
        .id("focus-tempo-percent")
        .flex()
        .justify_center()
        .w(px(PERCENT_WIDTH))
        .text_size(SMALL_TEXT_SIZE)
        .font_weight(FontWeight::MEDIUM)
        .text_color(if changed {
            palette.accent
        } else {
            palette.text_tertiary
        })
        .child(percent_text(rate))
        .test_support();
    let reset = secondary_button("Reset", palette)
        .id("focus-tempo-reset")
        .when(!changed, |button| button.opacity(0.4))
        .on_click(cx.listener(|this, _, _, cx| this.reset_tempo(cx)))
        .tooltip(|window, cx| Tooltip::new(TEMPO_RESET_RULE).build(window, cx))
        .test_support();
    let key_lock = toggle_row(
        "focus-key-lock",
        "Key lock",
        "Keeps the pitch",
        app.tempo.key_lock,
        &app.switches,
        palette,
    )
    .tooltip(|window, cx| Tooltip::new(KEY_LOCK_RULE).build(window, cx))
    .on_click(cx.listener(|this, _, _, cx| this.toggle_key_lock(cx)))
    .test_support();
    Some(
        div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(GAP_SMALL)
            .child(field)
            .child(percent)
            .child(reset)
            .child(key_lock),
    )
}

fn stepper(app: &EuphoriaApp, palette: &Palette, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let arrow = |is_down: bool, cx: &mut Context<'_, EuphoriaApp>| {
        let can_step = app.can_step_tempo(is_down);
        let hover_background = palette.selection;
        let hover_colour = palette.text;
        div()
            .id(if is_down {
                "focus-tempo-down"
            } else {
                "focus-tempo-up"
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
                if this.can_step_tempo(is_down) {
                    this.step_tempo(is_down, event.modifiers().alt, window, cx);
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
        .w(px(STEPPER_WIDTH))
        .flex()
        .flex_col()
        .child(arrow(false, cx))
        .child(arrow(true, cx))
}
