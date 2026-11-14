// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The toast in the window: showing one after a write, running its action,
//! and taking it away when its time is up.

use std::time::Duration;

use gpui::Context;

use crate::app::EuphoriaApp;
use crate::toast::ToastAction;

/// How often a showing toast is checked against its ten seconds.
const EXPIRY_CHECK: Duration = Duration::from_millis(250);

impl EuphoriaApp {
    /// Ends a write with a toast, in place of any toast showing.
    pub(crate) fn show_toast(
        &mut self,
        message: String,
        action: Option<ToastAction>,
        cx: &mut Context<'_, Self>,
    ) {
        let id = self.toasts.show(message, action);
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(EXPIRY_CHECK).await;
                let still_showing = this.update(cx, |this, cx| {
                    if this.toasts.expire_if_due() {
                        cx.notify();
                    }
                    this.toasts.is_showing(id)
                });
                if !matches!(still_showing, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// Ends a write with a toast that has a short second line, such as what
    /// to do next in another program.
    pub(crate) fn show_toast_with_detail(
        &mut self,
        message: String,
        detail: String,
        action: Option<ToastAction>,
        cx: &mut Context<'_, Self>,
    ) {
        self.show_toast(message, action, cx);
        if !detail.is_empty() {
            self.toasts.set_detail(detail);
        }
    }

    pub(crate) fn set_toast_hovered(&mut self, is_hovered: bool, cx: &mut Context<'_, Self>) {
        self.toasts.set_hovered(is_hovered);
        cx.notify();
    }

    /// Runs what the toast offers: Undo, or Show in Finder.
    pub(crate) fn run_toast_action(&mut self, cx: &mut Context<'_, Self>) {
        let Some(action) = self.toasts.current().and_then(|toast| toast.action.clone()) else {
            return;
        };
        match action {
            ToastAction::UndoTagWrite => {
                self.toasts.dismiss();
                self.undo_tag_write_from_toast(cx);
            }
            ToastAction::Reveal(path) => {
                self.toasts.dismiss();
                #[cfg(not(test))]
                crate::reveal::reveal(&path);
                #[cfg(test)]
                self.revealed.push(path);
                cx.notify();
            }
        }
    }

    /// Command Z outside a text field: undoes the write a toast offers to
    /// undo while one shows, and otherwise the latest cue edit. In a text
    /// field the key never gets here, because the field has its own undo.
    pub(crate) fn undo_latest(&mut self, cx: &mut Context<'_, Self>) {
        if self.toasts.offers_undo() {
            self.run_toast_action(cx);
        } else {
            self.undo_cue_edit(cx);
        }
    }
}
