// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The preview in the settings of what the tag choices write: for one
//! track, each field that would change and the label inside it. It is the
//! plan of a real write, made by the same code and never applied.

use std::path::PathBuf;
use std::sync::OnceLock;

use euphoria_core::tags::{FieldEdit, TagField, TextChange, WritePlan, apply_plan, plan_edit};
use euphoria_core::{Key, Mode, TrackAnalysis};

use crate::key_by_ear::CorrectionWriter;
use crate::tag_flow::plan_verified_writes;
use crate::tag_settings::{ProgramChoice, TagSettings};
use crate::tag_writing::{PlannedWrite, WriteTarget};

/// The comment of the built-in example file.
const EXAMPLE_COMMENT: &str = "Great vocal intro";

const EXAMPLE_FILE: &[u8] =
    include_bytes!("../../../crates/euphoria-core/tests/fixtures/sine-440hz-mono.mp3");

/// Whose tags the preview shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviewSource {
    /// The track selected in the table, with its own file's comment.
    SelectedTrack,
    /// A built-in track: 8A, 128 beats per minute, energy 7.
    Example,
}

impl PreviewSource {
    /// The caption saying which track the preview is for.
    pub(crate) fn caption(self) -> &'static str {
        match self {
            PreviewSource::SelectedTrack => "Selected track",
            PreviewSource::Example => "Example",
        }
    }
}

/// One field of the preview: its name in words and the value it would
/// hold, split around the label so the label can be highlighted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreviewField {
    /// The field in words, such as "Comment" or "Key".
    pub(crate) name: &'static str,
    /// The field as the file's tag format names it, such as `COMM`.
    pub(crate) tag: String,
    /// The value before the label.
    pub(crate) before: String,
    /// The key and energy label, empty for a field without one.
    pub(crate) label: String,
    /// The value after the label.
    pub(crate) after: String,
}

impl PreviewField {
    #[cfg(test)]
    /// The whole value.
    pub(crate) fn value(&self) -> String {
        format!("{}{}{}", self.before, self.label, self.after)
    }
}

/// What the settings would write to one track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LabelPreview {
    /// Whose tags this is.
    pub(crate) source: PreviewSource,
    /// The fields that would change.
    pub(crate) fields: Vec<PreviewField>,
    /// Values left out or kept, and anything that stopped the plan, each as
    /// a sentence.
    pub(crate) notes: Vec<String>,
    /// Who reads the cue markers, and where a program that does not read
    /// them gets the cues from. Empty when the cue markers are off and the
    /// program reads cues from files.
    pub(crate) cue_hints: Vec<String>,
}

/// The lines on cue markers for `settings`.
fn cue_hints(settings: &TagSettings) -> Vec<String> {
    let mut hints = Vec::new();
    if settings.write_cue_markers {
        hints.push("Cue markers: read by Serato DJ, Mixxx and VirtualDJ".to_string());
    }
    if matches!(
        settings.program,
        ProgramChoice::Rekordbox | ProgramChoice::Traktor
    ) {
        hints.push(format!(
            "{} doesn't read cues from files: use File > Export Library\u{2026}",
            settings.program.label()
        ));
    }
    hints
}

impl LabelPreview {
    /// The preview of `planned`, the plan of the one track `source` names.
    pub(crate) fn of(
        source: PreviewSource,
        planned: &PlannedWrite,
        settings: &TagSettings,
    ) -> Self {
        let mut fields = Vec::new();
        let mut notes = Vec::new();
        for plan in &planned.plans {
            fields.extend(
                plan.changes()
                    .iter()
                    .map(|change| preview_field(plan, change, settings)),
            );
            notes.extend(plan.skipped().iter().map(|skipped| skipped.describe()));
        }
        notes.extend(planned.left_alone.iter().map(|left| left.reason.clone()));
        notes.extend(planned.failures.iter().map(|left| left.reason.clone()));
        notes.extend(planned.not_analyzed.iter().map(|left| left.reason.clone()));
        if fields.is_empty() && notes.is_empty() {
            notes.push("Nothing would change in this file.".to_string());
        }
        Self {
            source,
            fields,
            notes,
            cue_hints: cue_hints(settings),
        }
    }
}

fn preview_field(
    plan: &WritePlan,
    change: &euphoria_core::tags::FieldChange,
    settings: &TagSettings,
) -> PreviewField {
    let format = plan.format();
    let preset = settings.program.dj_software();
    let field = change.field.as_str();
    let new = change.new.clone().unwrap_or_default();
    let is_in = |fields: &[&str]| fields.contains(&field);
    let (name, label_inside) = if is_in(format.comment_fields()) {
        ("Comment", true)
    } else if is_in(format.grouping_fields()) {
        ("Grouping", true)
    } else if is_in(preset.key_fields(format)) || is_in(preset.key_fields_if_present(format)) {
        ("Key", false)
    } else if is_in(preset.bpm_fields(format)) || is_in(preset.bpm_fields_if_present(format)) {
        ("BPM", false)
    } else {
        ("Field", false)
    };
    let tag = euphoria_core::tags::serato_field_name(field)
        .unwrap_or(field)
        .to_string();
    if !label_inside {
        return PreviewField {
            name,
            tag,
            before: new,
            label: String::new(),
            after: String::new(),
        };
    }
    let (before, label, after) = split_label(change.current.as_deref(), &new);
    PreviewField {
        name,
        tag,
        before,
        label,
        after,
    }
}

/// Finds the label in `new`, the text a write leaves in a field that held
/// `current`: what the write added in front of or after the text the field
/// had, or all of it when the field's label was replaced.
fn split_label(current: Option<&str>, new: &str) -> (String, String, String) {
    let separator: &[char] = &[' ', '-'];
    if let Some(current) = current.filter(|current| !current.is_empty()) {
        if let Some(label) = new.strip_suffix(current) {
            let kept = label.trim_end_matches(separator).len();
            return (
                String::new(),
                label[..kept].to_string(),
                label[kept..].to_string() + current,
            );
        }
        if let Some(label) = new.strip_prefix(current) {
            let kept = label.len() - label.trim_start_matches(separator).len();
            return (
                current.to_string() + &label[..kept],
                label[kept..].to_string(),
                String::new(),
            );
        }
    }
    (String::new(), new.to_string(), String::new())
}

/// The key, tempo and energy of the built-in example.
fn example_analysis() -> TrackAnalysis {
    TrackAnalysis {
        duration_seconds: 1.0,
        key: Some(Key::new(9, Mode::Minor)),
        bpm: Some(128.0),
        energy: Some(7),
        tuning_cents: None,
        key_runner_up: None,
        key_score_margin: None,
        key_source: None,
        tempo_estimate: None,
        tempo_score_margin: None,
        beat_grid: None,
        cues: Vec::new(),
        key_timeline: Vec::new(),
        quality: None,
    }
}

/// A small audio file in the temporary folder, tagged with the example
/// comment and nothing else, that the example is planned against. `None`
/// when it cannot be made.
fn example_file() -> Option<PathBuf> {
    static FILE: OnceLock<Option<PathBuf>> = OnceLock::new();
    FILE.get_or_init(|| {
        let folder =
            std::env::temp_dir().join(format!("euphoria-label-preview-{}", std::process::id()));
        std::fs::create_dir_all(&folder).ok()?;
        let path = folder.join("example.mp3");
        std::fs::write(&path, EXAMPLE_FILE).ok()?;
        let plan = plan_edit(
            &path,
            &[FieldEdit {
                field: TagField::Comment,
                change: TextChange::Set(EXAMPLE_COMMENT.to_string()),
            }],
        )
        .ok()?;
        apply_plan(&plan).ok()?;
        Some(path)
    })
    .clone()
}

/// What the preview is made from: a real track, or the example.
#[derive(Debug, Clone)]
pub(crate) struct PreviewInput {
    pub(crate) source: PreviewSource,
    pub(crate) target: WriteTarget,
}

impl PreviewInput {
    /// The selected track, when it has an analysis, and the example
    /// otherwise.
    pub(crate) fn choose(selected: Option<WriteTarget>) -> Option<Self> {
        match selected.filter(|target| target.analysis.is_some()) {
            Some(target) => Some(Self {
                source: PreviewSource::SelectedTrack,
                target,
            }),
            None => Some(Self {
                source: PreviewSource::Example,
                target: WriteTarget {
                    path: example_file()?,
                    analysis: Some(example_analysis()),
                    correction: Default::default(),
                    edited_cues: None,
                },
            }),
        }
    }
}

/// Works out the preview of `input` with `settings`. Reads the file, and
/// never writes it.
pub(crate) fn plan_preview(
    input: PreviewInput,
    settings: &TagSettings,
    cache: Option<&euphoria_core::CachedAnalyzer>,
    writer: &CorrectionWriter,
    log_directory: Option<&std::path::Path>,
) -> LabelPreview {
    let planned = plan_verified_writes(vec![input.target], settings, cache, writer, log_directory);
    LabelPreview::of(input.source, &planned, settings)
}

impl crate::app::EuphoriaApp {
    /// Works out the preview for the selected track and the current tag
    /// settings, off the main thread, and keeps it for the settings panel.
    /// Files are read when the settings or the selection change, never
    /// while drawing.
    pub(crate) fn refresh_label_preview(&mut self, cx: &mut gpui::Context<'_, Self>) {
        self.label_preview_generation += 1;
        let generation = self.label_preview_generation;
        let selected = self.selected_track().map(|track| WriteTarget {
            path: track.path.clone(),
            analysis: track.analysis().cloned(),
            correction: track.correction,
            edited_cues: track.edited_cues.clone(),
        });
        let settings = self.tag_settings;
        let cache = self.session.cache();
        let writer = self.by_ear.correction_writer.clone();
        let log_directory = self.write_log_directory.clone();
        cx.spawn(async move |this, cx| {
            let preview = cx
                .background_executor()
                .spawn(async move {
                    let input = PreviewInput::choose(selected)?;
                    Some(plan_preview(
                        input,
                        &settings,
                        cache.as_deref(),
                        &writer,
                        log_directory.as_deref(),
                    ))
                })
                .await;
            this.update(cx, |this, cx| {
                if this.label_preview_generation == generation {
                    this.label_preview = preview;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use euphoria_core::tags::{TagValues, plan_write};

    use super::*;
    use crate::tag_settings::{LabelChoice, NotationChoice, ProgramChoice};
    use crate::tag_writing::testing::{ScratchFolder, analysis};

    fn settings_with(label: LabelChoice) -> TagSettings {
        TagSettings {
            program: ProgramChoice::Rekordbox,
            label: Some(label),
            ..TagSettings::default()
        }
    }

    fn preview_of(input: PreviewInput, settings: &TagSettings) -> LabelPreview {
        plan_preview(input, settings, None, &CorrectionWriter::default(), None)
    }

    fn example(settings: &TagSettings) -> LabelPreview {
        preview_of(PreviewInput::choose(None).unwrap(), settings)
    }

    fn selected_input(path: &std::path::Path) -> PreviewInput {
        PreviewInput::choose(Some(WriteTarget {
            path: path.to_path_buf(),
            analysis: Some(analysis()),
            correction: Default::default(),
            edited_cues: None,
        }))
        .unwrap()
    }

    fn comment(preview: &LabelPreview) -> &PreviewField {
        preview
            .fields
            .iter()
            .find(|field| field.name == "Comment")
            .expect("a comment field")
    }

    fn set_comment(path: &std::path::Path, text: &str) {
        let plan = plan_edit(
            path,
            &[FieldEdit {
                field: TagField::Comment,
                change: TextChange::Set(text.to_string()),
            }],
        )
        .unwrap();
        apply_plan(&plan).unwrap();
    }

    #[test]
    fn the_example_shows_the_label_at_the_start_of_the_comment() {
        let preview = example(&settings_with(LabelChoice::CommentStart));
        assert_eq!(preview.source, PreviewSource::Example);
        let comment = comment(&preview);
        assert_eq!(comment.label, "8A - Energy 7");
        assert_eq!(comment.before, "");
        assert_eq!(comment.after, " - Great vocal intro");
        let names: Vec<&str> = preview.fields.iter().map(|field| field.name).collect();
        assert!(
            names.contains(&"Key") && names.contains(&"BPM"),
            "{names:?}"
        );
        let tempo = preview.fields.iter().find(|field| field.name == "BPM");
        assert_eq!(tempo.unwrap().value(), "128");
    }

    #[test]
    fn the_preview_changes_with_the_label_place() {
        let start = example(&settings_with(LabelChoice::CommentStart));
        let end = example(&settings_with(LabelChoice::CommentEnd));
        let grouping = example(&settings_with(LabelChoice::Grouping));
        let none = example(&settings_with(LabelChoice::None));

        assert_eq!(comment(&start).value(), "8A - Energy 7 - Great vocal intro");
        assert_eq!(comment(&end).value(), "Great vocal intro - 8A - Energy 7");
        assert_eq!(comment(&end).before, "Great vocal intro - ");
        assert_eq!(comment(&end).label, "8A - Energy 7");
        assert_eq!(grouping.fields.last().unwrap().name, "Grouping");
        assert_eq!(grouping.fields.last().unwrap().label, "8A - Energy 7");
        assert!(none.fields.iter().all(|field| field.label.is_empty()));
        assert!(none.fields.iter().all(|field| field.name != "Comment"));
    }

    #[test]
    fn the_preview_changes_with_the_key_notation() {
        let musical = TagSettings {
            key_notation: NotationChoice::Musical,
            ..settings_with(LabelChoice::CommentStart)
        };
        let preview = example(&musical);
        assert_eq!(comment(&preview).label, "Am - Energy 7");
        let key = preview.fields.iter().find(|field| field.name == "Key");
        assert_eq!(key.unwrap().value(), "Am");
    }

    #[test]
    fn the_selected_tracks_real_comment_is_used() {
        let scratch = ScratchFolder::new("label-preview-selected");
        let files = scratch.copy_fixtures();
        set_comment(&files[0], "Played at the warehouse");
        let before = std::fs::read(&files[0]).unwrap();

        let preview = preview_of(
            selected_input(&files[0]),
            &settings_with(LabelChoice::CommentStart),
        );
        assert_eq!(preview.source, PreviewSource::SelectedTrack);
        assert_eq!(
            comment(&preview).value(),
            "8A - Energy 7 - Played at the warehouse"
        );
        assert_eq!(std::fs::read(&files[0]).unwrap(), before);
    }

    #[test]
    fn a_track_without_an_analysis_falls_back_to_the_example() {
        let scratch = ScratchFolder::new("label-preview-unanalyzed");
        let files = scratch.copy_fixtures();
        let input = PreviewInput::choose(Some(WriteTarget {
            path: files[0].clone(),
            analysis: None,
            correction: Default::default(),
            edited_cues: None,
        }))
        .unwrap();
        assert_eq!(input.source, PreviewSource::Example);
        assert_eq!(PreviewSource::Example.caption(), "Example");
        assert_eq!(PreviewSource::SelectedTrack.caption(), "Selected track");
    }

    #[test]
    fn the_preview_matches_the_plan_of_a_real_write() {
        let scratch = ScratchFolder::new("label-preview-plan");
        let files = scratch.copy_fixtures();
        set_comment(&files[0], "Played at the warehouse");
        for label in LabelChoice::ALL {
            let settings = TagSettings {
                key_notation: NotationChoice::OpenKey,
                ..settings_with(label)
            };
            let plan = plan_write(
                &files[0],
                TagValues::from(&analysis()),
                &settings.write_options(),
            )
            .unwrap();
            let preview = preview_of(selected_input(&files[0]), &settings);
            let planned: Vec<(String, Option<String>)> = plan
                .changes()
                .iter()
                .map(|change| {
                    (
                        euphoria_core::tags::serato_field_name(&change.field)
                            .map_or_else(|| change.field.clone(), str::to_string),
                        change.new.clone(),
                    )
                })
                .collect();
            let shown: Vec<(String, Option<String>)> = preview
                .fields
                .iter()
                .map(|field| (field.tag.clone(), Some(field.value())))
                .collect();
            assert_eq!(shown, planned, "{label:?}");
        }
    }

    #[test]
    fn a_key_the_file_already_has_is_kept_unless_replacing() {
        let scratch = ScratchFolder::new("label-preview-kept");
        let files = scratch.copy_fixtures();
        let seeding = TagSettings {
            replace_existing: true,
            ..settings_with(LabelChoice::CommentStart)
        };
        let seeded = plan_write(
            &files[0],
            TagValues::from(&analysis()),
            &seeding.write_options(),
        )
        .unwrap();
        apply_plan(&seeded).unwrap();

        let other = PreviewInput::choose(Some(WriteTarget {
            path: files[0].clone(),
            analysis: Some(TrackAnalysis {
                key: Some(Key::new(4, Mode::Minor)),
                bpm: Some(150.0),
                ..analysis()
            }),
            correction: Default::default(),
            edited_cues: None,
        }))
        .unwrap();
        let kept = preview_of(other.clone(), &settings_with(LabelChoice::CommentStart));
        assert!(
            kept.notes
                .iter()
                .any(|note| note.contains("kept (already tagged 8A)")),
            "{:?}",
            kept.notes
        );
        assert!(kept.fields.iter().all(|field| field.name != "Key"));

        let replaced = preview_of(other, &seeding);
        assert!(replaced.fields.iter().any(|field| field.name == "Key"));
        assert!(replaced.notes.is_empty(), "{:?}", replaced.notes);
    }

    #[test]
    fn cue_hints_name_who_reads_the_markers_and_where_rekordbox_gets_cues() {
        let mut settings = settings_with(LabelChoice::CommentStart);
        let hints = example(&settings).cue_hints;
        assert_eq!(
            hints,
            ["rekordbox doesn't read cues from files: use File > Export Library\u{2026}"]
        );
        settings.write_cue_markers = true;
        settings.program = ProgramChoice::Serato;
        assert_eq!(
            example(&settings).cue_hints,
            ["Cue markers: read by Serato DJ, Mixxx and VirtualDJ"]
        );
        settings.program = ProgramChoice::Traktor;
        assert_eq!(example(&settings).cue_hints.len(), 2);
    }

    #[test]
    fn planning_the_example_never_changes_its_file() {
        let path = example_file().unwrap();
        let before = std::fs::read(&path).unwrap();
        example(&settings_with(LabelChoice::CommentEnd));
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}
