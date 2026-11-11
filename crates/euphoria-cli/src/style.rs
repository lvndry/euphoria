// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! When to use colour, and the few styles every command shares: bold for
//! what matters, dim for what is secondary, red for errors, yellow for
//! warnings and things worth a look, green for what is fine, and the hue of
//! each Camelot number, as in the app.

use std::fmt::Display;
use std::io::IsTerminal;
use std::sync::OnceLock;

use clap::ValueEnum;
use euphoria_core::Key;

/// When to colour the output, as `--color` sets it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum ColorChoice {
    #[default]
    Auto,
    Always,
    Never,
}

static CHOICE: OnceLock<ColorChoice> = OnceLock::new();

/// Remembers the choice for the rest of the run, and gives it to the
/// progress bars too.
pub fn init(choice: ColorChoice) {
    let _ = CHOICE.set(choice);
    console::set_colors_enabled(Theme::stdout().colour);
    console::set_colors_enabled_stderr(Theme::stderr().colour);
}

/// The choice given on the command line, read before the arguments are
/// parsed so that the help and the errors of the parser follow it too.
pub fn choice_in_arguments(arguments: &[std::ffi::OsString]) -> ColorChoice {
    let mut choice = ColorChoice::Auto;
    let mut arguments = arguments.iter().filter_map(|argument| argument.to_str());
    while let Some(argument) = arguments.next() {
        let value = if argument == "--color" {
            arguments.next()
        } else {
            argument.strip_prefix("--color=")
        };
        if let Some(parsed) = value.and_then(|value| ColorChoice::from_str(value, true).ok()) {
            choice = parsed;
        }
    }
    choice
}

/// Whether to colour a stream that is or is not a terminal, given the
/// choice and the environment.
fn wants_colour(
    choice: ColorChoice,
    is_terminal: bool,
    variable: impl Fn(&str) -> Option<String>,
) -> bool {
    let set = |name: &str| variable(name).is_some_and(|value| !value.is_empty());
    let set_to_zero = |name: &str| variable(name).is_some_and(|value| value == "0");
    match choice {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto if set("NO_COLOR") => false,
        ColorChoice::Auto if set("CLICOLOR_FORCE") && !set_to_zero("CLICOLOR_FORCE") => true,
        ColorChoice::Auto if set_to_zero("CLICOLOR") => false,
        ColorChoice::Auto => is_terminal && variable("TERM").is_none_or(|term| term != "dumb"),
    }
}

fn environment(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// How text written to one stream is styled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    colour: bool,
    true_colour: bool,
}

impl Theme {
    /// No colour and no style at all, for tests.
    #[cfg(test)]
    pub const PLAIN: Theme = Theme {
        colour: false,
        true_colour: false,
    };

    /// Colour with 24 bit key hues, for tests.
    #[cfg(test)]
    pub const COLOURED: Theme = Theme {
        colour: true,
        true_colour: true,
    };

    fn for_stream(is_terminal: bool) -> Self {
        let choice = CHOICE.get().copied().unwrap_or_default();
        let colour = wants_colour(choice, is_terminal, environment);
        let true_colour = environment("COLORTERM")
            .is_some_and(|value| matches!(value.as_str(), "truecolor" | "24bit"));
        Self {
            colour,
            true_colour: colour && true_colour,
        }
    }

    /// The theme for standard output.
    pub fn stdout() -> Self {
        Self::for_stream(std::io::stdout().is_terminal())
    }

    /// The theme for standard error.
    pub fn stderr() -> Self {
        Self::for_stream(std::io::stderr().is_terminal())
    }

    fn paint(&self, codes: &str, text: impl Display) -> String {
        if self.colour {
            format!("\x1b[{codes}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }

    pub fn bold(&self, text: impl Display) -> String {
        self.paint("1", text)
    }

    pub fn dim(&self, text: impl Display) -> String {
        self.paint("2", text)
    }

    /// Something that went wrong.
    pub fn bad(&self, text: impl Display) -> String {
        self.paint("31", text)
    }

    /// Something worth a look.
    pub fn caution(&self, text: impl Display) -> String {
        self.paint("33", text)
    }

    /// Something that is fine.
    pub fn good(&self, text: impl Display) -> String {
        self.paint("32", text)
    }

    /// The `error:` label.
    pub fn error_label(&self) -> String {
        self.paint("1;31", "error:")
    }

    /// The `warning:` label.
    pub fn warning_label(&self) -> String {
        self.paint("1;33", "warning:")
    }

    /// The `hint:` label, before what to do next.
    pub fn hint_label(&self) -> String {
        self.paint("1;36", "hint:")
    }

    /// The `note:` label.
    pub fn note_label(&self) -> String {
        self.paint("1;36", "note:")
    }

    /// `text` in the hue of the Camelot number of `key`, as the app tints
    /// its key chips: the two keys of one number share a hue, 30 degrees
    /// apart from the next number.
    pub fn key(&self, key: Key, text: impl Display) -> String {
        if !self.colour {
            return text.to_string();
        }
        let (red, green, blue) = key_rgb(key);
        if self.true_colour {
            self.paint(&format!("1;38;2;{red};{green};{blue}"), text)
        } else {
            self.paint(&format!("1;38;5;{}", nearest_256(red, green, blue)), text)
        }
    }
}

/// Lightness and chroma of the key hues in OKLCH. Light enough to read on
/// a dark background and dark enough to read on a light one.
const KEY_LIGHTNESS: f32 = 0.7;
const KEY_CHROMA: f32 = 0.13;

/// The colour of the Camelot number of `key`, in 8 bit sRGB.
fn key_rgb(key: Key) -> (u8, u8, u8) {
    let hue_degrees = f32::from(key.camelot_number() % 12) * 30.0;
    oklch_to_srgb(KEY_LIGHTNESS, KEY_CHROMA, hue_degrees)
}

fn oklch_to_srgb(lightness: f32, chroma: f32, hue_degrees: f32) -> (u8, u8, u8) {
    let hue = hue_degrees.to_radians();
    let green_red = chroma * hue.cos();
    let blue_yellow = chroma * hue.sin();
    let long = (lightness + 0.396_337_8 * green_red + 0.215_803_76 * blue_yellow).powi(3);
    let medium = (lightness - 0.105_561_35 * green_red - 0.063_854_17 * blue_yellow).powi(3);
    let short = (lightness - 0.089_484_18 * green_red - 1.291_485_5 * blue_yellow).powi(3);
    let red = 4.076_741_7 * long - 3.307_711_6 * medium + 0.230_969_94 * short;
    let green = -1.268_438 * long + 2.609_757_4 * medium - 0.341_319_38 * short;
    let blue = -0.004_196_086 * long - 0.703_418_6 * medium + 1.707_614_7 * short;
    let encode = |linear: f32| {
        let linear = linear.clamp(0.0, 1.0);
        let encoded = if linear <= 0.003_130_8 {
            12.92 * linear
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        };
        (encoded * 255.0).round() as u8
    };
    (encode(red), encode(green), encode(blue))
}

/// The closest colour of the 6 by 6 by 6 cube of 256 colour terminals.
fn nearest_256(red: u8, green: u8, blue: u8) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let step = |channel: u8| {
        (0..LEVELS.len())
            .min_by_key(|index| LEVELS[*index].abs_diff(channel))
            .unwrap_or(0) as u8
    };
    16 + 36 * step(red) + 6 * step(green) + step(blue)
}

#[cfg(test)]
mod tests {
    use euphoria_core::Mode;

    use super::*;

    fn variables<'pairs>(
        pairs: &'pairs [(&'pairs str, &'pairs str)],
    ) -> impl Fn(&str) -> Option<String> + 'pairs {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        }
    }

    #[test]
    fn auto_colours_a_terminal_only() {
        assert!(wants_colour(ColorChoice::Auto, true, variables(&[])));
        assert!(!wants_colour(ColorChoice::Auto, false, variables(&[])));
        assert!(!wants_colour(
            ColorChoice::Auto,
            true,
            variables(&[("TERM", "dumb")])
        ));
    }

    #[test]
    fn no_color_wins_over_clicolor_force_and_the_flag_wins_over_both() {
        let no_colour = [("NO_COLOR", "1")];
        let forced = [("CLICOLOR_FORCE", "1")];
        let both = [("NO_COLOR", "1"), ("CLICOLOR_FORCE", "1")];
        assert!(!wants_colour(
            ColorChoice::Auto,
            true,
            variables(&no_colour)
        ));
        assert!(wants_colour(ColorChoice::Auto, false, variables(&forced)));
        assert!(!wants_colour(ColorChoice::Auto, false, variables(&both)));
        assert!(wants_colour(
            ColorChoice::Always,
            false,
            variables(&no_colour)
        ));
        assert!(!wants_colour(ColorChoice::Never, true, variables(&forced)));
        assert!(!wants_colour(
            ColorChoice::Auto,
            true,
            variables(&[("CLICOLOR", "0")])
        ));
        assert!(wants_colour(
            ColorChoice::Auto,
            true,
            variables(&[("NO_COLOR", "")])
        ));
    }

    #[test]
    fn the_flag_is_found_in_either_spelling_and_the_last_one_counts() {
        let arguments = |list: &[&str]| -> Vec<std::ffi::OsString> {
            list.iter().map(std::ffi::OsString::from).collect()
        };
        assert_eq!(
            choice_in_arguments(&arguments(&["euphoria", "analyze", "x"])),
            ColorChoice::Auto
        );
        assert_eq!(
            choice_in_arguments(&arguments(&["euphoria", "--color", "never", "analyze"])),
            ColorChoice::Never
        );
        assert_eq!(
            choice_in_arguments(&arguments(&[
                "euphoria",
                "--color=never",
                "analyze",
                "--color=always"
            ])),
            ColorChoice::Always
        );
    }

    #[test]
    fn plain_text_has_no_escapes() {
        let theme = Theme::PLAIN;
        let key = Key::new(9, Mode::Minor);
        assert_eq!(theme.bold("a"), "a");
        assert_eq!(theme.key(key, "8A"), "8A");
        assert_eq!(theme.error_label(), "error:");
    }

    #[test]
    fn keys_of_one_number_share_a_hue_and_neighbours_differ() {
        let minor = key_rgb(Key::new(9, Mode::Minor));
        let major = key_rgb(Key::new(0, Mode::Major));
        let next = key_rgb(Key::new(4, Mode::Minor));
        assert_eq!(minor, major);
        assert_ne!(minor, next);
        assert!(
            Theme::COLOURED
                .key(Key::new(9, Mode::Minor), "8A")
                .contains("38;2;")
        );
    }

    #[test]
    fn the_cube_matches_its_own_corners() {
        assert_eq!(nearest_256(0, 0, 0), 16);
        assert_eq!(nearest_256(255, 255, 255), 231);
        assert_eq!(nearest_256(255, 0, 0), 196);
    }
}
