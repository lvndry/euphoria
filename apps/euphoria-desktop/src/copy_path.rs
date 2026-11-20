// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Copying the files of tracks as text: the paths a right-click on a row
//! covers, or those of the selection, one per line, with a toast saying how
//! many were copied.

use std::path::PathBuf;

use gpui::{ClipboardItem, Context};

use crate::app::EuphoriaApp;
use crate::format;

/// The label of the menu item that copies the paths of `count` tracks, or
/// `None` when there are none to copy.
pub(crate) fn menu_label(count: usize) -> Option<&'static str> {
    match count {
        0 => None,
        1 => Some("Copy path"),
        _ => Some("Copy paths"),
    }
}

/// The text copied for `paths`: each path on its own line, in the order
/// given.
pub(crate) fn clipboard_text(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The toast shown once the paths of `count` tracks are copied.
pub(crate) fn copied_message(count: usize) -> String {
    if count == 1 {
        "Copied path".to_string()
    } else {
        format!("Copied {} paths", format::count(count))
    }
}

impl EuphoriaApp {
    /// Copies the paths of a right-clicked row: the selection when the row
    /// is part of it, the row alone otherwise.
    pub(crate) fn copy_row_paths(&mut self, track_index: usize, cx: &mut Context<'_, Self>) {
        let paths = self.row_paths(track_index);
        self.copy_paths(&paths, cx);
    }

    /// Copies the paths of the selected tracks.
    pub(crate) fn copy_selected_paths(&mut self, cx: &mut Context<'_, Self>) {
        let paths = self.selected_paths();
        self.copy_paths(&paths, cx);
    }

    /// Puts `paths` on the clipboard as text and says so in a toast. Missing
    /// files are copied too, as their path still says where they were.
    fn copy_paths(&mut self, paths: &[PathBuf], cx: &mut Context<'_, Self>) {
        if paths.is_empty() {
            return;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(clipboard_text(paths)));
        self.show_toast(copied_message(paths.len()), None, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use gpui::TestAppContext;

    #[test]
    fn the_label_says_whether_one_or_several_paths_are_copied() {
        assert_eq!(menu_label(0), None);
        assert_eq!(menu_label(1), Some("Copy path"));
        assert_eq!(menu_label(2), Some("Copy paths"));
        assert_eq!(menu_label(40), Some("Copy paths"));
    }

    #[test]
    fn the_text_puts_each_path_on_its_own_line() {
        let one = vec![PathBuf::from("/music/house/first.flac")];
        assert_eq!(clipboard_text(&one), "/music/house/first.flac");
        let several = vec![
            PathBuf::from("/music/house/first.flac"),
            PathBuf::from("/music/techno/second track.mp3"),
            PathBuf::from("/music/house/third.wav"),
        ];
        assert_eq!(
            clipboard_text(&several),
            "/music/house/first.flac\n/music/techno/second track.mp3\n/music/house/third.wav"
        );
        assert_eq!(clipboard_text(&[]), "");
    }

    #[test]
    fn the_toast_says_how_many_paths() {
        assert_eq!(copied_message(1), "Copied path");
        assert_eq!(copied_message(3), "Copied 3 paths");
        assert_eq!(copied_message(1200), "Copied 1,200 paths");
    }

    /// The path of the track at `index` in the library.
    fn path_of(app: &EuphoriaApp, index: usize) -> PathBuf {
        app.session.library().track(index).unwrap().path.clone()
    }

    /// The text on the clipboard.
    fn clipboard(cx: &mut TestAppContext) -> Option<String> {
        cx.read_from_clipboard().and_then(|item| item.text())
    }

    /// A right-click on a row of the selection covers the whole selection in
    /// table order, and on any other row covers that row alone.
    #[gpui::test]
    fn a_row_copies_the_selection_it_is_part_of_or_itself(cx: &mut TestAppContext) {
        let (_, app) = crate::ui_tests::open_library(6, cx);
        cx.update(|cx| {
            app.update(cx, |app, _| {
                let first_shown = app.visible[0];
                let second_shown = app.visible[1];
                let fourth_shown = app.visible[3];
                let fifth_shown = app.visible[4];
                app.selected = Some(fourth_shown);
                app.marked = [fourth_shown, second_shown].into_iter().collect();
                assert_eq!(
                    app.row_paths(fourth_shown),
                    vec![path_of(app, second_shown), path_of(app, fourth_shown)]
                );
                assert_eq!(app.row_paths(fifth_shown), vec![path_of(app, fifth_shown)]);
                app.marked.clear();
                app.selected = Some(first_shown);
                assert_eq!(app.row_paths(first_shown), vec![path_of(app, first_shown)]);
                assert_eq!(app.row_paths(fifth_shown), vec![path_of(app, fifth_shown)]);
            });
        });
    }

    /// Copying puts the paths on the clipboard, one per line, and the toast
    /// says how many were copied.
    #[gpui::test]
    fn copying_puts_the_paths_on_the_clipboard(cx: &mut TestAppContext) {
        let (_, app) = crate::ui_tests::open_library(5, cx);
        let (first_shown, third_shown) = cx.update(|cx| {
            app.update(cx, |app, _| {
                let first_shown = app.visible[0];
                let third_shown = app.visible[2];
                app.selected = Some(third_shown);
                app.marked = [first_shown, third_shown].into_iter().collect();
                (first_shown, third_shown)
            })
        });

        cx.update(|cx| app.update(cx, |app, cx| app.copy_row_paths(third_shown, cx)));
        let expected = cx.update(|cx| {
            let app = app.read(cx);
            format!(
                "{}\n{}",
                path_of(app, first_shown).display(),
                path_of(app, third_shown).display()
            )
        });
        assert_eq!(clipboard(cx), Some(expected));
        cx.update(|cx| {
            let toast = &app.read(cx).toasts.current().unwrap().message;
            assert_eq!(toast, "Copied 2 paths");
        });

        cx.update(|cx| {
            app.update(cx, |app, cx| {
                app.marked.clear();
                app.selected = Some(first_shown);
                app.copy_selected_paths(cx);
            })
        });
        let expected = cx.update(|cx| path_of(app.read(cx), first_shown).display().to_string());
        assert_eq!(clipboard(cx), Some(expected));
        cx.update(|cx| {
            let toast = &app.read(cx).toasts.current().unwrap().message;
            assert_eq!(toast, "Copied path");
        });
    }

    /// With nothing selected the action copies nothing and shows no toast.
    #[gpui::test]
    fn copying_without_a_selection_is_a_no_op(cx: &mut TestAppContext) {
        let (_, app) = crate::ui_tests::open_library(3, cx);
        cx.update(|cx| {
            app.update(cx, |app, cx| {
                app.selected = None;
                app.marked.clear();
                app.copy_selected_paths(cx);
            })
        });
        assert_eq!(clipboard(cx), None);
        cx.update(|cx| assert!(app.read(cx).toasts.current().is_none()));
    }
}
