// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;
use std::str::FromStr;

use crate::error::EuphoriaError;

/// Whether a key is major or minor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mode {
    /// A major key.
    Major,
    /// A minor key.
    Minor,
}

/// A musical key: a tonic pitch class and a mode.
///
/// The tonic is always in the range 0 (C) to 11 (B); [`Key::new`] wraps any
/// other value into that range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    tonic: u8,
    mode: Mode,
}

const NOTE_NAMES: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];

impl Key {
    /// Creates a key, wrapping `tonic` into the range 0 to 11.
    pub fn new(tonic: u8, mode: Mode) -> Self {
        Self {
            tonic: tonic % 12,
            mode,
        }
    }

    /// Pitch class of the tonic, 0 (C) to 11 (B).
    pub fn tonic(&self) -> u8 {
        self.tonic
    }

    /// Major or minor.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Camelot code, for example "8A" for A minor and "8B" for C major.
    pub fn camelot(&self) -> String {
        let major_tonic = match self.mode {
            Mode::Major => self.tonic,
            Mode::Minor => (self.tonic + 3) % 12,
        };
        let number = (u32::from(major_tonic) * 7 + 7) % 12 + 1;
        let letter = match self.mode {
            Mode::Major => 'B',
            Mode::Minor => 'A',
        };
        format!("{number}{letter}")
    }
}

impl fmt::Display for Key {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mode_name = match self.mode {
            Mode::Major => "major",
            Mode::Minor => "minor",
        };
        write!(
            formatter,
            "{} {}",
            NOTE_NAMES[usize::from(self.tonic)],
            mode_name
        )
    }
}

impl FromStr for Key {
    type Err = EuphoriaError;

    /// Parses text such as "A minor", "Db major" or "F# minor". Anything after
    /// the mode is rejected.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let unknown = || EuphoriaError::UnknownKeyText(text.to_string());
        let mut words = text.split_whitespace();
        let note = words.next().ok_or_else(unknown)?;
        let mode = match words.next().map(str::to_ascii_lowercase).as_deref() {
            Some("major") => Mode::Major,
            Some("minor") => Mode::Minor,
            _ => return Err(unknown()),
        };
        if words.next().is_some() {
            return Err(unknown());
        }

        let mut characters = note.chars();
        let letter = characters.next().ok_or_else(unknown)?.to_ascii_uppercase();
        let natural: i32 = match letter {
            'C' => 0,
            'D' => 2,
            'E' => 4,
            'F' => 5,
            'G' => 7,
            'A' => 9,
            'B' => 11,
            _ => return Err(unknown()),
        };
        let accidental: i32 = match characters.as_str() {
            "" => 0,
            "b" => -1,
            "#" => 1,
            _ => return Err(unknown()),
        };
        let tonic = (natural + accidental).rem_euclid(12) as u8;
        Ok(Key::new(tonic, mode))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camelot_codes_match_the_wheel() {
        let expectations = [
            ("A minor", "8A"),
            ("C major", "8B"),
            ("E minor", "9A"),
            ("G major", "9B"),
            ("B major", "1B"),
            ("Ab minor", "1A"),
            ("F major", "7B"),
            ("Db minor", "12A"),
            ("F# minor", "11A"),
        ];
        for (text, camelot) in expectations {
            let key: Key = text.parse().unwrap();
            assert_eq!(key.camelot(), camelot, "{text}");
        }
    }

    #[test]
    fn display_round_trips_through_parse() {
        for tonic in 0..12 {
            for mode in [Mode::Major, Mode::Minor] {
                let key = Key::new(tonic, mode);
                assert_eq!(key.to_string().parse::<Key>().unwrap(), key);
            }
        }
    }

    #[test]
    fn tonic_is_always_a_valid_pitch_class() {
        assert_eq!(Key::new(6, Mode::Minor).to_string(), "F# minor");
        let key = Key::new(12, Mode::Minor);
        assert_eq!(key.tonic(), 0);
        assert_eq!(key.to_string(), "C minor");
        assert_eq!(Key::new(u8::MAX, Mode::Major).tonic(), u8::MAX % 12);
    }

    #[test]
    fn rejects_malformed_text() {
        for text in [
            "",
            "A",
            "H minor",
            "A dorian",
            "Abb minor",
            "A minor nonsense",
        ] {
            assert!(text.parse::<Key>().is_err(), "{text:?} should not parse");
        }
    }
}
