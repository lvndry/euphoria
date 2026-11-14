// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! What the app is busy with, for the logo in the toolbar. Every long action
//! is listed in `EuphoriaApp::busy_work`, which says what is running in the
//! words the logo's tooltip shows.

use gpui::Context;

use crate::app::EuphoriaApp;
use crate::library_export::ExportStage;
use crate::tag_flow::TagPanel;

/// Background jobs that have no state of their own to ask, such as an
/// export running on a thread: each says what it is doing while it runs.
#[derive(Debug, Default)]
pub(crate) struct BackgroundJobs {
    next_id: u64,
    running: Vec<(u64, &'static str)>,
}

impl BackgroundJobs {
    /// Records that a job is running and returns the id to finish it with.
    pub(crate) fn begin(&mut self, description: &'static str) -> u64 {
        self.next_id += 1;
        self.running.push((self.next_id, description));
        self.next_id
    }

    /// Records that the job `id` is over, however it ended.
    pub(crate) fn finish(&mut self, id: u64) {
        self.running.retain(|(running, _)| *running != id);
    }

    fn descriptions(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.running.iter().map(|(_, description)| *description)
    }
}

impl EuphoriaApp {
    /// What is running right now, one phrase for each kind of work. To make
    /// another kind of work show in the logo, add it here.
    pub(crate) fn busy_work(&self) -> Vec<String> {
        let mut work = Vec::new();
        let progress = self.session.progress();
        if let Some(label) = progress.counts_label() {
            work.push(label);
        }
        if self.scans_in_progress > 0 {
            work.push("Looking for audio files".to_string());
        }
        match &self.tag_panel {
            Some(TagPanel::Writing { .. }) => work.push("Writing tags".to_string()),
            Some(TagPanel::Undoing) => work.push("Undoing the tag write".to_string()),
            _ => {}
        }
        if self.auto_tags.is_writing() {
            work.push("Writing tags".to_string());
        }
        if let Some(panel) = &self.library_export
            && let ExportStage::Working { writing } = panel.stage
        {
            let program = panel.setup.program.label();
            work.push(if writing {
                format!("Exporting to {program}")
            } else {
                format!("Checking the export to {program}")
            });
        }
        if self.stems.progress().is_some() {
            work.push("Separating stems".to_string());
        }
        if self.decks.stem_library.is_scanning() {
            work.push("Looking for stems".to_string());
        }
        if self.set_builder.is_building {
            work.push("Building the set".to_string());
        }
        work.extend(self.background_jobs.descriptions().map(ToString::to_string));
        work
    }

    /// True while any long action runs.
    pub(crate) fn is_busy(&self) -> bool {
        !self.busy_work().is_empty()
    }

    /// The logo's tooltip while work runs, or `None` when idle.
    pub(crate) fn busy_description(&self) -> Option<String> {
        self.is_busy().then(|| self.busy_work().join("\n"))
    }

    /// Records a job that runs on a thread until `finish_background_job`.
    pub(crate) fn begin_background_job(
        &mut self,
        description: &'static str,
        cx: &mut Context<'_, Self>,
    ) -> u64 {
        let id = self.background_jobs.begin(description);
        cx.notify();
        id
    }
}
