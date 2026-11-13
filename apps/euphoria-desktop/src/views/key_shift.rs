// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The section of the track detail that moves the selected track into
//! another key: hear it, see what it mixes with there, export the copy.

use euphoria_core::shift::MAXIMUM_SHIFT_SEMITONES;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{Context, Div, TestSupportExt, div, px};

use crate::app::EuphoriaApp;
use crate::library::Track;
use crate::views::tag_panel::choice_button;
use crate::views::{SMALL_TEXT_SIZE, section_label};

/// The section for a track with a key, or `None` for one without, which
/// has no key to change.
pub(crate) fn section(
    track: &Track,
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<Div> {
    let key = track.key()?;
    let palette = app.theme.palette;
    let notation = app.display.key_notation;
    let semitones = app.key_shift;
    let shifted = key.transposed(semitones);

    let step = |id: &'static str, label: &'static str, change: i8, is_possible: bool| {
        choice_button(id, label, false, &palette)
            .when(!is_possible, |button| button.opacity(0.4).cursor_default())
            .when(is_possible, |button| {
                button.on_click(cx.listener(move |this, _, _, cx| {
                    let next = this.key_shift + change;
                    this.set_key_shift(next, cx);
                }))
            })
            .test_support()
    };
    let down = step(
        "key-shift-down",
        "−",
        -1,
        semitones > -MAXIMUM_SHIFT_SEMITONES,
    );
    let up = step("key-shift-up", "+", 1, semitones < MAXIMUM_SHIFT_SEMITONES);
    let amount = div()
        .flex()
        .items_center()
        .justify_center()
        .min_w(px(34.0))
        .text_color(if semitones == 0 {
            palette.text_tertiary
        } else {
            palette.text
        })
        .child(if semitones == 0 {
            "0".to_string()
        } else {
            format!("{semitones:+}")
        });
    let reset = (semitones != 0).then(|| {
        choice_button("key-shift-reset", "Original key", false, &palette)
            .on_click(cx.listener(|this, _, _, cx| this.set_key_shift(0, cx)))
            .test_support()
    });
    let export = (semitones != 0).then(|| {
        choice_button("key-shift-export", "Export shifted copy…", false, &palette)
            .tooltip(|window, cx| {
                Tooltip::new(
                    "Writes a FLAC copy in the new key, with the same tempo and the new key in its tags. The original is not changed.",
                )
                .build(window, cx)
            })
            .on_click(cx.listener(|this, _, _, cx| this.export_shifted_copy(cx)))
            .test_support()
    });
    let description = if semitones == 0 {
        format!(
            "In {}. Shift it to hear it in another key and see what it mixes with there.",
            notation.spell(&key)
        )
    } else {
        format!(
            "Heard and suggested in {}, {} {} from {}. The tempo stays the same.",
            notation.spell(&shifted),
            semitones.unsigned_abs(),
            if semitones.unsigned_abs() == 1 {
                "semitone"
            } else {
                "semitones"
            },
            notation.spell(&key)
        )
    };

    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(section_label("Change key", &palette))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.0))
                    .child(down)
                    .child(amount)
                    .child(up)
                    .children(reset)
                    .children(export),
            )
            .child(
                div()
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_tertiary)
                    .child(description),
            ),
    )
}
