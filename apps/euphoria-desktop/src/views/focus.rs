// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The focus view: the selected track across the whole window, as lanes on
//! one time axis with one playhead and one cursor, the tracks it mixes into
//! below on the left, and the plan for the transition on the right.

use std::rc::Rc;
use std::sync::Arc;

use euphoria_core::rekordbox_grid::{GridSource, choose_grid};
use euphoria_core::stems::Stem;
use gpui::component::Icon;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, ClickEvent, Context, Div, FontWeight, MouseButton, MouseDownEvent,
    MouseMoveEvent, Pixels, ScrollEvent, SharedString, TestSupportExt, Window, canvas, div, px,
    relative,
};

use crate::app::EuphoriaApp;
use crate::camelot::CamelotPosition;
use crate::focus::{
    CueSections, Lane, PlanTab, SectionBand, SectionKind, SectionSource, StemsLane, Viewport, clock,
};
use crate::format;
use crate::library::Track;
use crate::mixes::keys_related_by;
use crate::peaks::BandPeaks;
use crate::stems::{StemFocus, StemStatus};
use crate::theme::Palette;
use crate::track_overview::{TimeRange, VocalLane};
use crate::transition_plan::OPEN_PLANNER_LINK;
use crate::views::controls::choice_button;
use crate::views::cues;
use crate::views::favorites::star_button;
use crate::views::key_chip::key_chip;
use crate::views::track_overview::{LaneFrame, LaneInputs, VOCALS_HEIGHT, overview_lanes};
use crate::views::waveform::{WaveformColours, waveform};
use crate::views::wheel::{emphasis_for, wheel};
use crate::views::{
    GAP_LARGE, GAP_MEDIUM, GAP_SMALL, GAP_TIGHT, RADIUS_LARGE, RADIUS_MEDIUM, RADIUS_SMALL,
    TITLE_TEXT_SIZE,
};
use crate::views::{LABEL_TEXT_SIZE, SMALL_TEXT_SIZE, TEXT_SIZE, secondary_button, section_label};
use crate::waveforms::Waveform;
use euphoria_core::harmony::words::{
    LAYERS_LABEL, OVERLAP_LABEL, TEMPO_LABEL, TempoMove, relation_hint, relation_name,
};

/// Width of the column of lane names.
const GUTTER: f32 = 84.0;

/// Space between the names and the lanes.
const LANE_GAP: f32 = 10.0;

const OVERVIEW_HEIGHT: f32 = 40.0;
const SECTIONS_HEIGHT: f32 = 22.0;
const DETAIL_HEIGHT: f32 = 150.0;
const SLOT_HEIGHT: f32 = 28.0;
const STEM_ROW_HEIGHT: f32 = 14.0;
const STEM_ROW_GAP: f32 = 2.0;
const BOTTOM_HEIGHT: f32 = 320.0;

const CLASH_COLUMN_WIDTH: f32 = 150.0;

/// The height of a row in the list of tracks this one mixes into.
pub(crate) const MIX_ROW_HEIGHT: f32 = 32.0;
const MIX_KEY_WIDTH: f32 = 52.0;
const MIX_TEMPO_WIDTH: f32 = 52.0;
const MIX_TEMPO_CHIP_WIDTH: f32 = 76.0;
const MIX_RELATION_WIDTH: f32 = 132.0;

/// The lanes of the stems row, top to bottom.
const STEM_ROWS: [Stem; 4] = [Stem::Vocals, Stem::Drums, Stem::Bass, Stem::Other];

/// What the matches list says for a mix the user marked, whose keys do not
/// mix by the rules.
const YOUR_TRANSITION: &str = "Your mix";

/// The focus view of the selected track.
pub(crate) fn render(
    app: &EuphoriaApp,
    window: &Window,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    if app.planner.is_open {
        return crate::views::planner::render(app, window, cx);
    }
    let palette = app.theme.palette;
    let page = div()
        .id("focus-view")
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .gap(GAP_MEDIUM)
        .p(px(18.0))
        .bg(palette.background)
        .overflow_y_scroll();
    let Some(track) = app.selected_track() else {
        return page.test_support().into_any_element();
    };
    page.child(top_bar(app, cx))
        .child(header(track, app, cx))
        .child(timeline(track, app, cx))
        .child(bottom(track, app, cx))
        .test_support()
        .into_any_element()
}

fn top_bar(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let hover_background = palette.line;
    let back = div()
        .id("focus-back")
        .flex()
        .items_center()
        .gap(GAP_SMALL)
        .h(px(28.0))
        .px(px(10.0))
        .rounded(RADIUS_MEDIUM)
        .border_1()
        .border_color(palette.line)
        .bg(palette.raised)
        .cursor_pointer()
        .hover(move |style| style.bg(hover_background))
        .tooltip(|window, cx| Tooltip::new("Back to the library (Esc)").build(window, cx))
        .child(
            Icon::default()
                .path("icons/arrow-left.svg")
                .size(px(13.0))
                .text_color(palette.text_secondary),
        )
        .child("Library")
        .on_click(cx.listener(|this, _, _, cx| this.close_focus_view(cx)))
        .test_support();
    let zoom_button = |id: &'static str, label: &'static str, factor: f64| {
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .size(px(28.0))
            .rounded(RADIUS_MEDIUM)
            .border_1()
            .border_color(palette.line)
            .bg(palette.raised)
            .cursor_pointer()
            .hover(move |style| style.bg(hover_background))
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.zoom_focus(factor, 0.5, cx)))
            .test_support()
    };
    let lanes_button = div()
        .id("focus-lanes")
        .flex()
        .items_center()
        .h(px(28.0))
        .px(px(10.0))
        .rounded(RADIUS_MEDIUM)
        .border_1()
        .border_color(palette.line)
        .bg(if app.focus.lanes_menu_open {
            palette.line
        } else {
            palette.raised
        })
        .cursor_pointer()
        .hover(move |style| style.bg(hover_background))
        .child(LAYERS_LABEL)
        .on_click(cx.listener(|this, _, _, cx| this.toggle_focus_lanes_menu(cx)))
        .test_support();
    let menu = app.focus.lanes_menu_open.then(|| {
        div()
            .id("focus-lanes-menu")
            .absolute()
            .top(px(32.0))
            .right_0()
            .flex()
            .flex_col()
            .w(px(170.0))
            .p(px(4.0))
            .rounded(RADIUS_LARGE)
            .border_1()
            .border_color(palette.line)
            .bg(palette.panel)
            .shadow_lg()
            .occlude()
            .children(Lane::ALL.map(|lane| {
                let is_shown = app.focus.lanes.is_shown(lane);
                div()
                    .id(("focus-lane-toggle", lane.number()))
                    .flex()
                    .items_center()
                    .gap(GAP_SMALL)
                    .h(px(26.0))
                    .px(px(8.0))
                    .rounded(RADIUS_SMALL)
                    .cursor_pointer()
                    .hover(move |style| style.bg(palette.raised))
                    .child(
                        div()
                            .w(px(12.0))
                            .text_color(palette.accent)
                            .child(if is_shown { "\u{2713}" } else { "" }),
                    )
                    .child(
                        div()
                            .text_color(if is_shown {
                                palette.text
                            } else {
                                palette.text_tertiary
                            })
                            .child(lane.label()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_focus_lane(lane, cx)))
                    .test_support()
            }))
    });
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(GAP_SMALL)
        .child(back)
        .child(div().flex_1())
        .child(zoom_button("focus-zoom-out", "\u{2212}", 2.0))
        .child(zoom_button("focus-zoom-in", "+", 0.5))
        .child(div().relative().child(lanes_button).children(menu))
}

fn chip(label: &'static str, value: impl IntoElement, palette: &Palette) -> Div {
    div()
        .flex()
        .items_center()
        .gap(GAP_SMALL)
        .h(px(30.0))
        .px(px(10.0))
        .rounded(RADIUS_MEDIUM)
        .border_1()
        .border_color(palette.line)
        .bg(palette.panel)
        .child(
            div()
                .text_size(LABEL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child(label),
        )
        .child(value)
}

/// What the Grid chip says, or nothing while the detected grid is fine: the
/// chip appears only for a track without a grid or one whose tempo was set.
fn grid_chip_text(track: &Track) -> Option<&'static str> {
    if track.beat_grid().is_none() && track.correction.bpm.is_none() {
        return Some("none");
    }
    match choose_grid(&track.correction, None).source() {
        GridSource::User => Some("edited"),
        GridSource::Detected | GridSource::Rekordbox => None,
    }
}

fn header(track: &Track, app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let theme = &app.theme;
    let palette = theme.palette;
    let notation = app.tag_settings.key_notation;
    let dash = || "\u{2013}".to_string();
    let mut facts: Vec<String> = Vec::new();
    if let Some(seconds) = track.duration_seconds() {
        facts.push(format::duration(seconds));
    }
    facts.push(track.format.clone());
    if let Some(report) = track.quality() {
        facts.push(report.verdict.label().to_string());
    }
    let key = match track.key() {
        Some(key) => div()
            .flex()
            .items_center()
            .gap(GAP_SMALL)
            .child(key_chip(&key, theme, notation))
            .child(
                div()
                    .text_color(palette.text_secondary)
                    .child(key.to_string()),
            )
            .into_any_element(),
        None => div().child(dash()).into_any_element(),
    };
    let grid = grid_chip_text(track);
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(GAP_LARGE)
        .flex_wrap()
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(260.0))
                .gap(GAP_TIGHT)
                .child(
                    div()
                        .id("focus-title")
                        .truncate()
                        .text_size(TITLE_TEXT_SIZE)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(track.title().to_string()),
                )
                .child(
                    div()
                        .truncate()
                        .text_color(palette.text_secondary)
                        .child(track.artist.clone().unwrap_or_else(|| track.folder.clone())),
                )
                .child(
                    div()
                        .text_size(SMALL_TEXT_SIZE)
                        .text_color(palette.text_tertiary)
                        .child(facts.join(" \u{b7} ")),
                ),
        )
        .when(app.theme.layout == crate::theme::Layout::DeckBelow, |row| {
            row.child(crate::views::player::play_button(app, cx))
        })
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(GAP_SMALL)
                .child(
                    chip("Key", key, &palette)
                        .id("focus-chip-key")
                        .test_support(),
                )
                .child(
                    chip(
                        "BPM",
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(track.bpm().map_or_else(dash, format::bpm)),
                        &palette,
                    )
                    .id("focus-chip-bpm")
                    .test_support(),
                )
                .child(
                    chip(
                        "Energy",
                        div().font_weight(FontWeight::SEMIBOLD).child(
                            track
                                .energy()
                                .map_or_else(dash, |rating| rating.to_string()),
                        ),
                        &palette,
                    )
                    .id("focus-chip-energy")
                    .test_support(),
                )
                .children(grid.map(|grid| {
                    chip(
                        "Grid",
                        div().text_color(palette.text_secondary).child(grid),
                        &palette,
                    )
                    .id("focus-chip-grid")
                    .test_support()
                })),
        )
        .children(header_wheel(track, app))
}

/// The side of the Camelot wheel in the header.
const HEADER_WHEEL_SIZE: f32 = 76.0;

/// The Camelot wheel of the track's key, with the keys the suggestion filter
/// moves to tinted.
fn header_wheel(track: &Track, app: &EuphoriaApp) -> Option<AnyElement> {
    let key = track.key()?;
    let relations = app.decks.filter.mix_options().relations;
    Some(wheel(
        key.camelot(),
        emphasis_for(
            CamelotPosition::of(&key),
            keys_related_by(&key, &relations)
                .iter()
                .map(CamelotPosition::of),
        ),
        &app.theme.palette,
        HEADER_WHEEL_SIZE,
    ))
}

/// A lane: its name in the gutter and its content on the shared axis.
fn lane_row(label: impl IntoElement, height: f32, content: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .items_start()
        .gap(px(LANE_GAP))
        .child(
            div()
                .flex_shrink_0()
                .w(px(GUTTER))
                .h(px(height))
                .child(label),
        )
        .child(
            div()
                .relative()
                .flex_1()
                .min_w_0()
                .h(px(height))
                .child(content),
        )
}

fn lane_name(lane: Lane, palette: &Palette) -> Div {
    div()
        .flex()
        .items_center()
        .h_full()
        .text_size(LABEL_TEXT_SIZE)
        .text_color(palette.text_tertiary)
        .child(lane.label())
}

pub(crate) fn section_colour(kind: SectionKind, palette: &Palette) -> (gpui::Hsla, gpui::Hsla) {
    match kind {
        SectionKind::Intro | SectionKind::Outro | SectionKind::Section => {
            (palette.waveform.opacity(0.55), palette.text)
        }
        SectionKind::Main => (palette.accent.opacity(0.45), palette.text),
        SectionKind::Breakdown => (palette.text_secondary.opacity(0.28), palette.text),
        SectionKind::Drop => (palette.accent.opacity(0.9), palette.accent_ink),
    }
}

/// The part of `band` inside `viewport`, as fractions of the view.
fn visible_part(band: &SectionBand, viewport: &Viewport) -> Option<(f32, f32)> {
    let start = viewport.fraction_of(band.start_seconds).max(0.0);
    let end = viewport.fraction_of(band.end_seconds).min(1.0);
    (end > start).then_some((start, end))
}

fn timeline(track: &Track, app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> AnyElement {
    let palette = app.theme.palette;
    let lanes = app.focus.lanes;
    let viewport = app.focus.viewport;
    let duration = f64::from(track.duration_seconds().unwrap_or(0.0));
    let shown_cues = cues::shown_cues(track, app);
    let sections = CueSections.sections(track, &shown_cues, duration);
    let columns = match app.waveform_of(&track.path) {
        Some(Waveform::Ready(columns, _)) => Some(columns.clone()),
        _ => None,
    };
    let progress = app.player.progress();
    let is_selected_playing = app.player.track() == Some(track.path.as_path());
    let position = f64::from(app.player.position_seconds());

    let overview = lanes.overview.then(|| {
        let outline = (duration > 0.0).then(|| {
            div()
                .id("focus-window-outline")
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative((viewport.start / duration) as f32))
                .w(relative((viewport.span / duration) as f32))
                .border_1()
                .border_color(palette.accent)
                .bg(palette.accent.opacity(0.14))
                .test_support()
        });
        let content = div()
            .id("focus-overview")
            .relative()
            .size_full()
            .rounded(RADIUS_SMALL)
            .bg(palette.raised.opacity(0.5))
            .cursor_pointer()
            .child(waveform(
                columns.clone(),
                WaveformColours::of(&palette),
                progress.filter(|_| is_selected_playing),
                Vec::new(),
                app.focus.overview_bounds.clone(),
                px(OVERVIEW_HEIGHT),
            ))
            .children(outline)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.start_moving_window(event.position.x, cx);
                }),
            )
            .test_support();
        lane_row(
            lane_name(Lane::Overview, &palette),
            OVERVIEW_HEIGHT,
            content,
        )
    });

    let section_lane = lanes.sections.then(|| {
        let bands = sections.iter().enumerate().filter_map(|(index, band)| {
            let (start, end) = visible_part(band, &viewport)?;
            let (background, foreground) = section_colour(band.kind, &palette);
            Some(
                div()
                    .id(("focus-section", index))
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(start))
                    .w(relative(end - start))
                    .flex()
                    .items_center()
                    .px(px(6.0))
                    .overflow_hidden()
                    .border_l_1()
                    .border_color(palette.background)
                    .bg(background)
                    .text_color(foreground)
                    .text_size(LABEL_TEXT_SIZE)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(div().truncate().child(band.kind.label()))
                    .test_support(),
            )
        });
        let content = div()
            .id("focus-sections")
            .relative()
            .size_full()
            .rounded(RADIUS_SMALL)
            .overflow_hidden()
            .bg(palette.raised.opacity(0.4))
            .children(bands)
            .test_support();
        lane_row(
            lane_name(Lane::Sections, &palette),
            SECTIONS_HEIGHT,
            content,
        )
    });

    let detail = lanes.detail.then(|| {
        let tints = sections.iter().enumerate().filter_map(|(index, band)| {
            let (start, end) = visible_part(band, &viewport)?;
            let (colour, _) = section_colour(band.kind, &palette);
            Some(
                div()
                    .id(("focus-tint", index))
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(start))
                    .w(relative(end - start))
                    .bg(colour.opacity(0.18)),
            )
        });
        let visible = visible_columns(columns.as_ref(), &viewport, duration);
        let visible_cues: Vec<(usize, euphoria_core::Cue)> = shown_cues
            .iter()
            .cloned()
            .enumerate()
            .filter(|(_, cue)| viewport.contains(cue.seconds))
            .collect();
        let flags = (!visible_cues.is_empty()).then(|| {
            let indexes: Vec<usize> = visible_cues.iter().map(|(index, _)| *index).collect();
            let cue_list: Vec<euphoria_core::Cue> =
                visible_cues.iter().map(|(_, cue)| cue.clone()).collect();
            let fractions: Vec<f32> = cue_list
                .iter()
                .map(|cue| viewport.fraction_of(cue.seconds).clamp(0.0, 1.0))
                .collect();
            let width = app
                .focus
                .axis_bounds
                .get()
                .map(|bounds| bounds.size.width / px(1.0));
            let places = cues::marker_places(&cue_list, &fractions, width);
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .child(cues::markers_lane(
                    &cue_list, &indexes, &fractions, &places, &palette, cx,
                ))
        });
        let content = div()
            .id("focus-detail")
            .relative()
            .size_full()
            .rounded(RADIUS_SMALL)
            .overflow_hidden()
            .bg(palette.raised.opacity(0.4))
            .children(tints)
            .child(waveform(
                visible,
                WaveformColours::of(&palette),
                None,
                Vec::new(),
                app.focus.axis_bounds.clone(),
                px(DETAIL_HEIGHT),
            ))
            .children(flags)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    if event.click_count == 2 {
                        this.add_cue_at_pointer(event.position.x, event.modifiers, cx);
                    } else {
                        this.seek_in_detail(event.position.x, cx);
                    }
                }),
            )
            .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                this.scroll_focus(event, cx);
            }))
            .test_support();
        lane_row(lane_name(Lane::Detail, &palette), DETAIL_HEIGHT, content)
    });

    let separated_path = track.path.clone();
    let frame = LaneFrame {
        duration_seconds: duration as f32,
        range: TimeRange {
            start_seconds: viewport.start as f32,
            end_seconds: viewport.end() as f32,
        },
        width: None,
        progress: None,
        bounds: app.focus.axis_bounds.clone(),
    };
    let mut data_lanes = overview_lanes(&LaneInputs {
        energy: match app.waveform_of(&track.path) {
            Some(Waveform::Ready(_, energy)) => energy.as_ref(),
            _ => None,
        },
        vocals: app.vocal_lane_of(&track.path),
        stems: app.stems.status(&track.path),
        frame,
        palette,
        on_separate: Rc::new(cx.listener(move |this, _: &ClickEvent, _, cx| {
            this.request_stems_of(separated_path.clone(), cx)
        })),
    });
    let vocals_data = data_lanes
        .iter()
        .position(|lane| lane.height == VOCALS_HEIGHT)
        .map(|position| data_lanes.remove(position));
    let energy_data = data_lanes.pop();
    let energy = lanes.energy.then(|| match energy_data {
        Some(data) => lane_row(div(), data.height, data.element),
        None => energy_slot(&palette),
    });
    let stems_ready = matches!(app.focus.stems_of(&track.path), Some(StemsLane::Ready(_)));
    let vocals_measured = matches!(app.vocal_lane_of(&track.path), VocalLane::Ready(_));
    let has_stem_data = vocals_data.is_some() || stems_ready;
    let wants_stem_lanes = lanes.vocals || lanes.stems;
    let stems_card = (wants_stem_lanes && !has_stem_data).then(|| stems_empty_card(track, app, cx));
    let vocals = (lanes.vocals && has_stem_data && !stems_ready).then(|| match vocals_data {
        Some(_) if vocals_measured => reading_stems_slot(&palette),
        Some(data) => lane_row(div(), data.height, data.element),
        None => vocals_slot(&palette),
    });
    let shown_stems: Vec<Stem> = STEM_ROWS
        .into_iter()
        .filter(|stem| lanes.stems || (lanes.vocals && *stem == Stem::Vocals))
        .collect();
    let stems = (has_stem_data && (stems_ready || lanes.stems))
        .then(|| stems_lane(track, app, &viewport, duration, &shown_stems, cx));
    let stem_focus_control =
        (stems_ready && !shown_stems.is_empty()).then(|| stem_focus_control(app, &palette, cx));

    let axis_probe = {
        let seen = app.focus.axis_bounds.clone();
        canvas(
            move |bounds: Bounds<Pixels>, _, _| seen.set(Some(bounds)),
            |_, _, _, _| (),
        )
        .absolute()
        .top_0()
        .bottom_0()
        .left(px(GUTTER + LANE_GAP))
        .right_0()
    };
    let playhead = (is_selected_playing || app.player.progress().is_some())
        .then_some(position)
        .filter(|seconds| viewport.contains(*seconds))
        .map(|seconds| {
            div()
                .id("focus-playhead")
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative(viewport.fraction_of(seconds)))
                .w(px(1.5))
                .bg(palette.text)
                .test_support()
        });
    let cursor = app.focus.hover_seconds.map(|seconds| {
        div()
            .id("focus-cursor")
            .absolute()
            .top_0()
            .bottom_0()
            .left(relative(viewport.fraction_of(seconds)))
            .w(px(1.0))
            .bg(palette.accent)
            .test_support()
    });
    let overlay = div()
        .absolute()
        .top_0()
        .bottom_0()
        .left(px(GUTTER + LANE_GAP))
        .right_0()
        .children(playhead)
        .children(cursor);

    let windowed = div()
        .id("focus-lanes-stack")
        .relative()
        .flex()
        .flex_col()
        .gap(GAP_SMALL)
        .children(section_lane)
        .children(detail)
        .children(energy)
        .children(vocals)
        .children(stems)
        .children(stem_focus_control)
        .children(stems_card)
        .child(axis_probe)
        .child(overlay)
        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
            if event.pressed_button.is_none() {
                this.hover_focus(Some(event.position.x), cx);
            }
        }))
        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            if !*hovered {
                this.hover_focus(None, cx);
            }
        }))
        .test_support();

    let readout = app.focus_readout();
    let readout_line =
        div()
            .id("focus-readout")
            .flex()
            .items_center()
            .h(px(20.0))
            .pl(px(GUTTER + LANE_GAP))
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_secondary)
            .child(readout.unwrap_or_else(|| {
                format!("{} to {}", clock(viewport.start), clock(viewport.end()))
            }))
            .test_support();

    div()
        .id("focus-timeline")
        .flex()
        .flex_shrink_0()
        .flex_col()
        .gap(GAP_SMALL)
        .p(px(12.0))
        .rounded(RADIUS_LARGE)
        .border_1()
        .border_color(palette.line)
        .bg(palette.panel)
        .children(overview)
        .child(windowed)
        .child(readout_line)
        .test_support()
        .into_any_element()
}

/// The columns of the waveform that fall in `viewport`.
fn visible_columns(
    columns: Option<&Arc<[BandPeaks]>>,
    viewport: &Viewport,
    duration: f64,
) -> Option<Arc<[BandPeaks]>> {
    let columns = columns?;
    if duration <= 0.0 || columns.is_empty() {
        return None;
    }
    let count = columns.len() as f64;
    let first = (viewport.start / duration * count).floor() as usize;
    let last = ((viewport.end() / duration * count).ceil() as usize).min(columns.len());
    (last > first).then(|| Arc::from(&columns[first.min(last - 1)..last]))
}

/// What the Energy lane shows before the track has an energy curve.
fn energy_slot(palette: &Palette) -> Div {
    slot(
        Lane::Energy,
        "focus-energy-slot",
        "No energy curve yet",
        palette,
    )
}

/// What the Vocals lane shows while the stems are still being measured.
fn reading_stems_slot(palette: &Palette) -> Div {
    slot(
        Lane::Vocals,
        "focus-vocals-reading",
        "Reading the stems",
        palette,
    )
}

/// What the Vocals lane shows before the track has an energy curve.
fn vocals_slot(palette: &Palette) -> Div {
    slot(
        Lane::Vocals,
        "focus-vocals-slot",
        "No vocals lane yet",
        palette,
    )
}

fn slot(lane: Lane, id: &'static str, text: &'static str, palette: &Palette) -> Div {
    let content = div()
        .id(id)
        .flex()
        .items_center()
        .size_full()
        .px(px(8.0))
        .rounded(RADIUS_SMALL)
        .border_1()
        .border_color(palette.line)
        .text_size(LABEL_TEXT_SIZE)
        .text_color(palette.text_tertiary)
        .child(text)
        .test_support();
    lane_row(lane_name(lane, palette), SLOT_HEIGHT, content)
}

/// The one card the vocals and stems lanes give way to until the track's
/// stems are separated.
fn stems_empty_card(
    track: &Track,
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let palette = app.theme.palette;
    let status = app.stems.status(&track.path);
    let is_separating = matches!(status, Some(StemStatus::Separating));
    let (headline, body) = if is_separating {
        (
            "Separating the stems".to_string(),
            app.stems
                .progress()
                .map_or_else(|| "Working on it.".to_string(), |phase| phase.description()),
        )
    } else {
        (
            "Stems not separated yet".to_string(),
            "Splits the track into vocals, drums, bass and the rest. Takes about a minute."
                .to_string(),
        )
    };
    div()
        .id("focus-stems-empty")
        .flex()
        .items_center()
        .justify_between()
        .gap(GAP_LARGE)
        .px(GAP_LARGE)
        .py(GAP_MEDIUM)
        .rounded(RADIUS_MEDIUM)
        .border_1()
        .border_color(palette.line)
        .bg(palette.raised.opacity(0.5))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(GAP_TIGHT)
                .child(
                    div()
                        .text_size(TEXT_SIZE)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.text)
                        .child(headline),
                )
                .child(
                    div()
                        .text_size(SMALL_TEXT_SIZE)
                        .text_color(palette.text_secondary)
                        .child(body),
                ),
        )
        .when(!is_separating, |card| {
            card.child(
                secondary_button("Separate", &palette)
                    .id(if app.focus.lanes.stems {
                        "focus-stems-separate"
                    } else {
                        "focus-vocals-separate"
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.request_stems(cx)))
                    .test_support(),
            )
        })
        .test_support()
        .into_any_element()
}

fn stems_lane(
    track: &Track,
    app: &EuphoriaApp,
    viewport: &Viewport,
    duration: f64,
    shown: &[Stem],
    cx: &mut Context<'_, EuphoriaApp>,
) -> Div {
    let palette = app.theme.palette;
    let is_ready = matches!(app.focus.stems_of(&track.path), Some(StemsLane::Ready(_)));
    let shown: &[Stem] = if is_ready { shown } else { &STEM_ROWS };
    let focus = app.stems.focus();
    let height =
        shown.len() as f32 * STEM_ROW_HEIGHT + shown.len().saturating_sub(1) as f32 * STEM_ROW_GAP;
    let names = div()
        .flex()
        .flex_col()
        .gap(px(STEM_ROW_GAP))
        .children(shown.iter().map(|stem| {
            let stem = *stem;
            let name = div()
                .id(("focus-stem-label", stem as usize))
                .h(px(STEM_ROW_HEIGHT))
                .flex()
                .items_center()
                .text_size(LABEL_TEXT_SIZE)
                .line_height(px(STEM_ROW_HEIGHT))
                .text_color(stem_label_colour(focus, stem, &palette))
                .when(focus.is_some_and(|focus| !focus.is_audible(stem)), |name| {
                    name.line_through()
                })
                .child(stem.label())
                .test_support();
            if !is_ready {
                return name;
            }
            let hover_colour = palette.text;
            name.cursor_pointer()
                .hover(move |style| style.text_color(hover_colour))
                .tooltip(move |window, cx| {
                    Tooltip::new(format!(
                        "Solo {} (\u{2325}-click to mute it instead)",
                        stem.label()
                    ))
                    .build(window, cx)
                })
                .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                    this.click_stem_lane(stem, event.modifiers().alt, cx)
                }))
        }));
    let body = match app.focus.stems_of(&track.path) {
        Some(StemsLane::Ready(activity)) => {
            let rows = shown.iter().map(|stem| {
                let stem = *stem;
                let is_heard = focus.is_none_or(|focus| focus.is_audible(stem));
                let windows = activity.windows_of(stem);
                let runs = runs_of(windows);
                let bars = runs
                    .into_iter()
                    .enumerate()
                    .filter_map(|(run, (first, length))| {
                        let start_seconds = first as f64
                            * f64::from(euphoria_core::stems::StemActivity::WINDOW_SECONDS);
                        let end_seconds = start_seconds
                            + length as f64
                                * f64::from(euphoria_core::stems::StemActivity::WINDOW_SECONDS);
                        let start = viewport.fraction_of(start_seconds).max(0.0);
                        let end = viewport.fraction_of(end_seconds.min(duration)).min(1.0);
                        (end > start).then(|| {
                            div()
                                .id(("focus-stem-run", stem as usize * 1000 + run))
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .left(relative(start))
                                .w(relative(end - start))
                                .rounded(RADIUS_SMALL)
                                .bg(palette.accent.opacity(if is_heard { 0.7 } else { 0.2 }))
                                .test_support()
                        })
                    });
                div()
                    .id(("focus-stem-row", stem as usize))
                    .relative()
                    .h(px(STEM_ROW_HEIGHT))
                    .rounded(RADIUS_SMALL)
                    .bg(palette.raised.opacity(0.5))
                    .children(bars)
                    .test_support()
            });
            div()
                .id("focus-stems")
                .flex()
                .flex_col()
                .gap(px(STEM_ROW_GAP))
                .size_full()
                .children(rows)
                .test_support()
        }
        _ => {
            let status = app.stems.status(&track.path);
            let content: AnyElement = match status {
                Some(StemStatus::Separating) => div()
                    .text_color(palette.text_tertiary)
                    .child(
                        app.stems
                            .progress()
                            .map_or_else(|| "Separating".to_string(), |phase| phase.description()),
                    )
                    .into_any_element(),
                _ => {
                    let hover_colour = palette.text;
                    div()
                        .id("focus-stems-separate")
                        .cursor_pointer()
                        .text_color(palette.text_tertiary)
                        .hover(move |style| style.text_color(hover_colour))
                        .child("Separate the stems")
                        .tooltip(|window, cx| {
                            Tooltip::new(
                                "Splits the track into vocals, drums, bass and the rest, to show where each plays.",
                            )
                            .build(window, cx)
                        })
                        .on_click(cx.listener(|this, _, _, cx| this.request_stems(cx)))
                        .test_support()
                        .into_any_element()
                }
            };
            div()
                .id("focus-stems")
                .flex()
                .items_center()
                .size_full()
                .text_size(LABEL_TEXT_SIZE)
                .child(content)
                .test_support()
        }
    };
    lane_row(names, height, body)
}

fn stem_label_colour(focus: Option<StemFocus>, stem: Stem, palette: &Palette) -> gpui::Hsla {
    match focus {
        Some(StemFocus::Solo(soloed)) if soloed == stem => palette.accent,
        Some(StemFocus::Solo(_)) => palette.text_tertiary.opacity(0.55),
        Some(StemFocus::Mute(muted)) if muted == stem => palette.text_tertiary.opacity(0.55),
        _ => palette.text_tertiary,
    }
}

/// What is heard of the stems, with the way back to the full track.
fn stem_focus_control(
    app: &EuphoriaApp,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let focus = app.stems.focus();
    let hover_colour = palette.text;
    let summary = match focus {
        Some(focus) => focus.summary(),
        None => "Click a stem to hear it alone".to_string(),
    };
    let full_track = div()
        .id("focus-stem-full-track")
        .child("Full track")
        .when(focus.is_some(), |button| {
            button
                .cursor_pointer()
                .text_color(palette.text_secondary)
                .hover(move |style| style.text_color(hover_colour))
                .on_click(cx.listener(|this, _, _, cx| this.set_stem_focus(None, cx)))
        })
        .when(focus.is_none(), |button| {
            button.text_color(palette.text_tertiary)
        })
        .test_support();
    div()
        .id("focus-stem-control")
        .flex()
        .items_center()
        .gap(GAP_SMALL)
        .h(px(16.0))
        .pl(px(GUTTER + LANE_GAP))
        .text_size(LABEL_TEXT_SIZE)
        .text_color(if focus.is_some() {
            palette.accent
        } else {
            palette.text_tertiary
        })
        .child(summary)
        .child(div().text_color(palette.text_tertiary).child("\u{b7}"))
        .child(full_track)
        .test_support()
        .into_any_element()
}

/// The stretches of `windows` that are true, as (first window, length).
pub(crate) fn runs_of(windows: &[bool]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut start: Option<usize> = None;
    for (index, active) in windows.iter().enumerate() {
        match (active, start) {
            (true, None) => start = Some(index),
            (false, Some(first)) => {
                runs.push((first, index - first));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(first) = start {
        runs.push((first, windows.len() - first));
    }
    runs
}

fn panel(palette: &Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .h(px(BOTTOM_HEIGHT))
        .gap(GAP_SMALL)
        .p(px(14.0))
        .rounded(RADIUS_LARGE)
        .border_1()
        .border_color(palette.line)
        .bg(palette.panel)
}

fn bottom(track: &Track, app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .gap(GAP_MEDIUM)
        .child(mixes_into(app, cx).flex_1())
        .child(plan(track, app, cx).w(px(380.0)).flex_shrink_0())
}

fn tempo_chip(
    id: impl Into<gpui::ElementId>,
    tempo_move: TempoMove,
    palette: &Palette,
) -> impl IntoElement {
    let colour = crate::views::planner::tone_colour(tempo_move.tone(), palette);
    div()
        .id(id)
        .flex()
        .items_center()
        .px(px(6.0))
        .py(px(1.0))
        .rounded(RADIUS_SMALL)
        .bg(colour.opacity(0.16))
        .text_size(LABEL_TEXT_SIZE)
        .font_weight(FontWeight::MEDIUM)
        .text_color(colour)
        .child(tempo_move.label())
        .test_support()
}

fn column_label(text: &'static str, width: Option<f32>, palette: &Palette) -> Div {
    let label = div()
        .text_size(LABEL_TEXT_SIZE)
        .text_color(palette.text_tertiary)
        .child(text);
    match width {
        Some(width) => label.w(px(width)).flex_shrink_0(),
        None => label.flex_1().min_w_0(),
    }
}

fn mixes_into(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let theme = &app.theme;
    let palette = theme.palette;
    let selected_path = app.selected_track().map(|track| track.path.clone());
    let rows: Vec<AnyElement> = app
        .mixes
        .iter()
        .filter_map(|mix| {
            let track_index = mix.track_index;
            let track = app.session.library().track(track_index)?;
            let note = selected_path
                .as_ref()
                .and_then(|selected| app.session.favorites().transition(selected, &track.path))
                .and_then(|transition| transition.note.clone());
            let artist: SharedString = note
                .clone()
                .or_else(|| track.artist.clone())
                .map(|text| format!("\u{b7} {text}"))
                .unwrap_or_default()
                .into();
            let tempo_move = mix.tempo_difference_percent.map(TempoMove::of);
            let is_picked = app.focus.selected_match == Some(track_index);
            let star = star_button(
                ("focus-mix-star", track_index),
                mix.is_favorite,
                if mix.is_favorite {
                    "Take the mark off this transition"
                } else {
                    "Mark this transition as a favorite"
                },
                &palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_transition_from_selected(track_index, cx);
                cx.stop_propagation();
            }))
            .test_support();
            let relation_hint = mix.relation.and_then(relation_hint);
            Some(
                div()
                    .id(("focus-mix", track_index))
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(GAP_MEDIUM)
                    .h(px(MIX_ROW_HEIGHT))
                    .px(px(6.0))
                    .rounded(RADIUS_MEDIUM)
                    .cursor_pointer()
                    .map(|row| {
                        if is_picked {
                            row.bg(palette.selection)
                        } else {
                            row.hover(move |style| style.bg(palette.raised))
                        }
                    })
                    .child(star)
                    .child(
                        div().flex().flex_shrink_0().w(px(MIX_KEY_WIDTH)).children(
                            track
                                .key()
                                .map(|key| key_chip(&key, theme, app.tag_settings.key_notation)),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(GAP_SMALL)
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .child(div().truncate().child(track.title().to_string()))
                            .child(
                                div()
                                    .truncate()
                                    .min_w_0()
                                    .text_size(LABEL_TEXT_SIZE)
                                    .text_color(palette.text_tertiary)
                                    .child(artist),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .justify_end()
                            .w(px(MIX_TEMPO_WIDTH))
                            .text_color(palette.text_secondary)
                            .children(mix.tempo_difference_percent.map(format::signed_percent)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .w(px(MIX_TEMPO_CHIP_WIDTH))
                            .children(tempo_move.map(|tempo_move| {
                                tempo_chip(("focus-mix-tempo", track_index), tempo_move, &palette)
                            })),
                    )
                    .child(
                        div()
                            .id(("focus-mix-relation", track_index))
                            .flex_shrink_0()
                            .w(px(MIX_RELATION_WIDTH))
                            .truncate()
                            .text_size(LABEL_TEXT_SIZE)
                            .text_color(palette.text_tertiary)
                            .child(mix.relation.map_or(YOUR_TRANSITION, relation_name))
                            .when_some(relation_hint, |cell, text| {
                                cell.tooltip(move |window, cx| Tooltip::new(text).build(window, cx))
                            })
                            .test_support(),
                    )
                    .child(clash_cell(app, track_index, &palette))
                    .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                        this.pick_match(track_index, cx);
                        if event.click_count() >= 2 {
                            this.select_and_reveal(track_index, cx);
                        }
                        cx.notify();
                    }))
                    .test_support()
                    .into_any_element(),
            )
        })
        .collect();
    let empty = rows.is_empty().then(|| {
        div()
            .id("focus-mixes-empty")
            .text_color(palette.text_tertiary)
            .child("No track in the library mixes with this one yet.")
            .test_support()
    });
    panel(&palette)
        .child(
            div()
                .flex()
                .items_center()
                .gap(GAP_MEDIUM)
                .child(section_label("Mixes into", &palette))
                .child(
                    div()
                        .text_size(LABEL_TEXT_SIZE)
                        .text_color(palette.text_tertiary)
                        .child(format!("{} tracks, starred transitions first", rows.len())),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(GAP_MEDIUM)
                .px(px(6.0))
                .child(div().w(px(20.0)).flex_shrink_0())
                .child(column_label("Key", Some(MIX_KEY_WIDTH), &palette))
                .child(column_label("Track", None, &palette))
                .child(column_label(
                    TEMPO_LABEL,
                    Some(MIX_TEMPO_WIDTH + MIX_TEMPO_CHIP_WIDTH + 10.0),
                    &palette,
                ))
                .child(column_label("Relation", Some(MIX_RELATION_WIDTH), &palette))
                .child(column_label(
                    OVERLAP_LABEL,
                    Some(CLASH_COLUMN_WIDTH),
                    &palette,
                )),
        )
        .child(
            div()
                .id("focus-mixes")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(&app.focus.mixes_scroll)
                .on_scroll(cx.listener(|_, _: &ScrollEvent, _, cx| {
                    cx.spawn(async move |this, cx| {
                        this.update(cx, |this, cx| this.request_visible_plans(cx))
                            .ok();
                    })
                    .detach();
                }))
                .children(empty)
                .children(rows)
                .test_support(),
        )
}

fn plan(track: &Track, app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let tabs = div()
        .flex()
        .gap(GAP_SMALL)
        .children(PlanTab::ALL.map(|tab| {
            choice_button(
                ("focus-plan-tab", tab as usize),
                tab.label(),
                app.focus.plan_tab == tab,
                &palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_plan_tab(tab, cx)))
            .test_support()
        }));
    let body = match app.focus.plan_tab {
        PlanTab::Transition => crate::views::planner::transition_tab(app, cx),
        PlanTab::Mashup => div()
            .flex()
            .flex_col()
            .gap(GAP_MEDIUM)
            .child(
                div()
                    .text_color(palette.text_secondary)
                    .child("Layer the vocals of one track over the instrumental of another, in the key that fits."),
            )
            .child(
                div().flex().child(
                    choice_button("focus-open-mashup", "Open mashup mode", false, &palette)
                        .on_click(cx.listener(|this, _, _, cx| this.open_mashup_from_focus(cx)))
                        .test_support(),
                ),
            ).into_any_element(),
        PlanTab::Cues => cues::section(track, app, Some(px(BOTTOM_HEIGHT - 110.0)), cx).into_any_element(),
    };
    let can_open_planner =
        app.focus.plan_tab == PlanTab::Transition && app.focus.selected_match.is_some();
    let hover_colour = palette.text;
    panel(&palette)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(section_label("Plan", &palette))
                .when(can_open_planner, |header| {
                    header.child(
                        div()
                            .id("plan-open")
                            .flex_shrink_0()
                            .cursor_pointer()
                            .text_size(SMALL_TEXT_SIZE)
                            .text_color(palette.accent)
                            .hover(move |style| style.text_color(hover_colour))
                            .child(OPEN_PLANNER_LINK)
                            .on_click(cx.listener(|this, _, _, cx| this.open_planner(cx)))
                            .test_support(),
                    )
                }),
        )
        .child(tabs)
        .child(
            div()
                .id("focus-plan-body")
                .flex_1()
                .min_h_0()
                .child(body)
                .test_support(),
        )
}

fn clash_cell(app: &EuphoriaApp, track_index: usize, palette: &Palette) -> AnyElement {
    let state = app.plan_state(track_index);
    let label = app.clash_label(track_index);
    let verdict_colour = match label {
        "Clean" => palette.accent,
        "Vocals overlap" => palette.danger,
        "Bass overlap" => crate::views::planner::clash_colour(
            Some(euphoria_core::harmony::transition::Clash::Bass),
            palette,
        ),
        _ => palette.text_tertiary,
    };
    let strip = match state {
        Some(crate::transition_plan::PlanState::Ready(planned)) => planned
            .window(0)
            .filter(|_| planned.stems_known() == (true, true))
            .map(|window| crate::views::planner::row_strip(window, palette)),
        _ => None,
    };
    let tooltip: Option<SharedString> = match state {
        Some(crate::transition_plan::PlanState::Failed(error)) => {
            Some(crate::views::planner::failure_message(app, *error).into())
        }
        Some(crate::transition_plan::PlanState::Ready(planned))
            if planned.stems_known() != (true, true) =>
        {
            Some(
                "Separate the stems of both tracks to check whether vocals or bass overlap.".into(),
            )
        }
        _ => None,
    };
    div()
        .id(("focus-mix-clash", track_index))
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(GAP_SMALL)
        .w(px(CLASH_COLUMN_WIDTH))
        .children(strip)
        .child(
            div()
                .id(("focus-mix-clash-verdict", track_index))
                .text_size(LABEL_TEXT_SIZE)
                .font_weight(FontWeight::MEDIUM)
                .text_color(verdict_colour)
                .truncate()
                .child(label)
                .test_support(),
        )
        .when_some(tooltip, |cell, text| {
            cell.tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx))
        })
        .test_support()
        .into_any_element()
}
