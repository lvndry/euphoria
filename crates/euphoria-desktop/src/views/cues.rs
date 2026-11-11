// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The cues of the selected track: a marker for each above the waveform,
//! lined up with it, and the list where cues are added, renamed, put on a
//! hot cue pad, deleted, undone and reset to the automatic ones.

use euphoria_core::{Cue, HotCue};
use gpui::component::input::Input;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, FontWeight, MouseButton, MouseDownEvent, Pixels, SharedString,
    TestSupportExt, div, px, relative,
};

use crate::app::EuphoriaApp;
use crate::format;
use crate::library::Track;
use crate::theme::Palette;
use crate::views::{LABEL_TEXT_SIZE, SMALL_TEXT_SIZE, section_label};

/// Height of the row of markers above the waveform.
pub(crate) const CUE_LANE_HEIGHT: f32 = 18.0;

/// Past this far along the waveform a marker's label sits left of its
/// line, so it stays inside the waveform's width.
const LABEL_FLIP_FRACTION: f32 = 0.8;

/// Height of a row of the list of cues.
const CUE_ROW_HEIGHT: f32 = 26.0;

/// The id of the marker of cue `index` above the waveform.
pub(crate) fn marker_id(index: usize) -> (&'static str, usize) {
    ("cue-marker", index)
}

/// The cues of `track` as they are drawn: a cue being dragged shows where
/// it would land.
pub(crate) fn shown_cues(track: &Track, app: &EuphoriaApp) -> Vec<Cue> {
    let mut cues = track.cues().cues;
    if let Some(drag) = &app.cue_editor.drag
        && drag.track == track.path
        && let (Some(seconds), Some(cue)) = (drag.seconds, cues.get_mut(drag.index))
    {
        cue.seconds = seconds;
    }
    cues
}

/// How far along the track each of `cues` is, from 0.0 to 1.0.
pub(crate) fn fractions(cues: &[Cue], duration_seconds: f32) -> Vec<f32> {
    if duration_seconds <= 0.0 {
        return Vec::new();
    }
    cues.iter()
        .map(|cue| (cue.seconds as f32 / duration_seconds).clamp(0.0, 1.0))
        .collect()
}

/// The row of markers above the waveform, as wide as it: a press on a
/// marker plays from its cue, and a drag moves it.
pub(crate) fn lane(
    cues: &[Cue],
    duration_seconds: f32,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let markers = cues
        .iter()
        .zip(fractions(cues, duration_seconds))
        .enumerate()
        .map(|(index, (cue, fraction))| {
            let label = match cue.hot_cue {
                Some(hot_cue) => format!("{hot_cue} {}", cue.name),
                None => cue.name.clone(),
            };
            let explanation: SharedString = format!(
                "{} at {}. Click to play from here, drag to move it: it lands on a beat, or on a bar with Shift held.",
                cue_title(cue),
                format::cue_time(cue.seconds)
            )
            .into();
            let marker = div()
                .id(marker_id(index))
                .absolute()
                .top_0()
                .h(px(CUE_LANE_HEIGHT))
                .flex()
                .items_center()
                .px(px(4.0))
                .max_w(px(140.0))
                .bg(palette.raised)
                .text_size(LABEL_TEXT_SIZE)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(palette.text)
                .whitespace_nowrap()
                .overflow_hidden()
                .cursor_pointer()
                .child(label)
                .tooltip(move |window, cx| Tooltip::new(explanation.clone()).build(window, cx))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        this.press_cue(index, event.position.x);
                        cx.stop_propagation();
                    }),
                );
            if fraction > LABEL_FLIP_FRACTION {
                marker
                    .right(relative(1.0 - fraction))
                    .border_r_2()
                    .border_color(palette.accent)
            } else {
                marker
                    .left(relative(fraction))
                    .border_l_2()
                    .border_color(palette.accent)
            }
            .test_support()
        });
    div()
        .id("cue-lane")
        .relative()
        .w_full()
        .h(px(CUE_LANE_HEIGHT))
        .overflow_hidden()
        .children(markers)
        .test_support()
        .into_any_element()
}

/// The name of a cue as the interface says it.
fn cue_title(cue: &Cue) -> String {
    match (cue.name.is_empty(), cue.hot_cue) {
        (false, _) => cue.name.clone(),
        (true, Some(hot_cue)) => format!("Hot cue {hot_cue}"),
        (true, None) => "Memory cue".to_string(),
    }
}

/// The list of the selected track's cues with the commands that change
/// them. `list_height` limits the height of the list, in the strip under
/// the table.
pub(crate) fn section(
    track: &Track,
    app: &EuphoriaApp,
    list_height: Option<Pixels>,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Div {
    let palette = app.theme.palette;
    let track_cues = track.cues();
    let can_undo = app.cue_editor.history.can_undo(&track.path);
    let command = |id: &'static str,
                   label: &'static str,
                   explanation: &'static str,
                   cx: &mut Context<'_, EuphoriaApp>,
                   action: fn(&mut EuphoriaApp, &mut Context<'_, EuphoriaApp>)| {
        let hover_colour = palette.text;
        div()
            .id(id)
            .whitespace_nowrap()
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_secondary)
            .cursor_pointer()
            .hover(move |style| style.text_color(hover_colour))
            .child(label)
            .tooltip(move |window, cx| Tooltip::new(explanation).build(window, cx))
            .on_click(cx.listener(move |this, _, _, cx| action(this, cx)))
            .test_support()
    };

    let heading = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(10.0))
        .child(section_label("Cues", &palette))
        .when(track_cues.is_edited(), |heading| {
            heading.child(
                div()
                    .id("cues-set-by-you")
                    .text_size(LABEL_TEXT_SIZE)
                    .text_color(palette.accent)
                    .child("by you")
                    .tooltip(|window, cx| {
                        Tooltip::new(
                            "You edited these cues. They are used everywhere in place of the automatic ones, and kept when the track is analyzed again.",
                        )
                        .build(window, cx)
                    })
                    .test_support(),
            )
        })
        .child(div().flex_1())
        .child(command(
            "add-cue",
            "Add at playhead",
            "Add a cue where playing is, on the nearest beat. Shortcut: C. A double click on the waveform adds one there.",
            cx,
            EuphoriaApp::add_cue_at_playhead,
        ))
        .when(can_undo, |heading| {
            heading.child(command(
                "undo-cue-edit",
                "Undo",
                "Undo the last change to these cues.",
                cx,
                EuphoriaApp::undo_cue_edit,
            ))
        })
        .when(track_cues.is_edited(), |heading| {
            heading.child(command(
                "reset-cues",
                "Reset to automatic cues",
                "Forget your cues and use the ones analysis placed.",
                cx,
                EuphoriaApp::reset_cues,
            ))
        });

    let mut rows = Vec::with_capacity(track_cues.cues.len());
    for (index, cue) in track_cues.cues.iter().enumerate() {
        rows.push(cue_row(index, cue, app, &palette, cx).into_any_element());
        if app.cue_editor.choosing_hot_cue == Some(index) {
            rows.push(hot_cue_choice(index, &track_cues.cues, &palette, cx).into_any_element());
        }
    }
    let note = if track_cues.cues.is_empty() {
        Some("No cues. Double click the waveform, or add one at the playhead.")
    } else if !track_cues.is_edited() {
        Some("Placed by analysis on the bars of the beat grid. Any change makes them yours.")
    } else {
        None
    };
    let list = div()
        .id("cue-list")
        .flex()
        .flex_col()
        .gap(px(2.0))
        .when_some(list_height, |list, height| {
            list.max_h(height).overflow_y_scroll()
        })
        .children(note.map(|note| {
            div()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child(note)
        }))
        .children(rows);

    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(heading)
        .child(list)
}

/// One cue: its hot cue pad, its name, its time and a way to delete it.
fn cue_row(
    index: usize,
    cue: &Cue,
    app: &EuphoriaApp,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> impl IntoElement {
    let hover_background = palette.raised;
    let hover_colour = palette.text;
    let pad = div()
        .id(("cue-hot-cue", index))
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .w(px(22.0))
        .h(px(20.0))
        .rounded(px(4.0))
        .text_size(LABEL_TEXT_SIZE)
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .map(|pad| match cue.hot_cue {
            Some(hot_cue) => pad
                .bg(palette.accent)
                .text_color(palette.accent_ink)
                .child(hot_cue.to_string()),
            None => pad
                .border_1()
                .border_color(palette.line)
                .text_color(palette.text_tertiary)
                .child("\u{2013}"),
        })
        .tooltip(|window, cx| {
            Tooltip::new(
                "The hot cue pad, A to H. Click to choose another, or none for a memory cue only.",
            )
            .build(window, cx)
        })
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_hot_cue_choice(index, cx)))
        .test_support();

    let name = if app.cue_editor.renaming == Some(index) {
        div()
            .flex_1()
            .min_w_0()
            .child(Input::new(&app.cue_name_input).id("cue-name-input"))
            .into_any_element()
    } else {
        div()
            .id(("cue-name", index))
            .flex_1()
            .min_w_0()
            .truncate()
            .cursor_pointer()
            .rounded(px(4.0))
            .px(px(2.0))
            .hover(move |style| style.bg(hover_background))
            .when(cue.name.is_empty(), |name| {
                name.text_color(palette.text_tertiary)
            })
            .child(if cue.name.is_empty() {
                cue_title(cue)
            } else {
                cue.name.clone()
            })
            .tooltip(|window, cx| Tooltip::new("Click to rename").build(window, cx))
            .on_click(cx.listener(move |this, _, window, cx| {
                this.start_renaming_cue(index, window, cx);
            }))
            .test_support()
            .into_any_element()
    };

    div()
        .id(("cue-row", index))
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(8.0))
        .h(px(CUE_ROW_HEIGHT))
        .child(pad)
        .child(name)
        .child(
            div()
                .id(("cue-time", index))
                .flex_shrink_0()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_secondary)
                .cursor_pointer()
                .hover(move |style| style.text_color(hover_colour))
                .child(format::cue_time(cue.seconds))
                .tooltip(|window, cx| Tooltip::new("Play from here").build(window, cx))
                .on_click(cx.listener(move |this, _, _, cx| this.play_from_cue(index, cx)))
                .test_support(),
        )
        .child(
            div()
                .id(("cue-delete", index))
                .flex_shrink_0()
                .px(px(4.0))
                .text_color(palette.text_tertiary)
                .cursor_pointer()
                .hover(move |style| style.text_color(hover_colour))
                .child("\u{00d7}")
                .tooltip(|window, cx| Tooltip::new("Delete this cue").build(window, cx))
                .on_click(cx.listener(move |this, _, _, cx| this.delete_cue(index, cx)))
                .test_support(),
        )
        .test_support()
}

/// The pads to put cue `index` on, A to H, and none. A pad another cue
/// holds is marked; taking it swaps the two cues' pads.
fn hot_cue_choice(
    index: usize,
    cues: &[Cue],
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> impl IntoElement {
    let current = cues.get(index).and_then(|cue| cue.hot_cue);
    let hover_background = palette.line;
    let choice = |id: (&'static str, usize), label: String, is_current: bool, is_taken: bool| {
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .min_w(px(22.0))
            .h(px(20.0))
            .px(px(4.0))
            .rounded(px(4.0))
            .text_size(LABEL_TEXT_SIZE)
            .font_weight(FontWeight::SEMIBOLD)
            .cursor_pointer()
            .border_1()
            .border_color(palette.line)
            .hover(move |style| style.bg(hover_background))
            .map(|choice| {
                if is_current {
                    choice.bg(palette.accent).text_color(palette.accent_ink)
                } else if is_taken {
                    choice.text_color(palette.text_tertiary)
                } else {
                    choice.text_color(palette.text)
                }
            })
            .child(label)
    };
    let pads = HotCue::ALL.map(|hot_cue| {
        let is_taken = cues.iter().any(|cue| cue.hot_cue == Some(hot_cue));
        choice(
            ("cue-hot-cue-choice", usize::from(hot_cue.pad())),
            hot_cue.to_string(),
            current == Some(hot_cue),
            is_taken,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.set_cue_hot_cue(index, Some(hot_cue), cx);
        }))
        .test_support()
    });
    div()
        .id("cue-hot-cue-choices")
        .flex()
        .flex_wrap()
        .gap(px(4.0))
        .pl(px(30.0))
        .pb(px(4.0))
        .children(pads)
        .child(
            choice(
                ("cue-hot-cue-none", index),
                "None".to_string(),
                current.is_none(),
                false,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_cue_hot_cue(index, None, cx)))
            .test_support(),
        )
        .test_support()
}
