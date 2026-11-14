// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The collapsible sections of the track detail: a heading with a
//! disclosure chevron, as in the sidebar's folder tree, over contents whose
//! height eases open and shut. In the Deck strip the same sections are tabs.

use gpui::component::{Icon, IconName};
use gpui::prelude::*;
use gpui::{Bounds, Context, Div, Pixels, TestSupportExt, canvas, div, px};

use crate::app::EuphoriaApp;
use crate::inspector_sections::SectionKind;
use crate::views::{LABEL_TEXT_SIZE, next_rail, section_label};

/// Width of the box the chevron sits in, as in the sidebar's tree.
const DISCLOSURE_WIDTH: f32 = 16.0;

/// How tall to take the contents of a section that was never drawn, until
/// they are measured.
const UNMEASURED_HEIGHT: f32 = 120.0;

/// The element id of the heading that toggles `kind`.
pub(crate) fn toggle_id(kind: SectionKind) -> (&'static str, usize) {
    ("section-toggle", kind.index())
}

/// The element id of the tab that shows `kind` in the Deck strip.
pub(crate) fn tab_id(kind: SectionKind) -> (&'static str, usize) {
    ("section-tab", kind.index())
}

/// `body` under a heading that opens and closes it, or nothing when the
/// section has no body for this track. The contents keep the height they
/// have while the clip around them grows or shrinks, so nothing reflows
/// mid-motion.
pub(crate) fn collapsible(
    kind: SectionKind,
    app: &EuphoriaApp,
    body: Option<Div>,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<Div> {
    let body = body?;
    let palette = app.theme.palette;
    let fraction = app.inspector_sections.fraction(kind);
    let is_open = app.inspector_sections.motion(kind).is_target_open();
    let seen_height = app.inspector_sections.heights[kind.index()].clone();
    let natural_height = seen_height.get().unwrap_or(UNMEASURED_HEIGHT);
    let hover_background = palette.raised;

    let title = if kind == SectionKind::Mixes {
        app.mixes_title()
    } else {
        kind.title().to_string()
    };
    let toggle = div()
        .id(toggle_id(kind))
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap(px(4.0))
        .h(px(24.0))
        .rounded(px(4.0))
        .cursor_pointer()
        .hover(move |style| style.bg(hover_background))
        .on_click(cx.listener(move |this, _, _, cx| this.toggle_section(kind, cx)))
        .child(
            div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .size(px(DISCLOSURE_WIDTH))
                .text_color(palette.text_tertiary)
                .child(
                    Icon::new(if is_open {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    })
                    .size(px(12.0)),
                ),
        )
        .child(section_label(title, &palette).flex_1().min_w_0().truncate())
        .test_support();
    let heading = div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(toggle)
        .when(kind == SectionKind::Mixes, |heading| {
            heading.children(next_rail::control(app, cx))
        });

    let measured = div()
        .relative()
        .flex_shrink_0()
        .pt(px(6.0))
        .pb(px(8.0))
        .child(body)
        .child(
            canvas(
                move |bounds: Bounds<Pixels>, _, _| {
                    seen_height.set(Some(bounds.size.height / px(1.0)));
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        );

    Some(
        div()
            .flex()
            .flex_col()
            .child(heading)
            .when(fraction > 0.0, |section| {
                section.child(
                    div()
                        .id(("section-body", kind.index()))
                        .flex()
                        .flex_col()
                        .overflow_hidden()
                        .when(fraction < 1.0, |clip| clip.h(px(natural_height * fraction)))
                        .child(measured)
                        .test_support(),
                )
            }),
    )
}

/// The tabs at the right of the Deck strip, one per section, the one shown
/// marked.
pub(crate) fn tabs(
    app: &EuphoriaApp,
    shown: SectionKind,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Div {
    let palette = app.theme.palette;
    div()
        .flex()
        .flex_shrink_0()
        .flex_wrap()
        .gap(px(4.0))
        .w_full()
        .children(SectionKind::ALL.into_iter().map(|kind| {
            let is_shown = kind == shown;
            let hover_background = palette.raised;
            div()
                .id(tab_id(kind))
                .flex()
                .items_center()
                .h(px(22.0))
                .px(px(8.0))
                .rounded(px(11.0))
                .border_1()
                .whitespace_nowrap()
                .text_size(LABEL_TEXT_SIZE)
                .cursor_pointer()
                .map(|chip| {
                    if is_shown {
                        chip.border_color(palette.accent)
                            .bg(palette.accent)
                            .text_color(palette.accent_ink)
                    } else {
                        chip.border_color(palette.line)
                            .text_color(palette.text_secondary)
                            .hover(move |style| style.bg(hover_background))
                    }
                })
                .child(kind.tab_title())
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.strip_section = Some(kind);
                    cx.notify();
                }))
                .test_support()
        }))
}
