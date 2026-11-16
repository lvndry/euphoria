// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rescanning the library's folders in a window without a screen, with the
//! analysis stubbed so it can be counted: new files and folders join,
//! deleted files leave, the selection and the sound carry on, an unchanged
//! library analyzes nothing, and a folder's own rescan stays inside it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use euphoria_core::TrackAnalysis;
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::demo;
use crate::library::TrackFailure;
use crate::playback::testing::SilentOutput;
use crate::rescan::SELECTION_REMOVED;
use crate::settings::Settings;
use crate::state::Session;
use crate::tag_writing::testing::ScratchFolder;
use crate::ui_tests::{click, is_shown};

#[derive(Default)]
struct CountingAnalysis {
    calls: Mutex<HashMap<PathBuf, usize>>,
}

impl CountingAnalysis {
    fn analysis(&self, path: &Path) -> Result<TrackAnalysis, TrackFailure> {
        *self
            .calls
            .lock()
            .unwrap()
            .entry(path.to_path_buf())
            .or_default() += 1;
        Ok(demo::tracks(1).remove(0).outcome.unwrap())
    }

    fn total(&self) -> usize {
        self.calls.lock().unwrap().values().sum()
    }

    fn calls_for(&self, path: &Path) -> usize {
        self.calls.lock().unwrap().get(path).copied().unwrap_or(0)
    }
}

struct RescanLibrary {
    _scratch: ScratchFolder,
    music: PathBuf,
    analysis: Arc<CountingAnalysis>,
    window: AnyWindowHandle,
    view: Entity<EuphoriaApp>,
}

impl RescanLibrary {
    /// A copy of a real audio file at `relative` inside the library folder.
    fn add_file(&self, relative: &str) -> PathBuf {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/euphoria-core/tests/fixtures/sine-440hz-mono.mp3");
        let path = self.music.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::copy(fixture, &path).unwrap();
        path
    }

    fn paths(&self, cx: &mut TestAppContext) -> Vec<PathBuf> {
        cx.update(|cx| {
            let mut paths: Vec<PathBuf> = self
                .view
                .read(cx)
                .session
                .library()
                .tracks()
                .iter()
                .map(|track| track.path.clone())
                .collect();
            paths.sort();
            paths
        })
    }

    fn select(&self, path: &Path, cx: &mut TestAppContext) {
        cx.update(|cx| {
            self.view.update(cx, |app, cx| {
                let index = app.session.library().index_of(path).unwrap();
                app.select(index, cx);
            })
        });
    }

    fn selected_path(&self, cx: &mut TestAppContext) -> Option<PathBuf> {
        cx.update(|cx| {
            self.view
                .read(cx)
                .selected_track()
                .map(|track| track.path.clone())
        })
    }

    fn toast(&self, cx: &mut TestAppContext) -> (String, Option<String>) {
        cx.update(|cx| {
            let toast = self
                .view
                .read(cx)
                .toasts
                .current()
                .expect("a toast")
                .clone();
            (toast.message, toast.detail)
        })
    }

    fn wait_for_analysis(&self, cx: &mut TestAppContext) {
        cx.run_until_parked();
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let active = cx.update(|cx| {
                self.view.update(cx, |app, _| {
                    app.session.collect_finished();
                    app.session.progress().is_active()
                })
            });
            if !active {
                break;
            }
            assert!(Instant::now() < deadline, "the workers did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
        cx.update(|cx| self.view.update(cx, |app, _| app.refresh_visible()));
    }

    fn rescan_from_the_button(&self, cx: &mut TestAppContext) {
        click(self.window, "rescan-folders", cx);
        self.wait_for_analysis(cx);
    }
}

/// A window on a library folder holding `files`, scanned and analyzed.
fn open_library(name: &str, files: &[&str], cx: &mut TestAppContext) -> RescanLibrary {
    let scratch = ScratchFolder::new(name);
    let music = scratch.0.join("DJ tracks");
    std::fs::create_dir_all(&music).unwrap();
    let analysis = Arc::new(CountingAnalysis::default());
    let stub = analysis.clone();
    let (window, view) = cx.update(|cx| {
        gpui::init(cx);
        let mut settings = Settings::default();
        settings
            .first_run
            .get_or_insert(crate::first_run::FirstRun::already_done());
        let (window, view) = gpui::open_window(Default::default(), cx, |window, cx| {
            cx.new(|cx| {
                let session =
                    Session::start_stubbed(Arc::new(move |path| stub.analysis(path))).unwrap();
                EuphoriaApp::new(
                    session,
                    settings,
                    None,
                    None,
                    Box::new(SilentOutput),
                    window,
                    cx,
                )
            })
        })
        .unwrap();
        crate::register_commands(window, &view, cx);
        (window, view)
    });
    let library = RescanLibrary {
        _scratch: scratch,
        music,
        analysis,
        window,
        view,
    };
    for file in files {
        library.add_file(file);
    }
    let folder = library.music.clone();
    cx.update(|cx| {
        library
            .view
            .update(cx, |app, cx| app.add_paths(vec![folder], true, cx))
    });
    library.wait_for_analysis(cx);
    library
}

#[gpui::test]
fn new_files_and_a_new_folder_appear_after_a_rescan(cx: &mut TestAppContext) {
    let library = open_library("rescan-new", &["First.mp3", "Second.mp3"], cx);
    assert_eq!(library.analysis.total(), 2);
    assert!(is_shown(library.window, "rescan-folders", cx));

    let third = library.add_file("Third.mp3");
    let in_subfolder = library.add_file("Edits/Fourth.mp3");
    library.rescan_from_the_button(cx);

    let paths = library.paths(cx);
    assert!(paths.contains(&third));
    assert!(paths.contains(&in_subfolder));
    assert_eq!(paths.len(), 4);
    assert_eq!(
        library.analysis.total(),
        4,
        "only the new files are analyzed"
    );
    assert_eq!(library.analysis.calls_for(&third), 1);
    assert_eq!(library.toast(cx).0, "2 new tracks");
    cx.update(|cx| {
        let app = library.view.read(cx);
        assert!(
            app.folder_tree
                .index_of(&library.music.join("Edits"))
                .is_some()
        );
        assert!(!app.scans.is_running());
    });
}

#[gpui::test]
fn a_deleted_file_disappears_after_a_rescan(cx: &mut TestAppContext) {
    let library = open_library("rescan-deleted", &["Keep.mp3", "Delete.mp3"], cx);
    let deleted = library.music.join("Delete.mp3");
    std::fs::remove_file(&deleted).unwrap();

    library.rescan_from_the_button(cx);

    assert_eq!(library.paths(cx), vec![library.music.join("Keep.mp3")]);
    assert_eq!(library.toast(cx).0, "1 track removed");
    assert_eq!(library.analysis.total(), 2, "nothing is analyzed again");
}

#[gpui::test]
fn an_unchanged_library_analyzes_nothing_and_says_so(cx: &mut TestAppContext) {
    let library = open_library(
        "rescan-unchanged",
        &["One.mp3", "Two.mp3", "Sub/Three.mp3"],
        cx,
    );
    let before = library.paths(cx);

    library.rescan_from_the_button(cx);

    assert_eq!(library.paths(cx), before);
    assert_eq!(library.analysis.total(), 3);
    assert_eq!(library.toast(cx), ("No changes".to_string(), None));
}

#[gpui::test]
fn the_selection_and_the_playing_track_survive_a_rescan(cx: &mut TestAppContext) {
    let library = open_library("rescan-selection", &["A.mp3", "B.mp3", "C.mp3"], cx);
    let playing = library.music.join("C.mp3");
    library.select(&playing, cx);
    cx.update(|cx| {
        library
            .view
            .update(cx, |app, cx| app.play_or_pause_selection(cx))
    });
    cx.run_until_parked();
    cx.update(|cx| assert!(library.view.read(cx).player.offers_pause()));

    std::fs::remove_file(library.music.join("A.mp3")).unwrap();
    library.add_file("D.mp3");
    library.rescan_from_the_button(cx);

    assert_eq!(library.selected_path(cx), Some(playing.clone()));
    cx.update(|cx| {
        let app = library.view.read(cx);
        assert!(app.player.offers_pause(), "playback goes on");
        assert_eq!(app.player.track(), Some(playing.as_path()));
    });
    assert_eq!(
        library.toast(cx),
        ("1 new track, 1 removed".to_string(), None)
    );
}

#[gpui::test]
fn a_removed_selected_track_leaves_nothing_selected_and_the_sound_on(cx: &mut TestAppContext) {
    let library = open_library("rescan-selected-removed", &["A.mp3", "B.mp3"], cx);
    let selected = library.music.join("B.mp3");
    library.select(&selected, cx);
    cx.update(|cx| {
        library
            .view
            .update(cx, |app, cx| app.play_or_pause_selection(cx))
    });
    cx.run_until_parked();

    std::fs::remove_file(&selected).unwrap();
    library.rescan_from_the_button(cx);

    assert_eq!(library.selected_path(cx), None);
    assert_eq!(
        library.toast(cx),
        (
            "1 track removed".to_string(),
            Some(SELECTION_REMOVED.to_string())
        )
    );
    cx.update(|cx| assert!(library.view.read(cx).player.offers_pause()));
}

#[gpui::test]
fn rescanning_one_folder_touches_only_its_subtree(cx: &mut TestAppContext) {
    let library = open_library(
        "rescan-subtree",
        &["House/One.mp3", "Techno/Two.mp3", "Techno/Three.mp3"],
        cx,
    );
    let house = library.music.join("House");
    let new_in_house = library.add_file("House/New.mp3");
    let new_in_techno = library.add_file("Techno/New.mp3");
    let deleted_in_techno = library.music.join("Techno/Three.mp3");
    std::fs::remove_file(&deleted_in_techno).unwrap();

    cx.update(|cx| {
        library
            .view
            .update(cx, |app, cx| app.rescan_folder(house.clone(), cx))
    });
    library.wait_for_analysis(cx);

    let paths = library.paths(cx);
    assert!(paths.contains(&new_in_house));
    assert!(!paths.contains(&new_in_techno));
    assert!(
        paths.contains(&deleted_in_techno),
        "Techno was not rescanned"
    );
    assert_eq!(library.toast(cx).0, "1 new track");
    cx.update(|cx| {
        assert_eq!(
            library.view.read(cx).session.library().folders(),
            std::slice::from_ref(&library.music),
            "the subfolder is not added as a folder of its own"
        );
    });
}
