// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The filter bar under the track list's heading: a tempo range, an energy
//! range and the qualities to show. It is out of the way until the Filter
//! button opens it, and stays while a filter is set.

use gpui::component::input::Input;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{Context, Div, FontWeight, Stateful, TestSupportExt, div, px};

use crate::app::EuphoriaApp;
use crate::columns::SIDE_PADDING;
use crate::metronome_controls::TEMPO_RANGE_RULE;
use crate::theme::Palette;
use crate::track_filters::QualityChoice;
use crate::views::SMALL_TEXT_SIZE;

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
        .rounded(px(5.0))
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
        .tooltip(|window, cx| Tooltip::new("Filter by tempo, energy and quality").build(window, cx))
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
            .gap(px(4.0))
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
                choice.chip_label(),
                filters.qualities.contains(&choice),
                &palette,
            )
            .tooltip(move |window, cx| Tooltip::new(choice.full_label()).build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_track_quality(choice, cx)))
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
            .gap_x(px(12.0))
            .gap_y(px(6.0))
            .px(px(SIDE_PADDING))
            .py(px(6.0))
            .border_b_1()
            .border_color(palette.line)
            .child(
                div()
                    .id("filter-tempo")
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .tooltip(|window, cx| Tooltip::new(TEMPO_RANGE_RULE).build(window, cx))
                    .child(label("BPM"))
                    .child(range(&inputs.bpm_from, &inputs.bpm_to, "filter-bpm"))
                    .child(half_or_double),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
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
                    .gap(px(4.0))
                    .child(label("Quality"))
                    .children(qualities),
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
        .rounded(px(11.0))
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
