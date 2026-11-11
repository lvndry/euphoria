// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The commands menus and keyboard shortcuts can trigger.

use gpui::{Action, actions};

use crate::theme::ThemeName;

actions!(
    euphoria,
    [
        /// Quits the app.
        Quit,
        /// Opens the folder picker.
        AddFolder,
        /// Plays the selected track, or pauses it.
        TogglePlayback,
        /// Moves the keyboard to the search field.
        FocusSearch,
        /// Switches to the Slate theme.
        UseSlateTheme,
        /// Switches to the Paper theme.
        UsePaperTheme,
        /// Switches to the Deck theme.
        UseDeckTheme,
        /// Switches to the Graphite theme.
        UseGraphiteTheme,
        /// Switches to the Key first theme.
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
