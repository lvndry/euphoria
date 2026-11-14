// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Editing the tags of the selected tracks by hand: each field shows the
//! value the tracks share, or that they differ, and takes a new value; one
//! field at a time can also be changed by find and replace or by adding or
//! removing a prefix or a suffix. The edits go through the preview, the
//! write log and the undo of analysis writes.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use euphoria_core::TrackAnalysis;
use euphoria_core::tags::{
    EditableTags, FieldEdit, TagField, TextChange, plan_edit, read_editable_tags,
};
use gpui::component::input::InputState;
use gpui::{AppContext, Context, Entity, Window};

use crate::app::EuphoriaApp;
use crate::tag_flow::{Preview, PreviewKind, TagPanel, WriteScope};
use crate::tag_writing::{LeftOut, preview_rows};

/// What a field of the editor starts with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FieldValue {
    /// Every track has this value; `None` when none has the field.
    Same(Option<String>),
    /// The tracks differ.
    Mixed,
}

/// The value each field shows for tracks with these tags.
pub(crate) fn shared_values(tags: &[EditableTags]) -> Vec<(TagField, FieldValue)> {
    TagField::ALL
        .into_iter()
        .map(|field| {
            let mut values = tags.iter().map(|tags| tags.get(field));
            let first = values.next().flatten();
            let value = if values.all(|value| value == first) {
                FieldValue::Same(first.map(str::to_string))
            } else {
                FieldValue::Mixed
            };
            (field, value)
        })
        .collect()
}

/// The kinds of change the text tool makes to one field.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum TextTool {
    /// Find and replace.
    #[default]
    Replace,
    /// Put text in front.
    AddPrefix,
    /// Put text at the end.
    AddSuffix,
    /// Take text off the front.
    RemovePrefix,
    /// Take text off the end.
    RemoveSuffix,
}

impl TextTool {
    /// Every tool, in the order they are offered.
    pub(crate) const ALL: [TextTool; 5] = [
        TextTool::Replace,
        TextTool::AddPrefix,
        TextTool::AddSuffix,
        TextTool::RemovePrefix,
        TextTool::RemoveSuffix,
    ];

    /// The tool's name on its button.
    pub(crate) fn label(self) -> &'static str {
        match self {
            TextTool::Replace => "Find and replace",
            TextTool::AddPrefix => "Add prefix",
            TextTool::AddSuffix => "Add suffix",
            TextTool::RemovePrefix => "Remove prefix",
            TextTool::RemoveSuffix => "Remove suffix",
        }
    }

    /// The change for `text` and, for a replacement, `with`; `None` when
    /// there is nothing to change.
    pub(crate) fn change(self, text: &str, with: &str, match_case: bool) -> Option<TextChange> {
        if text.is_empty() {
            return None;
        }
        let text = text.to_string();
        Some(match self {
            TextTool::Replace => TextChange::Replace {
                find: text,
                replace: with.to_string(),
                match_case,
            },
            TextTool::AddPrefix => TextChange::AddPrefix(text),
            TextTool::AddSuffix => TextChange::AddSuffix(text),
            TextTool::RemovePrefix => TextChange::RemovePrefix(text),
            TextTool::RemoveSuffix => TextChange::RemoveSuffix(text),
        })
    }
}

/// The edits the editor's fields ask for: a field whose text the user
/// changed gets it, an empty text removing the field, and a field the user
/// cleared is removed. A field that differs between tracks and was left
/// empty is left alone. The text tool's change comes last.
pub(crate) fn field_edits(
    shown: &[(TagField, FieldValue)],
    typed: &[(TagField, String)],
    cleared: &HashSet<TagField>,
    tool: Option<FieldEdit>,
) -> Vec<FieldEdit> {
    let mut edits: Vec<FieldEdit> = shown
        .iter()
        .filter_map(|(field, value)| {
            let field = *field;
            if cleared.contains(&field) {
                return Some(FieldEdit {
                    field,
                    change: TextChange::Clear,
                });
            }
            let typed = typed
                .iter()
                .find(|(candidate, _)| *candidate == field)
                .map(|(_, text)| text.as_str())
                .unwrap_or_default();
            let is_change = match value {
                FieldValue::Same(current) => typed != current.as_deref().unwrap_or_default(),
                FieldValue::Mixed => !typed.is_empty(),
            };
            is_change.then(|| FieldEdit {
                field,
                change: TextChange::Set(typed.to_string()),
            })
        })
        .collect();
    edits.extend(tool);
    edits
}

/// The editor over the window, for the tracks selected when it opened.
pub(crate) struct TagEditor {
    /// The files, each with its analysis when it has one, to keep it known
    /// to the cache once its tags change.
    pub(crate) tracks: Vec<(PathBuf, Option<TrackAnalysis>)>,
    /// The value each field started with, once the tags are read.
    pub(crate) shown: Option<Vec<(TagField, FieldValue)>>,
    /// A text field for each tag field, in [`TagField::ALL`] order.
    pub(crate) inputs: Vec<(TagField, Entity<InputState>)>,
    /// The fields the user asked to remove.
    pub(crate) cleared: HashSet<TagField>,
    /// The field the text tool changes.
    pub(crate) tool_field: TagField,
    pub(crate) tool: TextTool,
    /// The text to find or to add or remove.
    pub(crate) tool_text: Entity<InputState>,
    /// What replaces the found text.
    pub(crate) tool_with: Entity<InputState>,
    pub(crate) match_case: bool,
    /// Files whose tags could not be read, which the edit leaves out.
    pub(crate) unreadable: Vec<LeftOut>,
    /// Why the edit could not be previewed yet, when it could not.
    pub(crate) problem: Option<String>,
}

impl std::fmt::Debug for TagEditor {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TagEditor")
            .field("tracks", &self.tracks.len())
            .field("shown", &self.shown)
            .field("cleared", &self.cleared)
            .field("tool", &self.tool)
            .field("tool_field", &self.tool_field)
            .finish_non_exhaustive()
    }
}

impl TagEditor {
    /// The text typed in each tag field.
    fn typed(&self, cx: &gpui::App) -> Vec<(TagField, String)> {
        self.inputs
            .iter()
            .map(|(field, input)| (*field, input.read(cx).value().to_string()))
            .collect()
    }

    /// The edits the editor asks for now.
    pub(crate) fn edits(&self, cx: &gpui::App) -> Vec<FieldEdit> {
        let Some(shown) = &self.shown else {
            return Vec::new();
        };
        let tool = self
            .tool
            .change(
                &self.tool_text.read(cx).value(),
                &self.tool_with.read(cx).value(),
                self.match_case,
            )
            .map(|change| FieldEdit {
                field: self.tool_field,
                change,
            });
        field_edits(shown, &self.typed(cx), &self.cleared, tool)
    }

    /// The input of `field`.
    pub(crate) fn input(&self, field: TagField) -> Option<&Entity<InputState>> {
        self.inputs
            .iter()
            .find(|(candidate, _)| *candidate == field)
            .map(|(_, input)| input)
    }
}

impl EuphoriaApp {
    /// Opens the tag editor for the selected tracks and reads their tags in
    /// the background.
    pub(crate) fn open_tag_editor(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.tag_panel.as_ref().is_some_and(TagPanel::is_busy) {
            return;
        }
        let library = self.session.library();
        let tracks: Vec<(PathBuf, Option<TrackAnalysis>)> = self
            .selected_tracks()
            .into_iter()
            .filter_map(|index| library.track(index))
            .map(|track| (track.path.clone(), track.analysis().cloned()))
            .collect();
        if tracks.is_empty() {
            self.notice = Some("Select the tracks whose tags you want to edit.".to_string());
            cx.notify();
            return;
        }
        self.settings_open = false;
        let new_input =
            |placeholder: &'static str, window: &mut Window, cx: &mut Context<'_, Self>| {
                cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
            };
        let inputs = TagField::ALL
            .into_iter()
            .map(|field| (field, new_input("", window, cx)))
            .collect();
        let editor = TagEditor {
            tracks: tracks.clone(),
            shown: None,
            inputs,
            cleared: HashSet::new(),
            tool_field: TagField::Title,
            tool: TextTool::Replace,
            tool_text: new_input("Find", window, cx),
            tool_with: new_input("Replace with", window, cx),
            match_case: false,
            unreadable: Vec::new(),
            problem: None,
        };
        self.tag_task_generation += 1;
        let generation = self.tag_task_generation;
        self.tag_panel = Some(TagPanel::Editing(Box::new(editor)));
        cx.notify();

        let paths: Vec<PathBuf> = tracks.into_iter().map(|(path, _)| path).collect();
        cx.spawn_in(window, async move |this, cx| {
            let read = cx
                .background_executor()
                .spawn(async move {
                    paths
                        .into_iter()
                        .map(|path| {
                            let tags = read_editable_tags(&path);
                            (path, tags)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            this.update_in(cx, |this, window, cx| {
                if this.tag_task_generation != generation {
                    return;
                }
                this.show_read_tags(read, window, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Fills the editor with the tags read: the shared value of each field,
    /// or "mixed".
    pub(crate) fn show_read_tags(
        &mut self,
        read: Vec<(PathBuf, Result<EditableTags, euphoria_core::EuphoriaError>)>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(TagPanel::Editing(editor)) = &mut self.tag_panel else {
            return;
        };
        let mut tags = Vec::new();
        let mut unreadable = Vec::new();
        for (path, result) in read {
            match result {
                Ok(read) => tags.push(read),
                Err(error) => unreadable.push(LeftOut {
                    path,
                    reason: error.chain(),
                }),
            }
        }
        let unreadable_paths: HashSet<PathBuf> =
            unreadable.iter().map(|file| file.path.clone()).collect();
        editor
            .tracks
            .retain(|(path, _)| !unreadable_paths.contains(path));
        let shown = shared_values(&tags);
        for (field, value) in &shown {
            if let Some(input) = editor.input(*field) {
                let (text, placeholder) = match value {
                    FieldValue::Same(text) => (text.clone().unwrap_or_default(), ""),
                    FieldValue::Mixed => (String::new(), "mixed"),
                };
                input.update(cx, |input, cx| {
                    input.set_value(text, window, cx);
                    input.set_placeholder(placeholder, window, cx);
                });
            }
        }
        editor.shown = Some(shown);
        editor.unreadable = unreadable;
        cx.notify();
    }

    /// Marks a field to be removed, or takes the mark back.
    pub(crate) fn toggle_cleared_field(&mut self, field: TagField, cx: &mut Context<'_, Self>) {
        if let Some(TagPanel::Editing(editor)) = &mut self.tag_panel {
            if !editor.cleared.remove(&field) {
                editor.cleared.insert(field);
            }
            cx.notify();
        }
    }

    /// Chooses the field the text tool changes.
    pub(crate) fn choose_tool_field(&mut self, field: TagField, cx: &mut Context<'_, Self>) {
        if let Some(TagPanel::Editing(editor)) = &mut self.tag_panel {
            editor.tool_field = field;
            cx.notify();
        }
    }

    /// Chooses what the text tool does.
    pub(crate) fn choose_text_tool(&mut self, tool: TextTool, cx: &mut Context<'_, Self>) {
        if let Some(TagPanel::Editing(editor)) = &mut self.tag_panel {
            editor.tool = tool;
            cx.notify();
        }
    }

    /// Turns matching of capital letters on or off.
    pub(crate) fn toggle_match_case(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(TagPanel::Editing(editor)) = &mut self.tag_panel {
            editor.match_case = !editor.match_case;
            cx.notify();
        }
    }

    /// Works out what the edits change in each file, off the main thread,
    /// and shows the same preview as an analysis write.
    pub(crate) fn preview_tag_edits(&mut self, cx: &mut Context<'_, Self>) {
        let Some(TagPanel::Editing(editor)) = &mut self.tag_panel else {
            return;
        };
        let edits = editor.edits(cx);
        if edits.is_empty() {
            editor.problem = Some("Change a field, or use the text tool, first.".to_string());
            cx.notify();
            return;
        }
        let tracks = editor.tracks.clone();
        let unreadable = editor.unreadable.clone();
        let summary = describe(&edits);
        self.tag_task_generation += 1;
        let generation = self.tag_task_generation;
        self.tag_panel = Some(TagPanel::Planning(WriteScope::Selection));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let planned = cx
                .background_executor()
                .spawn(async move {
                    let mut plans = Vec::new();
                    let mut analyses = Vec::new();
                    let mut up_to_date = 0;
                    let mut failures = unreadable;
                    for (path, analysis) in tracks {
                        match plan_edit(&path, &edits) {
                            Ok(plan) if plan.is_empty() => up_to_date += 1,
                            Ok(plan) => {
                                plans.push(plan);
                                analyses.push(analysis);
                            }
                            Err(error) => failures.push(LeftOut {
                                path,
                                reason: error.chain(),
                            }),
                        }
                    }
                    (plans, analyses, up_to_date, failures)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.tag_task_generation != generation {
                    return;
                }
                let (plans, analyses, up_to_date, failures) = planned;
                this.tag_panel = Some(TagPanel::Preview(Preview {
                    kind: PreviewKind::Edit { summary },
                    settings: this.tag_settings,
                    rows: preview_rows(&plans),
                    plans: Arc::new(plans),
                    analyses: Arc::new(analyses),
                    up_to_date,
                    not_analyzed: Vec::new(),
                    failures,
                    serato_notes: Vec::new(),
                }));
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// The edits in a sentence, for the preview.
pub(crate) fn describe(edits: &[FieldEdit]) -> String {
    let parts: Vec<String> = edits
        .iter()
        .map(|edit| {
            let field = edit.field.label();
            match &edit.change {
                TextChange::Set(text) if text.is_empty() => format!("{field} removed"),
                TextChange::Set(text) => format!("{field} set to \u{201c}{text}\u{201d}"),
                TextChange::Clear => format!("{field} removed"),
                TextChange::Replace {
                    find,
                    replace,
                    match_case,
                } => format!(
                    "\u{201c}{find}\u{201d} replaced with \u{201c}{replace}\u{201d} in {}{}",
                    field.to_lowercase(),
                    if *match_case { ", matching case" } else { "" }
                ),
                TextChange::AddPrefix(text) => {
                    format!(
                        "\u{201c}{text}\u{201d} put in front of {}",
                        field.to_lowercase()
                    )
                }
                TextChange::AddSuffix(text) => {
                    format!("\u{201c}{text}\u{201d} put after {}", field.to_lowercase())
                }
                TextChange::RemovePrefix(text) => {
                    format!(
                        "\u{201c}{text}\u{201d} taken off the front of {}",
                        field.to_lowercase()
                    )
                }
                TextChange::RemoveSuffix(text) => {
                    format!(
                        "\u{201c}{text}\u{201d} taken off the end of {}",
                        field.to_lowercase()
                    )
                }
            }
        })
        .collect();
    format!("{}.", parts.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(values: &[(TagField, &str)]) -> EditableTags {
        EditableTags::from_values(
            values
                .iter()
                .map(|(field, text)| (*field, text.to_string()))
                .collect(),
        )
    }

    #[test]
    fn a_field_shows_the_shared_value_or_that_the_tracks_differ() {
        let shown = shared_values(&[
            tags(&[(TagField::Genre, "House"), (TagField::Artist, "Ana")]),
            tags(&[(TagField::Genre, "House"), (TagField::Artist, "Ben")]),
        ]);
        let value = |field: TagField| {
            shown
                .iter()
                .find(|(candidate, _)| *candidate == field)
                .unwrap()
                .1
                .clone()
        };
        assert_eq!(
            value(TagField::Genre),
            FieldValue::Same(Some("House".to_string()))
        );
        assert_eq!(value(TagField::Artist), FieldValue::Mixed);
        assert_eq!(value(TagField::Album), FieldValue::Same(None));
        assert_eq!(shared_values(&[]).len(), TagField::ALL.len());
    }

    #[test]
    fn only_the_fields_the_user_changed_become_edits() {
        let shown = vec![
            (TagField::Genre, FieldValue::Same(Some("House".to_string()))),
            (TagField::Artist, FieldValue::Mixed),
            (TagField::Album, FieldValue::Same(None)),
            (TagField::Year, FieldValue::Same(Some("1999".to_string()))),
            (TagField::Comment, FieldValue::Mixed),
        ];
        let typed = vec![
            (TagField::Genre, "House".to_string()),
            (TagField::Artist, String::new()),
            (TagField::Album, "Late Hours".to_string()),
            (TagField::Year, String::new()),
            (TagField::Comment, String::new()),
        ];
        let cleared: HashSet<TagField> = [TagField::Comment].into();
        let tool = TextTool::AddSuffix
            .change(" (Edit)", "", false)
            .map(|change| FieldEdit {
                field: TagField::Title,
                change,
            });
        let edits = field_edits(&shown, &typed, &cleared, tool);
        assert_eq!(
            edits,
            [
                FieldEdit {
                    field: TagField::Album,
                    change: TextChange::Set("Late Hours".to_string())
                },
                FieldEdit {
                    field: TagField::Year,
                    change: TextChange::Set(String::new())
                },
                FieldEdit {
                    field: TagField::Comment,
                    change: TextChange::Clear
                },
                FieldEdit {
                    field: TagField::Title,
                    change: TextChange::AddSuffix(" (Edit)".to_string())
                },
            ]
        );
        assert_eq!(TextTool::Replace.change("", "x", true), None);
    }

    #[test]
    fn the_preview_says_what_the_edits_do() {
        let summary = describe(&[
            FieldEdit {
                field: TagField::Genre,
                change: TextChange::Set("House".to_string()),
            },
            FieldEdit {
                field: TagField::Album,
                change: TextChange::Clear,
            },
            FieldEdit {
                field: TagField::Title,
                change: TextChange::Replace {
                    find: "feat".to_string(),
                    replace: "ft".to_string(),
                    match_case: true,
                },
            },
        ]);
        assert_eq!(
            summary,
            "Genre set to \u{201c}House\u{201d}; Album removed; \u{201c}feat\u{201d} replaced \
             with \u{201c}ft\u{201d} in title, matching case."
        );
    }
}
