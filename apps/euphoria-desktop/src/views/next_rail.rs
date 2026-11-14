// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tracks to play after the one playing: a segmented control for where
//! the energy goes, and a row per track with why it fits.

use gpui::component::Icon;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{AnyElement, ClickEvent, Context, Div, Stateful, TestSupportExt, div, px};

use crate::app::EuphoriaApp;
use crate::assets::{LOAD_ON_DECK_ICON_PATH, PLAY_ICON_PATH};
use crate::format;
use crate::next_rail::{EnergyChoice, NextRow};
use crate::theme::Palette;
use crate::views::key_chip::key_chip;
use crate::views::{LABEL_TEXT_SIZE, NEXT_RAIL_KEY_CONTEXT, SMALL_TEXT_SIZE, section_label};

const ROW_GROUP: &str = "next-row";

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

/// Keep, Lift and Drop, the one chosen marked, or nothing while the
/// section follows the selected track.
pub(crate) fn control(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Option<Div> {
    app.rail_anchor?;
    let palette = app.theme.palette;
    Some(
        div()
            .flex()
            .flex_shrink_0()
            .items_center()
            .p(px(1.0))
            .gap(px(1.0))
            .rounded(px(6.0))
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
                    .rounded(px(5.0))
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
            })),
    )
}

fn pill(text: String, is_key: bool, palette: &Palette) -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(px(18.0))
        .px(px(7.0))
        .rounded(px(9.0))
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
        .rounded(px(4.0))
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
    let is_selected = app.selected == Some(track_index);
    let hover_background = palette.raised;
    let decks_open = app.decks.is_open;
    let pills = next.pills();
    Some(
        div()
            .id(row_id(track_index))
            .group(ROW_GROUP)
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(8.0))
            .h(px(28.0))
            .px(px(2.0))
            .rounded(px(4.0))
            .cursor_pointer()
            .map(|row| {
                if is_selected {
                    row.bg(palette.selection)
                } else {
                    row.hover(move |style| style.bg(hover_background))
                }
            })
            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                this.select_and_reveal(track_index, cx);
                this.rail_focus.focus(window, cx);
                if event.click_count() >= 2 {
                    this.play_next_track(track_index, cx);
                }
            }))
            .child(
                div().flex().flex_shrink_0().w(px(38.0)).children(
                    track
                        .key()
                        .map(|key| key_chip(&key, &app.theme, app.display.key_notation)),
                ),
            )
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
                    .w(px(46.0))
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_secondary)
                    .children(next.tempo_difference_percent.map(format::signed_percent)),
            )
            .children(
                pills
                    .into_iter()
                    .enumerate()
                    .map(|(position, text)| pill(text, position == 0, &palette)),
            )
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
            .when(decks_open, |row| {
                row.child(
                    icon_button(
                        load_id(track_index),
                        LOAD_ON_DECK_ICON_PATH,
                        "Load on deck B",
                        &palette,
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.load_on_deck(crate::deck_audio::DeckId::B, track_index, cx);
                        cx.stop_propagation();
                    }))
                    .test_support(),
                )
            })
            .test_support()
            .into_any_element(),
    )
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
            .gap(px(8.0))
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
        .flex()
        .flex_col()
        .gap(px(4.0))
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
