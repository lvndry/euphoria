// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `euphoria` command line tool.

mod batch;
mod corrections;
mod export;
mod help;
mod output;
mod playlists;
mod rewrites;
mod set;
mod set_view;
mod shift;
mod stages;
#[cfg(feature = "stems")]
mod stems;
mod style;
mod tag_edit;
mod tags;
mod terminal;
mod view;

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use euphoria_core::quality::QualityVerdict;
use euphoria_core::{
    CachedBatch, EuphoriaError, FileResult, TrackAnalysis, clear_cache, default_cache_path,
    find_audio_files,
};

use crate::batch::AnalysisOptions;
use crate::corrections::CorrectionsCommand;
use crate::export::ExportTarget;
use crate::output::Narrowing;
use crate::style::{ColorChoice, Theme};
use crate::tags::TagsCommand;
use crate::view::{KeyNotationChoice, ViewOptions};

#[derive(Debug, Parser)]
#[command(
    name = "euphoria",
    version,
    about = "Find the key, tempo, energy and sound quality of your music, write them into \
             your tags, and export them to rekordbox.",
    after_help = help::main_examples(),
)]
struct Cli {
    /// When to use colour: auto colours a terminal and respects NO_COLOR
    /// and CLICOLOR_FORCE.
    #[arg(
        long,
        global = true,
        value_enum,
        default_value_t,
        value_name = "WHEN",
        display_order = 1000
    )]
    color: ColorChoice,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show the key, tempo, energy and sound quality of tracks.
    #[command(
        long_about = "Show the key, tempo, energy and sound quality of tracks.\n\n\
                      On a terminal, a folder is shown as a table that fits the window and \
                      one file is shown in detail. Into a pipe or a file, the output is a \
                      plain table with fixed columns, or JSON with --json. Results are \
                      saved, so a second run over the same files is instant.",
        after_help = help::analyze_examples(),
    )]
    Analyze(AnalyzeArguments),
    /// List the playlists built in the desktop app.
    Playlists,
    /// Write the analysis into the tags of your files, or undo the last write.
    #[command(after_help = help::tags_examples())]
    Tags {
        #[command(subcommand)]
        command: TagsCommand,
    },
    /// Write the analysis for other DJ software, such as a rekordbox XML file.
    #[command(after_help = help::export_examples())]
    Export {
        #[command(subcommand)]
        target: ExportTarget,
    },
    /// Export the keys and tempos you corrected, to train a key model.
    #[command(after_help = help::corrections_examples())]
    Corrections {
        #[command(subcommand)]
        command: CorrectionsCommand,
    },

    /// Write a copy of a track in another key, with the same tempo.
    #[command(
        long_about = "Write a copy of a track in another key, with the same tempo.

                      The pitch moves by up to 6 semitones either way while every beat                       stays where it was. The copy is a 24 bit WAV or FLAC file tagged                       with the new key; the original is never changed.",
        after_help = help::shift_examples(),
    )]
    Shift(shift::ShiftArguments),
    /// Separate a track into vocals, drums, bass and the rest.
    #[cfg(feature = "stems")]
    #[command(
        long_about = "Separate a track into vocals, drums, bass and the rest, with the Demucs \
                      htdemucs model, and write one WAV file per stem.\n\n\
                      The model, 84 MB, is downloaded the first time. Separation runs on this \
                      computer, about 20 to 30 seconds per minute of audio on a recent Mac, \
                      and needs about 2.5 GB of free memory. The stems are saved, so separating \
                      the same track again is instant.",
        after_help = help::stems_examples(),
    )]
    Stems(stems::StemsArguments),
    /// Order tracks into a set that follows an energy curve, every mix in key.
    #[command(after_help = help::set_examples())]
    Set {
        #[command(subcommand)]
        command: set::SetCommand,
    },
    /// Show where the saved analyses are, or forget them.
    #[command(after_help = help::cache_examples())]
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
}

#[derive(Debug, Args)]
struct AnalyzeArguments {
    /// Audio files, or folders to search recursively.
    #[arg(required = true)]
    paths: Vec<PathBuf>,
    /// Print JSON instead of a table.
    #[arg(long, conflicts_with = "detail")]
    json: bool,
    /// Show every track in detail: the runner up key, the half or double
    /// tempo, the reason for the sound quality and more. One file on a
    /// terminal is always shown this way.
    #[arg(long)]
    detail: bool,
    /// List only the tracks whose key or tempo is rated low confidence,
    /// and the files that could not be analyzed.
    #[arg(long)]
    needs_review: bool,
    /// List only the tracks with one of these sound qualities, separated by
    /// commas, such as weak,reencoded to find the files to replace.
    #[arg(long, value_enum, value_delimiter = ',', value_name = "QUALITY")]
    quality: Vec<QualityChoice>,
    /// How keys are shown in the table and the detailed view.
    #[arg(long, value_enum, default_value_t, value_name = "NOTATION")]
    key_notation: KeyNotationChoice,
    #[command(flatten)]
    analysis: AnalysisOptions,
}

/// A sound quality to list with `--quality`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum QualityChoice {
    /// Lossless files with a full spectrum.
    Lossless,
    /// High bit rate files with the spectrum that bit rate keeps.
    Good,
    /// Medium bit rate files, fine in most rooms.
    Acceptable,
    /// Low bit rate files, whose top end suffers on a big system.
    #[value(alias = "weak-on-a-big-system")]
    Weak,
    /// Files whose sound stops where a lower quality file's would.
    #[value(alias = "likely-reencoded", alias = "re-encoded")]
    Reencoded,
}

impl QualityChoice {
    fn verdict(self) -> QualityVerdict {
        match self {
            QualityChoice::Lossless => QualityVerdict::Lossless,
            QualityChoice::Good => QualityVerdict::Good,
            QualityChoice::Acceptable => QualityVerdict::Acceptable,
            QualityChoice::Weak => QualityVerdict::WeakOnABigSystem,
            QualityChoice::Reencoded => QualityVerdict::LikelyReencoded,
        }
    }
}

#[derive(Debug, Subcommand)]
enum CacheCommand {
    /// Print where the cache database is stored.
    Path,
    /// Forget every saved analysis, so every track is analyzed again.
    Clear,
}

fn main() -> ExitCode {
    let arguments: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let choice = style::choice_in_arguments(&arguments);
    style::init(choice);
    let parser_colour = match choice {
        ColorChoice::Auto => clap::ColorChoice::Auto,
        ColorChoice::Always => clap::ColorChoice::Always,
        ColorChoice::Never => clap::ColorChoice::Never,
    };
    let mut command = Cli::command().color(parser_colour);
    let cli = command
        .try_get_matches_from_mut(arguments)
        .and_then(|mut matches| Cli::from_arg_matches_mut(&mut matches))
        .unwrap_or_else(|error| error.format(&mut command).exit());

    match run(cli) {
        Ok(exit_code) => exit_code,
        Err(error) if is_broken_pipe(&error) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = output::write_error(
                &mut std::io::stderr().lock(),
                &Theme::stderr(),
                terminal::Printable(format!("{error:#}")),
            );
            ExitCode::FAILURE
        }
    }
}

/// True when output failed because the reader went away, as when the command
/// is piped into `head`. The reader has what it wanted, so this is not an
/// error worth reporting.
fn is_broken_pipe(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io_error| io_error.kind() == std::io::ErrorKind::BrokenPipe)
    })
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Analyze(arguments) => analyze(&arguments),
        Command::Export { target } => export::run(target),
        Command::Tags { command } => tags::run(command),
        Command::Playlists => playlists::list(),
        Command::Corrections { command } => corrections::run(command),
        Command::Shift(arguments) => shift::run(&arguments),
        Command::Set { command } => set::run(command),
        Command::Cache { command } => manage_cache(command),
        #[cfg(feature = "stems")]
        Command::Stems(arguments) => stems::run(&arguments),
    }
}

fn manage_cache(command: CacheCommand) -> Result<ExitCode> {
    let cache_path = default_cache_path().map_err(with_causes)?;
    let mut standard_output = std::io::stdout().lock();
    match command {
        CacheCommand::Path => writeln!(standard_output, "{}", cache_path.display())?,
        CacheCommand::Clear => {
            if clear_cache(&cache_path).map_err(with_causes)? {
                writeln!(
                    standard_output,
                    "Cleared the cache at {}",
                    cache_path.display()
                )?;
            } else {
                writeln!(
                    standard_output,
                    "There is no cache at {}",
                    cache_path.display()
                )?;
            }
        }
    }
    standard_output.flush()?;
    Ok(ExitCode::SUCCESS)
}

/// How the results of `analyze` are shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    Json,
    /// The table with fixed columns, for pipes and files.
    PlainTable,
    /// The table that fits the terminal.
    Table,
    /// A detailed view per track.
    Detail,
}

fn analyze(arguments: &AnalyzeArguments) -> Result<ExitCode> {
    let analysis = &arguments.analysis;
    let audio_files = find_audio_files(&arguments.paths).map_err(with_causes)?;
    let standard_error_theme = Theme::stderr();
    if audio_files.is_empty() {
        output::write_no_audio_files(&mut std::io::stderr().lock(), &standard_error_theme)?;
        return Ok(ExitCode::SUCCESS);
    }
    let CachedBatch {
        results,
        cached_count,
        store_error,
    } = batch::analyze(
        &audio_files,
        analysis,
        if arguments.json {
            batch::Display::CountOnly
        } else {
            batch::Display::Stages
        },
    )?;
    let user_edits = batch::user_edits(&results, analysis, batch::CorrectionCheck::Quick)?;
    let results = batch::corrected(results, &user_edits);
    let corrections = batch::corrections(&results, analysis, batch::CorrectionCheck::Quick)?;
    let analyzed_count = results.len();

    let verdicts: Vec<QualityVerdict> = arguments
        .quality
        .iter()
        .map(|choice| choice.verdict())
        .collect();
    let is_listed = |analysis: &TrackAnalysis| {
        (!arguments.needs_review || analysis.needs_review())
            && (verdicts.is_empty()
                || analysis
                    .quality
                    .as_ref()
                    .is_some_and(|quality| verdicts.contains(&quality.verdict)))
    };
    let narrowing = Narrowing {
        needs_review: arguments.needs_review,
        verdicts: verdicts.clone(),
        shown_count: results
            .iter()
            .filter(|result| result.outcome.as_ref().is_ok_and(is_listed))
            .count(),
    };
    let failure_count = results
        .iter()
        .filter(|result| result.outcome.is_err())
        .count();

    let stdout_is_terminal = std::io::stdout().is_terminal();
    let layout = if arguments.json {
        Layout::Json
    } else if arguments.detail || (stdout_is_terminal && audio_files.len() == 1) {
        Layout::Detail
    } else if stdout_is_terminal {
        Layout::Table
    } else {
        Layout::PlainTable
    };
    let view_options = ViewOptions {
        notation: arguments.key_notation,
        tempo_range: analysis.tempo_range(),
        width: if stdout_is_terminal {
            view::terminal_width()
        } else {
            view::DEFAULT_WIDTH
        },
    };

    // Written before the results are narrowed, as it speaks of all of them,
    // and shown after them.
    let mut report = Vec::new();
    // On a terminal, blocks are set apart by a blank line, once something
    // is above them.
    let mut needs_space = layout != Layout::PlainTable && narrowing.shown_count > 0;
    if layout != Layout::Json {
        if failure_count > 0 {
            if needs_space {
                writeln!(report)?;
            }
            output::write_failures(&mut report, &standard_error_theme, &results)?;
            needs_space = layout != Layout::PlainTable;
        }
        let one_track_in_detail = layout == Layout::Detail
            && results.len() == 1
            && !arguments.needs_review
            && verdicts.is_empty();
        if needs_space && (!one_track_in_detail || cached_count > 0) {
            writeln!(report)?;
        }
        if !one_track_in_detail {
            output::write_analysis_summary(
                &mut report,
                &standard_error_theme,
                &results,
                &narrowing,
            )?;
        }
    }
    if cached_count > 0 {
        output::write_cache_summary(&mut report, cached_count, analyzed_count)?;
    }

    let listed_results: Vec<FileResult> = results
        .into_iter()
        .filter(|result| result.outcome.as_ref().map_or(true, is_listed))
        .collect();
    let standard_output_theme = Theme::stdout();
    let mut standard_output = std::io::stdout().lock();
    match layout {
        Layout::Json => output::write_json(
            &mut standard_output,
            &listed_results,
            analysis.tempo_range(),
            &user_edits,
        )?,
        Layout::PlainTable => {
            output::write_plain_table(&mut standard_output, &listed_results, &user_edits)?
        }
        Layout::Table => view::write_table(
            &mut standard_output,
            &standard_output_theme,
            &listed_results,
            &view_options,
            &corrections,
        )?,
        Layout::Detail => {
            let mut first = true;
            for result in &listed_results {
                if let Ok(track) = &result.outcome {
                    if !first {
                        writeln!(standard_output)?;
                    }
                    first = false;
                    view::write_detail(
                        &mut standard_output,
                        &standard_output_theme,
                        &result.path,
                        track,
                        &view_options,
                        corrections
                            .get(&result.path)
                            .and_then(|looked_up| looked_up.as_ref().ok()),
                    )?;
                }
            }
        }
    }
    standard_output.flush()?;
    drop(standard_output);

    let report = String::from_utf8_lossy(&report);
    let report = match view::standard_error_width() {
        Some(width) if std::io::stderr().is_terminal() => view::wrap_lines(&report, width),
        _ => report.into_owned(),
    };
    std::io::stderr().lock().write_all(report.as_bytes())?;

    if let Some(store_error) = store_error {
        return Err(with_causes(store_error).context("some results were not saved to the cache"));
    }
    Ok(if failure_count == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

pub(crate) fn with_causes(error: EuphoriaError) -> anyhow::Error {
    anyhow::anyhow!(error.chain())
}

#[cfg(test)]
mod tests {
    use anyhow::Context;

    use super::*;

    #[test]
    fn recognises_a_broken_pipe_under_added_context() {
        let broken_pipe = std::io::Error::from(std::io::ErrorKind::BrokenPipe);
        let error = Err::<(), _>(broken_pipe)
            .context("writing the table")
            .unwrap_err();
        assert!(is_broken_pipe(&error));
    }

    #[test]
    fn other_errors_are_still_reported() {
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert!(!is_broken_pipe(&anyhow::Error::from(denied)));
        assert!(!is_broken_pipe(&anyhow::anyhow!("no audio files")));
    }

    #[test]
    fn the_command_line_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_quality_choice_names_its_own_verdict() {
        let verdicts: Vec<QualityVerdict> = QualityChoice::value_variants()
            .iter()
            .map(|choice| choice.verdict())
            .collect();
        for verdict in [
            QualityVerdict::Lossless,
            QualityVerdict::Good,
            QualityVerdict::Acceptable,
            QualityVerdict::WeakOnABigSystem,
            QualityVerdict::LikelyReencoded,
        ] {
            assert!(verdicts.contains(&verdict), "{verdict:?}");
        }
        assert_eq!(
            QualityChoice::from_str("weak-on-a-big-system", false).unwrap(),
            QualityChoice::Weak
        );
    }
}
