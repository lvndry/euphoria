// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs an analysis batch for a command: the cache options, the workers, the
//! progress shown on a terminal and the warning when the cache cannot be
//! used.

use std::io::IsTerminal;
use std::num::NonZeroUsize;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use euphoria_core::{
    AnalysisObserver, Cache, CachePolicy, CachedBatch, EuphoriaError, Progress, TempoRange,
    Unobserved, analyze_files_cached_observed, analyze_files_observed, default_cache_path,
};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressStyle};

use crate::output;
use crate::stages::StageLines;

/// A worker peaks near 0.6 GB on a seven minute track, and 8 workers on full
/// length tracks peaked between 4.8 and 6.2 GB in total, which leaves
/// headroom on a 16 GB machine. One worker per core on machines with more
/// cores would not.
const DEFAULT_MAXIMUM_JOBS: usize = 8;

/// The options shared by every command that analyzes files.
#[derive(Debug, Args)]
pub struct AnalysisOptions {
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
    /// Report tempos from SLOWEST up to FASTEST beats per minute, at most an
    /// octave apart, such as 70-140 for hip hop or 120-200 for drum and bass.
    /// Changing it needs no new analysis.
    #[arg(long, value_name = "SLOWEST-FASTEST", default_value_t = TempoRange::DEFAULT)]
    tempo_range: TempoRange,
}

impl AnalysisOptions {
    /// The range tempos are reported in.
    pub fn tempo_range(&self) -> TempoRange {
        self.tempo_range
    }
}

/// What a run shows on a terminal while it analyzes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    /// A line per file in progress with its stage, under the bar that counts
    /// files when there is more than one.
    Stages,
    /// Only the bar that counts files, as for JSON output.
    CountOnly,
}

/// Analyzes `audio_files` as the options ask, showing progress on standard
/// error when it is a terminal. A cache that cannot be used is a warning on
/// standard error, and the files are analyzed without it.
pub fn analyze(
    audio_files: &[PathBuf],
    options: &AnalysisOptions,
    display: Display,
) -> Result<CachedBatch> {
    let policy = if options.no_cache {
        None
    } else if options.reanalyze {
        Some(CachePolicy::Reanalyze)
    } else {
        Some(CachePolicy::UseCached)
    };
    let mut cache = match policy {
        None => None,
        Some(policy) => match open_cache() {
            Ok(cache) => Some((cache, policy)),
            Err(error) => {
                output::write_cache_warning(&mut std::io::stderr().lock(), &error)?;
                None
            }
        },
    };

    let job_count = options
        .jobs
        .map(NonZeroUsize::get)
        .unwrap_or_else(default_job_count);
    let thread_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(job_count)
        .build()
        .context("starting the analysis workers")?;

    let is_terminal = std::io::stderr().is_terminal();
    let progress = if is_terminal {
        MultiProgress::new()
    } else {
        MultiProgress::with_draw_target(ProgressDrawTarget::hidden())
    };
    let show_stages = is_terminal && display == Display::Stages;
    let count_bar = progress.add(count_bar(
        audio_files.len(),
        is_terminal && (audio_files.len() > 1 || !show_stages),
    ));
    let stage_lines = show_stages.then(|| StageLines::new(progress.clone()));
    #[cfg(debug_assertions)]
    let _debug_hint = is_terminal.then(|| crate::stages::DebugBuildHint::start(progress.clone()));

    let batch = match &stage_lines {
        Some(stage_lines) => run(
            &thread_pool,
            &mut cache,
            audio_files,
            stage_lines,
            &count_bar,
            &progress,
            |path| stage_lines.finish(path),
        )?,
        None => run(
            &thread_pool,
            &mut cache,
            audio_files,
            &Unobserved,
            &count_bar,
            &progress,
            |_| {},
        )?,
    };
    count_bar.finish_and_clear();
    let _ = progress.clear();
    Ok(in_tempo_range(batch, options.tempo_range))
}

/// Runs the batch with `observer` watching each file, with the cache when
/// there is one, and without it when it cannot be used.
fn run(
    thread_pool: &rayon::ThreadPool,
    cache: &mut Option<(Cache, CachePolicy)>,
    audio_files: &[PathBuf],
    observer: &impl AnalysisObserver,
    count_bar: &ProgressBar,
    progress: &MultiProgress,
    on_file_done: impl Fn(&std::path::Path) + Sync,
) -> Result<CachedBatch> {
    let on_progress = |done: Progress<'_>| {
        on_file_done(done.path);
        count_bar.set_position(done.completed as u64);
    };
    let analyze_without_cache = || CachedBatch {
        results: thread_pool.install(|| analyze_files_observed(audio_files, observer, on_progress)),
        cached_count: 0,
        store_error: None,
    };
    Ok(match cache {
        None => analyze_without_cache(),
        Some((cache, policy)) => {
            let cached_batch = thread_pool.install(|| {
                analyze_files_cached_observed(cache, audio_files, *policy, observer, on_progress)
            });
            match cached_batch {
                Ok(batch) => batch,
                Err(error) => {
                    progress.suspend(|| {
                        output::write_cache_warning(&mut std::io::stderr().lock(), &error)
                    })?;
                    analyze_without_cache()
                }
            }
        }
    })
}

/// The batch with every tempo reported in `range`.
fn in_tempo_range(mut batch: CachedBatch, range: TempoRange) -> CachedBatch {
    for result in &mut batch.results {
        if let Ok(analysis) = &mut result.outcome {
            *analysis = analysis.clone().with_tempo_range(range);
        }
    }
    batch
}

/// Opens the analysis cache in the data folder.
pub fn open_cache() -> Result<Cache, EuphoriaError> {
    Cache::open(&default_cache_path()?)
}

impl AnalysisOptions {
    /// True unless the cache is turned off with `--no-cache`.
    pub fn uses_cache(&self) -> bool {
        !self.no_cache
    }
}

fn default_job_count() -> usize {
    std::thread::available_parallelism()
        .map(NonZeroUsize::get)
        .unwrap_or(1)
        .min(DEFAULT_MAXIMUM_JOBS)
}

/// The bar that counts finished files, hidden unless `shown`.
fn count_bar(total: usize, shown: bool) -> ProgressBar {
    if !shown {
        return ProgressBar::hidden();
    }
    let progress_bar = ProgressBar::new(total as u64);
    if let Ok(style) = ProgressStyle::with_template("Analyzing {pos}/{len} {wide_bar} {eta} left") {
        progress_bar.set_style(style);
    }
    progress_bar
}
