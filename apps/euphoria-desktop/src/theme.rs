// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Colour tokens, palettes and the five built in themes. Every view takes its
//! colours and sizes from an [`AppTheme`], never from literal values.

use gpui::component::{Theme as ComponentTheme, ThemeMode, oklch};
use gpui::{App, Hsla, Pixels, px, rgb};
use serde::{Deserialize, Serialize};

use crate::camelot::CamelotPosition;

/// The named colours every screen is drawn from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Palette {
    /// Behind the track table and the empty state.
    pub(crate) background: Hsla,
    /// Behind the toolbar and the sidebar.
    pub(crate) sidebar: Hsla,
    /// Behind the inspector and the drop zone.
    pub(crate) panel: Hsla,
    /// Controls and the selected sidebar row.
    pub(crate) raised: Hsla,
    /// Hairline borders.
    pub(crate) line: Hsla,
    /// Primary text.
    pub(crate) text: Hsla,
    /// Secondary text.
    pub(crate) text_secondary: Hsla,
    /// Labels, counts and placeholders.
    pub(crate) text_tertiary: Hsla,
    /// The one accent colour.
    pub(crate) accent: Hsla,
    /// Text on top of the accent colour.
    pub(crate) accent_ink: Hsla,
    /// Behind the selected table row.
    pub(crate) selection: Hsla,
    /// Waveform bars.
    pub(crate) waveform: Hsla,
    /// Failed tracks and error text.
    pub(crate) danger: Hsla,
    /// True for palettes with a dark background.
    pub(crate) is_dark: bool,
}

impl Palette {
    /// Dark neutral grey with a blue accent.
    fn slate() -> Self {
        Self {
            background: hex(0x0F1011),
            sidebar: hex(0x141516),
            panel: hex(0x17181A),
            raised: hex(0x1F2124),
            line: hex(0x26282B),
            text: hex(0xE6E7E9),
            text_secondary: hex(0xA0A3A9),
            text_tertiary: hex(0x868991),
            accent: hex(0x5B9BFF),
            accent_ink: hex(0x0F1011),
            selection: hex(0x1B2740),
            waveform: hex(0x4A4E57),
            danger: oklch(0.74, 0.14, 25.0),
            is_dark: true,
        }
    }

    /// Light with a blue accent.
    fn paper() -> Self {
        Self {
            background: hex(0xFFFFFF),
            sidebar: hex(0xF5F5F6),
            panel: hex(0xFAFAFA),
            raised: hex(0xEFEFF1),
            line: hex(0xE4E4E7),
            text: hex(0x1B1C1F),
            text_secondary: hex(0x55585F),
            text_tertiary: hex(0x6A6D74),
            accent: hex(0x2563EB),
            accent_ink: hex(0xFFFFFF),
            selection: hex(0xE6EEFD),
            waveform: hex(0xB9BCC4),
            danger: oklch(0.5, 0.17, 25.0),
            is_dark: false,
        }
    }

    /// Warm dark grey with an amber accent.
    fn graphite() -> Self {
        Self {
            background: hex(0x161514),
            sidebar: hex(0x1B1A18),
            panel: hex(0x1E1D1B),
            raised: hex(0x282623),
            line: hex(0x32302C),
            text: hex(0xECEAE6),
            text_secondary: hex(0xA8A49C),
            text_tertiary: hex(0x8C887F),
            accent: hex(0xE9A23B),
            accent_ink: hex(0x1A1408),
            selection: hex(0x33291A),
            waveform: hex(0x57534B),
            danger: oklch(0.74, 0.14, 25.0),
            is_dark: true,
        }
    }
}

/// How much of its colour the played part of the waveform keeps; the rest
/// is the surface behind it.
const PLAYED_WAVEFORM_STRENGTH: f32 = 0.38;

/// The colours of the three frequency bands of the waveform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct BandColours {
    /// Bass: kick drums and bass lines.
    pub(crate) low: Hsla,
    /// The middle: vocals, chords and snares.
    pub(crate) mid: Hsla,
    /// Treble: hi hats and cymbals.
    pub(crate) high: Hsla,
}

impl Palette {
    /// The band colours of the part of the waveform still to be played: blue
    /// bass, amber middle and a treble close to the text colour, each chosen
    /// to stand out from the panel behind it in this palette.
    pub(crate) fn band_colours(&self) -> BandColours {
        if self.is_dark {
            BandColours {
                low: oklch(0.62, 0.17, 262.0),
                mid: oklch(0.78, 0.15, 70.0),
                high: oklch(0.96, 0.01, 90.0),
            }
        } else {
            BandColours {
                low: oklch(0.50, 0.19, 264.0),
                mid: oklch(0.62, 0.15, 55.0),
                high: oklch(0.27, 0.02, 264.0),
            }
        }
    }

    /// The band colours of the part already played: the same hues faded
    /// towards the panel, so the playhead divides the waveform at a glance.
    pub(crate) fn played_band_colours(&self) -> BandColours {
        let bands = self.band_colours();
        let fade = |colour| blend(colour, self.panel, PLAYED_WAVEFORM_STRENGTH);
        BandColours {
            low: fade(bands.low),
            mid: fade(bands.mid),
            high: fade(bands.high),
        }
    }
}

/// `amount` of `colour` over `base`, mixed channel by channel.
pub(crate) fn blend(colour: Hsla, base: Hsla, amount: f32) -> Hsla {
    let colour = colour.to_rgb();
    let base = base.to_rgb();
    let mix = |first: f32, second: f32| first * amount + second * (1.0 - amount);
    Hsla::from(gpui::Rgba {
        r: mix(colour.r, base.r),
        g: mix(colour.g, base.g),
        b: mix(colour.b, base.b),
        a: 1.0,
    })
}

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

/// Where the detail of the selected track is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Layout {
    /// A column to the right of the table.
    InspectorRight,
    /// A full width strip under the table, with a wide waveform.
    DeckBelow,
}

/// How a key is shown in the table and the lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyDisplay {
    /// The code on a faint tint of its hue.
    TintedChip,
    /// The code on a solid block of its hue.
    SolidChip,
    /// The code as coloured text with no chip.
    ColouredText,
}

/// The colours of one key chip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct KeyColours {
    /// Behind the code, or `None` when the code is shown as plain text.
    pub(crate) background: Option<Hsla>,
    /// The code itself.
    pub(crate) foreground: Hsla,
}

/// The built in themes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ThemeName {
    /// Dark neutral grey, inspector on the right.
    #[default]
    Slate,
    /// Light, inspector on the right.
    Paper,
    /// Slate colours with the track detail in a strip under the table.
    Deck,
    /// Warm dark grey, compact rows, keys as coloured text.
    Graphite,
    /// Slate colours with solid key chips and the Camelot wheel shown.
    KeyFirst,
}

impl ThemeName {
    /// Every theme, in the order menus list them.
    pub(crate) const ALL: [ThemeName; 5] = [
        ThemeName::Slate,
        ThemeName::Paper,
        ThemeName::Deck,
        ThemeName::Graphite,
        ThemeName::KeyFirst,
    ];

    /// The name shown in menus.
    pub(crate) fn label(self) -> &'static str {
        match self {
            ThemeName::Slate => "Slate",
            ThemeName::Paper => "Paper",
            ThemeName::Deck => "Deck",
            ThemeName::Graphite => "Graphite",
            ThemeName::KeyFirst => "Key first",
        }
    }
}

/// A theme: a palette plus the layout, row height and key display that go
/// with it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AppTheme {
    /// Which built in theme this is.
    pub(crate) name: ThemeName,
    /// The colour tokens.
    pub(crate) palette: Palette,
    /// Where the track detail is shown.
    pub(crate) layout: Layout,
    /// Height of one table row.
    pub(crate) row_height: Pixels,
    /// How keys are shown.
    pub(crate) key_display: KeyDisplay,
    /// Whether the library opens sorted by key.
    pub(crate) opens_sorted_by_key: bool,
}

const COMFORTABLE_ROW_HEIGHT: Pixels = px(32.0);
const COMPACT_ROW_HEIGHT: Pixels = px(26.0);

impl AppTheme {
    /// The built in theme called `name`.
    pub(crate) fn named(name: ThemeName) -> Self {
        let slate = Self {
            name,
            palette: Palette::slate(),
            layout: Layout::InspectorRight,
            row_height: COMFORTABLE_ROW_HEIGHT,
            key_display: KeyDisplay::TintedChip,
            opens_sorted_by_key: false,
        };
        match name {
            ThemeName::Slate => slate,
            ThemeName::Paper => Self {
                palette: Palette::paper(),
                ..slate
            },
            ThemeName::Deck => Self {
                layout: Layout::DeckBelow,
                ..slate
            },
            ThemeName::Graphite => Self {
                palette: Palette::graphite(),
                row_height: COMPACT_ROW_HEIGHT,
                key_display: KeyDisplay::ColouredText,
                ..slate
            },
            ThemeName::KeyFirst => Self {
                key_display: KeyDisplay::SolidChip,
                opens_sorted_by_key: true,
                ..slate
            },
        }
    }

    /// The colours of the chip for a key at `position`. The two keys of one
    /// Camelot number share a hue, and every hue has the same lightness.
    pub(crate) fn key_colours(&self, position: CamelotPosition) -> KeyColours {
        let hue = position.hue_degrees();
        match (self.key_display, self.palette.is_dark) {
            (KeyDisplay::SolidChip, _) => KeyColours {
                background: Some(oklch(0.78, 0.11, hue)),
                foreground: hex(0x111214),
            },
            (KeyDisplay::ColouredText, true) => KeyColours {
                background: None,
                foreground: oklch(0.82, 0.1, hue),
            },
            (KeyDisplay::ColouredText, false) => KeyColours {
                background: None,
                foreground: oklch(0.45, 0.12, hue),
            },
            (KeyDisplay::TintedChip, true) => KeyColours {
                background: Some(oklch(0.29, 0.05, hue)),
                foreground: oklch(0.86, 0.08, hue),
            },
            (KeyDisplay::TintedChip, false) => KeyColours {
                background: Some(oklch(0.94, 0.04, hue)),
                foreground: oklch(0.4, 0.1, hue),
            },
        }
    }

    /// Gives the component library the same colours, so its inputs, menus and
    /// scrollbars match the rest of the window.
    pub(crate) fn apply_to_components(&self, cx: &mut App) {
        let palette = self.palette;
        let mode = if palette.is_dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        };
        ComponentTheme::change(mode, None, cx);
        ComponentTheme::update(cx, |theme| {
            theme.font_size = px(13.0);
            theme.radius = px(6.0);
            theme.shadow = false;

            let colours = &mut theme.colors;
            colours.background = palette.background;
            colours.foreground = palette.text;
            colours.border = palette.line;
            colours.input = palette.line;
            colours.muted = palette.raised;
            colours.muted_foreground = palette.text_tertiary;
            colours.popover = palette.panel;
            colours.popover_foreground = palette.text;
            colours.primary = palette.accent;
            colours.primary_foreground = palette.accent_ink;
            colours.secondary = palette.raised;
            colours.secondary_foreground = palette.text;
            colours.accent = palette.raised;
            colours.accent_foreground = palette.text;
            colours.ring = palette.accent;
            colours.caret = palette.text;
            colours.selection = palette.accent.opacity(0.35);
            colours.list = palette.background;
            colours.list_hover = palette.raised;
            colours.list_active = palette.selection;
            colours.scrollbar = gpui::transparent_black();
            colours.scrollbar_thumb = palette.text_tertiary.opacity(0.45);
            colours.scrollbar_thumb_hover = palette.text_tertiary.opacity(0.7);
        });
    }
}

#[cfg(test)]
mod tests {
    use gpui::Rgba;

    use super::*;

    fn hex_text(colour: Hsla) -> String {
        let rgba = Rgba::from(colour);
        let channel = |value: f32| (value * 255.0).round() as u8;
        format!(
            "#{:02X}{:02X}{:02X}",
            channel(rgba.r),
            channel(rgba.g),
            channel(rgba.b)
        )
    }

    /// Perceived lightness, from 0.0 for black to 1.0 for white.
    fn luminance(colour: Hsla) -> f32 {
        let rgba = Rgba::from(colour);
        let linear = |value: f32| {
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(rgba.r) + 0.7152 * linear(rgba.g) + 0.0722 * linear(rgba.b)
    }

    fn contrast(first: Hsla, second: Hsla) -> f32 {
        let lighter = luminance(first).max(luminance(second));
        let darker = luminance(first).min(luminance(second));
        (lighter + 0.05) / (darker + 0.05)
    }

    #[test]
    fn slate_is_the_default_theme() {
        assert_eq!(ThemeName::default(), ThemeName::Slate);
        let theme = AppTheme::named(ThemeName::default());
        assert_eq!(hex_text(theme.palette.background), "#0F1011");
        assert_eq!(hex_text(theme.palette.accent), "#5B9BFF");
        assert_eq!(hex_text(theme.palette.selection), "#1B2740");
        assert_eq!(theme.row_height, px(32.0));
        assert_eq!(theme.key_display, KeyDisplay::TintedChip);
    }

    #[test]
    fn each_theme_is_a_preset_of_palette_layout_density_and_key_display() {
        let paper = AppTheme::named(ThemeName::Paper);
        assert_eq!(hex_text(paper.palette.background), "#FFFFFF");
        assert_eq!(hex_text(paper.palette.accent), "#2563EB");
        assert!(!paper.palette.is_dark);

        let deck = AppTheme::named(ThemeName::Deck);
        assert_eq!(deck.palette, Palette::slate());
        assert_eq!(deck.layout, Layout::DeckBelow);

        let graphite = AppTheme::named(ThemeName::Graphite);
        assert_eq!(hex_text(graphite.palette.background), "#161514");
        assert_eq!(hex_text(graphite.palette.accent), "#E9A23B");
        assert_eq!(graphite.row_height, px(26.0));
        assert_eq!(graphite.key_display, KeyDisplay::ColouredText);

        let key_first = AppTheme::named(ThemeName::KeyFirst);
        assert_eq!(key_first.palette, Palette::slate());
        assert_eq!(key_first.key_display, KeyDisplay::SolidChip);
        assert!(key_first.opens_sorted_by_key);
    }

    #[test]
    fn the_two_keys_of_a_number_share_their_colours() {
        let theme = AppTheme::named(ThemeName::Slate);
        let minor = CamelotPosition {
            number: 8,
            is_major: false,
        };
        let major = CamelotPosition {
            number: 8,
            is_major: true,
        };
        assert_eq!(theme.key_colours(minor), theme.key_colours(major));
        let neighbour = CamelotPosition {
            number: 9,
            is_major: false,
        };
        assert_ne!(theme.key_colours(minor), theme.key_colours(neighbour));
    }

    #[test]
    fn key_codes_stay_readable_in_every_theme() {
        for name in ThemeName::ALL {
            let theme = AppTheme::named(name);
            for number in 1..=12 {
                let colours = theme.key_colours(CamelotPosition {
                    number,
                    is_major: false,
                });
                let behind = colours.background.unwrap_or(theme.palette.background);
                let ratio = contrast(colours.foreground, behind);
                assert!(ratio >= 4.5, "{name:?} key {number}: contrast {ratio:.2}");
            }
        }
    }

    #[test]
    fn waveform_bands_stand_out_from_the_panel_and_from_each_other() {
        for name in ThemeName::ALL {
            let palette = AppTheme::named(name).palette;
            let bands = palette.band_colours();
            for (band, colour) in [("low", bands.low), ("mid", bands.mid), ("high", bands.high)] {
                let ratio = contrast(colour, palette.panel);
                assert!(ratio >= 3.0, "{name:?} {band}: contrast {ratio:.2}");
            }
            assert!(
                contrast(bands.low, bands.mid) >= 1.5,
                "{name:?} low and mid"
            );
            assert!(
                contrast(bands.mid, bands.high) >= 1.5,
                "{name:?} mid and high"
            );
            assert!(
                contrast(bands.low, bands.high) >= 2.0,
                "{name:?} low and high"
            );
        }
    }

    #[test]
    fn the_played_part_is_fainter_but_still_visible() {
        for name in ThemeName::ALL {
            let palette = AppTheme::named(name).palette;
            let unplayed = palette.band_colours();
            let played = palette.played_band_colours();
            let pairs = [
                (unplayed.low, played.low),
                (unplayed.mid, played.mid),
                (unplayed.high, played.high),
            ];
            for (unplayed, played) in pairs {
                let played_ratio = contrast(played, palette.panel);
                let unplayed_ratio = contrast(unplayed, palette.panel);
                assert!(played_ratio >= 1.25, "{name:?}: played {played_ratio:.2}");
                assert!(
                    unplayed_ratio >= played_ratio * 1.6,
                    "{name:?}: {unplayed_ratio:.2} against {played_ratio:.2}"
                );
            }
        }
    }

    #[test]
    fn a_blend_lies_between_its_two_colours() {
        let grey = blend(gpui::white(), gpui::black(), 0.5).to_rgb();
        assert!((grey.r - 0.5).abs() < 0.01);
        assert_eq!(blend(gpui::white(), gpui::black(), 1.0).to_rgb().r, 1.0);
        assert_eq!(blend(gpui::white(), gpui::black(), 0.0).to_rgb().r, 0.0);
    }

    #[test]
    fn text_tokens_stay_readable_on_every_surface() {
        for name in ThemeName::ALL {
            let palette = AppTheme::named(name).palette;
            for surface in [palette.background, palette.sidebar, palette.panel] {
                for text in [palette.text, palette.text_secondary, palette.text_tertiary] {
                    let ratio = contrast(text, surface);
                    assert!(ratio >= 4.5, "{name:?}: contrast {ratio:.2}");
                }
                assert!(contrast(palette.danger, surface) >= 4.5, "{name:?} danger");
            }
            assert!(contrast(palette.accent_ink, palette.accent) >= 4.5);
        }
    }
}
