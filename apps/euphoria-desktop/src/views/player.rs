// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The player controls: the play and pause button, the position and length,
//! and the bar along the bottom of the window. In the layout with a strip
//! under the table, the controls sit in the strip beside the wide waveform
//! and there is no separate bar.

use std::sync::Arc;

use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, Context, Div, FontWeight, Hsla, MouseButton, MouseDownEvent,
    MouseMoveEvent, PathBuilder, Pixels, SharedString, Stateful, TestSupportExt, canvas, div, fill,
    point, px, relative, size,
};

use crate::app::EuphoriaApp;
use crate::artwork::ThumbnailSize;
use crate::library::Track;
use crate::peaks::BandPeaks;
use crate::player_wave::{BarPeaksCache, played_bars, section_ticks};
use crate::views::SMALL_TEXT_SIZE;
use crate::views::artwork::cover_tile;
use crate::views::cues;
use crate::views::waveform::SeenBounds;
use crate::views::{GAP_MEDIUM, GAP_SMALL, LABEL_TEXT_SIZE, RADIUS_SMALL};
use crate::waveforms::Waveform;

const BUTTON_SIZE: f32 = 30.0;
pub(crate) const GLYPH_SIZE: f32 = 10.0;

/// The round play and pause button. It is faint and does nothing while no
/// track is selected.
pub(crate) fn play_button(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> AnyElement {
    let palette = app.theme.palette;
    let shows_pause = app.player.offers_pause();
    let has_track = app.player.track().is_some();
    let glyph_colour = palette.background;

    div()
        .id("play-button")
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(BUTTON_SIZE))
        .rounded_full()
        .bg(palette.text)
        .map(|button| {
            if has_track {
                button
                    .cursor_pointer()
                    .hover(move |style| style.bg(palette.text_secondary))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_playback(cx)))
            } else {
                button.opacity(0.35)
            }
        })
        .child(
            canvas(
                |_, _, _| (),
                move |bounds: Bounds<Pixels>, _, window, _| {
                    if shows_pause {
                        paint_pause_glyph(bounds, glyph_colour, window);
                    } else {
                        paint_play_glyph(bounds, glyph_colour, window);
                    }
                },
            )
            .size(px(GLYPH_SIZE)),
        )
        .test_support()
        .into_any_element()
}

/// Two upright bars.
pub(crate) fn paint_pause_glyph(bounds: Bounds<Pixels>, colour: Hsla, window: &mut gpui::Window) {
    let bar_width = GLYPH_SIZE * 0.3;
    for left in [GLYPH_SIZE * 0.12, GLYPH_SIZE * 0.58] {
        let bar = Bounds::new(
            bounds.origin + point(px(left), px(GLYPH_SIZE * 0.08)),
            size(px(bar_width), px(GLYPH_SIZE * 0.84)),
        );
        window.paint_quad(fill(bar, colour));
    }
}

/// A triangle pointing right, nudged right so that it looks centred.
pub(crate) fn paint_play_glyph(bounds: Bounds<Pixels>, colour: Hsla, window: &mut gpui::Window) {
    let corner = |across: f32, down: f32| {
        bounds.origin + point(px(GLYPH_SIZE * across), px(GLYPH_SIZE * down))
    };
    let mut builder = PathBuilder::fill();
    builder.move_to(corner(0.22, 0.04));
    builder.line_to(corner(0.98, 0.5));
    builder.line_to(corner(0.22, 0.96));
    builder.close();
    if let Ok(path) = builder.build() {
        window.paint_path(path, colour);
    }
}

/// Makes `strip` a place to seek: a press jumps to that point of the track,
/// and dragging with the button held follows the pointer. `seen_bounds` is
/// where the strip was last drawn. A strip that shows another track than
/// the player's is left as it is: the player's track is not that track.
pub(crate) fn seek_on_press_and_drag(
    strip: Stateful<Div>,
    seen_bounds: &SeenBounds,
    shows_player_track: bool,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Stateful<Div> {
    if !shows_player_track {
        return strip;
    }
    let bounds_on_press = seen_bounds.clone();
    let bounds_on_drag = seen_bounds.clone();
    strip
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                this.seek_to_pointer(event.position.x, &bounds_on_press, cx);
                cx.stop_propagation();
            }),
        )
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            // A cue dragged across the strip moves the cue, not playing.
            if event.pressed_button == Some(MouseButton::Left) && this.cue_editor.drag.is_none() {
                this.seek_to_pointer(event.position.x, &bounds_on_drag, cx);
            }
        }))
}

/// Height of the waveform in the bar.
pub(crate) const WAVE_HEIGHT: f32 = 40.0;

/// Height of the row of cue letters above the waveform.
const LETTER_ROW_HEIGHT: f32 = 13.0;

const BAR_WIDTH: f32 = 2.0;
const SHORTEST_BAR_HEIGHT: f32 = 2.0;
const CUE_LINE_WIDTH: f32 = 2.0;
const TICK_HIT_WIDTH: f32 = 9.0;

/// The thin progress line shown while the waveform is not there.
fn thin_line(
    progress: f32,
    seen_bounds: SeenBounds,
    palette: crate::theme::Palette,
) -> Stateful<Div> {
    div()
        .id("progress-line")
        .flex()
        .flex_1()
        .items_center()
        .min_w(px(80.0))
        .h(px(20.0))
        .child(
            div()
                .relative()
                .w_full()
                .h(px(4.0))
                .rounded(RADIUS_SMALL)
                .bg(palette.raised)
                .child(
                    canvas(
                        move |bounds: Bounds<Pixels>, _, _| seen_bounds.set(Some(bounds)),
                        |_, _, _, _| (),
                    )
                    .absolute()
                    .size_full(),
                )
                .child(
                    div()
                        .h_full()
                        .w(relative(progress))
                        .rounded(RADIUS_SMALL)
                        .bg(palette.text_secondary),
                ),
        )
}

/// The waveform as the seek bar: bars filled up to the playing position in
/// the accent colour, a coloured line and a letter for each cue, and a
/// faint tick on each section boundary that names the section on hover.
fn wave_strip(
    track: &Track,
    columns: Arc<[BandPeaks]>,
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Stateful<Div> {
    let palette = app.theme.palette;
    let progress = app.player.progress();
    let duration = app
        .player
        .duration_seconds()
        .or(track.duration_seconds())
        .unwrap_or(0.0);
    let shown_cues = cues::shown_cues(track, app);
    let cue_fractions = cues::fractions(&shown_cues, duration);
    let ticks = section_ticks(&shown_cues, duration);
    let seen_bounds = app.progress_line_bounds.clone();
    let peaks = app.player_wave_peaks.clone();
    let played_colour = palette.accent.opacity(0.95);
    let unplayed_colour = palette.text_tertiary.opacity(0.55);

    let wave = canvas(
        move |bounds: Bounds<Pixels>, _, _| {
            seen_bounds.set(Some(bounds));
            let bar_count = BarPeaksCache::bar_count_for(bounds.size.width / px(1.0));
            peaks.borrow_mut().heights(&columns, bar_count)
        },
        move |bounds: Bounds<Pixels>, heights: Arc<[f32]>, window, _| {
            let played = played_bars(heights.len(), progress);
            let spacing = bounds.size.width / px(1.0) / heights.len().max(1) as f32;
            let half_height = WAVE_HEIGHT / 2.0;
            for (bar_index, height) in heights.iter().enumerate() {
                let bar_height = (height * WAVE_HEIGHT).max(SHORTEST_BAR_HEIGHT);
                let left = bar_index as f32 * spacing + (spacing - BAR_WIDTH) / 2.0;
                let bar = Bounds::new(
                    bounds.origin + point(px(left), px(half_height - bar_height / 2.0)),
                    size(px(BAR_WIDTH), px(bar_height)),
                );
                let colour = if bar_index < played {
                    played_colour
                } else {
                    unplayed_colour
                };
                window.paint_quad(fill(bar, colour));
            }
        },
    )
    .absolute()
    .top(px(LETTER_ROW_HEIGHT))
    .left_0()
    .w_full()
    .h(px(WAVE_HEIGHT));

    let cue_marks =
        shown_cues
            .iter()
            .zip(&cue_fractions)
            .enumerate()
            .flat_map(|(index, (cue, fraction))| {
                let fraction = *fraction;
                let letter = cue
                    .hot_cue
                    .map_or_else(|| "\u{2022}".to_string(), |hot_cue| hot_cue.to_string());
                let line = div()
                    .absolute()
                    .top(px(LETTER_ROW_HEIGHT - 2.0))
                    .h(px(WAVE_HEIGHT + 4.0))
                    .w(px(CUE_LINE_WIDTH))
                    .left(relative(fraction))
                    .bg(palette.accent);
                let label = div()
                    .id(("player-cue", index))
                    .absolute()
                    .top_0()
                    .h(px(LETTER_ROW_HEIGHT))
                    .left(relative(fraction))
                    .ml(px(-3.0))
                    .text_size(LABEL_TEXT_SIZE)
                    .font_weight(FontWeight::SEMIBOLD)
                    .font_family("Menlo")
                    .text_color(palette.accent)
                    .cursor_pointer()
                    .child(letter)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| this.jump_to_cue(index, cx)))
                    .test_support();
                [line.into_any_element(), label.into_any_element()]
            });

    let section_marks = ticks.into_iter().enumerate().map(|(index, tick)| {
        let name: SharedString = tick.name.into();
        div()
            .id(("player-section", index))
            .absolute()
            .top(px(LETTER_ROW_HEIGHT))
            .h(px(WAVE_HEIGHT))
            .w(px(TICK_HIT_WIDTH))
            .left(relative(tick.fraction))
            .ml(px(-TICK_HIT_WIDTH / 2.0))
            .flex()
            .justify_center()
            .child(
                div()
                    .h_full()
                    .w(px(1.0))
                    .bg(palette.text_tertiary.opacity(0.35)),
            )
            .tooltip(move |window, cx| Tooltip::new(name.clone()).build(window, cx))
            .test_support()
    });

    div()
        .id("player-wave")
        .relative()
        .flex_1()
        .min_w(px(80.0))
        .h(px(LETTER_ROW_HEIGHT + WAVE_HEIGHT))
        .child(wave)
        .children(section_marks)
        .children(cue_marks)
}

/// The id of the playing track's title in the player bar.
pub(crate) const TITLE_LINK_ID: &str = "player-title";

/// The id of the playing track's cover in the player bar.
pub(crate) const COVER_LINK_ID: &str = "player-cover-link";

/// What the playing track's title and cover say on hover.
const SHOW_IN_LIBRARY_TOOLTIP: &str = "Show in the library";

/// Makes `element` show the playing track in the table and the inspector
/// when clicked, leaving playing as it is.
pub(crate) fn shows_playing_track_on_click(
    element: Stateful<Div>,
    cx: &mut Context<'_, EuphoriaApp>,
) -> impl IntoElement {
    element
        .cursor_pointer()
        .tooltip(|window, cx| Tooltip::new(SHOW_IN_LIBRARY_TOOLTIP).build(window, cx))
        .on_click(cx.listener(|this, _, _, cx| this.show_playing_track(cx)))
        .test_support()
}

/// The title of the library track at `path`, or its file name.
fn title_of(app: &EuphoriaApp, path: &std::path::Path) -> String {
    app.session.library().track_by_path(path).map_or_else(
        || {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        },
        |track| track.title().to_string(),
    )
}

/// What the detail line under the title says.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum BarDetail {
    /// Who made the track, or its folder.
    Artist(String),
    /// A track asked for while this one sounds, named.
    Loading(String),
    /// Why the track, or the one asked for in its place, could not play.
    Problem(String),
}

/// What the player bar shows. All of it is the player's track, the one
/// heard while sound comes out, never the selection.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct BarContent {
    pub(crate) track: Option<std::path::PathBuf>,
    pub(crate) title: Option<String>,
    pub(crate) detail: Option<BarDetail>,
    pub(crate) position: String,
    pub(crate) length: String,
    /// The track whose waveform is drawn, when one is.
    pub(crate) waveform_of: Option<std::path::PathBuf>,
}

/// What the player bar shows now.
pub(crate) fn content(app: &EuphoriaApp) -> BarContent {
    let track = app.player_track();
    let detail = match (
        app.player.failure(),
        app.player.pending_track(),
        app.player.failed_switch(),
        track,
    ) {
        (Some(problem), _, _, _) => Some(BarDetail::Problem(problem.to_string())),
        (None, Some(pending), _, _) => Some(BarDetail::Loading(format!(
            "Loading {}\u{2026}",
            title_of(app, pending)
        ))),
        (None, None, Some((path, reason)), _) => Some(BarDetail::Problem(format!(
            "Could not play {}: {reason}",
            title_of(app, path)
        ))),
        (None, None, None, Some(track)) => Some(BarDetail::Artist(track.artist_or_folder())),
        (None, None, None, None) => None,
    };
    let has_waveform = matches!(
        app.player_waveform(),
        Some(Waveform::Ready(columns, _)) if !columns.is_empty()
    );
    BarContent {
        track: app.player.track().map(std::path::Path::to_path_buf),
        title: track.map(|track| track.title().to_string()),
        detail,
        position: crate::format::duration(app.player.position_seconds()),
        length: app
            .player
            .duration_seconds()
            .map(crate::format::duration)
            .unwrap_or_else(|| "\u{2013}".to_string()),
        waveform_of: track
            .filter(|_| has_waveform)
            .map(|track| track.path.clone()),
    }
}

/// The bar along the bottom of the window: the button, the track, the
/// position, the waveform that can be clicked to seek (a thin progress line
/// while there is none), and the length, as [`content`] says. A track asked
/// for while another sounds is named as loading until its sound starts.
pub(crate) fn bar(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> AnyElement {
    let palette = app.theme.palette;
    let track = app.player_track();
    let BarContent {
        title: name,
        detail,
        position,
        length,
        ..
    } = content(app);
    let detail = detail.map(|detail| match detail {
        BarDetail::Artist(text) => (text, palette.text_secondary),
        BarDetail::Loading(text) => (text, palette.text_tertiary),
        BarDetail::Problem(text) => (text, palette.danger),
    });
    let tempo_text = app.playing_tempo_text();
    let progress = app.player.progress().unwrap_or(0.0);

    let seen_bounds = app.progress_line_bounds.clone();
    let progress_line = match (track, app.player_waveform()) {
        (Some(track), Some(Waveform::Ready(columns, _))) if !columns.is_empty() => {
            wave_strip(track, columns.clone(), app, cx)
        }
        _ => thin_line(progress, seen_bounds, palette),
    };

    div()
        .id("player-bar")
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(GAP_MEDIUM)
        .px(px(14.0))
        .py(px(8.0))
        .border_t_1()
        .border_color(palette.line)
        .bg(palette.sidebar)
        .child(play_button(app, cx))
        .children(track.map(|track| {
            let cover = cover_tile(
                app,
                &track.path,
                px(40.0),
                ThumbnailSize::Small,
                "player-cover",
                cx,
            );
            shows_playing_track_on_click(div().id(COVER_LINK_ID).child(cover), cx)
        }))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_shrink_0()
                .w(px(if tempo_text.is_some() { 320.0 } else { 220.0 }))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(GAP_SMALL)
                        .child(match name {
                            Some(name) => shows_playing_track_on_click(
                                div()
                                    .id(TITLE_LINK_ID)
                                    .truncate()
                                    .min_w_0()
                                    .font_weight(FontWeight::MEDIUM)
                                    .hover(move |style| {
                                        style.text_color(palette.accent).underline()
                                    })
                                    .child(name),
                                cx,
                            )
                            .into_any_element(),
                            None => div()
                                .truncate()
                                .min_w_0()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette.text_tertiary)
                                .child("No track selected")
                                .into_any_element(),
                        })
                        .children(tempo_text.map(|text| {
                            div()
                                .id("player-tempo")
                                .flex_shrink_0()
                                .text_size(SMALL_TEXT_SIZE)
                                .text_color(palette.accent)
                                .child(text)
                                .test_support()
                        })),
                )
                .children(detail.map(|(text, colour)| {
                    div()
                        .truncate()
                        .text_size(SMALL_TEXT_SIZE)
                        .text_color(colour)
                        .child(text)
                })),
        )
        .child(
            div()
                .flex()
                .flex_shrink_0()
                .justify_end()
                .w(px(36.0))
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_secondary)
                .child(position),
        )
        .child(
            seek_on_press_and_drag(progress_line, &app.progress_line_bounds, true, cx)
                .test_support(),
        )
        .child(
            div()
                .flex_shrink_0()
                .w(px(36.0))
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_secondary)
                .child(length),
        )
        .test_support()
        .into_any_element()
}
