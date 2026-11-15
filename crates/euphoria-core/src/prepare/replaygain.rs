// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Writing the loudness of a track into its tags as ReplayGain 2.0, without
//! touching the audio, and putting the earlier values back.

use std::path::Path;

use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::probe::Probe;
use lofty::tag::{ItemKey, Tag, TagExt};
use serde::{Deserialize, Serialize};

use super::{TrackPlan, failure};
use crate::error::EuphoriaError;
use crate::shift::{ExistingOutput, create_temporary_file, install};

/// The loudness ReplayGain 2.0 plays tracks at, in LUFS.
pub const REPLAYGAIN_REFERENCE_LUFS: f32 = -18.0;

/// The ReplayGain fields of a track as the tag had them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayGainFields {
    /// `REPLAYGAIN_TRACK_GAIN`, such as `-3.20 dB`.
    pub gain: Option<String>,
    /// `REPLAYGAIN_TRACK_PEAK`, a linear number such as `0.987654`.
    pub peak: Option<String>,
}

impl ReplayGainFields {
    /// The fields a plan gives: the gain that brings the track to the
    /// ReplayGain reference, and its sample peak. `None` for a track with
    /// no loudness.
    pub fn from_plan(plan: &TrackPlan) -> Option<Self> {
        let lufs = plan.measured_lufs?;
        let peak = plan
            .sample_peak_db
            .map_or(0.0, |decibels| 10.0_f32.powf(decibels / 20.0));
        Some(Self {
            gain: Some(format!("{:+.2} dB", REPLAYGAIN_REFERENCE_LUFS - lufs)),
            peak: Some(format!("{peak:.6}")),
        })
    }
}

fn read_error(path: &Path, error: impl std::fmt::Display) -> EuphoriaError {
    failure(path, format!("cannot read its tags: {error}"))
}

/// The ReplayGain fields now in the file.
pub fn read_replaygain(path: &Path) -> Result<ReplayGainFields, EuphoriaError> {
    let file = Probe::open(path)
        .map_err(|error| read_error(path, error))?
        .read()
        .map_err(|error| read_error(path, error))?;
    let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else {
        return Ok(ReplayGainFields::default());
    };
    Ok(ReplayGainFields {
        gain: tag
            .get_string(ItemKey::ReplayGainTrackGain)
            .map(str::to_owned),
        peak: tag
            .get_string(ItemKey::ReplayGainTrackPeak)
            .map(str::to_owned),
    })
}

/// Sets the ReplayGain fields of the file to `fields`, removing a field
/// that is `None`. The audio is not touched: the file is copied, the tag of
/// the copy is changed, and the copy takes the place of the file in one
/// step, so a failure never leaves half a file.
pub fn write_replaygain(path: &Path, fields: &ReplayGainFields) -> Result<(), EuphoriaError> {
    let file = Probe::open(path)
        .map_err(|error| read_error(path, error))?
        .read()
        .map_err(|error| read_error(path, error))?;
    let mut tag = file
        .primary_tag()
        .or_else(|| file.first_tag())
        .cloned()
        .unwrap_or_else(|| Tag::new(file.file_type().primary_tag_type()));
    for (key, value) in [
        (ItemKey::ReplayGainTrackGain, &fields.gain),
        (ItemKey::ReplayGainTrackPeak, &fields.peak),
    ] {
        match value {
            Some(value) => {
                tag.insert_text(key, value.clone());
            }
            None => tag.remove_key(key),
        }
    }
    let (copy, temporary) =
        create_temporary_file(path).map_err(|source| EuphoriaError::ExportFile {
            path: path.to_path_buf(),
            source,
        })?;
    drop(copy);
    let outcome = std::fs::copy(path, &temporary)
        .map_err(|source| EuphoriaError::ExportFile {
            path: path.to_path_buf(),
            source,
        })
        .and_then(|_| {
            tag.save_to_path(&temporary, WriteOptions::default())
                .map_err(|error| failure(path, format!("cannot write the tags: {error}")))
        })
        .and_then(|()| install(&temporary, path, ExistingOutput::Replace));
    if outcome.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    outcome
}
