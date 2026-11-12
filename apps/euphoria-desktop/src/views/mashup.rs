// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mashup mode of the decks: the key shift and stem levels under each deck,
//! and the tracks that layer over deck A's, with why, arrows to step through
//! them, and the export.

use euphoria_core::harmony::MashupReason;
use euphoria_core::stems::Stem;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, FontWeight, SharedString, TestSupportExt, div, px};

use crate::app::EuphoriaApp;
use crate::deck_audio::DeckId;
use crate::decks::DeckMode;
use crate::format;
use crate::mashup::{INSTRUMENTAL_ONLY, VOCALS_ONLY};
use crate::views::decks::{deck_part, fader, scope_buttons};
use crate::views::key_chip::key_chip;
use crate::views::tag_panel::choice_button;
use crate::views::{SMALL_TEXT_SIZE, secondary_button, section_label};

/// Height of the list of ideas.
const IDEAS_HEIGHT: f32 = 190.0;

fn deck_number(deck: DeckId) -> usize {
    match deck {
        DeckId::A => 0,
        DeckId::B => 1,
    }
}

/// The two modes of the list under the decks.
pub(crate) fn mode_buttons(
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Vec<AnyElement> {
    let palette = app.theme.palette;
    [
        (DeckMode::NextTrack, "Mix", "Tracks to play after deck A"),
        (DeckMode::Mashup, "Mashup", "Tracks to layer over deck A"),
    ]
    .into_iter()
    .map(|(mode, label, tip)| {
        choice_button(
            ("deck-mode", mode as usize),
            label,
            app.decks.mode == mode,
            &palette,
        )
        .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
        .on_click(cx.listener(move |this, _, _, cx| this.set_deck_mode(mode, cx)))
        .test_support()
        .into_any_element()
    })
    .collect()
}

/// The key shift of a deck and, when its track has stems, a level for each.
pub(crate) fn deck_controls(
    app: &EuphoriaApp,
    deck: DeckId,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Div {
    let palette = app.theme.palette;
    let view = app.decks.view(deck);
    let shift = view.shift_semitones;
    let shifted_key = app
        .session
        .library()
        .track(view.track_index.unwrap_or(usize::MAX))
        .and_then(|track| track.key())
        .filter(|_| shift != 0)
        .map(|key| key.transposed(shift));
    let label = |text: &'static str| {
        div()
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_tertiary)
            .child(text)
    };
    let mut preset_row = None;
    if view.stem_files.is_some() {
        let use_stems = view.use_stems;
        let levels = view.stem_levels;
        let mut buttons = Vec::new();
        for (position, (name, preset)) in [
            ("Full", None),
            ("Vocals", Some(VOCALS_ONLY)),
            ("Instrumental", Some(INSTRUMENTAL_ONLY)),
        ]
        .into_iter()
        .enumerate()
        {
            let chosen = match preset {
                None => !use_stems,
                Some(preset) => use_stems && levels == preset,
            };
            buttons.push(
                choice_button(
                    ("deck-stem-preset", deck_number(deck) * 10 + position),
                    name,
                    chosen,
                    &palette,
                )
                .on_click(cx.listener(move |this, _, _, cx| this.set_deck_stems(deck, preset, cx)))
                .test_support()
                .into_any_element(),
            );
        }
        preset_row = Some(div().flex().gap(px(4.0)).children(buttons));
    }
    let shift_row = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(label("Key"))
        .child(
            secondary_button("\u{2212}", &palette)
                .id(deck_part("deck-shift-down", deck))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let semitones = this.decks.view(deck).shift_semitones - 1;
                    this.set_deck_shift(deck, semitones, cx);
                }))
                .test_support(),
        )
        .child(
            div()
                .id(deck_part("deck-shift", deck))
                .min_w(px(28.0))
                .text_size(SMALL_TEXT_SIZE)
                .font_weight(FontWeight::MEDIUM)
                .child(if shift == 0 {
                    "0".to_string()
                } else {
                    format!("{shift:+}")
                })
                .test_support(),
        )
        .child(
            secondary_button("+", &palette)
                .id(deck_part("deck-shift-up", deck))
                .on_click(cx.listener(move |this, _, _, cx| {
                    let semitones = this.decks.view(deck).shift_semitones + 1;
                    this.set_deck_shift(deck, semitones, cx);
                }))
                .test_support(),
        )
        .children(shifted_key.map(|key| key_chip(&key, &app.theme, app.display.key_notation)))
        .child(div().flex_1())
        .children(preset_row);

    let stems = if view.stem_files.is_some() && view.use_stems {
        let faders = Stem::ALL.iter().enumerate().map(|(slot, stem)| {
            let stem = *stem;
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(label(stem.label()))
                .child(fader(
                    ("deck-stem-level", deck_number(deck) * 10 + slot),
                    view.stem_levels[slot],
                    &app.decks.stem_fader_bounds[deck_number(deck)][slot],
                    &palette,
                    cx,
                    move |this, level, cx| this.set_deck_stem_level(deck, stem, level, cx),
                ))
        });
        Some(
            div()
                .flex()
                .gap(px(8.0))
                .children(faders.collect::<Vec<_>>()),
        )
    } else if view.track_index.is_some()
        && view.stem_files.is_none()
        && app.decks.mode == DeckMode::Mashup
    {
        Some(
            div()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child("No stems yet: separate this track in the track detail to set the level of its vocals and instruments."),
        )
    } else {
        None
    };
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(shift_row)
        .children(stems)
}

/// The tracks that layer over deck A's; a click puts one on deck B.
pub(crate) fn ideas(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let decks = &app.decks;
    let library = app.session.library();
    let notation = app.display.key_notation;
    let count = decks.ideas.len();
    let position = decks
        .idea_index
        .map(|position| format!("{} of {count}", position + 1))
        .unwrap_or_else(|| format!("{count} ideas"));
    let modes = mode_buttons(app, cx);
    let scopes = scope_buttons(app, cx);
    let heading = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.0))
        .children(modes)
        .child(section_label("Layer over deck A", &palette))
        .child(
            secondary_button("\u{2039}", &palette)
                .id("mashup-previous")
                .on_click(cx.listener(|this, _, _, cx| this.step_mashup_idea(-1, cx)))
                .test_support(),
        )
        .child(
            div()
                .id("mashup-position")
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_secondary)
                .child(position)
                .test_support(),
        )
        .child(
            secondary_button("\u{203a}", &palette)
                .id("mashup-next")
                .on_click(cx.listener(|this, _, _, cx| this.step_mashup_idea(1, cx)))
                .test_support(),
        )
        .child(
            secondary_button("Play both", &palette)
                .id("mashup-play")
                .tooltip(|window, cx| {
                    Tooltip::new("Start both decks, deck B's downbeats on deck A's.")
                        .build(window, cx)
                })
                .on_click(cx.listener(|this, _, _, cx| this.play_mashup(cx)))
                .test_support(),
        )
        .child(
            secondary_button("Export mashup\u{2026}", &palette)
                .id("mashup-export")
                .tooltip(|window, cx| {
                    Tooltip::new(
                        "Render both decks to a WAV file, from the downbeat each is on, with their shifts, stems and faders. The tracks are not changed.",
                    )
                    .build(window, cx)
                })
                .on_click(cx.listener(|this, _, _, cx| this.export_mashup(cx)))
                .test_support(),
        )
        .child(div().flex_1())
        .children(scopes);

    let hover_background = palette.raised;
    let chosen_background = palette.raised;
    let rows = decks
        .ideas
        .iter()
        .enumerate()
        .filter_map(|(position, idea)| {
            let track = library.track(idea.track_index)?;
            let describe =
                |reason: &MashupReason| reason.describe(|key| notation.spell(key).to_string());
            let mut reasons = idea.reasons.iter();
            let key_reason: SharedString = reasons.next().map(describe).unwrap_or_default().into();
            let other_reasons: SharedString = reasons
                .map(describe)
                .collect::<Vec<_>>()
                .join(" \u{b7} ")
                .into();
            let shift = idea.key.shift_semitones;
            let is_on_deck = decks.idea_index == Some(position);
            Some(
                div()
                    .id(("mashup-idea", idea.track_index))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .py(px(4.0))
                    .px(px(6.0))
                    .rounded(px(4.0))
                    .cursor_pointer()
                    .when(is_on_deck, |row| row.bg(chosen_background))
                    .hover(move |style| style.bg(hover_background))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .child(div().w(px(38.0)).flex_shrink_0().children(
                                track.key().map(|key| key_chip(&key, &app.theme, notation)),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(track.title().to_string()),
                            )
                            .child(
                                div()
                                    .w(px(48.0))
                                    .flex_shrink_0()
                                    .text_size(SMALL_TEXT_SIZE)
                                    .text_color(if shift == 0 {
                                        palette.text_tertiary
                                    } else {
                                        palette.accent
                                    })
                                    .child(if shift == 0 {
                                        "no shift".to_string()
                                    } else {
                                        format!("{shift:+} st")
                                    }),
                            )
                            .child(
                                div()
                                    .w(px(56.0))
                                    .flex_shrink_0()
                                    .text_color(palette.text_secondary)
                                    .child(format::signed_percent(idea.tempo.difference_percent)),
                            ),
                    )
                    .child(
                        div()
                            .pl(px(48.0))
                            .text_size(SMALL_TEXT_SIZE)
                            .text_color(palette.text_secondary)
                            .child(key_reason),
                    )
                    .child(
                        div()
                            .pl(px(48.0))
                            .text_size(SMALL_TEXT_SIZE)
                            .text_color(palette.text_tertiary)
                            .child(other_reasons),
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.load_mashup_idea(position, cx)),
                    )
                    .test_support(),
            )
        });
    let rows: Vec<_> = rows.collect();
    let empty = if decks.view(DeckId::A).track_index.is_none() {
        Some("Load a track on deck A to see what layers over it.".to_string())
    } else if decks.ideas.is_empty() && decks.stem_library.is_scanning() {
        Some("Looking for separated tracks\u{2026}".to_string())
    } else if decks.ideas.is_empty() {
        Some(
            "No track here layers over deck A: none is within two semitones of a fitting key and 8% of its tempo."
                .to_string(),
        )
    } else {
        None
    };
    div().flex().flex_col().gap(px(4.0)).child(heading).child(
        div()
            .id("mashup-ideas")
            .flex()
            .flex_col()
            .h(px(IDEAS_HEIGHT))
            .overflow_y_scroll()
            .children(rows)
            .children(empty.map(|text| {
                div()
                    .text_color(palette.text_tertiary)
                    .text_size(SMALL_TEXT_SIZE)
                    .child(text)
            })),
    )
}
