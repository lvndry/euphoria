// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the player bar's waveform needs apart from drawing: the height of
//! each bar worked out once per track and width, where the played part
//! ends, and the section boundaries that get a tooltip.

use std::sync::Arc;

use euphoria_core::{Cue, CueSection};

use crate::focus::SectionKind;
use crate::peaks::{BandPeaks, bars_from_columns};
use crate::views::waveform::is_played;

/// Distance from the left edge of one bar to the next.
pub(crate) const BAR_SPACING: f32 = 3.0;

/// A boundary this close to the start of the track is not drawn.
const FIRST_TICK_FRACTION: f32 = 0.004;

/// The bar heights of one waveform at one width, kept until either changes.
#[derive(Debug, Default)]
pub(crate) struct BarPeaksCache {
    columns: Option<Arc<[BandPeaks]>>,
    bar_count: usize,
    heights: Arc<[f32]>,
    computations: usize,
}

impl BarPeaksCache {
    /// How many bars of `width` pixels fit.
    pub(crate) fn bar_count_for(width: f32) -> usize {
        (width / BAR_SPACING).floor().max(0.0) as usize
    }

    /// The height of each of `bar_count` bars, from 0.0 to 1.0, for
    /// `columns`. It is worked out again only when `columns` is another
    /// waveform or `bar_count` differs from the last call.
    pub(crate) fn heights(&mut self, columns: &Arc<[BandPeaks]>, bar_count: usize) -> Arc<[f32]> {
        let is_current = self.bar_count == bar_count
            && self
                .columns
                .as_ref()
                .is_some_and(|kept| Arc::ptr_eq(kept, columns));
        if !is_current {
            self.heights = bars_from_columns(columns, bar_count)
                .iter()
                .map(BandPeaks::loudest)
                .collect();
            self.columns = Some(columns.clone());
            self.bar_count = bar_count;
            self.computations += 1;
        }
        self.heights.clone()
    }

    /// How many times the heights have been worked out.
    #[cfg(test)]
    pub(crate) fn computations(&self) -> usize {
        self.computations
    }
}

/// How many of `bar_count` bars are played when playing has reached
/// `progress`.
pub(crate) fn played_bars(bar_count: usize, progress: Option<f32>) -> usize {
    (0..bar_count)
        .take_while(|bar_index| is_played(*bar_index, bar_count, progress))
        .count()
}

/// A boundary between two sections of the track.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SectionTick {
    /// How far along the track the boundary is, from 0.0 to 1.0.
    pub(crate) fraction: f32,
    /// What the section that starts here is called.
    pub(crate) name: String,
}

/// The boundaries of the sections named by `cues`, in the order of the
/// cues, each named as the focus view names its section.
pub(crate) fn section_ticks(cues: &[Cue], duration_seconds: f32) -> Vec<SectionTick> {
    if duration_seconds <= 0.0 {
        return Vec::new();
    }
    cues.iter()
        .filter_map(|cue| {
            let section = CueSection::from_name(&cue.name)?;
            let fraction = (cue.seconds as f32 / duration_seconds).clamp(0.0, 1.0);
            let name = match section {
                CueSection::Section(_) => cue.name.clone(),
                other => SectionKind::from(other).label().to_string(),
            };
            (fraction >= FIRST_TICK_FRACTION).then_some(SectionTick { fraction, name })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn columns(length: usize) -> Arc<[BandPeaks]> {
        (0..length)
            .map(|index| BandPeaks {
                low: index as f32 / length as f32,
                mid: 0.1,
                high: 0.0,
            })
            .collect()
    }

    fn cue(seconds: f64, name: &str) -> Cue {
        Cue {
            seconds,
            name: name.to_string(),
            hot_cue: None,
            energy: None,
        }
    }

    #[test]
    fn the_heights_are_worked_out_once_per_waveform_and_width() {
        let mut cache = BarPeaksCache::default();
        let first = columns(200);
        let heights = cache.heights(&first, 50);
        assert_eq!(heights.len(), 50);
        for _ in 0..20 {
            cache.heights(&first, 50);
        }
        assert_eq!(cache.computations(), 1);

        assert_eq!(cache.heights(&first, 80).len(), 80);
        assert_eq!(cache.computations(), 2);

        cache.heights(&columns(200), 80);
        assert_eq!(cache.computations(), 3);
    }

    #[test]
    fn bars_that_fit_follow_the_width() {
        assert_eq!(BarPeaksCache::bar_count_for(300.0), 100);
        assert_eq!(BarPeaksCache::bar_count_for(2.0), 0);
    }

    #[test]
    fn bars_split_into_played_and_unplayed_at_the_position() {
        assert_eq!(played_bars(10, None), 0);
        assert_eq!(played_bars(10, Some(0.0)), 0);
        assert_eq!(played_bars(10, Some(0.32)), 3);
        assert_eq!(played_bars(10, Some(0.5)), 5);
        assert_eq!(played_bars(10, Some(1.0)), 10);
    }

    #[test]
    fn sections_are_named_as_the_focus_view_names_them() {
        let ticks = section_ticks(
            &[
                cue(0.0, "Start"),
                cue(30.0, "Breakdown"),
                cue(60.0, "Drop"),
                cue(75.0, "Section 3"),
                cue(80.0, "My own cue"),
                cue(90.0, "Outro"),
            ],
            100.0,
        );
        let names: Vec<&str> = ticks.iter().map(|tick| tick.name.as_str()).collect();
        assert_eq!(names, ["Breakdown", "Drop", "Section 3", "Outro"]);
        assert!((ticks[0].fraction - 0.3).abs() < 1e-6);
        assert!(section_ticks(&[cue(5.0, "Drop")], 0.0).is_empty());
    }
}
