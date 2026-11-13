// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Text that comes from files, shown on a terminal without letting it act
//! on the terminal.
//!
//! File names, tag values and the error messages that quote them are
//! written by whoever made the file. A carriage return, an escape sequence
//! or a text direction override in them could overwrite or disguise what
//! the user is shown, such as the preview of a tag write the user is asked
//! to confirm. Those characters are shown as visible escapes instead.

use std::fmt::{self, Display, Write};

/// Shows the wrapped value with every character that could act on a
/// terminal replaced by a visible escape such as `\r` or `\u{1b}`.
pub struct Printable<T>(pub T);

/// Characters that change the direction or the visibility of the text
/// around them without being control characters.
fn is_invisible_format_character(character: char) -> bool {
    matches!(
        character,
        '\u{061C}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

struct Escaping<'formatter, 'buffer> {
    formatter: &'formatter mut fmt::Formatter<'buffer>,
}

impl Write for Escaping<'_, '_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        for character in text.chars() {
            if character.is_ascii_control() {
                write!(self.formatter, "{}", character.escape_default())?;
            } else if character.is_control() || is_invisible_format_character(character) {
                write!(self.formatter, "{}", character.escape_unicode())?;
            } else {
                self.formatter.write_char(character)?;
            }
        }
        Ok(())
    }
}

impl<T: Display> Display for Printable<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(Escaping { formatter }, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_text_is_shown_as_it_is() {
        for text in [
            "8A - Energy 7 - https://example.com/a?b=c",
            "Été à Zürich",
            "東京 夜",
            "C:\\Music\\track.mp3",
            "",
        ] {
            assert_eq!(Printable(text).to_string(), text);
        }
    }

    #[test]
    fn characters_that_act_on_a_terminal_are_made_visible() {
        let cases = [
            ("safe\rhidden", "safe\\rhidden"),
            ("line\nbreak\ttab", "line\\nbreak\\ttab"),
            ("\x1b[2K\x1b[1Aerased", "\\u{1b}[2K\\u{1b}[1Aerased"),
            ("bell\x07 delete\x7f", "bell\\u{7} delete\\u{7f}"),
            ("c1 \u{9b}31m", "c1 \\u{9b}31m"),
            ("\u{202e}txt.3pm", "\\u{202e}txt.3pm"),
            ("zero\u{200b}width", "zero\\u{200b}width"),
        ];
        for (text, shown) in cases {
            assert_eq!(Printable(text).to_string(), shown);
        }
    }

    #[test]
    fn paths_are_escaped_through_their_display() {
        let path = std::path::Path::new("music/evil\r\x1b[31m.mp3");
        assert_eq!(
            Printable(path.display()).to_string(),
            "music/evil\\r\\u{1b}[31m.mp3"
        );
    }
}
