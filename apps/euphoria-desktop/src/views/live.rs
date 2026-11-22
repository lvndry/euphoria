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
    SharedString, StyleRefinement, TestSupportExt, Window, div, img, px,
};

use crate::actions::{
    OpenEuphoria, Quit, ToggleLiveKeepRunning, ToggleLiveListening, ToggleLiveOpenAtLogin,
};
use crate::artwork::{Lookup, ThumbnailSize};
use crate::assets::{MUSIC_ICON_PATH, PLAY_ICON_PATH};
use crate::format;
use crate::live_model::{Learning, ListenState, LiveModel, Steadiness, shown_title};
use crate::next_rail::{EnergyChoice, NextRow, energy_text, relation_pill};
use crate::tag_settings::NotationChoice;
use crate::theme::{AppTheme, Palette};
use crate::views::controls::{segment, segmented};
use crate::views::inspector::{quiet_note, value_with_unit};
use crate::views::key_chip::key_chip;
use crate::views::{
    GAP_MEDIUM, GAP_SMALL, GAP_TIGHT, LABEL_TEXT_SIZE, RADIUS_LARGE, RADIUS_MEDIUM, RADIUS_SMALL,
    SMALL_TEXT_SIZE, TEXT_SIZE, TITLE_TEXT_SIZE, section_label, tabular_digits,
};

/// The width of the panel.
pub(crate) const PANEL_WIDTH: f32 = 360.0;
/// The height the panel opens at, before its content is measured.
pub(crate) const PANEL_HEIGHT: f32 = 560.0;
/// The shortest the panel gets.
const PANEL_MINIMUM_HEIGHT: f32 = 150.0;
/// The tallest the panel gets, as a share of the display, before the list
/// scrolls.
const PANEL_MAXIMUM_SHARE: f32 = 0.7;
/// What the panel holds besides the Next list, to leave it the rest.
const PANEL_FIXED_HEIGHT: f32 = 300.0;
/// The least room the Next list gets, about two rows.
const PANEL_ROW_ROOM: f32 = 120.0;

/// The height of a row of the Next list, which is fixed so that the list
/// can be drawn again from its cache without measuring its rows.
const NEXT_ROW_HEIGHT: f32 = 51.0;
/// How far the panel's content may grow or shrink, without a row or a part
/// coming or going, before the window follows it.
const RESIZE_SLACK: f32 = 12.0;

const BIG_KEY_SIZE: f32 = 44.0;
const COVER_SIDE: f32 = 40.0;
const DOT_SMALLEST: f32 = 7.0;
const DOT_LARGEST: f32 = 12.0;

pub(crate) const LISTENING_ID: &str = "live-listening";
pub(crate) const PAUSE_ID: &str = "live-pause";
pub(crate) const MORE_ID: &str = "live-more";
pub(crate) const SOURCE_ID: &str = "live-source";
pub(crate) const KEY_BLOCK_ID: &str = "live-key-block";
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

/// What decides how tall the panel's content is: the rows and the parts that
/// come and go. The window follows a change of these at once, and a change of
/// the content's height alone only past [`RESIZE_SLACK`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Structure {
    pub(crate) rows: usize,
    pub(crate) picker_open: bool,
    pub(crate) earlier_open: bool,
    pub(crate) earlier: usize,
    pub(crate) has_card: bool,
    pub(crate) is_learning: bool,
    pub(crate) readout: ReadoutKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum ReadoutKind {
    #[default]
    Paused,
    Unavailable,
    Waiting,
    Silent,
    Heard,
}

fn structure(model: &LiveModel, rows: usize, picker_open: bool, earlier_open: bool) -> Structure {
    let readout = match &model.state {
        ListenState::Paused => ReadoutKind::Paused,
        ListenState::Unavailable(_) => ReadoutKind::Unavailable,
        ListenState::Listening if model.reading.listening || model.reading.key.is_none() => {
            if model.hears_only_silence() {
                ReadoutKind::Silent
            } else {
                ReadoutKind::Waiting
            }
        }
        ListenState::Listening => ReadoutKind::Heard,
    };
    Structure {
        rows,
        picker_open,
        earlier_open,
        earlier: model.earlier_readings().len(),
        has_card: model.matched_track().is_some(),
        is_learning: matches!(model.learning, Learning::Working { .. }),
        readout,
    }
}

/// What the panel does with the window's height after a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Sizing {
    Keep,
    Resize(f32),
    /// The list is being scrolled: look again after this long.
    AfterScroll(std::time::Duration),
}

/// When the window follows the height of the panel's content: when rows or
/// parts came or went, or the content moved past [`RESIZE_SLACK`], and never
/// while the list is being scrolled.
#[derive(Debug, Clone)]
pub(crate) struct PanelSizing {
    pub(crate) window_height: f32,
    sized_for: Option<Structure>,
    scroll_offset: f32,
    scrolled_at: Option<std::time::Instant>,
}

impl PanelSizing {
    pub(crate) fn new(window_height: f32) -> Self {
        Self {
            window_height,
            sized_for: None,
            scroll_offset: 0.0,
            scrolled_at: None,
        }
    }

    /// Notes where the list is scrolled to, at `now`.
    pub(crate) fn note_scroll(&mut self, offset: f32, now: std::time::Instant) {
        if offset != self.scroll_offset {
            self.scroll_offset = offset;
            self.scrolled_at = Some(now);
        }
    }

    /// What to do for content measured `content` tall with `structure`, in a
    /// window no taller than `maximum`.
    pub(crate) fn decide(
        &mut self,
        content: f32,
        structure: Structure,
        maximum: f32,
        now: std::time::Instant,
    ) -> Sizing {
        let wanted = content.clamp(PANEL_MINIMUM_HEIGHT, maximum).ceil();
        let change = (wanted - self.window_height).abs();
        if change < 1.0 {
            self.sized_for = Some(structure);
            return Sizing::Keep;
        }
        if self.sized_for == Some(structure) && change < RESIZE_SLACK {
            return Sizing::Keep;
        }
        if let Some(scrolled_at) = self.scrolled_at {
            let since = now.saturating_duration_since(scrolled_at);
            if since < crate::motion::LAYOUT {
                return Sizing::AfterScroll(crate::motion::LAYOUT - since);
            }
        }
        self.window_height = wanted;
        self.sized_for = Some(structure);
        Sizing::Resize(wanted)
    }
}

/// A row of the Next list as it is drawn, made once per ranking rather than
/// on every frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShownRow {
    track_index: usize,
    path: std::path::PathBuf,
    key: Option<euphoria_core::Key>,
    title: SharedString,
    artist: Option<SharedString>,
    tempo: Option<SharedString>,
    pills: Vec<(SharedString, bool)>,
}

fn shown_rows(model: &LiveModel) -> Vec<ShownRow> {
    let Some(library) = model.library.as_ref() else {
        return Vec::new();
    };
    model
        .next_rows
        .iter()
        .filter_map(|next: &NextRow| {
            let track = library.track(next.track_index)?;
            let mut pills = vec![(SharedString::from(relation_pill(next.relation)), true)];
            if let Some(text) = energy_text(track.energy(), next.energy_change) {
                pills.push((text.into(), false));
            }
            Some(ShownRow {
                track_index: next.track_index,
                path: track.path.clone(),
                key: track.key(),
                title: shown_title(track).into(),
                artist: track
                    .shown_artist()
                    .map(|artist| SharedString::from(artist.into_owned())),
                tempo: next
                    .tempo_difference_percent
                    .map(|percent| format::signed_percent(percent).into()),
                pills,
            })
        })
        .collect()
}

/// The Next list, a view of its own that is drawn from its cache unless its
/// rows, the selection or the scroll changed, so the level dot and the
/// readout redraw without it.
pub(crate) struct NextList {
    rows: Vec<ShownRow>,
    theme: AppTheme,
    notation: NotationChoice,
    selected_row: Option<usize>,
    scroll: ScrollHandle,
    #[cfg(test)]
    pub(crate) renders: usize,
}

impl NextList {
    fn show(
        &mut self,
        rows: Vec<ShownRow>,
        theme: AppTheme,
        notation: NotationChoice,
        selected_row: Option<usize>,
        cx: &mut Context<'_, Self>,
    ) {
        if self.rows == rows
            && self.theme == theme
            && self.notation == notation
            && self.selected_row == selected_row
        {
            return;
        }
        self.rows = rows;
        self.theme = theme;
        self.notation = notation;
        self.selected_row = selected_row;
        cx.notify();
    }

    /// How tall the list is with `rows` rows and at most `room` to fill.
    fn height(rows: usize, room: f32) -> f32 {
        let gaps = rows.saturating_sub(1) as f32 * (GAP_TIGHT / px(1.0));
        (rows as f32 * NEXT_ROW_HEIGHT + gaps).min(room)
    }
}

impl Render for NextList {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<'_, Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let theme = self.theme;
        let notation = self.notation;
        let selected_row = self.selected_row;
        div()
            .relative()
            .size_full()
            .child(
                div()
                    .id("live-next-list")
                    .flex()
                    .flex_col()
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .gap(GAP_TIGHT)
                    .children(self.rows.iter().enumerate().map(|(position, row)| {
                        next_row(row, &theme, notation, selected_row == Some(position))
                    }))
                    .test_support(),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .bottom_0()
                    .w(Scrollbar::width())
                    .child(Scrollbar::vertical(&self.scroll).viewport_from_layout()),
            )
    }
}

/// The dot that follows the level heard, a view of its own so that it is the
/// only part drawn again when only the level changed.
pub(crate) struct ListeningDot {
    model: Entity<LiveModel>,
}

impl Render for ListeningDot {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let level = model.level.read(cx).position;
        let palette = model.theme.palette;
        let dot_side = DOT_SMALLEST + (DOT_LARGEST - DOT_SMALLEST) * level;
        let (dot_colour, dot_tip) = match &model.state {
            ListenState::Listening if model.hears_only_silence() => (
                palette.text_tertiary.opacity(0.7),
                "Only silence so far. Nothing is recorded.".to_string(),
            ),
            ListenState::Listening => (
                palette.accent.opacity(0.55 + 0.45 * level),
                format!(
                    "Listening to {}. Nothing is recorded: the audio stays in memory.",
                    model.source.label()
                ),
            ),
            ListenState::Paused => (palette.text_tertiary, "Not listening".to_string()),
            ListenState::Unavailable(_) => (palette.warning, "Cannot listen".to_string()),
        };
        let dot_tip: SharedString = dot_tip.into();
        div()
            .id(LISTENING_ID)
            .flex()
            .items_center()
            .justify_center()
            .size(px(DOT_LARGEST))
            .tooltip(move |window, cx| Tooltip::new(dot_tip.clone()).build(window, cx))
            .child(div().size(px(dot_side)).rounded_full().bg(dot_colour))
            .test_support()
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
    pub(crate) dot: Entity<ListeningDot>,
    pub(crate) list: Entity<NextList>,
    /// How tall the content was last drawn, in points, and what it held.
    content_height: std::rc::Rc<std::cell::Cell<Option<(f32, Structure)>>>,
    pub(crate) sizing: PanelSizing,
    looks_again: bool,
    /// Where the popover's top left corner belongs, from the top left of the
    /// main display, once it is opened under the menu bar item.
    pub(crate) top_left: Option<(f32, f32)>,
    #[cfg(test)]
    pub(crate) resizes: Vec<(f32, f32)>,
    #[cfg(test)]
    pub(crate) renders: usize,
}

impl LiveView {
    pub(crate) fn new(model: Entity<LiveModel>, cx: &mut Context<'_, Self>) -> Self {
        cx.observe(&model, |view, _, cx| {
            view.show_rows(cx);
            cx.notify();
        })
        .detach();
        model.update(cx, |model, cx| model.ensure_library(cx));
        let scroll = ScrollHandle::new();
        let dot_model = model.clone();
        let dot = cx.new(|_| ListeningDot { model: dot_model });
        let list_scroll = scroll.clone();
        let (theme, notation) = {
            let model = model.read(cx);
            (model.theme, model.notation)
        };
        let list = cx.new(|_| NextList {
            rows: Vec::new(),
            theme,
            notation,
            selected_row: None,
            scroll: list_scroll,
            #[cfg(test)]
            renders: 0,
        });
        let mut view = Self {
            model,
            picker_open: false,
            earlier_open: false,
            scroll,
            focus: cx.focus_handle(),
            selected_row: None,
            dot,
            list,
            content_height: std::rc::Rc::default(),
            sizing: PanelSizing::new(PANEL_HEIGHT),
            looks_again: false,
            top_left: None,
            #[cfg(test)]
            resizes: Vec::new(),
            #[cfg(test)]
            renders: 0,
        };
        view.show_rows(cx);
        view
    }

    /// Hands the Next list the rows the model ranked, which redraws it only
    /// when they changed.
    fn show_rows(&mut self, cx: &mut Context<'_, Self>) {
        let model = self.model.read(cx);
        let rows = shown_rows(model);
        let theme = model.theme;
        let notation = model.notation;
        let selected_row = self.selected_row;
        self.list.update(cx, |list, cx| {
            list.show(rows, theme, notation, selected_row, cx);
        });
    }

    /// Gives the window the height of its content when [`PanelSizing`] says
    /// so, and looks again once a scroll has settled.
    fn follow_content_height(
        &mut self,
        maximum_height: f32,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let now = std::time::Instant::now();
        self.sizing
            .note_scroll(self.scroll.offset().y / px(1.0), now);
        let (Some((content, structure)), Some(top_left)) =
            (self.content_height.get(), self.top_left)
        else {
            return;
        };
        match self.sizing.decide(content, structure, maximum_height, now) {
            Sizing::Keep => {}
            Sizing::Resize(height) => {
                #[cfg(test)]
                self.resizes.push((content, height));
                window.defer(cx, move |window, cx| {
                    crate::panel_frame::set_height_keeping_top(window, top_left, height, cx);
                });
            }
            Sizing::AfterScroll(wait) => {
                if !self.looks_again {
                    self.looks_again = true;
                    cx.spawn(async move |this, cx| {
                        cx.background_executor().timer(wait).await;
                        this.update(cx, |view, cx| {
                            view.looks_again = false;
                            cx.notify();
                        })
                        .ok();
                    })
                    .detach();
                }
            }
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
        self.show_rows(cx);
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

fn pill(text: SharedString, is_key: bool, palette: &Palette) -> Div {
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
    let model_entity = view.model.clone();
    let is_listening = model.is_listening();
    let source_label = model.source.label().to_string();
    let hover_background = palette.line;
    let more_rows = more_menu(model, model.opens_at_login);
    div()
        .flex()
        .items_center()
        .gap(GAP_SMALL)
        .child(view.dot.clone())
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
                    .text_size(TITLE_TEXT_SIZE)
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
        let note = if model.hears_only_silence() {
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
    let energy = model
        .matched_track()
        .and_then(|(track, _)| track.energy())
        .or(reading.energy)
        .map_or_else(|| "\u{2013}".to_string(), |energy| energy.to_string());
    div()
        .flex()
        .flex_col()
        .gap(GAP_TIGHT)
        .child(
            div()
                .flex()
                .items_stretch()
                .justify_between()
                .gap(GAP_MEDIUM)
                .child(
                    div()
                        .id(KEY_BLOCK_ID)
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
                        )
                        .test_support(),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_end()
                        .justify_center()
                        .gap(GAP_SMALL)
                        .child(
                            div()
                                .id(BPM_ID)
                                .child(value_with_unit(bpm, "BPM", "live-bpm-unit", &palette))
                                .test_support(),
                        )
                        .child(
                            div()
                                .id(ENERGY_ID)
                                .child(value_with_unit(
                                    energy,
                                    "out of 10 energy",
                                    "live-energy-unit",
                                    &palette,
                                ))
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
    let position = model.match_position_seconds()?;
    let times = match track.duration_seconds() {
        Some(duration) => format!("{} / {}", clock(position), clock(duration)),
        None => clock(position),
    };
    let detail = [
        track.shown_artist().map(std::borrow::Cow::into_owned),
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

fn next_row(
    row: &ShownRow,
    theme: &AppTheme,
    notation: NotationChoice,
    is_selected: bool,
) -> AnyElement {
    let palette = theme.palette;
    let hover_background = palette.raised;
    let path = row.path.clone();
    div()
        .id(row_id(row.track_index))
        .flex()
        .flex_shrink_0()
        .items_start()
        .h(px(NEXT_ROW_HEIGHT))
        .gap(GAP_SMALL)
        .py(px(4.0))
        .px(px(2.0))
        .rounded(RADIUS_SMALL)
        .cursor_pointer()
        .when(is_selected, |element| element.bg(palette.selection))
        .hover(move |style| style.bg(hover_background))
        .on_click(move |_, _, cx| crate::live_app::show_in_euphoria(path.clone(), cx))
        .child(
            div()
                .flex()
                .flex_shrink_0()
                .w(px(38.0))
                .children(row.key.map(|key| key_chip(&key, theme, notation))),
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
                        .child(div().flex_1().min_w_0().truncate().child(row.title.clone()))
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(SMALL_TEXT_SIZE)
                                .text_color(palette.text_secondary)
                                .children(row.tempo.clone()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(GAP_SMALL)
                        .children(row.artist.clone().map(|artist| {
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(SMALL_TEXT_SIZE)
                                .text_color(palette.text_secondary)
                                .child(artist)
                        }))
                        .children(
                            row.pills
                                .iter()
                                .map(|(text, is_key)| pill(text.clone(), *is_key, &palette)),
                        ),
                ),
        )
        .test_support()
        .into_any_element()
}

fn next_section(
    model: &LiveModel,
    model_entity: &Entity<LiveModel>,
    list: &Entity<NextList>,
    rows: usize,
    list_room: f32,
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
    let has_key = model.is_listening() && !model.reading.listening && model.reading.key.is_some();
    let empty_text = if !has_key {
        "Tracks that mix in appear once a key is heard."
    } else if model.library.is_none() {
        "Reading your library\u{2026}"
    } else {
        "No track in your library mixes in with this."
    };
    let body = if rows == 0 {
        div()
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_tertiary)
            .child(empty_text)
            .into_any_element()
    } else {
        list.clone()
            .cached(
                StyleRefinement::default()
                    .w_full()
                    .h(px(NextList::height(rows, list_room))),
            )
            .into_any_element()
    };
    div()
        .flex()
        .flex_col()
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
        .child(body)
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
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        #[cfg(test)]
        {
            self.renders += 1;
        }
        let model_entity = self.model.clone();
        let picker_open = self.picker_open;
        let earlier_open = self.earlier_open;
        let screen_height = cx
            .primary_display()
            .map_or(PANEL_HEIGHT / PANEL_MAXIMUM_SHARE, |display| {
                display.bounds().size.height / px(1.0)
            });
        let maximum_height = (screen_height * PANEL_MAXIMUM_SHARE).max(PANEL_MINIMUM_HEIGHT);
        self.follow_content_height(maximum_height, window, cx);
        let rows = self.list.read(cx).rows.len();
        let model = self.model.read(cx);
        let palette = model.theme.palette;
        let seen_height = self.content_height.clone();
        let drawn = structure(model, rows, picker_open, earlier_open);
        let view_id = cx.entity_id();

        div().size_full().child(
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
                .relative()
                .flex()
                .flex_col()
                .w_full()
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
                    &self.list,
                    rows,
                    (maximum_height - PANEL_FIXED_HEIGHT).max(PANEL_ROW_ROOM),
                ))
                .children(earlier_row(model, earlier_open, cx))
                .child(
                    gpui::canvas(
                        move |bounds: gpui::Bounds<gpui::Pixels>, window, cx| {
                            let measured = Some((bounds.size.height / px(1.0), drawn));
                            if seen_height.replace(measured) != measured {
                                window.defer(cx, move |_, cx| cx.notify(view_id));
                            }
                        },
                        |_, _, _, _| (),
                    )
                    .absolute()
                    .size_full(),
                ),
        )
    }
}
