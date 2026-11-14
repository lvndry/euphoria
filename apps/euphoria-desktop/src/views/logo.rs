// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Euphoria mark: the curve two notes a perfect fifth apart trace on an
//! oscilloscope, drawn from its formula so it is sharp at any size and takes
//! the theme's colour.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::time::Instant;

use gpui::prelude::*;
use gpui::{Bounds, Hsla, PathBuilder, Pixels, Window, canvas, point, px};

use crate::logo_motion::{CURVE_END, CURVE_START, LogoClock, LogoFrame, system_reduces_motion};

/// Points along the whole curve, enough to look smooth at the largest size drawn.
const POINT_COUNT: usize = 300;

/// Points on the circle that rounds the end of the line.
const CAP_POINTS: usize = 16;

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
            paint_trace(bounds, window, colour, CURVE_START, CURVE_END);
        },
    )
    .size(size)
    .flex_shrink_0()
}

/// The mark as the activity indicator: the same drawing as `logo`, traced
/// and erased in a loop while `is_busy` and for as long as it takes to
/// settle after. It asks for a new frame only while it is moving.
pub(crate) fn busy_logo(
    size: Pixels,
    colour: Hsla,
    background: Hsla,
    clock: LogoClock,
    is_busy: bool,
) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, _, window, _| {
            let reduces_motion = is_busy && system_reduces_motion();
            let frame = clock.frame(is_busy, reduces_motion, Instant::now());
            match frame {
                LogoFrame::Still => paint_trace(bounds, window, colour, CURVE_START, CURVE_END),
                LogoFrame::Trace { from, to } => paint_trace(bounds, window, colour, from, to),
                LogoFrame::Pulse { fade } => paint_trace(
                    bounds,
                    window,
                    mix(colour, background, fade),
                    CURVE_START,
                    CURVE_END,
                ),
            }
            if frame.is_moving() {
                window.request_animation_frame();
            }
        },
    )
    .size(size)
    .flex_shrink_0()
}

/// `colour` moved toward `background` by `amount`, from 0.0 to 1.0, staying opaque.
fn mix(colour: Hsla, background: Hsla, amount: f32) -> Hsla {
    Hsla {
        h: colour.h,
        s: colour.s + (background.s - colour.s) * amount,
        l: colour.l + (background.l - colour.l) * amount,
        a: colour.a,
    }
}

/// Paints the stretch of the curve from `from` to `to` turns, with round ends.
fn paint_trace(bounds: Bounds<Pixels>, window: &mut Window, colour: Hsla, from: f32, to: f32) {
    let side = bounds.size.width / px(1.0);
    let centre = bounds.center();
    let reach = side * CURVE_SPAN / 2.0;
    let line_width = (side * LINE_WIDTH_SHARE).max(THINNEST_LINE);
    let count =
        (((to - from) / (CURVE_END - CURVE_START) * POINT_COUNT as f32).ceil() as usize).max(2);
    let positions: Vec<_> = curve_points(from, to, count)
        .into_iter()
        .map(|(across, down)| centre + point(px(across * reach), px(down * reach)))
        .collect();

    if to - from > 1.0e-4 {
        let mut builder = PathBuilder::stroke(px(line_width));
        for (index, position) in positions.iter().enumerate() {
            if index == 0 {
                builder.move_to(*position);
            } else {
                builder.line_to(*position);
            }
        }
        if let Ok(path) = builder.build() {
            window.paint_path(path, colour);
        }
    }
    if to - from > 0.0 {
        for end in [positions[0], positions[positions.len() - 1]] {
            let mut builder = PathBuilder::fill();
            let rim: Vec<_> = (0..CAP_POINTS)
                .map(|index| {
                    let angle = TAU * index as f32 / CAP_POINTS as f32;
                    end + point(
                        px(angle.cos() * line_width / 2.0),
                        px(angle.sin() * line_width / 2.0),
                    )
                })
                .collect();
            builder.add_polygon(&rim, true);
            if let Ok(path) = builder.build() {
                window.paint_path(path, colour);
            }
        }
    }
}

/// `count` points of the curve `x = sin(3t)`, `y = sin(2t + 90 degrees)` from
/// `from` to `to` turns of `t`, each coordinate from -1.0 to 1.0 with `y`
/// pointing down as in the logo artwork. The whole curve runs from the tip
/// at 0.25 turns to the tip at 0.75, out and back along the same line.
fn curve_points(from: f32, to: f32, count: usize) -> Vec<(f32, f32)> {
    (0..count)
        .map(|index| {
            let turn = TAU * (from + (to - from) * index as f32 / (count - 1) as f32);
            ((3.0 * turn).sin(), (2.0 * turn + FRAC_PI_2).sin())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn whole_curve() -> Vec<(f32, f32)> {
        curve_points(CURVE_START, CURVE_END, POINT_COUNT)
    }

    #[test]
    fn the_curve_runs_from_the_top_left_tip_to_the_top_right_tip() {
        let points = whole_curve();
        assert_eq!(points.len(), POINT_COUNT);
        let (first_across, first_down) = points[0];
        let (last_across, last_down) = points[POINT_COUNT - 1];
        assert!((first_across + 1.0).abs() < 1.0e-5 && (first_down + 1.0).abs() < 1.0e-5);
        assert!((last_across - 1.0).abs() < 1.0e-5 && (last_down + 1.0).abs() < 1.0e-5);
        assert!(
            points
                .iter()
                .all(|(across, down)| across.abs() <= 1.0001 && down.abs() <= 1.0001)
        );
    }

    #[test]
    fn the_curve_passes_through_the_bottom_centre_half_way() {
        let points = whole_curve();
        let (across, down) = points[POINT_COUNT / 2];
        assert!(across.abs() < 0.02);
        assert!(down > 0.99);
    }

    #[test]
    fn the_curve_reaches_all_four_edges() {
        let points = whole_curve();
        let furthest =
            |pick: fn(&(f32, f32)) -> f32| points.iter().map(pick).fold(f32::MIN, f32::max);
        assert!(furthest(|point| point.0) > 0.99);
        assert!(furthest(|point| -point.0) > 0.99);
        assert!(furthest(|point| point.1) > 0.99);
        assert!(furthest(|point| -point.1) > 0.99);
    }

    #[test]
    fn a_stretch_ends_where_the_whole_curve_has_the_same_turn() {
        let half = curve_points(CURVE_START, 0.5, 100);
        let whole = whole_curve();
        let (across, down) = half[99];
        let (whole_across, whole_down) = whole[POINT_COUNT / 2];
        assert!((across - whole_across).abs() < 0.02);
        assert!((down - whole_down).abs() < 0.02);
    }

    #[test]
    fn mixing_moves_the_colour_toward_the_background_and_stays_opaque() {
        let colour = gpui::hsla(0.0, 0.0, 1.0, 1.0);
        let background = gpui::hsla(0.0, 0.0, 0.0, 1.0);
        assert_eq!(mix(colour, background, 0.0), colour);
        assert!((mix(colour, background, 0.5).l - 0.5).abs() < 1.0e-6);
        assert_eq!(mix(colour, background, 0.5).a, 1.0);
    }
}
