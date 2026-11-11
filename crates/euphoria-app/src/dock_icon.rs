// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The icon shown in the Dock.
//!
//! A packaged app gets its icon from its bundle. The binary started with
//! `cargo run` has no bundle, so the icon is set while the app starts.

/// The Euphoria mark on its dark tile, with the margin macOS icons have.
#[cfg(target_os = "macos")]
const DOCK_ICON_PNG: &[u8] = include_bytes!("../assets/dock-icon.png");

/// Shows the Euphoria icon in the Dock. Does nothing when called off the
/// main thread or when the image cannot be read.
#[cfg(target_os = "macos")]
pub(crate) fn show() {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(DOCK_ICON_PNG);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let application = NSApplication::sharedApplication(main_thread);
    // SAFETY: the image is a valid, retained `NSImage`, which is what the
    // setter requires; only `None` is the unchecked case.
    unsafe { application.setApplicationIconImage(Some(&image)) };
}

/// Other systems take the icon from how the app is installed.
#[cfg(not(target_os = "macos"))]
pub(crate) fn show() {}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::DOCK_ICON_PNG;

    #[test]
    fn the_dock_icon_is_a_square_png_with_transparency() {
        assert_eq!(&DOCK_ICON_PNG[..8], b"\x89PNG\r\n\x1a\n");
        let width = u32::from_be_bytes([
            DOCK_ICON_PNG[16],
            DOCK_ICON_PNG[17],
            DOCK_ICON_PNG[18],
            DOCK_ICON_PNG[19],
        ]);
        let height = u32::from_be_bytes([
            DOCK_ICON_PNG[20],
            DOCK_ICON_PNG[21],
            DOCK_ICON_PNG[22],
            DOCK_ICON_PNG[23],
        ]);
        assert_eq!((width, height), (1024, 1024));
        let colour_type = DOCK_ICON_PNG[25];
        assert_eq!(colour_type, 6, "the icon needs an alpha channel");
    }
}
