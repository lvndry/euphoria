// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings panel: how tags are written.

use gpui::prelude::*;
use gpui::{AnyElement, Context, FontWeight, TestSupportExt, div, px};

use crate::app::EuphoriaApp;
use crate::tag_settings::{LabelChoice, NotationChoice, ProgramChoice};
use crate::theme::Palette;
use crate::views::tag_panel::choice_button;
use crate::views::{SMALL_TEXT_SIZE, modal, primary_button, secondary_button, section_label};

/// The settings panel.
pub(crate) fn render(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> AnyElement {
    let palette = app.theme.palette;
    let settings = app.tag_settings;
    let (backdrop, card) = modal("settings-panel", px(620.0), &palette);

    let programs = ProgramChoice::ALL
        .iter()
        .enumerate()
        .map(|(index, program)| {
            let program = *program;
            choice_button(
                ("program", index),
                program.label(),
                settings.program == program,
                &palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_tag_program(program, cx)))
            .test_support()
        });
    let notations = NotationChoice::ALL
        .iter()
        .enumerate()
        .map(|(index, notation)| {
            let notation = *notation;
            choice_button(
                ("notation", index),
                notation.label(),
                settings.key_notation == notation,
                &palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_key_notation(notation, cx)))
            .test_support()
        });
    let places = LabelChoice::ALL.iter().enumerate().map(|(index, place)| {
        let place = *place;
        choice_button(
            ("label-place", index),
            place.label(),
            settings.label == place,
            &palette,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.set_label_place(place, cx)))
        .test_support()
    });
    let tempo_hint = if settings.program == ProgramChoice::Serato {
        "Serato detects the tempo itself and builds its beat grid from it only when a file has \
         none, so it is left alone unless you tick this."
    } else {
        "Rounded to a whole number, except for Mixxx, which reads decimals."
    };

    backdrop
        .child(
            card.id("settings-card")
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Settings"),
                )
                .child(setting(
                    "DJ program",
                    "Tags are written to the fields this program reads.",
                    div().flex().flex_wrap().gap(px(6.0)).children(programs),
                    &palette,
                ))
                .child(setting(
                    "Key notation",
                    "How the key is spelled, in the key field and in the label.",
                    div().flex().flex_wrap().gap(px(6.0)).children(notations),
                    &palette,
                ))
                .child(setting(
                    "Energy label",
                    "Where a label such as \u{201c}8A - Energy 7\u{201d} goes. The text already \
                     there is kept.",
                    div().flex().flex_wrap().gap(px(6.0)).children(places),
                    &palette,
                ))
                .child(setting(
                    "Tempo",
                    tempo_hint,
                    div().flex().child(
                        choice_button(
                            "write-tempo",
                            if settings.write_tempo {
                                "\u{2713} Write the tempo"
                            } else {
                                "Write the tempo"
                            },
                            settings.write_tempo,
                            &palette,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_write_tempo(cx)))
                        .test_support(),
                    ),
                    &palette,
                ))
                .child(
                    div()
                        .flex()
                        .justify_between()
                        .pt(px(4.0))
                        .child(
                            secondary_button("Undo last tag write", &palette)
                                .id("settings-undo")
                                .on_click(cx.listener(|this, _, _, cx| this.open_tag_undo(cx)))
                                .test_support(),
                        )
                        .child(
                            primary_button("Done", &palette)
                                .id("close-settings")
                                .on_click(cx.listener(|this, _, _, cx| this.close_settings(cx)))
                                .test_support(),
                        ),
                )
                .test_support(),
        )
        .test_support()
        .into_any_element()
}

/// One setting: its name, a line on what it does, and its choices.
fn setting(
    name: &'static str,
    hint: &'static str,
    choices: impl IntoElement,
    palette: &Palette,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(section_label(name, palette))
        .child(
            div()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_secondary)
                .child(hint),
        )
        .child(choices)
}
