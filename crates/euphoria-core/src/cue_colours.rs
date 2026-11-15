// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The colours cues get in DJ programs.
//!
//! By default a cue analysis placed is coloured by the energy of the section
//! it starts, on Euphoria's scale from cool to warm: blue for the calmest
//! sections, then teal, green, amber and red for the most intense. A DJ sees
//! on the player where a track lifts and where it drops. The user's own cues,
//! and every cue with [`CueColours::Fixed`], keep the colour each program
//! gives its pad.

use std::fmt;
use std::str::FromStr;

/// How the cues of an export are coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CueColours {
    /// Each cue analysis placed by the energy of its section. The user's
    /// cues and cues without an energy keep the colour of their pad.
    #[default]
    Energy,
    /// Every cue the colour of its pad, as each program gives it.
    Fixed,
}

impl CueColours {
    /// Both choices, the default first.
    pub const ALL: [CueColours; 2] = [CueColours::Energy, CueColours::Fixed];

    /// The name of the choice as the command line and the settings store
    /// it.
    pub fn as_str(self) -> &'static str {
        match self {
            CueColours::Energy => "energy",
            CueColours::Fixed => "fixed",
        }
    }

    /// The energy to colour a cue by under this choice: the cue's own with
    /// [`CueColours::Energy`], and none with [`CueColours::Fixed`].
    pub fn energy_to_show(self, energy: Option<u8>) -> Option<u8> {
        match self {
            CueColours::Energy => energy,
            CueColours::Fixed => None,
        }
    }
}

impl fmt::Display for CueColours {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for CueColours {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        CueColours::ALL
            .into_iter()
            .find(|choice| choice.as_str() == text)
            .ok_or_else(|| format!("{text:?} is not energy or fixed"))
    }
}

/// The number of colours energy is shown in.
pub const ENERGY_COLOUR_STEPS: usize = 5;

/// Euphoria's energy colours as red, green and blue, from the calmest step
/// to the most intense: blue, teal, green, amber and red.
pub const ENERGY_CUE_COLOURS: [[u8; 3]; ENERGY_COLOUR_STEPS] = [
    [0x3A, 0x6F, 0xF7],
    [0x16, 0xB4, 0xC8],
    [0x34, 0xC4, 0x6B],
    [0xF2, 0xA6, 0x2E],
    [0xF0, 0x45, 0x22],
];

/// The step of an energy rating from 1 to 10, from 0 to 4: ratings 1 and 2
/// are step 0, 3 and 4 step 1, and so on up to 9 and 10 at step 4. A rating
/// outside 1 to 10 takes the nearest step.
pub fn energy_step(energy: u8) -> usize {
    usize::from(energy.clamp(1, 10) - 1) / 2
}

/// Euphoria's colour for a section of energy `energy`, as red, green and
/// blue.
pub fn energy_cue_colour(energy: u8) -> [u8; 3] {
    ENERGY_CUE_COLOURS[energy_step(energy)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_ratings_share_each_step_from_cool_to_warm() {
        let steps: Vec<usize> = (1..=10).map(energy_step).collect();
        assert_eq!(steps, vec![0, 0, 1, 1, 2, 2, 3, 3, 4, 4]);
        assert_eq!(energy_step(0), 0);
        assert_eq!(energy_step(u8::MAX), 4);
        assert_eq!(energy_cue_colour(1), [0x3A, 0x6F, 0xF7]);
        assert_eq!(energy_cue_colour(10), [0xF0, 0x45, 0x22]);
    }

    #[test]
    fn the_colours_warm_as_energy_rises() {
        let red_minus_blue: Vec<i32> = ENERGY_CUE_COLOURS
            .iter()
            .map(|[red, _, blue]| i32::from(*red) - i32::from(*blue))
            .collect();
        assert!(
            red_minus_blue.windows(2).all(|pair| pair[0] < pair[1]),
            "{red_minus_blue:?}"
        );
    }

    #[test]
    fn fixed_colours_show_no_energy() {
        assert_eq!(CueColours::Energy.energy_to_show(Some(7)), Some(7));
        assert_eq!(CueColours::Fixed.energy_to_show(Some(7)), None);
        assert_eq!(CueColours::default(), CueColours::Energy);
    }

    #[test]
    fn the_choice_reads_back_from_its_name() {
        for choice in CueColours::ALL {
            assert_eq!(choice.as_str().parse::<CueColours>(), Ok(choice));
        }
        assert!("purple".parse::<CueColours>().is_err());
    }
}
