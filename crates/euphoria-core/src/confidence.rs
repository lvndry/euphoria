// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt::{self, Display, Formatter};

/// How far to trust a detected key or tempo.
///
/// Each level comes from how clearly the detector preferred its answer to
/// the next best one, with band edges set on half of the GiantSteps previews
/// and checked on the other half. On that other half, a key rated high was
/// exactly right 74% of the time, medium 57% and low 41%; a tempo rated high
/// was right up to an octave 98% of the time, medium 90% and low 63%.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Confidence {
    /// About one track in five. Worth checking by ear.
    Low,
    /// About two tracks in five.
    Medium,
    /// About two tracks in five.
    High,
}

impl Confidence {
    /// The level as a lowercase word, as JSON output writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

impl Display for Confidence {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A key whose score beats the runner up by less than this is rated low.
/// libkeyfinder scores keys from 0 to 1; the margin of a two minute preview
/// is typically 0.01.
const KEY_MEDIUM_MARGIN: f32 = 0.003;
/// A key whose score beats the runner up by at least this is rated high.
const KEY_HIGH_MARGIN: f32 = 0.011;

/// A tempo whose tempogram score beats the best tempo that is not an
/// octave of it by less than this is rated low.
const TEMPO_MEDIUM_MARGIN: f32 = 0.09;
/// A tempo whose tempogram score beats the best tempo that is not an
/// octave of it by at least this is rated high.
const TEMPO_HIGH_MARGIN: f32 = 0.45;

/// How far to trust a key, from how far its score is above the runner up's.
pub fn key_confidence(score_margin: f32) -> Confidence {
    level(score_margin, KEY_MEDIUM_MARGIN, KEY_HIGH_MARGIN)
}

/// How far to trust a tempo, from how far its tempogram score is above that
/// of the best tempo that is not an octave of it. An estimate with no
/// margin, which happens only when the tempogram gave none and a fallback
/// estimate was used, is rated low.
pub fn tempo_confidence(score_margin: Option<f32>) -> Confidence {
    score_margin.map_or(Confidence::Low, |margin| {
        level(margin, TEMPO_MEDIUM_MARGIN, TEMPO_HIGH_MARGIN)
    })
}

fn level(margin: f32, medium_from: f32, high_from: f32) -> Confidence {
    if margin >= high_from {
        Confidence::High
    } else if margin >= medium_from {
        Confidence::Medium
    } else {
        Confidence::Low
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_margins_fall_into_their_bands() {
        assert_eq!(key_confidence(0.0), Confidence::Low);
        assert_eq!(key_confidence(0.002_9), Confidence::Low);
        assert_eq!(key_confidence(0.003), Confidence::Medium);
        assert_eq!(key_confidence(0.010_9), Confidence::Medium);
        assert_eq!(key_confidence(0.011), Confidence::High);
        assert_eq!(key_confidence(0.2), Confidence::High);
    }

    #[test]
    fn tempo_margins_fall_into_their_bands_and_a_missing_one_is_low() {
        assert_eq!(tempo_confidence(Some(0.05)), Confidence::Low);
        assert_eq!(tempo_confidence(Some(0.09)), Confidence::Medium);
        assert_eq!(tempo_confidence(Some(0.449)), Confidence::Medium);
        assert_eq!(tempo_confidence(Some(0.45)), Confidence::High);
        assert_eq!(tempo_confidence(None), Confidence::Low);
    }

    #[test]
    fn levels_order_from_low_to_high_and_read_as_words() {
        assert!(Confidence::Low < Confidence::Medium);
        assert!(Confidence::Medium < Confidence::High);
        assert_eq!(Confidence::High.to_string(), "high");
        assert_eq!(Confidence::Low.as_str(), "low");
    }
}
