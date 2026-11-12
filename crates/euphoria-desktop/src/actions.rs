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
        /// Folds the sidebar away, or brings it back.
        ToggleSidebar,
        /// Moves the keyboard to the search field.
        FocusSearch,
        /// Shows what writing tags to the selected tracks would change.
        WriteTags,
        /// Puts back the tags the last write replaced.
        UndoTagWrite,
        /// Opens the tag editor for the selected tracks.
        EditTags,
        /// Adds a cue to the selected track at the playhead.
        AddCueAtPlayhead,
        /// Undoes the latest edit of the selected track's cues.
        UndoCueEdit,
        /// Opens the settings.
        OpenSettings,
        /// Shows the two decks, or hides them.
        ToggleDecks,
        /// Selects every track the table shows.
        SelectAllTracks,
        /// Returns every column of the table to its usual width.
        ResetColumnWidths,
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
