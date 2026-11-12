// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The stars and transition marks the views share: the star beside a
//! track, the line between the decks that marks the transition from deck A
//! into deck B and holds its note, and the export buttons over the starred
//! tracks.

use gpui::component::Icon;
use gpui::component::input::Input;
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{Context, Div, ElementId, SharedString, Stateful, TestSupportExt, div, px};

use crate::app::EuphoriaApp;
use crate::assets::{STAR_FILLED_ICON_PATH, STAR_ICON_PATH};
use crate::deck_audio::DeckId;
use crate::playlist_flow::PlaylistFormat;
use crate::theme::Palette;
use crate::views::SMALL_TEXT_SIZE;

/// Size of a star's icon.
const STAR_SIZE: f32 = 13.0;

/// The star icon, filled in the accent colour when `is_starred`.
pub(crate) fn star_icon(is_starred: bool, palette: &Palette) -> Icon {
    Icon::default()
        .path(if is_starred {
            STAR_FILLED_ICON_PATH
        } else {
            STAR_ICON_PATH
        })
        .size(px(STAR_SIZE))
        .text_color(if is_starred {
            palette.accent
        } else {
            palette.text_tertiary
        })
}

/// A star that can be clicked, with `tooltip` on hover. The caller says
/// what the click does.
pub(crate) fn star_button(
    id: impl Into<ElementId>,
    is_starred: bool,
    tooltip: &'static str,
    palette: &Palette,
) -> Stateful<Div> {
    let hover_background = palette.raised;
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(20.0))
        .rounded(px(4.0))
        .cursor_pointer()
        .hover(move |style| style.bg(hover_background))
        .tooltip(move |window, cx| Tooltip::new(tooltip).build(window, cx))
        .child(star_icon(is_starred, palette))
}

/// The star in the heading of `deck`, when a track is on it.
pub(crate) fn deck_star(
    app: &EuphoriaApp,
    deck: DeckId,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<impl IntoElement> {
    let path = app.decks.view(deck).path.as_ref()?;
    let is_starred = app.session.favorites().is_starred(path);
    let number = match deck {
        DeckId::A => 0_usize,
        DeckId::B => 1,
    };
    Some(
        star_button(
            ("deck-star", number),
            is_starred,
            if is_starred {
                "Take the star off this track"
            } else {
                "Star this track"
            },
            &app.theme.palette,
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            this.toggle_star_on_deck(deck, cx);
            cx.stop_propagation();
        }))
        .test_support(),
    )
}

/// The line under the decks that marks the transition from deck A into
/// deck B as a favorite, and shows its note or the field to type it in.
pub(crate) fn deck_transition(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Option<Div> {
    let (from, to) = app.deck_pair()?;
    let palette = app.theme.palette;
    let transition = app.session.favorites().transition(&from, &to);
    let is_marked = transition.is_some();
    let is_editing = app
        .editing_transition_note
        .as_ref()
        .is_some_and(|(editing_from, editing_to)| *editing_from == from && *editing_to == to);
    let toggle = div()
        .id("deck-transition")
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(6.0))
        .px(px(8.0))
        .py(px(2.0))
        .rounded(px(5.0))
        .border_1()
        .border_color(if is_marked {
            palette.accent
        } else {
            palette.line
        })
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .cursor_pointer()
        .hover({
            let hover_background = palette.raised;
            move |style| style.bg(hover_background)
        })
        .child(star_icon(is_marked, &palette))
        .child(if is_marked {
            "Favorite transition A \u{2192} B"
        } else {
            "Save transition A \u{2192} B"
        })
        .on_click(cx.listener(|this, _, _, cx| this.toggle_deck_transition(cx)))
        .test_support();
    let note = transition.map(|transition| {
        let text: SharedString = transition
            .note
            .clone()
            .unwrap_or_else(|| "Add a note".to_string())
            .into();
        let has_note = transition.note.is_some();
        let (from, to) = (from.clone(), to.clone());
        div()
            .id("transition-note")
            .min_w_0()
            .truncate()
            .text_size(SMALL_TEXT_SIZE)
            .text_color(if has_note {
                palette.text
            } else {
                palette.text_tertiary
            })
            .cursor_text()
            .child(text)
            .on_click(cx.listener(move |this, _, window, cx| {
                this.start_editing_transition_note(from.clone(), to.clone(), window, cx);
            }))
            .test_support()
    });
    Some(
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(toggle)
            .when(is_editing, |line| {
                line.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&app.transition_note_input).id("transition-note-field")),
                )
            })
            .when(!is_editing, |line| line.children(note)),
    )
}

/// The export buttons over the starred tracks.
pub(crate) fn favorites_summary(
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<Div> {
    if !app.showing_favorites || app.playlist.is_some() {
        return None;
    }
    let palette = app.theme.palette;
    let transitions = app.session.favorites().transitions().len();
    let export_button = |element_id: &'static str, label: &'static str, format: PlaylistFormat| {
        let hover_background = palette.raised;
        div()
            .id(element_id)
            .flex_shrink_0()
            .px(px(8.0))
            .py(px(2.0))
            .rounded(px(5.0))
            .border_1()
            .border_color(palette.line)
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_secondary)
            .cursor_pointer()
            .hover(move |style| style.bg(hover_background))
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.export_favorites(format, cx)))
            .test_support()
    };
    Some(
        div()
            .flex()
            .items_center()
            .gap(px(12.0))
            .child(
                div()
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_secondary)
                    .child(match transitions {
                        0 => "No favorite transitions yet".to_string(),
                        1 => "1 favorite transition".to_string(),
                        count => format!("{count} favorite transitions"),
                    }),
            )
            .child(export_button(
                "export-favorites-rekordbox",
                "Export to rekordbox",
                PlaylistFormat::Rekordbox,
            ))
            .child(export_button(
                "export-favorites-m3u8",
                "Export M3U8",
                PlaylistFormat::M3u8,
            )),
    )
}
