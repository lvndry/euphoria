// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Showing a track in the library window when Live asks: selected in the
//! table, scrolled into view and open in the inspector, without playing it.

use std::path::PathBuf;

use gpui::{AppContext as _, Context};

use crate::app::EuphoriaApp;

impl EuphoriaApp {
    /// Selects the track at `path` and shows it in the table, taking off a
    /// search, filter, folder or playlist that hides it. The music plays in
    /// another app, so nothing is played. A track the library does not have
    /// yet, as in a window just opened, is shown once its folders are read.
    pub(crate) fn show_track_at_path(&mut self, path: PathBuf, cx: &mut Context<'_, Self>) {
        match self.session.library().index_of(&path) {
            Some(index) => self.reveal_in_window(index, cx),
            None => self.pending_reveal = Some(path),
        }
    }

    /// Shows the track Live asked for, once the library has it.
    pub(crate) fn reveal_pending(&mut self, cx: &mut Context<'_, Self>) {
        let Some(index) = self
            .pending_reveal
            .as_ref()
            .and_then(|path| self.session.library().index_of(path))
        else {
            return;
        };
        self.pending_reveal = None;
        self.reveal_in_window(index, cx);
    }

    fn reveal_in_window(&mut self, index: usize, cx: &mut Context<'_, Self>) {
        let window = self.window_handle;
        let view = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            cx.update_window(window, |_, window, cx| {
                view.update(cx, |app, cx| app.reveal_track_in_table(index, window, cx))
                    .ok();
            })
            .ok();
        })
        .detach();
    }
}
