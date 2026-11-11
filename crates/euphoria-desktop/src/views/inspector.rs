// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The detail of the selected track: what it is, its key and tempo, its
//! waveform and, when analysis failed, why. Shown as a column on the right
//! or as a strip under the table, depending on the theme's layout.

use gpui::component::Icon;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, FontWeight, Pixels, SharedString, Stateful, TestSupportExt, Window,
    div, px,
};

use crate::app::EuphoriaApp;
use crate::assets::MUSIC_ICON_PATH;
use crate::camelot::CamelotPosition;
use crate::format;
use crate::library::{Track, TrackStatus};
use crate::mixes::compatible_keys;
use crate::theme::{AppTheme, Layout, Palette};
use crate::track_overview::{EnergyCurve, key_stretches, shows_key_timeline};
use crate::views::key_chip::key_chip;
use crate::views::player::{self, seek_on_press_and_drag};
use crate::views::track_overview;
use crate::views::waveform::{WAVEFORM_HEIGHT, WaveformColours, waveform};
use crate::views::wheel::{emphasis_for, wheel};
use crate::views::{LABEL_TEXT_SIZE, SMALL_TEXT_SIZE, section_label};
use crate::waveforms::Waveform;

/// Space around the contents of the inspector.
const PADDING: f32 = 16.0;

/// Height of the line above the waveform and above the mixes list, with the
/// gap under it.
const HEADING_HEIGHT: f32 = 26.0;

/// The shortest the waveform is drawn in the strip, with the energy curve
/// and the key timeline under it in the shortest strip.
const SMALLEST_STRIP_WAVEFORM: f32 = 20.0;

/// Space between the rows of the waveform section.
const WAVEFORM_ROW_GAP: f32 = 6.0;

/// Height of one row of the mixes list, with the gap under it.
const MIX_ROW_HEIGHT: f32 = 32.0;

/// How tall the contents of the track detail may be. In the column the
/// contents scroll, so only the strip under the table has a limit.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Room {
    Column,
    Strip { inner_height: f32 },
}

impl Room {
    /// How tall to draw the waveform when `below` is taken under it by the
    /// energy curve and the key timeline: the usual height in the column,
    /// and as tall as the strip allows under its heading.
    fn waveform_height(self, below: f32) -> Pixels {
        match self {
            Room::Column => px(WAVEFORM_HEIGHT),
            Room::Strip { inner_height } => {
                px((inner_height - HEADING_HEIGHT - below).max(SMALLEST_STRIP_WAVEFORM))
            }
        }
    }

    /// How many of `available` mixing suggestions to list: all of them in
    /// the column, and as many as the strip has rows for, at least one.
    fn mix_count(self, available: usize) -> usize {
        match self {
            Room::Column => available,
            Room::Strip { inner_height } => {
                let rows = ((inner_height - HEADING_HEIGHT) / MIX_ROW_HEIGHT).floor() as usize;
                rows.max(1).min(available)
            }
        }
    }
}

/// The inspector for the selected track, or a hint when nothing is selected.
pub(crate) fn render(
    app: &EuphoriaApp,
    window: &Window,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let theme = &app.theme;
    let palette = theme.palette;
    let is_column = theme.layout == Layout::InspectorRight;
    let strip_height = app.strip_height(window);
    let room = match theme.layout {
        Layout::InspectorRight => Room::Column,
        Layout::DeckBelow => Room::Strip {
            inner_height: strip_height / px(1.0) - 2.0 * PADDING,
        },
    };

    let container = match theme.layout {
        Layout::InspectorRight => div()
            .id("inspector")
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w(app.inspector_width(window))
            .gap(px(16.0))
            .border_l_1()
            .overflow_y_scroll(),
        Layout::DeckBelow => div()
            .id("inspector")
            .flex()
            .flex_shrink_0()
            .h(strip_height)
            .items_start()
            .gap(px(24.0))
            .border_t_1()
            .overflow_x_scroll(),
    }
    .p(px(PADDING))
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
        .child(sized(waveform_section(track, app, room, cx), None))
        .children(key_wheel(track, theme).map(|section| sized(divided(section), Some(220.0))))
        .children(status(track, &palette).map(|section| sized(divided(section), Some(280.0))))
        .children(quality(track, &palette).map(|section| sized(divided(section), Some(280.0))))
        .children(mixes(app, room, cx).map(|section| sized(divided(section), Some(300.0))))
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

/// The waveform, which is also where to seek: a press or a drag on it moves
/// playing to that point. In the strip under the table the play button and
/// the position sit beside it, since that layout has no player bar.
fn waveform_section(
    track: &Track,
    app: &EuphoriaApp,
    room: Room,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Div {
    let palette = app.theme.palette;
    let state = app.selected_waveform();
    let is_in_strip = app.theme.layout == Layout::DeckBelow;
    let (columns, note): (_, SharedString) = match (state, &track.status) {
        (Some(Waveform::Ready(columns, _)), _) => (
            Some(columns.clone()),
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
    let note = match app.player.failure() {
        Some(problem) if is_in_strip => div().text_color(palette.danger).child(problem.to_string()),
        _ if is_in_strip => div().child(app.player.time_label()),
        _ => div().child(note),
    };

    let progress = app.player.progress();
    let energy = match state {
        Some(Waveform::Ready(_, Some(EnergyCurve::Steps(steps)))) if !steps.is_empty() => {
            Some(track_overview::energy_curve(steps, progress, &palette))
        }
        Some(Waveform::Ready(_, Some(EnergyCurve::Unavailable(reason)))) => {
            Some(track_overview::energy_unavailable(reason, &palette))
        }
        _ => None,
    };
    let key_timeline = track.analysis().and_then(|analysis| {
        if !shows_key_timeline(analysis.key, &analysis.key_timeline) {
            return None;
        }
        let stretches = key_stretches(&analysis.key_timeline, analysis.duration_seconds);
        (!stretches.is_empty()).then(|| {
            track_overview::key_timeline(
                &stretches,
                analysis.duration_seconds,
                progress,
                &app.theme,
            )
        })
    });
    let below = energy
        .as_ref()
        .map_or(0.0, |_| track_overview::ENERGY_HEIGHT + WAVEFORM_ROW_GAP)
        + key_timeline.as_ref().map_or(0.0, |_| {
            track_overview::KEY_TIMELINE_HEIGHT + WAVEFORM_ROW_GAP
        });

    let scrubber = seek_on_press_and_drag(
        div().id("waveform").child(waveform(
            columns,
            WaveformColours::of(&palette),
            progress,
            app.waveform_bounds.clone(),
            room.waveform_height(below),
        )),
        &app.waveform_bounds,
        cx,
    )
    .test_support();

    let section = div()
        .flex()
        .flex_col()
        .gap(px(WAVEFORM_ROW_GAP))
        .child(
            div()
                .flex()
                .justify_between()
                .text_size(LABEL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child("Waveform")
                .child(note),
        )
        .child(scrubber)
        .children(energy)
        .children(key_timeline)
        .children(match state {
            Some(Waveform::Unreadable(reason)) => Some(
                div()
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_secondary)
                    .child(reason.clone()),
            ),
            _ => None,
        });
    if is_in_strip {
        div()
            .flex()
            .items_center()
            .gap(px(14.0))
            .child(player::play_button(app, cx))
            .child(section.flex_1().min_w_0())
    } else {
        section
    }
}

/// The tracks in the library that mix with the selected one.
fn mixes(app: &EuphoriaApp, room: Room, cx: &mut Context<'_, EuphoriaApp>) -> Option<Div> {
    let theme = &app.theme;
    let palette = theme.palette;
    app.selected_track()?.key()?;

    let shown_count = room.mix_count(app.mixes.len());
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
                            .child(track.title().to_string()),
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

/// The sound quality verdict, why, and what it is based on.
fn quality(track: &Track, palette: &Palette) -> Option<Div> {
    let report = track.quality()?;
    Some(
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(section_label("Sound quality", palette))
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(report.verdict.label()),
            )
            .child(
                div()
                    .text_color(palette.text_secondary)
                    .child(report.reason()),
            )
            .child(
                div()
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_tertiary)
                    .child(format::quality_details(report)),
            ),
    )
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
