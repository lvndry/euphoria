// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The three steps of a first run: add music, analyze it, fill in tags. What
//! has finished is kept in the settings, and the card that shows the steps
//! goes away for good once the first analysis and the first write are done.

use serde::{Deserialize, Serialize};

/// What the first run has got through so far.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct FirstRun {
    /// The first analysis ran to its end.
    pub(crate) analysis_done: bool,
    /// The first write of tags finished.
    pub(crate) write_done: bool,
}

impl FirstRun {
    /// For someone who was using Euphoria before the first run card existed.
    pub(crate) fn already_done() -> Self {
        Self {
            analysis_done: true,
            write_done: true,
        }
    }

    /// What to start from when the settings file holds nothing about the
    /// first run: a library that already has folders means the person is
    /// past it.
    pub(crate) fn from_stored(stored: Option<Self>, has_folders: bool) -> Self {
        match stored {
            Some(first_run) => first_run,
            None if has_folders => Self::already_done(),
            None => Self::default(),
        }
    }

    /// True once both steps are done. The card never comes back after this.
    pub(crate) fn is_done(&self) -> bool {
        self.analysis_done && self.write_done
    }
}

/// Where one step stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StepState {
    /// Waiting for the steps before it.
    Later,
    /// The step the person is on.
    Now,
    /// Finished.
    Done,
}

/// The three steps, for the state of the library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Steps {
    pub(crate) add_music: StepState,
    pub(crate) analyze: StepState,
    pub(crate) fill_in_tags: StepState,
}

impl Steps {
    pub(crate) fn of(first_run: FirstRun, has_music: bool) -> Self {
        let add_music = if has_music {
            StepState::Done
        } else {
            StepState::Now
        };
        let analyze = if first_run.analysis_done {
            StepState::Done
        } else if has_music {
            StepState::Now
        } else {
            StepState::Later
        };
        let fill_in_tags = if first_run.write_done {
            StepState::Done
        } else if first_run.analysis_done {
            StepState::Now
        } else {
            StepState::Later
        };
        Self {
            add_music,
            analyze,
            fill_in_tags,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_steps_advance_with_the_state() {
        let fresh = FirstRun::default();
        assert_eq!(
            Steps::of(fresh, false),
            Steps {
                add_music: StepState::Now,
                analyze: StepState::Later,
                fill_in_tags: StepState::Later,
            }
        );
        assert_eq!(
            Steps::of(fresh, true),
            Steps {
                add_music: StepState::Done,
                analyze: StepState::Now,
                fill_in_tags: StepState::Later,
            }
        );
        let analyzed = FirstRun {
            analysis_done: true,
            write_done: false,
        };
        assert_eq!(
            Steps::of(analyzed, true),
            Steps {
                add_music: StepState::Done,
                analyze: StepState::Done,
                fill_in_tags: StepState::Now,
            }
        );
        assert_eq!(
            Steps::of(FirstRun::already_done(), true),
            Steps {
                add_music: StepState::Done,
                analyze: StepState::Done,
                fill_in_tags: StepState::Done,
            }
        );
    }

    #[test]
    fn it_is_done_only_when_both_the_analysis_and_a_write_are() {
        assert!(!FirstRun::default().is_done());
        assert!(
            !FirstRun {
                analysis_done: true,
                write_done: false
            }
            .is_done()
        );
        assert!(FirstRun::already_done().is_done());
    }

    #[test]
    fn someone_with_folders_and_no_stored_first_run_is_past_it() {
        assert!(FirstRun::from_stored(None, true).is_done());
        assert!(!FirstRun::from_stored(None, false).is_done());
        let half = FirstRun {
            analysis_done: true,
            write_done: false,
        };
        assert_eq!(FirstRun::from_stored(Some(half), true), half);
    }

    #[test]
    fn an_old_settings_file_with_folders_does_not_show_the_card() {
        let settings: crate::settings::Settings =
            serde_json::from_str(r#"{"folders":["/music"],"theme":"slate"}"#).unwrap();
        assert_eq!(settings.first_run, None);
        let first_run = FirstRun::from_stored(settings.first_run, !settings.folders.is_empty());
        assert!(first_run.is_done());
    }

    #[test]
    fn a_stored_first_run_survives_a_save_and_load() {
        let folder = crate::tag_writing::testing::ScratchFolder::new("first-run-settings");
        let path = folder.0.join("settings.json");
        let settings = crate::settings::Settings {
            first_run: Some(FirstRun {
                analysis_done: true,
                write_done: false,
            }),
            ..Default::default()
        };
        settings.save(&path).unwrap();
        assert_eq!(
            crate::settings::Settings::load(&path).unwrap().first_run,
            settings.first_run
        );
    }
}
