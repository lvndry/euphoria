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
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext, px, size};

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

    says_everywhere(window, &view, "Looking for audio files in DJ tracks", cx);
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

    says_everywhere(window, &view, "Looking for audio files in 3 folders", cx);
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
        "Looking for audio files in DJ tracks \u{b7} 214 found",
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

#[gpui::test]
fn the_line_fits_on_one_line_in_the_narrowest_window(cx: &mut TestAppContext) {
    let (window, view) = open_window_with(
        crate::demo::tracks(5),
        Box::new(SilentOutput),
        Settings::default(),
        None,
        size(px(960.0), px(560.0)),
        cx,
    );
    let (_, found) = begin_scan(
        &view,
        vec![home().join("Extended mixes from the summer festival season")],
        cx,
    );
    found.store(12_345, Ordering::Relaxed);
    cx.executor().advance_clock(COUNT_INTERVAL);
    let (label, _) = label_and_tooltip(&view, cx);
    assert_eq!(
        label.as_deref(),
        Some("Looking for audio files in Extended mixes from the\u{2026} \u{b7} 12345 found")
    );
    cx.update_window(window, |_, window, cx| {
        window.render_frame(cx);
        let status = window.find("scan-status").bounds();
        let live = window.find("open-live").bounds();
        assert!(
            status.size.height < px(24.0),
            "one line, not {:?}",
            status.size.height
        );
        assert!(
            status.right() <= live.left(),
            "{status:?} runs into {live:?}"
        );
        assert!(live.right() <= px(960.0));
    })
    .unwrap();
}
