// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Checking the key by ear in the track detail: a drone of the key, and of
//! the runner up when one is offered, a one octave piano with the key's
//! scale marked, a volume for both, and the editor that corrects the key, the
//! tempo or the energy by hand.

use euphoria_core::{Key, Mode};
use gpui::component::input::Input;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{Context, Div, FontWeight, Hsla, SharedString, TestSupportExt, div, hsla, px, relative};

use crate::app::EuphoriaApp;
use crate::key_by_ear::{LEVEL_STEPS, scale_of};
use crate::library::Track;
use crate::theme::Palette;
use crate::views::inspector::shown_runner_up;
use crate::views::key_chip::key_chip;
use crate::views::tag_panel::choice_button;
use crate::views::{SMALL_TEXT_SIZE, section_label};

/// The pitch classes of the white keys, from C.
const WHITE_KEYS: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];

/// The pitch classes of the black keys, each with the white key it sits
/// to the right of.
const BLACK_KEYS: [(u8, usize); 5] = [(1, 0), (3, 1), (6, 3), (8, 4), (10, 5)];

/// Note names for the piano, spelled as most DJs read them.
const NOTE_NAMES: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];

/// How tall the piano is drawn.
const PIANO_HEIGHT: f32 = 72.0;

/// How wide a black key is, as a share of the piano's width.
const BLACK_KEY_WIDTH: f32 = 0.09;

/// The key at position `index` of the picker: the minor keys 1A to 12A,
/// then the major keys 1B to 12B, in the order of the Camelot wheel.
pub(crate) fn picker_key(index: usize) -> Key {
    let camelot_number = (index % 12) as i32 + 1;
    let fifths_from_eight = 7 * (camelot_number - 8);
    if index < 12 {
        Key::new((9 + fifths_from_eight).rem_euclid(12) as u8, Mode::Minor)
    } else {
        Key::new(fifths_from_eight.rem_euclid(12) as u8, Mode::Major)
    }
}

/// The link under the key and the tempo that opens the editor, and the
/// editor when it is open: every key to pick from, and a field to type a
/// tempo in.
pub(crate) fn corrector(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let is_editing = app.by_ear.is_editing;
    let hover_colour = palette.text;
    let link = div()
        .id("edit-values")
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .cursor_pointer()
        .hover(move |style| style.text_color(hover_colour))
        .child(if is_editing {
            "Done correcting"
        } else {
            "Correct the key, BPM or energy"
        })
        .on_click(cx.listener(|this, _, window, cx| this.toggle_value_editor(window, cx)))
        .test_support();
    let editor = is_editing.then(|| editor(app, cx));
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(div().flex().child(link))
        .children(editor)
}

fn editor(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let notation = app.display.key_notation;
    let current = app.selected_track().and_then(Track::key);
    let keys = (0..24).map(|index| {
        let key = picker_key(index);
        let is_current = current == Some(key);
        div()
            .id(("pick-key", index))
            .flex()
            .rounded(px(5.0))
            .p(px(1.0))
            .border_1()
            .border_color(if is_current {
                palette.text
            } else {
                gpui::transparent_black()
            })
            .cursor_pointer()
            .child(key_chip(&key, &app.theme, notation))
            .on_click(cx.listener(move |this, _, _, cx| this.accept_key(key, cx)))
            .test_support()
    });
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .p(px(10.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(palette.line)
        .child(section_label("Key", &palette))
        .child(div().flex().flex_wrap().gap(px(2.0)).children(keys))
        .child(section_label("BPM", &palette))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .w(px(80.0))
                        .child(Input::new(&app.bpm_input).id("bpm-field")),
                )
                .child(
                    choice_button("set-bpm", "Use this tempo", false, &palette)
                        .on_click(cx.listener(|this, _, _, cx| this.accept_typed_bpm(cx)))
                        .test_support(),
                ),
        )
        .children(app.by_ear.bpm_problem.clone().map(|problem| {
            div()
                .id("bpm-problem")
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.danger)
                .child(problem)
                .test_support()
        }))
        .child(section_label("Energy", &palette))
        .child(energy_picker(app, cx))
}

/// The energy ratings 1 to 10 to pick from, and a way back to the detected
/// one while the user has set one.
fn energy_picker(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let track = app.selected_track();
    let shown = track.and_then(Track::energy);
    let detected = track.and_then(Track::detected_energy);
    let is_set = track.is_some_and(|track| track.correction.energy.is_some());
    let ratings = (1..=10_u8).map(|rating| {
        choice_button(
            ("pick-energy", usize::from(rating)),
            rating.to_string(),
            shown == Some(rating),
            &palette,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.accept_energy(rating, cx)))
        .test_support()
    });
    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(4.0))
        .children(ratings)
        .when(is_set, |row| {
            row.child(
                choice_button(
                    "reset-energy",
                    match detected {
                        Some(detected) => format!("Reset to detected ({detected})"),
                        None => "Reset to detected".to_string(),
                    },
                    false,
                    &palette,
                )
                .on_click(cx.listener(|this, _, _, cx| this.reset_energy(cx)))
                .test_support(),
            )
        })
}

/// The section for checking the key by ear, for a track with a key.
pub(crate) fn section(
    track: &Track,
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<Div> {
    let key = track.key()?;
    let palette = app.theme.palette;
    let notation = app.display.key_notation;
    let sounding = app.by_ear.drone_key();

    let tones = std::iter::once(key)
        .chain(shown_runner_up(track))
        .enumerate()
        .map(|(index, tone_key)| {
            let is_on = sounding == Some(tone_key);
            let description: SharedString = if is_on {
                "Stops the tone.".into()
            } else {
                format!(
                    "Plays the root and fifth of {tone_key} softly over the track. If it clashes, the key is likely wrong."
                )
                .into()
            };
            choice_button(
                ("tone", index),
                format!("{} tone", notation.spell(&tone_key)),
                is_on,
                &palette,
            )
            .tooltip(move |window, cx| Tooltip::new(description.clone()).build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_drone(tone_key, cx)))
            .test_support()
        });

    let level_step = app.by_ear.level_step();
    let levels = (1..=LEVEL_STEPS).map(|step| {
        let is_filled = step <= level_step;
        div()
            .id(("reference-level", usize::from(step)))
            .flex()
            .items_end()
            .h(px(16.0))
            .px(px(1.0))
            .cursor_pointer()
            .child(
                div()
                    .w(px(4.0))
                    .h(px(4.0 + 2.5 * f32::from(step)))
                    .rounded(px(1.0))
                    .bg(if is_filled {
                        palette.text_secondary
                    } else {
                        palette.line
                    }),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_reference_level(step, cx)))
            .test_support()
    });
    let volume = div()
        .id("reference-volume")
        .flex()
        .items_center()
        .gap(px(4.0))
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_tertiary)
        .child("Volume")
        .child(div().flex().items_end().children(levels))
        .tooltip(|window, cx| {
            Tooltip::new("How loud the tone and the piano are over the track.").build(window, cx)
        });

    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(section_label("Check by ear", &palette))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(px(6.0))
                    .children(tones)
                    .child(div().flex_1())
                    .child(volume),
            )
            .child(piano(key, &palette, cx))
            .child(
                div()
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_tertiary)
                    .child(format!(
                        "The notes of {} are marked. Play along to hear which fit.",
                        notation.spell(&key)
                    )),
            ),
    )
}

/// One octave of piano from middle C, with the notes of `key`'s scale
/// marked and its root named.
fn piano(key: Key, palette: &Palette, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let scale = scale_of(key);
    let in_scale = |pitch_class: u8| scale.contains(&pitch_class);
    let is_root = |pitch_class: u8| pitch_class == key.tonic();
    // A piano has ivory and ebony keys in every theme; the scale is marked
    // in the theme's accent, light on the white keys and deep on the black.
    let ivory = hsla(0.0, 0.0, 0.95, 1.0);
    let ebony = hsla(0.0, 0.0, 0.14, 1.0);
    let ink = hsla(0.0, 0.0, 0.2, 1.0);
    let light_accent = Hsla {
        l: 0.8,
        ..palette.accent
    };
    let deep_accent = Hsla {
        l: 0.4,
        ..palette.accent
    };
    let pressed = palette.accent;

    let white_keys = WHITE_KEYS.iter().map(|&pitch_class| {
        let background = if in_scale(pitch_class) {
            light_accent
        } else {
            ivory
        };
        div()
            .id(("piano-key", usize::from(pitch_class)))
            .flex()
            .flex_1()
            .flex_col()
            .justify_end()
            .items_center()
            .pb(px(4.0))
            .h_full()
            .border_1()
            .border_color(palette.line)
            .rounded_b(px(4.0))
            .bg(background)
            .cursor_pointer()
            .active(move |style| style.bg(pressed))
            .text_size(px(10.0))
            .text_color(ink)
            .when(is_root(pitch_class), |key| {
                key.font_weight(FontWeight::BOLD).underline()
            })
            .when(in_scale(pitch_class), |key| {
                key.child(NOTE_NAMES[usize::from(pitch_class)])
            })
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.play_piano_note(pitch_class, cx)),
            )
            .test_support()
    });

    let black_keys = BLACK_KEYS.iter().map(|&(pitch_class, after_white)| {
        let left = (after_white + 1) as f32 / WHITE_KEYS.len() as f32 - BLACK_KEY_WIDTH / 2.0;
        let background = if in_scale(pitch_class) {
            deep_accent
        } else {
            ebony
        };
        div()
            .id(("piano-key", usize::from(pitch_class)))
            .absolute()
            .top_0()
            .left(relative(left))
            .w(relative(BLACK_KEY_WIDTH))
            .h(relative(0.6))
            .rounded_b(px(3.0))
            .bg(background)
            .when(is_root(pitch_class), |key| {
                key.border_2().border_color(ivory)
            })
            .cursor_pointer()
            .active(move |style| style.bg(pressed))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    // A black key lies over two white ones, which would
                    // sound too.
                    cx.stop_propagation();
                    this.play_piano_note(pitch_class, cx);
                }),
            )
            .test_support()
    });

    div()
        .relative()
        .flex()
        .w_full()
        .max_w(px(360.0))
        .h(px(PIANO_HEIGHT))
        .children(white_keys)
        .children(black_keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_picker_follows_the_camelot_wheel() {
        let camelot: Vec<String> = (0..24).map(|index| picker_key(index).camelot()).collect();
        assert_eq!(camelot[0], "1A");
        assert_eq!(camelot[7], "8A");
        assert_eq!(camelot[11], "12A");
        assert_eq!(camelot[12], "1B");
        assert_eq!(camelot[23], "12B");
        assert_eq!(picker_key(7), Key::new(9, Mode::Minor));
        assert_eq!(picker_key(19), Key::new(0, Mode::Major));
        let mut distinct = camelot.clone();
        distinct.sort();
        distinct.dedup();
        assert_eq!(distinct.len(), 24);
    }
}
