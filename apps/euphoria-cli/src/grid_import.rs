// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Puts the beat grid of a rekordbox collection in place of the detected
//! one, track by track, and says where each track's grid came from.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use euphoria_core::FileResult;
use euphoria_core::rekordbox_grid::{
    GridChoice, GridSource, RekordboxGrid, VariableReason, choose_grid, read_collection_grids,
};
use rayon::prelude::*;

use crate::batch::{AnalysisOptions, UserEditsByFile};

/// The grids of a rekordbox collection, by the file each track is.
#[derive(Debug, Default)]
pub struct CollectionGrids {
    by_path: HashMap<PathBuf, RekordboxGrid>,
}

impl CollectionGrids {
    /// Reads the collection at `path`. A track is found under the path
    /// rekordbox wrote and, when that differs, under the path with every
    /// symbolic link followed, as the cue protection of an export finds it.
    pub fn read(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("cannot read the rekordbox collection {}", path.display()))?;
        let tracks = read_collection_grids(&text)
            .with_context(|| format!("cannot read the rekordbox collection {}", path.display()))?;
        let mut by_path = HashMap::new();
        for track in tracks {
            let Some(listed) = track.path else { continue };
            if let Ok(resolved) = std::fs::canonicalize(&listed) {
                by_path.insert(resolved, track.grid.clone());
            }
            by_path.insert(listed, track.grid);
        }
        Ok(Self { by_path })
    }

    fn lookup(&self, path: &Path) -> Option<&RekordboxGrid> {
        self.by_path.get(path).or_else(|| {
            std::fs::canonicalize(path)
                .ok()
                .and_then(|resolved| self.by_path.get(&resolved))
        })
    }
}

/// What the collection said about a track, when one was given.
#[derive(Debug, Clone, PartialEq)]
pub enum CollectionStatus {
    /// No collection was given.
    NotGiven,
    /// The track is not in the collection.
    NotInCollection,
    /// The track is in the collection with this grid.
    Found(RekordboxGrid),
}

impl CollectionStatus {
    /// The word JSON gives the status, or `None` when no collection was
    /// given.
    pub fn label(&self) -> Option<&'static str> {
        match self {
            CollectionStatus::NotGiven => None,
            CollectionStatus::NotInCollection => Some("not in collection"),
            CollectionStatus::Found(grid) => Some(match grid {
                RekordboxGrid::Constant(_) => "used",
                RekordboxGrid::Variable { .. } => "variable",
                RekordboxGrid::UnsupportedMeter { .. } => "unsupported meter",
                RekordboxGrid::Unreadable => "unreadable",
                RekordboxGrid::Missing => "no grid",
            }),
        }
    }

    /// Why the collection's grid was not used, in words, or `None` when it
    /// was used or there was nothing to use.
    pub fn skipped_because(&self) -> Option<String> {
        let CollectionStatus::Found(grid) = self else {
            return None;
        };
        match grid {
            RekordboxGrid::Variable {
                markers,
                slowest_bpm,
                fastest_bpm,
                reason,
            } => Some(match reason {
                VariableReason::TempoChanges => format!(
                    "its rekordbox grid changes tempo ({slowest_bpm:.2} to {fastest_bpm:.2} BPM \
                     over {markers} markers), which Euphoria's constant grid cannot follow"
                ),
                VariableReason::GridShift => format!(
                    "its rekordbox grid shifts between its {markers} markers, which Euphoria's \
                     constant grid cannot follow"
                ),
            }),
            RekordboxGrid::UnsupportedMeter { meter } => Some(format!(
                "its rekordbox grid is in {meter}, and Euphoria's grid counts 4/4 bars"
            )),
            RekordboxGrid::Unreadable => Some(
                "a marker of its rekordbox grid has no readable position, tempo or beat"
                    .to_string(),
            ),
            RekordboxGrid::Constant(_) | RekordboxGrid::Missing => None,
        }
    }
}

/// Where the grid of one track came from.
#[derive(Debug, Clone, PartialEq)]
pub struct GridInfo {
    /// Whose grid the track has, or `None` when it has none.
    pub source: Option<GridSource>,
    /// What the collection said.
    pub collection: CollectionStatus,
}

/// The grid information of a batch, by file.
pub type GridInfos = HashMap<PathBuf, GridInfo>;

/// Gives every analyzed track the grid [`choose_grid`] picks: the
/// rekordbox grid where `grids` has a constant one and the user did not set
/// the tempo, whose cues are placed again on its bars. Returns where every
/// track's grid came from. A file that cannot be decoded again to place its
/// cues becomes a failure, as it would have in the analysis.
pub fn apply(
    results: Vec<FileResult>,
    edits: &UserEditsByFile,
    grids: Option<&CollectionGrids>,
    options: &AnalysisOptions,
) -> Result<(Vec<FileResult>, GridInfos)> {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(options.job_count())
        .build()
        .context("starting the workers")?;
    let applied: Vec<(FileResult, Option<GridInfo>)> = pool.install(|| {
        results
            .into_par_iter()
            .map(|result| apply_to_one(result, edits, grids))
            .collect()
    });
    let mut infos = GridInfos::new();
    let results = applied
        .into_iter()
        .map(|(result, info)| {
            if let Some(info) = info {
                infos.insert(result.path.clone(), info);
            }
            result
        })
        .collect();
    Ok((results, infos))
}

fn apply_to_one(
    mut result: FileResult,
    edits: &UserEditsByFile,
    grids: Option<&CollectionGrids>,
) -> (FileResult, Option<GridInfo>) {
    let Ok(analysis) = &result.outcome else {
        return (result, None);
    };
    let correction = edits
        .get(&result.path)
        .and_then(|looked_up| looked_up.as_ref().ok())
        .map(|edits| edits.correction)
        .unwrap_or_default();
    let collection = match grids {
        None => CollectionStatus::NotGiven,
        Some(grids) => grids
            .lookup(&result.path)
            .cloned()
            .map_or(CollectionStatus::NotInCollection, CollectionStatus::Found),
    };
    let rekordbox = match &collection {
        CollectionStatus::Found(grid) => Some(grid),
        _ => None,
    };
    let choice = choose_grid(&correction, rekordbox);
    let source = match choice {
        GridChoice::Rekordbox(grid) => {
            match analysis
                .clone()
                .on_imported_grid_of_file(grid, &result.path)
            {
                Ok(placed) => {
                    result.outcome = Ok(placed);
                    Some(GridSource::Rekordbox)
                }
                Err(error) => {
                    result.outcome = Err(error);
                    None
                }
            }
        }
        GridChoice::User => Some(GridSource::User),
        GridChoice::Detected => analysis.beat_grid.map(|_| GridSource::Detected),
    };
    (result, Some(GridInfo { source, collection }))
}

/// How many tracks got the rekordbox grid, and why the others with a
/// rekordbox grid did not.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportSummary {
    /// Tracks that use the grid of the collection.
    pub used: usize,
    /// Tracks of the batch that are not in the collection.
    pub not_in_collection: usize,
    /// Tracks in the collection with no grid there.
    pub no_grid: usize,
    /// Tracks whose grid was left out, with the file and the reason.
    pub skipped: Vec<(PathBuf, String)>,
}

impl ImportSummary {
    /// Counts `infos`, in the order of `files`.
    pub fn of(files: &[PathBuf], infos: &GridInfos) -> Self {
        let mut summary = Self::default();
        for path in files {
            let Some(info) = infos.get(path) else {
                continue;
            };
            if info.source == Some(GridSource::Rekordbox) {
                summary.used += 1;
            }
            match (&info.collection, info.collection.skipped_because()) {
                (_, Some(reason)) => summary.skipped.push((path.clone(), reason)),
                (CollectionStatus::NotInCollection, None) => summary.not_in_collection += 1,
                (CollectionStatus::Found(RekordboxGrid::Missing), None) => summary.no_grid += 1,
                _ => {}
            }
        }
        summary
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skipped_grid_is_explained_and_a_used_or_missing_one_is_not() {
        let variable = CollectionStatus::Found(RekordboxGrid::Variable {
            markers: 3,
            slowest_bpm: 120.0,
            fastest_bpm: 126.0,
            reason: VariableReason::TempoChanges,
        });
        let reason = variable.skipped_because().unwrap();
        assert!(
            reason.contains("120.00 to 126.00 BPM over 3 markers"),
            "{reason}"
        );
        assert_eq!(variable.label(), Some("variable"));
        assert_eq!(
            CollectionStatus::Found(RekordboxGrid::Missing).skipped_because(),
            None
        );
        assert_eq!(CollectionStatus::NotGiven.label(), None);
        assert_eq!(
            CollectionStatus::NotInCollection.label(),
            Some("not in collection")
        );
    }
}
