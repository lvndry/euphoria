// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The detail of the selected track: what it is, its key and tempo, its
//! waveform and, when analysis failed, why. Shown as a column on the right
//! or as a strip under the table, depending on the theme's layout.

use euphoria_core::Confidence;
use euphoria_core::harmony::Relation;
use gpui::component::Icon;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, FontWeight, MouseButton, MouseDownEvent, Pixels, SharedString,
    Stateful, TestSupportExt, Window, div, px,
};

use crate::app::EuphoriaApp;
use crate::assets::MUSIC_ICON_PATH;
use crate::camelot::CamelotPosition;
use crate::decks::{SuggestionFilter, SuggestionScope};
use crate::format;
use crate::library::{Track, TrackStatus};
use crate::mixes::keys_related_by;
use crate::theme::{AppTheme, Layout, Palette};
use crate::track_overview::{EnergyCurve, key_stretches, shows_key_timeline};
use crate::views::by_ear;
use crate::views::cues;
use crate::views::favorites::star_button;
use crate::views::key_chip::key_chip;
use crate::views::key_shift;
use crate::views::player::{self, seek_on_press_and_drag};
use crate::views::stems;
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
    /// How tall to draw the waveform when `others` is taken by the rows
    /// around it: the cue markers above, the energy curve and the key
    /// timeline under it. The usual height in the column, and as tall as
    /// the strip allows under its heading.
    fn waveform_height(self, others: f32) -> Pixels {
        match self {
            Room::Column => px(WAVEFORM_HEIGHT),
            Room::Strip { inner_height } => {
                px((inner_height - HEADING_HEIGHT - others).max(SMALLEST_STRIP_WAVEFORM))
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
        .child(sized(
            summary(track, &palette)
                .child(div().flex_1())
                .child(track_star(app, cx)),
            Some(220.0),
        ))
        .child(sized(statistics(track, app, cx), Some(264.0)))
        .children(
            key_wheel(track, theme, &app.decks.filter.mix_options().relations)
                .map(|section| sized(divided(section), Some(220.0))),
        )
        .child(sized(stems::section(track, app, cx), None))
        .children(
            by_ear::section(track, app, cx).map(|section| sized(divided(section), Some(300.0))),
        )
        .children(
            key_shift::section(track, app, cx).map(|section| sized(divided(section), Some(300.0))),
        )
        .child(sized(waveform_section(track, app, room, cx), None))
        .children(
            cue_section(track, app, room, cx).map(|section| sized(divided(section), Some(280.0))),
        )
        .children(status(track, &palette).map(|section| sized(divided(section), Some(280.0))))
        .children(quality(track, &palette).map(|section| sized(divided(section), Some(280.0))))
        .children(mixes(app, room, cx).map(|section| sized(divided(section), Some(300.0))))
        .when(is_column, |inspector| {
            inspector.child(divided(location(track, &palette)))
        })
        .test_support()
        .into_any_element()
}

/// The star of the selected track.
fn track_star(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> impl IntoElement {
    let is_starred = app
        .selected_track()
        .is_some_and(|track| app.session.favorites().is_starred(&track.path));
    star_button(
        "inspector-star",
        is_starred,
        if is_starred {
            "Take the star off (F)"
        } else {
            "Star (F)"
        },
        &app.theme.palette,
    )
    .on_click(cx.listener(|this, _, _, cx| {
        if let Some(index) = this.selected {
            this.toggle_star_on_track(index, cx);
        }
    }))
    .test_support()
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

/// The key, the tempo and the energy in three boxes. The key and the tempo
/// say quietly how far to trust them, and, when the core offers one, what
/// else they could be: the runner up key when the key is not rated high,
/// and half or double the tempo. A click on either takes it instead. A
/// value the user set says so, with a way back to the detected one.
fn statistics(track: &Track, app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let theme = &app.theme;
    let palette = &theme.palette;
    let notation = app.display.key_notation;
    let not_known_yet = match track.status {
        TrackStatus::Queued => "queued",
        TrackStatus::Analyzing => "analyzing",
        TrackStatus::Failed(_) => "not analyzed",
        TrackStatus::Analyzed(_) => "not detected",
    };
    let analysis = track.analysis();
    let correction = track.correction;
    let key_confidence = analysis.and_then(|analysis| analysis.key_confidence());
    let tempo_confidence = analysis.and_then(|analysis| analysis.tempo_confidence());
    let runner_up = shown_runner_up(track);
    let tempo_alternative = analysis
        .and_then(|analysis| analysis.tempo_alternative_in(app.session.library().tempo_range()))
        .filter(|_| correction.bpm.is_none());

    let (key_code, key_note) = match (track.key(), correction.key) {
        (Some(key), Some(_)) => {
            let back_to = match track.detected_key() {
                Some(detected) => format!("reset to {}", notation.spell(&detected)),
                None => "reset".to_string(),
            };
            (
                notation.spell(&key),
                reset_note("key-reset", back_to, palette, cx, |this, cx| {
                    this.reset_key(cx)
                }),
            )
        }
        (Some(key), None) => (
            notation.spell(&key),
            quiet_note(key.to_string(), palette).into_any_element(),
        ),
        (None, _) => (
            "\u{2013}".to_string(),
            quiet_note(not_known_yet, palette).into_any_element(),
        ),
    };
    let key_value = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.0))
        .child(statistic_value(key_code))
        .children(runner_up.map(|runner_up| {
            let description: SharedString = format!(
                "The key that matched second best, {runner_up}. Compare the two by ear, and click to use it instead."
            )
            .into();
            let hover_background = palette.raised;
            div()
                .id("runner-up-key")
                .flex()
                .items_center()
                .gap(px(4.0))
                .px(px(2.0))
                .rounded(px(4.0))
                .cursor_pointer()
                .hover(move |style| style.bg(hover_background))
                .text_size(LABEL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child("or")
                .child(key_chip(&runner_up, theme, notation))
                .tooltip(move |window, cx| Tooltip::new(description.clone()).build(window, cx))
                .on_click(cx.listener(move |this, _, _, cx| this.accept_key(runner_up, cx)))
                .test_support()
        }));

    let (tempo, tempo_note) = match track.bpm() {
        Some(bpm) => (
            format::bpm(bpm),
            match (correction.bpm, tempo_alternative, track.detected_bpm()) {
                (Some(_), _, detected) => reset_note(
                    "tempo-reset",
                    match detected {
                        Some(detected) => format!("reset to {}", format::bpm(detected)),
                        None => "reset".to_string(),
                    },
                    palette,
                    cx,
                    |this, cx| this.reset_bpm(cx),
                ),
                (None, Some(alternative), _) => {
                    let hover_colour = palette.text;
                    let description: SharedString = format!(
                        "The beat could also be counted at {}. Click to use it instead.",
                        format::bpm(alternative)
                    )
                    .into();
                    quiet_note(format!("or {}", format::bpm(alternative)), palette)
                        .id("tempo-alternative")
                        .cursor_pointer()
                        .hover(move |style| style.text_color(hover_colour))
                        .tooltip(move |window, cx| {
                            Tooltip::new(description.clone()).build(window, cx)
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.accept_bpm(alternative, cx);
                        }))
                        .test_support()
                        .into_any_element()
                }
                _ => quiet_note("per minute", palette).into_any_element(),
            },
        ),
        None => (
            "\u{2013}".to_string(),
            quiet_note(not_known_yet, palette).into_any_element(),
        ),
    };
    let (energy, energy_note) = match track.energy() {
        Some(rating) => (rating.to_string(), "of 10"),
        None if track.analysis().is_some() => ("\u{2013}".to_string(), "not rated"),
        None => ("\u{2013}".to_string(), not_known_yet),
    };

    let key_trust = match (correction.key, key_confidence) {
        (Some(_), _) => Some(set_by_you("key-set-by-you", "key", palette)),
        (None, Some(confidence)) => Some(confidence_word(
            "Key",
            "key-confidence",
            confidence,
            palette,
        )),
        (None, None) => None,
    };
    let tempo_trust = match (correction.bpm, tempo_confidence) {
        (Some(_), _) => Some(set_by_you("tempo-set-by-you", "tempo", palette)),
        (None, Some(confidence)) => Some(confidence_word(
            "BPM",
            "tempo-confidence",
            confidence,
            palette,
        )),
        (None, None) => None,
    };

    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .flex()
                .rounded(px(8.0))
                .border_1()
                .border_color(palette.line)
                .child(statistic("Key", key_trust, key_value, key_note, palette))
                .child(
                    statistic(
                        "BPM",
                        tempo_trust,
                        statistic_value(tempo),
                        tempo_note,
                        palette,
                    )
                    .border_l_1()
                    .border_color(palette.line),
                )
                .child(
                    statistic(
                        "Energy",
                        None,
                        statistic_value(energy),
                        quiet_note(energy_note, palette).into_any_element(),
                        palette,
                    )
                    .border_l_1()
                    .border_color(palette.line),
                ),
        )
        .when(analysis.is_some(), |section| {
            section.child(by_ear::corrector(app, cx))
        })
}

/// The runner up key, offered beside the key when the key is not rated
/// high and the user has not set one.
pub(crate) fn shown_runner_up(track: &Track) -> Option<euphoria_core::Key> {
    let analysis = track.analysis()?;
    let is_doubtful = analysis
        .key_confidence()
        .is_some_and(|confidence| confidence != Confidence::High);
    analysis
        .key_runner_up
        .filter(|_| is_doubtful && track.correction.key.is_none())
}

/// The value of a box, large.
fn statistic_value(value: impl Into<SharedString>) -> Div {
    div()
        .whitespace_nowrap()
        .text_size(px(20.0))
        .font_weight(FontWeight::SEMIBOLD)
        .child(value.into())
}

/// The quiet line under the value of a box.
fn quiet_note(text: impl Into<SharedString>, palette: &Palette) -> Div {
    div()
        .truncate()
        .text_size(LABEL_TEXT_SIZE)
        .text_color(palette.text_tertiary)
        .child(text.into())
}

/// How far to trust a detected value, in a word, explained on hover.
fn confidence_word(
    label: &'static str,
    id: &'static str,
    confidence: Confidence,
    palette: &Palette,
) -> AnyElement {
    let (colour, explanation): (_, SharedString) = match confidence {
        Confidence::High => (
            palette.text_tertiary,
            format!("{label} confidence: high. The detector clearly preferred this answer.").into(),
        ),
        Confidence::Medium => (
            palette.text_tertiary,
            format!("{label} confidence: medium. Usually right, worth a listen if it matters.")
                .into(),
        ),
        Confidence::Low => (
            palette.danger,
            format!(
                "{label} confidence: low. Check it by ear; the track is listed under Needs review."
            )
            .into(),
        ),
    };
    div()
        .id(id)
        .text_color(colour)
        .child(confidence.as_str())
        .tooltip(move |window, cx| Tooltip::new(explanation.clone()).build(window, cx))
        .test_support()
        .into_any_element()
}

/// Says that the user set the value, which wins over the detected one.
fn set_by_you(id: &'static str, what: &'static str, palette: &Palette) -> AnyElement {
    let explanation: SharedString = format!(
        "You set this {what}. It is used everywhere in place of the detected one, and kept when the track is analyzed again."
    )
    .into();
    div()
        .id(id)
        .whitespace_nowrap()
        .text_color(palette.accent)
        .child("by you")
        .tooltip(move |window, cx| Tooltip::new(explanation.clone()).build(window, cx))
        .test_support()
        .into_any_element()
}

/// The quiet line under a value the user set: a click on it goes back to
/// the detected value it names.
fn reset_note(
    id: &'static str,
    text: String,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
    reset: impl Fn(&mut EuphoriaApp, &mut Context<'_, EuphoriaApp>) + 'static,
) -> AnyElement {
    let hover_colour = palette.text;
    quiet_note(text, palette)
        .id(id)
        .cursor_pointer()
        .hover(move |style| style.text_color(hover_colour))
        .on_click(cx.listener(move |this, _, _, cx| reset(this, cx)))
        .test_support()
        .into_any_element()
}

/// One box: its label, with how far to trust the value beside it, the
/// value, and a quiet note under it.
fn statistic(
    label: &'static str,
    trust: Option<AnyElement>,
    value: Div,
    note: AnyElement,
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
                .flex()
                .justify_between()
                .gap(px(6.0))
                .text_size(LABEL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child(label)
                .children(trust),
        )
        .child(value)
        .child(note)
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
    // The timeline is the detector's. Once the user corrects the key, the
    // detector's sections are the very thing they disagreed with, and a
    // strip of them under a different key reads as a contradiction, so it
    // is hidden until the correction is reset.
    let key_timeline = track.analysis().and_then(|analysis| {
        if track.correction.key.is_some()
            || !shows_key_timeline(analysis.key, &analysis.key_timeline)
        {
            return None;
        }
        let stretches = key_stretches(&analysis.key_timeline, analysis.duration_seconds);
        (!stretches.is_empty()).then(|| {
            track_overview::key_timeline(
                &stretches,
                analysis.duration_seconds,
                progress,
                &app.theme,
                app.display.key_notation,
            )
        })
    });
    let shown_cues = cues::shown_cues(track, app);
    let duration_seconds = track.duration_seconds().unwrap_or(0.0);
    let cue_fractions = cues::fractions(&shown_cues, duration_seconds);
    // The waveform's width as last drawn; the lane is as wide.
    let lane_width = app
        .waveform_bounds
        .get()
        .map(|bounds| bounds.size.width / px(1.0));
    // Saved cues can be loaded before a queued track's duration is known.
    // They have no waveform positions until that analysis arrives.
    let marker_places = if cue_fractions.is_empty() {
        Vec::new()
    } else {
        cues::marker_places(&shown_cues, &cue_fractions, lane_width)
    };
    let cue_lane = (!marker_places.is_empty())
        .then(|| cues::lane(&shown_cues, duration_seconds, &marker_places, &palette, cx));
    // The rows other than the waveform, which takes the height left.
    let others = energy
        .as_ref()
        .map_or(0.0, |_| track_overview::ENERGY_HEIGHT + WAVEFORM_ROW_GAP)
        + key_timeline.as_ref().map_or(0.0, |_| {
            track_overview::KEY_TIMELINE_HEIGHT + WAVEFORM_ROW_GAP
        })
        + cue_lane.as_ref().map_or(0.0, |_| {
            cues::lane_height(&marker_places) + WAVEFORM_ROW_GAP
        });

    let scrubber = seek_on_press_and_drag(
        div().id("waveform").child(
            div()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                        if event.click_count == 2 {
                            this.add_cue_at_pointer(event.position.x, event.modifiers, cx);
                        }
                    }),
                )
                .child(waveform(
                    columns,
                    WaveformColours::of(&palette),
                    progress,
                    cue_fractions,
                    app.waveform_bounds.clone(),
                    room.waveform_height(others),
                )),
        ),
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
        .children(cue_lane)
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

/// The cues of an analyzed track. The strip under the table has little
/// room, so there the list shows once the track has cues or the user's set,
/// and scrolls within the strip's height.
fn cue_section(
    track: &Track,
    app: &EuphoriaApp,
    room: Room,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<Div> {
    track.analysis()?;
    let list_height = match room {
        Room::Column => None,
        Room::Strip { inner_height } => {
            let track_cues = track.cues();
            if track_cues.cues.is_empty() && !track_cues.is_edited() {
                return None;
            }
            Some(px((inner_height - HEADING_HEIGHT).max(MIX_ROW_HEIGHT)))
        }
    };
    Some(cues::section(track, app, list_height, cx))
}

/// What a marked transition whose keys do not mix by the rules says in
/// place of its key relation.
const YOUR_TRANSITION: &str = "Your mix";

/// The tracks in the library that mix with the selected one, the marked
/// transitions out of it first.
fn mixes(app: &EuphoriaApp, room: Room, cx: &mut Context<'_, EuphoriaApp>) -> Option<Div> {
    let theme = &app.theme;
    let palette = theme.palette;
    if app.selected_track()?.key().is_none() && app.mixes.is_empty() {
        return None;
    }

    let is_narrowed = app.decks.filter != SuggestionFilter::default()
        || app.decks.scope != SuggestionScope::Library;
    let shown_count = room.mix_count(app.mixes.len());
    let rows: Vec<Stateful<Div>> = app
        .mixes
        .iter()
        .take(shown_count)
        .filter_map(|mix| {
            let track_index = mix.track_index;
            let track = app.session.library().track(track_index)?;
            let note: Option<SharedString> = app
                .selected_track()
                .and_then(|selected| {
                    app.session
                        .favorites()
                        .transition(&selected.path, &track.path)
                })
                .and_then(|transition| transition.note.clone())
                .map(Into::into);
            let star = star_button(
                ("mix-star", track_index),
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
                    .when_some(note, |row, note| {
                        row.tooltip(move |window, cx| Tooltip::new(note.clone()).build(window, cx))
                    })
                    .child(star)
                    .child(
                        div().flex().flex_shrink_0().w(px(38.0)).children(
                            track
                                .key()
                                .map(|key| key_chip(&key, theme, app.display.key_notation)),
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
                            .child(
                                mix.relation
                                    .map_or(YOUR_TRANSITION, |relation| relation.label()),
                            ),
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
                        .id("mixes-empty")
                        .text_color(palette.text_tertiary)
                        .child(if is_narrowed {
                            "No track in the suggestion pool mixes with this one. Widen the controls above the decks' suggestions."
                        } else {
                            "No track in the library mixes with this one yet."
                        })
                        .test_support(),
                )
            })
            .children(rows),
    )
}

/// The Camelot wheel, in themes that show it, for a track with a key.
fn key_wheel(track: &Track, theme: &AppTheme, relations: &[Relation]) -> Option<Div> {
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
                    keys_related_by(&key, relations)
                        .iter()
                        .map(CamelotPosition::of),
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
