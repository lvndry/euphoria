// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tracks with a reference tempo and a genre, from GiantSteps Tempo and from
//! GTZAN excerpts of any genres, for the benchmarks that score per genre.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::beats::annotated_tempo;
use crate::dataset;

/// A track with its reference tempo, its genre, and for GTZAN its
/// annotated beats.
pub(crate) struct Reference {
    pub(crate) name: String,
    pub(crate) audio_path: PathBuf,
    pub(crate) tempo: f32,
    pub(crate) genre: String,
    pub(crate) beats: Option<Vec<f64>>,
}

pub(crate) fn giantsteps_references(directory: &Path) -> Result<Vec<Reference>> {
    dataset::load_tempo_tracks(directory)?
        .tracks
        .into_iter()
        .map(|track| {
            let genre_path = directory
                .join("annotations/genre")
                .join(format!("{}.genre", track.name));
            let genre = std::fs::read_to_string(&genre_path)
                .with_context(|| format!("reading {}", genre_path.display()))?;
            Ok(Reference {
                name: track.name,
                audio_path: track.audio_path,
                tempo: track.annotation,
                genre: genre.trim().to_string(),
                beats: None,
            })
        })
        .collect()
}

/// The GTZAN excerpts in `directory`, of any genres: an annotation
/// `annotations/gtzan_<genre>_<number>.beats` with its audio
/// `audio/<genre>.<number>.wav`. The reference tempo is the median beat
/// interval.
pub(crate) fn gtzan_references(directory: &Path) -> Result<Vec<Reference>> {
    let annotations = directory.join("annotations");
    let mut references = Vec::new();
    for entry in std::fs::read_dir(&annotations)
        .with_context(|| format!("reading {}", annotations.display()))?
    {
        let path = entry?.path();
        let Some(name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".beats"))
        else {
            continue;
        };
        let Some((genre, number)) = name
            .strip_prefix("gtzan_")
            .and_then(|rest| rest.split_once('_'))
        else {
            continue;
        };
        let audio_path = directory
            .join("audio")
            .join(format!("{genre}.{number}.wav"));
        if !audio_path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let beats = beat_times(&text).with_context(|| format!("reading {}", path.display()))?;
        if beats.len() < 2 {
            continue;
        }
        references.push(Reference {
            name: name.to_string(),
            audio_path,
            tempo: annotated_tempo(&beats) as f32,
            genre: genre.to_string(),
            beats: Some(beats),
        });
    }
    references.sort_by(|first, second| first.name.cmp(&second.name));
    Ok(references)
}

/// The beat times of an annotation, one per line in the first column. Some
/// GTZAN annotations give no position in the bar, so further columns are
/// not read.
fn beat_times(text: &str) -> Result<Vec<f64>> {
    let mut times: Vec<f64> = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let Some(field) = line.split_whitespace().next() else {
            continue;
        };
        let time: f64 = field
            .parse()
            .ok()
            .filter(|time: &f64| time.is_finite() && *time >= 0.0)
            .with_context(|| format!("line {}: {field:?} is not a time", index + 1))?;
        if times.last().is_some_and(|previous| time <= *previous) {
            bail!(
                "line {}: {time} does not come after the beat before it",
                index + 1
            );
        }
        times.push(time);
    }
    Ok(times)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beat_times_read_the_first_column_with_or_without_a_position() {
        assert_eq!(beat_times("0.5 1\n1.0 2\n").unwrap(), [0.5, 1.0]);
        assert_eq!(beat_times("0.144\n0.6\n\n").unwrap(), [0.144, 0.6]);
        assert!(beat_times("1.0\n0.5\n").is_err());
        assert!(beat_times("soon\n").is_err());
    }
}
