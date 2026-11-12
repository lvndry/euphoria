// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The waveforms of the tracks looked at most recently, with their energy
//! curves, kept in memory so that going back to a track does not decode it
//! again.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::peaks::BandPeaks;
use crate::track_overview::EnergyCurve;

/// How many waveforms are kept. Each is a few kilobytes; the limit keeps a
/// long session from growing without bound.
pub(crate) const WAVEFORM_CAPACITY: usize = 32;

/// The waveform of one track.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Waveform {
    /// The audio is being decoded.
    Reading,
    /// Columns from the start of the track to its end, and its energy
    /// curve, `None` until worked out from the same audio.
    Ready(Arc<[BandPeaks]>, Option<EnergyCurve>),
    /// The audio could not be decoded; the text says why.
    Unreadable(String),
}

/// A fixed number of waveforms. Adding one beyond the capacity forgets the
/// one that was added or looked at longest ago.
#[derive(Debug)]
pub(crate) struct WaveformCache {
    capacity: usize,
    /// Oldest first.
    entries: VecDeque<(PathBuf, Waveform)>,
}

impl WaveformCache {
    /// An empty cache that keeps at most `capacity` waveforms.
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: VecDeque::with_capacity(capacity),
        }
    }

    /// The waveform of the file at `path`, when it is kept.
    pub(crate) fn get(&self, path: &Path) -> Option<&Waveform> {
        self.entries
            .iter()
            .find(|(kept_path, _)| kept_path == path)
            .map(|(_, waveform)| waveform)
    }

    /// Marks the waveform of the file at `path` as just looked at, so it is
    /// the last to be forgotten. Returns whether it is kept.
    pub(crate) fn touch(&mut self, path: &Path) -> bool {
        let Some(position) = self
            .entries
            .iter()
            .position(|(kept_path, _)| kept_path == path)
        else {
            return false;
        };
        if let Some(entry) = self.entries.remove(position) {
            self.entries.push_back(entry);
        }
        true
    }

    /// Keeps `waveform` for the file at `path`, replacing any kept for it,
    /// and forgets the oldest ones beyond the capacity.
    pub(crate) fn insert(&mut self, path: PathBuf, waveform: Waveform) {
        self.entries.retain(|(kept_path, _)| *kept_path != path);
        self.entries.push_back((path, waveform));
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
    }

    /// Adds the energy curve to the waveform kept for the file at `path`,
    /// when one is kept and ready.
    pub(crate) fn set_energy(&mut self, path: &Path, curve: EnergyCurve) {
        let kept = self
            .entries
            .iter_mut()
            .find(|(kept_path, _)| kept_path == path);
        if let Some((_, Waveform::Ready(_, energy))) = kept {
            *energy = Some(curve);
        }
    }

    /// How many waveforms are kept.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(number: usize) -> PathBuf {
        PathBuf::from(format!("/music/track-{number}.flac"))
    }

    fn ready(peak: f32) -> Waveform {
        Waveform::Ready(
            vec![BandPeaks {
                low: peak,
                mid: 0.0,
                high: 0.0,
            }]
            .into(),
            None,
        )
    }

    #[test]
    fn keeps_what_was_inserted() {
        let mut cache = WaveformCache::new(4);
        cache.insert(path(1), ready(0.5));
        assert_eq!(cache.get(&path(1)), Some(&ready(0.5)));
        assert_eq!(cache.get(&path(2)), None);
    }

    #[test]
    fn never_holds_more_than_its_capacity_and_forgets_the_oldest_first() {
        let mut cache = WaveformCache::new(3);
        for number in 1..=10 {
            cache.insert(path(number), ready(number as f32));
            assert!(cache.len() <= 3);
        }
        assert_eq!(cache.len(), 3);
        for forgotten in 1..=7 {
            assert_eq!(cache.get(&path(forgotten)), None);
        }
        for kept in 8..=10 {
            assert!(cache.get(&path(kept)).is_some());
        }
    }

    #[test]
    fn a_waveform_looked_at_again_outlives_newer_ones() {
        let mut cache = WaveformCache::new(2);
        cache.insert(path(1), ready(0.1));
        cache.insert(path(2), ready(0.2));
        assert!(cache.touch(&path(1)));
        cache.insert(path(3), ready(0.3));
        assert!(cache.get(&path(1)).is_some());
        assert_eq!(cache.get(&path(2)), None);
        assert!(!cache.touch(&path(2)));
    }

    #[test]
    fn inserting_again_replaces_without_taking_a_second_place() {
        let mut cache = WaveformCache::new(2);
        cache.insert(path(1), Waveform::Reading);
        cache.insert(path(1), ready(0.4));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(&path(1)), Some(&ready(0.4)));
    }

    #[test]
    fn the_energy_curve_joins_a_kept_waveform_and_nothing_else() {
        let mut cache = WaveformCache::new(2);
        let steps = EnergyCurve::Steps(
            vec![crate::track_overview::EnergyStep {
                start: 0.0,
                end: 1.0,
                rating: Some(7),
            }]
            .into(),
        );
        cache.insert(path(1), ready(0.5));
        cache.insert(path(2), Waveform::Reading);
        cache.set_energy(&path(1), steps.clone());
        cache.set_energy(&path(2), steps.clone());
        cache.set_energy(&path(3), steps.clone());
        match cache.get(&path(1)) {
            Some(Waveform::Ready(_, energy)) => assert_eq!(energy, &Some(steps)),
            other => panic!("{other:?}"),
        }
        assert_eq!(cache.get(&path(2)), Some(&Waveform::Reading));
        assert_eq!(cache.get(&path(3)), None);
    }

    #[test]
    fn the_app_keeps_a_few_dozen_waveforms() {
        assert!((24..=64).contains(&WAVEFORM_CAPACITY));
    }
}
