// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt::{self, Display, Formatter};
use std::str::FromStr;

use crate::error::EuphoriaError;

/// The tempos a track is reported at, from `slowest` up to but not
/// including `fastest`, in beats per minute.
///
/// A tempo detector often finds half or double the tempo a DJ counts, so
/// the estimate is doubled or halved until it falls in the range. The
/// default, 90 to 180, suits most dance music. A DJ who plays hip hop can
/// choose 70 to 140 and one who plays drum and bass 120 to 200.
///
/// A range is at most an octave wide, so every estimate has one reported
/// tempo. In a range narrower than an octave, an estimate none of whose
/// octaves fall in it is reported at the octave nearest to the range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoRange {
    slowest: f32,
    fastest: f32,
}

/// Half or double a reported tempo is offered as an alternative only when
/// it lies between these, widened to take in the chosen range. Set on half
/// of the GiantSteps previews and checked on the other half: with the
/// default range it resolves 22 of the 26 octave errors of that half, and
/// is offered for 42% of its tracks.
const SLOWEST_PLAUSIBLE_ALTERNATIVE: f32 = 75.0;
const FASTEST_PLAUSIBLE_ALTERNATIVE: f32 = 200.0;

impl TempoRange {
    /// From 90 up to 180 beats per minute.
    pub const DEFAULT: Self = Self {
        slowest: 90.0,
        fastest: 180.0,
    };

    /// A range from `slowest` up to but not including `fastest`. Fails with
    /// [`EuphoriaError::InvalidTempoRange`] unless both are positive and
    /// finite, `slowest` is below `fastest`, and `fastest` is at most twice
    /// `slowest`.
    pub fn new(slowest: f32, fastest: f32) -> Result<Self, EuphoriaError> {
        let invalid = |reason: &'static str| EuphoriaError::InvalidTempoRange {
            slowest,
            fastest,
            reason,
        };
        if !(slowest.is_finite() && fastest.is_finite() && slowest > 0.0) {
            return Err(invalid("both ends must be positive numbers"));
        }
        if slowest >= fastest {
            return Err(invalid("the slowest tempo must be below the fastest"));
        }
        if fastest > 2.0 * slowest {
            return Err(invalid(
                "it can be at most an octave wide, the fastest tempo at most twice the slowest",
            ));
        }
        Ok(Self { slowest, fastest })
    }

    /// The slowest tempo reported.
    pub fn slowest(&self) -> f32 {
        self.slowest
    }

    /// The tempo that reported tempos stay below.
    pub fn fastest(&self) -> f32 {
        self.fastest
    }

    /// The octave of a positive finite `estimate` that lies in the range,
    /// or the nearest representable octave when none does.
    ///
    /// # Panics
    ///
    /// Panics when `estimate` is not positive and finite.
    pub fn fold(&self, estimate: f32) -> f32 {
        assert!(
            estimate.is_finite() && estimate > 0.0,
            "a tempo estimate must be positive and finite"
        );
        // An octave just beyond a finite f32 range can overflow f32. The
        // wider arithmetic also keeps distance ratios finite at both ends
        // of the supported range.
        let mut folded = f64::from(estimate);
        let slowest = f64::from(self.slowest);
        let fastest = f64::from(self.fastest);
        while folded < slowest {
            folded *= 2.0;
        }
        while folded >= fastest {
            folded /= 2.0;
        }
        // Rounding a subnormal estimate back to f32 can land on the
        // excluded upper bound. Try its previous octave instead.
        let mut below = folded as f32;
        if below >= self.fastest {
            folded /= 2.0;
            below = folded as f32;
        }
        if below >= self.slowest {
            return below;
        }
        let above = (folded * 2.0) as f32;
        if !above.is_finite()
            || (slowest / f64::from(below)).ln() <= (f64::from(above) / fastest).ln()
        {
            below
        } else {
            above
        }
    }

    /// Half or double a tempo reported in this range, when a DJ could
    /// plausibly count it that way: half when it is at least 75 beats per
    /// minute, double when it is at most 200, or the ends of the range when
    /// those are further out. Half is offered when both are.
    pub fn alternative(&self, reported: f32) -> Option<f32> {
        let slowest_plausible = SLOWEST_PLAUSIBLE_ALTERNATIVE.min(self.slowest);
        let fastest_plausible = FASTEST_PLAUSIBLE_ALTERNATIVE.max(self.fastest);
        [reported / 2.0, reported * 2.0]
            .into_iter()
            .find(|candidate| (slowest_plausible..=fastest_plausible).contains(candidate))
    }
}

impl Default for TempoRange {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl Display for TempoRange {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}-{}", self.slowest, self.fastest)
    }
}

/// Reads a range written as two numbers joined by a hyphen, such as
/// `70-140`.
impl FromStr for TempoRange {
    type Err = EuphoriaError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let unreadable = || EuphoriaError::UnreadableTempoRange(text.to_string());
        let (slowest, fastest) = text.split_once('-').ok_or_else(unreadable)?;
        let slowest: f32 = slowest.trim().parse().map_err(|_| unreadable())?;
        let fastest: f32 = fastest.trim().parse().map_err(|_| unreadable())?;
        Self::new(slowest, fastest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(slowest: f32, fastest: f32) -> TempoRange {
        TempoRange::new(slowest, fastest).unwrap()
    }

    #[test]
    fn the_default_folds_into_90_up_to_180() {
        let default = TempoRange::default();
        assert_eq!(default, TempoRange::DEFAULT);
        let expectations = [
            (45.0, 90.0),
            (62.5, 125.0),
            (89.9, 179.8),
            (90.0, 90.0),
            (128.0, 128.0),
            (179.9, 179.9),
            (180.0, 90.0),
            (250.0, 125.0),
            (31.25, 125.0),
        ];
        for (estimate, reported) in expectations {
            assert_eq!(default.fold(estimate), reported, "{estimate}");
        }
    }

    #[test]
    fn a_range_for_hip_hop_or_drum_and_bass_folds_into_itself() {
        let hip_hop = range(70.0, 140.0);
        assert_eq!(hip_hop.fold(85.0), 85.0);
        assert_eq!(hip_hop.fold(170.0), 85.0);
        assert_eq!(hip_hop.fold(140.0), 70.0);
        assert_eq!(hip_hop.fold(62.0), 124.0);

        let drum_and_bass = range(120.0, 200.0);
        assert_eq!(drum_and_bass.fold(87.0), 174.0);
        assert_eq!(drum_and_bass.fold(174.0), 174.0);
        assert_eq!(drum_and_bass.fold(128.0), 128.0);
    }

    #[test]
    fn an_estimate_with_no_octave_inside_a_narrow_range_takes_the_nearest() {
        let drum_and_bass = range(120.0, 200.0);
        // 105 is 0.19 octave below 120, and 210 is 0.07 octave above 200.
        assert_eq!(drum_and_bass.fold(105.0), 210.0);
        // 112 is 0.10 octave below 120, and 224 is 0.16 octave above 200.
        assert_eq!(drum_and_bass.fold(112.0), 112.0);
        assert_eq!(drum_and_bass.fold(56.0), 112.0);
    }

    #[test]
    fn a_large_finite_range_does_not_overflow_while_folding() {
        let huge = range(2.0e38, 3.0e38);
        // The higher octave of 128 would be 2^128, which f32 cannot hold.
        assert_eq!(huge.fold(128.0), 2.0_f32.powi(127));
        // A representable upper octave remains eligible for a narrow range.
        assert_eq!(huge.fold(f32::MAX), f32::MAX);

        let octave = range(f32::MAX / 2.0, f32::MAX);
        for estimate in [f32::from_bits(1), 1.0, 120.0, 128.0, f32::MAX] {
            let folded = octave.fold(estimate);
            assert!(folded.is_finite());
            assert!((octave.slowest()..octave.fastest()).contains(&folded));
        }
    }

    #[test]
    fn subnormal_octave_ranges_keep_the_excluded_upper_bound_out() {
        for bits in 1..=32 {
            let slowest = f32::from_bits(bits);
            let octave = range(slowest, slowest * 2.0);
            for estimate in [f32::from_bits(1), 1.0, 120.0, 128.0, f32::MAX] {
                let folded = octave.fold(estimate);
                assert!(
                    (octave.slowest()..octave.fastest()).contains(&folded),
                    "{octave}: {estimate} folded to {folded}"
                );
            }
        }
        assert_eq!(TempoRange::DEFAULT.fold(f32::from_bits(1)), 128.0);
    }

    #[test]
    fn invalid_estimates_are_refused_without_entering_the_folding_loops() {
        for estimate in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(
                std::panic::catch_unwind(|| TempoRange::DEFAULT.fold(estimate)).is_err(),
                "{estimate} should have been refused"
            );
        }
    }

    #[test]
    fn half_or_double_is_offered_when_a_dj_could_count_it() {
        let default = TempoRange::DEFAULT;
        assert_eq!(default.alternative(174.0), Some(87.0));
        assert_eq!(default.alternative(150.0), Some(75.0));
        assert_eq!(default.alternative(149.0), None);
        assert_eq!(default.alternative(128.0), None);
        assert_eq!(default.alternative(100.0), Some(200.0));
        assert_eq!(default.alternative(95.0), Some(190.0));
        assert_eq!(default.alternative(101.0), None);

        // A range that starts below 75 widens the plausible span to its own
        // slowest tempo, not further.
        let hip_hop = range(60.0, 120.0);
        assert_eq!(hip_hop.alternative(119.0), None);
        assert_eq!(hip_hop.alternative(100.0), Some(200.0));
        assert_eq!(hip_hop.alternative(61.0), Some(122.0));
        let slower = range(50.0, 100.0);
        assert_eq!(slower.alternative(99.0), Some(198.0));
        assert_eq!(slower.alternative(60.0), Some(120.0));

        let fast = range(150.0, 300.0);
        assert_eq!(fast.alternative(150.0), Some(75.0));
    }

    #[test]
    fn ranges_that_cannot_give_one_tempo_are_refused() {
        for (slowest, fastest) in [
            (0.0, 100.0),
            (-70.0, 140.0),
            (140.0, 70.0),
            (100.0, 100.0),
            (60.0, 200.0),
            (f32::NAN, 140.0),
            (70.0, f32::INFINITY),
        ] {
            let error = TempoRange::new(slowest, fastest).unwrap_err();
            assert!(
                matches!(error, EuphoriaError::InvalidTempoRange { .. }),
                "{slowest} to {fastest}: {error}"
            );
        }
        assert!(TempoRange::new(90.0, 180.0).is_ok());
    }

    #[test]
    fn reads_a_range_written_with_a_hyphen() {
        assert_eq!("70-140".parse::<TempoRange>().unwrap(), range(70.0, 140.0));
        assert_eq!(
            " 120 - 200 ".parse::<TempoRange>().unwrap(),
            range(120.0, 200.0)
        );
        assert_eq!(
            "87.5-175".parse::<TempoRange>().unwrap(),
            range(87.5, 175.0)
        );
        assert_eq!(range(70.0, 140.0).to_string(), "70-140");
        for text in ["70", "70-", "-140", "seventy-140", "70-140-160", ""] {
            let error = text.parse::<TempoRange>().unwrap_err();
            assert!(
                matches!(error, EuphoriaError::UnreadableTempoRange(_)),
                "{text:?}: {error}"
            );
        }
        assert!(matches!(
            "60-200".parse::<TempoRange>().unwrap_err(),
            EuphoriaError::InvalidTempoRange { .. }
        ));
    }
}
