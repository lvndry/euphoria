// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads a GiantSteps dataset directory into tracks with parsed annotations.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use euphoria_core::Key;

/// One dataset track: its audio file and its parsed reference annotation.
#[derive(Debug)]
pub struct Track<Annotation> {
    /// File stem shared by the audio and annotation files.
    pub name: String,
    /// Path to the track's MP3 file.
    pub audio_path: PathBuf,
    /// The reference value the detectors are scored against.
    pub annotation: Annotation,
}

/// Loads the GiantSteps Key dataset, rooted at `dataset_directory`.
pub fn load_key_tracks(dataset_directory: &Path) -> Result<Vec<Track<Key>>> {
    let tracks = load_tracks(dataset_directory, "annotations/key", parse_key_annotation)?;
    if tracks.is_empty() {
        bail!("no key dataset audio found, run bench/download_datasets.sh first");
    }
    Ok(tracks)
}

/// Loads the GiantSteps Tempo dataset, rooted at `dataset_directory`, using
/// the version 2 annotations.
pub fn load_tempo_tracks(dataset_directory: &Path) -> Result<Vec<Track<f32>>> {
    let tracks = load_tracks(
        dataset_directory,
        "annotations_v2/tempo",
        parse_tempo_annotation,
    )?;
    if tracks.is_empty() {
        bail!("no tempo dataset audio found, run bench/download_datasets.sh first");
    }
    Ok(tracks)
}

/// Pairs each annotation file with `audio/<name>.mp3`, skipping annotations
/// whose audio is absent, and returns the tracks sorted by name.
fn load_tracks<Annotation>(
    dataset_directory: &Path,
    annotation_subdirectory: &str,
    parse: impl Fn(&str) -> Result<Annotation>,
) -> Result<Vec<Track<Annotation>>> {
    let annotation_directory = dataset_directory.join(annotation_subdirectory);
    let mut tracks = Vec::new();
    for entry in fs::read_dir(&annotation_directory)
        .with_context(|| format!("reading {}", annotation_directory.display()))?
    {
        let annotation_path = entry
            .with_context(|| format!("reading {}", annotation_directory.display()))?
            .path();
        let Some(name) = annotation_path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let audio_path = dataset_directory.join("audio").join(format!("{name}.mp3"));
        if !audio_path.is_file() {
            continue;
        }
        let text = fs::read_to_string(&annotation_path)
            .with_context(|| format!("reading {}", annotation_path.display()))?;
        let annotation = parse(&text)
            .with_context(|| format!("bad annotation in {}", annotation_path.display()))?;
        tracks.push(Track {
            name: name.to_string(),
            audio_path,
            annotation,
        });
    }
    tracks.sort_by(|first, second| first.name.cmp(&second.name));
    Ok(tracks)
}

/// Parses a key annotation such as "A minor" or "Eb major".
pub fn parse_key_annotation(text: &str) -> Result<Key> {
    Ok(text.trim().parse::<Key>()?)
}

/// Parses a tempo annotation. The first column is the tempo in beats per
/// minute; any further columns are ignored.
pub fn parse_tempo_annotation(text: &str) -> Result<f32> {
    let Some(first_column) = text.split_whitespace().next() else {
        bail!("tempo annotation is empty");
    };
    let tempo: f32 = first_column
        .parse()
        .with_context(|| format!("tempo {first_column:?} is not a number"))?;
    ensure!(
        tempo.is_finite() && tempo > 0.0,
        "tempo {first_column:?} is not a positive number"
    );
    Ok(tempo)
}

#[cfg(test)]
mod tests {
    use euphoria_core::Mode;

    use super::*;

    #[test]
    fn parses_key_text_with_surrounding_whitespace() {
        let key = parse_key_annotation("Eb minor\n").unwrap();
        assert_eq!(key, Key::new(3, Mode::Minor));
    }

    #[test]
    fn rejects_invalid_key_text_and_quotes_it() {
        let error = parse_key_annotation("H minor").unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("\"H minor\""), "{message}");
    }

    #[test]
    fn tempo_reads_the_first_column_and_ignores_the_rest() {
        assert_eq!(parse_tempo_annotation("128.5\t1.0\n").unwrap(), 128.5);
        assert_eq!(parse_tempo_annotation("  90 0.7 extra").unwrap(), 90.0);
    }

    #[test]
    fn rejects_bad_tempo_text() {
        for text in ["", "  \n", "fast 1.0", "0", "-120", "NaN"] {
            assert!(
                parse_tempo_annotation(text).is_err(),
                "{text:?} should not parse"
            );
        }
    }

    #[test]
    fn bad_annotation_error_names_the_file() {
        let dataset_directory = std::env::temp_dir().join(format!(
            "euphoria-bench-dataset-test-{}",
            std::process::id()
        ));
        let annotation_directory = dataset_directory.join("annotations/key");
        let audio_directory = dataset_directory.join("audio");
        fs::create_dir_all(&annotation_directory).unwrap();
        fs::create_dir_all(&audio_directory).unwrap();
        fs::write(annotation_directory.join("broken.key"), "H minor").unwrap();
        fs::write(audio_directory.join("broken.mp3"), b"").unwrap();

        let result = load_key_tracks(&dataset_directory);
        fs::remove_dir_all(&dataset_directory).unwrap();

        let message = format!("{:#}", result.unwrap_err());
        assert!(message.contains("broken.key"), "{message}");
        assert!(message.contains("\"H minor\""), "{message}");
    }
}
