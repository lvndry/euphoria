// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Why a track is under Needs review: a chip per reason in the table, and in
//! the inspector a box that says each reason in plain words with the quickest
//! way to settle it.

use euphoria_core::Key;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, FontWeight, SharedString, TestSupportExt, div, px};

use crate::app::EuphoriaApp;
use crate::library::{ReviewReason, Track};
use crate::tag_settings::NotationChoice;
use crate::theme::Palette;
use crate::views::controls::choice_button;
use crate::views::key_chip::key_chip;
use crate::views::{GAP_SMALL, GAP_TIGHT, RADIUS_LARGE, RADIUS_SMALL};
use crate::views::{LABEL_TEXT_SIZE, SMALL_TEXT_SIZE, section_label};

/// The shortest words for a reason, for a chip in a row.
pub(crate) fn chip_label(reason: &ReviewReason, notation: NotationChoice) -> String {
    match reason {
        ReviewReason::Failed(_) => "Failed".to_string(),
        ReviewReason::KeyUnsure => "Key: relative or 5th?".to_string(),
        ReviewReason::TempoUnsure => "BPM: half or double?".to_string(),
        ReviewReason::KeyConflict { tag_key } => format!("Tag says {}", notation.spell(tag_key)),
    }
}

/// One small chip per reason, in the order of [`Track::review_reasons`].
pub(crate) fn chips(
    track: &Track,
    track_index: usize,
    app: &EuphoriaApp,
    palette: &Palette,
) -> Vec<AnyElement> {
    track
        .review_reasons()
        .iter()
        .enumerate()
        .map(|(position, reason)| {
            let explanation: SharedString = match reason {
                ReviewReason::Failed(message) => format!("Could not be analyzed: {message}").into(),
                ReviewReason::KeyUnsure => {
                    "The key may be its relative or a fifth away. Select the track to compare it with the runner up and settle it."
                        .into()
                }
                ReviewReason::TempoUnsure => {
                    "The BPM may be half or double. Select the track to settle it.".into()
                }
                ReviewReason::KeyConflict { tag_key } => format!(
                    "The file's tags say {tag_key}. Select the track to pick which to keep."
                )
                .into(),
            };
            div()
                .id(("review-chip", track_index * 8 + position))
                .flex_shrink_0()
                .px(px(5.0))
                .py(px(1.0))
                .rounded(RADIUS_SMALL)
                .bg(Palette::tint(palette.warning))
                .text_size(LABEL_TEXT_SIZE)
                .text_color(palette.warning)
                .whitespace_nowrap()
                .child(chip_label(reason, app.tag_settings.key_notation))
                .tooltip(move |window, cx| Tooltip::new(explanation.clone()).build(window, cx))
                .test_support()
                .into_any_element()
        })
        .collect()
}

/// The "Needs review" box of the inspector, or nothing for a track with no
/// reason to review.
pub(crate) fn section(
    track: &Track,
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<Div> {
    let reasons = track.review_reasons();
    if reasons.is_empty() {
        return None;
    }
    let palette = app.theme.palette;
    let notation = app.tag_settings.key_notation;
    let detected_key = track.detected_key();
    let runner_up = crate::views::inspector::shown_runner_up(track);
    let shown_bpm = track.bpm();

    let mut lines = Vec::new();
    for reason in &reasons {
        let line = match reason {
            ReviewReason::Failed(message) => reason_line(
                "review-failed",
                format!("This file could not be analyzed: {message}"),
                vec![
                    choice_button("review-analyze-again", "Analyze again", false, &palette)
                        .on_click(
                            cx.listener(|this, _, _, cx| this.analyze_selected_failed_again(cx)),
                        )
                        .test_support()
                        .into_any_element(),
                ],
                &palette,
            ),
            ReviewReason::KeyUnsure => {
                let mut buttons = Vec::new();
                if let Some(runner_up) = runner_up {
                    buttons.push(key_button(
                        "review-runner-up",
                        "Try",
                        runner_up,
                        app,
                        cx.listener(move |this, _, _, cx| this.accept_key(runner_up, cx)),
                    ));
                }
                buttons.push(
                    choice_button("review-key-looks-right", "Looks right", false, &palette)
                        .on_click(cx.listener(|this, _, _, cx| this.confirm_key(cx)))
                        .test_support()
                        .into_any_element(),
                );
                buttons.push(correct_button(&palette, cx));
                reason_line(
                    "review-key-unsure",
                    "The key may be its relative or a fifth away. Play the key tone or the piano under the key (Check by ear), then try the runner up, pick the right key with Correct, or confirm this one."
                        .to_string(),
                    buttons,
                    &palette,
                )
            }
            ReviewReason::TempoUnsure => {
                let mut buttons = Vec::new();
                if let Some(bpm) = shown_bpm {
                    let half = bpm / 2.0;
                    let double = bpm * 2.0;
                    if half >= 1.0 {
                        buttons.push(
                            choice_button("review-tempo-half", "\u{BD} \u{D7}", false, &palette)
                                .tooltip(tooltip_of(format!(
                                    "Count it at {}",
                                    crate::format::bpm(half)
                                )))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.accept_bpm(half, cx);
                                }))
                                .test_support()
                                .into_any_element(),
                        );
                    }
                    if double <= 999.0 {
                        buttons.push(
                            choice_button("review-tempo-double", "2 \u{D7}", false, &palette)
                                .tooltip(tooltip_of(format!(
                                    "Count it at {}",
                                    crate::format::bpm(double)
                                )))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.accept_bpm(double, cx);
                                }))
                                .test_support()
                                .into_any_element(),
                        );
                    }
                }
                buttons.push(
                    choice_button("review-tempo-looks-right", "Looks right", false, &palette)
                        .on_click(cx.listener(|this, _, _, cx| this.confirm_bpm(cx)))
                        .test_support()
                        .into_any_element(),
                );
                reason_line(
                    "review-tempo-unsure",
                    "The BPM may be half or double. Turn on the metronome in the toolbar: it starts at this track's tempo, so you can hear whether the clicks sit on the beat."
                        .to_string(),
                    buttons,
                    &palette,
                )
            }
            ReviewReason::KeyConflict { tag_key } => {
                let tag_key = *tag_key;
                let tag_code = notation.spell(&tag_key);
                let found = detected_key
                    .map(|key| notation.spell(&key))
                    .unwrap_or_default();
                let mut buttons = vec![key_button(
                    "review-keep-tag-key",
                    "Keep",
                    tag_key,
                    app,
                    cx.listener(move |this, _, _, cx| this.accept_key(tag_key, cx)),
                )];
                if let Some(detected) = detected_key {
                    buttons.push(key_button(
                        "review-use-detected-key",
                        "Use",
                        detected,
                        app,
                        cx.listener(|this, _, _, cx| this.confirm_key(cx)),
                    ));
                }
                reason_line(
                    "review-key-conflict",
                    format!(
                        "Your file's tags say {tag_code}; Euphoria found {found}. Pick the one to trust."
                    ),
                    buttons,
                    &palette,
                )
            }
        };
        lines.push(line);
    }

    Some(
        div().child(
            div()
                .id("review-box")
                .flex()
                .flex_col()
                .gap(GAP_SMALL)
                .p(px(10.0))
                .rounded(RADIUS_LARGE)
                .border_1()
                .border_color(palette.warning.opacity(0.5))
                .child(section_label("Needs review", &palette).text_color(palette.warning))
                .children(lines)
                .test_support(),
        ),
    )
}

fn tooltip_of(
    text: String,
) -> impl Fn(&mut gpui::Window, &mut gpui::App) -> gpui::AnyView + 'static {
    move |window, cx| Tooltip::new(text.clone()).build(window, cx)
}

fn reason_line(
    id: &'static str,
    text: String,
    buttons: Vec<AnyElement>,
    palette: &Palette,
) -> AnyElement {
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(GAP_SMALL)
        .child(
            div()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_secondary)
                .child(text),
        )
        .child(div().flex().flex_wrap().gap(GAP_SMALL).children(buttons))
        .test_support()
        .into_any_element()
}

fn correct_button(palette: &Palette, cx: &mut Context<'_, EuphoriaApp>) -> AnyElement {
    choice_button("review-correct", "Correct\u{2026}", false, palette)
        .on_click(cx.listener(|this, _, window, cx| this.toggle_value_editor(window, cx)))
        .test_support()
        .into_any_element()
}

fn key_button(
    id: &'static str,
    verb: &'static str,
    key: Key,
    app: &EuphoriaApp,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let palette = app.theme.palette;
    let notation = app.tag_settings.key_notation;
    choice_button(id, verb, false, &palette)
        .gap(GAP_TIGHT)
        .font_weight(FontWeight::MEDIUM)
        .child(key_chip(&key, &app.theme, notation))
        .on_click(on_click)
        .test_support()
        .into_any_element()
}
