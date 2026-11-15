// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Preparing the selected files for a club system from the window: a sheet
//! with the options and a preview of what each file gets, then the writing
//! with a progress bar, then a toast that shows the new files.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use euphoria_core::prepare::{
    DEFAULT_SUFFIX, LossyOutput, PrepareSettings, TrackMeasurement, TrackPlan, measure_track,
    prepare_track, prepared_path,
};
use gpui::{Context, PathPromptOptions};

use crate::app::EuphoriaApp;
use crate::toast::ToastAction;

/// The loudness targets the sheet offers, in LUFS.
pub(crate) const TARGETS_LUFS: [f32; 4] = [-14.0, -11.0, -9.0, -7.0];
/// The true-peak ceilings the sheet offers, in dBTP.
pub(crate) const CEILINGS_DB: [f32; 3] = [-2.0, -1.0, -0.5];

/// How often the progress bar is redrawn while files are written.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);
/// Fractions of a file are counted in thousandths.
const PROGRESS_STEPS: usize = 1000;

/// One file in the sheet.
pub(crate) struct PrepareTrack {
    pub(crate) path: PathBuf,
    /// How far an analysis found the track from 440 Hz, in cents.
    pub(crate) tuning_cents: Option<f32>,
    /// What measuring found, or why it failed; `None` until it is done.
    pub(crate) measurement: Option<Result<TrackMeasurement, String>>,
}

/// What the preview says about one file.
#[derive(Debug, Clone)]
pub(crate) struct PreviewRow {
    pub(crate) path: PathBuf,
    pub(crate) file_name: String,
    /// The plan for the file, or why it cannot be prepared; `None` while it
    /// is still being measured.
    pub(crate) outcome: Option<Result<TrackPlan, String>>,
    pub(crate) output: Option<PathBuf>,
}

impl PreviewRow {
    pub(crate) fn plan(&self) -> Option<&TrackPlan> {
        self.outcome.as_ref()?.as_ref().ok()
    }

    pub(crate) fn will_be_written(&self) -> bool {
        self.plan().is_some_and(|plan| plan.skipped.is_none()) && self.output.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PrepareStage {
    /// Choices are open and the preview is on show.
    Setup,
    /// Files are being written.
    Writing,
}

/// How far the writing has come, shared with the thread that writes.
#[derive(Clone, Default)]
pub(crate) struct PrepareProgress {
    files_done: Arc<AtomicUsize>,
    current_thousandths: Arc<AtomicUsize>,
}

impl PrepareProgress {
    /// Files finished and the share of the file in progress, from 0 to 1.
    pub(crate) fn position(&self) -> (usize, f32) {
        (
            self.files_done.load(Ordering::Relaxed),
            self.current_thousandths.load(Ordering::Relaxed) as f32 / PROGRESS_STEPS as f32,
        )
    }
}

pub(crate) struct PreparePanel {
    pub(crate) settings: PrepareSettings,
    pub(crate) lossy: LossyOutput,
    /// Where the copies go; `None` is next to each original.
    pub(crate) output_folder: Option<PathBuf>,
    pub(crate) tracks: Vec<PrepareTrack>,
    pub(crate) rows: Vec<PreviewRow>,
    pub(crate) stage: PrepareStage,
    pub(crate) progress: PrepareProgress,
    pub(crate) problem: Option<String>,
}

impl PreparePanel {
    fn new(tracks: Vec<PrepareTrack>) -> Self {
        let mut panel = Self {
            settings: PrepareSettings::default(),
            lossy: LossyOutput::default(),
            output_folder: None,
            tracks,
            rows: Vec::new(),
            stage: PrepareStage::Setup,
            progress: PrepareProgress::default(),
            problem: None,
        };
        panel.refresh_rows();
        panel
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.stage == PrepareStage::Writing
    }

    pub(crate) fn measured_count(&self) -> usize {
        self.tracks
            .iter()
            .filter(|track| track.measurement.is_some())
            .count()
    }

    pub(crate) fn is_measuring(&self) -> bool {
        self.measured_count() < self.tracks.len()
    }

    pub(crate) fn files_to_write(&self) -> usize {
        self.rows.iter().filter(|row| row.will_be_written()).count()
    }

    /// Works out every row again from the measurements and the choices. The
    /// numbers are those of a dry run of the command line: the writing
    /// applies these plans.
    pub(crate) fn refresh_rows(&mut self) {
        let mut reserved = HashSet::new();
        self.rows = self
            .tracks
            .iter()
            .map(|track| {
                let file_name = track
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let outcome = track.measurement.as_ref().map(|measurement| {
                    measurement
                        .as_ref()
                        .map_err(Clone::clone)
                        .and_then(|measurement| {
                            measurement
                                .plan(&self.settings, track.tuning_cents, self.lossy)
                                .map_err(|error| error.chain().to_string())
                        })
                });
                let output = outcome
                    .as_ref()
                    .and_then(|outcome| outcome.as_ref().ok())
                    .filter(|plan| plan.skipped.is_none())
                    .map(|plan| {
                        let path = prepared_path(
                            &track.path,
                            self.output_folder.as_deref(),
                            DEFAULT_SUFFIX,
                            plan.output_format,
                            &reserved,
                        );
                        reserved.insert(path.clone());
                        path
                    });
                PreviewRow {
                    path: track.path.clone(),
                    file_name,
                    outcome,
                    output,
                }
            })
            .collect();
    }
}

/// "12 files", "1 file".
pub(crate) fn files(count: usize) -> String {
    let noun = if count == 1 { "file" } else { "files" };
    format!("{} {noun}", crate::format::count(count))
}

impl EuphoriaApp {
    /// Opens the sheet on the selected files and starts measuring them.
    pub(crate) fn open_prepare_files(&mut self, cx: &mut Context<'_, Self>) {
        if self
            .prepare_panel
            .as_ref()
            .is_some_and(PreparePanel::is_busy)
            || self
                .library_export
                .as_ref()
                .is_some_and(crate::library_export::LibraryExportPanel::is_busy)
            || self
                .tag_panel
                .as_ref()
                .is_some_and(crate::tag_flow::TagPanel::is_busy)
        {
            return;
        }
        let library = self.session.library();
        let tracks: Vec<PrepareTrack> = self
            .selected_tracks()
            .into_iter()
            .filter_map(|index| library.track(index))
            .map(|track| PrepareTrack {
                path: track.path.clone(),
                tuning_cents: track.analysis().and_then(|analysis| analysis.tuning_cents),
                measurement: None,
            })
            .collect();
        if tracks.is_empty() {
            return;
        }
        self.settings_open = false;
        self.tag_panel = None;
        self.library_export = None;
        let paths: Vec<PathBuf> = tracks.iter().map(|track| track.path.clone()).collect();
        self.prepare_panel = Some(PreparePanel::new(tracks));
        cx.notify();
        self.measure_prepare_tracks(paths, cx);
    }

    fn measure_prepare_tracks(&mut self, paths: Vec<PathBuf>, cx: &mut Context<'_, Self>) {
        cx.spawn(async move |this, cx| {
            for path in paths {
                let measured_path = path.clone();
                let outcome = cx
                    .background_executor()
                    .spawn(async move {
                        measure_track(&measured_path).map_err(|error| error.chain().to_string())
                    })
                    .await;
                let still_open = this
                    .update(cx, |this, cx| {
                        let Some(panel) = &mut this.prepare_panel else {
                            return false;
                        };
                        if let Some(track) =
                            panel.tracks.iter_mut().find(|track| track.path == path)
                        {
                            track.measurement = Some(outcome);
                        }
                        panel.refresh_rows();
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !still_open {
                    return;
                }
            }
        })
        .detach();
    }

    /// Closes the sheet, unless files are being written.
    pub(crate) fn close_prepare_files(&mut self, cx: &mut Context<'_, Self>) {
        if self
            .prepare_panel
            .as_ref()
            .is_some_and(PreparePanel::is_busy)
        {
            return;
        }
        self.fade_out_then(crate::surface_motion::Modal::Prepare, cx, |this, cx| {
            if this
                .prepare_panel
                .as_ref()
                .is_some_and(PreparePanel::is_busy)
            {
                return;
            }
            this.prepare_panel = None;
            cx.notify();
        });
    }

    /// Changes a choice and works the preview out again.
    pub(crate) fn change_prepare_options(
        &mut self,
        change: impl FnOnce(&mut PreparePanel),
        cx: &mut Context<'_, Self>,
    ) {
        let Some(panel) = &mut self.prepare_panel else {
            return;
        };
        if panel.is_busy() {
            return;
        }
        change(panel);
        panel.problem = None;
        panel.refresh_rows();
        cx.notify();
    }

    /// Asks for the folder the copies go to.
    pub(crate) fn choose_prepare_folder(&mut self, cx: &mut Context<'_, Self>) {
        let chosen = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = chosen.await
                && let Some(folder) = paths.into_iter().next()
            {
                this.update(cx, |this, cx| {
                    this.change_prepare_options(|panel| panel.output_folder = Some(folder), cx)
                })
                .ok();
            }
        })
        .detach();
    }

    /// Writes the copies the preview lists, off the main thread, with a
    /// progress bar, and ends with a toast that shows the first of them.
    pub(crate) fn confirm_prepare_files(&mut self, cx: &mut Context<'_, Self>) {
        let Some(panel) = &mut self.prepare_panel else {
            return;
        };
        if panel.is_busy() || panel.is_measuring() || panel.files_to_write() == 0 {
            return;
        }
        if let Some(folder) = &panel.output_folder
            && let Err(error) = std::fs::create_dir_all(folder)
        {
            panel.problem = Some(format!("Could not create {}: {error}", folder.display()));
            cx.notify();
            return;
        }
        let settings = panel.settings;
        let jobs: Vec<(PathBuf, TrackPlan, PathBuf)> = panel
            .rows
            .iter()
            .filter(|row| row.will_be_written())
            .filter_map(|row| Some((row.path.clone(), row.plan()?.clone(), row.output.clone()?)))
            .collect();
        let progress = PrepareProgress::default();
        panel.progress = progress.clone();
        panel.stage = PrepareStage::Writing;
        panel.problem = None;
        let running = self.begin_background_job("Preparing files", cx);
        let writer_progress = progress.clone();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { write_copies(jobs, &settings, &writer_progress) })
                .await;
            this.update(cx, |this, cx| {
                this.background_jobs.finish(running);
                this.finish_prepare_files(outcome, cx);
            })
            .ok();
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PROGRESS_INTERVAL).await;
                let writing = this.update(cx, |this, cx| {
                    cx.notify();
                    this.prepare_panel
                        .as_ref()
                        .is_some_and(PreparePanel::is_busy)
                });
                if !matches!(writing, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn finish_prepare_files(&mut self, outcome: WriteOutcome, cx: &mut Context<'_, Self>) {
        let WriteOutcome { written, failed } = outcome;
        if let Some(panel) = &mut self.prepare_panel {
            panel.stage = PrepareStage::Setup;
        }
        if let Some(first) = written.first().cloned() {
            let message = format!("Prepared {}", files(written.len()));
            self.show_toast(message, Some(ToastAction::Reveal(first)), cx);
            if !failed.is_empty() {
                self.notice = Some(format!(
                    "{} could not be prepared: {}",
                    files(failed.len()),
                    failed
                        .iter()
                        .map(|(path, reason)| format!(
                            "{}: {reason}",
                            path.file_name()
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        ))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            self.close_prepare_files(cx);
        } else if let Some(panel) = &mut self.prepare_panel {
            panel.problem = Some(match failed.first() {
                Some((_, reason)) => format!("Nothing was prepared: {reason}"),
                None => "Nothing was prepared.".to_string(),
            });
        }
        cx.notify();
    }
}

struct WriteOutcome {
    written: Vec<PathBuf>,
    failed: Vec<(PathBuf, String)>,
}

fn write_copies(
    jobs: Vec<(PathBuf, TrackPlan, PathBuf)>,
    settings: &PrepareSettings,
    progress: &PrepareProgress,
) -> WriteOutcome {
    let mut outcome = WriteOutcome {
        written: Vec::new(),
        failed: Vec::new(),
    };
    for (input, plan, output) in jobs {
        progress.current_thousandths.store(0, Ordering::Relaxed);
        let current = progress.current_thousandths.clone();
        let result = prepare_track(&input, &plan, &output, settings, &mut |fraction| {
            current.store(
                (fraction * PROGRESS_STEPS as f32) as usize,
                Ordering::Relaxed,
            );
        });
        match result {
            Ok(prepared) => outcome.written.push(prepared.output),
            Err(error) => outcome.failed.push((input, error.chain().to_string())),
        }
        progress.files_done.fetch_add(1, Ordering::Relaxed);
    }
    outcome
}

/// Where the sheet says copies go.
pub(crate) fn destination_text(folder: Option<&Path>) -> String {
    match folder {
        Some(folder) => folder.display().to_string(),
        None => "Next to each original".to_string(),
    }
}
