// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The settings panel: how keys and tempos are shown, and how tags are
//! written, and how tracks are separated into stems.

use gpui::component::input::Input;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{AnyElement, Context, FontWeight, ScrollHandle, SharedString, TestSupportExt, div, px};

use crate::app::EuphoriaApp;
use crate::display_settings::TempoPreset;
use crate::stem_settings::{MODES, mode_label};
use crate::surface_motion::Modal;
use crate::tag_settings::{LabelChoice, NotationChoice, ProgramChoice};
use crate::theme::Palette;
use crate::track_filters::STAGE_EXPLANATION;
use crate::views::controls::{segment, segmented, toggle_row};
use crate::views::{
    GAP_MEDIUM, GAP_SMALL, GAP_TIGHT, LABEL_TEXT_SIZE, RADIUS_MEDIUM, TITLE_TEXT_SIZE,
};
use crate::views::{
    SMALL_TEXT_SIZE, dialog_primary_button, modal, secondary_button, section_label,
};

/// The settings panel.
pub(crate) fn render(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> AnyElement {
    let palette = app.theme.palette;
    let settings = app.tag_settings;
    let (backdrop, card) = modal(
        "settings-panel",
        px(680.0),
        &palette,
        app.modal_fades.value(Modal::Settings),
    );

    let programs = ProgramChoice::ALL
        .iter()
        .enumerate()
        .map(|(index, program)| {
            let program = *program;
            segment(
                ("program", index),
                program.label(),
                settings.program == program,
                index == 0,
                &palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_tag_program(program, cx)))
            .test_support()
            .into_any_element()
        })
        .collect::<Vec<_>>();
    let places = LabelChoice::ALL
        .iter()
        .enumerate()
        .map(|(index, place)| {
            let place = *place;
            segment(
                ("label-place", index),
                place.label(),
                settings.effective_label() == place,
                index == 0,
                &palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_label_place(place, cx)))
            .test_support()
            .into_any_element()
        })
        .collect::<Vec<_>>();
    let display = app.display;
    let notations = NotationChoice::ALL
        .iter()
        .enumerate()
        .map(|(index, notation)| {
            let notation = *notation;
            segment(
                ("notation", index),
                notation.label(),
                settings.key_notation == notation,
                index == 0,
                &palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_key_notation(notation, cx)))
            .test_support()
            .into_any_element()
        })
        .collect::<Vec<_>>();
    let current_preset = (!app.is_custom_tempo_range)
        .then(|| display.tempo_range.preset())
        .flatten();
    let mut tempo_choices: Vec<AnyElement> = TempoPreset::ALL
        .iter()
        .enumerate()
        .map(|(index, preset)| {
            let preset = *preset;
            segment(
                ("tempo-preset", index),
                preset.label(),
                current_preset == Some(preset),
                index == 0,
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
        segment(
            "tempo-custom",
            "Custom",
            current_preset.is_none(),
            false,
            &palette,
        )
        .on_click(cx.listener(|this, _, window, cx| this.choose_custom_tempo_range(window, cx)))
        .test_support()
        .into_any_element(),
    );
    let custom_range = current_preset.is_none().then(|| {
        div()
            .flex()
            .flex_col()
            .gap(GAP_SMALL)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(GAP_SMALL)
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
    let metronome_sounds = crate::metronome::ClickSound::ALL
        .iter()
        .enumerate()
        .map(|(index, sound)| {
            let sound = *sound;
            segment(
                ("metronome-sound", index),
                sound.label(),
                metronome.sound == sound,
                index == 0,
                &palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_metronome_sound(sound, cx)))
            .test_support()
            .into_any_element()
        })
        .collect::<Vec<_>>();
    let metronome_volumes = (1..=crate::metronome_controls::VOLUME_STEPS)
        .map(|step| {
            segment(
                ("metronome-volume", usize::from(step)),
                format!(
                    "{}%",
                    u32::from(step) * 100 / u32::from(crate::metronome_controls::VOLUME_STEPS)
                ),
                app.metronome.volume_step() == step,
                step == 1,
                &palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_metronome_volume(step, cx)))
            .test_support()
            .into_any_element()
        })
        .collect::<Vec<_>>();

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
    let mut parts: Vec<AnyElement> =
        vec![part_heading("Showing keys and tempos", &palette).into_any_element()];
    parts.push(
        setting(
            "Key notation",
            "key-notation",
            "In the table, the tags and the exports. The wheel always shows Camelot.",
            None,
            segmented(notations, &palette),
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
                .gap(GAP_SMALL)
                .child(segmented(tempo_choices, &palette))
                .children(custom_range),
            &palette,
        )
        .into_any_element(),
    );
    parts.push(
        toggle_row(
            "show-stage-column",
            "Stage column",
            "Adds a Stage column to the track table: warm-up, build or peak.",
            display.show_stage_column,
            &app.switches,
            &palette,
        )
        .tooltip(|window, cx| Tooltip::new(STAGE_EXPLANATION).build(window, cx))
        .on_click(cx.listener(|this, _, _, cx| this.toggle_stage_column(cx)))
        .test_support()
        .into_any_element(),
    );
    parts.push(
        toggle_row(
            "show-matching-keys",
            "Show matching keys on the wheel",
            "Marks the keys that mix with the selected one. Click the wheel to choose the key moves.",
            display.show_matching_keys,
            &app.switches,
            &palette,
        )
        .on_click(cx.listener(|this, _, _, cx| this.toggle_matching_keys(cx)))
        .test_support()
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
            segmented(programs, &palette),
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
            segmented(places, &palette),
            &palette,
        )
        .into_any_element(),
    );
    parts.push(
        toggle_row(
            "write-tempo",
            "Write the tempo",
            tempo_hint,
            settings.write_tempo,
            &app.switches,
            &palette,
        )
        .tooltip(move |window, cx| Tooltip::new(tempo_detail).build(window, cx))
        .on_click(cx.listener(|this, _, _, cx| this.toggle_write_tempo(cx)))
        .test_support()
        .into_any_element(),
    );
    parts.push(
        toggle_row(
            "replace-existing",
            "Keep keys and tempos already in files",
            "Turn off to replace them with Euphoria's.",
            !settings.replace_existing,
            &app.switches,
            &palette,
        )
        .tooltip(|window, cx| {
            Tooltip::new(
                "A correct tag is never replaced by a guess. One that Euphoria wrote earlier is still updated.",
            )
            .build(window, cx)
        })
        .on_click(cx.listener(|this, _, _, cx| this.toggle_replace_existing(cx)))
        .test_support()
        .into_any_element(),
    );
    parts.push(
        toggle_row(
            "include-low-confidence",
            "Write low confidence values",
            "Usually worth checking by ear first; they are listed when left out.",
            settings.include_low_confidence,
            &app.switches,
            &palette,
        )
        .tooltip(|window, cx| {
            Tooltip::new("A key or tempo you corrected counts as high confidence.")
                .build(window, cx)
        })
        .on_click(cx.listener(|this, _, _, cx| this.toggle_include_low_confidence(cx)))
        .test_support()
        .into_any_element(),
    );
    parts.push(
        toggle_row(
            "write-cue-markers",
            "Write cue markers",
            "Read by Serato DJ, Mixxx and VirtualDJ.",
            settings.write_cue_markers,
            &app.switches,
            &palette,
        )
        .tooltip(|window, cx| {
            Tooltip::new(
                "The track's cues and beat grid go in as Serato cue points. Memory cues, on no pad, are left out. Formats that cannot hold them are skipped.",
            )
            .build(window, cx)
        })
        .on_click(cx.listener(|this, _, _, cx| this.toggle_write_cue_markers(cx)))
        .test_support()
        .into_any_element(),
    );
    parts.push(
        toggle_row(
            "colour-cues-by-energy",
            "Colour cues by energy",
            "Blue for calm sections up to red for intense ones.",
            settings.colour_cues_by_energy,
            &app.switches,
            &palette,
        )
        .tooltip(|window, cx| {
            Tooltip::new(
                "Applies to the cues analysis placed, in cue markers and in exports to DJ programs. Your own cues keep their pad colour. Off keeps each program's pad colours.",
            )
            .build(window, cx)
        })
        .on_click(cx.listener(|this, _, _, cx| this.toggle_colour_cues_by_energy(cx)))
        .test_support()
        .into_any_element(),
    );
    parts.push(
        toggle_row(
            "write-after-analysis",
            "Write key and tempo after analysis",
            "Fills in each track's missing key and tempo once it is analyzed.",
            settings.write_after_analysis,
            &app.switches,
            &palette,
        )
        .tooltip(|window, cx| {
            Tooltip::new(
                "Uses the choices above; tags a file already has are kept. Undo tag write puts them back.",
            )
            .build(window, cx)
        })
        .on_click(cx.listener(|this, _, _, cx| this.toggle_write_after_analysis(cx)))
        .test_support()
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
            segmented(metronome_sounds, &palette),
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
            segmented(metronome_volumes, &palette),
            &palette,
        )
        .into_any_element(),
    );
    parts.push(
        toggle_row(
            "metronome-accent",
            "Accent the first beat",
            "The first beat of every bar of four is higher and louder.",
            metronome.accent,
            &app.switches,
            &palette,
        )
        .on_click(cx.listener(|this, _, _, cx| this.toggle_metronome_accent(cx)))
        .test_support()
        .into_any_element(),
    );

    let current_mode = app.stem_settings.mode();
    let stem_modes = MODES
        .into_iter()
        .enumerate()
        .map(|(index, mode)| {
            segment(
                ("separation-mode", index),
                mode_label(mode),
                current_mode == mode,
                index == 0,
                &palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_stems_mode(mode, cx)))
            .test_support()
            .into_any_element()
        })
        .collect::<Vec<_>>();
    parts.push(part_heading("Stems", &palette).into_any_element());
    parts.push(
        setting(
            "Separation",
            "separation-mode",
            "Advanced mode separates more cleanly, most of all in the vocals.",
            Some(
                "It takes about seven times as long, needs about 4.5 GB of free memory, \
                 and downloads 214 MB the first time. Stems it made are used in either mode.",
            ),
            segmented(stem_modes, &palette),
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
                        .text_size(TITLE_TEXT_SIZE)
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
                                .gap(GAP_MEDIUM)
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
                            dialog_primary_button("Done", &palette)
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
        .gap(GAP_SMALL)
        .child(section_label(name, palette))
        .child(
            div()
                .id(SharedString::from(format!("hint-{key}")))
                .flex()
                .items_center()
                .gap(GAP_SMALL)
                .text_size(SMALL_TEXT_SIZE)
                .text_color(text_color)
                .test_support()
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
                        .text_size(LABEL_TEXT_SIZE)
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
/// the top. The bounds of the body's parts are where they sit unscrolled. `box_index` and `next_part_index` are the places in the body
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
    let scrolled_by = scroll.offset().y;
    in_flow.origin.y + scrolled_by < body_top
        && next_part.origin.y + scrolled_by > body_top + pinned_height
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
                    .line_through()
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
                .gap(GAP_SMALL)
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
        .gap(GAP_TIGHT)
        .p(px(10.0))
        .rounded(RADIUS_MEDIUM)
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
