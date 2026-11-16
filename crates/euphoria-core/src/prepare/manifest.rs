// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What a run of prepare made, written next to its output so the run can be
//! reverted: the files it wrote, and the tags it changed in place, each with
//! a checksum. A revert only removes a file the manifest names whose bytes
//! are still what the run wrote.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::replaygain::{ReplayGainFields, write_replaygain};
use super::{PrepareSettings, failure};
use crate::error::EuphoriaError;

/// The start of the name of a manifest file.
pub const MANIFEST_PREFIX: &str = "euphoria-prepare-";

const MANIFEST_VERSION: u32 = 1;

/// What a run did to each track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunMode {
    /// New files were written.
    Copies,
    /// ReplayGain tags were written into the originals.
    TagOnly,
}

/// The settings of a run, as recorded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedSettings {
    /// Target loudness in LUFS.
    pub target_lufs: f32,
    /// True-peak ceiling in dBTP.
    pub true_peak_ceiling_db: f32,
    /// The limiting allowance in dB.
    pub maximum_limiting_db: f32,
    /// Whether tuning was corrected.
    pub correct_tuning: bool,
}

/// One track of a run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// The original.
    pub source: PathBuf,
    /// BLAKE3 of the original when the run started.
    pub source_blake3: String,
    /// The file written; `None` for a tag-only run.
    pub output: Option<PathBuf>,
    /// BLAKE3 of the file written, or, for a tag-only run, of the original
    /// after its tags were changed.
    pub result_blake3: String,
    /// The ReplayGain fields the original had before a tag-only run.
    pub previous_replaygain: Option<ReplayGainFields>,
}

/// A run of prepare.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    /// The layout of this file.
    pub version: u32,
    /// When the run started, in seconds since 1970.
    pub created_unix_seconds: u64,
    /// What the run did.
    pub mode: RunMode,
    /// The settings of the run.
    pub settings: RecordedSettings,
    /// Every track the run changed or wrote.
    pub entries: Vec<ManifestEntry>,
}

/// The BLAKE3 checksum of the file at `path`, in hex.
pub fn checksum(path: &Path) -> Result<String, EuphoriaError> {
    let io_error = |source| EuphoriaError::ExportFile {
        path: path.to_path_buf(),
        source,
    };
    let mut file = std::fs::File::open(path).map_err(io_error)?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = vec![0_u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

impl Manifest {
    /// An empty record of a run that starts now.
    pub fn new(mode: RunMode, settings: &PrepareSettings) -> Self {
        Self {
            version: MANIFEST_VERSION,
            created_unix_seconds: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_secs()),
            mode,
            settings: RecordedSettings {
                target_lufs: settings.target_lufs,
                true_peak_ceiling_db: settings.true_peak_ceiling_db,
                maximum_limiting_db: settings.maximum_limiting_db,
                correct_tuning: settings.correct_tuning,
            },
            entries: Vec::new(),
        }
    }

    /// Records a file written from `source`.
    pub fn record_copy(&mut self, source: &Path, output: &Path) -> Result<(), EuphoriaError> {
        self.entries.push(ManifestEntry {
            source: source.to_path_buf(),
            source_blake3: checksum(source)?,
            output: Some(output.to_path_buf()),
            result_blake3: checksum(output)?,
            previous_replaygain: None,
        });
        Ok(())
    }

    /// The name of the manifest file of this run.
    pub fn file_name(&self) -> String {
        format!("{MANIFEST_PREFIX}{}.json", self.created_unix_seconds)
    }

    /// Writes the manifest into `directory`, under a name that is not
    /// taken, and returns its path.
    pub fn save(&self, directory: &Path) -> Result<PathBuf, EuphoriaError> {
        let mut path = directory.join(self.file_name());
        let mut number = 2;
        while path.exists() {
            path = directory.join(format!(
                "{MANIFEST_PREFIX}{}-{number}.json",
                self.created_unix_seconds
            ));
            number += 1;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|error| failure(&path, error.to_string()))?;
        std::fs::write(&path, text).map_err(|source| EuphoriaError::ExportFile {
            path: path.clone(),
            source,
        })?;
        Ok(path)
    }

    /// Reads a manifest.
    pub fn load(path: &Path) -> Result<Self, EuphoriaError> {
        let text = std::fs::read_to_string(path).map_err(|source| EuphoriaError::ExportFile {
            path: path.to_path_buf(),
            source,
        })?;
        let manifest: Self = serde_json::from_str(&text)
            .map_err(|error| failure(path, format!("not a prepare manifest: {error}")))?;
        if manifest.version != MANIFEST_VERSION {
            return Err(failure(path, "a manifest of a newer version of Euphoria"));
        }
        Ok(manifest)
    }
}

/// The manifests in `folder`, oldest first.
pub fn manifests_in(folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(MANIFEST_PREFIX) && name.ends_with(".json"))
        })
        .collect();
    found.sort();
    found
}

/// What reverting a manifest did.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct RevertReport {
    /// Files the run wrote, now removed.
    pub removed: Vec<PathBuf>,
    /// Originals whose tags were put back.
    pub restored: Vec<PathBuf>,
    /// Entries left alone, with the reason.
    pub kept: Vec<(PathBuf, String)>,
}

/// Undoes the run `manifest_path` records: removes the files it wrote and
/// puts back the tags it changed. A file is only touched when it is still
/// byte for byte what the run left; any other file, including one the user
/// changed since, is kept and reported. The manifest is removed when
/// everything in it is undone, and keeps the entries that are left
/// otherwise.
pub fn revert(manifest_path: &Path) -> Result<RevertReport, EuphoriaError> {
    let mut manifest = Manifest::load(manifest_path)?;
    let (report, remaining) = revert_entries(std::mem::take(&mut manifest.entries));
    if remaining.is_empty() {
        let _ = std::fs::remove_file(manifest_path);
    } else {
        manifest.entries = remaining;
        let text = serde_json::to_string_pretty(&manifest)
            .map_err(|error| failure(manifest_path, error.to_string()))?;
        std::fs::write(manifest_path, text).map_err(|source| EuphoriaError::ExportFile {
            path: manifest_path.to_path_buf(),
            source,
        })?;
    }
    Ok(report)
}

/// Takes back a run whose manifest could not be saved. Only files still
/// holding the exact result of the run are touched. The caller must report
/// any entries that could not be put back.
pub fn revert_unrecorded(manifest: &Manifest) -> RevertReport {
    revert_entries(manifest.entries.clone()).0
}

fn revert_entries(entries: Vec<ManifestEntry>) -> (RevertReport, Vec<ManifestEntry>) {
    let mut report = RevertReport::default();
    let mut remaining = Vec::new();
    let mut seen = HashSet::new();
    for entry in entries {
        let target = entry.output.clone().unwrap_or_else(|| entry.source.clone());
        if !seen.insert(target.clone()) {
            continue;
        }
        if !target.exists() {
            report.kept.push((target, "it is already gone".to_string()));
            continue;
        }
        let current = match checksum(&target) {
            Ok(current) => current,
            Err(error) => {
                report.kept.push((target.clone(), error.chain()));
                remaining.push(entry);
                continue;
            }
        };
        if current != entry.result_blake3 {
            report
                .kept
                .push((target.clone(), "it changed after the run".to_string()));
            remaining.push(entry);
            continue;
        }
        let outcome = match (&entry.output, &entry.previous_replaygain) {
            (Some(output), _) => std::fs::remove_file(output)
                .map(|()| report.removed.push(output.clone()))
                .map_err(|error| error.to_string()),
            (None, Some(previous)) => write_replaygain(&entry.source, previous)
                .map(|()| report.restored.push(entry.source.clone()))
                .map_err(|error| error.chain()),
            (None, None) => Err("the manifest has no earlier tags for it".to_string()),
        };
        if let Err(reason) = outcome {
            report.kept.push((target, reason));
            remaining.push(entry);
        }
    }
    (report, remaining)
}

/// Runs a tag-only pass over one track: records the tags it had, writes the
/// ReplayGain fields of `plan` into the file, and records the result in
/// `manifest`.
pub fn tag_track(
    manifest: &mut Manifest,
    source: &Path,
    fields: &ReplayGainFields,
) -> Result<(), EuphoriaError> {
    let before = checksum(source)?;
    let previous = super::replaygain::read_replaygain(source)?;
    write_replaygain(source, fields)?;
    manifest.entries.push(ManifestEntry {
        source: source.to_path_buf(),
        source_blake3: before,
        output: None,
        result_blake3: checksum(source)?,
        previous_replaygain: Some(previous),
    });
    Ok(())
}
