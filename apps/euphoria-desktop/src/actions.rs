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
        /// Looks through every library folder again for new, moved, changed
        /// and removed files.
        RescanFolders,
        /// Opens the sheet that exports the library to a DJ program.
        ExportLibrary,
        /// Plays the selected track, or pauses it.
        TogglePlayback,
        /// Starts the metronome, or stops it.
        ToggleMetronome,
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
        /// Opens the Live panel, which shows the key and tempo of what the
        /// Mac is playing.
        OpenLive,
        /// Pauses Live listening, or resumes it.
        ToggleLiveListening,
        /// Keeps Live running in the menu bar when the library window is
        /// closed, or quits with it.
        ToggleLiveKeepRunning,
        /// Opens Euphoria at login, with Live in the menu bar, or stops.
        ToggleLiveOpenAtLogin,
        /// Brings up the library window.
        OpenEuphoria,
        /// Opens the settings.
        OpenSettings,
        /// Shows the two decks, or hides them.
        ToggleDecks,
        /// Gives the selected track the whole window.
        OpenFocusView,
        /// Plays the selected track of the list of tracks to play next, or
        /// loads it on deck B.
        PlayNextRow,
        /// Leaves the focus view for the library.
        CloseFocusView,
        /// Loops 4 bars from the beat at or before the playhead in the focus
        /// view, or leaves the loop playing.
        ToggleLoop,
        /// Halves the loop playing in the focus view.
        HalveLoop,
        /// Doubles the loop playing in the focus view.
        DoubleLoop,
        /// Opens the set builder page.
        OpenSetBuilder,
        /// Builds the set on the set builder page.
        BuildSet,
        /// Stars the selected tracks, or takes their stars off.
        ToggleFavorite,
        /// Adds the selected tracks to the end of the set.
        AddToSet,
        /// Reveals the selected track in the file manager.
        RevealTrack,
        /// Copies the paths of the selected tracks, one per line.
        CopyTrackPath,
        /// Selects every track the table shows.
        SelectAllTracks,
        /// Returns every column of the table to its usual width.
        ResetColumnWidths,
        /// Opens the command palette, the one box for tracks and commands.
        OpenCommandPalette,
        /// Plays the track the command palette has selected.
        PlayPaletteTrack,
        /// Opens the focus view on the playing track and plans a transition
        /// into its best match.
        PlanTransition,
        /// Analyzes the selected tracks again, replacing their analysis.
        AnalyzeAgain,
        /// Stops the analysis of the tracks still waiting.
        StopAnalysis,
        /// Separates the stems of the selected track.
        SeparateStems,
        /// Switches to the Slate theme.
        UseSlateTheme,
        /// Switches to the Paper theme.
        UsePaperTheme,
        /// Switches to the Deck theme.
        UseDeckTheme,
        /// Switches to the Graphite theme.
        UseGraphiteTheme,
    ]
);

/// The action that switches to `theme`.
pub(crate) fn use_theme(theme: ThemeName) -> Box<dyn Action> {
    match theme {
        ThemeName::Slate => Box::new(UseSlateTheme),
        ThemeName::Paper => Box::new(UsePaperTheme),
        ThemeName::Deck => Box::new(UseDeckTheme),
        ThemeName::Graphite => Box::new(UseGraphiteTheme),
    }
}
