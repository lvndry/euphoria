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
use euphoria_core::{
    Cache, CachePolicy, CachedBatch, EuphoriaError, Progress, analyze_files, analyze_files_cached,
    clear_cache, default_cache_path, find_audio_files,
};
use indicatif::{ProgressBar, ProgressStyle};

/// A worker peaks near 0.6 GB on a seven minute track, and 8 workers on full
/// length tracks peaked between 4.8 and 6.2 GB in total, which leaves
/// headroom on a 16 GB machine. One worker per core on machines with more
/// cores would not.
const DEFAULT_MAXIMUM_JOBS: usize = 8;

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
    /// Detect the key, tempo and energy of audio files and folders.
    Analyze {
        /// Audio files, or folders to search recursively.
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
        /// Number of files to analyze at once. Defaults to the number of
        /// cores, at most 8.
        #[arg(long, short = 'j')]
        jobs: Option<NonZeroUsize>,
        /// Analyze every file and neither read nor update the cache.
        #[arg(long, conflicts_with = "reanalyze")]
        no_cache: bool,
        /// Analyze every file again and replace its cached result.
        #[arg(long)]
        reanalyze: bool,
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

/// How `analyze` uses the cache.
#[derive(Debug, Clone, Copy)]
enum CacheUse {
    Bypass,
    Use(CachePolicy),
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
            jobs,
            no_cache,
            reanalyze,
        } => {
            let cache_use = if no_cache {
                CacheUse::Bypass
            } else if reanalyze {
                CacheUse::Use(CachePolicy::Reanalyze)
            } else {
                CacheUse::Use(CachePolicy::UseCached)
            };
            analyze(&paths, json, jobs, cache_use)
        }
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

fn analyze(
    paths: &[PathBuf],
    json: bool,
    jobs: Option<NonZeroUsize>,
    cache_use: CacheUse,
) -> Result<ExitCode> {
    let audio_files = find_audio_files(paths).map_err(with_causes)?;
    if audio_files.is_empty() {
        eprintln!("No audio files found.");
        return Ok(ExitCode::SUCCESS);
    }
    let mut cache = match cache_use {
        CacheUse::Bypass => None,
        CacheUse::Use(policy) => match open_cache() {
            Ok(cache) => Some((cache, policy)),
            Err(error) => {
                output::write_cache_warning(&mut std::io::stderr().lock(), &error)?;
                None
            }
        },
    };

    let job_count = jobs
        .map(NonZeroUsize::get)
        .unwrap_or_else(default_job_count);
    let thread_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(job_count)
        .build()
        .context("starting the analysis workers")?;

    let progress_bar = progress_bar(audio_files.len());
    let on_progress = |progress: Progress<'_>| {
        progress_bar.set_position(progress.completed as u64);
    };
    let analyze_without_cache = || CachedBatch {
        results: thread_pool.install(|| analyze_files(&audio_files, on_progress)),
        cached_count: 0,
        store_error: None,
    };
    let batch = match &mut cache {
        None => analyze_without_cache(),
        Some((cache, policy)) => {
            let cached_batch = thread_pool
                .install(|| analyze_files_cached(cache, &audio_files, *policy, on_progress));
            match cached_batch {
                Ok(batch) => batch,
                Err(error) => {
                    progress_bar.suspend(|| {
                        output::write_cache_warning(&mut std::io::stderr().lock(), &error)
                    })?;
                    analyze_without_cache()
                }
            }
        }
    };
    progress_bar.finish_and_clear();
    let CachedBatch {
        results,
        cached_count,
        store_error,
    } = batch;

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

fn open_cache() -> Result<Cache, EuphoriaError> {
    Cache::open(&default_cache_path()?)
}

fn with_causes(error: EuphoriaError) -> anyhow::Error {
    anyhow::anyhow!(error.chain())
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
