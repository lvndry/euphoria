// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Loops on screen: the loop buttons at the top of the focus view, the
//! loop as a tinted stretch of the detail with an edge to drag at each end,
//! and the loops saved with a track, listed with its cues.

use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, FontWeight, MouseButton, MouseDownEvent, SharedString,
    TestSupportExt, div, px, relative,
};

use crate::app::EuphoriaApp;
use crate::focus::Viewport;
use crate::focus_loops::{LOOP_BEATS, LoopEdge, LoopGrid, length_label, length_words};
use crate::format;
use crate::library::Track;
use crate::theme::Palette;
use crate::views::controls::pressed_button;
use crate::views::{
    GAP_SMALL, GAP_TIGHT, LABEL_TEXT_SIZE, RADIUS_SMALL, SMALL_TEXT_SIZE, section_label,
};

/// The width of the grip at each end of the loop on the detail.
const EDGE_GRIP_WIDTH: f32 = 8.0;

/// The element id of the loop button for `beats` beats.
pub(crate) fn length_id(beats: f64) -> (&'static str, usize) {
    ("focus-loop-length", (beats * 2.0) as usize)
}

/// The element id of saved loop `index`.
pub(crate) fn saved_loop_id(index: usize) -> (&'static str, usize) {
    ("saved-loop", index)
}

/// The loop buttons: a length from the beat at or before the playhead,
/// half and double, leave and save.
pub(crate) fn loop_bar(
    track: &Track,
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let palette = app.theme.palette;
    let can_loop = app.can_loop_selection();
    let active = app.selected_loop();
    let is_looping = active.is_some();
    let snaps = LoopGrid::of(track).snaps;
    let label = |text: &'static str| {
        div()
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_secondary)
            .child(text)
    };
    let lengths = LOOP_BEATS.map(|beats| {
        let is_pressed = active.and_then(|active| active.beats) == Some(beats);
        let explanation: SharedString = if !can_loop {
            "Play the track to loop it.".into()
        } else if is_pressed {
            "Leave the loop: playing carries on past its end.".into()
        } else if is_looping {
            format!("Make the loop {} long from its start.", length_words(beats)).into()
        } else if snaps {
            format!(
                "Loop {} from the beat at or before the playhead.",
                length_words(beats)
            )
            .into()
        } else {
            format!(
                "Loop {} from the playhead. Without a beat grid the loop is not snapped to a beat.",
                length_words(beats)
            )
            .into()
        };
        pressed_button(length_id(beats), length_label(beats), is_pressed, &palette)
            .when(!can_loop, |button| button.opacity(0.5).cursor_default())
            .tooltip(move |window, cx| Tooltip::new(explanation.clone()).build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_beat_loop(beats, cx)))
            .test_support()
    });
    let [half_beat, one_beat, bars @ ..] = lengths;
    let command = |id: &'static str,
                   text: &'static str,
                   explanation: &'static str,
                   is_enabled: bool,
                   action: fn(&mut EuphoriaApp, &mut Context<'_, EuphoriaApp>),
                   cx: &mut Context<'_, EuphoriaApp>| {
        pressed_button(id, text, false, &palette)
            .when(!is_enabled, |button| button.opacity(0.5).cursor_default())
            .tooltip(move |window, cx| Tooltip::new(explanation).build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| action(this, cx)))
            .test_support()
    };
    let can_save = active.is_some_and(|active| {
        !track.saved_loops.iter().any(|saved| {
            (saved.start_seconds - active.start_seconds).abs() < 1e-6
                && (saved.end_seconds - active.end_seconds).abs() < 1e-6
        })
    });
    let summary = active.map(|active| {
        div()
            .id("focus-loop-summary")
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.accent)
            .child(format!(
                "Looping {} to {}",
                format::cue_time(active.start_seconds),
                format::cue_time(active.end_seconds)
            ))
            .test_support()
    });
    div()
        .id("focus-loop-bar")
        .flex()
        .flex_wrap()
        .items_center()
        .gap(GAP_TIGHT)
        .child(label("Loop"))
        .child(half_beat)
        .child(one_beat)
        .child(label("beat"))
        .children(bars)
        .child(label("bars"))
        .child(div().w(GAP_SMALL))
        .child(command(
            "focus-loop-halve",
            "\u{bd}\u{d7}",
            "Halve the loop (\u{2318}\u{2190}).",
            is_looping,
            |this, cx| this.resize_loop(0.5, cx),
            cx,
        ))
        .child(command(
            "focus-loop-double",
            "2\u{d7}",
            "Double the loop (\u{2318}\u{2192}).",
            is_looping,
            |this, cx| this.resize_loop(2.0, cx),
            cx,
        ))
        .child(command(
            "focus-loop-exit",
            "Exit loop",
            "Leave the loop: playing carries on past its end (L or Esc).",
            is_looping,
            EuphoriaApp::exit_loop,
            cx,
        ))
        .child(command(
            "focus-loop-save",
            "Save loop",
            "Keep this loop with the track's cues, to play it again with a click.",
            can_save,
            EuphoriaApp::save_loop,
            cx,
        ))
        .child(div().w(GAP_SMALL))
        .children(summary)
        .test_support()
        .into_any_element()
}

/// The loop on the detail: a tinted stretch with a grip at each end, the
/// loop being drawn or moved while the button is held.
pub(crate) fn detail_overlay(
    app: &EuphoriaApp,
    viewport: &Viewport,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<AnyElement> {
    let (start, end) = app.shown_loop()?;
    if end <= viewport.start || start >= viewport.end() {
        return None;
    }
    let palette = app.theme.palette;
    let left = viewport.fraction_of(start).clamp(0.0, 1.0);
    let right = viewport.fraction_of(end).clamp(0.0, 1.0);
    let grip = |id: &'static str, edge: LoopEdge, palette: &Palette| {
        div()
            .id(id)
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(EDGE_GRIP_WIDTH))
            .cursor_col_resize()
            .child(
                div()
                    .absolute()
                    .top_0()
                    .h(px(14.0))
                    .w(px(EDGE_GRIP_WIDTH))
                    .rounded(RADIUS_SMALL)
                    .bg(palette.accent),
            )
            .tooltip(|window, cx| {
                Tooltip::new(
                    "Drag to move this end of the loop: it lands on a beat, or anywhere with Option held.",
                )
                    .build(window, cx)
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.press_loop_edge(edge);
                    cx.stop_propagation();
                }),
            )
            .test_support()
    };
    let shows_in = viewport.contains(start);
    let shows_out = viewport.contains(end);
    let half_grip = px(EDGE_GRIP_WIDTH / 2.0);
    Some(
        div()
            .id("focus-loop-region")
            .absolute()
            .top_0()
            .bottom_0()
            .left(relative(left))
            .w(relative((right - left).max(0.0)))
            .bg(palette.accent.opacity(0.16))
            .when(shows_in, |region| region.border_l_2())
            .when(shows_out, |region| region.border_r_2())
            .border_color(palette.accent)
            .when(shows_in, |region| {
                region.child(grip("focus-loop-in", LoopEdge::In, &palette).left(-half_grip))
            })
            .when(shows_out, |region| {
                region.child(grip("focus-loop-out", LoopEdge::Out, &palette).right(-half_grip))
            })
            .test_support()
            .into_any_element(),
    )
}

/// The loop on the overview, so it shows wherever the detail is.
pub(crate) fn overview_tint(app: &EuphoriaApp, duration: f64) -> Option<Div> {
    let (start, end) = app.shown_loop()?;
    if duration <= 0.0 {
        return None;
    }
    let palette = app.theme.palette;
    let left = (start / duration).clamp(0.0, 1.0) as f32;
    let right = (end / duration).clamp(0.0, 1.0) as f32;
    Some(
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .left(relative(left))
            .w(relative((right - left).max(0.0)))
            .bg(palette.accent.opacity(0.3)),
    )
}

/// The loops saved with `track`: a click jumps to one and loops it.
pub(crate) fn saved_loops(
    track: &Track,
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<AnyElement> {
    if track.saved_loops.is_empty() {
        return None;
    }
    let palette = app.theme.palette;
    let playing = app.selected_loop().map(|active| active.region());
    let hover_background = palette.raised;
    let hover_colour = palette.text;
    let rows = track.saved_loops.iter().enumerate().map(|(index, saved)| {
        let is_playing = playing.is_some_and(|(start, end)| {
            (start - saved.start_seconds).abs() < 1e-6 && (end - saved.end_seconds).abs() < 1e-6
        });
        let name = if saved.name.is_empty() {
            "Loop".to_string()
        } else {
            saved.name.clone()
        };
        let delete = div()
            .id(("saved-loop-delete", index))
            .flex_shrink_0()
            .px(px(4.0))
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_tertiary)
            .cursor_pointer()
            .hover(move |style| style.text_color(hover_colour))
            .child("\u{d7}")
            .tooltip(|window, cx| Tooltip::new("Delete this loop").build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.delete_saved_loop(index, cx);
            }))
            .test_support();
        div()
            .id(saved_loop_id(index))
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(GAP_SMALL)
            .h(px(24.0))
            .px(px(4.0))
            .rounded(RADIUS_SMALL)
            .cursor_pointer()
            .hover(move |style| style.bg(hover_background))
            .child(
                div()
                    .flex_shrink_0()
                    .w(px(22.0))
                    .text_size(LABEL_TEXT_SIZE)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if is_playing {
                        palette.accent
                    } else {
                        palette.text_tertiary
                    })
                    .child("\u{27f3}"),
            )
            .child(div().flex_1().min_w_0().truncate().child(name))
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_secondary)
                    .child(format!(
                        "{} \u{2013} {}",
                        format::cue_time(saved.start_seconds),
                        format::cue_time(saved.end_seconds)
                    )),
            )
            .child(delete)
            .tooltip(|window, cx| Tooltip::new("Click to jump here and loop").build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| this.play_saved_loop(index, cx)))
            .test_support()
    });
    Some(
        div()
            .id("saved-loops")
            .flex()
            .flex_col()
            .gap(GAP_TIGHT)
            .child(section_label("Loops", &palette))
            .children(rows)
            .test_support()
            .into_any_element(),
    )
}
