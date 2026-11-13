// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The decks under the table: deck A and deck B side by side, each with its
//! waveform, beat grid, cues, energy and transport, the mixer between them,
//! and the tracks to play next under them.

use euphoria_core::harmony::{EnergyDirection, Relation};
use gpui::component::input::Input;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, FontWeight, MouseButton, MouseDownEvent, MouseMoveEvent,
    SharedString, Stateful, TestSupportExt, div, px, relative,
};

use crate::app::EuphoriaApp;
use crate::deck_audio::DeckId;
use crate::decks::{DeckMode, DeckView, SuggestionScope};
use crate::format;
use crate::player::fraction_along;
use crate::theme::Palette;
use crate::track_overview::EnergyCurve;
use crate::views::favorites;
use crate::views::key_chip::key_chip;
use crate::views::tag_panel::choice_button;
use crate::views::waveform::{SeenBounds, WaveformColours, waveform};
use crate::views::{SMALL_TEXT_SIZE, secondary_button, section_label};
use crate::views::{mashup, track_overview};
use crate::waveforms::Waveform;

/// Height of a deck's waveform.
const DECK_WAVEFORM_HEIGHT: f32 = 64.0;

/// Height of the list of tracks to play next.
const SUGGESTIONS_HEIGHT: f32 = 150.0;

/// The element id of a deck's part `name`.
pub(crate) fn deck_part(name: &'static str, deck: DeckId) -> (&'static str, usize) {
    (name, deck_number(deck))
}

fn deck_number(deck: DeckId) -> usize {
    match deck {
        DeckId::A => 0,
        DeckId::B => 1,
    }
}

/// The loop lengths a deck offers, in bars.
pub(crate) const LOOP_BARS: [u32; 5] = [1, 2, 4, 8, 16];

/// The element id of the button looping `bars` bars of `deck`.
pub(crate) fn loop_id(deck: DeckId, bars: u32) -> (&'static str, usize) {
    ("deck-loop", deck_number(deck) * 100 + bars as usize)
}

/// The element id of cue `cue_index` of `deck`.
pub(crate) fn cue_id(deck: DeckId, cue_index: usize) -> (&'static str, usize) {
    ("deck-cue", deck_number(deck) * 100 + cue_index)
}

/// Makes `strip` a control a press or a drag sets to the pointer's place
/// along it, from 0 at its left edge to 1 at its right.
fn press_and_drag<Set>(
    strip: Stateful<Div>,
    seen_bounds: &SeenBounds,
    cx: &mut Context<'_, EuphoriaApp>,
    set: Set,
) -> Stateful<Div>
where
    Set: Fn(&mut EuphoriaApp, f32, &mut Context<'_, EuphoriaApp>) + Clone + 'static,
{
    let set_on_drag = set.clone();
    let on_press = seen_bounds.clone();
    let on_drag = seen_bounds.clone();
    let along = |bounds: &SeenBounds, x: gpui::Pixels| {
        bounds.get().map(|bounds| {
            fraction_along(
                x / px(1.0),
                bounds.left() / px(1.0),
                bounds.size.width / px(1.0),
            )
        })
    };
    strip
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                if let Some(fraction) = along(&on_press, event.position.x) {
                    set(this, fraction, cx);
                }
                cx.stop_propagation();
            }),
        )
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            if event.pressed_button == Some(MouseButton::Left)
                && let Some(fraction) = along(&on_drag, event.position.x)
            {
                set_on_drag(this, fraction, cx);
            }
        }))
}

/// A horizontal fader at `value`, from 0 to 1.
pub(crate) fn fader<Set>(
    id: impl Into<gpui::ElementId>,
    value: f32,
    seen_bounds: &SeenBounds,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
    set: Set,
) -> AnyElement
where
    Set: Fn(&mut EuphoriaApp, f32, &mut Context<'_, EuphoriaApp>) + Clone + 'static,
{
    let bounds = seen_bounds.clone();
    let track = div()
        .id(id)
        .relative()
        .h(px(18.0))
        .w_full()
        .child(
            gpui::canvas(move |seen, _, _| bounds.set(Some(seen)), |_, _, _, _| ())
                .absolute()
                .size_full(),
        )
        .child(
            div()
                .absolute()
                .top(px(8.0))
                .left_0()
                .right_0()
                .h(px(2.0))
                .rounded(px(1.0))
                .bg(palette.line),
        )
        .child(
            div()
                .absolute()
                .top(px(3.0))
                .left(relative(value.clamp(0.0, 1.0)))
                .ml(px(-4.0))
                .w(px(8.0))
                .h(px(12.0))
                .rounded(px(3.0))
                .bg(palette.text_secondary),
        );
    press_and_drag(track, seen_bounds, cx, set)
        .test_support()
        .into_any_element()
}

/// The decks, the mixer and the suggestions.
pub(crate) fn render(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .gap(px(10.0))
        .p(px(12.0))
        .border_t_1()
        .border_color(palette.line)
        .bg(palette.panel)
        .child(
            div()
                .flex()
                .gap(px(12.0))
                .child(deck(app, DeckId::A, cx).test_support())
                .child(mixer(app, cx))
                .child(deck(app, DeckId::B, cx).test_support()),
        )
        .child(match app.decks.mode {
            DeckMode::NextTrack => suggestions(app, cx),
            DeckMode::Mashup => mashup::ideas(app, cx),
        })
        .children(favorites::deck_transition(app, cx))
}

fn deck(app: &EuphoriaApp, deck: DeckId, cx: &mut Context<'_, EuphoriaApp>) -> Stateful<Div> {
    let palette = app.theme.palette;
    let decks = &app.decks;
    let view = decks.view(deck);
    let library = app.session.library();
    let track = view.track_index.and_then(|index| library.track(index));
    let position = decks.position_seconds(deck);
    let progress = view
        .duration_seconds
        .filter(|duration| *duration > 0.0)
        .map(|duration| (position / duration).clamp(0.0, 1.0) as f32);
    let state = view.path.as_ref().and_then(|path| app.waveform_of(path));
    let columns = match state {
        Some(Waveform::Ready(columns, _)) => Some(columns.clone()),
        _ => None,
    };
    let energy = match state {
        Some(Waveform::Ready(_, Some(EnergyCurve::Steps(steps)))) if !steps.is_empty() => {
            Some(track_overview::energy_curve(steps, progress, &palette))
        }
        _ => None,
    };

    let title: SharedString = track
        .map(|track| track.title().to_string())
        .unwrap_or_else(|| "No track".to_string())
        .into();
    let tempo = view.bpm.map(|bpm| {
        let heard = bpm * view.rate;
        if (view.rate - 1.0).abs() < 1e-4 {
            format!("{} BPM", format::bpm(bpm as f32))
        } else {
            format!(
                "{} \u{2192} {} BPM",
                format::bpm(bpm as f32),
                format::bpm(heard as f32)
            )
        }
    });
    let header = div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .flex_shrink_0()
                .px(px(6.0))
                .rounded(px(4.0))
                .bg(palette.accent)
                .text_color(palette.accent_ink)
                .font_weight(FontWeight::SEMIBOLD)
                .child(deck.letter()),
        )
        .children(
            track
                .and_then(|track| track.key())
                .map(|key| key_chip(&key, &app.theme, app.display.key_notation)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .child(title),
        )
        .children(favorites::deck_star(app, deck, cx))
        .children(tempo.map(|tempo| {
            let out_of_reach = view.tempo_out_of_reach;
            div()
                .id(deck_part("deck-tempo", deck))
                .flex_shrink_0()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(if out_of_reach {
                    palette.danger
                } else {
                    palette.text_secondary
                })
                .child(if out_of_reach {
                    format!("{tempo}, not deck A's")
                } else {
                    tempo
                })
                .when(out_of_reach, |tempo| {
                    tempo.tooltip(|window, cx| {
                        Tooltip::new(
                            "Deck A's tempo is out of reach: a deck plays from half to twice its own speed, and this is the nearest.",
                        )
                        .build(window, cx)
                    })
                })
                .test_support()
        }));

    let waveform_area = waveform_with_grid(app, deck, view, columns, progress, cx);

    let cues = view.cues.iter().enumerate().map(|(cue_index, cue)| {
        let label: SharedString = format!(
            "{} {}",
            cue.section.name(),
            format::duration(cue.seconds as f32)
        )
        .into();
        choice_button(cue_id(deck, cue_index), label, false, &palette)
            .tooltip(|window, cx| {
                Tooltip::new("Play from this cue, aligning the upcoming downbeats when the other deck plays.")
                    .build(window, cx)
            })
            .on_click(cx.listener(move |this, _, _, cx| this.play_deck_cue(deck, cue_index, cx)))
            .test_support()
    });

    let play_label = if view.loading {
        "Loading…"
    } else if decks.is_waiting(deck) {
        "Waiting for the downbeat"
    } else if view.playing {
        "Pause"
    } else {
        "Play"
    };
    let duration = view
        .duration_seconds
        .map(|duration| format::duration(duration as f32))
        .unwrap_or_default();
    let has_grid = view.grid.is_some();
    let loops = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(4.0))
        .child(
            div()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_secondary)
                .child("Loop bars"),
        )
        .children(LOOP_BARS.map(|bars| {
            let is_looping = decks.is_looping(deck, bars);
            choice_button(loop_id(deck, bars), bars.to_string(), is_looping, &palette)
                .when(!has_grid, |button| button.opacity(0.5).cursor_default())
                .tooltip(move |window, cx| {
                    let text = if !has_grid {
                        "Looping needs the track's beat grid.".to_string()
                    } else if is_looping {
                        "Leave the loop: it plays to its end once more, then the track carries on."
                            .to_string()
                    } else {
                        format!(
                            "Loop {bars} {} from the next downbeat.",
                            if bars == 1 { "bar" } else { "bars" }
                        )
                    };
                    Tooltip::new(text).build(window, cx)
                })
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_deck_loop(deck, bars, cx)))
                .test_support()
        }));
    let transport = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.0))
        .child(
            choice_button(deck_part("deck-play", deck), play_label, view.playing, &palette)
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_deck_play(deck, cx)))
                .test_support(),
        )
        .child(
            div()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.text_secondary)
                .child(format!("{} / {duration}", format::duration(position as f32))),
        )
        .child(div().flex_1())
        .child(
            choice_button(deck_part("deck-keylock", deck), "Key lock", view.keylock, &palette)
                .tooltip(|window, cx| {
                    Tooltip::new(
                        "Keep the pitch when the tempo changes. Off, the pitch follows the speed, as on a turntable.",
                    )
                    .build(window, cx)
                })
                .on_click(cx.listener(move |this, _, _, cx| this.toggle_keylock(deck, cx)))
                .test_support(),
        )
        .when(deck == DeckId::B, |row| {
            row.child(
                choice_button("deck-sync", "Sync to A", app.decks.sync, &palette)
                    .tooltip(|window, cx| {
                        Tooltip::new("Play deck B at deck A's tempo, aligning their upcoming downbeats.")
                            .build(window, cx)
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_sync(cx)))
                    .test_support(),
            )
        })
        .child(
            secondary_button("Load selected", &palette)
                .id(deck_part("deck-load", deck))
                .on_click(cx.listener(move |this, _, _, cx| this.load_selected_on_deck(deck, cx)))
                .test_support(),
        );

    div()
        .id(deck_part("deck", deck))
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap(px(6.0))
        .child(header)
        .child(waveform_area)
        .children(energy)
        .child(div().flex().flex_wrap().gap(px(4.0)).children(cues))
        .child(mashup::deck_controls(app, deck, cx))
        .children(view.problem.clone().map(|problem| {
            div()
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.danger)
                .child(problem)
        }))
        .child(transport)
        .child(loops)
}

/// The waveform of a deck with the downbeats of its grid and its cues
/// marked on it; a press or a drag moves the deck there.
fn waveform_with_grid(
    app: &EuphoriaApp,
    deck: DeckId,
    view: &DeckView,
    columns: Option<std::sync::Arc<[crate::peaks::BandPeaks]>>,
    progress: Option<f32>,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let palette = app.theme.palette;
    let bounds = &app.decks.waveform_bounds[deck_number(deck)];
    let duration = view.duration_seconds.unwrap_or(0.0);
    let mut marks: Vec<AnyElement> = Vec::new();
    if let (Some(grid), true) = (view.grid, duration > 0.0) {
        let bar = grid.bar_seconds();
        let mut at = grid.first_downbeat_seconds;
        // One mark per bar, or per four bars on a long track, so marks stay
        // apart.
        let step = if duration / bar > 160.0 {
            bar * 4.0
        } else {
            bar
        };
        while at < duration {
            marks.push(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative((at / duration) as f32))
                    .w(px(1.0))
                    .bg(palette.text.opacity(0.12))
                    .into_any_element(),
            );
            at += step;
        }
    }
    if duration > 0.0
        && let Some((start, end)) = app.decks.loop_seconds(deck)
    {
        let left = (start / duration).clamp(0.0, 1.0) as f32;
        let right = (end / duration).clamp(0.0, 1.0) as f32;
        marks.push(
            div()
                .id(deck_part("deck-loop-region", deck))
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative(left))
                .w(relative((right - left).max(0.0)))
                .border_x_2()
                .border_color(palette.accent)
                .bg(palette.accent.opacity(0.18))
                .test_support()
                .into_any_element(),
        );
    }
    if duration > 0.0 {
        for cue in &view.cues {
            marks.push(
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative((cue.seconds / duration) as f32))
                    .w(px(2.0))
                    .bg(palette.accent)
                    .into_any_element(),
            );
        }
    }
    let strip = div()
        .id(deck_part("deck-waveform", deck))
        .relative()
        .h(px(DECK_WAVEFORM_HEIGHT))
        .child(waveform(
            columns,
            WaveformColours::of(&palette),
            progress,
            Vec::new(),
            bounds.clone(),
            px(DECK_WAVEFORM_HEIGHT),
        ))
        .children(marks);
    let seek: fn(&mut EuphoriaApp, f32, &mut Context<'_, EuphoriaApp>) = match deck {
        DeckId::A => |this, fraction, cx| this.seek_deck(DeckId::A, fraction, cx),
        DeckId::B => |this, fraction, cx| this.seek_deck(DeckId::B, fraction, cx),
    };
    press_and_drag(strip, bounds, cx, seek)
        .test_support()
        .into_any_element()
}

/// The faders of both decks and the crossfader between them.
fn mixer(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let decks = &app.decks;
    let label = |text: &'static str| {
        div()
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_tertiary)
            .child(text)
    };
    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .w(px(132.0))
        .gap(px(4.0))
        .child(section_label("Mixer", &palette))
        .child(label("Volume A"))
        .child(fader(
            deck_part("deck-volume", DeckId::A),
            decks.view(DeckId::A).volume,
            &decks.fader_bounds[0],
            &palette,
            cx,
            |this, value, cx| this.set_deck_volume(DeckId::A, value, cx),
        ))
        .child(label("Volume B"))
        .child(fader(
            deck_part("deck-volume", DeckId::B),
            decks.view(DeckId::B).volume,
            &decks.fader_bounds[1],
            &palette,
            cx,
            |this, value, cx| this.set_deck_volume(DeckId::B, value, cx),
        ))
        .child(label("A \u{2190} crossfader \u{2192} B"))
        .child(fader(
            "crossfader",
            decks.crossfader,
            &decks.crossfader_bounds,
            &palette,
            cx,
            |this, value, cx| this.set_crossfader(value, cx),
        ))
        .children(decks.load_readout().map(|readout| {
            div()
                .id("deck-load-readout")
                .pt(px(6.0))
                .text_size(px(10.0))
                .text_color(palette.text_tertiary)
                .child(readout)
                .test_support()
        }))
}

/// The tracks that mix best after the one on deck A; a click loads one on
/// deck B.
fn suggestions(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Div {
    let palette = app.theme.palette;
    let decks = &app.decks;
    let directions: Vec<_> = EnergyDirection::ALL
        .iter()
        .enumerate()
        .map(|(index, direction)| {
            let direction = *direction;
            choice_button(
                ("energy-direction", index),
                direction.label(),
                decks.direction == direction,
                &palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| this.set_energy_direction(direction, cx)))
            .test_support()
        })
        .collect();
    let scopes = scope_buttons(app, cx);
    let heading = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(6.0))
        .children(mashup::mode_buttons(app, cx))
        .child(section_label("Next on deck B", &palette))
        .child(div().flex_1())
        .children(directions)
        .child(div().w(px(8.0)))
        .children(scopes);
    let filters = filter_controls(app, cx);
    suggestion_rows(app, heading, filters, cx)
}

/// The pool the suggestions are chosen from: the tempo window, the key
/// relations, the energy and tempo of the candidates and their genre.
pub(crate) fn filter_controls(
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Vec<AnyElement> {
    let palette = app.theme.palette;
    let decks = &app.decks;
    let mut controls = Vec::new();
    let counted = decks.counted_relations();
    let relation_buttons: Vec<AnyElement> = Relation::ALL
        .into_iter()
        .enumerate()
        .map(|(index, relation)| {
            choice_button(
                ("suggestion-relation", index),
                relation.label(),
                counted.contains(&relation),
                &palette,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.toggle_suggestion_relation(relation, cx);
            }))
            .test_support()
            .into_any_element()
        })
        .collect();
    let mut window = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .child("Tempo window")
        .child("within");
    window = window.child(
        div()
            .w(px(48.0))
            .child(Input::new(&app.filter_window_input).id("suggestion-window")),
    );
    window = window.child("percent");
    let mut beat_row = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .child("Tempo")
        .child("from");
    beat_row = beat_row.child(
        div()
            .w(px(48.0))
            .child(Input::new(&app.filter_bpm_from_input).id("suggestion-tempo-from")),
    );
    beat_row = beat_row.child("to");
    beat_row = beat_row.child(
        div()
            .w(px(48.0))
            .child(Input::new(&app.filter_bpm_to_input).id("suggestion-tempo-to")),
    );
    beat_row = beat_row.child("BPM");
    let mut energy_row = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .child("Energy")
        .child("from");
    energy_row = energy_row.child(
        div()
            .w(px(32.0))
            .child(Input::new(&app.filter_energy_from_input).id("suggestion-energy-from")),
    );
    energy_row = energy_row.child("to");
    energy_row = energy_row.child(
        div()
            .w(px(32.0))
            .child(Input::new(&app.filter_energy_to_input).id("suggestion-energy-to")),
    );
    energy_row = energy_row.child("of 10");
    let mut genre_row = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .child("Genre");
    genre_row = genre_row.child(
        div()
            .w(px(96.0))
            .child(Input::new(&app.filter_genre_input).id("suggestion-genre")),
    );
    if let Some(problem) = &decks.filter_problem {
        let problem = problem.clone();
        controls.push(
            div()
                .id("suggestion-filter-problem")
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.danger)
                .child(problem)
                .test_support()
                .into_any_element(),
        );
    }
    controls.extend(relation_buttons);
    controls.push(window.into_any_element());
    controls.push(beat_row.into_any_element());
    controls.push(energy_row.into_any_element());
    controls.push(genre_row.into_any_element());
    controls
}

/// The choices of where suggestions come from.
pub(crate) fn scope_buttons(
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Vec<AnyElement> {
    let palette = app.theme.palette;
    let decks = &app.decks;
    let playlists = app.session.playlists().all();
    [
        (SuggestionScope::Library, "Whole library".to_string()),
        (SuggestionScope::Shown, "Tracks shown".to_string()),
        (SuggestionScope::Favorites, "Favorites".to_string()),
    ]
    .into_iter()
    .chain(playlists.iter().map(|playlist| {
        let name = match &playlist.label {
            Some(label) => format!("{label} {}", playlist.name),
            None => playlist.name.clone(),
        };
        (SuggestionScope::Playlist(playlist.id), name)
    }))
    .collect::<Vec<_>>()
    .into_iter()
    .enumerate()
    .map(|(index, (scope, label))| {
        choice_button(
            ("suggestion-scope", index),
            label,
            decks.scope == scope,
            &palette,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.set_suggestion_scope(scope, cx)))
        .test_support()
        .into_any_element()
    })
    .collect()
}

fn suggestion_rows(
    app: &EuphoriaApp,
    heading: Div,
    filters: Vec<AnyElement>,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Div {
    let palette = app.theme.palette;
    let decks = &app.decks;
    let library = app.session.library();

    let hover_background = palette.raised;
    let rows = decks.suggestions.iter().filter_map(|suggestion| {
        let track = library.track(suggestion.track_index)?;
        let track_index = suggestion.track_index;
        let tempo = suggestion
            .tempo_difference_percent
            .map(format::signed_percent)
            .unwrap_or_default();
        let energy = suggestion.energy_difference.map(|difference| {
            if difference > 0.0 {
                format!("energy +{difference:.0}")
            } else if difference < 0.0 {
                format!("energy {difference:.0}")
            } else {
                "same energy".to_string()
            }
        });
        Some(
            div()
                .id(("suggestion", track_index))
                .flex()
                .items_center()
                .gap(px(10.0))
                .h(px(26.0))
                .px(px(6.0))
                .rounded(px(4.0))
                .cursor_pointer()
                .hover(move |style| style.bg(hover_background))
                .child(
                    div()
                        .id(("suggestion-favorite", track_index))
                        .w(px(14.0))
                        .flex_shrink_0()
                        .when(suggestion.is_favorite, |mark| {
                            mark.child(favorites::star_icon(true, &palette)).tooltip(
                                |window, cx| {
                                    Tooltip::new("A transition you marked as a favorite")
                                        .build(window, cx)
                                },
                            )
                        }),
                )
                .child(
                    div().w(px(38.0)).flex_shrink_0().children(
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
                        .w(px(56.0))
                        .flex_shrink_0()
                        .text_color(palette.text_secondary)
                        .child(tempo),
                )
                .child(
                    div()
                        .w(px(88.0))
                        .flex_shrink_0()
                        .text_size(SMALL_TEXT_SIZE)
                        .text_color(palette.text_secondary)
                        .children(energy),
                )
                .child(
                    div()
                        .w(px(96.0))
                        .flex_shrink_0()
                        .text_size(SMALL_TEXT_SIZE)
                        .text_color(palette.text_tertiary)
                        .child(suggestion.relation.label()),
                )
                .on_click(cx.listener(move |this, _, _, cx| this.load_suggestion(track_index, cx)))
                .test_support(),
        )
    });
    let empty = if decks.view(DeckId::A).track_index.is_none() {
        Some("Load a track on deck A to see what mixes after it.")
    } else if decks.suggestions.is_empty() {
        Some("No track here mixes with deck A's key and tempo.")
    } else {
        None
    };
    let mut root = div().flex().flex_col().gap(px(4.0)).child(heading);
    if !filters.is_empty() {
        root = root.child(
            div()
                .id("suggestion-filters")
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.0))
                .children(filters),
        );
    }
    root.child(
        div()
            .id("deck-suggestions")
            .flex()
            .flex_col()
            .h(px(SUGGESTIONS_HEIGHT))
            .overflow_y_scroll()
            .children(rows)
            .children(empty.map(|text| {
                div()
                    .text_color(palette.text_tertiary)
                    .text_size(SMALL_TEXT_SIZE)
                    .child(text)
            })),
    )
}
