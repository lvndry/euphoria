// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs an analysis batch for a command: the cache options, the workers, the
//! progress shown on a terminal and the warning when the cache cannot be
//! used.

use std::collections::HashMap;
use std::io::IsTerminal;
use std::num::NonZeroUsize;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use euphoria_core::{
    AnalysisObserver, Cache, CachePolicy, CachedBatch, Correction, Cue, EuphoriaError, Progress,
    TempoRange, TrackCues, Unobserved, analyze_files_cached_observed, analyze_files_observed,
    default_cache_path,
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
    /// How many files are worked on at once.
    pub fn job_count(&self) -> usize {
        self.jobs
            .map(NonZeroUsize::get)
            .unwrap_or_else(default_job_count)
    }

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
    /// Only the bar that counts files, so the results on standard output
    /// are not crossed by stage lines, as for JSON output.
    CountOnly,
}

/// Analyzes `audio_files` as the options ask, showing progress on standard
/// error when it is a terminal: `display` says whether the line per file with
/// the stage it is in is shown, under a bar that counts files when there is
/// more than one. A cache that cannot be used is a warning on standard
/// error, and the files are analyzed without it.
pub fn analyze(
    audio_files: &[PathBuf],
    options: &AnalysisOptions,
    display: Display,
) -> Result<CachedBatch> {
    let batch = analyze_in_default_range(audio_files, options, display)?;
    Ok(in_tempo_range(batch, options.tempo_range))
}

/// Runs the same batch while retaining the detected analysis in the default
/// range. Tag writers keep this copy for the cache, since projecting a grid
/// into shorter bars can discard its original downbeat phase.
pub fn analyze_in_default_range(
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
    let show_stages = is_terminal && display == Display::Stages;
    let count_bar = if is_terminal && (audio_files.len() > 1 || !show_stages) {
        let count_bar = progress.add(count_bar(audio_files.len()));
        count_bar.enable_steady_tick(std::time::Duration::from_millis(250));
        count_bar
    } else {
        ProgressBar::hidden()
    };
    let stage_lines = show_stages.then(|| StageLines::new(progress.clone(), audio_files.len() > 1));
    #[cfg(debug_assertions)]
    let _debug_hint = show_stages.then(|| crate::stages::DebugBuildHint::start(progress.clone()));

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
    Ok(batch)
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

/// The corrections and cues users set, by file, for the files of `results`
/// that have any. Nothing is read without a cache, with `--no-cache`, or
/// when no file was ever edited; a cache that cannot be read gives none.
///
/// With [`CorrectionCheck::Quick`] an edit that cannot be looked up is left
/// out, so the detected values are shown. With
/// [`CorrectionCheck::ReadFile`] the file is marked as unchecked instead,
/// and [`corrected`] turns it into a failure so it is not written or
/// exported with values the user may have replaced. A cache that cannot be
/// opened at all gives none, as with `--no-cache`: the analysis already
/// warned that it runs without the cache.
pub fn user_edits(
    results: &[euphoria_core::FileResult],
    options: &AnalysisOptions,
    check: CorrectionCheck,
) -> Result<UserEditsByFile> {
    if options.no_cache || !default_cache_path().is_ok_and(|path| path.exists()) {
        return Ok(UserEditsByFile::new());
    }
    let analyzed = results.iter().filter(|result| result.outcome.is_ok());
    let cache = match (open_cache(), check) {
        (Ok(cache), _) => cache,
        // Showing goes on with the detected values; the analysis already
        // warned that it ran without the cache.
        (Err(_), CorrectionCheck::Quick) => return Ok(UserEditsByFile::new()),
        (Err(error), CorrectionCheck::ReadFile) => return Err(edits_unreadable(&error)),
    };
    Ok(analyzed
        .filter_map(|result| {
            let path = &result.path;
            let looked_up = match check {
                CorrectionCheck::Quick => Ok(UserEdits {
                    correction: cache
                        .correction_for(path)
                        .ok()
                        .flatten()
                        .unwrap_or_default(),
                    cues: cache.edited_cues_for(path).ok().flatten(),
                }),
                CorrectionCheck::ReadFile => {
                    cache
                        .verified_user_edits_for(path)
                        .map(|(correction, cues)| UserEdits {
                            correction: correction.unwrap_or_default(),
                            cues,
                        })
                }
            };
            match looked_up {
                Ok(edits) if edits.is_empty() => None,
                Ok(edits) => Some((path.clone(), Ok(edits))),
                Err(error) => Some((path.clone(), Err(error.chain()))),
            }
        })
        .collect())
}

/// Stops a tag write or an export before it starts when the cache exists
/// but cannot be opened: it may hold keys, tempos and cues the user set,
/// and the detected values must not be written over them unknowingly.
pub fn require_readable_user_edits(options: &AnalysisOptions) -> Result<()> {
    if options.no_cache || !default_cache_path().is_ok_and(|path| path.exists()) {
        return Ok(());
    }
    open_cache()
        .map(|_| ())
        .map_err(|error| edits_unreadable(&error))
}

/// The corrections users set, by file, for the views that show only the key
/// and tempo: a cue the user set does not change what a view draws.
pub fn corrections(
    results: &[euphoria_core::FileResult],
    options: &AnalysisOptions,
    check: CorrectionCheck,
) -> Result<Corrections> {
    let edits = user_edits(results, options, check)?;
    let corrections: Corrections = edits
        .into_iter()
        .map(|(path, edits)| (path, edits.map(|edits| edits.correction)))
        .collect();
    Ok(corrections)
}

fn edits_unreadable(error: &EuphoriaError) -> anyhow::Error {
    anyhow::anyhow!(
        "{}, so the keys, tempos and cues set in the desktop app cannot be read. Nothing was \
         written. Pass --no-cache to go on with the detected values instead.",
        error.chain()
    )
}

/// How sure to be that an edit belongs to the file it is applied to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CorrectionCheck {
    /// Trust a file at its recorded path with its recorded size and
    /// modification time, as the analysis cache does: enough for showing.
    Quick,
    /// Read the file to be sure, before values go into tags or another
    /// program's library.
    ReadFile,
}

/// What the user set for one file in the desktop app.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserEdits {
    /// The key and tempo the user set, if any.
    pub correction: Correction,
    /// The user's cues, which replace those analysis placed, or `None`.
    pub cues: Option<Vec<Cue>>,
}

impl UserEdits {
    /// True when the user set nothing.
    pub fn is_empty(&self) -> bool {
        self.correction.is_empty() && self.cues.is_none()
    }

    /// The cues of the file: the user's when set, or those analysis placed
    /// in `analysis`, which carries the correction already.
    pub fn track_cues(&self, analysis: &euphoria_core::TrackAnalysis) -> TrackCues {
        TrackCues::of(analysis, self.cues.clone())
    }
}

/// The edits of a batch, by file, or why a file's could not be checked.
pub type UserEditsByFile = HashMap<PathBuf, Result<UserEdits, String>>;

/// The corrections of a batch, by file, or why a file's could not be
/// checked.
pub type Corrections = HashMap<PathBuf, Result<Correction, String>>;

/// The user's cues in `edits`, by file, as an export takes them.
pub fn edited_cues(edits: &UserEditsByFile) -> HashMap<PathBuf, Vec<Cue>> {
    edits
        .iter()
        .filter_map(|(path, edits)| {
            let cues = edits.as_ref().ok()?.cues.clone()?;
            Some((path.clone(), cues))
        })
        .collect()
}

/// The results of a batch with every correction in place of the detected
/// key and tempo.
pub fn corrected(
    results: Vec<euphoria_core::FileResult>,
    edits: &UserEditsByFile,
) -> Vec<euphoria_core::FileResult> {
    results
        .into_iter()
        .map(|mut result| {
            match (&mut result.outcome, edits.get(&result.path)) {
                (Ok(analysis), Some(Ok(edits))) => {
                    *analysis = edits.correction.apply(analysis.clone());
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

/// Writes about how long is left, once enough files or segments have
/// finished for the estimate to mean something.
pub fn write_time_left(state: &ProgressState, writer: &mut dyn std::fmt::Write) {
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
