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
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget, ProgressState, ProgressStyle};

use crate::output;
use crate::stages::StageLines;
use crate::style::Theme;

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

/// Analyzes `audio_files` as the options ask, showing progress on standard
/// error when it is a terminal: a line per file with the stage it is in,
/// under a bar that counts files when there is more than one. A cache that
/// cannot be used is a warning on standard error, and the files are
/// analyzed without it.
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
                output::write_cache_warning(
                    &mut std::io::stderr().lock(),
                    &Theme::stderr(),
                    &error,
                )?;
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
    let count_bar = if is_terminal && audio_files.len() > 1 {
        let count_bar = progress.add(count_bar(audio_files.len()));
        count_bar.enable_steady_tick(std::time::Duration::from_millis(250));
        count_bar
    } else {
        ProgressBar::hidden()
    };
    let stage_lines = is_terminal.then(|| StageLines::new(progress.clone(), audio_files.len() > 1));
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
                        output::write_cache_warning(
                            &mut std::io::stderr().lock(),
                            &Theme::stderr(),
                            &error,
                        )
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

/// The corrections users set, by file, for the files of `results` that
/// have one. Nothing is read without a cache, with `--no-cache`, or when no
/// file was ever corrected; a cache that cannot be read gives none.
///
/// With [`CorrectionCheck::Quick`] a correction that cannot be looked up is
/// left out, so the detected values are shown. With
/// [`CorrectionCheck::ReadFile`] the file is marked as unchecked instead,
/// and [`corrected`] turns it into a failure so it is not written or
/// exported with values the user may have replaced. A cache that cannot be
/// opened at all gives none, as with `--no-cache`: the analysis already
/// warned that it runs without the cache.
pub fn corrections(
    results: &[euphoria_core::FileResult],
    options: &AnalysisOptions,
    check: CorrectionCheck,
) -> Result<Corrections> {
    if options.no_cache || !default_cache_path().is_ok_and(|path| path.exists()) {
        return Ok(Corrections::new());
    }
    let analyzed = results.iter().filter(|result| result.outcome.is_ok());
    let cache = match (open_cache(), check) {
        (Ok(cache), _) => cache,
        // Showing goes on with the detected values; the analysis already
        // warned that it ran without the cache.
        (Err(_), CorrectionCheck::Quick) => return Ok(Corrections::new()),
        (Err(error), CorrectionCheck::ReadFile) => return Err(corrections_unreadable(&error)),
    };
    Ok(analyzed
        .filter_map(|result| {
            let looked_up = match check {
                CorrectionCheck::Quick => cache.correction_for(&result.path),
                CorrectionCheck::ReadFile => cache.verified_correction_for(&result.path),
            };
            match (looked_up, check) {
                (Ok(correction), _) => {
                    correction.map(|correction| (result.path.clone(), Ok(correction)))
                }
                (Err(_), CorrectionCheck::Quick) => None,
                (Err(error), CorrectionCheck::ReadFile) => {
                    Some((result.path.clone(), Err(error.chain())))
                }
            }
        })
        .collect())
}

/// Stops a tag write or an export before it starts when the cache exists
/// but cannot be opened: it may hold keys and tempos the user set, and the
/// detected values must not be written over them unknowingly.
pub fn require_readable_corrections(options: &AnalysisOptions) -> Result<()> {
    if options.no_cache || !default_cache_path().is_ok_and(|path| path.exists()) {
        return Ok(());
    }
    open_cache()
        .map(|_| ())
        .map_err(|error| corrections_unreadable(&error))
}

fn corrections_unreadable(error: &EuphoriaError) -> anyhow::Error {
    anyhow::anyhow!(
        "{}, so the keys and tempos set in the desktop app cannot be read. Nothing was written. \
         Pass --no-cache to go on with the detected values instead.",
        error.chain()
    )
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

/// The corrections of a batch, by file, or why a file's could not be
/// checked.
pub type Corrections =
    std::collections::HashMap<PathBuf, Result<euphoria_core::Correction, String>>;

/// The results of a batch with every correction in place of the detected
/// key and tempo.
pub fn corrected(
    results: Vec<euphoria_core::FileResult>,
    corrections: &Corrections,
) -> Vec<euphoria_core::FileResult> {
    results
        .into_iter()
        .map(|mut result| {
            match (&mut result.outcome, corrections.get(&result.path)) {
                (Ok(analysis), Some(Ok(correction))) => {
                    *analysis = correction.apply(analysis.clone());
                }
                (Ok(_), Some(Err(reason))) => {
                    result.outcome = Err(EuphoriaError::CorrectionUnchecked {
                        path: result.path.clone(),
                        source: reason.clone().into(),
                    });
                }
                _ => {}
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

/// The bar that counts finished files, to add to a `MultiProgress`.
fn count_bar(total: usize) -> ProgressBar {
    // Hidden until `MultiProgress::add` gives it the terminal, so that it
    // never draws a line of its own.
    let progress_bar =
        ProgressBar::with_draw_target(Some(total as u64), ProgressDrawTarget::hidden());
    if let Ok(style) = ProgressStyle::with_template(
        "{prefix:.bold} {pos}/{len}  {bar:32.cyan/dim}  {elapsed} so far{left}",
    ) {
        progress_bar.set_style(
            style
                .progress_chars("━━─")
                .with_key("left", write_time_left),
        );
    }
    progress_bar.set_prefix("Analyzing");
    progress_bar
}

/// Writes about how long is left, once enough files have finished for the
/// estimate to mean something.
fn write_time_left(state: &ProgressState, writer: &mut dyn std::fmt::Write) {
    if state.pos() < 2 || state.pos() >= state.len().unwrap_or(0) {
        return;
    }
    let seconds = state.eta().as_secs();
    let _ = if seconds < 60 {
        write!(writer, ", about {seconds}s left")
    } else {
        write!(writer, ", about {} min left", seconds.div_ceil(60))
    };
}
