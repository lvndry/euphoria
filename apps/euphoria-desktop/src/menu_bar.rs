// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The menu bar item of Live: the commands it sends and what it shows,
//! behind a surface the system implements, so the rest runs, and is tested,
//! without a menu bar.

use gpui::{App, Entity};

use crate::live_model::LiveModel;

/// What the menu bar item asks the app to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MenuBarCommand {
    /// A click on the item: show the Live panel, or hide it.
    TogglePanel,
    OpenEuphoria,
    ToggleListening,
    ToggleKeepRunning,
    Quit,
}

/// What the item shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MenuBarState {
    pub(crate) title: String,
    pub(crate) is_listening: bool,
    pub(crate) keeps_running: bool,
}

impl MenuBarState {
    pub(crate) fn of(model: &LiveModel) -> Self {
        Self {
            title: model.menu_bar_title(),
            is_listening: model.is_listening(),
            keeps_running: model.settings.keep_running_when_closed,
        }
    }
}

/// The item in the system's menu bar.
pub(crate) trait MenuBarSurface {
    fn show(&mut self, state: &MenuBarState);
}

/// Shows `model` on `surface` now and whenever it changes, and carries out
/// the commands the item sends.
pub(crate) fn install(
    model: &Entity<LiveModel>,
    mut surface: Box<dyn MenuBarSurface>,
    commands: async_channel::Receiver<MenuBarCommand>,
    cx: &mut App,
) {
    surface.show(&MenuBarState::of(model.read(cx)));
    let mut last_shown = MenuBarState::of(model.read(cx));
    cx.observe(model, move |model, cx| {
        let state = MenuBarState::of(model.read(cx));
        if state != last_shown {
            surface.show(&state);
            last_shown = state;
        }
    })
    .detach();
    let model = model.downgrade();
    cx.spawn(async move |cx| {
        while let Ok(command) = commands.recv().await {
            let model = model.clone();
            cx.update(|cx| match command {
                MenuBarCommand::TogglePanel => crate::live_app::toggle_panel(cx),
                MenuBarCommand::OpenEuphoria => crate::open_euphoria(cx),
                MenuBarCommand::ToggleListening => {
                    model
                        .update(cx, |model, cx| model.toggle_listening(cx))
                        .ok();
                }
                MenuBarCommand::ToggleKeepRunning => {
                    model
                        .update(cx, |model, cx| {
                            let keep = !model.settings.keep_running_when_closed;
                            model.set_keep_running(keep, cx);
                        })
                        .ok();
                }
                MenuBarCommand::Quit => cx.quit(),
            });
        }
    })
    .detach();
}

#[cfg(test)]
pub(crate) mod testing {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::*;

    /// A surface that remembers what it was shown.
    #[derive(Clone, Default)]
    pub(crate) struct RecordingSurface {
        pub(crate) shown: Rc<RefCell<Vec<MenuBarState>>>,
    }

    impl MenuBarSurface for RecordingSurface {
        fn show(&mut self, state: &MenuBarState) {
            self.shown.borrow_mut().push(state.clone());
        }
    }
}
