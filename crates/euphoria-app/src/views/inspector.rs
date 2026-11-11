// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The detail of the selected track: what it is, its key and tempo, its
//! waveform and, when analysis failed, why. Shown as a column on the right
//! or as a strip under the table, depending on the theme's layout.

use gpui::component::Icon;
use gpui::prelude::*;
use gpui::{AnyElement, Context, Div, FontWeight, SharedString, Stateful, TestSupportExt, div, px};

use crate::app::EuphoriaApp;
use crate::assets::MUSIC_ICON_PATH;
use crate::camelot::CamelotPosition;
use crate::format;
use crate::library::{Track, TrackStatus};
use crate::mixes::compatible_keys;
use crate::theme::{AppTheme, Layout, Palette};
use crate::views::key_chip::key_chip;
use crate::views::waveform::waveform;
use crate::views::wheel::{emphasis_for, wheel};
use crate::views::{LABEL_TEXT_SIZE, SMALL_TEXT_SIZE, section_label};
use crate::waveforms::Waveform;

const INSPECTOR_WIDTH: f32 = 320.0;

/// How many mixing suggestions fit in the strip under the table.
const DECK_MIX_COUNT: usize = 4;

/// The inspector for the selected track, or a hint when nothing is selected.
pub(crate) fn render(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> AnyElement {
    let theme = &app.theme;
    let palette = theme.palette;
    let is_column = theme.layout == Layout::InspectorRight;

    let container = match theme.layout {
        Layout::InspectorRight => div()
            .id("inspector")
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(px(INSPECTOR_WIDTH))
            .gap(px(16.0))
            .border_l_1()
            .overflow_y_scroll(),
        Layout::DeckBelow => div()
            .id("inspector")
            .flex()
            .flex_shrink_0()
            .items_start()
            .gap(px(24.0))
            .border_t_1()
            .overflow_x_scroll(),
    }
    .p(px(16.0))
    .bg(palette.panel)
    .border_color(palette.line);

    let Some(track) = app.selected_track() else {
        return container
            .child(
                div()
                    .text_color(palette.text_tertiary)
                    .child("Select a track to see its key, tempo and waveform."),
            )
            .test_support()
            .into_any_element();
    };

    // In the column every section fills the width. In the strip each has a
    // width of its own and the waveform takes what is left.
    let sized = |section: Div, strip_width: Option<f32>| match (is_column, strip_width) {
        (true, _) => section,
        (false, Some(width)) => section.w(px(width)).flex_shrink_0(),
        (false, None) => section.flex_1().min_w(px(300.0)),
    };
    let divided = |section: Div| {
        if is_column {
            section.pt(px(14.0)).border_t_1().border_color(palette.line)
        } else {
            section
        }
    };

    container
        .child(sized(summary(track, &palette), Some(220.0)))
        .child(sized(statistics(track, &palette), Some(264.0)))
        .child(sized(
            waveform_section(track, app.selected_waveform(), &palette),
            None,
        ))
        .children(key_wheel(track, theme).map(|section| sized(divided(section), Some(220.0))))
        .children(status(track, &palette).map(|section| sized(divided(section), Some(280.0))))
        .children(mixes(app, cx).map(|section| sized(divided(section), Some(300.0))))
        .when(is_column, |inspector| {
            inspector.child(divided(location(track, &palette)))
        })
        .test_support()
        .into_any_element()
}

/// Cover placeholder, file name, folder and format.
fn summary(track: &Track, palette: &Palette) -> Div {
    let format_and_length = match track.duration_seconds() {
        Some(seconds) => format!("{}, {}", track.format, format::duration(seconds)),
        None => track.format.clone(),
    };
    div()
        .flex()
        .items_center()
        .gap(px(12.0))
        .child(
            div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .size(px(56.0))
                .rounded(px(6.0))
                .border_1()
                .border_color(palette.line)
                .bg(palette.raised)
                .child(
                    Icon::default()
                        .path(MUSIC_ICON_PATH)
                        .size(px(20.0))
                        .text_color(palette.text_tertiary),
                ),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .gap(px(2.0))
                .child(
                    div()
                        .text_size(px(15.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .line_clamp(2)
                        .child(track.file_name.clone()),
                )
                .child(
                    div()
                        .truncate()
                        .text_color(palette.text_secondary)
                        .child(track.folder.clone()),
                )
                .child(
                    div()
                        .text_size(SMALL_TEXT_SIZE)
                        .text_color(palette.text_tertiary)
                        .child(format_and_length),
                ),
        )
}

/// Key, tempo and energy in three boxes.
fn statistics(track: &Track, palette: &Palette) -> Div {
    let not_known_yet = match track.status {
        TrackStatus::Queued => "queued",
        TrackStatus::Analyzing => "analyzing",
        TrackStatus::Failed(_) => "not analyzed",
        TrackStatus::Analyzed(_) => "not detected",
    };
    let (key_code, key_name) = match track.key() {
        Some(key) => (key.camelot(), key.to_string()),
        None => ("\u{2013}".to_string(), not_known_yet.to_string()),
    };
    let (tempo, tempo_note) = match track.bpm() {
        Some(bpm) => (format::bpm(bpm), "per minute"),
        None => ("\u{2013}".to_string(), not_known_yet),
    };
    let (energy, energy_note) = match track.energy() {
        Some(rating) => (rating.to_string(), "of 10"),
        None if track.analysis().is_some() => ("\u{2013}".to_string(), "not rated"),
        None => ("\u{2013}".to_string(), not_known_yet),
    };

    div()
        .flex()
        .rounded(px(8.0))
        .border_1()
        .border_color(palette.line)
        .child(statistic("Key", key_code, key_name, palette))
        .child(
            statistic("BPM", tempo, tempo_note, palette)
                .border_l_1()
                .border_color(palette.line),
        )
        .child(
            statistic("Energy", energy, energy_note, palette)
                .border_l_1()
                .border_color(palette.line),
        )
}

fn statistic(
    label: &'static str,
    value: impl Into<SharedString>,
    note: impl Into<SharedString>,
    palette: &Palette,
) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(px(2.0))
        .px(px(12.0))
        .py(px(10.0))
        .child(
            div()
                .text_size(LABEL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child(label),
        )
        .child(
            div()
                .whitespace_nowrap()
                .text_size(px(20.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(value.into()),
        )
        .child(
            div()
                .truncate()
                .text_size(LABEL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child(note.into()),
        )
}

fn waveform_section(track: &Track, state: Option<&Waveform>, palette: &Palette) -> Div {
    let (peaks, note): (_, SharedString) = match (state, &track.status) {
        (Some(Waveform::Ready(peaks)), _) => (
            Some(peaks.clone()),
            track
                .duration_seconds()
                .map(format::duration)
                .unwrap_or_default()
                .into(),
        ),
        (Some(Waveform::Unreadable(_)), _) => (None, "Could not read this file".into()),
        (_, TrackStatus::Queued) => (None, "Waiting for analysis".into()),
        (_, TrackStatus::Analyzing) => (None, "Analyzing now".into()),
        (_, TrackStatus::Failed(_)) => (None, "Not available".into()),
        (Some(Waveform::Reading) | None, TrackStatus::Analyzed(_)) => {
            (None, "Reading audio".into())
        }
    };
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .justify_between()
                .text_size(LABEL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child("Waveform")
                .child(note),
        )
        .child(waveform(peaks, palette.waveform))
        .children(match state {
            Some(Waveform::Unreadable(reason)) => Some(
                div()
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_secondary)
                    .child(reason.clone()),
            ),
            _ => None,
        })
}

/// The tracks in the library that mix with the selected one.
fn mixes(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Option<Div> {
    let theme = &app.theme;
    let palette = theme.palette;
    app.selected_track()?.key()?;

    let shown_count = match theme.layout {
        Layout::InspectorRight => app.mixes.len(),
        Layout::DeckBelow => DECK_MIX_COUNT,
    };
    let rows: Vec<Stateful<Div>> = app
        .mixes
        .iter()
        .take(shown_count)
        .filter_map(|mix| {
            let track_index = mix.track_index;
            let track = app.session.library().track(track_index)?;
            Some(
                div()
                    .id(("mix", track_index))
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap(px(10.0))
                    .h(px(28.0))
                    .rounded(px(4.0))
                    .cursor_pointer()
                    .hover(move |style| style.bg(palette.raised))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_and_reveal(track_index, cx);
                    }))
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .w(px(38.0))
                            .children(track.key().map(|key| key_chip(&key, theme))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(track.file_name.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .justify_end()
                            .w(px(46.0))
                            .text_color(palette.text_secondary)
                            .children(mix.tempo_difference_percent.map(format::signed_percent)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .justify_end()
                            .w(px(62.0))
                            .text_size(SMALL_TEXT_SIZE)
                            .text_color(palette.text_tertiary)
                            .child(mix.relation.label()),
                    ),
            )
        })
        .collect();

    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(section_label("Mixes well with", &palette).pb(px(4.0)))
            .when(rows.is_empty(), |section| {
                section.child(
                    div()
                        .text_color(palette.text_tertiary)
                        .child("No track in the library mixes with this one yet."),
                )
            })
            .children(rows),
    )
}

/// The Camelot wheel, in themes that show it, for a track with a key.
fn key_wheel(track: &Track, theme: &AppTheme) -> Option<Div> {
    if !theme.shows_wheel {
        return None;
    }
    let key = track.key()?;
    let palette = &theme.palette;
    Some(
        div()
            .flex()
            .items_center()
            .gap(px(14.0))
            .child(wheel(
                key.camelot(),
                emphasis_for(
                    CamelotPosition::of(&key),
                    compatible_keys(&key).iter().map(CamelotPosition::of),
                ),
                palette,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_secondary)
                    .child(format!(
                        "Highlighted keys mix with {}, {}.",
                        key.camelot(),
                        key
                    )),
            ),
    )
}

/// Why a failed track failed, or that an unfinished one is queued or
/// being analyzed.
fn status(track: &Track, palette: &Palette) -> Option<Div> {
    let section = div().flex().flex_col().gap(px(8.0));
    match &track.status {
        TrackStatus::Analyzed(_) => None,
        TrackStatus::Queued => Some(
            section.child(section_label("Analysis", palette)).child(
                div()
                    .text_color(palette.text_secondary)
                    .child("Queued. It will be analyzed when the tracks ahead of it are done."),
            ),
        ),
        TrackStatus::Analyzing => Some(
            section.child(section_label("Analysis", palette)).child(
                div()
                    .text_color(palette.text_secondary)
                    .child("Being analyzed now."),
            ),
        ),
        TrackStatus::Failed(reason) => Some(
            section
                .child(section_label("Analysis", palette))
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette.danger)
                        .child("Could not analyze this file"),
                )
                .child(
                    div()
                        .text_color(palette.text_secondary)
                        .child(reason.clone()),
                ),
        ),
    }
}

/// Where the file is on disk.
fn location(track: &Track, palette: &Palette) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(section_label("Location", palette))
        .child(
            div()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_secondary)
                .child(track.path.to_string_lossy().into_owned()),
        )
}
