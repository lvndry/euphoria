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
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
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

    /// Position on the Camelot wheel, 1 to 12. A minor and C major are both 8.
    /// One step up the wheel is a fifth higher.
    pub fn camelot_number(&self) -> u8 {
        let major_tonic = match self.mode {
            Mode::Major => self.tonic,
            Mode::Minor => (self.tonic + 3) % 12,
        };
        (major_tonic * 7 + 7) % 12 + 1
    }

    /// Camelot code, for example "8A" for A minor and "8B" for C major.
    pub fn camelot(&self) -> String {
        let letter = match self.mode {
            Mode::Major => 'B',
            Mode::Minor => 'A',
        };
        format!("{}{letter}", self.camelot_number())
    }

    /// Open Key code, for example "1m" for A minor and "1d" for C major.
    ///
    /// Open Key numbers the same wheel as Camelot starting from C major and
    /// A minor, so Open Key 1 is Camelot 8.
    pub fn open_key(&self) -> String {
        let number = (self.camelot_number() + 4) % 12 + 1;
        let letter = match self.mode {
            Mode::Major => 'd',
            Mode::Minor => 'm',
        };
        format!("{number}{letter}")
    }

    /// Reads a Camelot code such as "8A" or an Open Key code such as "1m".
    fn from_wheel_code(code: &str) -> Option<Self> {
        let letter = code.chars().next_back()?;
        let digits = &code[..code.len() - letter.len_utf8()];
        if digits.is_empty()
            || digits.len() > 2
            || !digits.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let number: u8 = digits.parse().ok()?;
        if !(1..=12).contains(&number) {
            return None;
        }
        let (camelot_number, mode) = match letter.to_ascii_uppercase() {
            'A' => (number, Mode::Minor),
            'B' => (number, Mode::Major),
            'M' => ((number + 6) % 12 + 1, Mode::Minor),
            'D' => ((number + 6) % 12 + 1, Mode::Major),
            _ => return None,
        };
        let major_tonic = (camelot_number + 4) * 7 % 12;
        Some(match mode {
            Mode::Major => Key::new(major_tonic, mode),
            Mode::Minor => Key::new(major_tonic + 9, mode),
        })
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

    /// Parses a musical name such as "A minor", "Db major" or "F# minor", a
    /// Camelot code such as "8A", or an Open Key code such as "1m". Letter
    /// case is ignored. Anything after the mode is rejected.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if let Some(key) = Key::from_wheel_code(text.trim()) {
            return Ok(key);
        }
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
            "0A",
            "13A",
            "13m",
            "0d",
            "8C",
            "8",
            "m",
            "008A",
            "+8A",
            "8 A",
            "8Am",
            "8é",
        ] {
            assert!(text.parse::<Key>().is_err(), "{text:?} should not parse");
        }
    }

    #[test]
    fn open_key_codes_match_the_published_chart() {
        let expectations = [
            ("C major", "1d"),
            ("A minor", "1m"),
            ("G major", "2d"),
            ("E minor", "2m"),
            ("D major", "3d"),
            ("B minor", "3m"),
            ("A major", "4d"),
            ("F# minor", "4m"),
            ("E major", "5d"),
            ("C# minor", "5m"),
            ("B major", "6d"),
            ("G# minor", "6m"),
            ("F# major", "7d"),
            ("Eb minor", "7m"),
            ("Db major", "8d"),
            ("Bb minor", "8m"),
            ("Ab major", "9d"),
            ("F minor", "9m"),
            ("Eb major", "10d"),
            ("C minor", "10m"),
            ("Bb major", "11d"),
            ("G minor", "11m"),
            ("F major", "12d"),
            ("D minor", "12m"),
        ];
        for (text, open_key) in expectations {
            let key: Key = text.parse().unwrap();
            assert_eq!(key.open_key(), open_key, "{text}");
        }
    }

    #[test]
    fn open_key_number_is_the_camelot_number_minus_seven() {
        for tonic in 0..12 {
            for mode in [Mode::Major, Mode::Minor] {
                let key = Key::new(tonic, mode);
                let open_key = key.open_key();
                let open_key_number: u8 = open_key[..open_key.len() - 1].parse().unwrap();
                assert_eq!((open_key_number + 6) % 12 + 1, key.camelot_number());
            }
        }
    }

    #[test]
    fn camelot_numbers_cover_the_wheel_once_per_mode() {
        for mode in [Mode::Major, Mode::Minor] {
            let mut numbers: Vec<u8> = (0..12)
                .map(|tonic| Key::new(tonic, mode).camelot_number())
                .collect();
            numbers.sort_unstable();
            assert_eq!(numbers, (1..=12).collect::<Vec<u8>>());
        }
    }

    #[test]
    fn camelot_and_open_key_codes_round_trip_through_parse() {
        for tonic in 0..12 {
            for mode in [Mode::Major, Mode::Minor] {
                let key = Key::new(tonic, mode);
                assert_eq!(key.camelot().parse::<Key>().unwrap(), key);
                assert_eq!(key.open_key().parse::<Key>().unwrap(), key);
            }
        }
    }

    #[test]
    fn parses_codes_in_either_case_with_padding() {
        let a_minor = Key::new(9, Mode::Minor);
        let c_major = Key::new(0, Mode::Major);
        for text in ["8A", "8a", "08A", " 8A ", "1m", "1M", "01m"] {
            assert_eq!(text.parse::<Key>().unwrap(), a_minor, "{text:?}");
        }
        for text in ["8B", "8b", "1d", "1D"] {
            assert_eq!(text.parse::<Key>().unwrap(), c_major, "{text:?}");
        }
        assert_eq!("12B".parse::<Key>().unwrap(), Key::new(4, Mode::Major));
        assert_eq!("12d".parse::<Key>().unwrap(), Key::new(5, Mode::Major));
    }
}
