// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The library together with the analysis that fills it in. Nothing here
//! needs a window.

use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use crate::analysis::{AnalysisWorkers, FinishedFile};
use crate::library::Library;
use crate::progress::AnalysisProgress;

/// What was found under the files and folders the user added.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Scan {
    /// The paths that are folders.
    pub(crate) folders: Vec<PathBuf>,
    /// Every audio file found, sorted within each added path.
    pub(crate) files: Vec<PathBuf>,
    /// One message for each path that could not be read.
    pub(crate) problems: Vec<String>,
}

/// Asks the core for the audio files under each path. A path that cannot be
/// read is reported in [`Scan::problems`] and does not stop the others.
///
/// This walks folders on disk, so call it off the main thread.
pub(crate) fn scan(paths: &[PathBuf]) -> Scan {
    let mut scan = Scan::default();
    for path in paths {
        match euphoria_core::find_audio_files(std::slice::from_ref(path)) {
            Ok(files) => {
                if path.is_dir() {
                    scan.folders.push(path.clone());
                }
                scan.files.extend(files);
            }
            Err(error) => scan.problems.push(error.chain()),
        }
    }
    scan
}

/// The library and the background workers analyzing it.
pub(crate) struct Session {
    library: Library,
    progress: AnalysisProgress,
    workers: AnalysisWorkers,
    finished: Receiver<FinishedFile>,
}

impl Session {
    /// Starts with an empty library and idle workers.
    pub(crate) fn start() -> Result<Self, rayon::ThreadPoolBuildError> {
        let (workers, finished) = AnalysisWorkers::start()?;
        Ok(Self {
            library: Library::default(),
            progress: AnalysisProgress::default(),
            workers,
            finished,
        })
    }

    /// The tracks added so far.
    pub(crate) fn library(&self) -> &Library {
        &self.library
    }

    /// How far the current run of analysis has got.
    pub(crate) fn progress(&self) -> AnalysisProgress {
        self.progress
    }

    /// Adds scanned folders and files to the library and queues the files
    /// that were not in it yet. Returns how many files were queued.
    pub(crate) fn add(&mut self, scan: &Scan) -> usize {
        for folder in &scan.folders {
            self.library.add_folder(folder.clone());
        }
        let new_files = self.library.add_files(scan.files.clone());
        let queued = new_files.len();
        self.progress.enqueue(queued);
        self.workers.enqueue(new_files);
        queued
    }

    /// Moves every result the workers have finished into the library.
    /// Returns how many arrived.
    pub(crate) fn collect_finished(&mut self) -> usize {
        let mut collected = 0;
        while let Ok(file) = self.finished.try_recv() {
            self.library.record_outcome(&file.path, file.outcome);
            self.progress.finish_one();
            collected += 1;
        }
        collected
    }

    /// Skips the files still queued and waits up to `patience` for the ones
    /// being analyzed. Returns true when the workers are idle.
    pub(crate) fn stop(&self, patience: Duration) -> bool {
        self.workers.stop(patience)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::time::Instant;

    use super::*;
    use crate::library::TrackStatus;

    fn fixtures_folder() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../euphoria-core/tests/fixtures")
    }

    fn wait_until_idle(session: &mut Session) {
        let deadline = Instant::now() + Duration::from_secs(120);
        while session.progress().is_active() {
            assert!(Instant::now() < deadline, "analysis did not finish");
            session.collect_finished();
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn scanning_reports_unreadable_paths_and_keeps_the_rest() {
        let found = scan(&[PathBuf::from("/definitely/not/here"), fixtures_folder()]);
        assert_eq!(found.folders, vec![fixtures_folder()]);
        assert_eq!(found.files.len(), 7);
        assert_eq!(found.problems.len(), 1);
        assert!(found.problems[0].starts_with("cannot read /definitely/not/here"));
    }

    #[test]
    fn analyzes_the_fixture_folder_and_fills_in_every_track() {
        let mut session = Session::start().unwrap();
        let found = scan(&[fixtures_folder()]);

        assert_eq!(session.add(&found), 7);
        assert_eq!(session.library().folders(), [fixtures_folder()]);
        assert_eq!(session.library().counts().total, 7);
        assert!(session.progress().is_active());

        wait_until_idle(&mut session);

        for track in session.library().tracks() {
            match &track.status {
                TrackStatus::Analyzed(analysis) => {
                    assert!((analysis.duration_seconds - 1.0).abs() < 0.1, "{track:?}");
                }
                other => panic!("{} was not analyzed: {other:?}", track.file_name),
            }
        }
        assert!(session.stop(Duration::from_secs(30)));
    }

    #[test]
    fn adding_the_same_folder_again_queues_nothing() {
        let mut session = Session::start().unwrap();
        let found = scan(&[fixtures_folder()]);
        session.add(&found);
        assert_eq!(session.add(&found), 0);
        assert_eq!(session.library().folders().len(), 1);
        wait_until_idle(&mut session);
    }

    #[test]
    fn a_file_that_cannot_be_decoded_fails_with_a_reason() {
        let mut session = Session::start().unwrap();
        let found = scan(&[fixtures_folder().join("README.md")]);
        assert_eq!(session.add(&found), 1);
        wait_until_idle(&mut session);
        let reason = session.library().tracks()[0].failure().unwrap();
        assert!(reason.contains("README.md"), "{reason}");
        assert_eq!(session.library().counts().failed, 1);
    }
}
