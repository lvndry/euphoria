// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `euphoria` command line tool.

mod batch;
mod export;
mod output;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use euphoria_core::{
    CachedBatch, EuphoriaError, clear_cache, default_cache_path, find_audio_files,
};

use crate::batch::AnalysisOptions;
use crate::export::ExportTarget;

#[derive(Debug, Parser)]
#[command(
    name = "euphoria",
    version,
    about = "Find the key and BPM of every track in your music collection"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Detect the key and tempo of audio files and folders.
    Analyze {
        /// Audio files, or folders to search recursively.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        analysis: AnalysisOptions,
    },
    /// Analyze audio files and write the results for other DJ software.
    Export {
        #[command(subcommand)]
        target: ExportTarget,
    },
    /// Manage the cache of analysis results.
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
}

#[derive(Debug, Subcommand)]
enum CacheCommand {
    /// Print where the cache database is stored.
    Path,
    /// Forget every cached result, so every track is analyzed again.
    Clear,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(exit_code) => exit_code,
        Err(error) if is_broken_pipe(&error) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
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
        Command::Analyze {
            paths,
            json,
            analysis,
        } => analyze(&paths, json, &analysis),
        Command::Export { target } => export::run(target),
        Command::Cache { command } => manage_cache(command),
    }
}

fn manage_cache(command: CacheCommand) -> Result<ExitCode> {
    let cache_path = default_cache_path().map_err(with_causes)?;
    match command {
        CacheCommand::Path => println!("{}", cache_path.display()),
        CacheCommand::Clear => {
            if clear_cache(&cache_path).map_err(with_causes)? {
                println!("Cleared the cache at {}", cache_path.display());
            } else {
                println!("There is no cache at {}", cache_path.display());
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn analyze(paths: &[PathBuf], json: bool, analysis: &AnalysisOptions) -> Result<ExitCode> {
    let audio_files = find_audio_files(paths).map_err(with_causes)?;
    if audio_files.is_empty() {
        eprintln!("No audio files found.");
        return Ok(ExitCode::SUCCESS);
    }
    let CachedBatch {
        results,
        cached_count,
        store_error,
    } = batch::analyze(&audio_files, analysis)?;

    let mut standard_output = std::io::stdout().lock();
    if json {
        output::write_json(&mut standard_output, &results)?;
    } else {
        output::write_table(&mut standard_output, &results)?;
    }
    standard_output.flush()?;
    if cached_count > 0 {
        output::write_cache_summary(&mut std::io::stderr().lock(), cached_count, results.len())?;
    }

    let failure_count = results
        .iter()
        .filter(|result| result.outcome.is_err())
        .count();
    if failure_count > 0 && !json {
        output::write_failures(&mut std::io::stderr().lock(), &results)?;
    }
    if let Some(store_error) = store_error {
        return Err(with_causes(store_error).context("some results were not saved to the cache"));
    }
    Ok(if failure_count == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn with_causes(error: EuphoriaError) -> anyhow::Error {
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
}
