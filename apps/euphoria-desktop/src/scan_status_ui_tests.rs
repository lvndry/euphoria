// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The line shown while folders are searched for audio files: it names the
//! folder, keeps the full path in a tooltip, counts the files found as the
//! walk goes, and says the same in the toolbar, the empty table and the
//! logo's tooltip.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Bounds, Entity, Pixels, TestAppContext, px, size};

use crate::app::EuphoriaApp;
use crate::playback::testing::SilentOutput;
use crate::scan_status::COUNT_INTERVAL;
use crate::settings::Settings;
use crate::ui_tests::{is_shown, open_library, open_window_with};

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/Users/dj"), PathBuf::from)
}

fn begin_scan(
    view: &Entity<EuphoriaApp>,
    paths: Vec<PathBuf>,
    cx: &mut TestAppContext,
) -> (u64, Arc<AtomicUsize>) {
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            let (scan_id, found) = app.scans.begin(paths);
            app.redraw_scan_count(scan_id, cx);
            cx.notify();
            (scan_id, found)
        })
    })
}

fn label_and_tooltip(
    view: &Entity<EuphoriaApp>,
    cx: &mut TestAppContext,
) -> (Option<String>, Option<String>) {
    cx.update(|cx| {
        let app = view.read(cx);
        (app.scans.label(), app.scans.tooltip())
    })
}

/// The toolbar's line, the empty table's line and the logo's tooltip all
/// show `expected`.
fn says_everywhere(
    window: AnyWindowHandle,
    view: &Entity<EuphoriaApp>,
    expected: &str,
    cx: &mut TestAppContext,
) {
    assert!(is_shown(window, "scan-status", cx));
    assert!(is_shown(window, "scan-status-empty", cx));
    let (label, _) = label_and_tooltip(view, cx);
    assert_eq!(label.as_deref(), Some(expected));
    let busy = cx.update(|cx| view.read(cx).busy_work());
    assert_eq!(busy, [expected]);
}

#[gpui::test]
fn one_folder_is_named_with_its_path_in_the_tooltip(cx: &mut TestAppContext) {
    let (window, view) = open_library(0, cx);
    let folder = home().join("DJ tracks");
    let (scan_id, _) = begin_scan(&view, vec![folder], cx);

    says_everywhere(window, &view, "Looking for audio files in ~/DJ tracks", cx);
    let (_, tooltip) = label_and_tooltip(&view, cx);
    assert_eq!(tooltip.as_deref(), Some("~/DJ tracks"));
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        window.hover("scan-status", cx);
        window.render_frame(cx);
    })
    .unwrap();

    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.scans.finish(scan_id);
            cx.notify();
        })
    });
    assert!(!is_shown(window, "scan-status", cx));
    assert!(!is_shown(window, "scan-status-empty", cx));
}

#[gpui::test]
fn several_folders_are_counted_with_every_path_in_the_tooltip(cx: &mut TestAppContext) {
    let (window, view) = open_library(0, cx);
    begin_scan(
        &view,
        vec![
            home().join("DJ tracks"),
            PathBuf::from("/Volumes/USB/Edits"),
        ],
        cx,
    );
    begin_scan(&view, vec![home().join("Downloads")], cx);

    says_everywhere(
        window,
        &view,
        "Looking for audio files in ~/DJ tracks and 2 more",
        cx,
    );
    let (_, tooltip) = label_and_tooltip(&view, cx);
    assert_eq!(
        tooltip.as_deref(),
        Some("~/DJ tracks\n/Volumes/USB/Edits\n~/Downloads")
    );
}

#[gpui::test]
fn the_count_of_files_found_updates_a_few_times_a_second(cx: &mut TestAppContext) {
    let (window, view) = open_library(0, cx);
    let redraws = Rc::new(Cell::new(0));
    let _observer = cx.update(|cx| {
        let redraws = redraws.clone();
        cx.observe(&view, move |_, _| redraws.set(redraws.get() + 1))
    });
    let (scan_id, found) = begin_scan(&view, vec![home().join("DJ tracks")], cx);
    cx.run_until_parked();

    found.store(214, Ordering::Relaxed);
    cx.executor().advance_clock(COUNT_INTERVAL);
    says_everywhere(
        window,
        &view,
        "Looking for audio files in ~/DJ tracks \u{b7} 214 found",
        cx,
    );

    let before = redraws.get();
    for count in 215..=300 {
        found.store(count, Ordering::Relaxed);
        cx.executor().advance_clock(COUNT_INTERVAL / 20);
    }
    let redraws_for_the_count = redraws.get() - before;
    assert!(
        (1..=5).contains(&redraws_for_the_count),
        "86 new files over a second redraw {redraws_for_the_count} times"
    );
    let (label, _) = label_and_tooltip(&view, cx);
    assert!(label.unwrap().ends_with("found"));

    cx.update(|cx| view.update(cx, |app, _| app.scans.finish(scan_id)));
    cx.executor().advance_clock(COUNT_INTERVAL);
    let after_finish = redraws.get();
    found.store(400, Ordering::Relaxed);
    cx.executor().advance_clock(COUNT_INTERVAL * 4);
    assert_eq!(
        redraws.get(),
        after_finish,
        "the count stops once the search is over"
    );
}

fn open_sized(
    track_count: usize,
    width: f32,
    height: f32,
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<EuphoriaApp>) {
    open_window_with(
        crate::demo::tracks(track_count),
        Box::new(SilentOutput),
        Settings::default(),
        None,
        size(px(width), px(height)),
        cx,
    )
}

/// Begins the longest search line there is: a deep path, more folders and
/// a five figure count.
fn begin_longest_scan(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) {
    let (_, found) = begin_scan(
        view,
        vec![
            home()
                .join("Music")
                .join("Collections")
                .join("2026 summer festival season")
                .join("DJ tracks"),
            PathBuf::from("/Volumes/USB/Edits"),
            PathBuf::from("/Volumes/USB/Warm up"),
        ],
        cx,
    );
    found.store(12_345, Ordering::Relaxed);
    cx.executor().advance_clock(COUNT_INTERVAL);
}

struct ToolbarLine {
    row: Bounds<Pixels>,
    path: Bounds<Pixels>,
    path_text_width: Pixels,
    live: Bounds<Pixels>,
}

/// Where the toolbar draws the search line and its path, how wide the path
/// would be uncut, and where the Live button beside it is, after a frame.
fn toolbar_line(
    window: AnyWindowHandle,
    view: &Entity<EuphoriaApp>,
    cx: &mut TestAppContext,
) -> ToolbarLine {
    let path_text = cx.update(|cx| view.read(cx).scans.label_parts().unwrap().1.unwrap());
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let run = window.text_style().to_run(path_text.len());
        let path_text_width = window
            .text_system()
            .shape_line(
                path_text.clone().into(),
                crate::views::SMALL_TEXT_SIZE,
                &[run],
                None,
            )
            .width;
        ToolbarLine {
            row: window.find("toolbar-progress").bounds(),
            path: window.find("scan-status-path").bounds(),
            path_text_width,
            live: window.find("open-live").bounds(),
        }
    })
    .unwrap()
}

impl ToolbarLine {
    fn stays_clear_of_the_controls(&self, window_width: f32) {
        assert!(
            self.row.size.height < px(24.0),
            "one line, not {:?}",
            self.row.size.height
        );
        assert!(
            self.row.right() <= self.live.left(),
            "{:?} runs into {:?}",
            self.row,
            self.live
        );
        assert!(self.live.right() <= px(window_width));
    }
}

#[gpui::test]
fn on_first_launch_the_narrowest_window_shows_the_whole_path_of_one_folder(
    cx: &mut TestAppContext,
) {
    let (window, view) = open_sized(0, 960.0, 560.0, cx);
    let (_, found) = begin_scan(&view, vec![home().join("Music").join("DJ tracks")], cx);
    found.store(214, Ordering::Relaxed);
    cx.executor().advance_clock(COUNT_INTERVAL);
    let (label, _) = label_and_tooltip(&view, cx);
    assert_eq!(
        label.as_deref(),
        Some("Looking for audio files in ~/Music/DJ tracks \u{b7} 214 found")
    );
    let line = toolbar_line(window, &view, cx);
    line.stays_clear_of_the_controls(960.0);
    assert!(
        line.path.size.width >= line.path_text_width - px(0.5),
        "the path is cut to {:?} of {:?}",
        line.path.size.width,
        line.path_text_width
    );
}

#[gpui::test]
fn a_path_too_long_for_the_room_is_cut_in_the_middle(cx: &mut TestAppContext) {
    let (window, view) = open_sized(0, 960.0, 560.0, cx);
    begin_longest_scan(&view, cx);
    let (label, _) = label_and_tooltip(&view, cx);
    assert_eq!(
        label.as_deref(),
        Some(
            "Looking for audio files in ~/Music/Collections/\u{2026}/DJ tracks and 2 more \
             \u{b7} 12345 found"
        )
    );
    let line = toolbar_line(window, &view, cx);
    line.stays_clear_of_the_controls(960.0);
    assert!(
        line.path.size.width > px(0.0) && line.path.size.width < line.path_text_width,
        "the path is cut to fit, to {:?} of {:?}",
        line.path.size.width,
        line.path_text_width
    );
    assert!(line.path.right() <= line.row.right());
}

#[gpui::test]
fn beside_the_library_controls_the_line_never_runs_into_them(cx: &mut TestAppContext) {
    for width in [960.0, 1440.0] {
        let (window, view) = open_sized(5, width, 560.0, cx);
        begin_longest_scan(&view, cx);
        toolbar_line(window, &view, cx).stays_clear_of_the_controls(width);
        let busy = cx.update(|cx| view.read(cx).busy_work());
        assert!(busy[0].contains("~/Music/Collections/\u{2026}/DJ tracks"));
    }
}
