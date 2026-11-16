// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The stems of the selected track: separating them, and what to hear of
//! the separated ones.

use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{Context, Div, SharedString, TestSupportExt, div};

use crate::app::EuphoriaApp;
use crate::library::Track;
use crate::stem_export::StemExportKind;
use crate::stems::{StemMode, StemStatus};
use crate::views::controls::{choice_button, segment, segmented};
use crate::views::separation_progress::{BarIds, separation_bar};
use crate::views::{CONTROL_HEIGHT, GAP_SMALL, RADIUS_SMALL};
use euphoria_core::stems::{Mixdown, SeparationMode};

/// The stems section of the track detail: separate the track, wait for it,
/// and choose what to hear of the separated ones.
pub(crate) fn section(track: &Track, app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let status = app.stems.status(&track.path);
    let body = match status {
        Some(StemStatus::Failed(problem)) => div()
            .flex_col()
            .gap(GAP_SMALL)
            .child(div().text_size(crate::views::SMALL_TEXT_SIZE).child(problem.clone()).text_color(palette.text_secondary))
            .child(
                choice_button("stems-again", "Separate again", false, &palette)
                    .on_click(cx.listener(move |this, _, _, cx| this.request_stems(cx)))
                    .test_support(),
            ),
        Some(StemStatus::Separating) => {
            let bar = app.separation_shown(&track.path).map(|shown| {
                div()
                    .h(CONTROL_HEIGHT)
                    .rounded(RADIUS_SMALL)
                    .bg(palette.raised.opacity(0.5))
                    .child(separation_bar(
                        BarIds {
                            bar: "stems-separating",
                            fill: "stems-separating-fill",
                        },
                        &shown,
                        GAP_SMALL,
                        &palette,
                    ))
            });
            div().flex_col().gap(GAP_SMALL).children(bar)
        }
        Some(StemStatus::Ready(files)) => div()
            .flex_col()
            .gap(GAP_SMALL)
            .child(div().flex().child(segmented(
                StemMode::ALL
                    .iter()
                    .enumerate()
                    .map(|(index, mode)| {
                        segment(
                            ("stems-mode", index),
                            SharedString::from(mode.label()),
                            app.stems.audible() == mode.stems(),
                            index == 0,
                            &palette,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_stem_mode(*mode, cx)
                        }))
                        .test_support()
                        .into_any_element()
                    })
                    .collect(),
                &palette,
            )))
            .child(export_row(track, app, false, cx))
            .children(
                (app.key_shift != 0 && track.key().is_some())
                    .then(|| export_row(track, app, true, cx)),
            )
            .children(
                (app.stem_settings.mode() == SeparationMode::Advanced
                    && files.mode() == Some(SeparationMode::Default))
                .then(|| {
                    div().flex().child(
                        choice_button(
                            "stems-advanced",
                            "Separate again in Advanced mode",
                            false,
                            &palette,
                        )
                        .tooltip(move |window, cx| {
                            Tooltip::new(
                                "Makes cleaner stems, most of all in the vocals. The stems you have stay until the new ones are ready.",
                            )
                            .build(window, cx)
                        })
                        .on_click(cx.listener(move |this, _, _, cx| this.request_stems(cx)))
                        .test_support(),
                    )
                }),
            )
            .child(
                div().flex().child(
                    choice_button("stems-remove", "Remove stems", false, &palette)
                        .tooltip(move |window, cx| {
                            Tooltip::new(
                                "Deletes the separated stems of this track. It plays from its file and can be separated again.",
                            )
                            .build(window, cx)
                        })
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_stems(cx)))
                        .test_support(),
                ),
            ),
        None => div()
            .flex_col()
            .gap(GAP_SMALL)
            .child(
                choice_button("stems-separate", "Separate the stems", false, &palette)
                    .tooltip(
                        move |window, cx| {
                            Tooltip::new(
                                "Splits the track into vocals, drums, bass and the rest, so that only the vocals, or all but the vocals, can be heard.",
                            )
                            .build(window, cx)
                        },
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.request_stems(cx)))
                    .test_support(),
            ),
    };
    div().flex_col().gap(GAP_SMALL).child(body)
}

/// The buttons that save the separated stems, the acapella or the
/// instrumental of the track, in its own key or in the one it is shifted to.
fn export_row(
    track: &Track,
    app: &EuphoriaApp,
    shifted: bool,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Div {
    let palette = app.theme.palette;
    let suffix = match (shifted, track.key()) {
        (true, Some(key)) => format!(
            " in {}",
            app.tag_settings
                .key_notation
                .spell(&key.transposed(app.key_shift))
        ),
        _ => String::new(),
    };
    let tooltip = if shifted {
        "Shifts every stem with the pitch shifter of the shifted copy, so the file is what you hear, and tags it with the new key."
    } else {
        "Saves the file as it was separated, as 16 bit WAV at 44.1 kHz."
    };
    let choices = [
        (StemExportKind::Stems, "stems", "Export stems"),
        (
            StemExportKind::Mix(Mixdown::Acapella),
            "acapella",
            "Export acapella",
        ),
        (
            StemExportKind::Mix(Mixdown::Instrumental),
            "instrumental",
            "Export instrumental",
        ),
    ];
    div()
        .flex()
        .flex_wrap()
        .gap(GAP_SMALL)
        .children(choices.into_iter().map(|(kind, name, label)| {
            let id = if shifted {
                format!("stems-export-{name}-shifted")
            } else {
                format!("stems-export-{name}")
            };
            choice_button(
                SharedString::from(id),
                SharedString::from(format!("{label}{suffix}…")),
                false,
                &palette,
            )
            .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| this.choose_stem_export(kind, shifted, cx)))
            .test_support()
        }))
}
