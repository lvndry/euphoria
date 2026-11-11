// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The keys and tempos the user corrected, as JSON Lines for training and
//! evaluating a key or tempo model: one object per correction, each naming
//! the content by its hash rather than by its file.

use std::path::Path;

use serde::Serialize;

use super::rekordbox::{ExistingFile, write_file};
use crate::cache::StoredCorrection;
use crate::error::EuphoriaError;
use crate::key::{Key, Mode};

/// The version of the shape of each line. It goes up when a field is
/// removed or changes meaning, and stays when a field is added.
pub const CORRECTIONS_SCHEMA_VERSION: u32 = 1;

/// Whether each line names the file the correction was made on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathInclusion {
    /// Only the content hash identifies the track, so the export can be
    /// kept or moved without saying where anyone's music is.
    Omit,
    /// Each line also holds the absolute path the file was last seen at,
    /// for matching on the same computer.
    Include,
}

#[derive(Serialize)]
struct CorrectionLine<'stored> {
    schema_version: u32,
    content_hash: String,
    corrected_key: Option<KeyFields>,
    corrected_bpm: Option<f32>,
    corrected_at: i64,
    detected_key: Option<KeyFields>,
    detected_bpm: Option<f32>,
    analyzer_version: Option<&'stored str>,
    analyzed_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<Option<&'stored str>>,
}

#[derive(Serialize)]
struct KeyFields {
    name: String,
    camelot: String,
    tonic: u8,
    mode: &'static str,
}

impl From<Key> for KeyFields {
    fn from(key: Key) -> Self {
        Self {
            name: key.to_string(),
            camelot: key.camelot(),
            tonic: key.tonic(),
            mode: match key.mode() {
                Mode::Major => "major",
                Mode::Minor => "minor",
            },
        }
    }
}

/// The corrections as JSON Lines: one object per line, in the order given,
/// each ending in a line break.
///
/// # Errors
///
/// The error of `serde_json` should a line fail to serialize.
pub fn render_corrections_jsonl(
    corrections: &[StoredCorrection],
    paths: PathInclusion,
) -> serde_json::Result<String> {
    let mut document = String::new();
    for stored in corrections {
        let detected = stored.detected.as_ref();
        let line = CorrectionLine {
            schema_version: CORRECTIONS_SCHEMA_VERSION,
            content_hash: stored.content_hash.to_hex(),
            corrected_key: stored.correction.key.map(KeyFields::from),
            corrected_bpm: stored.correction.bpm,
            corrected_at: stored.corrected_at_seconds,
            detected_key: detected.and_then(|values| values.key).map(KeyFields::from),
            detected_bpm: detected.and_then(|values| values.bpm),
            analyzer_version: detected.map(|values| values.analyzer_version.as_str()),
            analyzed_at: detected.map(|values| values.analyzed_at_seconds),
            path: (paths == PathInclusion::Include).then(|| {
                detected
                    .and_then(|values| values.last_known_path.as_deref())
                    .and_then(Path::to_str)
            }),
        };
        document.push_str(&serde_json::to_string(&line)?);
        document.push('\n');
    }
    Ok(document)
}

/// Writes the corrections to `destination` as JSON Lines, in one step as
/// [`write_xml_file`](super::rekordbox::write_xml_file) does.
///
/// # Errors
///
/// [`EuphoriaError::ExportDestinationExists`] when the file exists and
/// `existing` is [`ExistingFile::Keep`], and [`EuphoriaError::ExportFile`]
/// when it cannot be written.
pub fn write_corrections_file(
    destination: &Path,
    corrections: &[StoredCorrection],
    paths: PathInclusion,
    existing: ExistingFile,
) -> Result<(), EuphoriaError> {
    let document = render_corrections_jsonl(corrections, paths).map_err(|error| {
        EuphoriaError::ExportFile {
            path: destination.to_path_buf(),
            source: std::io::Error::other(error),
        }
    })?;
    write_file(destination, document.as_bytes(), existing)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::cache::{ContentHash, DetectedValues};
    use crate::correction::Correction;

    fn hash() -> ContentHash {
        ContentHash::from_bytes(*blake3::hash(b"audio").as_bytes())
    }

    fn key_and_tempo() -> StoredCorrection {
        StoredCorrection {
            content_hash: hash(),
            correction: Correction::of_key(Key::new(4, Mode::Minor))
                .with_bpm(Some(87.5))
                .unwrap(),
            corrected_at_seconds: 1_800_000_000,
            detected: Some(DetectedValues {
                key: Some(Key::new(9, Mode::Minor)),
                bpm: Some(175.0),
                analyzer_version: "12+libkeyfinder".to_string(),
                analyzed_at_seconds: 1_799_999_000,
                last_known_path: Some(PathBuf::from("/music/track.flac")),
            }),
        }
    }

    fn lines(document: &str) -> Vec<serde_json::Value> {
        document
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn each_correction_is_one_line_with_its_hash_its_values_and_what_was_detected() {
        let document = render_corrections_jsonl(&[key_and_tempo()], PathInclusion::Omit).unwrap();
        assert!(document.ends_with('\n'));
        let lines = lines(&document);
        assert_eq!(lines.len(), 1);
        assert_eq!(
            lines[0],
            serde_json::json!({
                "schema_version": 1,
                "content_hash": blake3::hash(b"audio").to_hex().to_string(),
                "corrected_key": {"name": "E minor", "camelot": "9A", "tonic": 4, "mode": "minor"},
                "corrected_bpm": 87.5,
                "corrected_at": 1_800_000_000,
                "detected_key": {"name": "A minor", "camelot": "8A", "tonic": 9, "mode": "minor"},
                "detected_bpm": 175.0,
                "analyzer_version": "12+libkeyfinder",
                "analyzed_at": 1_799_999_000,
            })
        );
    }

    #[test]
    fn a_path_is_written_only_when_asked_for() {
        let omitted = render_corrections_jsonl(&[key_and_tempo()], PathInclusion::Omit).unwrap();
        assert!(!omitted.contains("/music/track.flac"));
        let included =
            render_corrections_jsonl(&[key_and_tempo()], PathInclusion::Include).unwrap();
        assert_eq!(lines(&included)[0]["path"], "/music/track.flac");
    }

    #[test]
    fn a_correction_without_an_analysis_has_null_detected_values_and_path() {
        let stored = StoredCorrection {
            correction: Correction::of_bpm(124.0).unwrap(),
            detected: None,
            ..key_and_tempo()
        };
        let line = &lines(&render_corrections_jsonl(&[stored], PathInclusion::Include).unwrap())[0];
        assert_eq!(line["corrected_key"], serde_json::Value::Null);
        assert_eq!(line["corrected_bpm"], 124.0);
        for field in [
            "detected_key",
            "detected_bpm",
            "analyzer_version",
            "analyzed_at",
            "path",
        ] {
            assert_eq!(line[field], serde_json::Value::Null, "{field}");
        }
    }

    #[test]
    fn a_tempo_is_written_as_the_user_typed_it() {
        let stored = StoredCorrection {
            correction: Correction::of_bpm(124.3).unwrap(),
            ..key_and_tempo()
        };
        let document = render_corrections_jsonl(&[stored], PathInclusion::Omit).unwrap();
        assert!(document.contains("\"corrected_bpm\":124.3,"), "{document}");
    }

    #[test]
    fn no_corrections_is_an_empty_document() {
        assert_eq!(
            render_corrections_jsonl(&[], PathInclusion::Omit).unwrap(),
            ""
        );
    }
}
