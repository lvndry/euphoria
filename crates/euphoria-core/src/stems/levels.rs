// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The level of the vocals, bass and drums stems over short stretches of a
//! track, which the transition planner turns into lanes of bars.

use super::profile::StemWav;
use super::{STEMS_SAMPLE_RATE, Stem, StemFiles};
use crate::error::EuphoriaError;
use crate::harmony::transition::{STEM_LEVEL_HOP_SECONDS, StemLevels};

/// Measures the root mean square level of three stems of `files` for every
/// [`STEM_LEVEL_HOP_SECONDS`], reading each file once.
pub fn measure_stem_levels(files: &StemFiles) -> Result<StemLevels, EuphoriaError> {
    let hop_frames = (STEM_LEVEL_HOP_SECONDS * f64::from(STEMS_SAMPLE_RATE)) as usize;
    let measure = |stem: Stem| -> Result<Vec<f32>, EuphoriaError> {
        let mut reader = StemWav::open(&files.path(stem))?;
        let mut window: Vec<Vec<f32>> = Vec::new();
        let mut levels = Vec::new();
        loop {
            reader.read_channels(&mut window, hop_frames)?;
            let samples: usize = window.iter().map(Vec::len).sum();
            if samples == 0 {
                break;
            }
            let squares: f32 = window
                .iter()
                .flat_map(|channel| channel.iter())
                .map(|sample| sample * sample)
                .sum();
            levels.push((squares / samples as f32).sqrt());
        }
        Ok(levels)
    };
    Ok(StemLevels {
        hop_seconds: STEM_LEVEL_HOP_SECONDS,
        vocals: measure(Stem::Vocals)?,
        bass: measure(Stem::Bass)?,
        drums: measure(Stem::Drums)?,
    })
}
