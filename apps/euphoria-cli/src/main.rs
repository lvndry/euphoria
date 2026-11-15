// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `euphoria` command line tool.

mod batch;
mod corrections;
mod export;
mod filters;
mod grid_import;
mod help;
mod live;
mod next;
mod output;
mod plan;
mod playlists;
mod prepare;
mod rewrites;
mod scan;
mod set;
mod set_view;
mod shift;
mod stages;
#[cfg(feature = "stems")]
mod stems;
mod style;
mod tag_edit;
mod tag_key;
mod tags;
mod terminal;
mod view;

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::Result;
use clap::{Args, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use euphoria_core::{
    CachedBatch, EuphoriaError, FileResult, TrackAnalysis, clear_cache, default_cache_path,
};

use crate::batch::AnalysisOptions;
use crate::corrections::CorrectionsCommand;
use crate::export::ExportTarget;
use crate::output::Narrowing;
use crate::style::{ColorChoice, Theme};
use crate::tag_key::TagKeys;
use crate::tags::TagsCommand;
use crate::view::{FileFacts, KeyNotationChoice, ViewOptions};

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
    /// Keep the technical detail in error messages, such as the decoder's
    /// own reason a file cannot be read.
    #[arg(long, global = true, display_order = 1001)]
    verbose: bool,
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
    /// Write copies of tracks at one loudness, under a true-peak ceiling, for a club system.
    #[command(
        long_about = "Write copies of tracks that play evenly on a club system.\n\n\
                      Each track is brought to the same integrated loudness (EBU R128) and a \
                      transparent look-ahead limiter keeps the true peak under a ceiling. A \
                      track that would need more limiting than allowed gets less gain instead. \
                      With --tune, tracks measured off A = 440 Hz are shifted back to concert \
                      pitch with the tempo unchanged. Already clipped passages are reported, \
                      not repaired.\n\n\
                      Copies are new 24 bit files with the tags and cover art of the \
                      original, in its format (WAV, AIFF or FLAC); lossy tracks become FLAC \
                      or WAV and are never written as lossy. Nothing is ever overwritten.",
        after_help = help::prepare_examples(),
    )]
    Prepare(prepare::PrepareArguments),
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
    /// Show the key and tempo of the audio playing on this Mac, live.
    #[command(
        long_about = "Listen to the audio this Mac is playing, from rekordbox, Spotify, a \
                      browser or any other application, and print its key and tempo as they \
                      change: one line every two seconds once enough audio has played.\n\n\
                      This is the analysis of the Live window in the desktop app. The audio \
                      stays in memory: nothing is recorded, written or sent anywhere. \
                      macOS 14.2 or later, and macOS asks once to allow listening.",
        after_help = help::live_examples(),
    )]
    Live(live::LiveArguments),
    /// Rank the best tracks to play after a track: key, tempo and energy.
    #[command(
        long_about = "Rank the best tracks to play after a track, the same ranking as the \
                      Next after list of the desktop app.\n\n\
                      Candidates come from the folder of the track, or from --from: a folder, \
                      audio files or a playlist built in the desktop app. A candidate mixes \
                      in when its key is the same, one step on the wheel or the relative key, \
                      and its tempo is within 6 percent. Among those, the order follows the \
                      energy you ask for: kept, lifted with --lift or dropped with --drop. \
                      Each row says how the key moves, how far the tempo moves (Easy, Pitch \
                      move or Big jump) and how the energy changes. A key or tempo you set in \
                      the desktop app is the one used.",
        after_help = help::next_examples(),
    )]
    Next(next::NextArguments),
    /// Plan how to mix one track into the next: tempo, clashes and the best windows.
    #[command(
        long_about = "Plan how to mix track A, playing, into track B, next.\n\n\
                      B is stretched to the tempo of A within what a deck can play and laid \
                      on the bars of A, downbeat to downbeat. Both tracks are read bar by bar, \
                      and the windows of 16, 32 or 64 bars where A can go out and B come in \
                      are ranked, with the bar to swap the bass and bring in the vocals. \
                      Vocals, bass and drums come from cached stems (euphoria stems); \
                      without them they are unknown and clashes are not checked.",
        after_help = help::plan_examples(),
    )]
    Plan(plan::PlanArguments),
    /// Order tracks into a set that follows an energy curve, every mix in key.
    #[command(after_help = help::set_examples())]
    Set {
        #[command(subcommand)]
        command: set::SetCommand,
    },
    /// Print a script that completes `euphoria` commands and options in your shell.
    #[command(after_help = help::completions_examples())]
    Completions {
        /// The shell to write the script for.
        #[arg(value_enum)]
        shell: ShellChoice,
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
    /// List only the tracks whose key or tempo is rated low confidence or
    /// whose tags name another key, and the files that could not be
    /// analyzed. A key or tempo you set or confirmed with "Looks right" in
    /// the desktop app is no longer in doubt.
    #[arg(long)]
    needs_review: bool,
    #[command(flatten)]
    filters: filters::FilterArguments,
    /// List only the tracks at these set stages, separated by commas:
    /// warm-up, build or peak. The stage comes from the tempo and loudness
    /// of a track compared with the other tracks analyzed in the same run,
    /// so it needs at least 10 tracks. A tempo you set yourself is the one
    /// used. There is no closing stage.
    #[arg(long, value_enum, value_delimiter = ',', value_name = "STAGE")]
    stage: Vec<filters::StageChoice>,
    /// How keys are shown in the table and the detailed view.
    #[arg(long, value_enum, default_value_t, value_name = "NOTATION")]
    key_notation: KeyNotationChoice,
    /// Your rekordbox collection exported as XML (File > Export Collection
    /// in xml format). Tracks in it with a constant beat grid use that grid
    /// in place of the detected one, and their cues are placed on its bars.
    /// A tempo you set yourself wins over it.
    #[arg(long, value_name = "FILE")]
    collection: Option<PathBuf>,
    #[command(flatten)]
    analysis: AnalysisOptions,
}

/// A shell to write completions for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum ShellChoice {
    Bash,
    Zsh,
    Fish,
    #[value(name = "powershell")]
    PowerShell,
}

impl ShellChoice {
    fn shell(self) -> clap_complete::Shell {
        match self {
            ShellChoice::Bash => clap_complete::Shell::Bash,
            ShellChoice::Zsh => clap_complete::Shell::Zsh,
            ShellChoice::Fish => clap_complete::Shell::Fish,
            ShellChoice::PowerShell => clap_complete::Shell::PowerShell,
        }
    }
}

fn write_completions(shell: ShellChoice) -> Result<ExitCode> {
    let mut script = Vec::new();
    clap_complete::generate(shell.shell(), &mut Cli::command(), "euphoria", &mut script);
    let mut standard_output = std::io::stdout().lock();
    standard_output.write_all(&script)?;
    standard_output.flush()?;
    Ok(ExitCode::SUCCESS)
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

    output::set_verbose(cli.verbose);
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
        let io_kind = cause
            .downcast_ref::<std::io::Error>()
            .map(std::io::Error::kind)
            .or_else(|| {
                cause
                    .downcast_ref::<serde_json::Error>()
                    .and_then(serde_json::Error::io_error_kind)
            });
        io_kind == Some(std::io::ErrorKind::BrokenPipe)
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
        Command::Prepare(arguments) => prepare::run(&arguments),
        Command::Set { command } => set::run(command),
        Command::Plan(arguments) => plan::run(&arguments),
        Command::Next(arguments) => next::run(&arguments),
        Command::Live(arguments) => live::run(&arguments),
        Command::Completions { shell } => write_completions(shell),
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
    let mut track_filters = arguments.filters.track_filters()?;
    track_filters.stages = arguments
        .stage
        .iter()
        .map(|choice| choice.stage())
        .collect();
    let analysis = &arguments.analysis;
    let (missing_paths, present_paths): (Vec<PathBuf>, Vec<PathBuf>) = arguments
        .paths
        .iter()
        .cloned()
        .partition(|path| matches!(path.try_exists(), Ok(false)));
    let scan = scan::scan_quietly(&present_paths)?;
    let standard_error_theme = Theme::stderr();
    if scan.files.is_empty() && missing_paths.is_empty() {
        let mut standard_error = std::io::stderr().lock();
        scan::write_notices(&mut standard_error, &standard_error_theme, &scan)?;
        output::write_no_audio_files(&mut standard_error, &standard_error_theme)?;
        return Ok(output::exit_status(false, true));
    }
    let audio_files = scan.files.clone();
    let collection_grids = arguments
        .collection
        .as_deref()
        .map(grid_import::CollectionGrids::read)
        .transpose()?;
    let CachedBatch {
        mut results,
        cached_count,
        store_error,
    } = if audio_files.is_empty() {
        CachedBatch {
            results: Vec::new(),
            cached_count: 0,
            store_error: None,
        }
    } else {
        batch::analyze(
            &audio_files,
            analysis,
            if arguments.json {
                batch::Display::CountOnly
            } else {
                batch::Display::Stages
            },
        )?
    };
    results.extend(missing_paths.into_iter().map(|path| FileResult {
        outcome: Err(EuphoriaError::Missing { path: path.clone() }),
        path,
    }));
    results.sort_by(|first, second| first.path.cmp(&second.path));
    let user_edits = batch::user_edits(&results, analysis, batch::CorrectionCheck::Quick)?;
    let results = batch::corrected(results, &user_edits);
    let (results, grid_infos) =
        grid_import::apply(results, &user_edits, collection_grids.as_ref(), analysis)?;
    let corrections = batch::corrections(&results, analysis, batch::CorrectionCheck::Quick)?;
    let tag_keys = TagKeys::read(&results, &user_edits);
    let analyzed_count = results.len();
    let import_summary = arguments.collection.as_ref().map(|_| {
        let files: Vec<PathBuf> = results.iter().map(|result| result.path.clone()).collect();
        grid_import::ImportSummary::of(&files, &grid_infos)
    });

    let stages = filters::LibraryStages::of(&results);
    let verdicts = arguments.filters.verdicts();
    let is_listed = |path: &Path, analysis: &TrackAnalysis| {
        (!arguments.needs_review || !tag_keys.review_reasons(path, analysis).is_empty())
            && track_filters.matches(&filters::Filtered(analysis, stages.get(path)))
    };
    let narrowing = Narrowing {
        needs_review: arguments.needs_review,
        verdicts: verdicts.clone(),
        ranges: arguments.filters.describe_ranges(),
        shown_count: results
            .iter()
            .filter(|result| {
                result
                    .outcome
                    .as_ref()
                    .is_ok_and(|analysis| is_listed(&result.path, analysis))
            })
            .count(),
    };
    let missing_count = results
        .iter()
        .filter(|result| output::is_missing(result))
        .count();
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
            && !track_filters.is_active();
        if needs_space && (!one_track_in_detail || cached_count > 0) {
            writeln!(report)?;
        }
        if !one_track_in_detail {
            output::write_analysis_summary(
                &mut report,
                &standard_error_theme,
                &results,
                &narrowing,
                &tag_keys,
            )?;
        }
    }
    scan::write_notices(&mut report, &standard_error_theme, &scan)?;
    if cached_count > 0 {
        output::write_cache_summary(&mut report, cached_count, analyzed_count)?;
    }
    if let (Some(summary), Some(collection)) = (&import_summary, &arguments.collection) {
        output::write_grid_import_summary(&mut report, &standard_error_theme, summary, collection)?;
    }

    let listed_results: Vec<FileResult> = results
        .into_iter()
        .filter(|result| {
            result
                .outcome
                .as_ref()
                .map_or(true, |analysis| is_listed(&result.path, analysis))
        })
        .collect();
    let standard_output_theme = Theme::stdout();
    let mut standard_output = std::io::stdout().lock();
    match layout {
        Layout::Json => output::write_json(
            &mut standard_output,
            &listed_results,
            analysis.tempo_range(),
            &user_edits,
            &tag_keys,
            &output::JsonLookups {
                grid_infos: &grid_infos,
                stages: &stages,
            },
            &scan,
        )?,
        Layout::PlainTable => output::write_plain_table(
            &mut standard_output,
            &listed_results,
            &user_edits,
            arguments.key_notation,
        )?,
        Layout::Table => view::write_table(
            &mut standard_output,
            &standard_output_theme,
            &listed_results,
            &view_options,
            &corrections,
            &tag_keys,
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
                        FileFacts {
                            tag_key: tag_keys.get(&result.path),
                            grid_info: grid_infos.get(&result.path),
                            review: tag_keys.review_reasons(&result.path, track),
                            reviewed: tag_keys.reviewed(&result.path),
                            stage: stages.get(&result.path),
                        },
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
    Ok(output::exit_status(
        failure_count > missing_count,
        missing_count > 0 || scan.has_unreadable(),
    ))
}

pub(crate) fn with_causes(error: EuphoriaError) -> anyhow::Error {
    anyhow::anyhow!(output::describe(&error))
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
    fn recognises_a_broken_pipe_inside_a_json_error() {
        struct ClosedPipe;
        impl Write for ClosedPipe {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let error = serde_json::to_writer_pretty(ClosedPipe, &[1, 2, 3]).unwrap_err();
        assert!(is_broken_pipe(&anyhow::Error::from(error)));
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
}
