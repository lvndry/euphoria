// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The steps of writing tags from the window: plan, preview, write after an
//! explicit confirmation, report, and undo. Writing is only reachable from
//! the preview's confirm button, so nothing is written unseen.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use euphoria_core::TrackAnalysis;
use euphoria_core::tags::WritePlan;
use gpui::Context;

use crate::app::EuphoriaApp;
use crate::tag_settings::TagSettings;
use crate::tag_writing::{
    LeftOut, PreviewRow, UndoReport, WriteReport, WriteTarget, plan_writes, preview_rows,
    undo_last, write_plans,
};

/// How often the progress of a write is shown.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Which tracks a write covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteScope {
    /// The tracks selected in the table.
    Selection,
    /// Every track the table shows: the folder on show, filtered by the
    /// search.
    Shown,
}

/// What a preview is for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreviewKind {
    /// Writing the analysis into the tracks `scope` covers.
    Analysis {
        /// The tracks the write covers.
        scope: WriteScope,
    },
    /// Hand edits of the selected tracks' fields.
    Edit {
        /// The edits, in a sentence.
        summary: String,
    },
}

/// What a write would change, shown before anything is written.
#[derive(Debug)]
pub(crate) struct Preview {
    /// What the write is.
    pub(crate) kind: PreviewKind,
    /// The settings the changes were worked out with.
    pub(crate) settings: TagSettings,
    /// The files that would change.
    pub(crate) plans: Arc<Vec<WritePlan>>,
    /// The analysis of each file, in the order of `plans`, recorded in the
    /// cache for the file written so it is not analyzed again. A hand edit
    /// of a track not analyzed yet has none.
    pub(crate) analyses: Arc<Vec<Option<TrackAnalysis>>>,
    /// The preview, a file at a time.
    pub(crate) rows: Vec<PreviewRow>,
    /// How many files already hold the values.
    pub(crate) up_to_date: usize,
    /// Tracks left out because they have no analysis.
    pub(crate) not_analyzed: Vec<LeftOut>,
    /// Files whose tags could not be read or written safely.
    pub(crate) failures: Vec<LeftOut>,
}

/// The tag panel over the window, at one step of a write or an undo.
#[derive(Debug)]
pub(crate) enum TagPanel {
    /// Working out the changes.
    Planning(WriteScope),
    /// Showing the changes and waiting for a confirmation.
    Preview(Preview),
    /// The editor of the selected tracks' fields.
    Editing(Box<crate::tag_editor::TagEditor>),
    /// Writing; the count of files done so far.
    Writing {
        /// Files done, written or failed.
        done: Arc<AtomicUsize>,
        /// Files to write.
        total: usize,
    },
    /// What the write did.
    Written(WriteReport),
    /// Asking before undoing the last write.
    ConfirmUndo,
    /// Undoing.
    Undoing,
    /// What the undo did, `None` when there was nothing to undo, or why it
    /// could not run.
    Undone(Result<Option<UndoReport>, String>),
}

impl TagPanel {
    /// True while files are being changed, when the panel cannot be closed.
    pub(crate) fn is_busy(&self) -> bool {
        matches!(self, TagPanel::Writing { .. } | TagPanel::Undoing)
    }
}

impl EuphoriaApp {
    /// Library indexes of the tracks selected in the table, in table order.
    pub(crate) fn selected_tracks(&self) -> Vec<usize> {
        self.visible
            .iter()
            .copied()
            .filter(|index| self.is_marked(*index))
            .collect()
    }

    /// Library indexes of the tracks `scope` covers.
    pub(crate) fn tracks_in(&self, scope: WriteScope) -> Vec<usize> {
        match scope {
            WriteScope::Selection => self.selected_tracks(),
            WriteScope::Shown => self.visible.clone(),
        }
    }

    /// Write tags: works out the changes for the selected tracks and shows
    /// them. Nothing is written until the preview is confirmed.
    pub(crate) fn open_tag_writer(&mut self, cx: &mut Context<'_, Self>) {
        if self.tag_panel.as_ref().is_some_and(TagPanel::is_busy) {
            return;
        }
        self.settings_open = false;
        let scope = if self.selected_tracks().is_empty() {
            WriteScope::Shown
        } else {
            WriteScope::Selection
        };
        self.plan_tag_write(scope, cx);
    }

    /// Works out the changes for the tracks `scope` covers, off the main
    /// thread, and shows them when ready.
    pub(crate) fn plan_tag_write(&mut self, scope: WriteScope, cx: &mut Context<'_, Self>) {
        if self.tag_panel.as_ref().is_some_and(TagPanel::is_busy) {
            return;
        }
        let library = self.session.library();
        let targets: Vec<WriteTarget> = self
            .tracks_in(scope)
            .into_iter()
            .filter_map(|index| library.track(index))
            .map(|track| WriteTarget {
                path: track.path.clone(),
                analysis: track.analysis().cloned(),
            })
            .collect();
        let settings = self.tag_settings;
        self.tag_task_generation += 1;
        let generation = self.tag_task_generation;
        self.tag_panel = Some(TagPanel::Planning(scope));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let planned = cx
                .background_executor()
                .spawn(async move { plan_writes(&targets, &settings.write_options()) })
                .await;
            this.update(cx, |this, cx| {
                if this.tag_task_generation != generation {
                    return;
                }
                this.tag_panel = Some(TagPanel::Preview(Preview {
                    kind: PreviewKind::Analysis { scope },
                    settings,
                    rows: preview_rows(&planned.plans),
                    plans: Arc::new(planned.plans),
                    analyses: Arc::new(planned.analyses.into_iter().map(Some).collect()),
                    up_to_date: planned.up_to_date,
                    not_analyzed: planned.not_analyzed,
                    failures: planned.failures,
                }));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The confirm button of the preview: writes the changes shown, through
    /// the shared write log, and reports each file.
    pub(crate) fn confirm_tag_write(&mut self, cx: &mut Context<'_, Self>) {
        let Some(TagPanel::Preview(preview)) = &self.tag_panel else {
            return;
        };
        if preview.plans.is_empty() {
            return;
        }
        let Some(log_directory) = self.write_log_directory.clone() else {
            self.report_problem(
                "Tags cannot be written: there is no data folder to keep their backups in.",
                cx,
            );
            return;
        };
        let plans = preview.plans.clone();
        let analyses = preview.analyses.clone();
        let is_edit = matches!(preview.kind, PreviewKind::Edit { .. });
        let cache = self.session.cache();
        let done = Arc::new(AtomicUsize::new(0));
        self.tag_panel = Some(TagPanel::Writing {
            done: done.clone(),
            total: plans.len(),
        });
        self.tag_task_generation += 1;
        self.follow_tag_progress(cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let report = cx
                .background_executor()
                .spawn(async move {
                    write_plans(&plans, &analyses, &log_directory, cache.as_deref(), &done)
                })
                .await;
            this.update(cx, |this, cx| {
                if is_edit {
                    // A title or an artist may have changed.
                    this.read_track_names(report.written.clone(), cx);
                }
                this.tag_panel = Some(TagPanel::Written(report));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Redraws a few times a second while files are written, so the count
    /// moves.
    fn follow_tag_progress(&mut self, cx: &mut Context<'_, Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(PROGRESS_INTERVAL).await;
                let still_writing = this.update(cx, |this, cx| {
                    cx.notify();
                    matches!(this.tag_panel, Some(TagPanel::Writing { .. }))
                });
                if !matches!(still_writing, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Undo last tag write: asks first.
    pub(crate) fn open_tag_undo(&mut self, cx: &mut Context<'_, Self>) {
        if self.tag_panel.as_ref().is_some_and(TagPanel::is_busy) {
            return;
        }
        self.settings_open = false;
        self.tag_task_generation += 1;
        self.tag_panel = Some(TagPanel::ConfirmUndo);
        cx.notify();
    }

    /// Puts back the tags the last write replaced, whether the app or the
    /// command line made it.
    pub(crate) fn confirm_tag_undo(&mut self, cx: &mut Context<'_, Self>) {
        if !matches!(self.tag_panel, Some(TagPanel::ConfirmUndo)) {
            return;
        }
        let Some(log_directory) = self.write_log_directory.clone() else {
            self.tag_panel = Some(TagPanel::Undone(Err(
                "there is no data folder to find the backups in".to_string(),
            )));
            cx.notify();
            return;
        };
        self.tag_panel = Some(TagPanel::Undoing);
        let cache = self.session.cache();
        cx.notify();
        cx.spawn(async move |this, cx| {
            let undone = cx
                .background_executor()
                .spawn(async move { undo_last(&log_directory, cache.as_deref()) })
                .await;
            this.update(cx, |this, cx| {
                this.tag_panel = Some(TagPanel::Undone(undone));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Closes the tag panel, unless files are being changed.
    pub(crate) fn close_tag_panel(&mut self, cx: &mut Context<'_, Self>) {
        if self.tag_panel.as_ref().is_some_and(TagPanel::is_busy) {
            return;
        }
        self.tag_task_generation += 1;
        self.tag_panel = None;
        cx.notify();
    }

    /// From the preview, opens the settings instead, so the write can be
    /// worked out again with other choices.
    pub(crate) fn change_tag_settings(&mut self, cx: &mut Context<'_, Self>) {
        if self.tag_panel.as_ref().is_some_and(TagPanel::is_busy) {
            return;
        }
        self.tag_task_generation += 1;
        self.tag_panel = None;
        self.settings_open = true;
        cx.notify();
    }

    fn report_problem(&mut self, problem: &str, cx: &mut Context<'_, Self>) {
        self.notice = Some(match self.notice.take() {
            Some(earlier) => format!("{earlier} {problem}"),
            None => problem.to_string(),
        });
        cx.notify();
    }

    /// The files a finished write changed, for tests.
    #[cfg(test)]
    pub(crate) fn written_files(&self) -> Option<Vec<std::path::PathBuf>> {
        match &self.tag_panel {
            Some(TagPanel::Written(report)) => Some(report.written.clone()),
            _ => None,
        }
    }

    /// How far a write has got, as files done and files in all, while it
    /// runs.
    pub(crate) fn tag_write_progress(&self) -> Option<(usize, usize)> {
        match &self.tag_panel {
            Some(TagPanel::Writing { done, total }) => {
                Some((done.load(Ordering::Relaxed).min(*total), *total))
            }
            _ => None,
        }
    }
}
