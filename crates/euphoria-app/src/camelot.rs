// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading the Camelot code the core gives a key, for sorting and colouring.

use euphoria_core::{Key, Mode};

/// A position on the Camelot wheel: a number from 1 to 12 and the inner (A,
/// minor) or outer (B, major) ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CamelotPosition {
    /// The wheel number, 1 to 12.
    pub(crate) number: u8,
    /// Whether the key is on the major ring, printed as the letter B.
    pub(crate) is_major: bool,
}

impl CamelotPosition {
    /// Reads the position from the code the core prints for `key`.
    pub(crate) fn of(key: &Key) -> Self {
        let code = key.camelot();
        let digits: String = code
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect();
        Self {
            number: digits.parse().unwrap_or(0),
            is_major: key.mode() == Mode::Major,
        }
    }

    /// Order around the wheel: by number, then the minor key before the major
    /// key of the same number.
    pub(crate) fn wheel_order(&self) -> u16 {
        u16::from(self.number) * 2 + u16::from(self.is_major)
    }

    /// The hue shared by the two keys of this number, in degrees.
    pub(crate) fn hue_degrees(&self) -> f32 {
        f32::from(self.number % 12) * 30.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(text: &str) -> CamelotPosition {
        CamelotPosition::of(&text.parse::<Key>().unwrap())
    }

    #[test]
    fn reads_number_and_ring_from_the_core_code() {
        assert_eq!(
            position("A minor"),
            CamelotPosition {
                number: 8,
                is_major: false
            }
        );
        assert_eq!(
            position("C major"),
            CamelotPosition {
                number: 8,
                is_major: true
            }
        );
        assert_eq!(position("Db minor").number, 12);
    }

    #[test]
    fn orders_keys_around_the_wheel_by_number_then_letter() {
        let mut keys = ["Db minor", "C major", "A minor", "B major", "Ab minor"]
            .map(|text| text.parse::<Key>().unwrap())
            .to_vec();
        keys.sort_by_key(|key| CamelotPosition::of(key).wheel_order());
        let codes: Vec<String> = keys.iter().map(Key::camelot).collect();
        assert_eq!(codes, ["1A", "1B", "8A", "8B", "12A"]);
    }

    #[test]
    fn hue_is_the_number_times_thirty_degrees() {
        assert_eq!(position("A minor").hue_degrees(), 240.0);
        assert_eq!(position("Ab minor").hue_degrees(), 30.0);
        assert_eq!(position("Db minor").hue_degrees(), 0.0);
    }
}
