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
const EXAMPLE_COMMENT: &str = "Your comment";

const EXAMPLE_FILE: &[u8] =
    include_bytes!("../../../crates/euphoria-core/tests/fixtures/sine-440hz-mono.mp3");

/// Whose tags the preview shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreviewSource {
    /// The track selected in the table, with its own file's comment.
    SelectedTrack,
    /// A built-in track: 8A, 128 beats per minute, energy 7.
    Example,
    /// The built-in track, because the selected track's file cannot be
    /// read.
    ExampleForMissingFile,
}

impl PreviewSource {
    /// The caption saying which track the preview is for.
    pub(crate) fn caption(self) -> &'static str {
        match self {
            PreviewSource::SelectedTrack => "Selected track",
            PreviewSource::Example => "Example",
            PreviewSource::ExampleForMissingFile => {
                "Example (the selected track's file is missing)"
            }
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
    /// Why an option keeps this row out of the write, such as "not
    /// written". The row is then shown struck out, with the value it would
    /// have held.
    pub(crate) struck: Option<String>,
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
        unrestricted: &PlannedWrite,
        settings: &TagSettings,
    ) -> Self {
        let mut notes = Vec::new();
        for plan in &planned.plans {
            notes.extend(plan.skipped().iter().map(|skipped| skipped.describe()));
        }
        let left_out: Vec<String> = notes
            .iter()
            .chain(planned.left_alone.iter().map(|left| &left.reason))
            .cloned()
            .collect();
        notes.extend(planned.left_alone.iter().map(|left| left.reason.clone()));
        notes.extend(planned.failures.iter().map(|left| left.reason.clone()));
        notes.extend(planned.not_analyzed.iter().map(|left| left.reason.clone()));
        let written = planned_fields(planned, settings);
        if written.is_empty() && notes.is_empty() {
            notes.push("Nothing would change in this file.".to_string());
        }
        let fields = with_struck_rows(
            written,
            planned_fields(unrestricted, settings),
            settings,
            &left_out,
        );
        Self {
            source,
            fields,
            notes,
            cue_hints: cue_hints(settings),
        }
    }
}

/// The rows of every field `planned` changes, rows with the same value
/// folded together.
fn planned_fields(planned: &PlannedWrite, settings: &TagSettings) -> Vec<PreviewField> {
    merge_same_rows(
        planned
            .plans
            .iter()
            .flat_map(|plan| {
                plan.changes()
                    .iter()
                    .map(|change| preview_field(plan, change, settings))
            })
            .collect(),
    )
}

/// `written` with a struck out row, in its natural place, for each kind of
/// field that only `unrestricted`, the plan with every option allowing a
/// write, would write.
fn with_struck_rows(
    written: Vec<PreviewField>,
    unrestricted: Vec<PreviewField>,
    settings: &TagSettings,
    left_out: &[String],
) -> Vec<PreviewField> {
    let mut names: Vec<&'static str> = Vec::new();
    for field in &unrestricted {
        if !names.contains(&field.name) {
            names.push(field.name);
        }
    }
    let position = |name: &str| names.iter().position(|known| *known == name);
    let mut rows = written;
    for field in &unrestricted {
        let is_written = rows
            .iter()
            .any(|row| row.name == field.name && row.struck.is_none());
        let is_struck = rows
            .iter()
            .any(|row| row.name == field.name && row.struck.is_some());
        if is_written || is_struck {
            continue;
        }
        let mut struck = field.clone();
        struck.struck = Some(struck_reason(field.name, settings, left_out));
        let index = rows
            .iter()
            .position(|row| position(row.name) > position(field.name))
            .unwrap_or(rows.len());
        rows.insert(index, struck);
    }
    rows
}

/// What kept a row out, in a few words.
fn struck_reason(name: &str, settings: &TagSettings, left_out: &[String]) -> String {
    let not_written = "not written".to_string();
    match name {
        "BPM" if !settings.write_tempo => return not_written,
        "Cue markers" | "Beat grid" if !settings.write_cue_markers => return not_written,
        _ => {}
    }
    let word = match name {
        "Key" => "key",
        "BPM" => "tempo",
        _ => return not_written,
    };
    let clause = left_out
        .iter()
        .flat_map(|note| note.split("; "))
        .find(|clause| clause.starts_with(word));
    match clause {
        Some(clause) if clause.contains("low confidence") => {
            "not written, low confidence".to_string()
        }
        Some(clause) if clause.contains("kept") => "kept as it is in the file".to_string(),
        _ => not_written,
    }
}

/// Folds rows that show the same value under the same name into one row
/// naming every tag: a program can read the key or tempo from several tags,
/// and the write fills each of them with the same text.
fn merge_same_rows(fields: Vec<PreviewField>) -> Vec<PreviewField> {
    let mut merged: Vec<PreviewField> = Vec::new();
    for field in fields {
        let same = merged.iter_mut().find(|row| {
            row.name == field.name
                && row.before == field.before
                && row.label == field.label
                && row.after == field.after
        });
        match same {
            Some(row) => {
                if !row.tag.split(", ").any(|tag| tag == field.tag) {
                    row.tag = format!("{}, {}", row.tag, field.tag);
                }
            }
            None => merged.push(field),
        }
    }
    merged
}

fn preview_field(
    plan: &WritePlan,
    change: &euphoria_core::tags::FieldChange,
    settings: &TagSettings,
) -> PreviewField {
    let format = plan.format();
    let field = change.field.as_str();
    let new = change.new.clone().unwrap_or_default();
    let (name, label_inside) = field_name(format, settings.program.dj_software(), field);
    let tag = euphoria_core::tags::serato_field_name(field)
        .unwrap_or(field)
        .to_string();
    if !label_inside {
        return PreviewField {
            name,
            tag,
            before: euphoria_core::tags::describe_serato_value(field, &new).unwrap_or(new),
            label: String::new(),
            after: String::new(),
            struck: None,
        };
    }
    let (mut before, label, mut after) = split_label(change.current.as_deref(), &new);
    // The text a file already holds is shown as a placeholder, not as itself:
    // a comment can hold a link or a private note, and the preview is about
    // where the label goes, not about that text.
    if let Some(current) = change
        .current
        .as_deref()
        .filter(|current| !current.is_empty())
    {
        let placeholder = if name == "Grouping" {
            "Your grouping"
        } else {
            "Your comment"
        };
        before = before.replacen(current, placeholder, 1);
        after = after.replacen(current, placeholder, 1);
    }
    PreviewField {
        name,
        tag,
        before,
        label,
        after,
        struck: None,
    }
}

/// What `field` of a file in `format` holds for `preset`, in words, and
/// whether the energy label can go inside it. Fields that are none of the
/// four everyone knows are named "Field".
pub(crate) fn field_name(
    format: euphoria_core::tags::TagFormat,
    preset: euphoria_core::tags::DjSoftware,
    field: &str,
) -> (&'static str, bool) {
    let is_in = |fields: &[&str]| fields.contains(&field);
    if is_in(format.comment_fields()) {
        ("Comment", true)
    } else if is_in(format.grouping_fields()) {
        ("Grouping", true)
    } else if is_in(preset.key_fields(format)) || is_in(preset.key_fields_if_present(format)) {
        ("Key", false)
    } else if is_in(preset.bpm_fields(format)) || is_in(preset.bpm_fields_if_present(format)) {
        ("BPM", false)
    } else if let Some(serato) = euphoria_core::tags::serato_field_name(field) {
        if serato.contains("beat grid") {
            ("Beat grid", false)
        } else {
            ("Cue markers", false)
        }
    } else {
        ("Field", false)
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
        loudness_range_lu: None,
        pitch_class_profile: None,
        tempo_estimate: None,
        tempo_score_margin: None,
        tempo_octave_evidence: None,
        beat_grid: Some(euphoria_core::detect::BeatGrid {
            first_downbeat_seconds: 0.1,
            bpm: 128.0,
            salience: 10.0,
            drift_seconds: None,
        }),
        cues: vec![euphoria_core::detect::SectionCue {
            seconds: 0.1,
            section: euphoria_core::detect::CueSection::Start,
            energy: None,
        }],
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
    /// The selected track, when it has an analysis and its file can be
    /// read, and the example otherwise.
    pub(crate) fn choose(selected: Option<WriteTarget>) -> Option<Self> {
        let selected = selected.filter(|target| target.analysis.is_some());
        let source = match &selected {
            Some(target) if std::fs::File::open(&target.path).is_ok() => {
                return Some(Self {
                    source: PreviewSource::SelectedTrack,
                    target: selected?,
                });
            }
            Some(_) => PreviewSource::ExampleForMissingFile,
            None => PreviewSource::Example,
        };
        Some(Self {
            source,
            target: WriteTarget {
                path: example_file()?,
                analysis: Some(example_analysis()),
                correction: Default::default(),
                edited_cues: None,
                saved_loops: Vec::new(),
            },
        })
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
    let targets = vec![input.target];
    let planned = plan_verified_writes(targets.clone(), settings, cache, writer, log_directory);
    let unrestricted_settings = TagSettings {
        write_tempo: true,
        write_cue_markers: true,
        colour_cues_by_energy: true,
        replace_existing: true,
        include_low_confidence: true,
        ..*settings
    };
    let unrestricted = plan_verified_writes(
        targets,
        &unrestricted_settings,
        cache,
        writer,
        log_directory,
    );
    LabelPreview::of(input.source, &planned, &unrestricted, settings)
}

/// How many files of the library the tag settings would change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LibraryChange {
    /// Files a write would change.
    pub(crate) would_change: usize,
    /// Files in the library.
    pub(crate) total: usize,
}

impl LibraryChange {
    /// The line the settings show.
    pub(crate) fn sentence(self) -> String {
        format!(
            "Would change in your library: {} of {} files",
            crate::format::count(self.would_change),
            crate::format::count(self.total)
        )
    }
}

/// Plans a write of `targets`, the whole library, and counts the files it
/// would change. Reads the files, and never writes them.
pub(crate) fn plan_library_change(
    targets: Vec<WriteTarget>,
    settings: &TagSettings,
    cache: Option<&euphoria_core::CachedAnalyzer>,
    writer: &CorrectionWriter,
    log_directory: Option<&std::path::Path>,
    reads: &crate::tag_reads::TagReads,
) -> LibraryChange {
    let total = targets.len();
    let planned = crate::tag_flow::plan_verified_writes_reading(
        targets,
        settings,
        cache,
        writer,
        log_directory,
        Some(reads),
    );
    LibraryChange {
        would_change: planned.plans.len(),
        total,
    }
}

/// How long the settings must stay unchanged before the library is counted
/// again.
pub(crate) const LIBRARY_COUNT_DELAY: std::time::Duration = std::time::Duration::from_millis(250);

impl crate::app::EuphoriaApp {
    /// Counts, off the main thread, the library files the current tag
    /// settings would change, and keeps the count for the settings panel.
    /// Waits for the settings to rest first, and a count that a later
    /// change overtook is dropped. Files are read once and planned again
    /// from memory.
    pub(crate) fn refresh_library_change(&mut self, cx: &mut gpui::Context<'_, Self>) {
        self.library_change_generation += 1;
        self.library_change = None;
        let generation = self.library_change_generation;
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(LIBRARY_COUNT_DELAY).await;
            let job = this
                .update(cx, |this, _| {
                    (this.library_change_generation == generation).then(|| {
                        let targets: Vec<WriteTarget> = this
                            .session
                            .library()
                            .tracks()
                            .iter()
                            .map(|track| WriteTarget {
                                path: track.path.clone(),
                                analysis: track.analysis().cloned(),
                                correction: track.correction,
                                edited_cues: track.edited_cues.clone(),
                                saved_loops: track.saved_loops.clone(),
                            })
                            .collect();
                        (
                            targets,
                            this.tag_settings,
                            this.session.cache(),
                            this.by_ear.correction_writer.clone(),
                            this.write_log_directory.clone(),
                            this.tag_reads.clone(),
                        )
                    })
                })
                .ok()
                .flatten();
            let Some((targets, settings, cache, writer, log_directory, reads)) = job else {
                return;
            };
            let change = cx
                .background_executor()
                .spawn(async move {
                    plan_library_change(
                        targets,
                        &settings,
                        cache.as_deref(),
                        &writer,
                        log_directory.as_deref(),
                        &reads,
                    )
                })
                .await;
            this.update(cx, |this, cx| {
                if this.library_change_generation == generation {
                    this.library_change = Some(change);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    /// Tags were written or put back: what was read from the files is out
    /// of date, and the count in the settings, when they are open, is
    /// worked out again.
    pub(crate) fn tags_changed(&mut self, cx: &mut gpui::Context<'_, Self>) {
        self.tag_reads.invalidate();
        if self.settings_open {
            self.refresh_library_change(cx);
        }
    }

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
            saved_loops: track.saved_loops.clone(),
        });
        let settings = self.tag_settings;
        let cache = self.session.cache();
        let writer = self.by_ear.correction_writer.clone();
        let log_directory = self.write_log_directory.clone();
        self.refresh_library_change(cx);
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

    fn row(name: &'static str, tag: &str, value: &str) -> PreviewField {
        PreviewField {
            name,
            tag: tag.to_string(),
            before: value.to_string(),
            label: String::new(),
            after: String::new(),
            struck: None,
        }
    }

    #[test]
    fn a_value_written_to_several_tags_is_one_row_naming_each_tag() {
        let merged = merge_same_rows(vec![
            row("Key", "TKEY", "8A"),
            row("BPM", "TBPM", "128"),
            row("Key", "TXXX:INITIALKEY", "8A"),
            row("BPM", "TXXX:BPM", "128"),
            row("BPM", "TBPM", "128"),
        ]);
        assert_eq!(
            merged,
            vec![
                row("Key", "TKEY, TXXX:INITIALKEY", "8A"),
                row("BPM", "TBPM, TXXX:BPM", "128"),
            ]
        );
    }

    #[test]
    fn different_values_under_one_name_stay_separate_rows() {
        let merged = merge_same_rows(vec![row("Key", "TKEY", "8A"), row("Key", "TXXX:KEY", "Am")]);
        assert_eq!(merged.len(), 2);
    }

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
            saved_loops: Vec::new(),
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
        assert_eq!(comment.after, " - Your comment");
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

        assert_eq!(comment(&start).value(), "8A - Energy 7 - Your comment");
        assert_eq!(comment(&end).value(), "Your comment - 8A - Energy 7");
        assert_eq!(comment(&end).before, "Your comment - ");
        assert_eq!(comment(&end).label, "8A - Energy 7");
        let grouping_row = grouping
            .fields
            .iter()
            .find(|field| field.name == "Grouping")
            .expect("a grouping row");
        assert_eq!(grouping_row.label, "8A - Energy 7");
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
    fn the_selected_tracks_comment_is_shown_as_a_placeholder() {
        let scratch = ScratchFolder::new("label-preview-selected");
        let files = scratch.copy_fixtures();
        set_comment(&files[0], "Played at the warehouse");
        let before = std::fs::read(&files[0]).unwrap();

        let preview = preview_of(
            selected_input(&files[0]),
            &settings_with(LabelChoice::CommentStart),
        );
        assert_eq!(preview.source, PreviewSource::SelectedTrack);
        assert_eq!(comment(&preview).value(), "8A - Energy 7 - Your comment");
        assert!(
            !preview
                .fields
                .iter()
                .any(|field| field.value().contains("warehouse")),
            "the file's own text is never shown"
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
            saved_loops: Vec::new(),
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
            let planned: Vec<(String, Option<String>)> =
                plan.changes()
                    .iter()
                    .map(|change| {
                        (
                            euphoria_core::tags::serato_field_name(&change.field)
                                .map_or_else(|| change.field.clone(), str::to_string),
                            change.new.clone().map(|new| {
                                new.replacen("Played at the warehouse", "Your comment", 1)
                            }),
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
            saved_loops: Vec::new(),
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
        assert!(
            kept.fields
                .iter()
                .all(|field| field.name != "Key" || field.struck.is_some())
        );

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
