// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `quality` command: the files whose audio is too poor for a club
//! system, worst first, each with why, so they can be bought or downloaded
//! again. The ranking and the wording are the core's, the same the desktop
//! app shows.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use euphoria_core::export::rekordbox::ExistingFile;
use euphoria_core::quality::{
    QualityReport, QualityVerdict, ReplaceProblem, ReplaceSummary, compare_for_replacement,
};
use euphoria_core::{CachedBatch, M3uTrack, read_track_name, write_m3u8_file};
use serde::Serialize;

use crate::batch::{self, AnalysisOptions};
use crate::export::{check_output_file, check_output_is_not_an_input};
use crate::style::Theme;
use crate::terminal::Printable;
use crate::{output, scan, view, with_causes};

#[derive(Debug, Args)]
pub struct QualityArguments {
    /// Audio files, or folders to search recursively.
    #[arg(required = true)]
    paths: Vec<PathBuf>,
    /// List every track with its verdict, not only the files to replace.
    #[arg(long)]
    all: bool,
    /// The verdict from which a file is worth replacing: acceptable, weak
    /// or reencoded. Damaged files are listed whatever it is.
    #[arg(long, value_enum, default_value_t = ThresholdChoice::Weak, value_name = "VERDICT")]
    min: ThresholdChoice,
    /// Print JSON instead of the list.
    #[arg(long)]
    json: bool,
    /// Also write the list as a CSV file, for a spreadsheet.
    #[arg(long, value_name = "FILE")]
    csv: Option<PathBuf>,
    /// Also write the files to replace as an M3U8 playlist, to find them in
    /// a DJ program.
    #[arg(long, value_name = "FILE")]
    m3u: Option<PathBuf>,
    /// Replace the CSV or M3U8 file if it exists.
    #[arg(long)]
    force: bool,
    #[command(flatten)]
    analysis: AnalysisOptions,
}

/// A verdict from which a file is worth replacing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ThresholdChoice {
    /// Medium bit rate files and everything worse.
    Acceptable,
    /// Low bit rate files and the re-encoded ones.
    #[value(alias = "weak-on-a-big-system")]
    Weak,
    /// Only the files whose sound stops where a lower quality file's would.
    #[value(alias = "likely-reencoded", alias = "re-encoded")]
    Reencoded,
}

impl ThresholdChoice {
    fn verdict(self) -> QualityVerdict {
        match self {
            ThresholdChoice::Acceptable => QualityVerdict::Acceptable,
            ThresholdChoice::Weak => QualityVerdict::WeakOnABigSystem,
            ThresholdChoice::Reencoded => QualityVerdict::LikelyReencoded,
        }
    }
}

/// An analyzed track with its quality report, as the list shows it.
struct Row {
    path: PathBuf,
    shown_path: PathBuf,
    duration_seconds: f32,
    report: QualityReport,
    problems: Vec<ReplaceProblem>,
    artist: Option<String>,
    title: Option<String>,
}

impl Row {
    fn to_replace(&self) -> bool {
        !self.problems.is_empty()
    }

    /// "Artist - Title" from the tags, the title alone without an artist,
    /// and the file name when the tags say nothing.
    fn display_name(&self) -> String {
        match (&self.artist, &self.title) {
            (Some(artist), Some(title)) => format!("{artist} - {title}"),
            (None, Some(title)) => title.clone(),
            _ => self
                .path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }

    /// The concrete reasons, worst first, or the format of a file that is
    /// fine, with its clipping.
    fn reasons(&self) -> Vec<String> {
        self.report.row_reasons(&self.problems, &self.path)
    }

    /// The worst problem as a heading, or "Keep" for a file that is fine.
    fn heading(&self) -> String {
        self.problems.first().map_or_else(
            || "Keep".to_string(),
            |problem| capitalized(problem.label()),
        )
    }
}

fn capitalized(text: &str) -> String {
    let mut characters = text.chars();
    characters.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(characters).collect()
    })
}

/// `file` relative to the folder argument that holds it, or its name when
/// it was given on its own.
fn shown_path(file: &Path, arguments: &[PathBuf]) -> PathBuf {
    arguments
        .iter()
        .filter(|argument| argument.is_dir())
        .find_map(|folder| file.strip_prefix(folder).ok())
        .filter(|relative| !relative.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .or_else(|| file.file_name().map(PathBuf::from))
        .unwrap_or_else(|| file.to_path_buf())
}

fn existing_file(force: bool) -> ExistingFile {
    if force {
        ExistingFile::Replace
    } else {
        ExistingFile::Keep
    }
}

fn output_file(path: Option<&Path>, force: bool) -> Result<Option<PathBuf>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let absolute = std::path::absolute(path)
        .with_context(|| format!("cannot use {} as the output", path.display()))?;
    check_output_file(&absolute, existing_file(force))?;
    Ok(Some(absolute))
}

pub fn run(arguments: &QualityArguments) -> Result<ExitCode> {
    for path in &arguments.paths {
        if matches!(path.try_exists(), Ok(false)) {
            bail!("{} does not exist", path.display());
        }
    }
    let csv_file = output_file(arguments.csv.as_deref(), arguments.force)?;
    let m3u_file = output_file(arguments.m3u.as_deref(), arguments.force)?;
    if csv_file.is_some() && csv_file == m3u_file {
        bail!("--csv and --m3u name the same file");
    }
    let scan = scan::scan_quietly(&arguments.paths)?;
    let standard_error_theme = Theme::stderr();
    if scan.files.is_empty() {
        let mut standard_error = io::stderr().lock();
        scan::write_notices(&mut standard_error, &standard_error_theme, &scan)?;
        output::write_no_audio_files(&mut standard_error, &standard_error_theme)?;
        return Ok(ExitCode::FAILURE);
    }
    for file in csv_file.iter().chain(&m3u_file) {
        check_output_is_not_an_input(file, &scan.files, &arguments.paths)?;
    }

    let CachedBatch {
        results,
        cached_count,
        store_error,
    } = batch::analyze(
        &scan.files,
        &arguments.analysis,
        if arguments.json {
            batch::Display::CountOnly
        } else {
            batch::Display::Stages
        },
    )?;
    let threshold = arguments.min.verdict();
    let analyzed_count = results.len();
    let (analyzed, failures): (Vec<_>, Vec<_>) = results
        .into_iter()
        .partition(|result| result.outcome.is_ok());
    let mut unmeasured_count = 0;
    let mut rows: Vec<Row> = analyzed
        .into_iter()
        .filter_map(|result| {
            let analysis = result.outcome.ok()?;
            let Some(report) = analysis.quality else {
                unmeasured_count += 1;
                return None;
            };
            Some(Row {
                shown_path: shown_path(&result.path, &arguments.paths),
                problems: report.replace_problems(threshold),
                duration_seconds: analysis.duration_seconds,
                report,
                path: result.path,
                artist: None,
                title: None,
            })
        })
        .collect();
    rows.sort_by(|first, second| first.shown_path.cmp(&second.shown_path));
    rows.sort_by(|first, second| compare_for_replacement(&first.report, &second.report, threshold));
    let summary = ReplaceSummary::of(rows.iter().map(|row| &row.report), threshold);
    if !arguments.all {
        rows.retain(Row::to_replace);
    }
    for row in &mut rows {
        if let Ok(name) = read_track_name(&row.path) {
            row.artist = name.artist;
            row.title = name.title;
        }
    }

    let mut standard_output = io::stdout().lock();
    if arguments.json {
        write_json(&mut standard_output, &rows, &summary, threshold)?;
    } else {
        write_list(&mut standard_output, &Theme::stdout(), &rows, &summary)?;
    }
    standard_output.flush()?;
    drop(standard_output);

    let mut report = Vec::new();
    if let Some(csv_file) = &csv_file {
        std::fs::write(csv_file, render_csv(&rows))
            .with_context(|| format!("cannot write {}", csv_file.display()))?;
        writeln!(
            report,
            "Wrote {} to {}",
            counted_tracks(rows.len()),
            Printable(csv_file.display())
        )?;
    }
    if let Some(m3u_file) = &m3u_file {
        let tracks: Vec<M3uTrack> = rows
            .iter()
            .filter(|row| row.to_replace())
            .map(|row| M3uTrack {
                path: std::path::absolute(&row.path).unwrap_or_else(|_| row.path.clone()),
                duration_seconds: Some(row.duration_seconds),
                display_name: row.display_name(),
            })
            .collect();
        write_m3u8_file(
            m3u_file,
            "Files to replace",
            &tracks,
            existing_file(arguments.force),
        )
        .map_err(with_causes)?;
        writeln!(
            report,
            "Wrote {} to replace to {}",
            counted_tracks(tracks.len()),
            Printable(m3u_file.display())
        )?;
    }
    if !arguments.json {
        output::write_failures(&mut report, &standard_error_theme, &failures)?;
        if !failures.is_empty() {
            writeln!(
                report,
                "{} could not be read at all, and are worth replacing too; the errors above say why.",
                output::counted_files(failures.len())
            )?;
        }
        if unmeasured_count > 0 {
            writeln!(
                report,
                "{} have no quality report; --reanalyze measures them.",
                output::counted_files(unmeasured_count)
            )?;
        }
    }
    scan::write_notices(&mut report, &standard_error_theme, &scan)?;
    if cached_count > 0 && !arguments.json {
        output::write_cache_summary(&mut report, cached_count, analyzed_count)?;
    }
    let report = String::from_utf8_lossy(&report);
    let report = match view::standard_error_width() {
        Some(width) if io::stderr().is_terminal() => view::wrap_lines(&report, width),
        _ => report.into_owned(),
    };
    io::stderr().lock().write_all(report.as_bytes())?;

    if let Some(store_error) = store_error {
        return Err(with_causes(store_error).context("some results were not saved to the cache"));
    }
    let missing_count = failures
        .iter()
        .filter(|result| output::is_missing(result))
        .count();
    Ok(output::exit_status(
        failures.len() > missing_count,
        missing_count > 0,
    ))
}

fn counted_tracks(count: usize) -> String {
    format!("{count} {}", if count == 1 { "track" } else { "tracks" })
}

/// The summary line, then one block per track: the worst problem and the
/// name, then the verdict with the reasons, then the path.
fn write_list(
    writer: &mut impl Write,
    theme: &Theme,
    rows: &[Row],
    summary: &ReplaceSummary,
) -> io::Result<()> {
    writeln!(writer, "{}", theme.bold(summary.line()))?;
    let heading_width = rows
        .iter()
        .map(|row| view::width_of(&row.heading()))
        .max()
        .unwrap_or(0);
    for row in rows {
        writeln!(writer)?;
        let heading = row.heading();
        let padding = " ".repeat(heading_width - view::width_of(&heading));
        let styled_heading = if row.to_replace() {
            theme.caution(&heading)
        } else {
            theme.good(&heading)
        };
        writeln!(
            writer,
            "{styled_heading}{padding}  {}",
            Printable(row.display_name())
        )?;
        writeln!(
            writer,
            "  {}: {}",
            row.report.verdict.label(),
            Printable(row.reasons().join("; "))
        )?;
        writeln!(
            writer,
            "  {}",
            theme.dim(Printable(row.shown_path.display()))
        )?;
    }
    Ok(())
}

#[derive(Serialize)]
struct JsonReport<'rows> {
    schema_version: u32,
    threshold: QualityVerdict,
    summary: JsonSummary,
    tracks: Vec<JsonTrack<'rows>>,
}

#[derive(Serialize)]
struct JsonSummary {
    tracks: usize,
    to_replace: usize,
    by_problem: Vec<JsonProblemCount>,
    line: String,
}

#[derive(Serialize)]
struct JsonProblemCount {
    problem: ReplaceProblem,
    label: &'static str,
    count: usize,
}

#[derive(Serialize)]
struct JsonTrack<'rows> {
    path: &'rows Path,
    relative_path: &'rows Path,
    artist: Option<&'rows str>,
    title: Option<&'rows str>,
    verdict: QualityVerdict,
    verdict_label: &'static str,
    to_replace: bool,
    problems: &'rows [ReplaceProblem],
    reasons: Vec<String>,
    explanation: String,
    notes: Vec<String>,
    quality: &'rows QualityReport,
}

fn write_json(
    writer: &mut impl Write,
    rows: &[Row],
    summary: &ReplaceSummary,
    threshold: QualityVerdict,
) -> io::Result<()> {
    let document = JsonReport {
        schema_version: output::JSON_SCHEMA_VERSION,
        threshold,
        summary: JsonSummary {
            tracks: summary.track_count,
            to_replace: summary.to_replace_count,
            by_problem: summary
                .by_problem
                .iter()
                .map(|(problem, count)| JsonProblemCount {
                    problem: *problem,
                    label: problem.label(),
                    count: *count,
                })
                .collect(),
            line: summary.line(),
        },
        tracks: rows
            .iter()
            .map(|row| JsonTrack {
                path: &row.path,
                relative_path: &row.shown_path,
                artist: row.artist.as_deref(),
                title: row.title.as_deref(),
                verdict: row.report.verdict,
                verdict_label: row.report.verdict.label(),
                to_replace: row.to_replace(),
                problems: &row.problems,
                reasons: row.reasons(),
                explanation: row.report.reason(),
                notes: row.report.notes(),
                quality: &row.report,
            })
            .collect(),
    };
    serde_json::to_writer_pretty(&mut *writer, &document)?;
    writeln!(writer)
}

const CSV_HEADINGS: [&str; 8] = [
    "Path",
    "Artist",
    "Title",
    "Verdict",
    "To replace",
    "Problems",
    "Reason",
    "Full path",
];

/// The rows as CSV, one line per track, with fields quoted as RFC 4180
/// asks when they hold a comma, a quote or a line break.
fn render_csv(rows: &[Row]) -> String {
    let field = |text: &str| {
        if text.contains([',', '"', '\n', '\r']) {
            format!("\"{}\"", text.replace('"', "\"\""))
        } else {
            text.to_string()
        }
    };
    let mut document = CSV_HEADINGS.join(",");
    document.push_str("\r\n");
    for row in rows {
        let problems: Vec<&str> = row.problems.iter().map(|problem| problem.label()).collect();
        let cells = [
            row.shown_path.to_string_lossy().into_owned(),
            row.artist.clone().unwrap_or_default(),
            row.title.clone().unwrap_or_default(),
            row.report.verdict.label().to_string(),
            if row.to_replace() { "yes" } else { "no" }.to_string(),
            problems.join("; "),
            row.reasons().join("; "),
            row.path.to_string_lossy().into_owned(),
        ];
        let line: Vec<String> = cells.iter().map(|cell| field(cell)).collect();
        document.push_str(&line.join(","));
        document.push_str("\r\n");
    }
    document
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_shown_relative_to_the_folder_given() {
        let folder = std::env::temp_dir();
        let file = folder.join("Crate").join("track.mp3");
        assert_eq!(
            shown_path(&file, std::slice::from_ref(&folder)),
            Path::new("Crate/track.mp3")
        );
        assert_eq!(
            shown_path(&file, std::slice::from_ref(&file)),
            Path::new("track.mp3")
        );
    }

    #[test]
    fn csv_fields_with_commas_or_quotes_are_quoted() {
        let row = Row {
            path: PathBuf::from("/Music/a.mp3"),
            shown_path: PathBuf::from("a.mp3"),
            duration_seconds: 300.0,
            report: QualityReport {
                codec: "mp3".to_string(),
                lossless: false,
                bitrate_kbps: Some(128),
                sample_rate: 44_100,
                bit_depth: None,
                channels: Some(2),
                cutoff_hz: Some(16_000.0),
                clipped_share: 0.0,
                integrated_loudness_lufs: None,
                true_peak_dbtp: None,
                verdict: QualityVerdict::WeakOnABigSystem,
                damage: Default::default(),
                cancelled_share: 0.0,
            },
            problems: vec![ReplaceProblem::LowBitrate],
            artist: Some("Smith, Jones".to_string()),
            title: Some("The \"Dub\"".to_string()),
        };
        assert_eq!(
            render_csv(&[row]),
            "Path,Artist,Title,Verdict,To replace,Problems,Reason,Full path\r\n\
             a.mp3,\"Smith, Jones\",\"The \"\"Dub\"\"\",Weak on a big system,yes,low bit rate,\"MP3, 128 kbit/s\",/Music/a.mp3\r\n"
        );
    }
}
