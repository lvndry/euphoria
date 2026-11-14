// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Camelot wheel: twelve numbers around it, the major keys (B) on the
//! outer ring and the minor keys (A) on the inner ring, each cell labelled.
//! The selected key's cell is filled with the accent colour and the cells of
//! the keys that mix with it with a lighter tint, so the moves a DJ can make
//! are visible at a glance.

use euphoria_core::{Key, Mode};
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, FontWeight, Hsla, PathBuilder, Pixels, Point, SharedString, TestSupportExt,
    canvas, div, point, px,
};

use crate::camelot::CamelotPosition;
use crate::theme::{Palette, blend};

/// The size the radii below are drawn for; the wheel is scaled from it.
const WHEEL_SIZE: f32 = 148.0;
/// Width and height of the wheel beside the statistics in the inspector
/// column, which is 256 px wide inside its padding at the narrowest.
pub(crate) const COLUMN_WHEEL_SIZE: f32 = 96.0;
/// The smallest the wheel is drawn, in the strip at its shortest.
pub(crate) const SMALLEST_WHEEL_SIZE: f32 = 100.0;
/// The largest the wheel is drawn, in a tall strip.
pub(crate) const LARGEST_WHEEL_SIZE: f32 = 220.0;
/// Radius where the outer ring, the major keys, ends, at `WHEEL_SIZE`.
const OUTER_RADIUS: f32 = WHEEL_SIZE / 2.0;
/// Radius between the two rings, at `WHEEL_SIZE`.
const MIDDLE_RADIUS: f32 = 52.0;
/// Radius where the inner ring, the minor keys, starts, at `WHEEL_SIZE`.
const HOLE_RADIUS: f32 = 30.0;
/// The smallest cell label, so the numbers stay legible on a small wheel.
const SMALLEST_LABEL_SIZE: f32 = 8.0;
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

/// The key a wheel cell stands for.
pub(crate) fn key_at(position: CamelotPosition) -> Key {
    let mode = if position.is_major {
        Mode::Major
    } else {
        Mode::Minor
    };
    (0..12)
        .map(|tonic| Key::new(tonic, mode))
        .find(|key| CamelotPosition::of(key) == position)
        .unwrap_or_else(|| Key::new(0, mode))
}

/// The wheel, `size` pixels square, with `code` in the middle. `emphasized`
/// lists the keys to colour; every other cell is drawn quiet. Hovering a
/// cell names its key.
pub(crate) fn wheel(
    code: String,
    emphasized: Vec<(CamelotPosition, Emphasis)>,
    palette: &Palette,
    size: f32,
) -> AnyElement {
    let scale = size / WHEEL_SIZE;
    let outer_radius = OUTER_RADIUS * scale;
    let middle_radius = MIDDLE_RADIUS * scale;
    let hole_radius = HOLE_RADIUS * scale;
    let label_size = (CELL_LABEL_SIZE * scale).max(SMALLEST_LABEL_SIZE);
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
            None => palette.text_secondary,
        }
    };

    let cells: Vec<(CamelotPosition, Option<Emphasis>)> = (1..=12_u8)
        .flat_map(|number| [false, true].map(|is_major| CamelotPosition { number, is_major }))
        .map(|position| (position, emphasis_of(position)))
        .collect();
    let painted = cells.clone();
    let labels = cells
        .iter()
        .enumerate()
        .map(|(index, (position, emphasis))| {
            let (radius, letter) = if position.is_major {
                ((outer_radius + middle_radius) / 2.0, 'B')
            } else {
                ((middle_radius + hole_radius) / 2.0, 'A')
            };
            let radians = (f32::from(position.number % 12) * SEGMENT_DEGREES).to_radians();
            let centre_x = size / 2.0 + radius * radians.sin();
            let centre_y = size / 2.0 - radius * radians.cos();
            let code = format!("{}{letter}", position.number);
            let description: SharedString = format!("{code}, {}", key_at(*position)).into();
            div()
                .id(("camelot-cell", index))
                .tooltip(move |window, cx| Tooltip::new(description.clone()).build(window, cx))
                .absolute()
                .left(px(centre_x - 12.0))
                .top(px(centre_y - 7.0))
                .w(px(24.0))
                .h(px(14.0))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(label_size))
                .line_height(px(14.0))
                .font_weight(if emphasis.is_some() {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(label_colour(*emphasis))
                .child(code)
        });

    div()
        .id("camelot-wheel")
        .relative()
        .flex_shrink_0()
        .size(px(size))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds: Bounds<Pixels>, _, window, _| {
                    let centre = bounds.center();
                    for (position, emphasis) in &painted {
                        if let Some(path) = cell_path(centre, *position, scale) {
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
                .text_size(px(CODE_SIZE * scale.max(0.8)))
                .text_color(palette.text)
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
fn cell_path(
    centre: Point<Pixels>,
    position: CamelotPosition,
    scale: f32,
) -> Option<gpui::Path<Pixels>> {
    let (outer, inner) = if position.is_major {
        (OUTER_RADIUS * scale, MIDDLE_RADIUS * scale + RING_GAP / 2.0)
    } else {
        (MIDDLE_RADIUS * scale - RING_GAP / 2.0, HOLE_RADIUS * scale)
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
    fn a_cell_names_the_key_it_stands_for() {
        for key_text in ["A minor", "C major", "Db minor", "B major"] {
            let key: Key = key_text.parse().unwrap();
            assert_eq!(key_at(CamelotPosition::of(&key)), key);
        }
        assert_eq!(key_at(position("A minor")).camelot(), "8A");
    }

    #[test]
    fn every_key_has_a_cell_of_its_own_on_its_ring() {
        let centre = point(px(0.0), px(0.0));
        for number in 1..=12_u8 {
            for is_major in [false, true] {
                assert!(cell_path(centre, CamelotPosition { number, is_major }, 1.0).is_some());
            }
        }
    }

    #[test]
    fn labels_read_at_least_4_5_to_1_in_every_theme() {
        use crate::theme::{AppTheme, ThemeName};
        for name in ThemeName::ALL {
            let palette = AppTheme::named(name).palette;
            let compatible = blend(palette.accent, palette.raised, 0.4);
            for (label, fill) in [
                (palette.accent_ink, palette.accent),
                (palette.text, compatible),
                (palette.text_secondary, palette.raised),
            ] {
                let ratio = contrast(label, fill);
                assert!(ratio >= 4.5, "{name:?}: contrast {ratio:.2}");
            }
        }
    }

    fn contrast(first: Hsla, second: Hsla) -> f32 {
        let luminance = |colour: Hsla| {
            let rgba = colour.to_rgb();
            let linear = |value: f32| {
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(rgba.r) + 0.7152 * linear(rgba.g) + 0.0722 * linear(rgba.b)
        };
        let lighter = luminance(first).max(luminance(second));
        let darker = luminance(first).min(luminance(second));
        (lighter + 0.05) / (darker + 0.05)
    }
}
