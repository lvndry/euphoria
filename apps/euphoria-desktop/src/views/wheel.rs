// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Camelot wheel: twelve numbers around it, the major keys (B) on the
//! outer ring and the minor keys (A) on the inner ring, each cell labelled.
//! The selected key's cell is filled with the accent colour and the cells of
//! the keys that mix with it with a lighter tint, so the moves a DJ can make
//! are visible at a glance.

use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, FontWeight, Hsla, PathBuilder, Pixels, Point, TestSupportExt, canvas, div,
    point, px,
};

use crate::camelot::CamelotPosition;
use crate::theme::{Palette, blend};

/// Width and height of the wheel.
pub(crate) const WHEEL_SIZE: f32 = 148.0;
/// Radius where the outer ring, the major keys, ends.
const OUTER_RADIUS: f32 = WHEEL_SIZE / 2.0;
/// Radius between the two rings.
const MIDDLE_RADIUS: f32 = 52.0;
/// Radius where the inner ring, the minor keys, starts.
const HOLE_RADIUS: f32 = 30.0;
const SEGMENT_DEGREES: f32 = 30.0;
/// Space left between neighbouring cells, in degrees and in pixels.
const GAP_DEGREES: f32 = 1.5;
const RING_GAP: f32 = 1.5;
/// Degrees between the points that approximate each arc.
const ARC_STEP_DEGREES: f32 = 3.0;
/// Size of the label in each cell.
const CELL_LABEL_SIZE: f32 = 9.5;
/// Size of the code in the middle.
const CODE_SIZE: f32 = 14.0;

/// How strongly a cell of the wheel is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Emphasis {
    /// The selected key.
    Selected,
    /// A key that mixes with the selected key.
    Compatible,
}

/// The wheel with `code` in the middle. `emphasized` lists the keys to
/// colour; every other cell is drawn quiet.
pub(crate) fn wheel(
    code: String,
    emphasized: Vec<(CamelotPosition, Emphasis)>,
    palette: &Palette,
) -> AnyElement {
    let quiet = palette.raised;
    let selected = palette.accent;
    let compatible = blend(palette.accent, palette.raised, 0.4);
    let emphasis_of = move |position: CamelotPosition| {
        emphasized
            .iter()
            .find(|(emphasized_position, _)| *emphasized_position == position)
            .map(|(_, emphasis)| *emphasis)
    };
    let fill_of = move |emphasis: Option<Emphasis>| match emphasis {
        Some(Emphasis::Selected) => selected,
        Some(Emphasis::Compatible) => compatible,
        None => quiet,
    };
    let label_colour = |emphasis: Option<Emphasis>| -> Hsla {
        match emphasis {
            Some(Emphasis::Selected) => palette.accent_ink,
            Some(Emphasis::Compatible) => palette.text,
            None => palette.text_tertiary,
        }
    };

    let cells: Vec<(CamelotPosition, Option<Emphasis>)> = (1..=12_u8)
        .flat_map(|number| [false, true].map(|is_major| CamelotPosition { number, is_major }))
        .map(|position| (position, emphasis_of(position)))
        .collect();
    let painted = cells.clone();
    let labels = cells.iter().map(|(position, emphasis)| {
        let (radius, letter) = if position.is_major {
            ((OUTER_RADIUS + MIDDLE_RADIUS) / 2.0, 'B')
        } else {
            ((MIDDLE_RADIUS + HOLE_RADIUS) / 2.0, 'A')
        };
        let radians = (f32::from(position.number % 12) * SEGMENT_DEGREES).to_radians();
        let centre_x = WHEEL_SIZE / 2.0 + radius * radians.sin();
        let centre_y = WHEEL_SIZE / 2.0 - radius * radians.cos();
        div()
            .absolute()
            .left(px(centre_x - 12.0))
            .top(px(centre_y - 7.0))
            .w(px(24.0))
            .h(px(14.0))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(CELL_LABEL_SIZE))
            .line_height(px(14.0))
            .font_weight(if emphasis.is_some() {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::NORMAL
            })
            .text_color(label_colour(*emphasis))
            .child(format!("{}{letter}", position.number))
    });

    div()
        .id("camelot-wheel")
        .relative()
        .flex_shrink_0()
        .size(px(WHEEL_SIZE))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds: Bounds<Pixels>, _, window, _| {
                    let centre = bounds.center();
                    for (position, emphasis) in &painted {
                        if let Some(path) = cell_path(centre, *position) {
                            window.paint_path(path, fill_of(*emphasis));
                        }
                    }
                },
            )
            .absolute()
            .size_full(),
        )
        .children(labels)
        .child(
            div()
                .absolute()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(CODE_SIZE))
                .font_weight(FontWeight::SEMIBOLD)
                .child(code),
        )
        .test_support()
        .into_any_element()
}

/// The keys to colour for a selected key and the keys that mix with it.
pub(crate) fn emphasis_for(
    selected: CamelotPosition,
    compatible: impl IntoIterator<Item = CamelotPosition>,
) -> Vec<(CamelotPosition, Emphasis)> {
    let mut emphasized = vec![(selected, Emphasis::Selected)];
    for position in compatible {
        if !emphasized
            .iter()
            .any(|(emphasized_position, _)| *emphasized_position == position)
        {
            emphasized.push((position, Emphasis::Compatible));
        }
    }
    emphasized
}

/// The outline of the cell for `position`. Twelve sits at the top and the
/// numbers run clockwise, as on a clock face; the major key of a number is
/// on the outer ring and its minor key on the inner ring.
fn cell_path(centre: Point<Pixels>, position: CamelotPosition) -> Option<gpui::Path<Pixels>> {
    let (outer, inner) = if position.is_major {
        (OUTER_RADIUS, MIDDLE_RADIUS + RING_GAP / 2.0)
    } else {
        (MIDDLE_RADIUS - RING_GAP / 2.0, HOLE_RADIUS)
    };
    let middle = f32::from(position.number % 12) * SEGMENT_DEGREES;
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
    builder.move_to(on_circle(outer, start));
    for degrees in angles.iter().skip(1) {
        builder.line_to(on_circle(outer, *degrees));
    }
    for degrees in angles.iter().rev() {
        builder.line_to(on_circle(inner, *degrees));
    }
    builder.close();
    builder.build().ok()
}

#[cfg(test)]
mod tests {
    use euphoria_core::Key;

    use super::*;
    use crate::mixes::compatible_keys;

    fn position(text: &str) -> CamelotPosition {
        CamelotPosition::of(&text.parse::<Key>().unwrap())
    }

    fn emphasized_codes(key: &str) -> Vec<(String, Emphasis)> {
        let key: Key = key.parse().unwrap();
        emphasis_for(
            CamelotPosition::of(&key),
            compatible_keys(&key).iter().map(CamelotPosition::of),
        )
        .into_iter()
        .map(|(position, emphasis)| {
            let letter = if position.is_major { 'B' } else { 'A' };
            (format!("{}{letter}", position.number), emphasis)
        })
        .collect()
    }

    #[test]
    fn a_minor_key_lights_its_own_cell_its_neighbours_and_its_relative_major() {
        let mut emphasized = emphasized_codes("A minor");
        emphasized.sort_by(|first, second| first.0.cmp(&second.0));
        assert_eq!(
            emphasized,
            [
                ("7A".to_string(), Emphasis::Compatible),
                ("8A".to_string(), Emphasis::Selected),
                ("8B".to_string(), Emphasis::Compatible),
                ("9A".to_string(), Emphasis::Compatible),
            ]
        );
    }

    #[test]
    fn the_two_rings_of_one_number_are_told_apart() {
        let emphasized = emphasis_for(position("A minor"), [position("C major")]);
        assert_eq!(
            emphasized,
            [
                (position("A minor"), Emphasis::Selected),
                (position("C major"), Emphasis::Compatible),
            ]
        );
        assert_ne!(position("A minor"), position("C major"));
        assert_eq!(position("A minor").number, position("C major").number);
    }

    #[test]
    fn the_selected_key_is_listed_once_even_when_it_is_also_compatible() {
        let emphasized = emphasis_for(position("A minor"), [position("A minor")]);
        assert_eq!(emphasized, [(position("A minor"), Emphasis::Selected)]);
    }

    #[test]
    fn every_key_has_a_cell_of_its_own_on_its_ring() {
        let centre = point(px(0.0), px(0.0));
        for number in 1..=12_u8 {
            for is_major in [false, true] {
                assert!(cell_path(centre, CamelotPosition { number, is_major }).is_some());
            }
        }
    }
}
