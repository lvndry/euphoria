// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings panel: how keys and tempos are shown, and how tags are
//! written.

use gpui::component::input::Input;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{AnyElement, Context, FontWeight, ScrollHandle, SharedString, TestSupportExt, div, px};

use crate::app::EuphoriaApp;
use crate::display_settings::TempoPreset;
use crate::tag_settings::{LabelChoice, NotationChoice, ProgramChoice};
use crate::theme::Palette;
use crate::views::tag_panel::choice_button;
use crate::views::{SMALL_TEXT_SIZE, modal, primary_button, secondary_button, section_label};

/// The place of the key notation among the parts of the settings body.
pub(crate) const KEY_NOTATION_ITEM: usize = 1;

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

    let (tempo_hint, tempo_detail) = if settings.program == ProgramChoice::Serato {
        (
            "Serato finds the tempo itself, so it is left alone unless you tick this.",
            "Serato builds its beat grid from the tempo only when a file has none.",
        )
    } else {
        (
            "Written as a whole number, except for Mixxx.",
            "Mixxx reads decimals, so it gets the exact tempo.",
        )
    };
    let toggle = |id: &'static str, label: &'static str, is_on: bool| {
        choice_button(
            id,
            if is_on {
                format!("\u{2713} {label}")
            } else {
                label.to_string()
            },
            is_on,
            &palette,
        )
        .test_support()
    };

    let mut parts: Vec<AnyElement> = Vec::new();
    parts.push(part_heading("Showing keys and tempos", &palette).into_any_element());
    parts.push(
        setting(
            "Key notation",
            "key-notation",
            "In the table, the tags and the exports. The wheel always shows Camelot.",
            None,
            div().flex().flex_wrap().gap(px(6.0)).children(notations),
            &palette,
        )
        .into_any_element(),
    );
    parts.push(
        setting(
            "Tempo range",
            "tempo-range",
            "Tempos are shown from the first number up to the second.",
            Some(
                "At most an octave apart. A track counted at half or double speed moves \
                 into the range; nothing is analyzed again.",
            ),
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
        )
        .into_any_element(),
    );
    parts.push(part_heading("Writing tags", &palette).into_any_element());
    let writing_box_index = parts.len();
    parts.push(label_preview_box(app, "label-preview", &palette).into_any_element());
    parts.push(
        setting(
            "DJ program",
            "dj-program",
            "Tags go to the fields this program reads.",
            None,
            div().flex().flex_wrap().gap(px(6.0)).children(programs),
            &palette,
        )
        .into_any_element(),
    );
    parts.push(
        setting(
            "Energy label",
            "energy-label",
            "Where a label such as \u{201c}8A - Energy 7\u{201d} goes.",
            Some("The text already there is kept."),
            div().flex().flex_wrap().gap(px(6.0)).children(places),
            &palette,
        )
        .into_any_element(),
    );
    parts.push(
        setting(
            "Tempo",
            "tempo",
            tempo_hint,
            Some(tempo_detail),
            div().flex().child(
                toggle("write-tempo", "Write the tempo", settings.write_tempo)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_write_tempo(cx))),
            ),
            &palette,
        )
        .into_any_element(),
    );
    parts.push(
        setting(
            "Existing key and tempo",
            "existing",
            "A key or tempo a file already has is kept.",
            Some(
                "A correct tag is never replaced by a guess. One that Euphoria wrote \
                 earlier is still updated.",
            ),
            div().flex().child(
                toggle(
                    "replace-existing",
                    "Replace existing key and tempo",
                    settings.replace_existing,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_replace_existing(cx))),
            ),
            &palette,
        )
        .into_any_element(),
    );
    parts.push(
        setting(
            "Low confidence",
            "low-confidence",
            "A key or tempo rated low confidence is left out and listed.",
            Some("A key or tempo you corrected counts as high confidence."),
            div().flex().child(
                toggle(
                    "include-low-confidence",
                    "Also write low confidence values",
                    settings.include_low_confidence,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_include_low_confidence(cx))),
            ),
            &palette,
        )
        .into_any_element(),
    );
    parts.push(
        setting(
            "Cue markers",
            "cue-markers",
            "Written as Serato cues; also read by Mixxx and VirtualDJ.",
            Some(
                "The track's cues and beat grid go in as Serato cue points. Memory cues, \
                 on no pad, are left out. Formats that cannot hold them are skipped.",
            ),
            div().flex().child(
                toggle(
                    "write-cue-markers",
                    "Write the cue markers",
                    settings.write_cue_markers,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_write_cue_markers(cx))),
            ),
            &palette,
        )
        .into_any_element(),
    );
    parts.push(
        setting(
            "After analysis",
            "after-analysis",
            "Fills in each track's missing key and tempo once it is analyzed.",
            Some(
                "Uses the choices above; tags a file already has are kept. Undo tag write \
                 puts them back.",
            ),
            div().flex().child(
                toggle(
                    "write-after-analysis",
                    "Write key and tempo to files after analysis",
                    settings.write_after_analysis,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_write_after_analysis(cx))),
            ),
            &palette,
        )
        .into_any_element(),
    );
    let metronome_index = parts.len();
    parts.push(part_heading("Metronome", &palette).into_any_element());
    parts.push(
        setting(
            "Sound",
            "metronome-sound",
            "The click of the toolbar metronome; choosing one plays it.",
            None,
            div()
                .flex()
                .flex_wrap()
                .gap(px(6.0))
                .children(metronome_sounds),
            &palette,
        )
        .into_any_element(),
    );
    parts.push(
        setting(
            "Volume",
            "metronome-volume",
            "How loud the clicks are next to the music.",
            None,
            div()
                .flex()
                .flex_wrap()
                .gap(px(6.0))
                .children(metronome_volumes),
            &palette,
        )
        .into_any_element(),
    );
    parts.push(
        setting(
            "Accent",
            "metronome-accent",
            "The first beat of every bar of four is higher and louder.",
            None,
            div().flex().child(
                toggle(
                    "metronome-accent",
                    "Accent the first beat",
                    metronome.accent,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_metronome_accent(cx))),
            ),
            &palette,
        )
        .into_any_element(),
    );

    let pinned_box =
        settings_box_is_pinned(&app.settings_scroll, writing_box_index, metronome_index).then(
            || {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .right_0()
                    .pb(px(8.0))
                    .bg(palette.panel)
                    .child(label_preview_box(app, "label-preview-pinned", &palette))
            },
        );

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
                        .relative()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .child(
                            div()
                                .id("settings-body")
                                .flex()
                                .flex_col()
                                .gap(px(14.0))
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .track_scroll(&app.settings_scroll)
                                .on_scroll(cx.listener(|_, _, _, cx| cx.notify()))
                                .children(parts)
                                .test_support(),
                        )
                        .children(pinned_box),
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

/// One setting: its name, one short line on what it does with an info
/// glyph that holds the longer explanation, and its choices. `key` names
/// the line for tests.
fn setting(
    name: &'static str,
    key: &'static str,
    hint: &'static str,
    detail: Option<&'static str>,
    choices: impl IntoElement,
    palette: &Palette,
) -> impl IntoElement {
    let text_color = palette.text_secondary;
    let glyph_color = palette.text_tertiary;
    let glyph_line = palette.line;
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(section_label(name, palette))
        .child(
            div()
                .id(SharedString::from(format!("hint-{key}")))
                .flex()
                .items_center()
                .gap(px(6.0))
                .text_size(SMALL_TEXT_SIZE)
                .text_color(text_color)
                .child(div().min_w_0().child(hint))
                .children(detail.map(|detail| {
                    div()
                        .id(SharedString::from(format!("hint-info-{key}")))
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .justify_center()
                        .size(px(14.0))
                        .rounded_full()
                        .border_1()
                        .border_color(glyph_line)
                        .text_size(px(9.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(glyph_color)
                        .cursor_default()
                        .child("i")
                        .tooltip(move |window, cx| Tooltip::new(detail).build(window, cx))
                        .test_support()
                })),
        )
        .child(choices)
}

/// Whether the "In your files" box is out of view above the settings body
/// while the writing options are still on show, so a copy is pinned over
/// the top. `box_index` and `next_part_index` are the places in the body
/// of the in-flow box and of the part after the writing options.
fn settings_box_is_pinned(scroll: &ScrollHandle, box_index: usize, next_part_index: usize) -> bool {
    let body_top = scroll.bounds().origin.y;
    let (Some(in_flow), Some(next_part)) = (
        scroll.bounds_for_item(box_index),
        scroll.bounds_for_item(next_part_index),
    ) else {
        return false;
    };
    let pinned_height = in_flow.size.height + px(8.0);
    in_flow.origin.y < body_top && next_part.origin.y > body_top + pinned_height
}

/// The box at the top of the writing options: what the choices write to
/// one track, a row struck out when an option keeps it out, and how many
/// files of the library the choices would change. `id` tells the in-flow
/// box from its pinned copy.
fn label_preview_box(app: &EuphoriaApp, id: &'static str, palette: &Palette) -> impl IntoElement {
    let preview = app.label_preview.as_ref();
    let rows = preview.into_iter().flat_map(|preview| {
        preview.fields.iter().enumerate().map(|(index, field)| {
            let is_struck = field.struck.is_some();
            let name_color = palette.text_secondary;
            let value_color = if is_struck {
                palette.text_tertiary
            } else {
                palette.text
            };
            let value = div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_wrap()
                .text_color(value_color)
                .when(is_struck, |value| value.line_through())
                .child(field.before.clone())
                .child(
                    div()
                        .text_color(if is_struck {
                            palette.text_tertiary
                        } else {
                            palette.accent
                        })
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(field.label.clone()),
                )
                .child(field.after.clone());
            let name = match &field.struck {
                Some(reason) => div()
                    .flex_shrink_0()
                    .text_color(palette.text_tertiary)
                    .child(format!("{} \u{b7} {reason}", field.name)),
                None => div()
                    .w(px(84.0))
                    .flex_shrink_0()
                    .text_color(name_color)
                    .child(format!("{}:", field.name)),
            };
            let tag = field.tag.clone();
            div()
                .id((id, index))
                .flex()
                .gap(px(8.0))
                .child(name)
                .when(!is_struck, |row| row.child(value))
                .when(is_struck, |row| row.child(div().flex_1()))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(palette.text_tertiary)
                        .child(tag),
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
    let library_line = app.library_change.map_or_else(
        || "Working out your library\u{2026}".to_string(),
        |change| change.sentence(),
    );
    div()
        .id(id)
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
                        .text_color(palette.text_tertiary)
                        .child(preview.source.caption())
                })),
        )
        .children(rows)
        .children(notes)
        .children(hints)
        .child(
            div()
                .id(SharedString::from(format!("{id}-library")))
                .pt(px(4.0))
                .text_color(palette.text_secondary)
                .child(library_line)
                .test_support(),
        )
        .test_support()
}
