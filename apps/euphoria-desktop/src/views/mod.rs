// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The parts of the window. Each view reads the root view's state and turns
//! it into elements, taking every colour from the theme.

pub(crate) mod artwork;
pub(crate) mod by_ear;
pub(crate) mod command_palette;
pub(crate) mod controls;
pub(crate) mod cues;
pub(crate) mod decks;
pub(crate) mod empty_state;
pub(crate) mod favorites;
pub(crate) mod first_run;
pub(crate) mod focus;
pub(crate) mod inspector;
pub(crate) mod inspector_section;
pub(crate) mod key_chip;
pub(crate) mod key_shift;
pub(crate) mod library_export_panel;
pub(crate) mod live;
pub(crate) mod logo;
pub(crate) mod loops;
pub(crate) mod mashup;
pub(crate) mod next_rail;
pub(crate) mod note_strip;
pub(crate) mod notice;
pub(crate) mod panel_edge;
pub(crate) mod planner;
pub(crate) mod player;
pub(crate) mod playlists;
pub(crate) mod prepare_panel;
pub(crate) mod review;
pub(crate) mod row_actions;
pub(crate) mod set_builder;
pub(crate) mod settings_panel;
pub(crate) mod sidebar;
pub(crate) mod stems;
pub(crate) mod tag_editor;
pub(crate) mod tag_panel;
pub(crate) mod tempo;
pub(crate) mod toast;
pub(crate) mod toolbar;
pub(crate) mod track_filter_bar;
pub(crate) mod track_overview;
pub(crate) mod track_table;
pub(crate) mod waveform;
pub(crate) mod wheel;
pub(crate) mod wheel_menu;

use gpui::prelude::*;
use gpui::{Div, FontWeight, Pixels, SharedString, Stateful, div, px};

use crate::theme::Palette;

/// The key context of the window, which keyboard shortcuts for the library
/// and the player are bound in.
pub(crate) const LIBRARY_KEY_CONTEXT: &str = "Library";

/// The key context of the window while the focus view is open: the
/// library's, with the loop keys besides.
pub(crate) const FOCUS_VIEW_KEY_CONTEXT: &str = "Library FocusView";

/// The context of the list of tracks to play next, where Enter plays the
/// selected one.
pub(crate) const NEXT_RAIL_KEY_CONTEXT: &str = "NextRail";

/// The key context of the command palette, where Command Enter plays the
/// selected track.
pub(crate) const PALETTE_KEY_CONTEXT: &str = "CommandPalette";

/// Size of a screen's or dialog's title; used with semibold.
pub(crate) const TITLE_TEXT_SIZE: Pixels = px(20.0);
/// Size of ordinary interface text.
pub(crate) const TEXT_SIZE: Pixels = px(13.0);
/// Size of secondary text such as counts and progress.
pub(crate) const SMALL_TEXT_SIZE: Pixels = px(12.0);
/// Size of labels above sections and columns; used with semibold.
pub(crate) const LABEL_TEXT_SIZE: Pixels = px(11.0);
/// Size of a big figure such as a key, a tempo or an energy; used with
/// semibold and tabular digits.
pub(crate) const STAT_TEXT_SIZE: Pixels = px(22.0);

/// Corner radius of small parts: bars, rows and tags.
pub(crate) const RADIUS_SMALL: Pixels = px(4.0);
/// Corner radius of buttons, fields and cards.
pub(crate) const RADIUS_MEDIUM: Pixels = px(6.0);
/// Corner radius of dialogs, panels and chips.
pub(crate) const RADIUS_LARGE: Pixels = px(10.0);
/// Corner radius of buttons and fields.
pub(crate) const CONTROL_RADIUS: Pixels = RADIUS_MEDIUM;

/// Gap between parts that belong together tightly.
pub(crate) const GAP_TIGHT: Pixels = px(4.0);
/// Gap between neighbouring controls and lines.
pub(crate) const GAP_SMALL: Pixels = px(8.0);
/// Gap between groups inside a card.
pub(crate) const GAP_MEDIUM: Pixels = px(12.0);
/// Gap between cards and sections.
pub(crate) const GAP_LARGE: Pixels = px(16.0);
/// Gap between the big parts of a screen.
pub(crate) const GAP_HUGE: Pixels = px(24.0);

/// Height of buttons and fields.
pub(crate) const CONTROL_HEIGHT: Pixels = px(28.0);
/// Height of the single primary action of a dialog.
pub(crate) const DIALOG_ACTION_HEIGHT: Pixels = px(32.0);

/// The font features that give digits one width, so figures do not jump.
pub(crate) fn tabular_digits() -> gpui::FontFeatures {
    gpui::FontFeatures(std::sync::Arc::new(vec![("tnum".to_string(), 1)]))
}

/// The small bold label above a section.
pub(crate) fn section_label(text: impl Into<SharedString>, palette: &Palette) -> Div {
    div()
        .text_size(LABEL_TEXT_SIZE)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(palette.text_tertiary)
        .child(text.into())
}

/// A quiet bordered button, as used for secondary commands.
pub(crate) fn secondary_button(label: impl Into<SharedString>, palette: &Palette) -> Div {
    let hover_background = palette.line;
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(CONTROL_HEIGHT)
        .px(GAP_MEDIUM)
        .rounded(CONTROL_RADIUS)
        .border_1()
        .border_color(palette.line)
        .bg(palette.raised)
        .text_size(TEXT_SIZE)
        .text_color(palette.text)
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(move |style| style.bg(hover_background))
        .child(label.into())
}

/// A filled accent button, as used for the main command of a screen.
pub(crate) fn primary_button(label: impl Into<SharedString>, palette: &Palette) -> Div {
    let look = controls::chip_look(controls::ChipRole::Primary, palette);
    let hover_background = palette.accent.opacity(0.9);
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(CONTROL_HEIGHT)
        .px(GAP_LARGE)
        .rounded(CONTROL_RADIUS)
        .bg(look.background.unwrap_or(palette.accent))
        .text_size(TEXT_SIZE)
        .text_color(look.text)
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .hover(move |style| style.bg(hover_background))
        .child(label.into())
}

/// The one filled button of a dialog: the same as [`primary_button`] but
/// taller, the only button that is.
pub(crate) fn dialog_primary_button(label: impl Into<SharedString>, palette: &Palette) -> Div {
    primary_button(label, palette).h(DIALOG_ACTION_HEIGHT)
}

/// A card in the middle of the window over a dimmed backdrop, for a panel
/// that needs an answer before the window is used again. Clicks on the
/// backdrop go nowhere. `fade` runs from 0 to 1 as the dialog comes in: the
/// backdrop darkens, and the card fades in and rises.
pub(crate) fn modal(
    id: &'static str,
    width: Pixels,
    palette: &Palette,
    fade: f32,
) -> (Stateful<Div>, Div) {
    let backdrop = div()
        .id(id)
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(gpui::black().opacity(if palette.is_dark { 0.55 } else { 0.3 } * fade))
        .occlude();
    let card = div()
        .flex()
        .flex_col()
        .w(width)
        .max_w(gpui::relative(0.92))
        .max_h(gpui::relative(0.86))
        .gap(GAP_MEDIUM)
        .p(px(20.0))
        .mt(px(2.0 * crate::surface_motion::CARD_RISE * (1.0 - fade)))
        .opacity(fade)
        .rounded(RADIUS_LARGE)
        .border_1()
        .border_color(palette.line)
        .bg(palette.panel)
        .shadow_lg();
    (backdrop, card)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// Calls that take a radius, a gap or a text size. Handed a literal
    /// number they are strays: each is one of the tokens above, or a named
    /// layout measurement.
    const STRAY_LITERALS: [&str; 10] = [
        ".rounded(px(",
        ".rounded_t(px(",
        ".rounded_b(px(",
        ".rounded_l(px(",
        ".rounded_r(px(",
        ".gap(px(",
        ".gap_x(px(",
        ".gap_y(px(",
        ".text_size(px(",
        ".text_size(gpui::px(",
    ];

    fn view_sources() -> Vec<(String, String)> {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/views");
        let mut sources: Vec<(String, String)> = std::fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "rs"))
            .map(|path| {
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                let text = std::fs::read_to_string(&path).unwrap();
                let live = text
                    .split("#[cfg(test)]")
                    .next()
                    .unwrap_or_default()
                    .to_string();
                (name, live)
            })
            .collect();
        sources.sort();
        sources
    }

    #[test]
    fn views_take_radii_gaps_and_text_sizes_from_the_tokens() {
        let mut strays = Vec::new();
        for (name, text) in view_sources() {
            for (number, line) in text.lines().enumerate() {
                let has_literal = STRAY_LITERALS.iter().any(|stray| {
                    line.match_indices(stray).any(|(at, _)| {
                        line[at + stray.len()..]
                            .chars()
                            .next()
                            .is_some_and(|next| next.is_ascii_digit())
                    })
                });
                if has_literal {
                    strays.push(format!("{name}:{}: {}", number + 1, line.trim()));
                }
            }
        }
        assert!(
            strays.is_empty(),
            "use RADIUS_*, GAP_* and the text sizes of views/mod.rs:\n{}",
            strays.join("\n")
        );
    }

    #[test]
    fn the_tokens_are_the_agreed_scale() {
        assert_eq!(
            [RADIUS_SMALL, RADIUS_MEDIUM, RADIUS_LARGE],
            [px(4.0), px(6.0), px(10.0)]
        );
        assert_eq!(
            [GAP_TIGHT, GAP_SMALL, GAP_MEDIUM, GAP_LARGE, GAP_HUGE],
            [px(4.0), px(8.0), px(12.0), px(16.0), px(24.0)]
        );
        assert_eq!(
            [
                TITLE_TEXT_SIZE,
                TEXT_SIZE,
                SMALL_TEXT_SIZE,
                LABEL_TEXT_SIZE,
                STAT_TEXT_SIZE
            ],
            [px(20.0), px(13.0), px(12.0), px(11.0), px(22.0)]
        );
        assert_eq!((CONTROL_HEIGHT, DIALOG_ACTION_HEIGHT), (px(28.0), px(32.0)));
    }

    #[test]
    fn digits_in_figures_have_one_width() {
        let features = tabular_digits();
        assert!(
            features
                .0
                .iter()
                .any(|(tag, value)| tag == "tnum" && *value == 1)
        );
    }
}
