// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Live panel, a popover under the menu bar item: what Euphoria listens
//! to, the key and tempo it hears, the library track it recognises, and the
//! tracks to play after it.

use euphoria_core::fingerprint::Playback;
use euphoria_core::live::ListenSource;
use gpui::component::button::{Button, ButtonVariants};
use gpui::component::menu::DropdownMenu;
use gpui::component::scroll::Scrollbar;
use gpui::component::tooltip::Tooltip;
use gpui::component::{Icon, IconName};
use gpui::prelude::*;
use gpui::{
    Action, AnyElement, Context, Div, Entity, FocusHandle, FontWeight, ObjectFit, ScrollHandle,
    SharedString, TestSupportExt, Window, div, img, px,
};

use crate::actions::{
    OpenEuphoria, Quit, ToggleLiveKeepRunning, ToggleLiveListening, ToggleLiveOpenAtLogin,
};
use crate::artwork::{Lookup, ThumbnailSize};
use crate::assets::{MUSIC_ICON_PATH, PLAY_ICON_PATH};
use crate::format;
use crate::live_model::{Learning, ListenState, LiveModel, Steadiness, shown_title};
use crate::next_rail::{EnergyChoice, NextRow, energy_text, relation_pill};
use crate::theme::Palette;
use crate::views::controls::{segment, segmented};
use crate::views::inspector::{quiet_note, statistic_value, value_with_unit};
use crate::views::key_chip::key_chip;
use crate::views::{
    GAP_MEDIUM, GAP_SMALL, GAP_TIGHT, LABEL_TEXT_SIZE, RADIUS_LARGE, RADIUS_MEDIUM, RADIUS_SMALL,
    SMALL_TEXT_SIZE, TEXT_SIZE, section_label, tabular_digits,
};

/// The width of the panel.
pub(crate) const PANEL_WIDTH: f32 = 360.0;
/// The tallest the panel gets before the list scrolls.
pub(crate) const PANEL_HEIGHT: f32 = 560.0;

const BIG_KEY_SIZE: f32 = 44.0;
const COVER_SIDE: f32 = 40.0;
const DOT_SMALLEST: f32 = 7.0;
const DOT_LARGEST: f32 = 12.0;

pub(crate) const LISTENING_ID: &str = "live-listening";
pub(crate) const PAUSE_ID: &str = "live-pause";
pub(crate) const MORE_ID: &str = "live-more";
pub(crate) const SOURCE_ID: &str = "live-source";
pub(crate) const KEY_ID: &str = "live-key";
pub(crate) const BPM_ID: &str = "live-bpm";
pub(crate) const ENERGY_ID: &str = "live-energy-value";
pub(crate) const STATE_ID: &str = "live-state";
pub(crate) const WAITING_ID: &str = "live-waiting";
pub(crate) const MATCH_ID: &str = "live-match";
pub(crate) const LEARNING_ID: &str = "live-learning";
pub(crate) const HEADING_ID: &str = "live-heading";
pub(crate) const EARLIER_ID: &str = "live-earlier";

pub(crate) fn energy_id(choice: EnergyChoice) -> (&'static str, usize) {
    ("live-energy", choice.index())
}

pub(crate) fn row_id(track_index: usize) -> (&'static str, usize) {
    ("live-row", track_index)
}

pub(crate) fn source_option_id(position: usize) -> (&'static str, usize) {
    ("live-source-option", position)
}

/// What the "…" menu holds, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MoreItem {
    Pause,
    KeepRunning,
    OpenAtLogin,
    OpenEuphoria,
    Quit,
}

/// One row of the "…" menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MenuRow {
    pub(crate) item: MoreItem,
    pub(crate) label: &'static str,
    /// The tick, for the items that are settings.
    pub(crate) checked: Option<bool>,
}

/// The rows of the "…" menu now.
pub(crate) fn more_menu(model: &LiveModel, opens_at_login: bool) -> Vec<MenuRow> {
    vec![
        MenuRow {
            item: MoreItem::Pause,
            label: if model.is_listening() {
                "Pause listening"
            } else {
                "Resume listening"
            },
            checked: None,
        },
        MenuRow {
            item: MoreItem::KeepRunning,
            label: "Keep Live running when the window is closed",
            checked: Some(model.settings.keep_running_when_closed),
        },
        MenuRow {
            item: MoreItem::OpenAtLogin,
            label: "Open Live at login",
            checked: Some(opens_at_login),
        },
        MenuRow {
            item: MoreItem::OpenEuphoria,
            label: "Open Euphoria",
            checked: None,
        },
        MenuRow {
            item: MoreItem::Quit,
            label: "Quit Euphoria",
            checked: None,
        },
    ]
}

impl MoreItem {
    fn action(self) -> Box<dyn Action> {
        match self {
            MoreItem::Pause => Box::new(ToggleLiveListening),
            MoreItem::KeepRunning => Box::new(ToggleLiveKeepRunning),
            MoreItem::OpenAtLogin => Box::new(ToggleLiveOpenAtLogin),
            MoreItem::OpenEuphoria => Box::new(OpenEuphoria),
            MoreItem::Quit => Box::new(Quit),
        }
    }
}

pub(crate) struct LiveView {
    pub(crate) model: Entity<LiveModel>,
    pub(crate) picker_open: bool,
    pub(crate) earlier_open: bool,
    pub(crate) scroll: ScrollHandle,
    pub(crate) focus: FocusHandle,
    /// The row the arrow keys are on, among those the Next list shows.
    pub(crate) selected_row: Option<usize>,
}

impl LiveView {
    pub(crate) fn new(model: Entity<LiveModel>, cx: &mut Context<'_, Self>) -> Self {
        cx.observe(&model, |_, _, cx| cx.notify()).detach();
        model.update(cx, |model, cx| model.ensure_library(cx));
        Self {
            model,
            picker_open: false,
            earlier_open: false,
            scroll: ScrollHandle::new(),
            focus: cx.focus_handle(),
            selected_row: None,
        }
    }

    /// Moves the arrow keys' row by `step` and scrolls it into view.
    pub(crate) fn move_row(&mut self, step: isize, cx: &mut Context<'_, Self>) {
        let count = self.model.read(cx).next_rows.len();
        if count == 0 {
            return;
        }
        let next = match self.selected_row {
            None if step > 0 => 0,
            None => count - 1,
            Some(current) => current.saturating_add_signed(step).min(count - 1),
        };
        self.selected_row = Some(next);
        self.scroll.scroll_to_item(next);
        cx.notify();
    }

    fn open_selected_row(&mut self, cx: &mut Context<'_, Self>) {
        let path = self.selected_row.and_then(|row| {
            let model = self.model.read(cx);
            let next = model.next_rows.get(row)?;
            Some(
                model
                    .library
                    .as_ref()?
                    .track(next.track_index)?
                    .path
                    .clone(),
            )
        });
        if let Some(path) = path {
            crate::live_app::show_in_euphoria(path, cx);
        }
    }
}

fn pill(text: String, is_key: bool, palette: &Palette) -> Div {
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

fn clock(seconds: f32) -> String {
    let whole = seconds.max(0.0) as u32;
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// The words for how a matched track is played compared with the file.
pub(crate) fn playback_words(playback: Playback) -> Option<String> {
    let change = (playback.tempo_ratio - 1.0) * 100.0;
    if change.abs() < 2.5 {
        return None;
    }
    let tempo = format::signed_percent(change);
    Some(if playback.pitch_follows_tempo {
        format!("{tempo}, pitch follows")
    } else {
        format!("{tempo}, key lock")
    })
}

fn icon_button(
    id: &'static str,
    path: &'static str,
    tooltip: &'static str,
    palette: &Palette,
) -> gpui::Stateful<Div> {
    let hover_background = palette.line;
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(24.0))
        .rounded(RADIUS_SMALL)
        .cursor_pointer()
        .hover(move |style| style.bg(hover_background))
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
        .child(
            Icon::default()
                .path(path)
                .size(px(14.0))
                .text_color(palette.text_secondary),
        )
}

fn header(model: &LiveModel, view: &LiveView, cx: &Context<'_, LiveView>) -> Div {
    let palette = model.theme.palette;
    let level = model
        .reading
        .level
        .filter(|_| model.is_listening())
        .map_or(0.0, |level| level.meter_position());
    let dot_side = DOT_SMALLEST + (DOT_LARGEST - DOT_SMALLEST) * level;
    let (dot_colour, dot_tip) = match &model.state {
        ListenState::Listening => (
            palette.danger.opacity(0.55 + 0.45 * level),
            format!(
                "Listening to {}. Nothing is recorded: the audio stays in memory.",
                model.source.label()
            ),
        ),
        ListenState::Paused => (palette.text_tertiary, "Not listening".to_string()),
        ListenState::Unavailable(_) => (palette.warning, "Cannot listen".to_string()),
    };
    let dot_tip: SharedString = dot_tip.into();
    let model_entity = view.model.clone();
    let is_listening = model.is_listening();
    let source_label = model.source.label().to_string();
    let hover_background = palette.line;
    let more_rows = more_menu(model, crate::login_item::is_enabled());
    div()
        .flex()
        .items_center()
        .gap(GAP_SMALL)
        .child(
            div()
                .id(LISTENING_ID)
                .flex()
                .items_center()
                .justify_center()
                .size(px(DOT_LARGEST))
                .tooltip(move |window, cx| Tooltip::new(dot_tip.clone()).build(window, cx))
                .child(div().size(px(dot_side)).rounded_full().bg(dot_colour))
                .test_support(),
        )
        .child(
            div()
                .id(SOURCE_ID)
                .flex()
                .items_center()
                .gap(GAP_TIGHT)
                .h(px(24.0))
                .px(GAP_SMALL)
                .rounded(RADIUS_SMALL)
                .cursor_pointer()
                .text_size(SMALL_TEXT_SIZE)
                .font_weight(FontWeight::MEDIUM)
                .text_color(palette.text)
                .hover(move |style| style.bg(hover_background))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.picker_open = !this.picker_open;
                    if this.picker_open {
                        this.model.update(cx, |model, cx| model.refresh_sources(cx));
                    }
                    cx.notify();
                }))
                .child(source_label)
                .child(
                    Icon::new(if view.picker_open {
                        IconName::ChevronUp
                    } else {
                        IconName::ChevronDown
                    })
                    .size(px(12.0))
                    .text_color(palette.text_tertiary),
                )
                .test_support(),
        )
        .child(div().flex_1())
        .child(
            icon_button(
                PAUSE_ID,
                if is_listening {
                    "icons/pause.svg"
                } else {
                    PLAY_ICON_PATH
                },
                if is_listening {
                    "Pause listening"
                } else {
                    "Resume listening"
                },
                &palette,
            )
            .on_click(move |_, _, cx| {
                model_entity.update(cx, |model, cx| model.toggle_listening(cx));
            })
            .test_support(),
        )
        .child(
            Button::new(MORE_ID)
                .ghost()
                .icon(Icon::new(IconName::Ellipsis).text_color(palette.text_secondary))
                .dropdown_menu(move |menu, _, _| {
                    more_rows.iter().fold(menu, |menu, row| {
                        let menu = if row.item == MoreItem::OpenEuphoria {
                            menu.separator()
                        } else {
                            menu
                        };
                        match row.checked {
                            Some(checked) => {
                                menu.menu_with_check(row.label, checked, row.item.action())
                            }
                            None => menu.menu(row.label, row.item.action()),
                        }
                    })
                }),
        )
}

fn source_options(model: &LiveModel, cx: &Context<'_, LiveView>) -> Div {
    let palette = model.theme.palette;
    let mut options: Vec<ListenSource> = vec![ListenSource::AllAudio];
    options.extend(model.sources.iter().cloned());
    if !options.contains(&model.source) {
        options.push(model.source.clone());
    }
    let hover_background = palette.line;
    div()
        .flex()
        .flex_col()
        .p(px(2.0))
        .rounded(RADIUS_MEDIUM)
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
        }))
}

fn state_line(model: &LiveModel) -> AnyElement {
    let palette = model.theme.palette;
    let word = match model.steadiness() {
        Some(Steadiness::Steady) => "steady",
        _ => "settling\u{2026}",
    };
    let listening = model
        .listening_for()
        .map(|elapsed| format!(" \u{b7} listening {}", clock(elapsed.as_secs_f32())))
        .unwrap_or_default();
    div()
        .id(STATE_ID)
        .text_size(LABEL_TEXT_SIZE)
        .text_color(palette.text_tertiary)
        .child(format!("{word}{listening}"))
        .test_support()
        .into_any_element()
}

fn readout(model: &LiveModel) -> AnyElement {
    let palette = model.theme.palette;
    let calm = |title: &str, note: Option<String>| {
        div()
            .flex()
            .flex_col()
            .gap(GAP_TIGHT)
            .py(GAP_MEDIUM)
            .child(
                div()
                    .id(WAITING_ID)
                    .text_size(px(20.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.text_secondary)
                    .child(title.to_string())
                    .test_support(),
            )
            .children(note.map(|note| {
                div()
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_tertiary)
                    .child(note)
            }))
            .into_any_element()
    };
    match &model.state {
        ListenState::Paused => {
            return calm(
                "Not listening",
                Some("Resume to read what your Mac plays.".to_string()),
            );
        }
        ListenState::Unavailable(reason) => {
            return calm("Cannot listen", Some(reason.clone()));
        }
        ListenState::Listening => {}
    }
    let reading = &model.reading;
    let Some(key) = reading.key.filter(|_| !reading.listening) else {
        let note = if reading.no_signal_seconds >= 10.0 {
            "Only silence so far. If audio is playing, allow Euphoria under System Settings, \
             Privacy & Security, Screen & System Audio Recording."
        } else {
            "Euphoria listens to what your Mac plays. Nothing is recorded."
        };
        return calm("Listening\u{2026}", Some(note.to_string()));
    };
    let bpm = reading.tempo.map_or_else(
        || "\u{2013}".to_string(),
        |tempo| format!("{:.1}", tempo.bpm),
    );
    let energy = reading
        .energy
        .map_or_else(|| "\u{2013}".to_string(), |energy| energy.to_string());
    div()
        .flex()
        .flex_col()
        .gap(GAP_TIGHT)
        .child(
            div()
                .flex()
                .items_end()
                .justify_between()
                .gap(GAP_MEDIUM)
                .child(
                    div()
                        .flex()
                        .flex_col()
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
                                .text_size(TEXT_SIZE)
                                .text_color(palette.text_secondary)
                                .child(key.key.to_string()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_end()
                        .gap(GAP_TIGHT)
                        .child(
                            div()
                                .id(BPM_ID)
                                .child(value_with_unit(bpm, "BPM", "live-bpm-unit", &palette))
                                .test_support(),
                        )
                        .child(
                            div()
                                .id(ENERGY_ID)
                                .flex()
                                .items_baseline()
                                .gap(GAP_TIGHT)
                                .whitespace_nowrap()
                                .child(quiet_note("energy", &palette))
                                .child(statistic_value(energy))
                                .child(quiet_note("out of 10", &palette))
                                .test_support(),
                        ),
                ),
        )
        .child(state_line(model))
        .into_any_element()
}

fn cover(
    model: &LiveModel,
    path: &std::path::Path,
    cx: &Context<'_, LiveView>,
) -> impl IntoElement {
    let palette = model.theme.palette;
    let store = model.artwork();
    let picture = match store.lookup(path) {
        Lookup::Ready(found) => Some(found),
        Lookup::Needed(job) => {
            let store = store.clone();
            cx.spawn(async move |this, cx| {
                let loaded = cx
                    .background_executor()
                    .spawn(async move { job.run() })
                    .await;
                store.finish(loaded);
                this.update(cx, |_, cx| cx.notify()).ok();
            })
            .detach();
            None
        }
        Lookup::Loading | Lookup::Absent => None,
    };
    let tile = div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .size(px(COVER_SIDE))
        .rounded(RADIUS_SMALL)
        .border_1()
        .border_color(palette.line)
        .bg(palette.raised);
    match picture {
        Some(found) => tile
            .child(
                img(found.image(ThumbnailSize::Small))
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            )
            .into_any_element(),
        None => tile
            .child(
                Icon::default()
                    .path(MUSIC_ICON_PATH)
                    .size(px(COVER_SIDE * 0.4))
                    .text_color(palette.text_tertiary),
            )
            .into_any_element(),
    }
}

fn library_card(model: &LiveModel, cx: &Context<'_, LiveView>) -> Option<AnyElement> {
    let palette = model.theme.palette;
    let (track, found) = model.matched_track()?;
    let heard = model.reading.heard_seconds.min(15.0);
    let position = found.start_seconds + heard * found.playback.tempo_ratio;
    let times = match track.duration_seconds() {
        Some(duration) => format!("{} / {}", clock(position), clock(duration)),
        None => clock(position),
    };
    let detail = [
        track.artist().map(str::to_string),
        Some(times),
        playback_words(found.playback),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" \u{b7} ");
    let path = track.path.clone();
    let hover_background = palette.raised;
    Some(
        div()
            .id(MATCH_ID)
            .flex()
            .items_center()
            .gap(GAP_SMALL)
            .p(GAP_SMALL)
            .rounded(RADIUS_MEDIUM)
            .border_1()
            .border_color(palette.line)
            .cursor_pointer()
            .hover(move |style| style.bg(hover_background))
            .on_click(move |_, _, cx| crate::live_app::show_in_euphoria(path.clone(), cx))
            .child(cover(model, &track.path, cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().child(shown_title(track)))
                    .child(
                        div()
                            .truncate()
                            .text_size(SMALL_TEXT_SIZE)
                            .text_color(palette.text_secondary)
                            .child(detail),
                    ),
            )
            .test_support()
            .into_any_element(),
    )
}

fn learning_note(model: &LiveModel) -> Option<AnyElement> {
    let Learning::Working { total } = model.learning else {
        return None;
    };
    Some(
        quiet_note(
            format!(
                "Learning your library to recognise tracks: {} of {total}",
                model.learned_count()
            ),
            &model.theme.palette,
        )
        .id(LEARNING_ID)
        .test_support()
        .into_any_element(),
    )
}

fn next_row(model: &LiveModel, next: &NextRow, is_selected: bool) -> Option<AnyElement> {
    let palette = model.theme.palette;
    let track = model.library.as_ref()?.track(next.track_index)?;
    let hover_background = palette.raised;
    let mut pills = vec![(relation_pill(next.relation).to_string(), true)];
    if let Some(text) = energy_text(track.energy(), next.energy_change) {
        pills.push((text, false));
    }
    let path = track.path.clone();
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
            .cursor_pointer()
            .when(is_selected, |row| row.bg(palette.selection))
            .hover(move |style| style.bg(hover_background))
            .on_click(move |_, _, cx| crate::live_app::show_in_euphoria(path.clone(), cx))
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
                                    .child(shown_title(track)),
                            )
                            .child(
                                div()
                                    .flex_shrink_0()
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
                            .children(track.artist().map(|artist| {
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(SMALL_TEXT_SIZE)
                                    .text_color(palette.text_secondary)
                                    .child(artist.to_string())
                            }))
                            .children(
                                pills
                                    .into_iter()
                                    .map(|(text, is_key)| pill(text, is_key, &palette)),
                            ),
                    ),
            )
            .test_support()
            .into_any_element(),
    )
}

fn next_section(
    model: &LiveModel,
    model_entity: &Entity<LiveModel>,
    scroll: &ScrollHandle,
    selected_row: Option<usize>,
) -> Div {
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
        .enumerate()
        .filter_map(|(position, next)| next_row(model, next, selected_row == Some(position)))
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
                .gap(GAP_SMALL)
                .child(
                    div()
                        .id(HEADING_ID)
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(section_label(model.next_heading(), &palette))
                        .test_support(),
                )
                .child(control),
        )
        .child(
            div()
                .relative()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .child(
                    div()
                        .id("live-next-list")
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(scroll)
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
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .bottom_0()
                        .w(Scrollbar::width())
                        .child(Scrollbar::vertical(scroll).viewport_from_layout()),
                ),
        )
}

fn earlier_row(model: &LiveModel, is_open: bool, cx: &Context<'_, LiveView>) -> Option<AnyElement> {
    let palette = model.theme.palette;
    let earlier = model.earlier_readings();
    if earlier.is_empty() {
        return None;
    }
    let describe = |entry: &crate::live_model::Earlier| {
        let key = model.notation.spell(&entry.key);
        let bpm = entry
            .bpm
            .map(|bpm| format!(" {bpm:.0}"))
            .unwrap_or_default();
        match &entry.title {
            Some(title) => format!("{key}{bpm}  {title}"),
            None => format!("{key}{bpm}"),
        }
    };
    let summary = earlier
        .iter()
        .map(|entry| {
            let key = model.notation.spell(&entry.key);
            match entry.bpm {
                Some(bpm) => format!("{key} {bpm:.0}"),
                None => key,
            }
        })
        .collect::<Vec<_>>()
        .join(" \u{b7} ");
    Some(
        div()
            .id(EARLIER_ID)
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap(GAP_TIGHT)
            .pt(GAP_SMALL)
            .border_t_1()
            .border_color(palette.line)
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_secondary)
            .child(
                div()
                    .id("live-earlier-toggle")
                    .flex()
                    .items_center()
                    .gap(GAP_TIGHT)
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.earlier_open = !this.earlier_open;
                        cx.notify();
                    }))
                    .child(
                        Icon::new(if is_open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .size(px(12.0))
                        .text_color(palette.text_tertiary),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(if is_open {
                        "Earlier".to_string()
                    } else {
                        format!("Earlier: {summary}")
                    }))
                    .test_support(),
            )
            .when(is_open, |column| {
                column.children(earlier.iter().rev().map(|entry| {
                    div()
                        .pl(px(16.0))
                        .truncate()
                        .child(describe(entry))
                        .into_any_element()
                }))
            })
            .test_support()
            .into_any_element(),
    )
}

impl Render for LiveView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let model_entity = self.model.clone();
        let model = self.model.read(cx);
        let palette = model.theme.palette;
        let picker_open = self.picker_open;
        let earlier_open = self.earlier_open;

        div()
            .id("live-panel")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "down" => this.move_row(1, cx),
                    "up" => this.move_row(-1, cx),
                    "enter" => this.open_selected_row(cx),
                    "escape" => window.remove_window(),
                    _ => {}
                }
            }))
            .flex()
            .flex_col()
            .size_full()
            .gap(GAP_MEDIUM)
            .p(GAP_MEDIUM)
            .rounded(RADIUS_LARGE)
            .border_1()
            .border_color(palette.line)
            .bg(palette.background)
            .text_color(palette.text)
            .text_size(TEXT_SIZE)
            .child(header(model, self, cx))
            .when(picker_open, |panel| panel.child(source_options(model, cx)))
            .child(readout(model))
            .children(library_card(model, cx))
            .children(learning_note(model))
            .child(next_section(
                model,
                &model_entity,
                &self.scroll,
                self.selected_row,
            ))
            .children(earlier_row(model, earlier_open, cx))
    }
}
