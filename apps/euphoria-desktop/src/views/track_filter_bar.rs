// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The filter bar under the track list's heading: a tempo range, an energy
//! range, the qualities and the set stages to show. It is out of the way until the Filter
//! button opens it, and stays while a filter is set.

use gpui::component::input::Input;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{Context, Div, FontWeight, Stateful, TestSupportExt, div, px};

use crate::app::EuphoriaApp;
use crate::columns::SIDE_PADDING;
use crate::metronome_controls::TEMPO_RANGE_RULE;
use crate::theme::Palette;
use crate::track_filters::{QualityChoice, STAGE_EXPLANATION, SetStage};
use crate::views::SMALL_TEXT_SIZE;
use crate::views::{GAP_MEDIUM, GAP_SMALL, GAP_TIGHT, RADIUS_LARGE, RADIUS_MEDIUM};

const FIELD_WIDTH: f32 = 52.0;
const CHIP_HEIGHT: f32 = 22.0;

/// The Filter button of the heading, filled while the bar is open.
pub(crate) fn toggle(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> impl IntoElement {
    let palette = app.theme.palette;
    let is_active = app.track_filters.is_active();
    let is_open = is_active || app.is_filter_bar_open;
    let hover_background = palette.raised;
    div()
        .id("filter-button")
        .flex()
        .items_center()
        .h(px(CHIP_HEIGHT))
        .px(px(8.0))
        .rounded(RADIUS_MEDIUM)
        .text_size(SMALL_TEXT_SIZE)
        .cursor_pointer()
        .map(|button| {
            if is_open {
                button
                    .bg(palette.raised)
                    .text_color(palette.text)
                    .font_weight(FontWeight::MEDIUM)
            } else {
                button
                    .text_color(palette.text_secondary)
                    .hover(move |style| style.bg(hover_background))
            }
        })
        .child("Filter")
        .tooltip(|window, cx| {
            Tooltip::new("Filter by tempo, energy, quality and set stage").build(window, cx)
        })
        .on_click(cx.listener(|this, _, _, cx| this.toggle_filter_bar(cx)))
        .test_support()
}

/// The bar, when it is open or a filter is set.
pub(crate) fn render(
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<impl IntoElement> {
    let filters = &app.track_filters;
    if !app.is_filter_bar_open && !filters.is_active() {
        return None;
    }
    let palette = app.theme.palette;
    let inputs = &app.track_filter_inputs;
    let label = |text: &'static str| {
        div()
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_secondary)
            .child(text)
    };
    let range = |from: &gpui::Entity<gpui::component::input::InputState>,
                 to: &gpui::Entity<gpui::component::input::InputState>,
                 id: &'static str| {
        div()
            .flex()
            .items_center()
            .gap(GAP_TIGHT)
            .child(
                div()
                    .w(px(FIELD_WIDTH))
                    .child(Input::new(from).id((id, 0usize))),
            )
            .child(label("\u{2013}"))
            .child(
                div()
                    .w(px(FIELD_WIDTH))
                    .child(Input::new(to).id((id, 1usize))),
            )
    };
    let half_or_double = chip(
        "filter-half-double",
        "\u{bd} / 2\u{d7}",
        filters.half_or_double,
        &palette,
    )
    .tooltip(|window, cx| {
        Tooltip::new("Also show tracks at half or double this tempo range").build(window, cx)
    })
    .on_click(cx.listener(|this, _, _, cx| this.toggle_half_or_double(cx)))
    .test_support();
    let qualities = QualityChoice::ALL
        .into_iter()
        .enumerate()
        .map(|(index, choice)| {
            chip(
                ("filter-quality", index),
                crate::track_filters::chip_label(choice),
                filters.qualities.contains(&choice),
                &palette,
            )
            .tooltip(move |window, cx| Tooltip::new(choice.full_label()).build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_track_quality(choice, cx)))
            .test_support()
        });
    let stages = SetStage::ALL.into_iter().enumerate().map(|(index, stage)| {
        chip(
            ("filter-stage", index),
            stage.label(),
            filters.stages.contains(&stage),
            &palette,
        )
        .tooltip(|window, cx| Tooltip::new(STAGE_EXPLANATION).build(window, cx))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_track_stage(stage, cx)))
        .test_support()
    });
    let clear = div()
        .id("filter-clear")
        .flex()
        .items_center()
        .h(px(CHIP_HEIGHT))
        .px(px(6.0))
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .cursor_pointer()
        .hover(move |style| style.text_color(palette.text))
        .child("Clear")
        .on_click(cx.listener(|this, _, window, cx| this.clear_track_filters(window, cx)))
        .test_support();
    Some(
        div()
            .id("filter-bar")
            .flex()
            .flex_shrink_0()
            .flex_wrap()
            .items_center()
            .gap_x(GAP_MEDIUM)
            .gap_y(GAP_SMALL)
            .px(px(SIDE_PADDING))
            .py(px(6.0))
            .border_b_1()
            .border_color(palette.line)
            .child(
                div()
                    .id("filter-tempo")
                    .flex()
                    .items_center()
                    .gap(GAP_SMALL)
                    .tooltip(|window, cx| Tooltip::new(TEMPO_RANGE_RULE).build(window, cx))
                    .child(label("BPM"))
                    .child(range(&inputs.bpm_from, &inputs.bpm_to, "filter-bpm"))
                    .child(half_or_double),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(GAP_SMALL)
                    .child(label("Energy"))
                    .child(range(
                        &inputs.energy_from,
                        &inputs.energy_to,
                        "filter-energy",
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap(GAP_TIGHT)
                    .child(label("Quality"))
                    .children(qualities),
            )
            .child(
                div()
                    .id("filter-stage")
                    .flex()
                    .items_center()
                    .gap(GAP_TIGHT)
                    .tooltip(|window, cx| Tooltip::new(STAGE_EXPLANATION).build(window, cx))
                    .child(label("Stage"))
                    .children(stages),
            )
            .when(filters.is_active(), |bar| bar.child(clear))
            .test_support(),
    )
}

fn chip(
    id: impl Into<gpui::ElementId>,
    text: &'static str,
    is_chosen: bool,
    palette: &Palette,
) -> Stateful<Div> {
    let hover_background = palette.raised;
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(px(CHIP_HEIGHT))
        .px(px(8.0))
        .rounded(RADIUS_LARGE)
        .border_1()
        .text_size(SMALL_TEXT_SIZE)
        .cursor_pointer()
        .map(|chip| {
            if is_chosen {
                chip.border_color(palette.accent)
                    .bg(palette.accent)
                    .text_color(palette.accent_ink)
                    .font_weight(FontWeight::MEDIUM)
            } else {
                chip.border_color(palette.line)
                    .text_color(palette.text_secondary)
                    .hover(move |style| style.bg(hover_background))
            }
        })
        .child(text)
}
