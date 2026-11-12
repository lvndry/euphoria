// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The cues of a track as a DJ uses them: where each one is, its name and
//! the hot cue pad it sits on.
//!
//! A track has the cues analysis placed, or, once the user edited them, the
//! user's set. The user's set is kept in the cache in a table of its own,
//! under the content of the file and with no analyzer version, like a
//! [`Correction`](crate::Correction), so analyzing again never loses it.
//! Every reader of a track's cues goes through [`TrackCues::of`], which
//! decides between the two.

use crate::analysis::TrackAnalysis;
use crate::detect::{MAXIMUM_CUES, SectionCue};
use crate::error::EuphoriaError;

/// A hot cue pad, lettered from A to H.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HotCue(u8);

impl HotCue {
    /// Every pad, from A to H.
    pub const ALL: [HotCue; MAXIMUM_CUES] = [
        HotCue(0),
        HotCue(1),
        HotCue(2),
        HotCue(3),
        HotCue(4),
        HotCue(5),
        HotCue(6),
        HotCue(7),
    ];

    /// The pad numbered `pad`, where 0 is A, or `None` past H.
    pub fn from_pad(pad: u8) -> Option<Self> {
        (usize::from(pad) < MAXIMUM_CUES).then_some(Self(pad))
    }

    /// The pad lettered `letter`, in either case, or `None` for a letter
    /// outside A to H.
    pub fn from_letter(letter: char) -> Option<Self> {
        let upper = letter.to_ascii_uppercase();
        if !upper.is_ascii_uppercase() {
            return None;
        }
        Self::from_pad(upper as u8 - b'A')
    }

    /// The pad's number, where 0 is A.
    pub fn pad(self) -> u8 {
        self.0
    }

    /// The pad's letter, from A to H.
    pub fn letter(self) -> char {
        char::from(b'A' + self.0)
    }
}

impl std::fmt::Display for HotCue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.letter())
    }
}

/// One cue of a track.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    /// Seconds from the start of the track.
    pub seconds: f64,
    /// The name shown beside the cue. It may be empty.
    pub name: String,
    /// The hot cue pad the cue is on, or `None` for a memory cue only.
    pub hot_cue: Option<HotCue>,
}

/// Where the cues of a track come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CueOrigin {
    /// Placed by analysis, on the bars of the beat grid.
    Automatic,
    /// Set by the user, which wins over analysis.
    Edited,
}

/// The cues of a track, in order of time, and where they come from.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackCues {
    /// The cues, earliest first.
    pub cues: Vec<Cue>,
    /// Whether analysis placed them or the user set them.
    pub origin: CueOrigin,
}

impl TrackCues {
    /// The cues analysis placed, each on the next hot cue pad from A, as
    /// they are exported.
    pub fn automatic(section_cues: &[SectionCue]) -> Self {
        let cues = section_cues
            .iter()
            .zip(HotCue::ALL)
            .map(|(section_cue, hot_cue)| Cue {
                seconds: section_cue.seconds,
                name: section_cue.section.name().to_string(),
                hot_cue: Some(hot_cue),
            })
            .collect();
        Self {
            cues,
            origin: CueOrigin::Automatic,
        }
    }

    /// The user's cues, put in order of time.
    pub fn edited(mut cues: Vec<Cue>) -> Self {
        cues.sort_by(|first, second| first.seconds.total_cmp(&second.seconds));
        Self {
            cues,
            origin: CueOrigin::Edited,
        }
    }

    /// The cues of a track: the user's set when it has one, even an empty
    /// one, and otherwise those analysis placed in `analysis`.
    ///
    /// Pass the analysis with any [`Correction`](crate::Correction) applied:
    /// a corrected tempo drops the cues analysis placed, which sit on the
    /// grid of the detected tempo, but never the user's.
    ///
    /// A cue of the user's past the end of the track, kept from before the
    /// file was cut shorter or stored by another program, is left out: it
    /// marks no audio.
    pub fn of(analysis: &TrackAnalysis, edited: Option<Vec<Cue>>) -> Self {
        match edited {
            Some(cues) => {
                let end = f64::from(analysis.duration_seconds);
                Self::edited(
                    cues.into_iter()
                        .filter(|cue| end.is_nan() || end <= 0.0 || cue.seconds <= end)
                        .collect(),
                )
            }
            None => Self::automatic(&analysis.cues),
        }
    }

    /// True when the user set these cues.
    pub fn is_edited(&self) -> bool {
        self.origin == CueOrigin::Edited
    }
}

/// Checks that `cues` can be kept as a track's cues: each at a finite
/// number of seconds from 0, and no two on the same hot cue pad.
///
/// # Errors
///
/// [`EuphoriaError::InvalidCues`] saying what is wrong.
pub fn check_cues(cues: &[Cue]) -> Result<(), EuphoriaError> {
    if cues
        .iter()
        .any(|cue| !(cue.seconds.is_finite() && cue.seconds >= 0.0))
    {
        return Err(EuphoriaError::InvalidCues {
            reason: "a cue must be at a number of seconds from 0",
        });
    }
    let mut taken = [false; MAXIMUM_CUES];
    for hot_cue in cues.iter().filter_map(|cue| cue.hot_cue) {
        let pad = usize::from(hot_cue.pad());
        if taken[pad] {
            return Err(EuphoriaError::InvalidCues {
                reason: "two cues are on the same hot cue pad",
            });
        }
        taken[pad] = true;
    }
    Ok(())
}

/// Checks that `cues` can be kept for a track `duration_seconds` long: as
/// [`check_cues`] does, with a finite, nonnegative duration and every cue
/// within the track.
///
/// # Errors
///
/// [`EuphoriaError::InvalidCues`] saying what is wrong.
pub fn check_cues_within(cues: &[Cue], duration_seconds: f64) -> Result<(), EuphoriaError> {
    check_cues(cues)?;
    if !duration_seconds.is_finite() || duration_seconds < 0.0 {
        return Err(EuphoriaError::InvalidCues {
            reason: "the track duration must be a finite, nonnegative number of seconds",
        });
    }
    if cues.iter().any(|cue| cue.seconds > duration_seconds) {
        return Err(EuphoriaError::InvalidCues {
            reason: "a cue must be within the track",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::CueSection;

    fn cue(seconds: f64, name: &str, hot_cue: Option<char>) -> Cue {
        Cue {
            seconds,
            name: name.to_string(),
            hot_cue: hot_cue.and_then(HotCue::from_letter),
        }
    }

    #[test]
    fn a_known_track_duration_must_be_finite_and_nonnegative() {
        let cues = [cue(12.0, "Drop", None)];
        for duration in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
            assert!(check_cues_within(&cues, duration).is_err(), "{duration}");
            assert!(check_cues_within(&[], duration).is_err(), "{duration}");
        }
        assert!(check_cues_within(&[], 0.0).is_ok());
        assert!(check_cues_within(&[cue(0.0, "Start", None)], 0.0).is_ok());
    }

    fn analysis_with_cues() -> TrackAnalysis {
        TrackAnalysis {
            duration_seconds: 300.0,
            key: None,
            bpm: Some(124.0),
            energy: None,
            tuning_cents: None,
            key_timeline: Vec::new(),
            quality: None,
            key_runner_up: None,
            key_score_margin: None,
            tempo_estimate: None,
            tempo_score_margin: None,
            beat_grid: None,
            cues: vec![
                SectionCue {
                    seconds: 0.5,
                    section: CueSection::Start,
                },
                SectionCue {
                    seconds: 31.5,
                    section: CueSection::Main,
                },
            ],
        }
    }

    #[test]
    fn hot_cue_letters_go_from_a_to_h() {
        assert_eq!(HotCue::from_letter('a'), HotCue::from_pad(0));
        assert_eq!(HotCue::from_letter('H').map(HotCue::pad), Some(7));
        assert_eq!(HotCue::from_letter('I'), None);
        assert_eq!(HotCue::from_letter('1'), None);
        assert_eq!(HotCue::from_pad(8), None);
        let letters: String = HotCue::ALL.iter().map(|hot_cue| hot_cue.letter()).collect();
        assert_eq!(letters, "ABCDEFGH");
    }

    #[test]
    fn automatic_cues_take_the_pads_in_order() {
        let cues = TrackCues::of(&analysis_with_cues(), None);
        assert_eq!(cues.origin, CueOrigin::Automatic);
        assert_eq!(
            cues.cues,
            vec![cue(0.5, "Start", Some('A')), cue(31.5, "Main", Some('B'))]
        );
    }

    #[test]
    fn the_users_set_wins_even_when_empty_and_is_put_in_order() {
        let edited = vec![cue(90.0, "Vocal", None), cue(12.0, "Intro", Some('C'))];
        let cues = TrackCues::of(&analysis_with_cues(), Some(edited));
        assert!(cues.is_edited());
        assert_eq!(
            cues.cues,
            vec![cue(12.0, "Intro", Some('C')), cue(90.0, "Vocal", None)]
        );
        let none = TrackCues::of(&analysis_with_cues(), Some(Vec::new()));
        assert!(none.is_edited());
        assert!(none.cues.is_empty());
    }

    #[test]
    fn a_corrected_tempo_drops_the_automatic_cues_but_not_the_users() {
        let corrected = crate::Correction::of_bpm(87.0)
            .unwrap()
            .apply(analysis_with_cues());
        assert!(TrackCues::of(&corrected, None).cues.is_empty());
        let edited = vec![cue(12.0, "Intro", Some('A'))];
        assert_eq!(TrackCues::of(&corrected, Some(edited.clone())).cues, edited);
    }

    #[test]
    fn cues_at_impossible_times_or_sharing_a_pad_are_refused() {
        for cues in [
            vec![cue(-1.0, "Before", None)],
            vec![cue(f64::NAN, "Nowhere", None)],
            vec![cue(1.0, "One", Some('A')), cue(2.0, "Two", Some('A'))],
        ] {
            assert!(matches!(
                check_cues(&cues),
                Err(EuphoriaError::InvalidCues { .. })
            ));
        }
        check_cues(&[cue(0.0, "", None), cue(0.0, "Same time", Some('B'))]).unwrap();
    }

    #[test]
    fn an_unknown_duration_keeps_the_users_cues() {
        let cues = vec![cue(999.0, "Mine", Some('A'))];
        for duration_seconds in [f32::NAN, 0.0, -1.0] {
            let analysis = TrackAnalysis {
                duration_seconds,
                ..analysis_with_cues()
            };
            let chosen = TrackCues::of(&analysis, Some(cues.clone()));
            assert!(chosen.is_edited());
            assert_eq!(chosen.cues, cues);
        }
    }

    #[test]
    fn cues_past_the_end_of_the_track_are_refused_and_never_listed() {
        let past_the_end = vec![cue(12.0, "Intro", Some('A')), cue(999.0, "Lost", Some('B'))];
        assert!(matches!(
            check_cues_within(&past_the_end, 300.0),
            Err(EuphoriaError::InvalidCues { .. })
        ));
        check_cues_within(&past_the_end[..1], 300.0).unwrap();
        check_cues_within(&[cue(300.0, "At the end", None)], 300.0).unwrap();
        assert_eq!(
            TrackCues::of(&analysis_with_cues(), Some(past_the_end)).cues,
            vec![cue(12.0, "Intro", Some('A'))]
        );
    }
}
