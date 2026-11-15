// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A strip along the bottom of the window that reports a problem.

use gpui::prelude::*;
use gpui::{
    AnyElement, Bounds, Context, Div, Pixels, SharedString, Stateful, TestSupportExt, canvas, div,
    px,
};

use crate::app::EuphoriaApp;
use crate::surface_motion::{Strip, UNMEASURED_STRIP_HEIGHT};
use crate::views::SMALL_TEXT_SIZE;

/// `strip` with its height grown to the share of its full height that its
/// appearance has reached. The strip keeps its full height inside the clip,
/// so its text does not reflow while it grows.
fn grown(app: &EuphoriaApp, which: Strip, strip: impl IntoElement) -> AnyElement {
    let fraction = app.strip_growth.value(which);
    let seen_height = app.strip_growth.seen_height(which);
    let natural_height = seen_height.get().unwrap_or(UNMEASURED_STRIP_HEIGHT);
    div()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .overflow_hidden()
        .when(fraction < 1.0, |clip| clip.h(px(natural_height * fraction)))
        .child(
            div()
                .relative()
                .flex()
                .flex_col()
                .flex_shrink_0()
                .child(strip)
                .child(
                    canvas(
                        move |bounds: Bounds<Pixels>, _, _| {
                            seen_height.set(Some(bounds.size.height / px(1.0)));
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                ),
        )
        .into_any_element()
}

/// The notice strip, when there is a problem to report.
pub(crate) fn render(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Option<AnyElement> {
    render_strip(app, cx).map(|strip| grown(app, Strip::Notice, strip))
}

fn render_strip(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Option<Stateful<Div>> {
    let palette = app.theme.palette;
    let message = app.notice.clone()?;
    Some(
        div()
            .id("notice")
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(12.0))
            .px(px(14.0))
            .py(px(8.0))
            .border_t_1()
            .border_color(palette.line)
            .bg(palette.sidebar)
            .text_size(SMALL_TEXT_SIZE)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(palette.danger)
                    .child(message),
            )
            .child(
                div()
                    .id("dismiss-notice")
                    .flex_shrink_0()
                    .px(px(8.0))
                    .py(px(2.0))
                    .rounded(px(4.0))
                    .text_color(palette.text_secondary)
                    .cursor_pointer()
                    .hover(move |style| style.bg(palette.raised).text_color(palette.text))
                    .on_click(cx.listener(|this, _, _, cx| this.dismiss_notice(cx)))
                    .child("Dismiss"),
            ),
    )
}

/// The strip that says tracks are ready to tag but a DJ program is running,
/// and offers to write them anyway.
pub(crate) fn render_held_tags(
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<AnyElement> {
    render_held_tags_strip(app, cx).map(|strip| grown(app, Strip::HeldTags, strip))
}

fn render_held_tags_strip(
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<impl IntoElement> {
    let palette = app.theme.palette;
    let programs = app.auto_tags.held_by();
    if programs.is_empty() || app.auto_tags.pending_count() == 0 {
        return None;
    }
    let message: SharedString =
        crate::auto_tags::held_sentence(app.auto_tags.pending_count(), programs).into();
    let button = |id: &'static str, label: &'static str| {
        div()
            .id(id)
            .flex_shrink_0()
            .px(px(8.0))
            .py(px(2.0))
            .rounded(px(4.0))
            .text_color(palette.text_secondary)
            .cursor_pointer()
            .hover(move |style| style.bg(palette.raised).text_color(palette.text))
            .child(label)
    };
    Some(
        div()
            .id("held-tags")
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(12.0))
            .px(px(14.0))
            .py(px(8.0))
            .border_t_1()
            .border_color(palette.line)
            .bg(palette.sidebar)
            .text_size(SMALL_TEXT_SIZE)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(palette.text)
                    .child(message),
            )
            .child(
                button("write-held-tags", "Write now")
                    .on_click(cx.listener(|this, _, _, cx| this.write_held_tags_now(cx)))
                    .test_support(),
            )
            .child(
                button("dismiss-held-tags", "Dismiss")
                    .on_click(cx.listener(|this, _, _, cx| this.dismiss_held_tags(cx)))
                    .test_support(),
            )
            .test_support(),
    )
}

/// The strip that asks whether to link a missing track to a file Euphoria
/// cannot check, while one waits for an answer.
pub(crate) fn render_link_confirmation(
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<AnyElement> {
    render_link_confirmation_strip(app, cx).map(|strip| grown(app, Strip::LinkConfirmation, strip))
}

fn render_link_confirmation_strip(
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<Stateful<Div>> {
    let palette = app.theme.palette;
    let link = app.pending_link.as_ref()?;
    let file_name = |path: &std::path::Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    };
    let question: SharedString = format!(
        "Euphoria can\u{2019}t check that {} is the same track as the missing {}. Link it anyway?",
        file_name(&link.picked),
        file_name(&link.old)
    )
    .into();
    let button = |id: &'static str, label: &'static str| {
        div()
            .id(id)
            .flex_shrink_0()
            .px(px(8.0))
            .py(px(2.0))
            .rounded(px(4.0))
            .text_color(palette.text_secondary)
            .cursor_pointer()
            .hover(move |style| style.bg(palette.raised).text_color(palette.text))
            .child(label)
    };
    Some(
        div()
            .id("link-confirmation")
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(12.0))
            .px(px(14.0))
            .py(px(8.0))
            .border_t_1()
            .border_color(palette.line)
            .bg(palette.sidebar)
            .text_size(SMALL_TEXT_SIZE)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(palette.text)
                    .child(question),
            )
            .child(
                button("link-anyway", "Link anyway")
                    .on_click(cx.listener(|this, _, _, cx| this.confirm_pending_link(cx))),
            )
            .child(
                button("cancel-link", "Cancel")
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_pending_link(cx))),
            ),
    )
}

/// The strip that asks whether to replace the files an export of stems
/// would overwrite, while one waits for an answer.
pub(crate) fn render_stem_export_confirmation(
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<AnyElement> {
    render_stem_export_confirmation_strip(app, cx).map(|strip| grown(app, Strip::StemExport, strip))
}

fn render_stem_export_confirmation_strip(
    app: &EuphoriaApp,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<impl IntoElement> {
    let palette = app.theme.palette;
    let request = app.pending_stem_export.as_ref()?;
    let existing = request.existing();
    let first = existing
        .first()
        .and_then(|path| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let question: SharedString = match existing.len() {
        0 | 1 => format!("{first} already exists. Replace it?"),
        count => format!(
            "{first} and {} more files already exist. Replace them?",
            count - 1
        ),
    }
    .into();
    let button = |id: &'static str, label: &'static str| {
        div()
            .id(id)
            .flex_shrink_0()
            .px(px(8.0))
            .py(px(2.0))
            .rounded(px(4.0))
            .text_color(palette.text_secondary)
            .cursor_pointer()
            .hover(move |style| style.bg(palette.raised).text_color(palette.text))
            .child(label)
    };
    Some(
        div()
            .id("stem-export-confirmation")
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(12.0))
            .px(px(14.0))
            .py(px(8.0))
            .border_t_1()
            .border_color(palette.line)
            .bg(palette.sidebar)
            .text_size(SMALL_TEXT_SIZE)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(palette.text)
                    .child(question),
            )
            .child(
                button("replace-stem-export", "Replace")
                    .on_click(cx.listener(|this, _, _, cx| this.confirm_pending_stem_export(cx)))
                    .test_support(),
            )
            .child(
                button("cancel-stem-export", "Cancel")
                    .on_click(cx.listener(|this, _, _, cx| this.cancel_pending_stem_export(cx)))
                    .test_support(),
            )
            .test_support(),
    )
}
