// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A small Camelot wheel: twelve segments, one per number, with the selected
//! key's number in the accent colour.

use gpui::prelude::*;
use gpui::{Bounds, Div, FontWeight, Hsla, PathBuilder, Pixels, Point, canvas, div, point, px};

use crate::camelot::CamelotPosition;
use crate::theme::Palette;

const WHEEL_SIZE: f32 = 96.0;
const HOLE_SIZE: f32 = 52.0;
const SEGMENT_DEGREES: f32 = 30.0;
const GAP_DEGREES: f32 = 1.5;
/// Degrees between the points that approximate each arc.
const ARC_STEP_DEGREES: f32 = 3.0;

/// How strongly a segment of the wheel is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Emphasis {
    /// The number of the selected key.
    Selected,
    /// A number whose keys mix with the selected key.
    Compatible,
}

/// The wheel with `code` in the middle. `emphasized` lists the numbers to
/// colour; every other segment is drawn quiet.
pub(crate) fn wheel(code: String, emphasized: Vec<(u8, Emphasis)>, palette: &Palette) -> Div {
    let quiet = palette.raised;
    let selected = palette.accent;
    let compatible = blend(palette.accent, palette.raised, 0.45);

    div()
        .relative()
        .flex_shrink_0()
        .size(px(WHEEL_SIZE))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds: Bounds<Pixels>, _, window, _| {
                    let centre = bounds.center();
                    for number in 1..=12_u8 {
                        let colour = match emphasized
                            .iter()
                            .find(|(emphasized_number, _)| *emphasized_number == number)
                        {
                            Some((_, Emphasis::Selected)) => selected,
                            Some((_, Emphasis::Compatible)) => compatible,
                            None => quiet,
                        };
                        if let Some(path) = segment_path(centre, number) {
                            window.paint_path(path, colour);
                        }
                    }
                },
            )
            .absolute()
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .font_weight(FontWeight::SEMIBOLD)
                .child(code),
        )
}

/// The numbers to colour for a selected key and the keys that mix with it.
pub(crate) fn emphasis_for(
    selected: CamelotPosition,
    compatible: impl IntoIterator<Item = CamelotPosition>,
) -> Vec<(u8, Emphasis)> {
    let mut emphasized = vec![(selected.number, Emphasis::Selected)];
    for position in compatible {
        if !emphasized
            .iter()
            .any(|(number, _)| *number == position.number)
        {
            emphasized.push((position.number, Emphasis::Compatible));
        }
    }
    emphasized
}

/// The outline of the segment for `number`. Twelve sits at the top and the
/// numbers run clockwise, as on a clock face.
fn segment_path(centre: Point<Pixels>, number: u8) -> Option<gpui::Path<Pixels>> {
    let middle = f32::from(number % 12) * SEGMENT_DEGREES;
    let start = middle - SEGMENT_DEGREES / 2.0 + GAP_DEGREES / 2.0;
    let end = middle + SEGMENT_DEGREES / 2.0 - GAP_DEGREES / 2.0;
    let step_count = ((end - start) / ARC_STEP_DEGREES).ceil() as usize;
    let angles: Vec<f32> = (0..=step_count)
        .map(|step| start + (end - start) * step as f32 / step_count as f32)
        .collect();
    let on_circle = |radius: f32, degrees: f32| {
        let radians = degrees.to_radians();
        centre + point(px(radius * radians.sin()), px(-radius * radians.cos()))
    };

    let mut builder = PathBuilder::fill();
    builder.move_to(on_circle(WHEEL_SIZE / 2.0, start));
    for degrees in angles.iter().skip(1) {
        builder.line_to(on_circle(WHEEL_SIZE / 2.0, *degrees));
    }
    for degrees in angles.iter().rev() {
        builder.line_to(on_circle(HOLE_SIZE / 2.0, *degrees));
    }
    builder.close();
    builder.build().ok()
}

/// `amount` of `colour` over `base`, mixed channel by channel.
fn blend(colour: Hsla, base: Hsla, amount: f32) -> Hsla {
    let colour = colour.to_rgb();
    let base = base.to_rgb();
    let mix = |first: f32, second: f32| first * amount + second * (1.0 - amount);
    Hsla::from(gpui::Rgba {
        r: mix(colour.r, base.r),
        g: mix(colour.g, base.g),
        b: mix(colour.b, base.b),
        a: 1.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(number: u8, is_major: bool) -> CamelotPosition {
        CamelotPosition { number, is_major }
    }

    #[test]
    fn the_selected_number_is_listed_once_even_when_its_other_ring_mixes() {
        let emphasized = emphasis_for(
            position(8, false),
            [position(8, true), position(7, false), position(9, false)],
        );
        assert_eq!(
            emphasized,
            [
                (8, Emphasis::Selected),
                (7, Emphasis::Compatible),
                (9, Emphasis::Compatible)
            ]
        );
    }

    #[test]
    fn a_blend_lies_between_its_two_colours() {
        let white = gpui::white();
        let black = gpui::black();
        let grey = blend(white, black, 0.5).to_rgb();
        assert!((grey.r - 0.5).abs() < 0.01);
        assert_eq!(blend(white, black, 1.0).to_rgb().r, 1.0);
    }
}
