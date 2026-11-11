// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The panel that previews a tag write, asks for the go ahead, shows the
//! progress and reports each file, and the same for an undo.

use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{
    AnyElement, Context, Div, FontWeight, SharedString, TestSupportExt, div, px, relative,
    uniform_list,
};

use crate::app::EuphoriaApp;
use crate::format;
use crate::tag_flow::{Preview, PreviewKind, TagPanel, WriteScope};
use crate::tag_settings::TagSettings;
use crate::tag_writing::{LeftOut, PreviewRow};
use crate::theme::Palette;
use crate::views::{SMALL_TEXT_SIZE, modal, primary_button, secondary_button};

/// Height of one line of the preview list.
const PREVIEW_ROW_HEIGHT: f32 = 24.0;

/// The tag panel, while a write or an undo is on screen.
pub(crate) fn render(app: &EuphoriaApp, cx: &mut Context<'_, EuphoriaApp>) -> Option<AnyElement> {
    let panel = app.tag_panel.as_ref()?;
    let palette = app.theme.palette;
    let (backdrop, card) = modal("tag-panel", px(760.0), &palette);
    let card = card.id("tag-panel-card");
    let contents: Vec<AnyElement> = match panel {
        TagPanel::Planning(scope) => vec![
            title(&format!("Write tags to {}", scope_words(app, *scope))),
            quiet(
                "Working out what would change. Nothing is written yet.",
                &palette,
            ),
            buttons(vec![cancel_button("cancel-tag-write", &palette, cx)]),
        ],
        TagPanel::Preview(preview) => preview_contents(app, preview, &palette, cx),
        TagPanel::Editing(editor) => crate::views::tag_editor::contents(editor, &palette, cx),
        TagPanel::Writing { .. } => {
            let (done, total) = app.tag_write_progress().unwrap_or_default();
            vec![
                title("Writing tags"),
                quiet(
                    &format!(
                        "{} of {} done. Each file's backup is saved before it is written.",
                        format::count(done),
                        counted(total, "file")
                    ),
                    &palette,
                ),
                progress_bar(done, total, &palette),
            ]
        }
        TagPanel::Written(report) => {
            let mut contents = vec![
                title(&match report.written.len() {
                    0 => "No files were written".to_string(),
                    written => format!("Tags written to {}", counted(written, "file")),
                }),
                quiet(
                    "Undo puts back the exact tags these files had, from this window or with \
                     euphoria tags undo.",
                    &palette,
                ),
            ];
            contents.extend(problems(
                "Not written, left as they were",
                ProblemList::NotWritten,
                report.failures.len(),
                &palette,
                cx,
            ));
            contents.extend(problems(
                "Written, but the log could not note it; undo still restores them",
                ProblemList::NotNoted,
                report.not_noted.len(),
                &palette,
                cx,
            ));
            if let Some(problem) = &report.cache_problem {
                contents.push(quiet(
                    &format!(
                        "The analysis cache could not be updated, so these files will be \
                         analyzed again the next time they are added: {problem}"
                    ),
                    &palette,
                ));
            }
            contents.push(buttons(vec![
                secondary_button("Undo this write", &palette)
                    .id("undo-tag-write")
                    .on_click(cx.listener(|this, _, _, cx| this.open_tag_undo(cx)))
                    .test_support()
                    .into_any_element(),
                done_button(&palette, cx),
            ]));
            contents
        }
        TagPanel::ConfirmUndo => vec![
            title("Undo the last tag write?"),
            quiet(
                "The files it changed get back the exact tags they had, whether the write was \
                 made here or with euphoria tags write. A file whose tags were changed since is \
                 left alone and listed.",
                &palette,
            ),
            buttons(vec![
                cancel_button("cancel-tag-undo", &palette, cx),
                primary_button("Undo write", &palette)
                    .id("confirm-tag-undo")
                    .on_click(cx.listener(|this, _, _, cx| this.confirm_tag_undo(cx)))
                    .test_support()
                    .into_any_element(),
            ]),
        ],
        TagPanel::Undoing => vec![
            title("Undoing the last write"),
            quiet("Putting the old tags back.", &palette),
        ],
        TagPanel::Undone(outcome) => {
            let mut contents = match outcome {
                Ok(None) => vec![title("There is no tag write to undo")],
                Ok(Some(report)) => {
                    let mut contents = vec![title(&format!(
                        "Old tags put back in {}",
                        counted(report.restored.len(), "file")
                    ))];
                    if report.untouched > 0 {
                        contents.push(quiet(
                            &format!(
                                "{} the write had not reached needed nothing.",
                                counted(report.untouched, "file")
                            ),
                            &palette,
                        ));
                    }
                    contents.extend(problems(
                        "Not restored; undo again to try them again",
                        ProblemList::NotRestored,
                        report.failures.len(),
                        &palette,
                        cx,
                    ));
                    contents
                }
                Err(reason) => vec![
                    title("The write could not be undone"),
                    quiet(reason, &palette),
                ],
            };
            contents.push(buttons(vec![done_button(&palette, cx)]));
            contents
        }
    };
    Some(
        backdrop
            .child(card.children(contents).test_support())
            .test_support()
            .into_any_element(),
    )
}

fn preview_contents(
    app: &EuphoriaApp,
    preview: &Preview,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Vec<AnyElement> {
    let file_count = preview.plans.len();
    let mut summary = vec![if file_count == 0 {
        "Nothing to change".to_string()
    } else {
        format!("{} to change", counted(file_count, "file"))
    }];
    if preview.up_to_date > 0 {
        summary.push(format!(
            "{} already up to date",
            format::count(preview.up_to_date)
        ));
    }
    if !preview.not_analyzed.is_empty() {
        summary.push(format!(
            "{} not analyzed",
            format::count(preview.not_analyzed.len())
        ));
    }
    if !preview.failures.is_empty() {
        summary.push(format!(
            "{} cannot be written",
            format::count(preview.failures.len())
        ));
    }

    let mut contents = match &preview.kind {
        PreviewKind::Analysis { scope } => vec![
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.0))
                .child(title(&format!(
                    "Write tags for {}",
                    preview.settings.program.label_in_sentence()
                )))
                .child(
                    secondary_button("Change settings", palette)
                        .id("change-tag-settings")
                        .on_click(cx.listener(|this, _, _, cx| this.change_tag_settings(cx)))
                        .test_support(),
                )
                .into_any_element(),
            quiet(&settings_sentence(&preview.settings), palette),
            scope_switch(app, *scope, palette, cx),
        ],
        PreviewKind::Edit { summary } => vec![
            title("Edit tags"),
            div()
                .id("edit-summary")
                .text_color(palette.text_secondary)
                .child(summary.clone())
                .test_support()
                .into_any_element(),
        ],
    };
    contents.push(
        div()
            .id("tag-summary")
            .text_size(SMALL_TEXT_SIZE)
            .text_color(palette.text_secondary)
            .child(summary.join(", ") + ".")
            .test_support()
            .into_any_element(),
    );
    if !preview.rows.is_empty() {
        contents.push(preview_list(preview, palette, cx));
    }
    contents.extend(problems(
        "Cannot be written",
        ProblemList::CannotBeWritten,
        preview.failures.len(),
        palette,
        cx,
    ));
    contents.extend(problems(
        "Not analyzed yet",
        ProblemList::NotAnalyzed,
        preview.not_analyzed.len(),
        palette,
        cx,
    ));
    contents.push(buttons(vec![
        cancel_button("cancel-tag-write", palette, cx),
        primary_button(
            if file_count == 0 {
                "Nothing to write".to_string()
            } else {
                format!("Write {}", counted(file_count, "file"))
            },
            palette,
        )
        .id("confirm-tag-write")
        .when(file_count == 0, |button| {
            button.opacity(0.5).cursor_default()
        })
        .on_click(cx.listener(|this, _, _, cx| this.confirm_tag_write(cx)))
        .test_support()
        .into_any_element(),
    ]));
    contents
}

/// What the write will do with the settings in use, in one sentence.
fn settings_sentence(settings: &TagSettings) -> String {
    format!(
        "Keys in {}, the energy label {}, the tempo {}. Nothing is written until you confirm.",
        settings.key_notation.label(),
        match settings.label {
            crate::tag_settings::LabelChoice::None => "not written".to_string(),
            place => format!("at the {}", place.label().to_lowercase()),
        },
        if settings.write_tempo {
            "written"
        } else {
            "left alone"
        }
    )
}

/// Two buttons that choose between the selected tracks and every track on
/// show.
fn scope_switch(
    app: &EuphoriaApp,
    scope: WriteScope,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let option =
        |id: &'static str, label: String, choice: WriteScope, cx: &mut Context<'_, EuphoriaApp>| {
            let is_chosen = scope == choice;
            choice_button(id, label, is_chosen, palette)
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !is_chosen {
                        this.plan_tag_write(choice, cx);
                    }
                }))
                .test_support()
                .into_any_element()
        };
    let selected = app.selected_tracks().len();
    let shown = app.visible.len();
    div()
        .flex()
        .gap(px(6.0))
        .child(option(
            "scope-selection",
            format!("Selected tracks ({})", format::count(selected)),
            WriteScope::Selection,
            cx,
        ))
        .child(option(
            "scope-shown",
            format!("All shown ({})", format::count(shown)),
            WriteScope::Shown,
            cx,
        ))
        .into_any_element()
}

/// A button among a few choices, filled when it is the one chosen.
pub(crate) fn choice_button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    is_chosen: bool,
    palette: &Palette,
) -> gpui::Stateful<Div> {
    let hover_background = palette.raised;
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .h(px(26.0))
        .px(px(10.0))
        .rounded(px(6.0))
        .border_1()
        .text_size(SMALL_TEXT_SIZE)
        .cursor_pointer()
        .map(|button| {
            if is_chosen {
                button
                    .border_color(palette.accent)
                    .bg(palette.accent)
                    .text_color(palette.accent_ink)
                    .font_weight(FontWeight::MEDIUM)
            } else {
                button
                    .border_color(palette.line)
                    .text_color(palette.text_secondary)
                    .hover(move |style| style.bg(hover_background))
            }
        })
        .child(label.into())
}

/// Every change, a file at a time, in a list that builds only the lines in
/// view.
fn preview_list(
    preview: &Preview,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    let row_count = preview.rows.len();
    let height = (row_count as f32 * PREVIEW_ROW_HEIGHT).min(320.0);
    let line = palette.line;
    div()
        .id("tag-preview")
        .flex_shrink_0()
        .h(px(height))
        .border_1()
        .border_color(line)
        .rounded(px(6.0))
        .child(
            uniform_list(
                "tag-preview-rows",
                row_count,
                cx.processor(
                    |this: &mut EuphoriaApp, range: std::ops::Range<usize>, _, _| {
                        let Some(TagPanel::Preview(preview)) = &this.tag_panel else {
                            return Vec::new();
                        };
                        let palette = this.theme.palette;
                        range
                            .filter_map(|index| preview.rows.get(index).map(|row| (index, row)))
                            .map(|(index, row)| preview_row(index, row, &palette))
                            .collect::<Vec<_>>()
                    },
                ),
            )
            .size_full(),
        )
        .test_support()
        .into_any_element()
}

fn preview_row(index: usize, row: &PreviewRow, palette: &Palette) -> AnyElement {
    let line = div()
        .id(("tag-preview-row", index))
        .w_full()
        .flex()
        .items_center()
        .gap(px(10.0))
        .h(px(PREVIEW_ROW_HEIGHT))
        .px(px(10.0))
        .text_size(SMALL_TEXT_SIZE);
    match row {
        PreviewRow::File { path, change_count } => {
            let full_path: SharedString = path.to_string_lossy().into_owned().into();
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| full_path.to_string());
            line.bg(palette.raised.opacity(0.5))
                .font_weight(FontWeight::MEDIUM)
                .tooltip(move |window, cx| Tooltip::new(full_path.clone()).build(window, cx))
                .child(div().flex_1().min_w_0().truncate().child(name))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(palette.text_tertiary)
                        .font_weight(FontWeight::NORMAL)
                        .child(counted(*change_count, "change")),
                )
                .test_support()
                .into_any_element()
        }
        PreviewRow::Change(change) => line
            .pl(px(24.0))
            .child(
                div()
                    .w(px(150.0))
                    .flex_shrink_0()
                    .truncate()
                    .text_color(palette.text_secondary)
                    .child(change.field.clone()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(palette.text_tertiary)
                    .child(shown_value(change.current.as_deref())),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(palette.text_tertiary)
                    .child("\u{2192}"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(shown_value(change.new.as_deref())),
            )
            .test_support()
            .into_any_element(),
    }
}

/// A value as the preview shows it, "(none)" for a field the file lacks.
fn shown_value(value: Option<&str>) -> String {
    value.map_or_else(|| "(none)".to_string(), str::to_string)
}

/// Height of one line of a list of files left out or failed.
const PROBLEM_ROW_HEIGHT: f32 = 22.0;

/// A list of files the panel can show as left out or failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProblemList {
    /// Files the preview found could not be written safely.
    CannotBeWritten,
    /// Tracks the preview left out because they have no analysis.
    NotAnalyzed,
    /// Files a write could not change.
    NotWritten,
    /// Files written whose note in the log could not be saved.
    NotNoted,
    /// Files an undo could not restore.
    NotRestored,
}

impl ProblemList {
    /// The files of this list in `panel`, empty when the panel does not
    /// have it.
    pub(crate) fn files(self, panel: &TagPanel) -> &[LeftOut] {
        match (self, panel) {
            (ProblemList::CannotBeWritten, TagPanel::Preview(preview)) => &preview.failures,
            (ProblemList::NotAnalyzed, TagPanel::Preview(preview)) => &preview.not_analyzed,
            (ProblemList::NotWritten, TagPanel::Written(report)) => &report.failures,
            (ProblemList::NotNoted, TagPanel::Written(report)) => &report.not_noted,
            (ProblemList::NotRestored, TagPanel::Undone(Ok(Some(report)))) => &report.failures,
            _ => &[],
        }
    }

    fn id(self) -> &'static str {
        match self {
            ProblemList::CannotBeWritten => "cannot-be-written",
            ProblemList::NotAnalyzed => "not-analyzed",
            ProblemList::NotWritten => "not-written",
            ProblemList::NotNoted => "not-noted",
            ProblemList::NotRestored => "not-restored",
        }
    }
}

/// A heading and the `count` files of `list`, a line each with its reason,
/// in a list that builds only the lines in view; or nothing when there are
/// none. A line too long for the panel shows in full in a tooltip.
fn problems(
    heading: &str,
    list: ProblemList,
    count: usize,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> Option<AnyElement> {
    if count == 0 {
        return None;
    }
    let height = (count as f32 * PROBLEM_ROW_HEIGHT).min(160.0);
    Some(
        div()
            .id(SharedString::from(format!("{}-section", list.id())))
            .flex()
            .flex_col()
            .flex_shrink_0()
            .gap(px(4.0))
            .text_size(SMALL_TEXT_SIZE)
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.danger)
                    .child(format!("{heading} ({})", format::count(count))),
            )
            .child(
                div().h(px(height)).child(
                    uniform_list(
                        list.id(),
                        count,
                        cx.processor(
                            move |this: &mut EuphoriaApp, range: std::ops::Range<usize>, _, _| {
                                let Some(panel) = &this.tag_panel else {
                                    return Vec::new();
                                };
                                let palette = this.theme.palette;
                                let files = list.files(panel);
                                range
                                    .filter_map(|index| files.get(index).map(|file| (index, file)))
                                    .map(|(index, file)| problem_row(list, index, file, &palette))
                                    .collect::<Vec<_>>()
                            },
                        ),
                    )
                    .size_full(),
                ),
            )
            .test_support()
            .into_any_element(),
    )
}

fn problem_row(list: ProblemList, index: usize, file: &LeftOut, palette: &Palette) -> AnyElement {
    let name = file
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.path.to_string_lossy().into_owned());
    let in_full: SharedString = format!("{}\n{}", file.path.to_string_lossy(), file.reason).into();
    div()
        .id((list.id(), index))
        .flex()
        .w_full()
        .items_center()
        .gap(px(10.0))
        .h(px(PROBLEM_ROW_HEIGHT))
        .tooltip(move |window, cx| Tooltip::new(in_full.clone()).build(window, cx))
        .child(div().w(px(220.0)).flex_shrink_0().truncate().child(name))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(palette.text_secondary)
                .child(file.reason.clone()),
        )
        .test_support()
        .into_any_element()
}

fn progress_bar(done: usize, total: usize, palette: &Palette) -> AnyElement {
    let fraction = if total == 0 {
        0.0
    } else {
        done as f32 / total as f32
    };
    div()
        .id("tag-progress")
        .h(px(6.0))
        .w_full()
        .rounded(px(3.0))
        .bg(palette.raised)
        .child(
            div()
                .h_full()
                .w(relative(fraction))
                .rounded(px(3.0))
                .bg(palette.accent),
        )
        .test_support()
        .into_any_element()
}

fn title(text: &str) -> AnyElement {
    div()
        .text_size(px(15.0))
        .font_weight(FontWeight::SEMIBOLD)
        .child(text.to_string())
        .into_any_element()
}

fn quiet(text: &str, palette: &Palette) -> AnyElement {
    div()
        .text_size(SMALL_TEXT_SIZE)
        .text_color(palette.text_secondary)
        .child(text.to_string())
        .into_any_element()
}

fn buttons(buttons: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .justify_end()
        .gap(px(8.0))
        .pt(px(4.0))
        .children(buttons)
        .into_any_element()
}

fn cancel_button(
    id: &'static str,
    palette: &Palette,
    cx: &mut Context<'_, EuphoriaApp>,
) -> AnyElement {
    secondary_button("Cancel", palette)
        .id(id)
        .on_click(cx.listener(|this, _, _, cx| this.close_tag_panel(cx)))
        .test_support()
        .into_any_element()
}

fn done_button(palette: &Palette, cx: &mut Context<'_, EuphoriaApp>) -> AnyElement {
    primary_button("Done", palette)
        .id("close-tag-panel")
        .on_click(cx.listener(|this, _, _, cx| this.close_tag_panel(cx)))
        .test_support()
        .into_any_element()
}

/// "1 file", "12 files".
fn counted(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{} {noun}s", format::count(count))
    }
}

/// The tracks `scope` covers, in words.
fn scope_words(app: &EuphoriaApp, scope: WriteScope) -> String {
    let count = app.tracks_in(scope).len();
    match scope {
        WriteScope::Selection => counted(count, "selected track"),
        WriteScope::Shown => format!("all {} shown", counted(count, "track")),
    }
}
