// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The loops a user saved on a track: named stretches of it to play over
//! and over, such as an intro to mix out of or a drum break to mix into.
//!
//! Saved loops are kept in the cache beside the user's cues, under the
//! content of the file and with no analyzer version, so analyzing again
//! never loses them. They live apart from the cues: saving a loop does not
//! turn the cues analysis placed into the user's own set.

use crate::error::EuphoriaError;

/// The most loops a track keeps, the number of memory loops DJ programs
/// show comfortably beside a track's cues.
pub const MAXIMUM_SAVED_LOOPS: usize = 32;

/// One loop saved on a track.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedLoop {
    /// Where the loop starts, in seconds from the start of the track.
    pub start_seconds: f64,
    /// Where it ends and jumps back to its start, in seconds from the start
    /// of the track.
    pub end_seconds: f64,
    /// The name shown for the loop. It may be empty.
    pub name: String,
}

impl SavedLoop {
    /// How long one pass of the loop lasts, in seconds.
    pub fn length_seconds(&self) -> f64 {
        self.end_seconds - self.start_seconds
    }
}

/// `loops` in order of their start, then of their end.
pub fn sorted_loops(mut loops: Vec<SavedLoop>) -> Vec<SavedLoop> {
    loops.sort_by(|first, second| {
        first
            .start_seconds
            .total_cmp(&second.start_seconds)
            .then(first.end_seconds.total_cmp(&second.end_seconds))
    });
    loops
}

/// The loops of `loops` that end within a track `duration_seconds` long,
/// in order. A duration that is not one, such as one not yet recorded,
/// keeps every loop, to be filtered when it is known.
pub fn loops_within(loops: Vec<SavedLoop>, duration_seconds: f64) -> Vec<SavedLoop> {
    sorted_loops(
        loops
            .into_iter()
            .filter(|saved| {
                duration_seconds.is_nan()
                    || duration_seconds <= 0.0
                    || saved.end_seconds <= duration_seconds
            })
            .collect(),
    )
}

/// Checks that `loops` can be kept as a track's loops: each starting at a
/// finite number of seconds from 0 and ending after it, and no more than
/// [`MAXIMUM_SAVED_LOOPS`].
///
/// # Errors
///
/// [`EuphoriaError::InvalidLoops`] saying what is wrong.
pub fn check_loops(loops: &[SavedLoop]) -> Result<(), EuphoriaError> {
    if loops.len() > MAXIMUM_SAVED_LOOPS {
        return Err(EuphoriaError::InvalidLoops {
            reason: "a track keeps at most 32 loops",
        });
    }
    let is_valid = |saved: &SavedLoop| {
        saved.start_seconds.is_finite()
            && saved.end_seconds.is_finite()
            && saved.start_seconds >= 0.0
            && saved.end_seconds > saved.start_seconds
    };
    if !loops.iter().all(is_valid) {
        return Err(EuphoriaError::InvalidLoops {
            reason: "a loop must start at a number of seconds from 0 and end after it starts",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(start_seconds: f64, end_seconds: f64) -> SavedLoop {
        SavedLoop {
            start_seconds,
            end_seconds,
            name: String::new(),
        }
    }

    #[test]
    fn loops_are_kept_in_order_and_within_the_track() {
        let loops = vec![saved(30.0, 38.0), saved(2.0, 4.0), saved(290.0, 310.0)];
        let within = loops_within(loops.clone(), 300.0);
        assert_eq!(within, vec![saved(2.0, 4.0), saved(30.0, 38.0)]);
        assert_eq!(loops_within(loops, f64::NAN).len(), 3);
        assert_eq!(saved(2.0, 4.5).length_seconds(), 2.5);
    }

    #[test]
    fn a_loop_must_start_from_zero_and_end_after_it_starts() {
        assert!(check_loops(&[saved(0.0, 1.0)]).is_ok());
        for wrong in [
            saved(-1.0, 1.0),
            saved(4.0, 4.0),
            saved(4.0, 2.0),
            saved(f64::NAN, 2.0),
        ] {
            assert!(check_loops(&[wrong]).is_err());
        }
        let too_many = vec![saved(0.0, 1.0); MAXIMUM_SAVED_LOOPS + 1];
        assert!(check_loops(&too_many).is_err());
    }
}
