// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Formats analysis results as a table, as JSON, and as a failure summary.

use std::io::{self, Write};

use euphoria_core::{EuphoriaError, FileResult, Key};
use serde::Serialize;

#[derive(Serialize)]
struct JsonTrack<'result> {
    path: &'result std::path::Path,
    duration_seconds: Option<f32>,
    key: Option<JsonKey>,
    bpm: Option<f32>,
    error: Option<String>,
}

#[derive(Serialize)]
struct JsonKey {
    camelot: String,
    name: String,
}

impl From<Key> for JsonKey {
    fn from(key: Key) -> Self {
        Self {
            camelot: key.camelot(),
            name: key.to_string(),
        }
    }
}

/// Writes one JSON array with an object per file, including failed files with
/// their error.
pub fn write_json(writer: &mut impl Write, results: &[FileResult]) -> io::Result<()> {
    let tracks: Vec<JsonTrack<'_>> = results
        .iter()
        .map(|result| match &result.outcome {
            Ok(analysis) => JsonTrack {
                path: &result.path,
                duration_seconds: Some(round_to_tenths(analysis.duration_seconds)),
                key: analysis.key.map(JsonKey::from),
                bpm: analysis.bpm.map(round_to_tenths),
                error: None,
            },
            Err(error) => JsonTrack {
                path: &result.path,
                duration_seconds: None,
                key: None,
                bpm: None,
                error: Some(error.chain()),
            },
        })
        .collect();
    serde_json::to_writer_pretty(&mut *writer, &tracks)?;
    writeln!(writer)
}

/// Writes an aligned table of the files that were analyzed.
pub fn write_table(writer: &mut impl Write, results: &[FileResult]) -> io::Result<()> {
    let header = ["Key", "", "BPM", "Time", "File"];
    let rows: Vec<[String; 5]> = results
        .iter()
        .filter_map(|result| {
            let analysis = result.outcome.as_ref().ok()?;
            Some([
                analysis
                    .key
                    .map(|key| key.camelot())
                    .unwrap_or_else(|| "-".to_string()),
                analysis.key.map(|key| key.to_string()).unwrap_or_default(),
                analysis
                    .bpm
                    .map(|bpm| format!("{bpm:.1}"))
                    .unwrap_or_else(|| "-".to_string()),
                format_duration(analysis.duration_seconds),
                result.path.display().to_string(),
            ])
        })
        .collect();

    let mut widths = header.map(str::len);
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }

    let header_cells = header.map(str::to_string);
    for row in std::iter::once(&header_cells).chain(&rows) {
        let [camelot, key_name, bpm, duration, file] = row;
        writeln!(
            writer,
            "{camelot:<camelot_width$}  {key_name:<key_name_width$}  {bpm:>bpm_width$}  {duration:>duration_width$}  {file}",
            camelot_width = widths[0],
            key_name_width = widths[1],
            bpm_width = widths[2],
            duration_width = widths[3],
        )?;
    }
    Ok(())
}

/// Writes the files that could not be analyzed and why.
pub fn write_failures(writer: &mut impl Write, results: &[FileResult]) -> io::Result<()> {
    let failures: Vec<_> = results
        .iter()
        .filter_map(|result| Some((&result.path, result.outcome.as_ref().err()?)))
        .collect();
    let noun = if failures.len() == 1 { "file" } else { "files" };
    writeln!(writer, "\nCould not analyze {} {noun}:", failures.len())?;
    for (path, error) in failures {
        writeln!(writer, "  {}: {}", path.display(), error.chain())?;
    }
    Ok(())
}

/// Writes how many of the tracks were served from the cache.
pub fn write_cache_summary(
    writer: &mut impl Write,
    cached_count: usize,
    total: usize,
) -> io::Result<()> {
    let noun = if total == 1 { "track" } else { "tracks" };
    writeln!(
        writer,
        "{cached_count} of {total} {noun} came from the cache."
    )
}

/// Writes that the cache could not be used and why, that the files are
/// analyzed regardless, and how to make the warning go away.
pub fn write_cache_warning(writer: &mut impl Write, error: &EuphoriaError) -> io::Result<()> {
    writeln!(
        writer,
        "warning: analyzing without the cache: {}",
        error.chain()
    )?;
    writeln!(
        writer,
        "  Run `euphoria cache clear` to reset the cache, or pass --no-cache to skip it."
    )
}

fn format_duration(seconds: f32) -> String {
    let whole_seconds = seconds.round() as u64;
    format!("{}:{:02}", whole_seconds / 60, whole_seconds % 60)
}

fn round_to_tenths(value: f32) -> f32 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use euphoria_core::{Mode, TrackAnalysis};

    use super::*;

    fn sample_results() -> Vec<FileResult> {
        vec![
            FileResult {
                path: PathBuf::from("music/low tide.flac"),
                outcome: Ok(TrackAnalysis {
                    duration_seconds: 408.4,
                    key: Some(Key::new(9, Mode::Minor)),
                    bpm: Some(124.04),
                }),
            },
            FileResult {
                path: PathBuf::from("music/intro.wav"),
                outcome: Ok(TrackAnalysis {
                    duration_seconds: 59.6,
                    key: None,
                    bpm: None,
                }),
            },
            FileResult {
                path: PathBuf::from("music/broken.mp3"),
                outcome: Err(EuphoriaError::UnknownKeyText("example".to_string())),
            },
        ]
    }

    fn render(write: impl Fn(&mut Vec<u8>, &[FileResult]) -> io::Result<()>) -> String {
        let mut buffer = Vec::new();
        write(&mut buffer, &sample_results()).unwrap();
        String::from_utf8(buffer).unwrap()
    }

    #[test]
    fn table_aligns_columns_and_leaves_failures_out() {
        let expected = "\
Key             BPM  Time  File
8A   A minor  124.0  6:48  music/low tide.flac
-                 -  1:00  music/intro.wav
";
        assert_eq!(render(write_table), expected);
    }

    #[test]
    fn json_includes_every_file_and_the_error_of_failed_ones() {
        let parsed: serde_json::Value = serde_json::from_str(&render(write_json)).unwrap();
        assert_eq!(parsed[0]["key"]["camelot"], "8A");
        assert_eq!(parsed[0]["key"]["name"], "A minor");
        assert_eq!(parsed[0]["bpm"], 124.0);
        assert_eq!(parsed[0]["duration_seconds"], 408.4);
        assert!(parsed[1]["key"].is_null());
        assert!(parsed[2]["error"].as_str().unwrap().contains("example"));
        assert_eq!(parsed.as_array().unwrap().len(), 3);
    }

    #[test]
    fn cache_summary_counts_cached_tracks() {
        let mut buffer = Vec::new();
        write_cache_summary(&mut buffer, 5, 7).unwrap();
        write_cache_summary(&mut buffer, 1, 1).unwrap();
        assert_eq!(
            String::from_utf8(buffer).unwrap(),
            "5 of 7 tracks came from the cache.\n1 of 1 track came from the cache.\n"
        );
    }

    #[test]
    fn cache_warning_gives_the_cause_and_the_two_ways_out() {
        let mut buffer = Vec::new();
        let error = EuphoriaError::UnknownKeyText("example".to_string());
        write_cache_warning(&mut buffer, &error).unwrap();
        let warning = String::from_utf8(buffer).unwrap();
        assert!(warning.starts_with("warning: analyzing without the cache: "));
        assert!(warning.contains("example"));
        assert!(warning.contains("euphoria cache clear"));
        assert!(warning.contains("--no-cache"));
    }

    #[test]
    fn failure_summary_names_each_failed_file() {
        let summary = render(write_failures);
        assert!(summary.contains("Could not analyze 1 file:"));
        assert!(summary.contains("music/broken.mp3"));
    }
}
