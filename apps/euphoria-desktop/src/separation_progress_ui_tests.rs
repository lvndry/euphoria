// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The progress bars of a separation, driven by a stand in separation that
//! reports each step only when the test says so.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use euphoria_core::EuphoriaError;
use euphoria_core::stems::{SeparationObserver, SeparationStage, StemFiles};
use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::stems::{StemSeparation, StemSession, StemStatus};
use crate::tag_writing::testing::ScratchFolder;
use crate::track_overview::VocalLane;
use crate::ui_tests::{
    click, is_shown, open_cued_library, open_fixture_library, waveform_with_energy,
    write_stems_with_vocals_for,
};

enum Step {
    Stage(SeparationStage),
    Separated(usize, usize),
    Finish,
}

struct SteppedSeparation {
    steps: Arc<Mutex<Receiver<Step>>>,
    reported: Sender<()>,
    files: StemFiles,
}

impl StemSeparation for SteppedSeparation {
    fn stems(
        &mut self,
        _path: &Path,
        observer: &dyn SeparationObserver,
    ) -> Result<StemFiles, EuphoriaError> {
        loop {
            let step = self.steps.lock().unwrap().recv();
            match step {
                Ok(Step::Stage(stage)) => observer.stage(stage),
                Ok(Step::Separated(done, total)) => observer.separated(done, total),
                Ok(Step::Finish) => {
                    let _ = self.reported.send(());
                    return Ok(self.files.clone());
                }
                Err(_) => return Err(EuphoriaError::NoDataDirectory),
            }
            let _ = self.reported.send(());
        }
    }
}

/// The test's end of a stepped separation.
struct Stepper {
    steps: Sender<Step>,
    reported: Receiver<()>,
}

impl Stepper {
    /// Reports `step`, then lets the app poll it.
    fn step(&self, step: Step, cx: &mut TestAppContext) {
        self.steps.send(step).unwrap();
        self.reported
            .recv_timeout(Duration::from_secs(30))
            .expect("the separation took the step");
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
    }
}

fn stepped_session(files: StemFiles) -> (StemSession, Stepper) {
    let (step_sender, step_receiver) = channel();
    let (reported_sender, reported_receiver) = channel();
    let steps = Arc::new(Mutex::new(step_receiver));
    let session = StemSession::new(move |_| {
        Ok(Box::new(SteppedSeparation {
            steps: Arc::clone(&steps),
            reported: reported_sender.clone(),
            files: files.clone(),
        }) as Box<dyn StemSeparation>)
    });
    let stepper = Stepper {
        steps: step_sender,
        reported: reported_receiver,
    };
    (session, stepper)
}

fn selected_path(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> PathBuf {
    cx.update(|cx| view.read(cx).selected_track().unwrap().path.clone())
}

fn caption(view: &Entity<EuphoriaApp>, path: &Path, cx: &mut TestAppContext) -> Option<String> {
    cx.update(|cx| {
        view.read(cx)
            .separation_shown(path)
            .map(|shown| shown.caption)
    })
}

/// How much of `bar` its fill covers, from 0.0 to 1.0.
fn filled_share(
    window: AnyWindowHandle,
    bar: &'static str,
    fill: &'static str,
    cx: &mut TestAppContext,
) -> f32 {
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let bar = window.find(bar).bounds();
        let fill = window.find(fill).bounds();
        assert_eq!(fill.left(), bar.left(), "the fill grows from the left");
        assert_eq!(fill.size.height, bar.size.height, "at the bar's height");
        fill.size.width / bar.size.width
    })
    .unwrap()
}

fn open_with_empty_vocals_lane(
    session: StemSession,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>, PathBuf) {
    let (window, view) = open_fixture_library(cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.stems = session;
            app.show_waveform(waveform_with_energy());
            let path = app.selected_track().unwrap().path.clone();
            app.show_vocal_lane(path, VocalLane::NoStems);
            cx.notify();
        })
    });
    let path = selected_path(&view, cx);
    (window, view, path)
}

#[gpui::test]
fn the_vocals_lane_fills_with_the_progress_then_with_the_vocals(cx: &mut TestAppContext) {
    let folder = ScratchFolder::new("vocal-lane-progress");
    let (session, stepper) = stepped_session(write_stems_with_vocals_for(&folder, 16, 6));
    let (window, view, path) = open_with_empty_vocals_lane(session, cx);

    click(window, "vocals-separate", cx);
    assert!(is_shown(window, "vocals-separating", cx));
    assert!(!is_shown(window, "vocals-separate", cx));
    assert_eq!(
        caption(&view, &path, cx).as_deref(),
        Some("Separating the stems")
    );
    let share = filled_share(window, "vocals-separating", "vocals-separating-fill", cx);
    assert!(share < 0.01, "{share}");

    stepper.step(Step::Stage(SeparationStage::LoadingModel), cx);
    assert_eq!(
        caption(&view, &path, cx).as_deref(),
        Some("Separating the stems \u{b7} Loading the model")
    );

    stepper.step(Step::Stage(SeparationStage::Separating), cx);
    stepper.step(Step::Separated(1, 8), cx);
    assert_eq!(
        caption(&view, &path, cx).as_deref(),
        Some("Separating the stems \u{b7} 12%")
    );
    let eighth = filled_share(window, "vocals-separating", "vocals-separating-fill", cx);
    assert!((eighth - 0.125).abs() < 0.02, "{eighth}");

    stepper.step(Step::Separated(2, 8), cx);
    assert_eq!(
        caption(&view, &path, cx).as_deref(),
        Some("Separating the stems \u{b7} 25%")
    );
    let quarter = filled_share(window, "vocals-separating", "vocals-separating-fill", cx);
    assert!(quarter > eighth, "the fill grows");
    assert!((quarter - 0.25).abs() < 0.02, "{quarter}");

    stepper.step(Step::Finish, cx);
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !is_shown(window, ("vocal-stretch", 0_usize), cx) {
        assert!(
            std::time::Instant::now() < deadline,
            "the lane never filled with the vocals"
        );
        cx.executor().advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(matches!(
        cx.update(|cx| view.read(cx).stems.status(&path).cloned()),
        Some(StemStatus::Ready(_))
    ));
    assert!(!is_shown(window, "vocals-separating", cx));
    assert_eq!(caption(&view, &path, cx), None);
}

#[gpui::test]
fn a_track_in_line_says_how_many_are_ahead_with_an_empty_bar(cx: &mut TestAppContext) {
    let folder = ScratchFolder::new("vocal-lane-queued");
    let (session, stepper) = stepped_session(write_stems_with_vocals_for(&folder, 16, 6));
    let (window, view, _) = open_with_empty_vocals_lane(session, cx);
    let tracks: Vec<PathBuf> = cx.update(|cx| {
        view.read(cx)
            .session
            .library()
            .tracks()
            .iter()
            .map(|track| track.path.clone())
            .collect()
    });
    assert!(tracks.len() >= 3);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            for path in &tracks[..3] {
                app.request_stems_of(path.clone(), cx);
            }
            app.select(2, cx);
            app.show_waveform(waveform_with_energy());
            app.show_vocal_lane(tracks[2].clone(), VocalLane::NoStems);
        })
    });
    stepper.step(Step::Stage(SeparationStage::Separating), cx);
    stepper.step(Step::Separated(1, 2), cx);

    let third = selected_path(&view, cx);
    assert_eq!(third, tracks[2]);
    assert_eq!(
        caption(&view, &third, cx).as_deref(),
        Some("Waiting: 2 tracks ahead")
    );
    assert_eq!(
        caption(&view, &tracks[1], cx).as_deref(),
        Some("Waiting: 1 track ahead")
    );
    assert_eq!(
        caption(&view, &tracks[0], cx).as_deref(),
        Some("Separating the stems \u{b7} 50%")
    );
    assert!(is_shown(window, "vocals-separating", cx));
    assert!(
        !is_shown(window, "vocals-separating-fill", cx),
        "no fill while in line"
    );
}

#[gpui::test]
fn the_focus_view_stem_lanes_show_the_same_bar(cx: &mut TestAppContext) {
    let folder = ScratchFolder::new("focus-separation-progress");
    let (session, stepper) = stepped_session(write_stems_with_vocals_for(&folder, 16, 6));
    let (window, view) = open_cued_library(cx);
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.stems = session;
            app.show_waveform(waveform_with_energy());
            let path = app.selected_track().unwrap().path.clone();
            app.show_vocal_lane(path, VocalLane::NoStems);
        })
    });
    click(window, "focus-open", cx);
    click(window, "focus-stems-separate", cx);
    assert!(is_shown(window, "focus-stems-separating", cx));
    assert!(
        is_shown(window, "vocals-separating", cx),
        "the vocals lane above shows it too"
    );
    assert!(!is_shown(window, "focus-stems-separate", cx));

    stepper.step(Step::Stage(SeparationStage::Separating), cx);
    stepper.step(Step::Separated(1, 2), cx);
    let path = selected_path(&view, cx);
    assert_eq!(
        caption(&view, &path, cx).as_deref(),
        Some("Separating the stems \u{b7} 50%")
    );
    for (bar, fill) in [
        ("focus-stems-separating", "focus-stems-separating-fill"),
        ("vocals-separating", "vocals-separating-fill"),
    ] {
        let half = filled_share(window, bar, fill, cx);
        assert!((half - 0.5).abs() < 0.02, "{bar}: {half}");
    }
}
