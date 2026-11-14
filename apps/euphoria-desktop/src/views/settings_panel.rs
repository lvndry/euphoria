// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings panel: how keys and tempos are shown, and how tags are
//! written.

use gpui::component::input::Input;
use gpui::prelude::*;
use gpui::{AnyElement, Context, FontWeight, TestSupportExt, div, px};

use crate::app::EuphoriaApp;
use crate::display_settings::TempoPreset;
use crate::tag_settings::{LabelChoice, NotationChoice, ProgramChoice};
use crate::theme::Palette;
use crate::views::tag_panel::choice_button;
use crate::views::{SMALL_TEXT_SIZE, modal, primary_button, secondary_button, section_label};

/// The settings panel.
pub(crate) fn render(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> AnyElement {
    let palette = app.theme.palette;
    let settings = app.tag_settings;
    let (backdrop, card) = modal("settings-panel", px(680.0), &palette);

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
            settings.effective_label() == place,
            &palette,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.set_label_place(place, cx)))
        .test_support()
    });
    let display = app.display;
    let shown_notations = NotationChoice::ALL
        .iter()
        .enumerate()
        .map(|(index, notation)| {
            let notation = *notation;
            choice_button(
                ("key-display", index),
                notation.label(),
                display.key_notation == notation,
                &palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_key_display(notation, cx)))
            .test_support()
        });
    let current_preset = (!app.is_custom_tempo_range)
        .then(|| display.tempo_range.preset())
        .flatten();
    let mut tempo_choices: Vec<AnyElement> = TempoPreset::ALL
        .iter()
        .enumerate()
        .map(|(index, preset)| {
            let preset = *preset;
            choice_button(
                ("tempo-preset", index),
                preset.label(),
                current_preset == Some(preset),
                &palette,
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.set_tempo_preset(preset, window, cx);
            }))
            .test_support()
            .into_any_element()
        })
        .collect();
    tempo_choices.push(
        choice_button("tempo-custom", "Custom", current_preset.is_none(), &palette)
            .on_click(cx.listener(|this, _, window, cx| this.choose_custom_tempo_range(window, cx)))
            .test_support()
            .into_any_element(),
    );
    let custom_range = current_preset.is_none().then(|| {
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_secondary)
                    .child("From")
                    .child(
                        div()
                            .w(px(64.0))
                            .child(Input::new(&app.tempo_from_input).id("tempo-from")),
                    )
                    .child("up to")
                    .child(
                        div()
                            .w(px(64.0))
                            .child(Input::new(&app.tempo_to_input).id("tempo-to")),
                    )
                    .child("beats per minute"),
            )
            .children(app.tempo_range_problem.clone().map(|problem| {
                div()
                    .id("tempo-range-problem")
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.danger)
                    .child(problem)
                    .test_support()
            }))
    });

    let metronome = app.metronome.settings;
    let metronome_sounds =
        crate::metronome::ClickSound::ALL
            .iter()
            .enumerate()
            .map(|(index, sound)| {
                let sound = *sound;
                choice_button(
                    ("metronome-sound", index),
                    sound.label(),
                    metronome.sound == sound,
                    &palette,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.set_metronome_sound(sound, cx)))
                .test_support()
            });
    let metronome_volumes = (1..=crate::metronome_controls::VOLUME_STEPS).map(|step| {
        choice_button(
            ("metronome-volume", usize::from(step)),
            format!(
                "{}%",
                u32::from(step) * 100 / u32::from(crate::metronome_controls::VOLUME_STEPS)
            ),
            app.metronome.volume_step() == step,
            &palette,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.set_metronome_volume(step, cx)))
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
                .child(
                    div()
                        .id("settings-body")
                        .flex()
                        .flex_col()
                        .gap(px(14.0))
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                .child(part_heading("Showing keys and tempos", &palette))
                .child(setting(
                    "Keys",
                    "How keys are written in the table and the track detail. The wheel always \
                     uses Camelot codes.",
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.0))
                        .children(shown_notations),
                    &palette,
                ))
                .child(setting(
                    "Tempo range",
                    "Tempos are shown from the first number up to the second, at most an octave \
                     apart. A track counted at half or double speed moves into the range; \
                     nothing is analyzed again.",
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(6.0))
                                .children(tempo_choices),
                        )
                        .children(custom_range),
                    &palette,
                ))
                .child(part_heading("Writing tags", &palette))
                .child(setting(
                    "DJ program",
                    "Tags are written to the fields this program reads.",
                    div().flex().flex_wrap().gap(px(6.0)).children(programs),
                    &palette,
                ))
                .child(setting(
                    "Key notation",
                    "How the key is spelled in the tags, in the key field and in the label.",
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
                .child(label_preview_box(app, &palette))
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
                .child(setting(
                    "Existing key and tempo",
                    "A key or tempo a file already has is kept, so a correct tag is never \
                     replaced by a guess. One that Euphoria wrote earlier is still updated.",
                    div().flex().child(
                        choice_button(
                            "replace-existing",
                            if settings.replace_existing {
                                "\u{2713} Replace existing key and tempo"
                            } else {
                                "Replace existing key and tempo"
                            },
                            settings.replace_existing,
                            &palette,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_replace_existing(cx)))
                        .test_support(),
                    ),
                    &palette,
                ))
                .child(setting(
                    "Low confidence",
                    "A key or tempo rated low confidence is left out and listed in the \
                     preview. A key or tempo you corrected counts as high.",
                    div().flex().child(
                        choice_button(
                            "include-low-confidence",
                            if settings.include_low_confidence {
                                "\u{2713} Also write low confidence values"
                            } else {
                                "Also write low confidence values"
                            },
                            settings.include_low_confidence,
                            &palette,
                        )
                        .on_click(
                            cx.listener(|this, _, _, cx| this.toggle_include_low_confidence(cx)),
                        )
                        .test_support(),
                    ),
                    &palette,
                ))
                .child(setting(
                    "Cue markers",
                    "The track's cues and beat grid are written as Serato cue \
                     points, which Serato DJ, Mixxx and VirtualDJ read too. \
                     Memory cues, on no pad, are left out. Formats that cannot \
                     hold them are skipped.",
                    div().flex().child(
                        choice_button(
                            "write-cue-markers",
                            if settings.write_cue_markers {
                                "\u{2713} Write the cue markers"
                            } else {
                                "Write the cue markers"
                            },
                            settings.write_cue_markers,
                            &palette,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_write_cue_markers(cx)))
                        .test_support(),
                    ),
                    &palette,
                ))
                .child(setting(
                    "After analysis",
                    "Fills in each track's missing key and tempo once it is analyzed, using \
                     the choices above. On by default; tags a file already has are kept. \
                     Undo tag write puts them back.",
                    div().flex().child(
                        choice_button(
                            "write-after-analysis",
                            if settings.write_after_analysis {
                                "\u{2713} Write key and tempo to files after analysis"
                            } else {
                                "Write key and tempo to files after analysis"
                            },
                            settings.write_after_analysis,
                            &palette,
                        )
                        .on_click(
                            cx.listener(|this, _, _, cx| this.toggle_write_after_analysis(cx)),
                        )
                        .test_support(),
                    ),
                    &palette,
                ))
                .child(part_heading("Metronome", &palette))
                .child(setting(
                    "Sound",
                    "The click of the metronome button in the toolbar. Choosing a sound plays \
                     one click of it.",
                    div().flex().flex_wrap().gap(px(6.0)).children(metronome_sounds),
                    &palette,
                ))
                .child(setting(
                    "Volume",
                    "How loud the clicks are next to the music.",
                    div().flex().flex_wrap().gap(px(6.0)).children(metronome_volumes),
                    &palette,
                ))
                .child(setting(
                    "Accent",
                    "The first beat of every bar of four is higher and louder.",
                    div().flex().child(
                        choice_button(
                            "metronome-accent",
                            if metronome.accent {
                                "\u{2713} Accent the first beat"
                            } else {
                                "Accent the first beat"
                            },
                            metronome.accent,
                            &palette,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_metronome_accent(cx)))
                        .test_support(),
                    ),
                    &palette,
                ))
                .test_support()
                )
                .child(
                    div()
                        .flex()
                        .flex_shrink_0()
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

/// The title of a part of the settings.
fn part_heading(text: &'static str, palette: &Palette) -> impl IntoElement {
    div()
        .pt(px(6.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette.text)
        .child(text)
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

/// The box under the label choices: what the choices write to one track.
fn label_preview_box(app: &EuphoriaApp, palette: &Palette) -> impl IntoElement {
    let preview = app.label_preview.as_ref();
    let rows = preview.into_iter().flat_map(|preview| {
        preview.fields.iter().enumerate().map(|(index, field)| {
            let value = div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_wrap()
                .child(field.before.clone())
                .child(
                    div()
                        .id(("label-preview-label", index))
                        .text_color(palette.accent)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(field.label.clone())
                        .test_support(),
                )
                .child(field.after.clone());
            div()
                .id(("label-preview-field", index))
                .flex()
                .gap(px(8.0))
                .child(
                    div()
                        .w(px(84.0))
                        .flex_shrink_0()
                        .text_color(palette.text_secondary)
                        .child(format!("{}:", field.name)),
                )
                .child(value)
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(palette.text_tertiary)
                        .child(field.tag.clone()),
                )
                .test_support()
        })
    });
    let notes = preview.into_iter().flat_map(|preview| {
        preview.notes.iter().enumerate().map(|(index, note)| {
            div()
                .id(("label-preview-note", index))
                .text_color(palette.text_tertiary)
                .child(note.clone())
                .test_support()
        })
    });
    let hints = preview.into_iter().flat_map(|preview| {
        preview.cue_hints.iter().enumerate().map(|(index, hint)| {
            div()
                .id(("label-preview-hint", index))
                .text_color(palette.text_secondary)
                .child(hint.clone())
                .test_support()
        })
    });
    div()
        .id("label-preview")
        .flex()
        .flex_col()
        .gap(px(4.0))
        .p(px(10.0))
        .rounded(px(6.0))
        .bg(palette.raised)
        .text_size(SMALL_TEXT_SIZE)
        .child(
            div()
                .flex()
                .justify_between()
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.text)
                        .child("In your files"),
                )
                .children(preview.map(|preview| {
                    div()
                        .id("label-preview-source")
                        .text_color(palette.text_tertiary)
                        .child(preview.source.caption())
                        .test_support()
                })),
        )
        .children(rows)
        .children(notes)
        .children(hints)
        .test_support()
}
