// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tracks to play after the one playing: a segmented control for where
//! the energy goes, and a row per track with why it fits.

use gpui::component::Icon;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, ClickEvent, Context, Div, KeyDownEvent, Pixels, ScrollHandle, Stateful,
    TestSupportExt, canvas, div, point, px,
};

use crate::app::EuphoriaApp;
use crate::assets::{LOAD_ON_DECK_ICON_PATH, PLAY_ICON_PATH};
use crate::format;
use crate::next_rail::{EnergyChoice, NextRow};
use crate::theme::Palette;
use crate::views::key_chip::key_chip;
use crate::views::{GAP_SMALL, GAP_TIGHT, RADIUS_LARGE, RADIUS_MEDIUM, RADIUS_SMALL};
use crate::views::{LABEL_TEXT_SIZE, NEXT_RAIL_KEY_CONTEXT, SMALL_TEXT_SIZE, section_label};
use crate::views::{primary_button, secondary_button};

const ROW_GROUP: &str = "next-row";

/// How tall to take the details of a row never drawn open, until measured.
const UNMEASURED_DETAILS_HEIGHT: f32 = 52.0;

const ACTION_HEIGHT: f32 = 24.0;

pub(crate) fn energy_id(choice: EnergyChoice) -> (&'static str, usize) {
    ("next-energy", choice.index())
}

pub(crate) fn row_id(track_index: usize) -> (&'static str, usize) {
    ("next-row", track_index)
}

pub(crate) fn play_id(track_index: usize) -> (&'static str, usize) {
    ("next-play", track_index)
}

pub(crate) fn load_id(track_index: usize) -> (&'static str, usize) {
    ("next-load", track_index)
}

pub(crate) fn details_id(track_index: usize) -> (&'static str, usize) {
    ("next-details", track_index)
}

pub(crate) fn preview_play_id(track_index: usize) -> (&'static str, usize) {
    ("next-preview-play", track_index)
}

pub(crate) fn preview_plan_id(track_index: usize) -> (&'static str, usize) {
    ("next-preview-plan", track_index)
}

pub(crate) fn preview_add_id(track_index: usize) -> (&'static str, usize) {
    ("next-preview-add", track_index)
}

/// Keep, Lift and Drop, the one chosen marked, or nothing while the
/// section follows the selected track.
pub(crate) fn control(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Option<Div> {
    app.rail_anchor?;
    Some(choice_control(app, cx))
}

/// Keep, Lift and Drop, the one chosen marked, wherever the list they order
/// is anchored.
pub(crate) fn choice_control(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    {
        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .p(px(1.0))
            .gap(GAP_TIGHT)
            .rounded(RADIUS_MEDIUM)
            .border_1()
            .border_color(palette.line)
            .children(EnergyChoice::ALL.into_iter().map(|choice| {
                let is_chosen = app.next_energy == choice;
                let hover_background = palette.raised;
                div()
                    .id(energy_id(choice))
                    .flex()
                    .items_center()
                    .h(px(18.0))
                    .px(px(7.0))
                    .rounded(RADIUS_MEDIUM)
                    .text_size(LABEL_TEXT_SIZE)
                    .cursor_pointer()
                    .map(|segment| {
                        if is_chosen {
                            segment.bg(palette.accent).text_color(palette.accent_ink)
                        } else {
                            segment
                                .text_color(palette.text_secondary)
                                .hover(move |style| style.bg(hover_background))
                        }
                    })
                    .tooltip(move |window, cx| Tooltip::new(choice.hint()).build(window, cx))
                    .child(choice.label())
                    .on_click(cx.listener(move |this, _, _, cx| this.set_next_energy(choice, cx)))
                    .test_support()
            }))
    }
}

pub(crate) fn pill(text: String, is_key: bool, palette: &Palette) -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(px(18.0))
        .px(px(7.0))
        .rounded(RADIUS_LARGE)
        .border_1()
        .whitespace_nowrap()
        .text_size(LABEL_TEXT_SIZE)
        .border_color(if is_key { palette.accent } else { palette.line })
        .text_color(if is_key {
            palette.text
        } else {
            palette.text_secondary
        })
        .child(text)
}

fn icon_button(
    id: (&'static str, usize),
    path: &'static str,
    tooltip: &'static str,
    palette: &Palette,
) -> Stateful<Div> {
    let hover_background = palette.line;
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(20.0))
        .rounded(RADIUS_SMALL)
        .cursor_pointer()
        .invisible()
        .group_hover(ROW_GROUP, |style| style.visible())
        .hover(move |style| style.bg(hover_background))
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
        .child(
            Icon::default()
                .path(path)
                .size(px(12.0))
                .text_color(palette.text_secondary),
        )
}

fn row(app: &EuphoriaApp, next: &NextRow, cx: &mut Context<'_, EuphoriaApp>) -> Option<AnyElement> {
    let palette = app.theme.palette;
    let track_index = next.track_index;
    let track = app.session.library().track(track_index)?;
    let hover_background = palette.raised;
    let decks_open = app.decks.is_open;
    let pills = next.pills();
    let fraction = app.next_preview.fraction(track_index);
    let is_expanded = app.next_preview.expanded() == Some(track_index);
    let summary = div()
        .id(row_id(track_index))
        .group(ROW_GROUP)
        .flex()
        .flex_shrink_0()
        .items_start()
        .gap(GAP_SMALL)
        .py(px(4.0))
        .px(px(2.0))
        .rounded(RADIUS_SMALL)
        .cursor_pointer()
        .map(|row| {
            if is_expanded {
                row.bg(palette.selection)
            } else {
                row.hover(move |style| style.bg(hover_background))
            }
        })
        .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
            this.rail_focus.focus(window, cx);
            if event.click_count() >= 2 {
                this.play_next_track(track_index, cx);
            } else {
                this.toggle_next_preview(track_index, cx);
            }
        }))
        .child(
            div().flex().flex_shrink_0().w(px(38.0)).children(
                track
                    .key()
                    .map(|key| key_chip(&key, &app.theme, app.tag_settings.key_notation)),
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
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(GAP_SMALL)
                        .children(
                            pills
                                .into_iter()
                                .enumerate()
                                .map(|(position, text)| pill(text, position == 0, &palette)),
                        )
                        .child(div().flex_1())
                        .child(
                            icon_button(
                                play_id(track_index),
                                PLAY_ICON_PATH,
                                if decks_open {
                                    "Load on deck B (Enter)"
                                } else {
                                    "Play (Enter)"
                                },
                                &palette,
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.play_next_track(track_index, cx);
                                cx.stop_propagation();
                            }))
                            .test_support(),
                        )
                        .when(decks_open, |actions| {
                            actions.child(
                                icon_button(
                                    load_id(track_index),
                                    LOAD_ON_DECK_ICON_PATH,
                                    "Load on deck B",
                                    &palette,
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.load_on_deck(
                                        crate::deck_audio::DeckId::B,
                                        track_index,
                                        cx,
                                    );
                                    cx.stop_propagation();
                                }))
                                .test_support(),
                            )
                        }),
                ),
        )
        .test_support();
    Some(
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .rounded(RADIUS_SMALL)
            .when(is_expanded, |block| block.bg(palette.raised))
            .child(summary)
            .when(fraction > 0.0, |block| {
                block.child(details(app, track, track_index, fraction, cx))
            })
            .into_any_element(),
    )
}

/// Scrolls the inspector so that `bounds`, the open row's details at full
/// height, are in view, when the inspector scrolls and they are below it.
fn scroll_into_view(scroll: &ScrollHandle, bounds: Bounds<Pixels>, window: &mut gpui::Window) {
    let viewport = scroll.bounds();
    let overflow = bounds.bottom() - viewport.bottom();
    if viewport.size.height <= px(0.0) || overflow <= px(0.0) {
        return;
    }
    let offset = scroll.offset();
    let lowest = -scroll.max_offset().y;
    scroll.set_offset(point(
        offset.x,
        (offset.y - overflow - GAP_SMALL).max(lowest),
    ));
    window.refresh();
}

/// The open row's details: key, tempo and energy in a line, then what can
/// be done with the track. The clip around them grows with `fraction`.
fn details(
    app: &EuphoriaApp,
    track: &crate::library::Track,
    track_index: usize,
    fraction: f32,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let palette = app.theme.palette;
    let seen_height = app.next_preview.seen_height.clone();
    let reveal = app.next_preview.reveal.clone();
    let inspector_scroll = app.inspector_scroll.clone();
    let is_open = app.next_preview.expanded() == Some(track_index);
    let natural_height = seen_height.get().unwrap_or(UNMEASURED_DETAILS_HEIGHT);
    let facts: Vec<String> = [
        track
            .key()
            .map(|key| app.tag_settings.key_notation.spell(&key)),
        track.bpm().map(|bpm| format!("{} BPM", format::bpm(bpm))),
        track.energy().map(|energy| format!("Energy {energy}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let play_label = if app.decks.is_open {
        "Load on deck B"
    } else {
        "Play"
    };
    let has_set = app.set_builder.planned.is_some();
    let small = |button: Div| button.h(px(ACTION_HEIGHT)).text_size(SMALL_TEXT_SIZE);
    let measured = div()
        .relative()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .gap(GAP_SMALL)
        .pt(GAP_TIGHT)
        .pb(GAP_SMALL)
        .pl(px(48.0))
        .pr(px(2.0))
        .child(
            div()
                .id(("next-facts", track_index))
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_secondary)
                .truncate()
                .child(facts.join(" \u{b7} "))
                .test_support(),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(GAP_TIGHT)
                .child(
                    small(primary_button(play_label, &palette))
                        .id(preview_play_id(track_index))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.play_next_track(track_index, cx);
                            cx.stop_propagation();
                        }))
                        .test_support(),
                )
                .child(
                    small(secondary_button("Plan transition", &palette))
                        .id(preview_plan_id(track_index))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.plan_transition_to(track_index, window, cx);
                            cx.stop_propagation();
                        }))
                        .test_support(),
                )
                .when(has_set, |actions| {
                    actions.child(
                        small(secondary_button("Add to set", &palette))
                            .id(preview_add_id(track_index))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.add_track_to_set(track_index, cx);
                                cx.stop_propagation();
                            }))
                            .test_support(),
                    )
                }),
        )
        .child(
            canvas(
                move |bounds: Bounds<Pixels>, window, _| {
                    seen_height.set(Some(bounds.size.height / px(1.0)));
                    if is_open && reveal.take() {
                        scroll_into_view(&inspector_scroll, bounds, window);
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        );
    div()
        .id(details_id(track_index))
        .flex()
        .flex_col()
        .overflow_hidden()
        .when(fraction < 1.0, |clip| {
            clip.h(px(natural_height * fraction)).opacity(fraction)
        })
        .child(measured)
        .test_support()
        .into_any_element()
}

/// The rows of the tracks to play next, at most `shown_count`. In the Deck
/// strip there is no section heading, so `with_heading` puts the title and
/// the control above them.
pub(crate) fn section(
    app: &EuphoriaApp,
    shown_count: usize,
    with_heading: bool,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Stateful<Div> {
    let palette = app.theme.palette;
    let rows: Vec<AnyElement> = app
        .next_rows
        .iter()
        .take(shown_count)
        .filter_map(|next| row(app, next, cx))
        .collect();
    let heading = with_heading.then(|| {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(GAP_SMALL)
            .child(
                section_label(app.mixes_title(), &palette)
                    .flex_1()
                    .min_w_0()
                    .truncate(),
            )
            .children(control(app, cx))
    });
    div()
        .id("next-rail")
        .track_focus(&app.rail_focus)
        .key_context(NEXT_RAIL_KEY_CONTEXT)
        .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
            this.next_rail_key_down(event, cx);
        }))
        .flex()
        .flex_col()
        .gap(GAP_TIGHT)
        .children(heading)
        .when(rows.is_empty(), |section| {
            section.child(
                div()
                    .id("next-empty")
                    .text_color(palette.text_tertiary)
                    .child(if app.track_filters.is_active() {
                        "No track left fits after this one under the filter bar."
                    } else {
                        "No track left fits after this one."
                    })
                    .test_support(),
            )
        })
        .children(rows)
}
