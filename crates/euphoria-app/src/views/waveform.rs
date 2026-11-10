// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The waveform of the selected track, drawn as thin vertical bars.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{Bounds, Hsla, Pixels, canvas, fill, point, px, size};

use crate::peaks::bars_from_peaks;

/// Height of the waveform.
pub(crate) const WAVEFORM_HEIGHT: Pixels = px(64.0);

const BAR_SPACING: f32 = 4.0;
const BAR_WIDTH: f32 = 2.4;
const SHORTEST_BAR_HALF_HEIGHT: f32 = 1.5;

/// A waveform as wide as its container. Without peaks it is a flat line of
/// the shortest bars.
pub(crate) fn waveform(peaks: Option<Arc<[f32]>>, colour: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, _, window, _| {
            let width = bounds.size.width / px(1.0);
            let height = bounds.size.height / px(1.0);
            let bar_count = (width / BAR_SPACING).floor() as usize;
            if bar_count == 0 {
                return;
            }
            let bars = match &peaks {
                Some(peaks) if !peaks.is_empty() => bars_from_peaks(peaks, bar_count),
                _ => vec![0.0; bar_count],
            };
            let spacing = width / bars.len() as f32;
            let tallest_half_height = height / 2.0 - 2.0;
            for (bar_index, peak) in bars.iter().enumerate() {
                let half_height = (peak * tallest_half_height).max(SHORTEST_BAR_HALF_HEIGHT);
                let left = bar_index as f32 * spacing + (spacing - BAR_WIDTH) / 2.0;
                let bar = Bounds::new(
                    bounds.origin + point(px(left), px(height / 2.0 - half_height)),
                    size(px(BAR_WIDTH), px(half_height * 2.0)),
                );
                window.paint_quad(fill(bar, colour));
            }
        },
    )
    .w_full()
    .h(WAVEFORM_HEIGHT)
}
