// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Giving the Live popover the height of its content while its top edge stays
//! where it is, under the menu bar item. A macOS window resized by its size
//! keeps its bottom edge, so the frame is set whole.

/// The native window behind `window`.
#[cfg(target_os = "macos")]
fn native_window(window: &gpui::Window) -> Option<objc2::rc::Retained<objc2_app_kit::NSWindow>> {
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: the handle holds the window's content view, which is alive while
    // the window is, and this runs on the main thread that draws the window.
    let view = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
    view.window()
}

/// Gives `window` the height `height`, keeping its top edge where it is.
/// Resizing a macOS window keeps its bottom edge, so the top left corner is
/// put back once the window has its new size.
#[cfg(target_os = "macos")]
pub(crate) fn set_height_keeping_top(
    window: &mut gpui::Window,
    top_left: (f32, f32),
    height: f32,
    cx: &mut gpui::App,
) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSScreen;
    use objc2_foundation::NSPoint;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(main_height) = NSScreen::screens(mtm)
        .firstObject()
        .map(|screen| screen.frame().size.height)
    else {
        return;
    };
    let top_left = NSPoint::new(f64::from(top_left.0), main_height - f64::from(top_left.1));
    window.resize(gpui::size(window.bounds().size.width, gpui::px(height)));
    window.on_next_frame(move |window, _| {
        if let Some(native) = native_window(window) {
            native.setFrameTopLeftPoint(top_left);
        }
    });
    let _ = cx;
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn set_height_keeping_top(
    _window: &mut gpui::Window,
    _top_left: (f32, f32),
    _height: f32,
    _cx: &mut gpui::App,
) {
}
