// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The icon shown in the Dock.
//!
//! A packaged app gets its icon from its bundle and keeps it. The binary
//! started with `cargo run` has no bundle, so the icon is set while the app
//! starts.

#[cfg(target_os = "macos")]
use std::path::Path;

/// The Euphoria mark on its dark tile, with the margin macOS icons have.
#[cfg(target_os = "macos")]
const DOCK_ICON_PNG: &[u8] = include_bytes!("../assets/dock-icon.png");

/// Whether the executable sits where a macOS app bundle keeps it:
/// `Name.app/Contents/MacOS/`.
#[cfg(target_os = "macos")]
fn is_inside_app_bundle(executable: &Path) -> bool {
    let mut folders = executable.ancestors().skip(1);
    let in_macos_folder = folders
        .next()
        .is_some_and(|folder| folder.file_name().is_some_and(|name| name == "MacOS"));
    let in_contents_folder = folders
        .next()
        .is_some_and(|folder| folder.file_name().is_some_and(|name| name == "Contents"));
    let in_app_folder = folders.next().is_some_and(|folder| {
        folder
            .extension()
            .is_some_and(|extension| extension == "app")
    });
    in_macos_folder && in_contents_folder && in_app_folder
}

/// Shows the Euphoria icon in the Dock when the app runs outside a bundle.
/// Does nothing when called off the main thread or when the image cannot be
/// read.
#[cfg(target_os = "macos")]
pub(crate) fn show() {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let bundled = std::env::current_exe().is_ok_and(|executable| is_inside_app_bundle(&executable));
    if bundled {
        return;
    }
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
    use std::path::Path;

    use super::{DOCK_ICON_PNG, is_inside_app_bundle};

    const BYTES_PER_PIXEL: usize = 4;

    fn alpha_at(pixels: &[u8], width: usize, column: usize, row: usize) -> u8 {
        pixels[(row * width + column) * BYTES_PER_PIXEL + 3]
    }

    #[test]
    fn the_dock_icon_is_an_opaque_tile_with_transparent_corners() {
        let mut reader = png::Decoder::new(std::io::Cursor::new(DOCK_ICON_PNG))
            .read_info()
            .unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((frame.width, frame.height), (1024, 1024));
        assert_eq!(frame.color_type, png::ColorType::Rgba);
        assert_eq!(frame.bit_depth, png::BitDepth::Eight);

        let width = frame.width as usize;
        let last = width - 1;
        for (column, row) in [(0, 0), (last, 0), (0, last), (last, last), (110, 110)] {
            assert_eq!(alpha_at(&pixels, width, column, row), 0, "{column}, {row}");
        }
        assert_eq!(alpha_at(&pixels, width, 512, 160), 255);
    }

    #[test]
    fn only_an_executable_in_an_app_bundle_counts_as_bundled() {
        let bundled = "/Applications/Euphoria.app/Contents/MacOS/euphoria-desktop";
        assert!(is_inside_app_bundle(Path::new(bundled)));
        for unbundled in [
            "/Users/someone/euphoria/target/release/euphoria-desktop",
            "/opt/Euphoria.app/euphoria-desktop",
            "/opt/Euphoria.app/Contents/Resources/euphoria-desktop",
            "/opt/Euphoria/Contents/MacOS/euphoria-desktop",
            "euphoria-desktop",
        ] {
            assert!(!is_inside_app_bundle(Path::new(unbundled)), "{unbundled}");
        }
    }
}
