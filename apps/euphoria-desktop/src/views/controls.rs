// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The one vocabulary of controls: a button among a few choices, a joined
//! segmented control for pick-one settings, a row with a switch for every
//! on/off setting, a pressed toggle button, and the text link. A selected
//! choice is an accent tint with accent text; solid accent is for the one
//! primary action of a screen.

use std::cell::RefCell;
use std::collections::HashMap;

use gpui::prelude::*;
use gpui::{
    AnyElement, Div, ElementId, FontWeight, Hsla, SharedString, Stateful, TestSupportExt, div, px,
};

use crate::motion::{self, Transition};
use crate::theme::{Palette, blend};
use crate::views::{
    CONTROL_HEIGHT, GAP_MEDIUM, GAP_SMALL, GAP_TIGHT, RADIUS_LARGE, RADIUS_MEDIUM, SMALL_TEXT_SIZE,
    TEXT_SIZE,
};

/// Width of a switch.
pub(crate) const SWITCH_WIDTH: f32 = 32.0;
/// Height of a switch.
pub(crate) const SWITCH_HEIGHT: f32 = 18.0;
/// Width and height of a switch's knob.
pub(crate) const KNOB_SIZE: f32 = 14.0;
/// Room between a switch's edge and its knob.
pub(crate) const KNOB_INSET: f32 = 2.0;

/// What a chip is for, which decides how loud it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChipRole {
    /// The choice that is on among others: an accent tint.
    Selected,
    /// The one primary action of a screen: solid accent.
    Primary,
    /// A choice that is not taken.
    Idle,
}

/// The colours of a chip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ChipLook {
    pub(crate) background: Option<Hsla>,
    pub(crate) text: Hsla,
    pub(crate) border: Hsla,
}

/// The look of a chip of `role`.
pub(crate) fn chip_look(role: ChipRole, palette: &Palette) -> ChipLook {
    match role {
        ChipRole::Selected => ChipLook {
            background: Some(Palette::tint(palette.accent)),
            text: palette.accent,
            border: palette.accent.opacity(0.32),
        },
        ChipRole::Primary => ChipLook {
            background: Some(palette.accent),
            text: palette.accent_ink,
            border: palette.accent,
        },
        ChipRole::Idle => ChipLook {
            background: None,
            text: palette.text_secondary,
            border: palette.line,
        },
    }
}

/// A button among a few choices: tinted when it is the one chosen.
pub(crate) fn choice_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    is_chosen: bool,
    palette: &Palette,
) -> Stateful<Div> {
    let role = if is_chosen {
        ChipRole::Selected
    } else {
        ChipRole::Idle
    };
    let look = chip_look(role, palette);
    let hover_background = palette.raised;
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(CONTROL_HEIGHT)
        .px(GAP_MEDIUM)
        .rounded(RADIUS_LARGE)
        .border_1()
        .border_color(look.border)
        .text_size(SMALL_TEXT_SIZE)
        .text_color(look.text)
        .cursor_pointer()
        .map(|button| match look.background {
            Some(background) => button.bg(background).font_weight(FontWeight::MEDIUM),
            None => button.hover(move |style| style.bg(hover_background)),
        })
        .child(label.into())
}

/// A button that stays down while something is on: the raised surface with
/// accent text.
pub(crate) fn pressed_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    is_pressed: bool,
    palette: &Palette,
) -> Stateful<Div> {
    let hover_background = palette.line;
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(CONTROL_HEIGHT)
        .px(GAP_MEDIUM)
        .rounded(RADIUS_MEDIUM)
        .border_1()
        .border_color(palette.line)
        .text_size(SMALL_TEXT_SIZE)
        .cursor_pointer()
        .map(|button| {
            if is_pressed {
                button
                    .bg(palette.raised)
                    .text_color(palette.accent)
                    .font_weight(FontWeight::SEMIBOLD)
            } else {
                button
                    .text_color(palette.text_secondary)
                    .hover(move |style| style.bg(hover_background))
            }
        })
        .child(label.into())
}

/// The frame of a joined segmented control, which holds [`segment`]s.
pub(crate) fn segmented(segments: Vec<AnyElement>, palette: &Palette) -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .self_start()
        .h(CONTROL_HEIGHT)
        .rounded(RADIUS_MEDIUM)
        .border_1()
        .border_color(palette.line)
        .overflow_hidden()
        .children(segments)
}

/// One segment of a segmented control, tinted when it is the one chosen.
pub(crate) fn segment(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    is_selected: bool,
    is_first: bool,
    palette: &Palette,
) -> Stateful<Div> {
    let hover_background = palette.raised;
    div()
        .id(id)
        .flex()
        .items_center()
        .px(GAP_MEDIUM)
        .text_size(SMALL_TEXT_SIZE)
        .cursor_pointer()
        .when(!is_first, |segment| {
            segment.border_l_1().border_color(palette.line)
        })
        .map(|segment| {
            if is_selected {
                segment
                    .bg(Palette::tint(palette.accent))
                    .text_color(palette.accent)
                    .font_weight(FontWeight::MEDIUM)
            } else {
                segment
                    .text_color(palette.text_secondary)
                    .hover(move |style| style.bg(hover_background))
            }
        })
        .child(label.into())
}

/// A link in running text or beside a control: accent text, underlined on
/// hover.
pub(crate) fn text_link(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    palette: &Palette,
) -> Stateful<Div> {
    div()
        .id(id)
        .cursor_pointer()
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.accent)
        .hover(|style| style.underline())
        .child(label.into())
}

/// The colours of a switch `position` of the way from off (0) to on (1):
/// its track, its knob and its border.
pub(crate) fn switch_colours(position: f32, palette: &Palette) -> (Hsla, Hsla, Hsla) {
    let position = position.clamp(0.0, 1.0);
    (
        blend(palette.accent, palette.raised, 0.35 * position),
        blend(palette.accent, palette.text_tertiary, position),
        blend(palette.accent, palette.line, 0.6 * position),
    )
}

/// Where the left edge of the knob is, in pixels from the switch's edge.
pub(crate) fn knob_left(position: f32) -> f32 {
    KNOB_INSET + position.clamp(0.0, 1.0) * (SWITCH_WIDTH - KNOB_SIZE - 2.0 * KNOB_INSET)
}

/// The sliding of every switch on show, by the id of its row.
pub(crate) struct SwitchSlides {
    switches: RefCell<HashMap<String, Transition>>,
    start: Box<dyn Fn(bool) -> Transition>,
}

impl Default for SwitchSlides {
    fn default() -> Self {
        Self {
            switches: RefCell::new(HashMap::new()),
            start: Box::new(Transition::at_rest),
        }
    }
}

impl SwitchSlides {
    #[cfg(test)]
    pub(crate) fn with_sources(make: impl Fn() -> motion::Sources + 'static) -> Self {
        Self {
            switches: RefCell::new(HashMap::new()),
            start: Box::new(move |is_on| Transition::at_rest(is_on).with_sources(make())),
        }
    }

    /// How far on the switch `id` is, from 0 to 1, now. A switch seen for
    /// the first time is where it should be; one whose state changed
    /// slides over [`motion::SWITCH`], or jumps when motion is reduced.
    pub(crate) fn position(&self, id: &str, is_on: bool) -> f32 {
        let mut switches = self.switches.borrow_mut();
        let transition = switches
            .entry(id.to_string())
            .or_insert_with(|| (self.start)(is_on));
        if transition.is_target_full() != is_on {
            transition.go_to(is_on, motion::SWITCH);
        }
        transition.value(transition.now())
    }

    /// True while any switch still slides, so another frame is due; the
    /// switches that have arrived are settled.
    pub(crate) fn wants_frame(&self) -> bool {
        let mut is_moving = false;
        for transition in self.switches.borrow_mut().values_mut() {
            let now = transition.now();
            transition.settle(now);
            is_moving |= transition.is_moving(now);
        }
        is_moving
    }
}

/// A row for an on/off setting: its label, a line on what it does, and a
/// switch. The whole row is the click target; the caller adds `on_click`.
pub(crate) fn toggle_row(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    helper: impl Into<SharedString>,
    is_on: bool,
    slides: &SwitchSlides,
    palette: &Palette,
) -> Stateful<Div> {
    let id: ElementId = id.into();
    let key = id.to_string();
    let position = slides.position(&key, is_on);
    let (track, knob, border) = switch_colours(position, palette);
    let hover_background = palette.raised;
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .gap(GAP_MEDIUM)
        .px(GAP_SMALL)
        .py(GAP_SMALL)
        .rounded(RADIUS_MEDIUM)
        .cursor_pointer()
        .hover(move |style| style.bg(hover_background))
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .gap(GAP_TIGHT)
                .child(
                    div()
                        .text_size(TEXT_SIZE)
                        .text_color(palette.text)
                        .child(label.into()),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("hint-{key}")))
                        .text_size(SMALL_TEXT_SIZE)
                        .text_color(palette.text_secondary)
                        .child(helper.into())
                        .test_support(),
                ),
        )
        .child(
            div()
                .id(SharedString::from(format!("{key}-switch")))
                .relative()
                .flex_shrink_0()
                .w(px(SWITCH_WIDTH))
                .h(px(SWITCH_HEIGHT))
                .rounded_full()
                .border_1()
                .border_color(border)
                .bg(track)
                .child(
                    div()
                        .absolute()
                        .top(px(KNOB_INSET - 1.0))
                        .left(px(knob_left(position) - 1.0))
                        .size(px(KNOB_SIZE))
                        .rounded_full()
                        .bg(knob),
                )
                .test_support(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion::testing::FakeClock;
    use crate::theme::{AppTheme, ThemeName};

    fn close(left: f32, right: f32) -> bool {
        (left - right).abs() < 0.001
    }

    #[test]
    fn a_selected_chip_is_a_tint_with_accent_text_and_only_the_primary_is_solid() {
        for name in ThemeName::ALL {
            let palette = AppTheme::named(name).palette;
            let selected = chip_look(ChipRole::Selected, &palette);
            let background = selected.background.unwrap();
            assert_eq!(selected.text, palette.accent, "{name:?}");
            assert!(
                (background.a - crate::theme::TINT_STRENGTH).abs() < 0.001,
                "{name:?}: a tint is about 16% accent"
            );
            assert_eq!(background.h, palette.accent.h, "{name:?}");
            let primary = chip_look(ChipRole::Primary, &palette);
            assert_eq!(primary.background, Some(palette.accent), "{name:?}");
            assert_eq!(primary.text, palette.accent_ink, "{name:?}");
            let idle = chip_look(ChipRole::Idle, &palette);
            assert_eq!(idle.background, None, "{name:?}");
        }
    }

    #[test]
    fn the_switch_is_32_by_18_with_a_knob_that_travels_edge_to_edge() {
        assert_eq!((SWITCH_WIDTH, SWITCH_HEIGHT), (32.0, 18.0));
        assert!(close(knob_left(0.0), KNOB_INSET));
        assert!(close(knob_left(1.0), SWITCH_WIDTH - KNOB_SIZE - KNOB_INSET));
        assert!(knob_left(0.5) > knob_left(0.0) && knob_left(0.5) < knob_left(1.0));
    }

    #[test]
    fn the_knob_stands_out_from_its_track_on_and_off_in_every_theme() {
        let luminance = |colour: Hsla| {
            let rgb = colour.to_rgb();
            let linear = |value: f32| {
                if value <= 0.03928 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(rgb.r) + 0.7152 * linear(rgb.g) + 0.0722 * linear(rgb.b)
        };
        for name in ThemeName::ALL {
            let palette = AppTheme::named(name).palette;
            for position in [0.0, 1.0] {
                let (track, knob, _) = switch_colours(position, &palette);
                let (light, dark) = (
                    luminance(track).max(luminance(knob)),
                    luminance(track).min(luminance(knob)),
                );
                let ratio = (light + 0.05) / (dark + 0.05);
                assert!(ratio >= 1.5, "{name:?} at {position}: {ratio:.2}");
            }
        }
    }

    #[test]
    fn a_switch_slides_over_150_ms_and_is_still_while_nothing_changes() {
        let clock = std::rc::Rc::new(FakeClock::new());
        let handle = clock.clone();
        let slides = SwitchSlides::with_sources(move || handle.sources());
        assert_eq!(slides.position("accent", false), 0.0);
        assert!(!slides.wants_frame());
        clock.at(0.0);
        assert_eq!(slides.position("accent", true), 0.0, "starts where it was");
        assert!(slides.wants_frame());
        clock.at(0.075);
        let halfway = slides.position("accent", true);
        assert!(halfway > 0.5 && halfway < 1.0, "{halfway}");
        clock.at(0.151);
        assert!(close(slides.position("accent", true), 1.0));
        assert!(!slides.wants_frame());
        clock.at(5.0);
        assert!(close(slides.position("accent", true), 1.0));
    }

    #[test]
    fn a_switch_first_seen_on_is_already_on() {
        let clock = FakeClock::new();
        let handle = std::rc::Rc::new(clock);
        let again = handle.clone();
        let slides = SwitchSlides::with_sources(move || again.sources());
        assert_eq!(slides.position("tempo", true), 1.0);
    }

    #[test]
    fn with_reduced_motion_a_switch_jumps() {
        let clock = std::rc::Rc::new(FakeClock::new());
        clock.reduce.set(true);
        let handle = clock.clone();
        let slides = SwitchSlides::with_sources(move || handle.sources());
        assert_eq!(slides.position("sound", false), 0.0);
        clock.at(0.0);
        assert_eq!(slides.position("sound", true), 1.0);
        assert!(!slides.wants_frame());
        assert_eq!(slides.position("sound", false), 0.0);
        assert!(!slides.wants_frame());
    }
}
