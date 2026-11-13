// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Revealing a file in the file manager: the command each platform runs and
//! the app's action that starts it for the selected track.

use std::ffi::OsString;
use std::path::Path;

use crate::app::EuphoriaApp;

/// The file manager's command and arguments that reveal `path`. On macOS,
/// `open -R` selects the file in Finder; on Windows, Explorer opens its
/// folder; on Linux, the folder that holds it is opened, as there is no
/// standard reveal.
pub(crate) fn reveal_command(path: &Path) -> (OsString, Vec<OsString>) {
    #[cfg(target_os = "macos")]
    {
        (
            OsString::from("open"),
            vec![OsString::from("-R"), path.as_os_str().to_os_string()],
        )
    }
    #[cfg(target_os = "windows")]
    {
        let folder = path.parent().unwrap_or_else(|| Path::new("."));
        (
            OsString::from("explorer"),
            vec![folder.as_os_str().to_os_string()],
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let folder = path.parent().unwrap_or_else(|| Path::new("."));
        (
            OsString::from("xdg-open"),
            vec![folder.as_os_str().to_os_string()],
        )
    }
}

/// The label of the menu item that reveals a file, named after this
/// platform's file manager.
pub(crate) fn reveal_label() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Open in Finder"
    }
    #[cfg(not(target_os = "macos"))]
    {
        "Open in File Manager"
    }
}

/// Starts the command that reveals `path` in the file manager, logging the
/// problem when it cannot be started.
pub(crate) fn reveal(path: &Path) {
    let (program, arguments) = reveal_command(path);
    if let Err(problem) = std::process::Command::new(&program)
        .args(&arguments)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        eprintln!("could not open {path:?} in the file manager: {problem}");
    }
}

impl EuphoriaApp {
    /// Opens the selected track in the file manager.
    pub(crate) fn reveal_selected_track(&self) {
        let Some(track) = self.selected_track() else {
            return;
        };
        reveal(&track.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_reveals_the_file() {
        let (program, arguments) = reveal_command(Path::new("/music/demo/track.mp3"));
        #[cfg(target_os = "macos")]
        {
            assert_eq!(program, "open");
            assert_eq!(
                arguments,
                vec![
                    OsString::from("-R"),
                    OsString::from("/music/demo/track.mp3")
                ]
            );
        }
        #[cfg(target_os = "windows")]
        {
            assert_eq!(program, "explorer");
            assert_eq!(arguments, vec![OsString::from("/music/demo")]);
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            assert_eq!(program, "xdg-open");
            assert_eq!(arguments, vec![OsString::from("/music/demo")]);
        }
    }

    #[test]
    fn the_label_names_the_file_manager() {
        #[cfg(target_os = "macos")]
        assert_eq!(reveal_label(), "Open in Finder");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(reveal_label(), "Open in File Manager");
    }

    /// The action does not touch the state when nothing is selected, so it
    /// can sit in the View menu next to the selection actions.
    #[gpui::test]
    fn revealing_without_a_selection_is_a_no_op(cx: &mut gpui::TestAppContext) {
        let (_, app) = crate::ui_tests::open_library(0, cx);
        cx.update(|cx| {
            let app = app.read(cx);
            assert_eq!(app.selected, None);
            app.reveal_selected_track();
        });
    }
}
