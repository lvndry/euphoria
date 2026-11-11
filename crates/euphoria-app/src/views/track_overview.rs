// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The energy curve and the key timeline drawn under the waveform, across
//! the same width, so a breakdown, a drop or a key change sits right under
//! the part of the waveform it belongs to, and the playhead runs through
//! all three.

use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{AnyElement, FontWeight, SharedString, TestSupportExt, div, px, relative};

use crate::camelot::CamelotPosition;
use crate::format;
use crate::theme::{AppTheme, Palette};
use crate::track_overview::{EnergyStep, KeyStretch};

/// Height of the energy curve.
pub(crate) const ENERGY_HEIGHT: f32 = 20.0;

/// Height of the key timeline.
pub(crate) const KEY_TIMELINE_HEIGHT: f32 = 14.0;

/// The energy curve: a bar for each step, as tall as its rating out of 10,
/// fainter once played, with gaps where the track is silent.
pub(crate) fn energy_curve(
    steps: &[EnergyStep],
    progress: Option<f32>,
    palette: &Palette,
) -> AnyElement {
    let colour = palette.accent;
    let bars = steps.iter().filter_map(|step| {
        let rating = step.rating?;
        let is_played = progress.is_some_and(|progress| step.end <= progress);
        Some(
            div()
                .absolute()
                .bottom_0()
                .left(relative(step.start))
                .w(relative(step.end - step.start))
                .h(relative(f32::from(rating) / 10.0))
                .bg(colour.opacity(if is_played { 0.3 } else { 0.7 })),
        )
    });
    div()
        .id("energy-curve")
        .relative()
        .w_full()
        .h(px(ENERGY_HEIGHT))
        .flex_shrink_0()
        .overflow_hidden()
        .rounded(px(2.0))
        .bg(palette.raised.opacity(0.5))
        .children(bars)
        .children(playhead(progress, palette))
        .tooltip(|window, cx| {
            Tooltip::new("Energy over time, from 1 to 10, every 4 seconds").build(window, cx)
        })
        .test_support()
        .into_any_element()
}

/// The key timeline: a stretch for each section in its key's colour, with
/// its Camelot code where there is room, and gaps where the track is
/// silent.
pub(crate) fn key_timeline(
    stretches: &[KeyStretch],
    duration_seconds: f32,
    progress: Option<f32>,
    theme: &AppTheme,
) -> AnyElement {
    let palette = &theme.palette;
    let sections = stretches.iter().enumerate().map(|(index, stretch)| {
        let colours = theme.key_colours(CamelotPosition::of(&stretch.key));
        let (background, foreground) = match colours.background {
            Some(background) => (background, colours.foreground),
            None => (colours.foreground.opacity(0.28), colours.foreground),
        };
        let description: SharedString = format!(
            "{}, {}, {} to {}",
            stretch.key.camelot(),
            stretch.key,
            format::duration(stretch.start * duration_seconds),
            format::duration(stretch.end * duration_seconds)
        )
        .into();
        div()
            .id(("key-stretch", index))
            .absolute()
            .top_0()
            .bottom_0()
            .left(relative(stretch.start))
            .w(relative(stretch.end - stretch.start))
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .rounded(px(2.0))
            .border_l_1()
            .border_color(palette.panel)
            .bg(background)
            .text_color(foreground)
            .text_size(px(10.0))
            .font_weight(FontWeight::SEMIBOLD)
            .child(stretch.key.camelot())
            .tooltip(move |window, cx| Tooltip::new(description.clone()).build(window, cx))
            .test_support()
    });
    div()
        .id("key-timeline")
        .relative()
        .w_full()
        .h(px(KEY_TIMELINE_HEIGHT))
        .flex_shrink_0()
        .children(sections)
        .children(playhead(progress, palette))
        .test_support()
        .into_any_element()
}

/// A thin line where playing is, as on the waveform.
fn playhead(progress: Option<f32>, palette: &Palette) -> Option<gpui::Div> {
    let progress = progress?;
    Some(
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(relative(progress.clamp(0.0, 1.0)))
            .w(px(1.0))
            .bg(palette.text),
    )
}
