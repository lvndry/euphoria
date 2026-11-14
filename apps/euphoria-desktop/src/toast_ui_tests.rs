// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The toast that ends a write: what it says, what Undo does, how long it
//! stays, and when Command Z is its Undo.

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::test::TestWindowExt;
use gpui::{AnyWindowHandle, AppContext, Entity, TestAppContext};

use crate::app::EuphoriaApp;
use crate::tag_writing::testing::{self as tag_testing, ScratchFolder};
use crate::toast::{ToastAction, Toasts};
use crate::ui_tests::{click, is_shown, open_taggable_files};

const UNDO_KEY: &str = if cfg!(target_os = "macos") {
    "cmd-z"
} else {
    "ctrl-z"
};

fn write_all_tracks(window: AnyWindowHandle, cx: &mut TestAppContext) {
    click(window, "write-tags", cx);
    click(window, "scope-shown", cx);
    click(window, "confirm-tag-write", cx);
}

fn toast_message(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> Option<String> {
    cx.update(|cx| {
        view.read(cx)
            .toasts
            .current()
            .map(|toast| toast.message.clone())
    })
}

fn press(window: AnyWindowHandle, key: &str, cx: &mut TestAppContext) {
    cx.update_window(window, |_, window, cx| window.press(key, cx))
        .unwrap();
    cx.run_until_parked();
}

fn fake_clock(view: &Entity<EuphoriaApp>, cx: &mut TestAppContext) -> impl Fn(f32) + use<> {
    let origin = Instant::now();
    let offset = Rc::new(Cell::new(0.0_f32));
    let reading = offset.clone();
    cx.update(|cx| {
        view.update(cx, |app, _| {
            app.toasts = Toasts::default().with_sources(
                move || origin + Duration::from_secs_f32(reading.get()),
                || false,
            );
        })
    });
    move |seconds| offset.set(seconds)
}

#[gpui::test]
fn a_write_ends_with_a_toast_that_counts_the_files(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("toast-write");
    let files = scratch.copy_fixtures();
    let before = tag_testing::contents(&files);
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);

    write_all_tracks(window, cx);

    assert_ne!(tag_testing::contents(&files), before);
    assert_eq!(
        toast_message(&view, cx).as_deref(),
        Some("Wrote key and BPM to 3 files")
    );
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.tag_panel.is_none(), "no sheet is left to dismiss");
        assert!(app.notice.is_none());
        assert_eq!(
            app.toasts.current().unwrap().action,
            Some(ToastAction::UndoTagWrite)
        );
    });
    assert!(is_shown(window, "toast", cx));
    assert!(is_shown(window, "toast-action", cx));
}

#[gpui::test]
fn undo_in_the_toast_puts_every_byte_back(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("toast-undo");
    let files = scratch.copy_fixtures();
    let before = tag_testing::contents(&files);
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    write_all_tracks(window, cx);
    assert_ne!(tag_testing::contents(&files), before);

    click(window, "toast-action", cx);

    assert_eq!(tag_testing::contents(&files), before);
    assert_eq!(
        toast_message(&view, cx).as_deref(),
        Some("Put the old tags back in 3 files")
    );
    cx.update(|cx| {
        let app = view.read(cx);
        assert!(app.tag_panel.is_none());
        assert!(!app.toasts.offers_undo(), "there is nothing left to undo");
    });
}

#[gpui::test]
fn command_z_undoes_the_write_while_the_toast_shows(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("toast-shortcut");
    let files = scratch.copy_fixtures();
    let before = tag_testing::contents(&files);
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    write_all_tracks(window, cx);

    press(window, UNDO_KEY, cx);

    assert_eq!(tag_testing::contents(&files), before);
    assert_eq!(
        toast_message(&view, cx).as_deref(),
        Some("Put the old tags back in 3 files")
    );
}

#[gpui::test]
fn command_z_in_a_text_field_leaves_the_write_alone(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("toast-text-field");
    let files = scratch.copy_fixtures();
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    write_all_tracks(window, cx);
    let written = tag_testing::contents(&files);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |app, cx| app.focus_search(window, cx));
    })
    .unwrap();

    press(window, UNDO_KEY, cx);

    assert_eq!(tag_testing::contents(&files), written);
    assert!(cx.update(|cx| view.read(cx).toasts.offers_undo()));
}

#[gpui::test]
fn the_toast_goes_after_ten_seconds_and_waits_while_hovered(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("toast-expiry");
    let files = scratch.copy_fixtures();
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    let set_time = fake_clock(&view, cx);
    write_all_tracks(window, cx);
    let expire = |cx: &mut TestAppContext| {
        cx.update(|cx| view.update(cx, |app, _| app.toasts.expire_if_due()))
    };

    set_time(9.0);
    assert!(!expire(cx));
    cx.update(|cx| view.update(cx, |app, cx| app.set_toast_hovered(true, cx)));
    set_time(120.0);
    assert!(!expire(cx), "hovering holds it");
    cx.update(|cx| view.update(cx, |app, cx| app.set_toast_hovered(false, cx)));
    set_time(120.9);
    assert!(!expire(cx), "one second was left");
    set_time(121.0);
    assert!(expire(cx));
    assert_eq!(toast_message(&view, cx), None);
    assert!(!is_shown(window, "toast", cx));
}

#[gpui::test]
fn a_second_toast_replaces_the_first(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("toast-replace");
    let files = scratch.copy_fixtures();
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    write_all_tracks(window, cx);

    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.show_toast(
                "Exported 575 tracks to rekordbox XML".to_string(),
                Some(ToastAction::Reveal("/exports/library.xml".into())),
                cx,
            )
        })
    });

    assert_eq!(
        toast_message(&view, cx).as_deref(),
        Some("Exported 575 tracks to rekordbox XML")
    );
    assert!(!cx.update(|cx| view.read(cx).toasts.offers_undo()));
    assert!(is_shown(window, "toast", cx));
}

#[gpui::test]
fn show_in_finder_reveals_the_file_and_closes_the_toast(cx: &mut TestAppContext) {
    let scratch = ScratchFolder::new("toast-reveal");
    let files = scratch.copy_fixtures();
    let (window, view) = open_taggable_files(&scratch, &files, None, cx);
    cx.update(|cx| {
        view.update(cx, |app, cx| {
            app.show_toast(
                "Exported 2 tracks to rekordbox XML".to_string(),
                Some(ToastAction::Reveal("/exports/library.xml".into())),
                cx,
            )
        })
    });

    click(window, "toast-action", cx);

    cx.update(|cx| {
        let app = view.read(cx);
        assert_eq!(
            app.revealed,
            vec![std::path::PathBuf::from("/exports/library.xml")]
        );
        assert!(app.toasts.current().is_none());
    });
}
