// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The energy curve, the vocals lane and the key timeline drawn under the waveform, across
//! the same width, so a breakdown, a drop or a key change sits right under
//! the part of the waveform it belongs to, and the playhead runs through
//! all three.

use std::rc::Rc;

use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{
    AnyElement, App, ClickEvent, Div, FontWeight, SharedString, Stateful, TestSupportExt, Window,
    div, px, relative,
};

use crate::camelot::CamelotPosition;
use crate::format;
use crate::stems::StemStatus;
use crate::tag_settings::NotationChoice;
use crate::theme::{AppTheme, Palette};
use crate::track_overview::{self, EnergyCurve, EnergyStep, KeyStretch, TimeRange, VocalLane};
use crate::views::waveform::SeenBounds;

/// Height of the energy curve.
pub(crate) const ENERGY_HEIGHT: f32 = 20.0;

/// Height of the vocals lane.
pub(crate) const VOCALS_HEIGHT: f32 = 12.0;

/// Height of the key timeline.
pub(crate) const KEY_TIMELINE_HEIGHT: f32 = 14.0;

/// A row under the waveform: drawn across the width of the waveform, so that
/// what is under a part of the waveform lines up with it. A lane for the
/// sections of a track joins the others by being added to [`overview_lanes`].
pub(crate) struct Lane {
    pub(crate) element: AnyElement,
    pub(crate) height: f32,
}

/// What a press on the link that starts separating the stems does.
pub(crate) type SeparateHandler = Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>;

/// Where and how wide the lanes are drawn: the part of the track they show,
/// whole or zoomed, and how wide they are. A lane has the width of its
/// container unless `width` is given.
#[derive(Clone)]
pub(crate) struct LaneFrame {
    pub(crate) duration_seconds: f32,
    pub(crate) range: TimeRange,
    pub(crate) width: Option<f32>,
    pub(crate) progress: Option<f32>,
    /// Where the lanes were last drawn, read for the tooltips.
    pub(crate) bounds: SeenBounds,
}

impl LaneFrame {
    /// The lanes over the whole of a track as wide as their container.
    pub(crate) fn whole(duration_seconds: f32, progress: Option<f32>, bounds: SeenBounds) -> Self {
        Self {
            duration_seconds,
            range: TimeRange::whole(duration_seconds),
            width: None,
            progress,
            bounds,
        }
    }

    fn lane_span(&self, start: f32, end: f32) -> Option<(f32, f32)> {
        self.range.lane_span(start, end, self.duration_seconds)
    }

    fn pointer_track_fraction(&self, window: &Window) -> f32 {
        let lane_fraction = pointer_fraction_in(window, &self.bounds);
        self.range
            .track_fraction(lane_fraction, self.duration_seconds)
    }

    fn sized(&self, lane: Stateful<Div>) -> Stateful<Div> {
        match self.width {
            Some(width) => lane.w(px(width)),
            None => lane.w_full(),
        }
    }
}

/// What the lanes under a track's waveform are drawn from.
pub(crate) struct LaneInputs<'a> {
    pub(crate) energy: Option<&'a EnergyCurve>,
    pub(crate) vocals: &'a VocalLane,
    pub(crate) stems: Option<&'a StemStatus>,
    pub(crate) frame: LaneFrame,
    pub(crate) palette: Palette,
    pub(crate) on_separate: SeparateHandler,
}

/// The lanes between the waveform and the key timeline, top to bottom.
pub(crate) fn overview_lanes(inputs: &LaneInputs<'_>) -> Vec<Lane> {
    let mut lanes = Vec::new();
    match inputs.energy {
        Some(EnergyCurve::Steps(steps)) if !steps.is_empty() => lanes.push(Lane {
            element: energy_curve(steps, &inputs.frame, &inputs.palette),
            height: ENERGY_HEIGHT,
        }),
        Some(EnergyCurve::Unavailable(reason)) => lanes.push(Lane {
            element: energy_unavailable(reason, &inputs.palette),
            height: ENERGY_HEIGHT,
        }),
        _ => {}
    }
    if inputs.energy.is_some() {
        lanes.push(Lane {
            element: vocals_lane(inputs),
            height: VOCALS_HEIGHT,
        });
    }
    lanes
}

/// The quiet label at the left of a lane, which says what the lane is.
fn lane_label(
    id: &'static str,
    text: &'static str,
    explanation: &'static str,
    palette: &Palette,
) -> AnyElement {
    div()
        .id(id)
        .flex_shrink_0()
        .px(px(4.0))
        .rounded(px(2.0))
        .bg(palette.panel.opacity(0.7))
        .text_size(px(LANE_LABEL_SIZE))
        .line_height(px(LANE_LABEL_SIZE + 2.0))
        .text_color(palette.text_tertiary)
        .child(text)
        .tooltip(move |window, cx| Tooltip::new(explanation).build(window, cx))
        .test_support()
        .into_any_element()
}

const LANE_LABEL_SIZE: f32 = 9.0;

/// The energy curve: a bar for each step, as tall as its rating out of 10,
/// fainter once played, with gaps where the track is silent. The tooltip
/// says the energy where the pointer is.
pub(crate) fn energy_curve(
    steps: &[EnergyStep],
    frame: &LaneFrame,
    palette: &Palette,
) -> AnyElement {
    let colour = palette.accent;
    let progress = frame.progress;
    let bars = steps.iter().filter_map(|step| {
        let rating = step.rating?;
        let (start, end) = frame.lane_span(step.start, step.end)?;
        let is_played = progress.is_some_and(|progress| step.end <= progress);
        Some(
            div()
                .absolute()
                .bottom_0()
                .left(relative(start))
                .w(relative(end - start))
                .h(relative(f32::from(rating) / 10.0))
                .bg(colour.opacity(if is_played { 0.3 } else { 0.7 })),
        )
    });
    let steps: Vec<EnergyStep> = steps.to_vec();
    let tooltip_frame = frame.clone();
    frame
        .sized(div().id("energy-curve"))
        .relative()
        .h(px(ENERGY_HEIGHT))
        .flex_shrink_0()
        .overflow_hidden()
        .rounded(px(2.0))
        .bg(palette.raised.opacity(0.5))
        .children(bars)
        .child(div().absolute().top_0().left_0().child(lane_label(
            "energy-label",
            "Energy",
            track_overview::ENERGY_EXPLANATION,
            palette,
        )))
        .children(lane_playhead(frame, palette))
        .tooltip(move |window, cx| {
            let fraction = tooltip_frame.pointer_track_fraction(window);
            Tooltip::new(track_overview::energy_reading(
                &steps,
                tooltip_frame.duration_seconds,
                fraction,
            ))
            .build(window, cx)
        })
        .test_support()
        .into_any_element()
}

fn pointer_fraction_in(window: &Window, bounds: &SeenBounds) -> f32 {
    let Some(bounds) = bounds.get() else {
        return 0.0;
    };
    track_overview::pointer_fraction(
        window.mouse_position().x / px(1.0),
        bounds.left() / px(1.0),
        bounds.size.width / px(1.0),
    )
}

/// The curve's row when the core could not rate the audio: a quiet line
/// saying so, with the reason in a tooltip.
pub(crate) fn energy_unavailable(reason: &str, palette: &Palette) -> AnyElement {
    let reason: SharedString = reason.to_string().into();
    div()
        .id("energy-unavailable")
        .flex()
        .items_center()
        .w_full()
        .h(px(ENERGY_HEIGHT))
        .flex_shrink_0()
        .text_size(px(11.0))
        .text_color(palette.text_secondary)
        .child("Energy over time not available")
        .tooltip(move |window, cx| Tooltip::new(reason.clone()).build(window, cx))
        .test_support()
        .into_any_element()
}

/// The vocals lane: filled where the vocals stem is loud, empty where it is
/// not. A track without separated stems has it empty, with a link that
/// starts separating them. The tooltip says where the stretch under the
/// pointer starts and ends.
fn vocals_lane(inputs: &LaneInputs<'_>) -> AnyElement {
    let palette = &inputs.palette;
    let label = lane_label(
        "vocals-label",
        "Vocals",
        track_overview::VOCALS_EXPLANATION,
        palette,
    );
    let frame = &inputs.frame;
    let lane = frame
        .sized(div().id("vocals-lane"))
        .relative()
        .flex()
        .items_center()
        .gap(px(6.0))
        .h(px(VOCALS_HEIGHT))
        .flex_shrink_0()
        .overflow_hidden()
        .rounded(px(2.0))
        .bg(palette.raised.opacity(0.5));
    let label = div().absolute().top_0().left_0().child(label);
    match inputs.vocals {
        VocalLane::Ready(windows) => {
            let stretches = track_overview::vocal_stretches(windows, frame.duration_seconds);
            let progress = frame.progress;
            let fills = stretches.iter().enumerate().filter_map(|(index, stretch)| {
                let (start, end) = frame.lane_span(stretch.start, stretch.end)?;
                let is_played = progress.is_some_and(|progress| stretch.end <= progress);
                Some(
                    div()
                        .id(("vocal-stretch", index))
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(relative(start))
                        .w(relative(end - start))
                        .bg(palette.accent.opacity(if is_played { 0.3 } else { 0.7 }))
                        .test_support(),
                )
            });
            let tooltip_frame = frame.clone();
            lane.children(fills)
                .child(label)
                .children(lane_playhead(frame, palette))
                .tooltip(move |window, cx| {
                    let fraction = tooltip_frame.pointer_track_fraction(window);
                    Tooltip::new(track_overview::vocal_reading(
                        &stretches,
                        tooltip_frame.duration_seconds,
                        fraction,
                    ))
                    .build(window, cx)
                })
                .test_support()
                .into_any_element()
        }
        VocalLane::NoStems => {
            let hover_colour = palette.text;
            let is_separating = matches!(inputs.stems, Some(StemStatus::Separating));
            let prompt = if is_separating {
                div()
                    .id("vocals-separating")
                    .child("Separating the stems")
                    .test_support()
                    .into_any_element()
            } else {
                let on_separate = Rc::clone(&inputs.on_separate);
                div()
                    .id("vocals-separate")
                    .cursor_pointer()
                    .hover(move |style| style.text_color(hover_colour))
                    .child("Separate the stems to see the vocals")
                    .on_click(move |event, window, cx| on_separate(event, window, cx))
                    .test_support()
                    .into_any_element()
            };
            lane.child(label)
                .child(
                    div()
                        .absolute()
                        .left(px(48.0))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .text_size(px(LANE_LABEL_SIZE + 1.0))
                        .text_color(palette.text_secondary)
                        .child(prompt),
                )
                .test_support()
                .into_any_element()
        }
        VocalLane::Unavailable(reason) => {
            let reason: SharedString = reason.clone().into();
            lane.child(label)
                .child(
                    div()
                        .absolute()
                        .left(px(48.0))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .text_size(px(LANE_LABEL_SIZE + 1.0))
                        .text_color(palette.text_secondary)
                        .child("Vocals not available"),
                )
                .tooltip(move |window, cx| Tooltip::new(reason.clone()).build(window, cx))
                .test_support()
                .into_any_element()
        }
        VocalLane::Checking => lane.child(label).test_support().into_any_element(),
    }
}

/// The key timeline: a stretch for each section in its key's colour, with
/// its Camelot code where there is room, and gaps where the track is
/// silent.
pub(crate) fn key_timeline(
    stretches: &[KeyStretch],
    duration_seconds: f32,
    progress: Option<f32>,
    theme: &AppTheme,
    notation: NotationChoice,
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
            notation.spell(&stretch.key),
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
            .child(notation.spell(&stretch.key))
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

/// The playhead of a lane showing part of the track, when playing is in it.
fn lane_playhead(frame: &LaneFrame, palette: &Palette) -> Option<Div> {
    let progress = frame
        .range
        .lane_point(frame.progress?, frame.duration_seconds)?;
    playhead(Some(progress), palette)
}

/// A thin line where playing is, as on the waveform.
fn playhead(progress: Option<f32>, palette: &Palette) -> Option<Div> {
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
