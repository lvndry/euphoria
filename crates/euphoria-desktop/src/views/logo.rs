// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Euphoria mark: the curve two notes a perfect fifth apart trace on an
//! oscilloscope, drawn from its formula so it is sharp at any size and takes
//! the theme's colour.

use std::f32::consts::{FRAC_PI_2, TAU};

use gpui::prelude::*;
use gpui::{Bounds, Hsla, PathBuilder, Pixels, canvas, point, px};

/// Points along the curve, enough to look smooth at the largest size drawn.
const POINT_COUNT: usize = 240;

/// Share of the mark's size the curve spans; the rest is margin on each side.
const CURVE_SPAN: f32 = 0.75;

/// Line width as a share of the mark's size, as in the logo artwork.
const LINE_WIDTH_SHARE: f32 = 2.6 / 64.0;

/// The thinnest line drawn, so the mark holds together at toolbar size.
const THINNEST_LINE: f32 = 1.4;

/// The mark, `size` wide and tall, in `colour`.
pub(crate) fn logo(size: Pixels, colour: Hsla) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, _, window, _| {
            let side = bounds.size.width / px(1.0);
            let centre = bounds.center();
            let reach = side * CURVE_SPAN / 2.0;
            let line_width = (side * LINE_WIDTH_SHARE).max(THINNEST_LINE);

            let mut builder = PathBuilder::stroke(px(line_width));
            for (index, (across, down)) in curve_points(POINT_COUNT).into_iter().enumerate() {
                let position = centre + point(px(across * reach), px(down * reach));
                if index == 0 {
                    builder.move_to(position);
                } else {
                    builder.line_to(position);
                }
            }
            builder.close();
            if let Ok(path) = builder.build() {
                window.paint_path(path, colour);
            }
        },
    )
    .size(size)
    .flex_shrink_0()
}

/// `count` points of the curve `x = sin(3t)`, `y = sin(2t + 90 degrees)` over
/// one full turn of `t`, each coordinate from -1.0 to 1.0 with `y` pointing
/// down as in the logo artwork, so the loop sits under the crossing.
fn curve_points(count: usize) -> Vec<(f32, f32)> {
    (0..count)
        .map(|index| {
            let turn = TAU * index as f32 / count as f32;
            ((3.0 * turn).sin(), (2.0 * turn + FRAC_PI_2).sin())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_curve_starts_at_the_bottom_centre_and_stays_in_bounds() {
        let points = curve_points(POINT_COUNT);
        assert_eq!(points.len(), POINT_COUNT);
        assert!(points[0].0.abs() < 1.0e-6);
        assert!((points[0].1 - 1.0).abs() < 1.0e-6);
        assert!(
            points
                .iter()
                .all(|(across, up)| across.abs() <= 1.0 && up.abs() <= 1.0)
        );
    }

    #[test]
    fn the_curve_is_symmetric_left_to_right() {
        let points = curve_points(POINT_COUNT);
        for index in 1..POINT_COUNT {
            let (across, up) = points[index];
            let (mirrored_across, mirrored_up) = points[POINT_COUNT - index];
            assert!((across + mirrored_across).abs() < 1.0e-4);
            assert!((up - mirrored_up).abs() < 1.0e-4);
        }
    }

    #[test]
    fn the_curve_reaches_all_four_edges() {
        let points = curve_points(POINT_COUNT);
        let furthest =
            |pick: fn(&(f32, f32)) -> f32| points.iter().map(pick).fold(f32::MIN, f32::max);
        assert!(furthest(|point| point.0) > 0.99);
        assert!(furthest(|point| -point.0) > 0.99);
        assert!(furthest(|point| point.1) > 0.99);
        assert!(furthest(|point| -point.1) > 0.99);
    }
}
