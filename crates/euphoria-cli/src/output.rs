// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Formats analysis results as a table, as JSON, and as a failure summary,
//! and reports what an export wrote.

use std::io::{self, Write};
use std::path::Path;

use euphoria_core::{EuphoriaError, FileResult, Key, SkippedTrack, UnreadTags};
use serde::Serialize;

#[derive(Serialize)]
struct JsonTrack<'result> {
    path: &'result Path,
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

/// What an export wrote.
pub struct ExportSummary<'export> {
    /// Tracks in the exported file.
    pub track_count: usize,
    /// Playlists in the exported file, or `None` when playlists were not
    /// asked for.
    pub playlist_count: Option<usize>,
    /// How many of the tracks were served from the cache.
    pub cached_count: usize,
    /// The file that was written.
    pub output_file: &'export Path,
}

/// Writes one line saying how much was exported and where.
pub fn write_export_summary(
    writer: &mut impl Write,
    summary: &ExportSummary<'_>,
) -> io::Result<()> {
    write!(
        writer,
        "Exported {}",
        counted(summary.track_count, "track", "tracks")
    )?;
    if let Some(playlist_count) = summary.playlist_count {
        write!(
            writer,
            " in {}",
            counted(playlist_count, "playlist", "playlists")
        )?;
    }
    write!(writer, " to {}", summary.output_file.display())?;
    if summary.cached_count > 0 {
        write!(writer, " ({} from the cache)", summary.cached_count)?;
    }
    writeln!(writer)
}

/// Writes how to bring an exported file into rekordbox, and where the full
/// guide is.
pub fn write_rekordbox_import_steps(writer: &mut impl Write) -> io::Result<()> {
    writeln!(
        writer,
        "In rekordbox: 1. Preferences > View > Layout: tick rekordbox xml. 2. Preferences > Advanced > Database: choose this file. 3. Refresh rekordbox xml in the tree, then import its tracks or playlists."
    )?;
    writeln!(
        writer,
        "Full guide: https://github.com/lvndry/euphoria/blob/main/docs/guides/export-to-rekordbox.md"
    )
}

/// Writes the files an export left out and why.
pub fn write_skipped_tracks(writer: &mut impl Write, skipped: &[SkippedTrack]) -> io::Result<()> {
    writeln!(
        writer,
        "Left {} out of the export:",
        counted(skipped.len(), "file", "files")
    )?;
    for skipped_track in skipped {
        writeln!(
            writer,
            "  {}: {}",
            skipped_track.path.display(),
            skipped_track.reason.chain()
        )?;
    }
    Ok(())
}

/// Writes the exported tracks that are named after their file because their
/// tags could not be read, and why.
pub fn write_unread_tags(writer: &mut impl Write, unread_tags: &[UnreadTags]) -> io::Result<()> {
    if unread_tags.len() == 1 {
        writeln!(
            writer,
            "Could not read the title of 1 file, used its file name:"
        )?;
    } else {
        writeln!(
            writer,
            "Could not read the title of {} files, used their file names:",
            unread_tags.len()
        )?;
    }
    for track in unread_tags {
        writeln!(
            writer,
            "  {}: {}",
            track.path.display(),
            track.reason.chain()
        )?;
    }
    Ok(())
}

fn counted(count: usize, singular: &str, plural: &str) -> String {
    let noun = if count == 1 { singular } else { plural };
    format!("{count} {noun}")
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

    fn export_summary(summary: &ExportSummary<'_>) -> String {
        let mut buffer = Vec::new();
        write_export_summary(&mut buffer, summary).unwrap();
        String::from_utf8(buffer).unwrap()
    }

    #[test]
    fn export_summary_counts_tracks_and_playlists_on_one_line() {
        let output_file = PathBuf::from("/Users/dj/library.xml");
        let mut summary = ExportSummary {
            track_count: 340,
            playlist_count: Some(12),
            cached_count: 0,
            output_file: &output_file,
        };
        assert_eq!(
            export_summary(&summary),
            "Exported 340 tracks in 12 playlists to /Users/dj/library.xml\n"
        );

        summary.track_count = 1;
        summary.playlist_count = Some(1);
        summary.cached_count = 1;
        assert_eq!(
            export_summary(&summary),
            "Exported 1 track in 1 playlist to /Users/dj/library.xml (1 from the cache)\n"
        );

        summary.playlist_count = None;
        summary.cached_count = 0;
        assert_eq!(
            export_summary(&summary),
            "Exported 1 track to /Users/dj/library.xml\n"
        );
    }

    #[test]
    fn import_steps_are_two_lines_ending_with_the_guide() {
        let mut buffer = Vec::new();
        write_rekordbox_import_steps(&mut buffer).unwrap();
        let steps = String::from_utf8(buffer).unwrap();
        let lines: Vec<&str> = steps.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("In rekordbox: 1. "), "{steps}");
        assert!(
            lines[1].ends_with("docs/guides/export-to-rekordbox.md"),
            "{steps}"
        );
    }

    #[test]
    fn skipped_tracks_are_listed_with_the_reason() {
        let skipped = [SkippedTrack {
            path: PathBuf::from("music/broken.mp3"),
            reason: EuphoriaError::UnknownKeyText("example".to_string()),
        }];
        let mut buffer = Vec::new();
        write_skipped_tracks(&mut buffer, &skipped).unwrap();
        assert_eq!(
            String::from_utf8(buffer).unwrap(),
            "Left 1 file out of the export:\n  music/broken.mp3: unrecognised key text: \"example\"\n"
        );
    }

    #[test]
    fn unread_tags_are_a_note_with_the_reason() {
        let unread = |name: &str| UnreadTags {
            path: PathBuf::from(name),
            reason: EuphoriaError::UnknownKeyText("example".to_string()),
        };
        let mut buffer = Vec::new();
        write_unread_tags(&mut buffer, &[unread("music/a.mp3")]).unwrap();
        write_unread_tags(&mut buffer, &[unread("music/a.mp3"), unread("music/b.mp3")]).unwrap();
        assert_eq!(
            String::from_utf8(buffer).unwrap(),
            "Could not read the title of 1 file, used its file name:\n  music/a.mp3: unrecognised key text: \"example\"\nCould not read the title of 2 files, used their file names:\n  music/a.mp3: unrecognised key text: \"example\"\n  music/b.mp3: unrecognised key text: \"example\"\n"
        );
    }

    #[test]
    fn failure_summary_names_each_failed_file() {
        let summary = render(write_failures);
        assert!(summary.contains("Could not analyze 1 file:"));
        assert!(summary.contains("music/broken.mp3"));
    }
}
