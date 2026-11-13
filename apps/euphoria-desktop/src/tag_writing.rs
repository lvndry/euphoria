// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Writing the analysis into the files' tags, in the steps the command line
//! takes: plan every file without touching it, show the plans, write them
//! through the core's write log once confirmed, and undo the last write
//! from that same log. The app and `euphoria tags undo` share the log, so
//! either can undo a write made by the other.
//!
//! A file written or put back is recorded in the analysis cache, so its
//! new bytes are not taken for a new track and analyzed again.
//!
//! Everything here reads or writes files, so it runs off the main thread.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::playlists::PlaylistRewrite;
use crate::state::PlaylistSaver;
use crate::tag_settings::TagSettings;
use euphoria_core::tags::{
    FieldChange, LoggedWrite, RewriteStage, SeratoMarkers, WriteLog, WritePlan, add_serato_markers,
    plan_write, undo_last_write_with_observer,
};
use euphoria_core::{CachedAnalyzer, Correction, Cue, TrackAnalysis};

struct RewriteUpdates<'a> {
    playlists: Option<&'a PlaylistSaver>,
    cache: Option<&'a CachedAnalyzer>,
    before: HashMap<PathBuf, Result<euphoria_core::ContentHash, String>>,
    rewrites: Vec<PlaylistRewrite>,
    problem: Option<String>,
}

impl<'a> RewriteUpdates<'a> {
    fn new(playlists: Option<&'a PlaylistSaver>, cache: Option<&'a CachedAnalyzer>) -> Self {
        Self {
            playlists,
            cache,
            before: HashMap::new(),
            rewrites: Vec::new(),
            problem: None,
        }
    }

    fn observe(&mut self, path: &Path, stage: RewriteStage) {
        if self.playlists.is_none() && self.cache.is_none() {
            return;
        }
        match stage {
            RewriteStage::Before => {
                self.before.insert(
                    path.to_path_buf(),
                    euphoria_core::ContentHash::of_file(path).map_err(|error| error.to_string()),
                );
            }
            RewriteStage::After => {
                let mut update = || -> Result<PlaylistRewrite, String> {
                    let before = self
                        .before
                        .remove(path)
                        .ok_or("the original file identity was not captured")??;
                    let after = euphoria_core::ContentHash::of_file(path)
                        .map_err(|error| error.to_string())?;
                    let path = std::path::absolute(path).map_err(|error| error.to_string())?;
                    let mut paths = HashMap::from([(path.clone(), path.clone())]);
                    if let Ok(canonical) = std::fs::canonicalize(&path) {
                        paths.insert(canonical.clone(), canonical);
                    }
                    let rewrite = PlaylistRewrite {
                        path,
                        before,
                        after,
                        paths,
                    };
                    Ok(rewrite)
                };
                match update() {
                    Ok(mut rewrite) => {
                        if let Some(cache) = self.cache
                            && let Err(error) =
                                cache.carry_rewritten_correction(rewrite.before, rewrite.after)
                        {
                            self.note_problem(error.chain());
                        }
                        if let Some(playlists) = self.playlists
                            && let Err(error) = playlists.rebind(&mut rewrite)
                        {
                            self.note_problem(error.chain());
                        }
                        self.rewrites.push(rewrite);
                    }
                    Err(error) => {
                        self.note_problem(error);
                    }
                }
            }
        }
    }

    fn note_problem(&mut self, error: String) {
        match &mut self.problem {
            Some(problem) => {
                problem.push_str("; ");
                problem.push_str(&error);
            }
            None => self.problem = Some(error),
        }
    }
}

/// A file and what it should say about itself, or `None` when it has no
/// analysis to write.
#[derive(Debug, Clone)]
pub(crate) struct WriteTarget {
    /// The audio file.
    pub(crate) path: PathBuf,
    /// The analysis found, which the cache keeps.
    pub(crate) analysis: Option<TrackAnalysis>,
    /// The key and tempo the user set, which are written in place of the
    /// detected ones.
    pub(crate) correction: Correction,
    /// The cues the user set, which are written as Serato cue points in
    /// place of those analysis placed, or `None`.
    pub(crate) edited_cues: Option<Vec<Cue>>,
}

/// A file left out of a write, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LeftOut {
    /// The audio file.
    pub(crate) path: PathBuf,
    /// Why, in a sentence.
    pub(crate) reason: String,
}

/// What writing would change, worked out without changing anything.
#[derive(Debug, Default)]
pub(crate) struct PlannedWrite {
    /// The files that would change, each with its changes.
    pub(crate) plans: Vec<WritePlan>,
    /// The analysis found for each of those files, in the same order. The
    /// cache records it for the written file, so a correction can still be
    /// reset to it afterwards.
    pub(crate) analyses: Vec<TrackAnalysis>,
    /// How many files already hold the values.
    pub(crate) up_to_date: usize,
    /// Files without an analysis, which there is nothing to write for.
    pub(crate) not_analyzed: Vec<LeftOut>,
    /// Files whose tags could not be read or could not be written safely.
    pub(crate) failures: Vec<LeftOut>,
    /// Files whose Serato cue points or beat grid do not all go where they
    /// were meant to, each with a note of one sentence. A file with several
    /// notes is listed once for each.
    pub(crate) serato_notes: Vec<LeftOut>,
}

/// Works out the changes for every target with `settings`. Reads the files,
/// writes none.
pub(crate) fn plan_writes(targets: &[WriteTarget], settings: &TagSettings) -> PlannedWrite {
    let options = settings.write_options();
    let mut planned = PlannedWrite::default();
    for target in targets {
        let Some(analysis) = &target.analysis else {
            planned.not_analyzed.push(LeftOut {
                path: target.path.clone(),
                reason: "It has not been analyzed.".to_string(),
            });
            continue;
        };
        let written = target.correction.apply(analysis.clone());
        let mut notes = Vec::new();
        let plan = plan_write(&target.path, &written, &options).and_then(|mut plan| {
            if settings.write_cue_markers {
                let markers = SeratoMarkers::of_cues(&written, target.edited_cues.clone());
                notes = add_serato_markers(&mut plan, &markers)?.notes();
            }
            Ok(plan)
        });
        if plan.is_ok() {
            planned
                .serato_notes
                .extend(notes.into_iter().map(|note| LeftOut {
                    path: target.path.clone(),
                    reason: note,
                }));
        }
        match plan {
            Ok(plan) if plan.is_empty() => planned.up_to_date += 1,
            Ok(plan) => {
                planned.plans.push(plan);
                planned.analyses.push(analysis.clone());
            }
            Err(error) => planned.failures.push(LeftOut {
                path: target.path.clone(),
                reason: error.chain(),
            }),
        }
    }
    planned
}

/// One line of the preview: a file, or one field of the file above it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PreviewRow {
    /// A file that would change.
    File {
        /// The audio file.
        path: PathBuf,
        /// How many of its fields would change.
        change_count: usize,
    },
    /// One field of the file above.
    Change(FieldChange),
}

/// The preview of `plans`, a file at a time, each followed by its changes.
pub(crate) fn preview_rows(plans: &[WritePlan]) -> Vec<PreviewRow> {
    let mut rows = Vec::new();
    for plan in plans {
        rows.push(PreviewRow::File {
            path: plan.path().to_path_buf(),
            change_count: plan.changes().len(),
        });
        rows.extend(plan.changes().iter().cloned().map(PreviewRow::Change));
    }
    rows
}

/// What a write did.
#[derive(Debug, Default)]
pub(crate) struct WriteReport {
    /// The files written.
    pub(crate) written: Vec<PathBuf>,
    /// The files that were not written, and why. They are as they were.
    pub(crate) failures: Vec<LeftOut>,
    /// Files written whose note in the log could not be saved. Their backup
    /// was saved first, so undo still restores them.
    pub(crate) not_noted: Vec<LeftOut>,
    /// Why the cache could not be told about the files written, which are
    /// then analyzed again the next time they are added.
    pub(crate) cache_problem: Option<String>,
    pub(crate) playlist_rewrites: Vec<PlaylistRewrite>,
    pub(crate) metadata_problem: Option<String>,
}

/// Writes `plans` through a new write log in `log_directory`, counting each
/// file done in `done` as it goes, and records `analyses`, in the same
/// order, in `cache` for each file written. A file that fails does not stop
/// the others.
#[cfg(test)]
pub(crate) fn write_plans(
    plans: &[WritePlan],
    analyses: &[Option<TrackAnalysis>],
    log_directory: &Path,
    cache: Option<&CachedAnalyzer>,
    done: &Arc<AtomicUsize>,
) -> WriteReport {
    write_plans_with_playlists(plans, analyses, log_directory, cache, done, None)
}

pub(crate) fn write_plans_with_playlists(
    plans: &[WritePlan],
    analyses: &[Option<TrackAnalysis>],
    log_directory: &Path,
    cache: Option<&CachedAnalyzer>,
    done: &Arc<AtomicUsize>,
    playlists: Option<&PlaylistSaver>,
) -> WriteReport {
    let mut log = WriteLog::new(log_directory);
    let mut report = WriteReport::default();
    let mut updates = RewriteUpdates::new(playlists, cache);
    for (plan, analysis) in plans.iter().zip(analyses) {
        let path = plan.path().to_path_buf();
        // The display range can irreversibly project a grid's phase. Keep
        // the canonical cached detector result while its old stamp matches.
        let canonical = cache
            .and_then(|cache| cache.stored_analysis(&path))
            .or_else(|| analysis.clone());
        let written = log.apply_with_observer(plan, |path, stage| updates.observe(path, stage));
        if written.is_ok()
            && report.cache_problem.is_none()
            && let (Some(cache), Some(analysis)) = (cache, &canonical)
            && let Err(error) = cache.record_retagged_file(&path, analysis)
        {
            report.cache_problem = Some(error.chain());
        }
        match written {
            Ok(LoggedWrite::Recorded) => report.written.push(path),
            Ok(LoggedWrite::NotRecorded(error)) => {
                report.written.push(path.clone());
                report.not_noted.push(LeftOut {
                    path,
                    reason: error.chain(),
                });
            }
            Err(error) => report.failures.push(LeftOut {
                path,
                reason: error.chain(),
            }),
        }
        done.fetch_add(1, Ordering::Relaxed);
    }
    report.playlist_rewrites = updates.rewrites;
    report.metadata_problem = updates.problem;
    report
}

/// What undoing the last write did.
#[derive(Debug, Default)]
pub(crate) struct UndoReport {
    /// The files that got their old tags back.
    pub(crate) restored: Vec<PathBuf>,
    /// Files whose original tags were already in place, including an undo
    /// that restored the bytes before reporting a synchronization error.
    pub(crate) untouched: usize,
    /// Files that could not be restored, and why. Undoing again tries them
    /// again.
    pub(crate) failures: Vec<LeftOut>,
    pub(crate) playlist_rewrites: Vec<PlaylistRewrite>,
    pub(crate) metadata_problem: Option<String>,
}

/// Undoes the last write logged in `log_directory`, by the app or by the
/// command line, and tells `cache` where the restored files are, since it
/// usually knows their old bytes. `Ok(None)` when there is no write to undo.
#[cfg(test)]
pub(crate) fn undo_last(
    log_directory: &Path,
    cache: Option<&CachedAnalyzer>,
) -> Result<Option<UndoReport>, String> {
    undo_last_with_playlists(log_directory, cache, None)
}

pub(crate) fn undo_last_with_playlists(
    log_directory: &Path,
    cache: Option<&CachedAnalyzer>,
    playlists: Option<&PlaylistSaver>,
) -> Result<Option<UndoReport>, String> {
    let mut updates = RewriteUpdates::new(playlists, cache);
    let undone =
        undo_last_write_with_observer(log_directory, |path, stage| updates.observe(path, stage))
            .map_err(|error| error.chain())?;
    if let (Some(cache), Some(undone)) = (cache, &undone) {
        for path in &undone.restored {
            if cache.recognise_file(path).is_err() {
                break;
            }
        }
    }
    Ok(undone.map(|undone| UndoReport {
        playlist_rewrites: updates.rewrites,
        metadata_problem: updates.problem,
        restored: undone.restored,
        untouched: undone.untouched.len(),
        failures: undone
            .failed
            .into_iter()
            .map(|(path, error)| LeftOut {
                path,
                reason: error.chain(),
            })
            .collect(),
    }))
}

#[cfg(test)]
pub(crate) mod testing {
    //! Scratch copies of real audio files for tests that write tags.

    use std::path::{Path, PathBuf};

    use euphoria_core::{Key, TrackAnalysis};

    /// A folder under the system's temporary folder, deleted when dropped.
    pub(crate) struct ScratchFolder(pub(crate) PathBuf);

    impl ScratchFolder {
        /// An empty folder whose name includes `name` and the process id.
        pub(crate) fn new(name: &str) -> Self {
            let folder = std::env::temp_dir().join(format!(
                "euphoria-desktop-tags-{name}-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&folder);
            std::fs::create_dir_all(&folder).unwrap();
            Self(folder)
        }

        /// Copies the core's test files of every format that takes tags into
        /// the folder `music` inside this one and returns the copies.
        pub(crate) fn copy_fixtures(&self) -> Vec<PathBuf> {
            let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../crates/euphoria-core/tests/fixtures");
            let music = self.0.join("music");
            std::fs::create_dir_all(&music).unwrap();
            [
                "sine-440hz-mono.mp3",
                "sine-440hz-mono.flac",
                "sine-440hz-stereo.wav",
            ]
            .into_iter()
            .map(|name| {
                let copy = music.join(name);
                std::fs::copy(fixtures.join(name), &copy).unwrap();
                copy
            })
            .collect()
        }

        /// Where write logs go in tests.
        pub(crate) fn log_directory(&self) -> PathBuf {
            self.0.join("tag-writes")
        }
    }

    impl Drop for ScratchFolder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// An analysis with a key, a tempo and an energy rating to write.
    pub(crate) fn analysis() -> TrackAnalysis {
        TrackAnalysis {
            duration_seconds: 1.0,
            key: Some(Key::new(9, euphoria_core::Mode::Minor)),
            bpm: Some(124.0),
            energy: Some(7),
            tuning_cents: None,
            key_runner_up: None,
            key_score_margin: None,
            tempo_estimate: None,
            tempo_score_margin: None,
            beat_grid: None,
            cues: Vec::new(),
            key_timeline: Vec::new(),
            quality: None,
        }
    }

    /// The bytes of each file, to compare after an undo.
    pub(crate) fn contents(paths: &[PathBuf]) -> Vec<Vec<u8>> {
        paths
            .iter()
            .map(|path| std::fs::read(path).unwrap())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use euphoria_core::Cache;
    #[test]
    fn a_disposable_cache_failure_does_not_prevent_playlist_identity_updates() {
        let scratch = ScratchFolder::new("playlist-cache-failure");
        let files = scratch.copy_fixtures();
        let path = &files[0];
        let before = euphoria_core::ContentHash::of_file(path).unwrap();
        let cache_path = scratch.0.join("analysis-cache.sqlite3");
        let mut cache = Cache::open(&cache_path).unwrap();
        cache.record_retagged_file(path, &analysis()).unwrap();
        let cache = CachedAnalyzer::new(cache).unwrap();
        let store_path = scratch.0.join("playlists.sqlite3");
        let mut store = euphoria_core::PlaylistStore::open(&store_path).unwrap();
        store
            .save(&[euphoria_core::StoredPlaylist {
                name: "Set".into(),
                label: None,
                entries: vec![euphoria_core::PlaylistEntry {
                    path: path.clone(),
                    content: before,
                }],
            }])
            .unwrap();
        let mut session = crate::state::Session::start(None).unwrap();
        session.use_playlist_store(store).unwrap();
        let saver = session.playlist_saver().unwrap();
        saver.register(session.playlists(), session.favorites());
        rusqlite::Connection::open(&cache_path)
            .unwrap()
            .execute("DROP TABLE corrections", [])
            .unwrap();
        let plan = euphoria_core::tags::plan_edit(
            path,
            &[euphoria_core::tags::FieldEdit {
                field: euphoria_core::tags::TagField::Genre,
                change: euphoria_core::tags::TextChange::Set("Deep House".into()),
            }],
        )
        .unwrap();
        let report = write_plans_with_playlists(
            &[plan],
            &[None],
            &scratch.log_directory(),
            Some(&cache),
            &Arc::new(AtomicUsize::new(0)),
            Some(&saver),
        );
        assert_eq!(report.written.len(), 1);
        assert!(report.metadata_problem.is_some());
        let kept = euphoria_core::PlaylistStore::open(&store_path)
            .unwrap()
            .load()
            .unwrap();
        assert_eq!(
            kept[0].entries[0].content,
            euphoria_core::ContentHash::of_file(path).unwrap()
        );
        assert_ne!(kept[0].entries[0].content, before);
    }

    #[test]
    fn a_tag_write_keeps_the_canonical_cached_grid_instead_of_its_display_projection() {
        let scratch = ScratchFolder::new("canonical-grid");
        let files = scratch.copy_fixtures();
        let path = &files[0];
        let mut canonical = analysis();
        canonical.bpm = Some(160.0);
        canonical.tempo_estimate = Some(80.0);
        canonical.beat_grid = Some(euphoria_core::detect::BeatGrid {
            bpm: 160.0,
            first_downbeat_seconds: 0.9,
            salience: 2.0,
            drift_seconds: None,
        });
        let mut cache = Cache::open(&scratch.0.join("analysis-cache.sqlite3")).unwrap();
        cache.record_retagged_file(path, &canonical).unwrap();
        let cache = CachedAnalyzer::new(cache).unwrap();
        let projected = canonical
            .clone()
            .with_tempo_range(euphoria_core::TempoRange::new(200.0, 400.0).unwrap());
        assert_ne!(projected.beat_grid, canonical.beat_grid);
        let target = WriteTarget {
            path: path.clone(),
            analysis: Some(projected),
            correction: Correction::default(),
            edited_cues: None,
        };
        let planned = plan_writes(&[target], &TagSettings::default());
        let report = write_plans(
            &planned.plans,
            &planned.analyses.into_iter().map(Some).collect::<Vec<_>>(),
            &scratch.log_directory(),
            Some(&cache),
            &Arc::new(AtomicUsize::new(0)),
        );
        assert!(report.cache_problem.is_none());
        let reopened = launch_cache(&scratch);
        assert_eq!(
            reopened.stored_analysis(path).unwrap().beat_grid,
            canonical.beat_grid
        );
    }

    use super::testing::{ScratchFolder, analysis, contents};
    use super::*;
    use crate::tag_settings::{ProgramChoice, TagSettings};

    fn targets(paths: &[PathBuf]) -> Vec<WriteTarget> {
        paths
            .iter()
            .map(|path| WriteTarget {
                path: path.clone(),
                analysis: Some(analysis()),
                correction: Correction::default(),
                edited_cues: None,
            })
            .collect()
    }

    #[test]
    fn planning_reads_and_never_writes() {
        let scratch = ScratchFolder::new("plan");
        let files = scratch.copy_fixtures();
        let before = contents(&files);
        let planned = plan_writes(&targets(&files), &TagSettings::default());
        assert_eq!(contents(&files), before);
        assert_eq!(planned.plans.len(), files.len());
        assert!(planned.failures.is_empty(), "{:?}", planned.failures);
        assert!(!scratch.log_directory().exists());
    }

    #[test]
    fn the_preview_lists_each_file_then_each_field_with_old_and_new_values() {
        let scratch = ScratchFolder::new("preview");
        let files = scratch.copy_fixtures();
        let planned = plan_writes(&targets(&files[..1]), &TagSettings::default());
        let rows = preview_rows(&planned.plans);
        assert_eq!(
            rows[0],
            PreviewRow::File {
                path: files[0].clone(),
                change_count: 3
            }
        );
        let changes: Vec<(String, Option<String>, Option<String>)> = rows[1..]
            .iter()
            .map(|row| match row {
                PreviewRow::Change(change) => (
                    change.field.clone(),
                    change.current.clone(),
                    change.new.clone(),
                ),
                PreviewRow::File { .. } => panic!("a file inside the changes"),
            })
            .collect();
        assert_eq!(
            changes,
            [
                ("TKEY".to_string(), None, Some("8A".to_string())),
                ("TBPM".to_string(), None, Some("124".to_string())),
                ("COMM".to_string(), None, Some("8A - Energy 7".to_string())),
            ]
        );
    }

    #[test]
    fn tracks_without_an_analysis_are_left_out_and_said_so() {
        let scratch = ScratchFolder::new("not-analyzed");
        let files = scratch.copy_fixtures();
        let mut targets = targets(&files);
        targets[1].analysis = None;
        let planned = plan_writes(&targets, &TagSettings::default());
        assert_eq!(planned.plans.len(), 2);
        assert_eq!(planned.not_analyzed.len(), 1);
        assert_eq!(planned.not_analyzed[0].path, files[1]);
    }

    #[test]
    fn a_missing_file_is_a_failure_that_does_not_stop_the_others() {
        let scratch = ScratchFolder::new("missing");
        let mut files = scratch.copy_fixtures();
        files.push(scratch.0.join("music/gone.mp3"));
        let planned = plan_writes(&targets(&files), &TagSettings::default());
        assert_eq!(planned.plans.len(), 3);
        assert_eq!(planned.failures.len(), 1);
        assert!(!planned.failures[0].reason.is_empty());
    }

    #[test]
    fn writing_then_undoing_returns_every_file_to_its_exact_bytes() {
        let scratch = ScratchFolder::new("write-undo");
        let files = scratch.copy_fixtures();
        let before = contents(&files);
        let mut settings = TagSettings::default();
        settings.choose_program(ProgramChoice::Rekordbox);
        let planned = plan_writes(&targets(&files), &settings);

        let done = Arc::new(AtomicUsize::new(0));
        let report = write_plans(
            &planned.plans,
            &planned
                .analyses
                .iter()
                .cloned()
                .map(Some)
                .collect::<Vec<_>>(),
            &scratch.log_directory(),
            None,
            &done,
        );
        assert_eq!(report.written, files);
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert_eq!(done.load(Ordering::Relaxed), files.len());
        assert_ne!(contents(&files), before);
        let replanned = plan_writes(&targets(&files), &settings);
        assert_eq!(replanned.up_to_date, files.len());

        let undone = undo_last(&scratch.log_directory(), None).unwrap().unwrap();
        assert_eq!(undone.restored.len(), files.len());
        assert!(undone.failures.is_empty(), "{:?}", undone.failures);
        assert_eq!(contents(&files), before);
        assert!(undo_last(&scratch.log_directory(), None).unwrap().is_none());
    }

    #[test]
    fn a_file_changed_after_planning_is_not_written_and_the_rest_are() {
        let scratch = ScratchFolder::new("changed");
        let files = scratch.copy_fixtures();
        let planned = plan_writes(&targets(&files), &TagSettings::default());
        let other_settings = TagSettings {
            write_tempo: false,
            ..TagSettings::default()
        };
        let meanwhile = plan_writes(&targets(&files[..1]), &other_settings);
        euphoria_core::tags::apply_plan(&meanwhile.plans[0]).unwrap();

        let done = Arc::new(AtomicUsize::new(0));
        let report = write_plans(
            &planned.plans,
            &planned
                .analyses
                .iter()
                .cloned()
                .map(Some)
                .collect::<Vec<_>>(),
            &scratch.log_directory(),
            None,
            &done,
        );
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].path, files[0]);
        assert_eq!(report.written, files[1..]);
    }

    /// The analyzer over a cache in the scratch folder, as a new launch of
    /// the app opens it.
    fn launch_cache(scratch: &ScratchFolder) -> CachedAnalyzer {
        let cache = Cache::open(&scratch.0.join("analysis-cache.sqlite3")).unwrap();
        CachedAnalyzer::new(cache).unwrap()
    }

    #[test]
    fn written_and_restored_files_are_served_from_the_cache_on_the_next_launch() {
        let scratch = ScratchFolder::new("cache");
        let files = scratch.copy_fixtures();
        let first_launch = launch_cache(&scratch);
        let targets: Vec<WriteTarget> = files
            .iter()
            .map(|path| WriteTarget {
                path: path.clone(),
                analysis: Some(first_launch.analyze_file(path).unwrap().analysis),
                correction: Correction::default(),
                edited_cues: None,
            })
            .collect();
        let planned = plan_writes(&targets, &TagSettings::default());
        let done = Arc::new(AtomicUsize::new(0));
        let report = write_plans(
            &planned.plans,
            &planned
                .analyses
                .iter()
                .cloned()
                .map(Some)
                .collect::<Vec<_>>(),
            &scratch.log_directory(),
            Some(&first_launch),
            &done,
        );
        assert_eq!(report.written, files);
        assert_eq!(report.cache_problem, None);

        let after_write = launch_cache(&scratch);
        for target in &targets {
            assert_eq!(
                after_write.stored_analysis(&target.path),
                target.analysis,
                "{}",
                target.path.display()
            );
            assert!(after_write.analyze_file(&target.path).unwrap().from_cache);
        }

        let undone = undo_last(&scratch.log_directory(), Some(&after_write))
            .unwrap()
            .unwrap();
        assert_eq!(undone.restored.len(), files.len());
        let after_undo = launch_cache(&scratch);
        for target in &targets {
            assert_eq!(after_undo.stored_analysis(&target.path), target.analysis);
            assert!(after_undo.analyze_file(&target.path).unwrap().from_cache);
        }
    }

    #[test]
    fn with_nothing_written_there_is_nothing_to_undo() {
        let scratch = ScratchFolder::new("nothing");
        assert!(undo_last(&scratch.log_directory(), None).unwrap().is_none());
    }
}
