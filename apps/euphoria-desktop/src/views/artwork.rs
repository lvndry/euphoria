// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The square that shows the cover of a track: its picture, rounded like the
//! other surfaces, or the music note when it has none.

use std::path::Path;

use gpui::component::Icon;
use gpui::prelude::*;
use gpui::{Context, Div, ElementId, ObjectFit, Pixels, TestSupportExt, div, img, px};

use crate::app::EuphoriaApp;
use crate::artwork::{Lookup, ThumbnailSize};
use crate::assets::MUSIC_ICON_PATH;
use crate::views::{RADIUS_MEDIUM, RADIUS_SMALL};

/// A square of `side` points showing the cover of the track at `path`, or
/// the music note while it is being read and when there is none. The picture
/// is read in the background the first time it is wanted, and the window
/// draws again when it is ready. `tile_id` names the tile when it shows a picture, so tests can tell it from
/// the placeholder.
pub(crate) fn cover_tile(
    app: &EuphoriaApp,
    path: &Path,
    side: Pixels,
    thumbnail: ThumbnailSize,
    tile_id: impl Into<ElementId>,
    cx: &mut Context<'_, EuphoriaApp>,
) -> impl IntoElement {
    let palette = &app.theme.palette;
    let radius = if side < px(48.0) {
        RADIUS_SMALL
    } else {
        RADIUS_MEDIUM
    };
    let store = app.artwork();
    let cover = match store.lookup(path) {
        Lookup::Ready(cover) => Some(cover),
        Lookup::Needed(job) => {
            let store = store.clone();
            cx.spawn(async move |this, cx| {
                let loaded = cx
                    .background_executor()
                    .spawn(async move { job.run() })
                    .await;
                store.finish(loaded);
                this.update(cx, |_, cx| cx.notify()).ok();
            })
            .detach();
            None
        }
        Lookup::Loading | Lookup::Absent => None,
    };
    let tile: Div = div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .overflow_hidden()
        .size(side)
        .rounded(radius)
        .border_1()
        .border_color(palette.line)
        .bg(palette.raised);
    match cover {
        Some(cover) => tile
            .id(tile_id)
            .test_support()
            .child(
                img(cover.image(thumbnail))
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            )
            .into_any_element(),
        None => tile
            .child(
                Icon::default()
                    .path(MUSIC_ICON_PATH)
                    .size(side * 0.36)
                    .text_color(palette.text_tertiary),
            )
            .into_any_element(),
    }
}
