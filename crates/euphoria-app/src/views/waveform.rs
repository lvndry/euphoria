// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The waveform of the selected track, drawn as thin vertical bars. Each bar
//! shows three bands on top of one another, bass behind and treble in front,
//! so a kick, a breakdown and a hi hat each look different. The part already
//! played is fainter, and a line marks where playing is.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{Bounds, Hsla, Pixels, canvas, fill, point, px, size};

use crate::peaks::{BandPeaks, bars_from_columns};
use crate::theme::{BandColours, Palette};

/// Height of the waveform.
pub(crate) const WAVEFORM_HEIGHT: Pixels = px(64.0);

const BAR_SPACING: f32 = 4.0;
const BAR_WIDTH: f32 = 2.4;
const SHORTEST_BAR_HALF_HEIGHT: f32 = 1.5;
const PLAYHEAD_WIDTH: f32 = 1.5;

/// Where a view was last drawn, written while painting and read when a click
/// has to be turned into a position along it.
pub(crate) type SeenBounds = Rc<Cell<Option<Bounds<Pixels>>>>;

/// The colours a waveform is drawn in.
#[derive(Debug, Clone, Copy)]
pub(crate) struct WaveformColours {
    /// The bands of the part still to be played.
    unplayed: BandColours,
    /// The bands of the part already played.
    played: BandColours,
    /// The flat line drawn while there is no waveform to show.
    flat: Hsla,
    /// The line that marks where playing is.
    playhead: Hsla,
}

impl WaveformColours {
    /// The waveform colours of `palette`.
    pub(crate) fn of(palette: &Palette) -> Self {
        Self {
            unplayed: palette.band_colours(),
            played: palette.played_band_colours(),
            flat: palette.waveform,
            playhead: palette.text,
        }
    }
}

/// A waveform as wide as its container. Without columns it is a flat line of
/// the shortest bars. `progress` is how far playing has got, from 0.0 to 1.0,
/// and `seen_bounds` is told where the waveform was drawn.
pub(crate) fn waveform(
    columns: Option<Arc<[BandPeaks]>>,
    colours: WaveformColours,
    progress: Option<f32>,
    seen_bounds: SeenBounds,
) -> impl IntoElement {
    canvas(
        move |bounds: Bounds<Pixels>, _, _| seen_bounds.set(Some(bounds)),
        move |bounds: Bounds<Pixels>, _, window, _| {
            let width = bounds.size.width / px(1.0);
            let height = bounds.size.height / px(1.0);
            let bar_count = (width / BAR_SPACING).floor() as usize;
            if bar_count == 0 {
                return;
            }
            let bars = match &columns {
                Some(columns) if !columns.is_empty() => bars_from_columns(columns, bar_count),
                _ => vec![BandPeaks::default(); bar_count],
            };
            let has_waveform = columns.as_ref().is_some_and(|columns| !columns.is_empty());
            let spacing = width / bars.len() as f32;
            let tallest_half_height = height / 2.0 - 2.0;
            let mut paint_bar = |left: f32, half_height: f32, colour: Hsla| {
                let bar = Bounds::new(
                    bounds.origin + point(px(left), px(height / 2.0 - half_height)),
                    size(px(BAR_WIDTH), px(half_height * 2.0)),
                );
                window.paint_quad(fill(bar, colour));
            };

            for (bar_index, bar) in bars.iter().enumerate() {
                let left = bar_index as f32 * spacing + (spacing - BAR_WIDTH) / 2.0;
                if !has_waveform {
                    paint_bar(left, SHORTEST_BAR_HALF_HEIGHT, colours.flat);
                    continue;
                }
                let bands = if is_played(bar_index, bars.len(), progress) {
                    colours.played
                } else {
                    colours.unplayed
                };
                paint_bar(
                    left,
                    (bar.low * tallest_half_height).max(SHORTEST_BAR_HALF_HEIGHT),
                    bands.low,
                );
                for (level, colour) in [(bar.mid, bands.mid), (bar.high, bands.high)] {
                    let half_height = level * tallest_half_height;
                    if half_height >= SHORTEST_BAR_HALF_HEIGHT / 2.0 {
                        paint_bar(left, half_height, colour);
                    }
                }
            }

            if let Some(progress) = progress {
                let left = (progress.clamp(0.0, 1.0) * width - PLAYHEAD_WIDTH / 2.0)
                    .clamp(0.0, width - PLAYHEAD_WIDTH);
                let playhead = Bounds::new(
                    bounds.origin + point(px(left), px(0.0)),
                    size(px(PLAYHEAD_WIDTH), px(height)),
                );
                window.paint_quad(fill(playhead, colours.playhead));
            }
        },
    )
    .w_full()
    .h(WAVEFORM_HEIGHT)
}

/// True when the middle of bar `bar_index` of `bar_count` lies before the
/// point playing has reached.
pub(crate) fn is_played(bar_index: usize, bar_count: usize, progress: Option<f32>) -> bool {
    let Some(progress) = progress else {
        return false;
    };
    if bar_count == 0 {
        return false;
    }
    (bar_index as f32 + 0.5) / bar_count as f32 <= progress
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_is_played_before_playing_starts_or_without_a_position() {
        assert!(!is_played(0, 10, None));
        assert!(!is_played(0, 10, Some(0.0)));
        assert!(!is_played(0, 0, Some(1.0)));
    }

    #[test]
    fn bars_before_the_playhead_are_played_and_bars_after_it_are_not() {
        let played: Vec<bool> = (0..10)
            .map(|bar_index| is_played(bar_index, 10, Some(0.32)))
            .collect();
        assert_eq!(
            played,
            [
                true, true, true, false, false, false, false, false, false, false
            ]
        );
    }

    #[test]
    fn every_bar_is_played_at_the_end() {
        assert!((0..72).all(|bar_index| is_played(bar_index, 72, Some(1.0))));
    }
}
