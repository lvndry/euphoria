// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Writing key and tempo into the files as tracks finish analysis, when the
//! setting is on. The planning and writing are those of Write tags, so its
//! safe defaults all apply and the write is logged for Undo. Writes are
//! batched: a batch goes out when 25 tracks wait, when the oldest has waited
//! 10 seconds, or when analysis ends. That is a few writes a minute during a
//! long analysis, not one a track, and no track waits long once analysis is
//! over. Nothing is written while a DJ program runs: the tracks are held and
//! a strip offers to write them anyway.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::time::{Duration, Instant};

use gpui::Context;

use crate::app::EuphoriaApp;
use crate::tag_flow::{TagPanel, plan_verified_writes};
use crate::tag_writing::{LeftOut, write_plans_with_playlists};

/// How many waiting tracks make a batch go out at once.
const BATCH_SIZE: usize = 25;
/// How long the oldest waiting track waits for others before its batch goes
/// out.
const BATCH_INTERVAL: Duration = Duration::from_secs(10);
/// How often waiting tracks are checked against the two limits above.
const CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// The tracks waiting to be tagged after analysis, and what the run so far
/// did.
#[derive(Debug, Default)]
pub(crate) struct AutoTags {
    pending: Vec<PathBuf>,
    queued: HashSet<PathBuf>,
    waiting_since: Option<Instant>,
    is_writing: bool,
    is_following: bool,
    held_by: Vec<&'static str>,
    failures: Vec<LeftOut>,
}

impl AutoTags {
    /// True while a batch is being planned and written.
    pub(crate) fn is_writing(&self) -> bool {
        self.is_writing
    }

    /// The DJ programs the waiting tracks are held for, when held.
    pub(crate) fn held_by(&self) -> &[&'static str] {
        &self.held_by
    }

    /// How many tracks wait to be tagged.
    pub(crate) fn pending_count(&self) -> usize {
        self.pending.len()
    }

    fn clear_pending(&mut self) {
        self.pending.clear();
        self.queued.clear();
        self.waiting_since = None;
        self.held_by.clear();
    }
}

/// Whether the waiting tracks should be written now.
fn is_due(waiting: usize, waited: Duration, is_analyzing: bool) -> bool {
    waiting > 0 && (waiting >= BATCH_SIZE || waited >= BATCH_INTERVAL || !is_analyzing)
}

/// The strip's sentence about tracks held for a running program.
pub(crate) fn held_sentence(count: usize, programs: &[&str]) -> String {
    let tracks = if count == 1 {
        "1 track has".to_string()
    } else {
        format!("{} tracks have", crate::format::count(count))
    };
    let programs = programs.join(" and ");
    let (verb, pronoun) = if programs.contains(" and ") {
        ("are", "them")
    } else {
        ("is", "it")
    };
    format!(
        "{tracks} a key and tempo to write into the file. Waiting because {programs} {verb} \
         open: close {pronoun} and choose Write tags, or write now."
    )
}

fn failure_sentence(failures: &[LeftOut]) -> String {
    let first = &failures[0];
    let name = first
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| first.path.display().to_string());
    let tracks = if failures.len() == 1 {
        "1 track".to_string()
    } else {
        format!("{} tracks", crate::format::count(failures.len()))
    };
    format!(
        "Key and tempo could not be written to {tracks}, such as {name}: {} Choose Write tags \
         to see each one.",
        first.reason
    )
}

impl EuphoriaApp {
    /// Takes the files analysis has just finished from scratch, and queues
    /// them for tags when the setting is on.
    pub(crate) fn queue_fresh_analyses(&mut self, cx: &mut Context<'_, Self>) {
        let fresh = self.session.take_fresh_analyses();
        self.queue_auto_tags(fresh, cx);
    }

    /// Queues tracks whose analysis is new or changed, such as a corrected
    /// one, to have their tags written with the next batch. Does nothing
    /// while the setting is off.
    pub(crate) fn queue_auto_tags(&mut self, paths: Vec<PathBuf>, cx: &mut Context<'_, Self>) {
        if !self.tag_settings.write_after_analysis || paths.is_empty() {
            return;
        }
        let auto = &mut self.auto_tags;
        for path in paths {
            if auto.queued.insert(path.clone()) {
                auto.pending.push(path);
            }
        }
        auto.waiting_since.get_or_insert_with(Instant::now);
        self.follow_auto_tags(cx);
        self.flush_due_auto_tags(cx);
        cx.notify();
    }

    /// Checks the waiting tracks against the batch limits once a second for
    /// as long as there is anything to do.
    fn follow_auto_tags(&mut self, cx: &mut Context<'_, Self>) {
        if self.auto_tags.is_following {
            return;
        }
        self.auto_tags.is_following = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CHECK_INTERVAL).await;
                let more = this.update(cx, |this, cx| {
                    this.flush_due_auto_tags(cx);
                    let auto = &this.auto_tags;
                    let has_work = (!auto.pending.is_empty() && auto.held_by.is_empty())
                        || auto.is_writing
                        || !auto.failures.is_empty();
                    this.auto_tags.is_following = has_work;
                    has_work
                });
                if !matches!(more, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Writes the waiting tracks when a batch is due, and otherwise reports
    /// what a finished run could not write.
    pub(crate) fn flush_due_auto_tags(&mut self, cx: &mut Context<'_, Self>) {
        if self.auto_tags.is_writing || !self.auto_tags.held_by.is_empty() {
            return;
        }
        let is_analyzing = self.session.progress().is_active();
        if self.auto_tags.pending.is_empty() {
            self.report_auto_tag_failures(is_analyzing, cx);
            return;
        }
        if self.tag_panel.as_ref().is_some_and(TagPanel::is_busy) {
            return;
        }
        let waited = self
            .auto_tags
            .waiting_since
            .map(|since| since.elapsed())
            .unwrap_or_default();
        if is_due(self.auto_tags.pending.len(), waited, is_analyzing) {
            self.write_auto_tags(false, cx);
        }
    }

    /// One notice for everything a run could not write, once it is over.
    fn report_auto_tag_failures(&mut self, is_analyzing: bool, cx: &mut Context<'_, Self>) {
        if is_analyzing || self.auto_tags.failures.is_empty() {
            return;
        }
        let failures = std::mem::take(&mut self.auto_tags.failures);
        self.report(failure_sentence(&failures));
        cx.notify();
    }

    /// "Write now" of the strip: writes the held tracks although a DJ
    /// program runs.
    pub(crate) fn write_held_tags_now(&mut self, cx: &mut Context<'_, Self>) {
        if self.auto_tags.is_writing || self.tag_panel.as_ref().is_some_and(TagPanel::is_busy) {
            return;
        }
        self.write_auto_tags(true, cx);
    }

    /// Dismisses the strip and gives up on tagging the held tracks, which
    /// Write tags can still do.
    pub(crate) fn dismiss_held_tags(&mut self, cx: &mut Context<'_, Self>) {
        self.auto_tags.clear_pending();
        cx.notify();
    }

    /// Ticks or unticks writing key and tempo after analysis. Turning it off
    /// drops the tracks still waiting.
    pub(crate) fn toggle_write_after_analysis(&mut self, cx: &mut Context<'_, Self>) {
        self.tag_settings.write_after_analysis = !self.tag_settings.write_after_analysis;
        if !self.tag_settings.write_after_analysis {
            self.auto_tags.clear_pending();
        }
        self.remember_tag_settings(cx);
    }

    /// Plans and writes the waiting tracks as one batch, with the planning
    /// and writing of Write tags. Unless `ignore_programs`, a running DJ
    /// program holds the batch instead.
    fn write_auto_tags(&mut self, ignore_programs: bool, cx: &mut Context<'_, Self>) {
        if !ignore_programs {
            let running = self.program_checks.running();
            if !running.is_empty() {
                self.auto_tags.held_by = running;
                cx.notify();
                return;
            }
        }
        self.auto_tags.held_by.clear();
        let Some(log_directory) = self.write_log_directory.clone() else {
            self.auto_tags.clear_pending();
            self.report(
                "Tags cannot be written: there is no data folder to keep their backups in."
                    .to_string(),
            );
            cx.notify();
            return;
        };
        let paths = std::mem::take(&mut self.auto_tags.pending);
        self.auto_tags.queued.clear();
        self.auto_tags.waiting_since = None;
        let targets = self.write_targets(&paths);
        let settings = self.tag_settings;
        let cache = self.session.cache();
        let writer = self.by_ear.correction_writer.clone();
        let playlists = self.session.playlist_saver();
        if let Some(saver) = &playlists {
            saver.register(self.session.playlists(), self.session.favorites());
        }
        self.auto_tags.is_writing = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (report, mut failures) = cx
                .background_executor()
                .spawn(async move {
                    let planned = plan_verified_writes(
                        targets,
                        &settings,
                        cache.as_deref(),
                        &writer,
                        Some(&log_directory),
                    );
                    let analyses: Vec<_> = planned.analyses.into_iter().map(Some).collect();
                    let report = write_plans_with_playlists(
                        &planned.plans,
                        &analyses,
                        &log_directory,
                        cache.as_deref(),
                        &Arc::new(AtomicUsize::new(0)),
                        playlists.as_ref(),
                    );
                    (report, planned.failures)
                })
                .await;
            this.update(cx, |this, cx| {
                failures.extend(report.failures.iter().map(|failure| LeftOut {
                    path: failure.path.clone(),
                    reason: failure.reason.clone(),
                }));
                this.follow_written_playlists(&report.playlist_rewrites);
                if !report.written.is_empty() {
                    this.note_first_write();
                }
                this.read_track_names(report.written.clone(), cx);
                #[cfg(test)]
                {
                    this.last_written = report.written.clone();
                }
                if !report.written.is_empty() {
                    this.show_toast(
                        crate::tag_flow::wrote_sentence(&settings, report.written.len()),
                        Some(crate::toast::ToastAction::UndoTagWrite),
                        cx,
                    );
                }
                this.auto_tags.failures.extend(failures);
                this.auto_tags.is_writing = false;
                this.flush_due_auto_tags(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batch_is_due_at_the_size_the_age_or_the_end_of_analysis() {
        let soon = Duration::from_secs(1);
        assert!(!is_due(0, BATCH_INTERVAL, false), "nothing waits");
        assert!(!is_due(3, soon, true), "a few, recently, mid-analysis");
        assert!(is_due(BATCH_SIZE, soon, true), "enough tracks");
        assert!(is_due(3, BATCH_INTERVAL, true), "the oldest has waited");
        assert!(is_due(1, soon, false), "analysis ended: the final flush");
    }

    #[test]
    fn the_held_sentence_names_the_count_and_the_programs() {
        assert_eq!(
            held_sentence(1, &["rekordbox"]),
            "1 track has a key and tempo to write into the file. Waiting because rekordbox is \
             open: close it and choose Write tags, or write now."
        );
        assert_eq!(
            held_sentence(12, &["rekordbox", "Traktor"]),
            "12 tracks have a key and tempo to write into the file. Waiting because rekordbox and \
             Traktor are open: close them and choose Write tags, or write now."
        );
    }
}
