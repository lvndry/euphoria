// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Checking the key by ear in the track detail: a drone of the key, and of
//! the runner up when one is offered, a one octave piano with the key's
//! scale marked (all in the key the track is heard in, which "Hear it in
//! another key" may have moved), a volume for both, and the editor that corrects the key, the
//! tempo or the energy by hand.

use euphoria_core::{Key, Mode};
use gpui::component::input::Input;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{
    ClickEvent, Context, Div, FontWeight, Hsla, SharedString, TestSupportExt, div, px, relative,
};

use crate::app::EuphoriaApp;
use crate::key_by_ear::{ARPEGGIO_STEP, LEVEL_STEPS, scale_of};
use crate::library::Track;
use crate::tag_settings::NotationChoice;
use crate::theme::{Palette, blend};
use crate::views::controls::choice_button;
use crate::views::inspector::shown_runner_up;
use crate::views::key_chip::key_chip;
use crate::views::{
    GAP_SMALL, GAP_TIGHT, LABEL_TEXT_SIZE, RADIUS_LARGE, RADIUS_MEDIUM, RADIUS_SMALL,
};
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
        .gap(GAP_SMALL)
        .child(div().flex().child(link))
        .children(editor)
}

fn editor(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let notation = app.tag_settings.key_notation;
    let current = app.selected_track().and_then(Track::key);
    let keys = (0..24).map(|index| {
        let key = picker_key(index);
        let is_current = current == Some(key);
        div()
            .id(("pick-key", index))
            .flex()
            .rounded(RADIUS_MEDIUM)
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
        .gap(GAP_SMALL)
        .p(px(10.0))
        .rounded(RADIUS_LARGE)
        .border_1()
        .border_color(palette.line)
        .child(section_label("Key", &palette))
        .child(div().flex().flex_wrap().gap(GAP_TIGHT).children(keys))
        .child(section_label("BPM", &palette))
        .child(
            div()
                .flex()
                .items_center()
                .gap(GAP_SMALL)
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
        .gap(GAP_TIGHT)
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

/// The key of `track` and its runner up, if one is offered, as heard when
/// the track is moved `shift` semitones: what the tone, the piano and the
/// chords of the section are built on.
pub(crate) fn heard_keys(track: &Track, shift: i8) -> Option<(Key, Option<Key>)> {
    let key = track.key()?.transposed(shift);
    let runner_up = shown_runner_up(track).map(|runner_up| runner_up.transposed(shift));
    Some((key, runner_up))
}

fn tone_label(notation: NotationChoice, key: Key) -> String {
    format!("{} tone", notation.spell(&key))
}

/// A chord button says its key only when there are two to tell apart.
fn chord_label(notation: NotationChoice, key: Key, has_runner_up: bool) -> String {
    if has_runner_up {
        format!("{} chord", notation.spell(&key))
    } else {
        "Play chord".to_string()
    }
}

/// The section for checking the key by ear, for a track with a key.
pub(crate) fn section(
    track: &Track,
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<Div> {
    let (key, runner_up) = heard_keys(track, app.key_shift)?;
    let palette = app.theme.palette;
    let notation = app.tag_settings.key_notation;
    let sounding = app.by_ear.drone_key();

    let tones = std::iter::once(key)
        .chain(runner_up)
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
                tone_label(notation, tone_key),
                is_on,
                &palette,
            )
            .tooltip(move |window, cx| Tooltip::new(description.clone()).build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_drone(tone_key, cx)))
            .test_support()
        });

    let has_runner_up = runner_up.is_some();
    let chords = std::iter::once(key)
        .chain(runner_up)
        .enumerate()
        .map(|(index, chord_key)| {
            let label = chord_label(notation, chord_key, has_runner_up);
            let description = format!(
                "Plays the {} triad of {chord_key}, its root, third and fifth, together on the piano. Shift-click plays it as an arpeggio, one note after another about {} ms apart.",
                match chord_key.mode() {
                    Mode::Major => "major",
                    Mode::Minor => "minor",
                },
                ARPEGGIO_STEP.as_millis()
            );
            choice_button(("chord", index), label, false, &palette)
                .tooltip(move |window, cx| Tooltip::new(description.clone()).build(window, cx))
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    this.play_chord(chord_key, event.modifiers().shift, cx)
                }))
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
                    .rounded(RADIUS_SMALL)
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
        .gap(GAP_TIGHT)
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
            .gap(GAP_SMALL)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(GAP_SMALL)
                    .children(tones)
                    .children(chords)
                    .child(div().flex_1())
                    .child(volume),
            )
            .child(piano(key, app, &palette, cx))
            .child(
                div()
                    .id("piano-legend")
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_tertiary)
                    .child(format!(
                        "Marked: notes of {}. Lit: playing.",
                        notation.spell(&key)
                    ))
                    .test_support(),
            ),
    )
}

/// The ivory of the white keys, the same in every theme.
const IVORY: Hsla = Hsla {
    h: 0.0,
    s: 0.0,
    l: 0.95,
    a: 1.0,
};

/// The ebony of the black keys, the same in every theme.
const EBONY: Hsla = Hsla {
    h: 0.0,
    s: 0.0,
    l: 0.14,
    a: 1.0,
};

/// The note names on the white keys.
const INK: Hsla = Hsla {
    h: 0.0,
    s: 0.0,
    l: 0.2,
    a: 1.0,
};

/// How much accent tints a white key of the scale.
const WHITE_SCALE_TINT: f32 = 0.25;

/// How much accent tints a black key of the scale, which stays dark so it
/// never reads as sounding.
const BLACK_SCALE_TINT: f32 = 0.3;

/// The least lightness of the bar on a black key of the scale, so a dark
/// accent still shows on ebony.
const MARK_LIGHTNESS: f32 = 0.7;

/// How a piano key is painted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct KeyLook {
    /// The key itself.
    pub(crate) fill: Hsla,
    /// The bar near the bottom of a black key of the scale.
    pub(crate) mark: Option<Hsla>,
    /// The note name on a white key.
    pub(crate) ink: Hsla,
}

/// True for the pitch classes of the black keys.
fn is_black(pitch_class: u8) -> bool {
    BLACK_KEYS
        .iter()
        .any(|&(black_pitch_class, _)| black_pitch_class == pitch_class)
}

/// The colour of a key whose note is sounding, white or black.
pub(crate) fn sounding_fill(palette: &Palette) -> Hsla {
    palette.accent
}

/// How the key `pitch_class` is painted on the piano of `key`, `glow` of
/// the way from resting (0) to sounding (1). A key at rest is ivory or
/// ebony, a key of the scale keeps its ivory or ebony with a touch of
/// accent (a tint on white, a tint and a bar on black), and a sounding key
/// is filled with the full accent.
pub(crate) fn key_look(pitch_class: u8, key: Key, glow: f32, palette: &Palette) -> KeyLook {
    let in_scale = scale_of(key).contains(&pitch_class);
    let sounding = sounding_fill(palette);
    let glow = glow.clamp(0.0, 1.0);
    let (resting, mark) = match (is_black(pitch_class), in_scale) {
        (false, false) => (IVORY, None),
        (false, true) => (blend(palette.accent, IVORY, WHITE_SCALE_TINT), None),
        (true, false) => (EBONY, None),
        (true, true) => (
            blend(palette.accent, EBONY, BLACK_SCALE_TINT),
            Some(Hsla {
                l: palette.accent.l.max(MARK_LIGHTNESS),
                ..palette.accent
            }),
        ),
    };
    KeyLook {
        fill: if glow >= 1.0 {
            sounding
        } else {
            blend(sounding, resting, glow)
        },
        mark: mark.map(|mark| blend(sounding, mark, glow)),
        ink: if glow < 0.5 { INK } else { palette.accent_ink },
    }
}

/// How each of the twelve keys of the piano of `key` is painted now, by
/// pitch class from C.
pub(crate) fn piano_looks(key: Key, app: &EuphoriaApp) -> [KeyLook; 12] {
    let palette = app.theme.palette;
    std::array::from_fn(|pitch_class| {
        let pitch_class = pitch_class as u8;
        key_look(pitch_class, key, app.by_ear.glow(pitch_class), &palette)
    })
}

/// One octave of piano from middle C, with the notes of `key`'s scale
/// marked and its root named.
fn piano(key: Key, app: &EuphoriaApp, palette: &Palette, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let scale = scale_of(key);
    let in_scale = |pitch_class: u8| scale.contains(&pitch_class);
    let is_root = |pitch_class: u8| pitch_class == key.tonic();
    let pressed = sounding_fill(palette);
    let looks = piano_looks(key, app);

    let white_keys = WHITE_KEYS.iter().map(|&pitch_class| {
        let look = looks[usize::from(pitch_class)];
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
            .rounded_b(RADIUS_SMALL)
            .bg(look.fill)
            .cursor_pointer()
            .active(move |style| style.bg(pressed))
            .text_size(LABEL_TEXT_SIZE)
            .text_color(look.ink)
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
        let look = looks[usize::from(pitch_class)];
        div()
            .id(("piano-key", usize::from(pitch_class)))
            .absolute()
            .top_0()
            .left(relative(left))
            .w(relative(BLACK_KEY_WIDTH))
            .h(relative(0.6))
            .rounded_b(RADIUS_SMALL)
            .bg(look.fill)
            .when(is_root(pitch_class), |key| {
                key.border_2().border_color(IVORY)
            })
            .when_some(look.mark, |key, mark| {
                key.child(
                    div()
                        .absolute()
                        .bottom(px(5.0))
                        .left(relative(0.2))
                        .w(relative(0.6))
                        .h(px(3.0))
                        .rounded_full()
                        .bg(mark),
                )
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
    use crate::theme::tests::contrast;
    use crate::theme::{AppTheme, ThemeName};

    #[test]
    fn the_three_states_of_a_key_stand_apart_in_every_theme() {
        let d_minor = Key::new(2, Mode::Minor);
        let b_flat = 10;
        let c_sharp = 1;
        let d_natural = 2;
        let e_flat = 3;
        for theme in ThemeName::ALL {
            let palette = AppTheme::named(theme).palette;
            let sounding = sounding_fill(&palette);
            let marked_black = key_look(b_flat, d_minor, 0.0, &palette);
            let plain_black = key_look(c_sharp, d_minor, 0.0, &palette);
            let marked_white = key_look(d_natural, d_minor, 0.0, &palette);
            assert_eq!(key_look(e_flat, d_minor, 0.0, &palette).fill, EBONY);
            assert!(
                contrast(marked_black.fill, sounding) >= 1.5,
                "{theme:?}: a marked black key is far darker than a sounding one"
            );
            assert!(
                contrast(marked_white.fill, sounding) >= 1.5,
                "{theme:?}: a marked white key is far lighter than a sounding one"
            );
            assert_ne!(marked_black.fill, plain_black.fill, "{theme:?}");
            let mark = marked_black.mark.unwrap();
            assert!(
                contrast(mark, marked_black.fill) >= 3.0,
                "{theme:?}: the bar shows on the black key"
            );
            assert!(plain_black.mark.is_none(), "{theme:?}");
            assert!(
                contrast(INK, marked_white.fill) >= 4.5,
                "{theme:?}: the note name reads on a marked key"
            );

            let lit_black = key_look(b_flat, d_minor, 1.0, &palette);
            let lit_white = key_look(d_natural, d_minor, 1.0, &palette);
            assert_eq!(lit_black.fill, sounding, "{theme:?}");
            assert_eq!(lit_white.fill, sounding, "{theme:?}");
            assert!(
                contrast(lit_white.ink, sounding) >= 4.5,
                "{theme:?}: the note name reads on a sounding key"
            );
            let halfway = key_look(b_flat, d_minor, 0.5, &palette);
            assert_ne!(halfway.fill, marked_black.fill, "{theme:?}: it fades");
            assert_ne!(halfway.fill, sounding, "{theme:?}: it fades");
        }
    }

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

    #[test]
    fn the_labels_name_the_key_the_track_is_heard_in() {
        let seven_a = Key::new(2, Mode::Minor);
        let heard = seven_a.transposed(2);
        assert_eq!(heard.camelot(), "9A");
        assert_eq!(
            chord_label(NotationChoice::Camelot, heard, true),
            "9A chord"
        );
        assert_eq!(tone_label(NotationChoice::Camelot, heard), "9A tone");
        assert_eq!(
            chord_label(NotationChoice::Camelot, heard, false),
            "Play chord"
        );
        let seven_b = Key::new(3, Mode::Major);
        assert_eq!(seven_b.camelot(), "5B");
        assert_eq!(seven_b.transposed(7).camelot(), "6B");
        assert_eq!(seven_b.transposed(0), seven_b);
    }
}
