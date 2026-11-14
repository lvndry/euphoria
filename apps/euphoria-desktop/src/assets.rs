// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The icons built into the app: the set the component library needs for its
//! own controls, plus the few more the views use.

use std::borrow::Cow;

use gpui::assets::{Assets as ComponentIcons, icon_assets};
use gpui::{AssetSource, Result, SharedString};

icon_assets!(ExtraIcons, [Music, Expand]);

/// Path of the music note icon shown where a track has no artwork.
pub(crate) const MUSIC_ICON_PATH: &str = "icons/music.svg";

/// Path of the expand icon on the button that opens the focus view.
pub(crate) const EXPAND_ICON_PATH: &str = "icons/expand.svg";

/// Path of the outlined star of a track or transition that is not a
/// favorite.
pub(crate) const STAR_ICON_PATH: &str = "icons/star.svg";

/// Path of the filled star of a favorite.
pub(crate) const STAR_FILLED_ICON_PATH: &str = "icons/star-fill.svg";

/// Every icon the app can draw.
pub(crate) struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        match ExtraIcons.load(path)? {
            Some(icon) => Ok(Some(icon)),
            None => ComponentIcons.load(path),
        }
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut paths = ComponentIcons.list(path)?;
        paths.extend(ExtraIcons.list(path)?);
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serves_the_extra_icons_and_the_component_icons() {
        for path in [
            MUSIC_ICON_PATH,
            STAR_ICON_PATH,
            STAR_FILLED_ICON_PATH,
            EXPAND_ICON_PATH,
            "icons/arrow-left.svg",
            "icons/search.svg",
            "icons/folder.svg",
        ] {
            let icon = AppAssets.load(path).unwrap();
            assert!(icon.is_some_and(|bytes| !bytes.is_empty()), "{path}");
        }
        let missing = AppAssets.load("icons/not-an-icon.svg");
        assert!(missing.ok().flatten().is_none());
    }
}
