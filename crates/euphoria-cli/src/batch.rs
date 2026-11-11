// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs an analysis batch for a command: the cache options, the workers, the
//! progress bar and the warning when the cache cannot be used.

use std::io::IsTerminal;
use std::num::NonZeroUsize;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use euphoria_core::{
    Cache, CachePolicy, CachedBatch, EuphoriaError, Progress, TempoRange, analyze_files,
    analyze_files_cached, default_cache_path,
};
use indicatif::{ProgressBar, ProgressStyle};

use crate::output;

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

/// Analyzes `audio_files` as the options ask, showing a progress bar on a
/// terminal. A cache that cannot be used is a warning on standard error, and
/// the files are analyzed without it.
pub fn analyze(audio_files: &[PathBuf], options: &AnalysisOptions) -> Result<CachedBatch> {
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

    let progress_bar = progress_bar(audio_files.len());
    let on_progress = |progress: Progress<'_>| {
        progress_bar.set_position(progress.completed as u64);
    };
    let analyze_without_cache = || CachedBatch {
        results: thread_pool.install(|| analyze_files(audio_files, on_progress)),
        cached_count: 0,
        store_error: None,
    };
    let batch = match &mut cache {
        None => analyze_without_cache(),
        Some((cache, policy)) => {
            let cached_batch = thread_pool
                .install(|| analyze_files_cached(cache, audio_files, *policy, on_progress));
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
    Ok(in_tempo_range(batch, options.tempo_range))
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

/// The corrections users set, by file, for the files of `results` that
/// have one. Nothing is read without a cache, with `--no-cache`, or when no
/// file was ever corrected; a cache that cannot be read gives none.
pub fn corrections(
    results: &[euphoria_core::FileResult],
    options: &AnalysisOptions,
    check: CorrectionCheck,
) -> Corrections {
    if options.no_cache || !default_cache_path().is_ok_and(|path| path.exists()) {
        return Corrections::new();
    }
    let Ok(cache) = open_cache() else {
        return Corrections::new();
    };
    results
        .iter()
        .filter(|result| result.outcome.is_ok())
        .filter_map(|result| {
            let correction = match check {
                CorrectionCheck::Quick => cache.correction_for(&result.path),
                CorrectionCheck::ReadFile => cache.verified_correction_for(&result.path),
            };
            let correction = correction.ok().flatten()?;
            Some((result.path.clone(), correction))
        })
        .collect()
}

/// How sure to be that a correction belongs to the file it is applied to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectionCheck {
    /// Trust a file at its recorded path with its recorded size and
    /// modification time, as the analysis cache does: enough for showing.
    Quick,
    /// Read the file to be sure, before values go into tags or another
    /// program's library.
    ReadFile,
}

/// The corrections of a batch, by file.
pub type Corrections = std::collections::HashMap<PathBuf, euphoria_core::Correction>;

/// The results of a batch with every correction in place of the detected
/// key and tempo.
pub fn corrected(
    results: Vec<euphoria_core::FileResult>,
    corrections: &Corrections,
) -> Vec<euphoria_core::FileResult> {
    results
        .into_iter()
        .map(|mut result| {
            if let (Ok(analysis), Some(correction)) =
                (&mut result.outcome, corrections.get(&result.path))
            {
                *analysis = correction.apply(analysis.clone());
            }
            result
        })
        .collect()
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
