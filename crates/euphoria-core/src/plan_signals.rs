// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the transition planner reads of a track beyond its analysis: the
//! energy over time and the levels of its stems. Both take seconds to
//! measure, so they are kept in a folder of small files named by the
//! content of the track, and a second plan reads them back instantly.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::audio::decode_mono;
use crate::cache::ContentHash;
use crate::energy::{ENERGY_MODEL_VERSION, energy_curve};
use crate::error::EuphoriaError;
use crate::harmony::transition::{EnergyPoint, StemLevels};

/// The energy and stem levels of one track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackSignals {
    /// The [`ENERGY_MODEL_VERSION`] the energy was rated with.
    pub energy_model_version: u32,
    /// The energy over time.
    pub energy: Vec<EnergyPoint>,
    /// The stem levels, or `None` when the track had no cached stems when
    /// they were last looked for.
    pub stems: Option<StemLevels>,
}

/// The folder the signals of tracks are kept in.
#[derive(Debug, Clone)]
pub struct SignalsCache {
    directory: PathBuf,
}

impl SignalsCache {
    /// The `plan-signals` folder of Euphoria's data folder.
    pub fn open_default() -> Result<Self, EuphoriaError> {
        Ok(Self::new(
            crate::cache::location::default_data_directory()?.join("plan-signals"),
        ))
    }

    /// A cache in `directory`.
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    fn file(&self, hash: &ContentHash) -> PathBuf {
        self.directory.join(format!("{}.json", hash.to_hex()))
    }

    /// The signals saved for the track with content `hash`, unless they were
    /// rated with another energy model or cannot be read.
    pub fn load(&self, hash: &ContentHash) -> Option<TrackSignals> {
        let bytes = std::fs::read(self.file(hash)).ok()?;
        let signals: TrackSignals = serde_json::from_slice(&bytes).ok()?;
        (signals.energy_model_version == ENERGY_MODEL_VERSION).then_some(signals)
    }

    /// Saves `signals` for the track with content `hash`, through a
    /// temporary file so that a reader never sees half of it.
    pub fn store(&self, hash: &ContentHash, signals: &TrackSignals) -> Result<(), EuphoriaError> {
        let failed = |path: &Path, source: std::io::Error| EuphoriaError::Scan {
            path: path.to_path_buf(),
            source,
        };
        std::fs::create_dir_all(&self.directory)
            .map_err(|source| failed(&self.directory, source))?;
        let target = self.file(hash);
        let temporary = target.with_extension(format!("tmp{}", std::process::id()));
        let bytes = serde_json::to_vec(signals).map_err(|error| EuphoriaError::Scan {
            path: target.clone(),
            source: std::io::Error::other(error),
        })?;
        std::fs::write(&temporary, bytes).map_err(|source| failed(&temporary, source))?;
        std::fs::rename(&temporary, &target).map_err(|source| failed(&target, source))
    }
}

/// Rates the energy of the file at `path` over time, decoding it.
pub fn measure_energy(path: &Path, tempo: Option<f32>) -> Result<Vec<EnergyPoint>, EuphoriaError> {
    let audio = decode_mono(path)?;
    Ok(energy_curve(&audio, tempo)?
        .iter()
        .map(EnergyPoint::from)
        .collect())
}

/// A [`TrackSignals`] with the energy model of this build and nothing
/// measured of the stems.
pub fn signals_with_energy(energy: Vec<EnergyPoint>) -> TrackSignals {
    TrackSignals {
        energy_model_version: ENERGY_MODEL_VERSION,
        energy,
        stems: None,
    }
}
