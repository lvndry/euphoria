// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Live panel: the key and tempo of what the Mac is playing, how loud
//! it is, whether and what Euphoria listens to, and the tracks to play after
//! it.

use euphoria_core::Confidence;
use euphoria_core::live::ListenSource;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, Entity, FontWeight, TestSupportExt, Window, div, px, relative,
};

use crate::format;
use crate::live_model::{ListenState, LiveModel};
use crate::next_rail::{EnergyChoice, NextRow};
use crate::theme::Palette;
use crate::views::controls::{pressed_button, segment, segmented};
use crate::views::key_chip::key_chip;
use crate::views::next_rail;
use crate::views::{
    GAP_LARGE, GAP_MEDIUM, GAP_SMALL, GAP_TIGHT, LABEL_TEXT_SIZE, RADIUS_LARGE, RADIUS_SMALL,
    SMALL_TEXT_SIZE, TEXT_SIZE, primary_button, secondary_button, section_label, tabular_digits,
};

/// The width of the panel.
pub(crate) const PANEL_WIDTH: f32 = 340.0;
/// The height of the panel.
pub(crate) const PANEL_HEIGHT: f32 = 620.0;

const BIG_KEY_SIZE: f32 = 44.0;
const BIG_FIGURE_SIZE: f32 = 22.0;
const METER_HEIGHT: f32 = 6.0;

pub(crate) const LISTENING_ID: &str = "live-listening";
pub(crate) const PAUSE_ID: &str = "live-pause";
pub(crate) const SOURCE_ID: &str = "live-source";
pub(crate) const KEEP_ID: &str = "live-keep";
pub(crate) const KEY_ID: &str = "live-key";
pub(crate) const BPM_ID: &str = "live-bpm";
pub(crate) const WAITING_ID: &str = "live-waiting";
pub(crate) const OPEN_ID: &str = "live-open";

pub(crate) fn energy_id(choice: EnergyChoice) -> (&'static str, usize) {
    ("live-energy", choice.index())
}

pub(crate) fn row_id(track_index: usize) -> (&'static str, usize) {
    ("live-row", track_index)
}

pub(crate) fn source_option_id(position: usize) -> (&'static str, usize) {
    ("live-source-option", position)
}

pub(crate) struct LiveView {
    pub(crate) model: Entity<LiveModel>,
    pub(crate) picker_open: bool,
}

impl LiveView {
    pub(crate) fn new(model: Entity<LiveModel>, cx: &mut Context<'_, Self>) -> Self {
        cx.observe(&model, |_, _, cx| cx.notify()).detach();
        model.update(cx, |model, cx| model.ensure_library(cx));
        Self {
            model,
            picker_open: false,
        }
    }
}

fn confidence_word(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::High => "high confidence",
        Confidence::Medium => "medium confidence",
        Confidence::Low => "low confidence",
    }
}

fn confidence_colour(confidence: Confidence, palette: &Palette) -> gpui::Hsla {
    match confidence {
        Confidence::High => palette.success,
        Confidence::Medium => palette.text_secondary,
        Confidence::Low => palette.warning,
    }
}

fn indicator(model: &LiveModel, palette: &Palette) -> impl IntoElement {
    let (dot, text) = match &model.state {
        ListenState::Listening => (palette.danger, "Listening"),
        ListenState::Paused => (palette.text_tertiary, "Paused"),
        ListenState::Unavailable(_) => (palette.warning, "Not listening"),
    };
    div()
        .id(LISTENING_ID)
        .flex()
        .items_center()
        .gap(GAP_TIGHT)
        .text_size(SMALL_TEXT_SIZE)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(if model.is_listening() {
            palette.text
        } else {
            palette.text_secondary
        })
        .child(div().size(px(8.0)).rounded_full().bg(dot))
        .child(text)
        .test_support()
}

fn level_meter(model: &LiveModel, palette: &Palette) -> Div {
    let position = model
        .reading
        .level
        .filter(|_| model.is_listening())
        .map_or(0.0, |level| level.meter_position());
    let is_hot = model
        .reading
        .level
        .is_some_and(|level| level.peak_db > -1.0);
    div()
        .h(px(METER_HEIGHT))
        .w_full()
        .rounded(RADIUS_SMALL)
        .bg(palette.line)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative(position))
                .rounded(RADIUS_SMALL)
                .bg(if is_hot {
                    palette.danger
                } else {
                    palette.accent
                }),
        )
}

fn big_reading(model: &LiveModel) -> AnyElement {
    let palette = model.theme.palette;
    let reading = &model.reading;
    let waiting = |text: String| {
        div()
            .id(WAITING_ID)
            .flex()
            .flex_col()
            .gap(GAP_TIGHT)
            .child(
                div()
                    .text_size(px(BIG_FIGURE_SIZE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.text_secondary)
                    .child(text),
            )
            .test_support()
            .into_any_element()
    };
    match &model.state {
        ListenState::Paused => return waiting("Not listening".to_string()),
        ListenState::Unavailable(reason) => {
            return div()
                .flex()
                .flex_col()
                .gap(GAP_TIGHT)
                .child(
                    div()
                        .id(WAITING_ID)
                        .text_size(px(BIG_FIGURE_SIZE))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(palette.text_secondary)
                        .child("Cannot listen")
                        .test_support(),
                )
                .child(
                    div()
                        .text_size(SMALL_TEXT_SIZE)
                        .text_color(palette.text_secondary)
                        .child(reason.clone()),
                )
                .into_any_element();
        }
        ListenState::Listening => {}
    }
    let Some(key) = reading.key.filter(|_| !reading.listening) else {
        let hint = (reading.no_signal_seconds >= 10.0).then_some(
            "Only silence so far. If audio is playing, allow Euphoria under System Settings, \
             Privacy & Security, Screen & System Audio Recording.",
        );
        return div()
            .flex()
            .flex_col()
            .gap(GAP_TIGHT)
            .child(
                div()
                    .id(WAITING_ID)
                    .text_size(px(BIG_FIGURE_SIZE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.text_secondary)
                    .child("Listening\u{2026}")
                    .test_support(),
            )
            .children(hint.map(|hint| {
                div()
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_secondary)
                    .child(hint)
            }))
            .into_any_element();
    };
    div()
        .flex()
        .items_end()
        .gap(GAP_MEDIUM)
        .child(
            div()
                .id(KEY_ID)
                .text_size(px(BIG_KEY_SIZE))
                .line_height(px(BIG_KEY_SIZE + 4.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.text)
                .font_features(tabular_digits())
                .child(model.notation.spell(&key.key))
                .test_support(),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .pb(px(4.0))
                .child(
                    div()
                        .text_size(TEXT_SIZE)
                        .text_color(palette.text)
                        .child(key.key.to_string()),
                )
                .child(
                    div()
                        .text_size(SMALL_TEXT_SIZE)
                        .text_color(confidence_colour(key.confidence, &palette))
                        .child(confidence_word(key.confidence)),
                ),
        )
        .into_any_element()
}

fn figures(model: &LiveModel) -> Div {
    let palette = model.theme.palette;
    let reading = &model.reading;
    let shown = model.is_listening() && !reading.listening;
    let figure = |label: &'static str, value: Option<String>, id: Option<&'static str>| {
        let has_value = value.is_some();
        let value_element = div()
            .text_size(px(BIG_FIGURE_SIZE))
            .font_weight(FontWeight::SEMIBOLD)
            .font_features(tabular_digits())
            .text_color(if value.is_some() {
                palette.text
            } else {
                palette.text_tertiary
            })
            .child(value.unwrap_or_else(|| "\u{2013}".to_string()));
        div()
            .flex()
            .flex_col()
            .gap(GAP_TIGHT)
            .child(section_label(label, &palette))
            .child(match id {
                Some(id) if has_value => value_element.id(id).test_support().into_any_element(),
                _ => value_element.into_any_element(),
            })
    };
    let bpm = shown
        .then_some(reading.tempo)
        .flatten()
        .map(|tempo| format!("{:.1}", tempo.bpm));
    let energy = shown
        .then_some(reading.energy)
        .flatten()
        .map(|energy| energy.to_string());
    div()
        .flex()
        .gap(GAP_LARGE)
        .child(figure("BPM", bpm, Some(BPM_ID)))
        .child(figure("ENERGY", energy, None))
}

fn source_picker(model: &LiveModel, picker_open: bool, cx: &Context<'_, LiveView>) -> Div {
    let palette = model.theme.palette;
    let label = format!("Listen to: {}", model.source.label());
    let mut options: Vec<ListenSource> = vec![ListenSource::AllAudio];
    options.extend(model.sources.iter().cloned());
    if !options.contains(&model.source) {
        options.push(model.source.clone());
    }
    let hover_background = palette.raised;
    div()
        .flex()
        .flex_col()
        .gap(GAP_TIGHT)
        .child(
            secondary_button(format!("{label}  \u{25BE}"), &palette)
                .id(SOURCE_ID)
                .w_full()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.picker_open = !this.picker_open;
                    if this.picker_open {
                        this.model.update(cx, |model, cx| model.refresh_sources(cx));
                    }
                    cx.notify();
                }))
                .test_support(),
        )
        .when(picker_open, |picker| {
            picker.child(
                div()
                    .flex()
                    .flex_col()
                    .p(px(2.0))
                    .rounded(RADIUS_LARGE)
                    .border_1()
                    .border_color(palette.line)
                    .bg(palette.raised)
                    .children(options.into_iter().enumerate().map(|(position, source)| {
                        let is_chosen = source == model.source;
                        let chosen_source = source.clone();
                        div()
                            .id(source_option_id(position))
                            .flex()
                            .items_center()
                            .h(px(24.0))
                            .px(GAP_SMALL)
                            .rounded(RADIUS_SMALL)
                            .text_size(SMALL_TEXT_SIZE)
                            .cursor_pointer()
                            .text_color(if is_chosen {
                                palette.accent
                            } else {
                                palette.text
                            })
                            .hover(move |style| style.bg(hover_background))
                            .child(source.label().to_string())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.picker_open = false;
                                let source = chosen_source.clone();
                                this.model
                                    .update(cx, |model, cx| model.choose_source(source, cx));
                                cx.notify();
                            }))
                            .test_support()
                    })),
            )
        })
}

fn next_row(model: &LiveModel, next: &NextRow) -> Option<AnyElement> {
    let palette = model.theme.palette;
    let track = model.library.as_ref()?.track(next.track_index)?;
    let hover_background = palette.raised;
    Some(
        div()
            .id(row_id(next.track_index))
            .flex()
            .flex_shrink_0()
            .items_start()
            .gap(GAP_SMALL)
            .py(px(4.0))
            .px(px(2.0))
            .rounded(RADIUS_SMALL)
            .hover(move |style| style.bg(hover_background))
            .child(
                div().flex().flex_shrink_0().w(px(38.0)).children(
                    track
                        .key()
                        .map(|key| key_chip(&key, &model.theme, model.notation)),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(GAP_TIGHT)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(GAP_SMALL)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .child(track.title().to_string()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_shrink_0()
                                    .justify_end()
                                    .text_size(SMALL_TEXT_SIZE)
                                    .text_color(palette.text_secondary)
                                    .children(
                                        next.tempo_difference_percent.map(format::signed_percent),
                                    ),
                            ),
                    )
                    .child(div().flex().items_center().gap(GAP_SMALL).children(
                        next_rail::row_pills(
                            next.pills(next_rail::PLAYING),
                            "live-energy-pill",
                            next.track_index,
                            &palette,
                        ),
                    )),
            )
            .test_support()
            .into_any_element(),
    )
}

fn next_section(model: &LiveModel, model_entity: &Entity<LiveModel>) -> Div {
    let palette = model.theme.palette;
    let control = segmented(
        EnergyChoice::ALL
            .into_iter()
            .enumerate()
            .map(|(position, choice)| {
                let model_entity = model_entity.clone();
                segment(
                    energy_id(choice),
                    choice.label(),
                    model.energy == choice,
                    position == 0,
                    &palette,
                )
                .on_click(move |_, _, cx| {
                    model_entity.update(cx, |model, cx| model.set_energy(choice, cx));
                })
                .test_support()
                .into_any_element()
            })
            .collect(),
        &palette,
    );
    let rows: Vec<AnyElement> = model
        .next_rows
        .iter()
        .filter_map(|next| next_row(model, next))
        .collect();
    let has_key = model.is_listening() && !model.reading.listening && model.reading.key.is_some();
    let empty_text = if !has_key {
        "Tracks that mix in appear once a key is heard."
    } else if model.library.is_none() {
        "Reading your library\u{2026}"
    } else {
        "No track in your library mixes in with this."
    };
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .gap(GAP_SMALL)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(section_label("Next", &palette))
                .child(control),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .gap(GAP_TIGHT)
                .when(rows.is_empty(), |list| {
                    list.child(
                        div()
                            .text_size(SMALL_TEXT_SIZE)
                            .text_color(palette.text_tertiary)
                            .child(empty_text),
                    )
                })
                .children(rows),
        )
}

impl Render for LiveView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let model_entity = self.model.clone();
        let model = self.model.read(cx);
        let palette = model.theme.palette;
        let is_listening = model.is_listening();
        let picker_open = self.picker_open;
        let keep_running = model.settings.keep_running_when_closed;

        let pause = {
            let label = if is_listening {
                "Pause listening"
            } else {
                "Start listening"
            };
            let button = if is_listening {
                secondary_button(label, &palette)
            } else {
                primary_button(label, &palette)
            };
            let model_entity = model_entity.clone();
            button
                .id(PAUSE_ID)
                .w_full()
                .justify_center()
                .on_click(move |_, _, cx| {
                    model_entity.update(cx, |model, cx| model.toggle_listening(cx));
                })
                .test_support()
        };
        let keep = {
            let model_entity = model_entity.clone();
            pressed_button(
                KEEP_ID,
                "Keep Live running when the window is closed",
                keep_running,
                &palette,
            )
            .h(px(24.0))
            .on_click(move |_, _, cx| {
                model_entity.update(cx, |model, cx| {
                    let keep = !model.settings.keep_running_when_closed;
                    model.set_keep_running(keep, cx);
                });
            })
            .test_support()
        };

        let source = source_picker(model, picker_open, cx);
        div()
            .id("live-panel")
            .flex()
            .flex_col()
            .size_full()
            .gap(GAP_MEDIUM)
            .p(GAP_LARGE)
            .bg(palette.background)
            .text_color(palette.text)
            .text_size(TEXT_SIZE)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(indicator(model, &palette))
                    .child(
                        div()
                            .text_size(LABEL_TEXT_SIZE)
                            .text_color(palette.text_tertiary)
                            .child("Nothing is recorded"),
                    ),
            )
            .child(source)
            .child(big_reading(model))
            .child(figures(model))
            .child(level_meter(model, &palette))
            .child(next_section(model, &model_entity))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(GAP_SMALL)
                    .child(pause)
                    .child(keep)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                crate::views::controls::text_link(
                                    OPEN_ID,
                                    "Open Euphoria",
                                    &palette,
                                )
                                .on_click(|_, _, cx| crate::open_euphoria(cx))
                                .test_support(),
                            )
                            .child(
                                crate::views::controls::text_link(
                                    "live-quit",
                                    "Quit Euphoria",
                                    &palette,
                                )
                                .on_click(|_, _, cx| cx.quit()),
                            ),
                    ),
            )
    }
}
