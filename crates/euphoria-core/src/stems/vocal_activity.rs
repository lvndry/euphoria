// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where the vocals are in a separated track: the level of the vocals stem
//! over the same windows the energy curve is rated over, kept next to the
//! stems so that it is measured once, and read as active or not against the
//! track's own vocal level.

use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::profile::StemWav;
use super::{STEMS_SAMPLE_RATE, Stem, StemFiles};
use crate::energy::{ENERGY_CURVE_HOP_SECONDS, ENERGY_CURVE_WINDOW_SECONDS};
use crate::error::EuphoriaError;

/// The name of the file the levels are kept in, in the folder of the stems.
pub const VOCAL_ACTIVITY_FILE_NAME: &str = "vocal-levels.json";

/// Changes when the way levels are measured does, so that levels kept by
/// another way are measured again.
const LEVELS_VERSION: u32 = 1;

/// The level given to a window of digital silence, in dB below full scale.
const SILENT_LEVEL_DB: f32 = -120.0;

/// The vocal level of a track is the level that this share of its windows
/// reach: the loud passages, but not a single loud shout.
const REFERENCE_SHARE: f32 = 0.9;

/// A window has vocals when its level is within this many dB of the vocal
/// level of the track.
const RELATIVE_THRESHOLD_DB: f32 = 20.0;

/// Whatever the track's own level, a window quieter than this, in dB below
/// full scale, has no vocals: the stem of an instrumental carries only what
/// the model leaks into it.
const ABSOLUTE_FLOOR_DB: f32 = -50.0;

/// One window of the vocals stem.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VocalWindow {
    /// Where the window starts, in seconds.
    pub start_seconds: f32,
    /// How long the window is, in seconds.
    pub duration_seconds: f32,
    /// Whether the vocals sound in it.
    pub active: bool,
}

/// The windows of a track, each with whether the vocals sound in it.
#[derive(Debug, Clone, PartialEq)]
pub struct VocalActivity {
    /// The windows from the start of the track, as the energy curve has
    /// them: [`ENERGY_CURVE_WINDOW_SECONDS`] long every
    /// [`ENERGY_CURVE_HOP_SECONDS`].
    pub windows: Vec<VocalWindow>,
}

/// The level of the vocals stem in each window, in dB below full scale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VocalLevels {
    version: u32,
    window_seconds: f32,
    hop_seconds: f32,
    vocals_bytes: u64,
    levels_db: Vec<f32>,
    window_lengths_seconds: Vec<f32>,
}

impl VocalLevels {
    /// Levels for windows that start every hop and last `window_lengths`
    /// seconds each.
    #[cfg(test)]
    fn new(levels_db: Vec<f32>) -> Self {
        let window_lengths_seconds = vec![ENERGY_CURVE_WINDOW_SECONDS; levels_db.len()];
        Self {
            version: LEVELS_VERSION,
            window_seconds: ENERGY_CURVE_WINDOW_SECONDS,
            hop_seconds: ENERGY_CURVE_HOP_SECONDS,
            vocals_bytes: 0,
            levels_db,
            window_lengths_seconds,
        }
    }

    /// The level of each window, in dB below full scale.
    pub fn levels_db(&self) -> &[f32] {
        &self.levels_db
    }

    /// Measures the vocals stem of `files`, reading it once.
    pub fn measure(files: &StemFiles) -> Result<Self, EuphoriaError> {
        let path = files.path(Stem::Vocals);
        let vocals_bytes = fs::metadata(&path)
            .map_err(|source| EuphoriaError::StemFiles {
                path: path.clone(),
                source,
            })?
            .len();
        let mut wav = StemWav::open(&path)?;
        let hop_frames = (ENERGY_CURVE_HOP_SECONDS * STEMS_SAMPLE_RATE as f32) as usize;
        let window_frames = (ENERGY_CURVE_WINDOW_SECONDS * STEMS_SAMPLE_RATE as f32) as usize;
        let mut hops: Vec<Hop> = Vec::new();
        let mut block = Vec::new();
        loop {
            wav.read_channels(&mut block, hop_frames)?;
            let frames = block.iter().map(Vec::len).min().unwrap_or(0);
            if frames == 0 {
                break;
            }
            let energy: f64 = block
                .iter()
                .flat_map(|channel| channel.iter())
                .map(|sample| f64::from(*sample) * f64::from(*sample))
                .sum();
            hops.push(Hop {
                energy,
                frames,
                samples: frames * block.len(),
            });
        }
        let total_frames: usize = hops.iter().map(|hop| hop.frames).sum();
        let (levels_db, window_lengths_seconds) = if total_frames == 0 {
            (Vec::new(), Vec::new())
        } else if total_frames < window_frames {
            let energy: f64 = hops.iter().map(|hop| hop.energy).sum();
            let samples: usize = hops.iter().map(|hop| hop.samples).sum();
            (
                vec![level_db(energy, samples)],
                vec![total_frames as f32 / STEMS_SAMPLE_RATE as f32],
            )
        } else {
            let count = (total_frames - window_frames) / hop_frames + 1;
            let levels = (0..count)
                .map(|index| {
                    let pair = &hops[index..index + 2];
                    level_db(
                        pair.iter().map(|hop| hop.energy).sum(),
                        pair.iter().map(|hop| hop.samples).sum(),
                    )
                })
                .collect();
            (levels, vec![ENERGY_CURVE_WINDOW_SECONDS; count])
        };
        Ok(Self {
            version: LEVELS_VERSION,
            window_seconds: ENERGY_CURVE_WINDOW_SECONDS,
            hop_seconds: ENERGY_CURVE_HOP_SECONDS,
            vocals_bytes,
            levels_db,
            window_lengths_seconds,
        })
    }

    /// The windows with whether the vocals sound in each: within
    /// [`RELATIVE_THRESHOLD_DB`] of the level the loudest tenth of the
    /// windows reach, and above [`ABSOLUTE_FLOOR_DB`].
    pub fn activity(&self) -> VocalActivity {
        let threshold_db = self.threshold_db();
        let windows = self
            .levels_db
            .iter()
            .zip(&self.window_lengths_seconds)
            .enumerate()
            .map(|(index, (level, length))| VocalWindow {
                start_seconds: index as f32 * self.hop_seconds,
                duration_seconds: *length,
                active: *level >= threshold_db,
            })
            .collect();
        VocalActivity { windows }
    }

    fn threshold_db(&self) -> f32 {
        let mut sorted = self.levels_db.clone();
        sorted.sort_by(f32::total_cmp);
        let reference = match sorted.len() {
            0 => return f32::INFINITY,
            length => sorted[(((length - 1) as f32) * REFERENCE_SHARE).round() as usize],
        };
        (reference - RELATIVE_THRESHOLD_DB).max(ABSOLUTE_FLOOR_DB)
    }

    fn is_current(&self, vocals_bytes: u64) -> bool {
        self.version == LEVELS_VERSION
            && self.window_seconds == ENERGY_CURVE_WINDOW_SECONDS
            && self.hop_seconds == ENERGY_CURVE_HOP_SECONDS
            && self.vocals_bytes == vocals_bytes
    }

    fn read_from(directory: &Path) -> Option<Self> {
        let bytes = fs::read(directory.join(VOCAL_ACTIVITY_FILE_NAME)).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    fn write_to(&self, directory: &Path) -> io::Result<()> {
        let temporary = directory.join(format!("{VOCAL_ACTIVITY_FILE_NAME}.part"));
        fs::write(
            &temporary,
            serde_json::to_vec(self).map_err(io::Error::other)?,
        )?;
        fs::rename(temporary, directory.join(VOCAL_ACTIVITY_FILE_NAME))
    }
}

struct Hop {
    energy: f64,
    frames: usize,
    samples: usize,
}

fn level_db(energy: f64, samples: usize) -> f32 {
    if samples == 0 || energy <= 0.0 {
        return SILENT_LEVEL_DB;
    }
    ((10.0 * (energy / samples as f64).log10()) as f32).max(SILENT_LEVEL_DB)
}

/// Where the vocals sound in the separated track in `files`. The levels are
/// measured the first time and kept in the folder of the stems, so that
/// later calls read a small file instead of the stem. Levels that cannot be
/// kept, as on a read-only disk, are still returned.
pub fn vocal_activity(files: &StemFiles) -> Result<VocalActivity, EuphoriaError> {
    let path = files.path(Stem::Vocals);
    let vocals_bytes = fs::metadata(&path)
        .map_err(|source| EuphoriaError::StemFiles {
            path: path.clone(),
            source,
        })?
        .len();
    if let Some(kept) = VocalLevels::read_from(files.directory())
        && kept.is_current(vocals_bytes)
    {
        return Ok(kept.activity());
    }
    let levels = VocalLevels::measure(files)?;
    let _ = levels.write_to(files.directory());
    Ok(levels.activity())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stems::wav::write_wav;

    fn write_vocals(directory: &Path, seconds: usize, sounding: &[(f32, f32, f32)]) -> StemFiles {
        let files = StemFiles::in_directory(directory.to_path_buf());
        let rate = STEMS_SAMPLE_RATE as usize;
        let mut interleaved = vec![0.0_f32; seconds * rate * 2];
        for (from, to, amplitude) in sounding {
            for frame in (*from * rate as f32) as usize..(*to * rate as f32) as usize {
                let time = frame as f32 / rate as f32;
                let sample = amplitude * (std::f32::consts::TAU * 220.0 * time).sin();
                interleaved[frame * 2] = sample;
                interleaved[frame * 2 + 1] = sample;
            }
        }
        write_wav(
            &files.path(Stem::Vocals),
            &interleaved,
            2,
            STEMS_SAMPLE_RATE,
        )
        .unwrap();
        files
    }

    fn pattern(activity: &VocalActivity) -> String {
        activity
            .windows
            .iter()
            .map(|window| if window.active { '#' } else { '.' })
            .collect()
    }

    fn levels_pattern(levels_db: &[f32]) -> String {
        pattern(&VocalLevels::new(levels_db.to_vec()).activity())
    }

    #[test]
    fn vocals_in_known_stretches_light_up_the_windows_that_hear_them() {
        let folder = tempfile::tempdir().unwrap();
        let files = write_vocals(folder.path(), 40, &[(8.0, 24.0, 0.3)]);
        let activity = vocal_activity(&files).unwrap();
        assert_eq!(pattern(&activity), ".#####...");
        let starts: Vec<f32> = activity
            .windows
            .iter()
            .map(|window| window.start_seconds)
            .collect();
        assert_eq!(starts, [0.0, 4.0, 8.0, 12.0, 16.0, 20.0, 24.0, 28.0, 32.0]);
        assert!(
            activity
                .windows
                .iter()
                .all(|window| window.duration_seconds == ENERGY_CURVE_WINDOW_SECONDS)
        );
    }

    #[test]
    fn the_windows_are_those_the_energy_curve_rates() {
        let folder = tempfile::tempdir().unwrap();
        let files = write_vocals(folder.path(), 43, &[(0.0, 43.0, 0.3)]);
        let activity = vocal_activity(&files).unwrap();
        assert_eq!(activity.windows.len(), (43 - 8) / 4 + 1);

        let short = tempfile::tempdir().unwrap();
        let files = write_vocals(short.path(), 5, &[(0.0, 5.0, 0.3)]);
        let activity = vocal_activity(&files).unwrap();
        assert_eq!(activity.windows.len(), 1);
        assert_eq!(activity.windows[0].duration_seconds, 5.0);
        assert!(activity.windows[0].active);

        let empty = tempfile::tempdir().unwrap();
        let files = write_vocals(empty.path(), 0, &[]);
        assert!(vocal_activity(&files).unwrap().windows.is_empty());
    }

    #[test]
    fn a_stem_with_only_leakage_has_no_vocals_whatever_its_own_level() {
        assert_eq!(levels_pattern(&[-70.0, -68.0, -72.0, -69.0]), "....");
        assert_eq!(levels_pattern(&[-120.0; 4]), "....");
    }

    #[test]
    fn a_window_has_vocals_within_twenty_decibels_of_the_vocal_level_of_the_track() {
        let loud = [
            -12.0, -12.0, -12.0, -12.0, -12.0, -12.0, -12.0, -12.0, -12.0,
        ];
        let mut levels = loud.to_vec();
        levels[2] = -31.0;
        levels[5] = -33.0;
        assert_eq!(levels_pattern(&levels), "#####.###");
    }

    #[test]
    fn a_quiet_track_is_judged_by_its_own_level_and_a_loud_one_by_its_own() {
        let quiet_vocals = [
            -45.0, -45.0, -45.0, -45.0, -80.0, -80.0, -45.0, -45.0, -45.0, -45.0,
        ];
        assert_eq!(levels_pattern(&quiet_vocals), "####..####");
        let loud_with_bleed = [
            -10.0, -10.0, -10.0, -10.0, -35.0, -35.0, -10.0, -10.0, -10.0, -10.0,
        ];
        assert_eq!(levels_pattern(&loud_with_bleed), "####..####");
    }

    #[test]
    fn the_edge_of_the_threshold_is_twenty_decibels_below_the_vocal_level() {
        let mut levels = vec![-10.0_f32; 10];
        levels[0] = -29.0;
        levels[1] = -31.0;
        assert_eq!(levels_pattern(&levels), "#.########");
    }

    #[test]
    fn the_levels_are_measured_once_and_kept_with_the_stems() {
        let folder = tempfile::tempdir().unwrap();
        let files = write_vocals(folder.path(), 40, &[(8.0, 24.0, 0.3)]);
        let first = vocal_activity(&files).unwrap();
        let kept = folder.path().join(VOCAL_ACTIVITY_FILE_NAME);
        assert!(kept.is_file());
        assert!(
            !folder
                .path()
                .join(format!("{VOCAL_ACTIVITY_FILE_NAME}.part"))
                .exists()
        );

        let bytes = fs::metadata(files.path(Stem::Vocals)).unwrap().len();
        fs::write(
            files.path(Stem::Vocals),
            vec![0_u8; usize::try_from(bytes).unwrap()],
        )
        .unwrap();
        assert_eq!(
            vocal_activity(&files).unwrap(),
            first,
            "read from the kept file, not measured again"
        );

        let read = VocalLevels::read_from(folder.path()).unwrap();
        assert_eq!(read.activity(), first);
        assert_eq!(read.levels_db().len(), first.windows.len());
    }

    #[test]
    fn levels_kept_for_other_audio_or_another_measurement_are_measured_again() {
        let folder = tempfile::tempdir().unwrap();
        let files = write_vocals(folder.path(), 40, &[(8.0, 24.0, 0.3)]);
        assert_eq!(pattern(&vocal_activity(&files).unwrap()), ".#####...");

        write_vocals(folder.path(), 44, &[(0.0, 44.0, 0.3)]);
        assert_eq!(pattern(&vocal_activity(&files).unwrap()), "##########");
    }

    #[test]
    fn an_unreadable_levels_file_is_measured_again_and_a_missing_stem_is_an_error() {
        let folder = tempfile::tempdir().unwrap();
        let files = write_vocals(folder.path(), 20, &[(0.0, 20.0, 0.3)]);
        fs::write(folder.path().join(VOCAL_ACTIVITY_FILE_NAME), b"not json").unwrap();
        assert_eq!(pattern(&vocal_activity(&files).unwrap()), "####");
        assert!(VocalLevels::read_from(folder.path()).is_some());

        let missing = StemFiles::in_directory(folder.path().join("none"));
        assert!(vocal_activity(&missing).is_err());
    }
}
