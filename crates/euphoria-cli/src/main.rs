// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `euphoria` command line tool.

mod output;

use std::io::{IsTerminal, Write};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use euphoria_core::{analyze_files, find_audio_files};
use indicatif::{ProgressBar, ProgressStyle};

/// Each worker can hold 2 to 3 GB while analyzing a full length track, so the
/// default stays well below the core count of most machines.
const DEFAULT_MAXIMUM_JOBS: usize = 4;

#[derive(Debug, Parser)]
#[command(
    name = "euphoria",
    version,
    about = "Key, tempo and energy analysis for DJ libraries"
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
        /// Number of files to analyze at once. Defaults to the number of
        /// cores, at most 4.
        #[arg(long, short = 'j')]
        jobs: Option<NonZeroUsize>,
    },
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(exit_code) => exit_code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Analyze { paths, json, jobs } => analyze(&paths, json, jobs),
    }
}

fn analyze(paths: &[PathBuf], json: bool, jobs: Option<NonZeroUsize>) -> Result<ExitCode> {
    let audio_files = find_audio_files(paths).map_err(|error| anyhow::anyhow!(error.chain()))?;
    if audio_files.is_empty() {
        eprintln!("No audio files found.");
        return Ok(ExitCode::SUCCESS);
    }

    let job_count = jobs
        .map(NonZeroUsize::get)
        .unwrap_or_else(default_job_count);
    let thread_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(job_count)
        .build()
        .context("starting the analysis workers")?;

    let progress_bar = progress_bar(audio_files.len());
    let results = thread_pool.install(|| {
        analyze_files(&audio_files, |progress| {
            progress_bar.set_position(progress.completed as u64);
        })
    });
    progress_bar.finish_and_clear();

    let mut standard_output = std::io::stdout().lock();
    if json {
        output::write_json(&mut standard_output, &results)?;
    } else {
        output::write_table(&mut standard_output, &results)?;
    }
    standard_output.flush()?;

    let failure_count = results
        .iter()
        .filter(|result| result.outcome.is_err())
        .count();
    if failure_count == 0 {
        return Ok(ExitCode::SUCCESS);
    }
    if !json {
        output::write_failures(&mut std::io::stderr().lock(), &results)?;
    }
    Ok(ExitCode::FAILURE)
}

fn default_job_count() -> usize {
    std::thread::available_parallelism()
        .map(NonZeroUsize::get)
        .unwrap_or(1)
        .min(DEFAULT_MAXIMUM_JOBS)
}

fn progress_bar(total: usize) -> ProgressBar {
    if !std::io::stderr().is_terminal() {
        return ProgressBar::hidden();
    }
    let progress_bar = ProgressBar::new(total as u64);
    if let Ok(style) = ProgressStyle::with_template("Analyzing {pos}/{len} {wide_bar} {eta} left") {
        progress_bar.set_style(style);
    }
    progress_bar
}
