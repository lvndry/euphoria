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

/// Tonic names for major keys, spelled as their key signatures are written.
const MAJOR_TONIC_NAMES: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];

/// Tonic names for minor keys, spelled as their key signatures are written:
/// C# minor has four sharps where Db minor would need eight flats.
const MINOR_TONIC_NAMES: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "G#", "A", "Bb", "B",
];

impl Key {
    /// Creates a key, wrapping `tonic` into the range 0 to 11.
    pub fn new(tonic: u8, mode: Mode) -> Self {
        Self {
            tonic: tonic % 12,
            mode,
        }
    }

    /// The same mode with the tonic `semitones` higher, or lower when
    /// negative: the key of the track after shifting its pitch by that much.
    pub fn transposed(&self, semitones: i8) -> Self {
        let tonic = (i16::from(self.tonic) + i16::from(semitones)).rem_euclid(12);
        Self::new(tonic as u8, self.mode)
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

    /// Short musical name in the form audio tags use, for example "Am" for
    /// A minor and "Db" for D flat major.
    pub fn short_name(&self) -> String {
        let (tonic_names, suffix) = match self.mode {
            Mode::Major => (&MAJOR_TONIC_NAMES, ""),
            Mode::Minor => (&MINOR_TONIC_NAMES, "m"),
        };
        format!("{}{suffix}", tonic_names[usize::from(self.tonic)])
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

    /// Parses the key field of an audio file, in the notations DJ programs
    /// write there: Camelot ("8A", "08A"), Open Key ("1m") and musical
    /// names ("Am", "A min", "A minor", "Abm", "A♭m", "Bb"). A tempo after
    /// the key, as in "8A 114" or "8A 114bpm", and an energy label after a
    /// dash, as in "8A - Energy 7", are ignored. Two readings joined by a
    /// slash, as in "8A/Am", must name the same key. Returns `None` for
    /// anything else, including the "o" ID3 uses for a track with no key.
    pub fn parse_file_key(text: &str) -> Option<Self> {
        let normalized = text.replace('\u{266F}', "#").replace('\u{266D}', "b");
        let before_label = normalized.split(" - ").next().unwrap_or_default();
        let mut readings = before_label.split('/').map(Self::parse_one_file_key);
        let first = readings.next().flatten()?;
        readings
            .all(|reading| reading == Some(first))
            .then_some(first)
    }

    fn parse_one_file_key(text: &str) -> Option<Self> {
        let mut words: Vec<&str> = text.split_whitespace().collect();
        if words.len() > 1 && words.last().copied().is_some_and(is_tempo) {
            words.pop();
        }
        match words.as_slice() {
            [code] => Self::from_wheel_code(code).or_else(|| Self::from_musical_name(code, "")),
            [note, mode] => Self::from_musical_name(note, mode),
            _ => None,
        }
    }

    /// Reads a musical name: a note with an optional sharp or flat, and the
    /// mode either joined to it ("Am", "Amin") or as the next word
    /// (`separate_mode`, as in "A minor"). A bare note is major. A lone
    /// capital "M" is rejected, as programs disagree on what it means.
    fn from_musical_name(note: &str, separate_mode: &str) -> Option<Self> {
        let mut characters = note.chars();
        let natural: i16 = match characters.next()?.to_ascii_uppercase() {
            'C' => 0,
            'D' => 2,
            'E' => 4,
            'F' => 5,
            'G' => 7,
            'A' => 9,
            'B' => 11,
            _ => return None,
        };
        let rest = characters.as_str();
        let (accidental, joined_mode): (i16, &str) = if let Some(after) = rest.strip_prefix('#') {
            (1, after)
        } else if let Some(after) = rest.strip_prefix('b') {
            (-1, after)
        } else {
            (0, rest)
        };
        let mode_text = match (joined_mode, separate_mode) {
            (joined, "") => joined,
            ("", separate) => separate,
            _ => return None,
        };
        let mode = match mode_text {
            "" => Mode::Major,
            "m" => Mode::Minor,
            other => match other.to_ascii_lowercase().as_str() {
                "min" | "minor" => Mode::Minor,
                "maj" | "major" => Mode::Major,
                _ => return None,
            },
        };
        let tonic = (natural + accidental).rem_euclid(12) as u8;
        Some(Key::new(tonic, mode))
    }
}

/// True for a tempo written after a key: a positive number, optionally
/// followed by "bpm".
fn is_tempo(word: &str) -> bool {
    let lowercase = word.to_ascii_lowercase();
    let number = lowercase.strip_suffix("bpm").unwrap_or(&lowercase);
    number
        .parse::<f32>()
        .is_ok_and(|tempo| tempo.is_finite() && tempo > 0.0)
}

impl fmt::Display for Key {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (tonic_names, mode_name) = match self.mode {
            Mode::Major => (&MAJOR_TONIC_NAMES, "major"),
            Mode::Minor => (&MINOR_TONIC_NAMES, "minor"),
        };
        write!(
            formatter,
            "{} {}",
            tonic_names[usize::from(self.tonic)],
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
    fn short_names_follow_the_tag_convention() {
        let expectations = [
            ("A minor", "Am"),
            ("Db major", "Db"),
            ("F# minor", "F#m"),
            ("C# minor", "C#m"),
            ("G# minor", "G#m"),
        ];
        for (text, short_name) in expectations {
            let key: Key = text.parse().unwrap();
            assert_eq!(key.short_name(), short_name, "{text}");
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
    fn spells_keys_as_their_key_signatures_are_written() {
        let expectations = [
            (1, Mode::Major, "Db major"),
            (1, Mode::Minor, "C# minor"),
            (8, Mode::Major, "Ab major"),
            (8, Mode::Minor, "G# minor"),
            (6, Mode::Major, "F# major"),
            (6, Mode::Minor, "F# minor"),
            (3, Mode::Major, "Eb major"),
            (3, Mode::Minor, "Eb minor"),
            (10, Mode::Minor, "Bb minor"),
        ];
        for (tonic, mode, name) in expectations {
            assert_eq!(Key::new(tonic, mode).to_string(), name);
        }
    }

    #[test]
    fn file_key_fields_are_read_in_every_notation_programs_write() {
        let a_minor = Key::new(9, Mode::Minor);
        let a_minor_spellings = [
            "8A",
            "08A",
            "8a",
            "1m",
            "Am",
            "am",
            "Amin",
            "A min",
            "A minor",
            "A Minor",
            "8A 114",
            "8A 114bpm",
            "a minor 114",
            "8A - Energy 7",
            "8A/Am",
            "Am/8A",
            "  Am  ",
        ];
        for text in a_minor_spellings {
            assert_eq!(Key::parse_file_key(text), Some(a_minor), "{text}");
        }
        let expectations = [
            ("8B", Key::new(0, Mode::Major)),
            ("C", Key::new(0, Mode::Major)),
            ("Cmaj 122", Key::new(0, Mode::Major)),
            ("1d", Key::new(0, Mode::Major)),
            ("Ebm", Key::new(3, Mode::Minor)),
            ("Eb minor", Key::new(3, Mode::Minor)),
            ("D#m", Key::new(3, Mode::Minor)),
            ("Bb", Key::new(10, Mode::Major)),
            ("bbm", Key::new(10, Mode::Minor)),
            ("A\u{266D}m", Key::new(8, Mode::Minor)),
            ("F\u{266F}", Key::new(6, Mode::Major)),
            ("b min", Key::new(11, Mode::Minor)),
            ("12A", Key::new(1, Mode::Minor)),
        ];
        for (text, key) in expectations {
            assert_eq!(Key::parse_file_key(text), Some(key), "{text}");
        }
    }

    #[test]
    fn every_key_euphoria_writes_reads_back() {
        for tonic in 0..12 {
            for mode in [Mode::Major, Mode::Minor] {
                let key = Key::new(tonic, mode);
                for written in [
                    key.camelot(),
                    key.open_key(),
                    key.short_name(),
                    key.to_string(),
                    format!("{} - Energy 7", key.camelot()),
                ] {
                    assert_eq!(Key::parse_file_key(&written), Some(key), "{written}");
                }
            }
        }
    }

    #[test]
    fn text_that_is_not_a_key_is_not_read_as_one() {
        let unreadable = [
            "",
            "   ",
            "o",
            "unknown",
            "Energy 7",
            "8A something 114",
            "13A",
            "0A",
            "8C",
            "8",
            "H minor",
            "AM",
            "A dorian",
            "Am minor",
            "A minor major",
            "8A/9A",
            "8A 0",
            "8A -114",
            "\u{1F525}\u{1F525}",
            "\0\0\0",
            "8A 114 extra",
        ];
        for text in unreadable {
            assert_eq!(Key::parse_file_key(text), None, "{text:?}");
        }
    }

    #[test]
    fn parses_either_spelling_of_the_same_key() {
        assert_eq!(
            "C# minor".parse::<Key>().unwrap(),
            "Db minor".parse::<Key>().unwrap()
        );
        assert_eq!(
            "G# minor".parse::<Key>().unwrap(),
            "Ab minor".parse::<Key>().unwrap()
        );
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
