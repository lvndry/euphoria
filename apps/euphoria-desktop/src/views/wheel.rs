// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Camelot wheel: twelve numbers around it, the major keys (B) on the
//! outer ring and the minor keys (A) on the inner ring, each cell labelled.
//! The selected key's cell is filled with the accent colour and the cells of
//! the keys that mix with it, the matching keys, with an accent tint and an
//! accent ring, so the moves a DJ can make are visible at a glance.

use euphoria_core::harmony::Relation;
use euphoria_core::harmony::words::relation_name;
use euphoria_core::{Key, Mode};
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, FontWeight, Hsla, PathBuilder, Pixels, Point, SharedString, TestSupportExt,
    canvas, div, point, px,
};

use crate::camelot::CamelotPosition;
use crate::mixes::keys_related_by;
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
/// How much accent the fill of a matching key keeps over the quiet fill:
/// enough to tell it from the other keys at a glance, little enough that the
/// text on it stays readable and the selected key stays the strongest.
const MATCHING_STRENGTH: f32 = 0.45;
/// Width of the accent ring around a matching key.
const MATCHING_RING_WIDTH: f32 = 1.5;

/// How strongly a cell of the wheel is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Emphasis {
    /// The selected key.
    Selected,
    /// A key that mixes with the selected key by this move.
    Matching(Relation),
}

/// The fills, ring and label colours of the wheel in one palette.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WheelColours {
    /// The fill of a key that is neither selected nor matching.
    pub(crate) quiet: Hsla,
    /// The label on a quiet key.
    pub(crate) quiet_label: Hsla,
    /// The fill of a matching key.
    pub(crate) matching: Hsla,
    /// The ring around a matching key.
    pub(crate) matching_ring: Hsla,
    /// The label on a matching key.
    pub(crate) matching_label: Hsla,
    /// The fill of the selected key.
    pub(crate) selected: Hsla,
    /// The label on the selected key.
    pub(crate) selected_label: Hsla,
}

impl WheelColours {
    /// The wheel's colours in `palette`.
    pub(crate) fn of(palette: &Palette) -> Self {
        Self {
            quiet: palette.raised,
            quiet_label: palette.text_secondary,
            matching: blend(palette.accent, palette.raised, MATCHING_STRENGTH),
            matching_ring: palette.accent,
            matching_label: palette.text,
            selected: palette.accent,
            selected_label: palette.accent_ink,
        }
    }

    fn fill(&self, emphasis: Option<Emphasis>) -> Hsla {
        match emphasis {
            Some(Emphasis::Selected) => self.selected,
            Some(Emphasis::Matching(_)) => self.matching,
            None => self.quiet,
        }
    }

    fn label(&self, emphasis: Option<Emphasis>) -> Hsla {
        match emphasis {
            Some(Emphasis::Selected) => self.selected_label,
            Some(Emphasis::Matching(_)) => self.matching_label,
            None => self.quiet_label,
        }
    }
}

/// The id of the cell for the key `code`, which says how the cell is drawn:
/// `camelot-selected-12A`, `camelot-matching-1A` or `camelot-key-5B`.
pub(crate) fn cell_id(code: &str, emphasis: Option<Emphasis>) -> SharedString {
    let state = match emphasis {
        Some(Emphasis::Selected) => "selected",
        Some(Emphasis::Matching(_)) => "matching",
        None => "key",
    };
    format!("camelot-{state}-{code}").into()
}

/// What hovering a cell says: its code and key and, on a matching key, the
/// move that reaches it, such as "1A, Bb minor, one step on the wheel".
pub(crate) fn cell_description(code: &str, key: Key, emphasis: Option<Emphasis>) -> String {
    match emphasis {
        Some(Emphasis::Matching(relation)) => {
            format!("{code}, {key}, {}", relation_name(relation).to_lowercase())
        }
        _ => format!("{code}, {key}"),
    }
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
    let colours = WheelColours::of(palette);
    let emphasis_of = move |position: CamelotPosition| {
        emphasized
            .iter()
            .find(|(emphasized_position, _)| *emphasized_position == position)
            .map(|(_, emphasis)| *emphasis)
    };

    let cells: Vec<(CamelotPosition, Option<Emphasis>)> = (1..=12_u8)
        .flat_map(|number| [false, true].map(|is_major| CamelotPosition { number, is_major }))
        .map(|position| (position, emphasis_of(position)))
        .collect();
    let painted = cells.clone();
    let labels = cells.iter().map(|(position, emphasis)| {
        let (radius, letter) = if position.is_major {
            ((outer_radius + middle_radius) / 2.0, 'B')
        } else {
            ((middle_radius + hole_radius) / 2.0, 'A')
        };
        let radians = (f32::from(position.number % 12) * SEGMENT_DEGREES).to_radians();
        let centre_x = size / 2.0 + radius * radians.sin();
        let centre_y = size / 2.0 - radius * radians.cos();
        let code = format!("{}{letter}", position.number);
        let description: SharedString =
            cell_description(&code, key_at(*position), *emphasis).into();
        div()
            .id(cell_id(&code, *emphasis))
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
            .text_color(colours.label(*emphasis))
            .child(code)
            .test_support()
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
                        if let Some(path) = cell_path(PathBuilder::fill(), centre, *position, scale)
                        {
                            window.paint_path(path, colours.fill(*emphasis));
                        }
                        if matches!(emphasis, Some(Emphasis::Matching(_)))
                            && let Some(ring) = cell_path(
                                PathBuilder::stroke(px(MATCHING_RING_WIDTH)),
                                centre,
                                *position,
                                scale,
                            )
                        {
                            window.paint_path(ring, colours.matching_ring);
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

/// The keys to colour for the selected `key`: the key itself and, when
/// `show_matching` is on, every key one of `relations` reaches, each with its
/// move.
pub(crate) fn emphasis_for(
    key: &Key,
    relations: &[Relation],
    show_matching: bool,
) -> Vec<(CamelotPosition, Emphasis)> {
    let mut emphasized = vec![(CamelotPosition::of(key), Emphasis::Selected)];
    if show_matching {
        emphasized.extend(
            keys_related_by(key, relations)
                .iter()
                .filter_map(|matching| {
                    key.relation_to(matching).map(|relation| {
                        (CamelotPosition::of(matching), Emphasis::Matching(relation))
                    })
                }),
        );
    }
    emphasized
}

/// The outline of the cell for `position`. Twelve sits at the top and the
/// numbers run clockwise, as on a clock face; the major key of a number is
/// on the outer ring and its minor key on the inner ring.
fn cell_path(
    mut builder: PathBuilder,
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
    use euphoria_core::harmony::MixOptions;

    use super::*;
    use crate::theme::{AppTheme, ThemeName};

    fn position(text: &str) -> CamelotPosition {
        CamelotPosition::of(&text.parse::<Key>().unwrap())
    }

    fn emphasized_codes(key: &str, relations: &[Relation]) -> Vec<(String, Emphasis)> {
        let key: Key = key.parse().unwrap();
        let mut emphasized: Vec<(String, Emphasis)> = emphasis_for(&key, relations, true)
            .into_iter()
            .map(|(position, emphasis)| {
                let letter = if position.is_major { 'B' } else { 'A' };
                (format!("{}{letter}", position.number), emphasis)
            })
            .collect();
        emphasized.sort_by(|first, second| first.0.cmp(&second.0));
        emphasized
    }

    #[test]
    fn a_minor_key_lights_its_own_cell_its_neighbours_and_its_relative_major() {
        assert_eq!(
            emphasized_codes("A minor", &MixOptions::default().relations),
            [
                ("7A".to_string(), Emphasis::Matching(Relation::Adjacent)),
                ("8A".to_string(), Emphasis::Selected),
                ("8B".to_string(), Emphasis::Matching(Relation::Relative)),
                ("9A".to_string(), Emphasis::Matching(Relation::Adjacent)),
            ]
        );
    }

    #[test]
    fn twelve_a_reaches_round_the_top_of_the_wheel_to_one_a() {
        let twelve_a = Key::new(1, Mode::Minor);
        assert_eq!(twelve_a.camelot(), "12A");
        assert_eq!(
            emphasized_codes("C# minor", &MixOptions::default().relations),
            [
                ("11A".to_string(), Emphasis::Matching(Relation::Adjacent)),
                ("12A".to_string(), Emphasis::Selected),
                ("12B".to_string(), Emphasis::Matching(Relation::Relative)),
                ("1A".to_string(), Emphasis::Matching(Relation::Adjacent)),
            ]
        );
    }

    #[test]
    fn with_matching_keys_off_only_the_selected_key_is_coloured() {
        let key: Key = "A minor".parse().unwrap();
        assert_eq!(
            emphasis_for(&key, &Relation::ALL, false),
            [(position("A minor"), Emphasis::Selected)]
        );
    }

    #[test]
    fn the_same_key_move_never_takes_the_selected_cell() {
        assert_eq!(
            emphasized_codes("A minor", &[Relation::Same]),
            [("8A".to_string(), Emphasis::Selected)]
        );
    }

    #[test]
    fn a_matching_cell_names_the_move_that_reaches_it() {
        let b_flat_minor = key_at(position("Bb minor"));
        assert_eq!(
            cell_description(
                "3A",
                b_flat_minor,
                Some(Emphasis::Matching(Relation::Adjacent))
            ),
            "3A, Bb minor, one step on the wheel"
        );
        assert_eq!(
            cell_description("3A", b_flat_minor, Some(Emphasis::Selected)),
            "3A, Bb minor"
        );
        assert_eq!(cell_description("3A", b_flat_minor, None), "3A, Bb minor");
        assert_eq!(
            cell_id("3A", Some(Emphasis::Matching(Relation::Adjacent))),
            "camelot-matching-3A"
        );
        assert_eq!(cell_id("3A", None), "camelot-key-3A");
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
    fn every_key_has_a_cell_and_a_ring_of_its_own_on_its_ring() {
        let centre = point(px(0.0), px(0.0));
        for number in 1..=12_u8 {
            for is_major in [false, true] {
                let position = CamelotPosition { number, is_major };
                assert!(cell_path(PathBuilder::fill(), centre, position, 1.0).is_some());
                assert!(cell_path(PathBuilder::stroke(px(1.5)), centre, position, 1.0).is_some());
            }
        }
    }

    #[test]
    fn matching_keys_stand_out_and_every_label_reads_in_every_theme() {
        for name in ThemeName::ALL {
            let colours = WheelColours::of(&AppTheme::named(name).palette);
            let matching_against_quiet = contrast(colours.matching, colours.quiet);
            assert!(
                matching_against_quiet >= 1.5,
                "{name:?}: matching fill against the other keys {matching_against_quiet:.2}"
            );
            let selected_against_matching = contrast(colours.selected, colours.matching);
            assert!(
                selected_against_matching >= 1.5,
                "{name:?}: selected fill against a matching one {selected_against_matching:.2}"
            );
            let ring_against_quiet = contrast(colours.matching_ring, colours.quiet);
            assert!(
                ring_against_quiet >= 3.0,
                "{name:?}: ring against the other keys {ring_against_quiet:.2}"
            );
            for (label, fill, what) in [
                (colours.selected_label, colours.selected, "selected"),
                (colours.matching_label, colours.matching, "matching"),
                (colours.quiet_label, colours.quiet, "quiet"),
            ] {
                let ratio = contrast(label, fill);
                assert!(ratio >= 4.5, "{name:?}: {what} label {ratio:.2}");
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
