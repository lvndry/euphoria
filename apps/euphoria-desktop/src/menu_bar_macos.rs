// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The macOS side of the menu bar item: an `NSStatusItem` whose title is the
//! key heard. A click sends `TogglePanel`; a right click or a control click
//! opens a menu with the other commands.

use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSEventMask, NSEventModifierFlags, NSEventType, NSMenu, NSMenuItem, NSStatusBar,
    NSStatusItem, NSVariableStatusItemLength,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSString, ns_string};

use crate::menu_bar::{MenuBarCommand, MenuBarState, MenuBarSurface};

struct TargetState {
    sender: async_channel::Sender<MenuBarCommand>,
    state: RefCell<Option<MenuBarState>>,
    item: RefCell<Option<Retained<NSStatusItem>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "EuphoriaLiveMenuTarget"]
    #[ivars = TargetState]
    struct MenuTarget;

    unsafe impl NSObjectProtocol for MenuTarget {}

    impl MenuTarget {
        #[unsafe(method(statusItemClicked:))]
        fn status_item_clicked(&self, _sender: Option<&AnyObject>) {
            let application = NSApplication::sharedApplication(self.mtm());
            let wants_menu = application.currentEvent().is_some_and(|event| {
                event.r#type() == NSEventType::RightMouseUp
                    || event
                        .modifierFlags()
                        .contains(NSEventModifierFlags::Control)
            });
            if wants_menu {
                self.show_menu();
            } else {
                self.send(MenuBarCommand::TogglePanel);
            }
        }

        #[unsafe(method(openEuphoria:))]
        fn open_euphoria(&self, _sender: Option<&AnyObject>) {
            self.send(MenuBarCommand::OpenEuphoria);
        }

        #[unsafe(method(toggleListening:))]
        fn toggle_listening(&self, _sender: Option<&AnyObject>) {
            self.send(MenuBarCommand::ToggleListening);
        }

        #[unsafe(method(toggleKeepRunning:))]
        fn toggle_keep_running(&self, _sender: Option<&AnyObject>) {
            self.send(MenuBarCommand::ToggleKeepRunning);
        }

        #[unsafe(method(showPanel:))]
        fn show_panel(&self, _sender: Option<&AnyObject>) {
            self.send(MenuBarCommand::TogglePanel);
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            self.send(MenuBarCommand::Quit);
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker, sender: async_channel::Sender<MenuBarCommand>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TargetState {
            sender,
            state: RefCell::new(None),
            item: RefCell::new(None),
        });
        // SAFETY: NSObject's init, called once on the allocated object.
        unsafe { msg_send![super(this), init] }
    }

    fn send(&self, command: MenuBarCommand) {
        let _ = self.ivars().sender.try_send(command);
    }
}

impl MenuTarget {
    fn show_menu(&self) {
        let mtm = self.mtm();
        let (Some(state), Some(item)) = (
            self.ivars().state.borrow().clone(),
            self.ivars().item.borrow().clone(),
        ) else {
            return;
        };
        let Some(button) = item.button(mtm) else {
            return;
        };
        let menu = self.menu(mtm, &state);
        item.setMenu(Some(&menu));
        // SAFETY: a plain click on a live button.
        unsafe { button.performClick(None) };
        item.setMenu(None);
    }

    fn menu(&self, mtm: MainThreadMarker, state: &MenuBarState) -> Retained<NSMenu> {
        let menu = NSMenu::new(mtm);
        let add = |title: &str, action: objc2::runtime::Sel, checked: bool| {
            // SAFETY: the title, action and key equivalent are valid, and the target
            // implements the action.
            unsafe {
                let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                    NSMenuItem::alloc(mtm),
                    &NSString::from_str(title),
                    Some(action),
                    ns_string!(""),
                );
                item.setTarget(Some(self));
                item.setState(if checked { 1 } else { 0 });
                menu.addItem(&item);
            }
        };
        add("Show Live", sel!(showPanel:), false);
        add(
            if state.is_listening {
                "Pause listening"
            } else {
                "Start listening"
            },
            sel!(toggleListening:),
            false,
        );
        add(
            "Keep Live running when the window is closed",
            sel!(toggleKeepRunning:),
            state.keeps_running,
        );
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        add("Open Euphoria", sel!(openEuphoria:), false);
        add("Quit Euphoria", sel!(quit:), false);
        menu
    }
}

/// The item in the menu bar, removed again when dropped.
pub(crate) struct StatusItem {
    item: Retained<NSStatusItem>,
    target: Retained<MenuTarget>,
}

/// Adds the item to the menu bar and returns it with the commands it sends.
/// `None` when not called on the main thread.
pub(crate) fn connect() -> Option<(StatusItem, async_channel::Receiver<MenuBarCommand>)> {
    let mtm = MainThreadMarker::new()?;
    let (sender, receiver) = async_channel::unbounded();
    let target = MenuTarget::new(mtm, sender);
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(NSVariableStatusItemLength);
    let button = item.button(mtm)?;
    // SAFETY: the target is a live object that implements the action.
    unsafe {
        button.setTarget(Some(&target));
        button.setAction(Some(sel!(statusItemClicked:)));
        let _ = button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
    }
    *target.ivars().item.borrow_mut() = Some(item.clone());
    Some((StatusItem { item, target }, receiver))
}

impl MenuBarSurface for StatusItem {
    fn show(&mut self, state: &MenuBarState) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        if let Some(button) = self.item.button(mtm) {
            button.setTitle(&NSString::from_str(&state.title));
        }
        *self.target.ivars().state.borrow_mut() = Some(state.clone());
    }
}

impl Drop for StatusItem {
    fn drop(&mut self) {
        *self.target.ivars().item.borrow_mut() = None;
        NSStatusBar::systemStatusBar().removeStatusItem(&self.item);
    }
}
