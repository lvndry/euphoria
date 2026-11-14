// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The stems of the selected track: separating them, and what to hear of
//! the separated ones.

use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{Context, Div, SharedString, TestSupportExt, div, px};

use crate::app::EuphoriaApp;
use crate::library::Track;
use crate::stems::{StemMode, StemStatus};
use crate::views::section_label;
use crate::views::tag_panel::choice_button;

/// The stems section of the track detail: separate the track, wait for it,
/// and choose what to hear of the separated ones.
pub(crate) fn section(track: &Track, app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let status = app.stems.status(&track.path);
    let body = match status {
        Some(StemStatus::Failed(problem)) => div()
            .flex_col()
            .gap(px(8.0))
            .child(div().text_size(crate::views::SMALL_TEXT_SIZE).child(problem.clone()).text_color(palette.text_secondary))
            .child(
                choice_button("stems-again", "Separate again", false, &palette)
                    .on_click(cx.listener(move |this, _, _, cx| this.request_stems(cx)))
                    .test_support(),
            ),
        Some(StemStatus::Separating) => {
            let caption = app
                .stems
                .progress()
                .map(|phase| phase.description())
                .unwrap_or_else(|| "Separating".to_string());
            div()
                .flex_col()
                .gap(px(8.0))
                .child(div().text_size(crate::views::SMALL_TEXT_SIZE).child(caption).text_color(palette.text_secondary))

        }
        Some(StemStatus::Ready(_)) => div()
            .flex_col()
            .gap(px(8.0))
            .child(div()
            .flex()
            .flex_wrap()
            .gap(px(8.0))
            .children(
                StemMode::ALL
                    .iter()
                    .enumerate()
                    .map(|(index, mode)| {
                        choice_button(
                            ("stems-mode", index),
                            SharedString::from(mode.label()),
                            app.stems.mode() == *mode,
                            &palette,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.set_stem_mode(*mode, cx)
                        }))
                        .test_support()

                    }),
            ))
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
            .gap(px(8.0))
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
    div()
        .flex_col()
        .gap(px(8.0))
        .child(section_label("Stems", &palette))
        .child(body)
}
