// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tag editor over the window: a field for each tag with the value the
//! selected tracks share or "mixed", a way to remove a field, and a text
//! tool that finds and replaces or adds or removes a prefix or a suffix in
//! one field.

use euphoria_core::tags::TagField;
use gpui::component::input::Input;
use gpui::prelude::*;
use gpui::{AnyElement, Context, FontWeight, TestSupportExt, div, px};

use crate::app::EuphoriaApp;
use crate::format;
use crate::tag_editor::{FieldValue, TagEditor, TextTool};
use crate::theme::Palette;
use crate::views::tag_panel::choice_button;
use crate::views::{SMALL_TEXT_SIZE, primary_button, secondary_button, section_label};

/// The element id of the text field of `field`.
pub(crate) fn field_input_id(field: TagField) -> (&'static str, usize) {
    ("edit-field", field_position(field))
}

/// The element id of the button that removes `field`.
pub(crate) fn clear_button_id(field: TagField) -> (&'static str, usize) {
    ("clear-field", field_position(field))
}

fn field_position(field: TagField) -> usize {
    TagField::ALL
        .iter()
        .position(|candidate| *candidate == field)
        .unwrap_or_default()
}

/// What the tag panel shows while the editor is open.
pub(crate) fn contents(
    editor: &TagEditor,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Vec<AnyElement> {
    let track_count = editor.tracks.len();
    let heading = div()
        .text_size(px(16.0))
        .font_weight(FontWeight::SEMIBOLD)
        .child(match track_count {
            1 => "Edit tags of 1 track".to_string(),
            count => format!("Edit tags of {} tracks", format::count(count)),
        })
        .into_any_element();
    let explanation = div()
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .child(
            "Each field shows the value the tracks share, or mixed when they differ. Only the \
             fields you change are written, after a preview of every change, and Undo puts the \
             old tags back. A comment keeps its key and energy label.",
        )
        .into_any_element();

    let Some(shown) = &editor.shown else {
        return vec![
            heading,
            explanation,
            div()
                .id("reading-tags")
                .text_color(palette.text_tertiary)
                .child("Reading the tags\u{2026}")
                .test_support()
                .into_any_element(),
            footer(editor, palette, cx),
        ];
    };

    let rows = shown.iter().map(|(field, value)| {
        let field = *field;
        let is_cleared = editor.cleared.contains(&field);
        let input = editor.input(field).map(|input| {
            div()
                .flex_1()
                .min_w_0()
                .when(is_cleared, |input| input.opacity(0.4))
                .child(Input::new(input).id(field_input_id(field)))
        });
        let hover_colour = palette.text;
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(
                div()
                    .w(px(96.0))
                    .flex_shrink_0()
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(palette.text_secondary)
                    .child(field.label())
                    .when(*value == FieldValue::Mixed, |label| {
                        label.child(
                            div()
                                .text_size(px(10.0))
                                .text_color(palette.text_tertiary)
                                .child("differs"),
                        )
                    }),
            )
            .children(input)
            .child(
                div()
                    .id(clear_button_id(field))
                    .w(px(64.0))
                    .flex_shrink_0()
                    .text_size(SMALL_TEXT_SIZE)
                    .text_color(if is_cleared {
                        palette.danger
                    } else {
                        palette.text_tertiary
                    })
                    .cursor_pointer()
                    .hover(move |style| style.text_color(hover_colour))
                    .child(if is_cleared { "Keep" } else { "Remove" })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_cleared_field(field, cx);
                    }))
                    .test_support(),
            )
            .into_any_element()
    });

    let tools = TextTool::ALL.iter().enumerate().map(|(index, tool)| {
        let tool = *tool;
        choice_button(
            ("text-tool", index),
            tool.label(),
            editor.tool == tool,
            palette,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.choose_text_tool(tool, cx)))
        .test_support()
    });
    let tool_fields = TagField::ALL.iter().map(|field| {
        let field = *field;
        choice_button(
            ("tool-field", field_position(field)),
            field.label(),
            editor.tool_field == field,
            palette,
        )
        .on_click(cx.listener(move |this, _, _, cx| this.choose_tool_field(field, cx)))
        .test_support()
    });
    let is_replace = editor.tool == TextTool::Replace;
    let text_tool = div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .pt(px(6.0))
        .child(section_label("Change text in one field", palette))
        .child(div().flex().flex_wrap().gap(px(6.0)).children(tools))
        .child(div().flex().flex_wrap().gap(px(4.0)).children(tool_fields))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .child(Input::new(&editor.tool_text).id("tool-text")),
                )
                .when(is_replace, |row| {
                    row.child(
                        div()
                            .flex_1()
                            .child(Input::new(&editor.tool_with).id("tool-with")),
                    )
                    .child(
                        choice_button("match-case", "Match case", editor.match_case, palette)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_match_case(cx)))
                            .test_support(),
                    )
                }),
        );

    let mut contents = vec![
        heading,
        explanation,
        div()
            .id("tag-editor-fields")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(px(6.0))
            .overflow_y_scroll()
            .children(rows)
            .child(text_tool)
            .into_any_element(),
    ];
    if !editor.unreadable.is_empty() {
        contents.push(
            div()
                .id("unreadable-tags")
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.danger)
                .child(format!(
                    "{} could not be read and will be left out.",
                    match editor.unreadable.len() {
                        1 => "1 file".to_string(),
                        count => format!("{} files", format::count(count)),
                    }
                ))
                .test_support()
                .into_any_element(),
        );
    }
    contents.push(footer(editor, palette, cx));
    contents
}

/// A problem with the edit, if any, and the buttons.
fn footer(editor: &TagEditor, palette: &Palette, cx: &mut Context<'_, EuphoriaApp>) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .children(editor.problem.clone().map(|problem| {
            div()
                .id("edit-problem")
                .text_size(SMALL_TEXT_SIZE)
                .text_color(palette.danger)
                .child(problem)
                .test_support()
        }))
        .child(div().flex_1())
        .child(
            secondary_button("Cancel", palette)
                .id("cancel-tag-edit")
                .on_click(cx.listener(|this, _, _, cx| this.close_tag_panel(cx)))
                .test_support(),
        )
        .child(
            primary_button("Preview changes", palette)
                .id("preview-tag-edits")
                .when(editor.shown.is_none(), |button| {
                    button.opacity(0.5).cursor_default()
                })
                .on_click(cx.listener(|this, _, _, cx| this.preview_tag_edits(cx)))
                .test_support(),
        )
        .into_any_element()
}
