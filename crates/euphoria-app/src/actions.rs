// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The commands menus and keyboard shortcuts can trigger.

// The macro defines public structs without documentation of their own; the
// list below says what each one does.
#![allow(missing_docs)]

use gpui::{Action, actions};

use crate::theme::ThemeName;

actions!(
    euphoria,
    [
        // Quits the app.
        Quit,
        // Opens the folder picker.
        AddFolder,
        // Moves the keyboard to the search field.
        FocusSearch,
        // Switch to one of the built in themes.
        UseSlateTheme,
        UsePaperTheme,
        UseDeckTheme,
        UseGraphiteTheme,
        UseKeyFirstTheme,
    ]
);

/// The action that switches to `theme`.
pub(crate) fn use_theme(theme: ThemeName) -> Box<dyn Action> {
    match theme {
        ThemeName::Slate => Box::new(UseSlateTheme),
        ThemeName::Paper => Box::new(UsePaperTheme),
        ThemeName::Deck => Box::new(UseDeckTheme),
        ThemeName::Graphite => Box::new(UseGraphiteTheme),
        ThemeName::KeyFirst => Box::new(UseKeyFirstTheme),
    }
}
