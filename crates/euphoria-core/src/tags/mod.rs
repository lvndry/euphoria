// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tags are changed in three steps. [`crate::tags::plan_write`] computes the changes
//! without touching the file, so they can be shown first. [`crate::tags::apply_plan`]
//! writes them and returns a [`crate::tags::TagBackup`] of the values it replaced.
//! [`crate::tags::undo`] puts those values back. [`crate::tags::WriteLog`] keeps the backups of a
//! write on disk, so the write can be undone later by another run.

mod base64;
mod edit;
mod flac;
mod history;
mod id3v2;
pub(crate) mod label;
mod mp4;
mod native;
mod ogg;
mod preset;
mod replace;
mod riff_info;
mod serato;
mod vorbis;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::analysis::TrackAnalysis;
use crate::detect::Analysis;
use crate::error::EuphoriaError;
use crate::key::Key;
pub use edit::{EditableTags, FieldEdit, TagField, TextChange, plan_edit, read_editable_tags};
pub use history::{
    LoggedWrite, RewriteStage, UndoneWrite, WriteLog, default_write_log_directory, undo_last_write,
    undo_last_write_with_observer,
};
use id3v2::COMMENT as ID3_COMMENT;
pub use label::LabelPlacement;
use mp4::TEMPO as MP4_TEMPO;
use native::{Purpose, TagFile};
pub use preset::DjSoftware;
pub use serato::{
    GridOutcome, PlacedCue, SERATO_CUE_COLOURS, SERATO_HOT_CUES, SeratoCue, SeratoGrid,
    SeratoMarkers, SeratoReport, SeratoSkip, add_serato_markers, describe_serato_value,
    serato_field_name,
};

const BPM_MATCH_TOLERANCE: f64 = 0.005;

/// The kind of tag a file stores its metadata in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TagFormat {
    /// ID3v2 frames, used by MP3, WAV and AIFF files.
    Id3v2,
    /// MP4 atoms, used by M4A files.
    Mp4,
    /// Vorbis comments, used by FLAC files.
    VorbisComments,
}

impl TagFormat {
    fn title_fields(self) -> &'static [&'static str] {
        match self {
            TagFormat::Id3v2 => &["TIT2"],
            TagFormat::Mp4 => &["©nam"],
            TagFormat::VorbisComments => &["TITLE"],
        }
    }

    fn artist_fields(self) -> &'static [&'static str] {
        match self {
            TagFormat::Id3v2 => &["TPE1"],
            TagFormat::Mp4 => &["©ART"],
            TagFormat::VorbisComments => &["ARTIST"],
        }
    }

    fn key_fields(self) -> &'static [&'static str] {
        match self {
            TagFormat::Id3v2 => &[preset::ID3_KEY],
            TagFormat::Mp4 => &[preset::MP4_INITIAL_KEY, preset::MP4_KEY],
            TagFormat::VorbisComments => &[preset::VORBIS_INITIAL_KEY, preset::VORBIS_KEY],
        }
    }

    fn bpm_fields(self) -> &'static [&'static str] {
        match self {
            TagFormat::Id3v2 => &[preset::ID3_BPM],
            TagFormat::Mp4 => &[preset::MP4_FREEFORM_BPM, MP4_TEMPO],
            TagFormat::VorbisComments => &[preset::VORBIS_BPM, preset::VORBIS_TEMPO],
        }
    }

    fn comment_fields(self) -> &'static [&'static str] {
        match self {
            TagFormat::Id3v2 => &[ID3_COMMENT, riff_info::COMMENT],
            TagFormat::Mp4 => &["©cmt"],
            TagFormat::VorbisComments => &["COMMENT", "DESCRIPTION"],
        }
    }

    fn grouping_fields(self) -> &'static [&'static str] {
        match self {
            TagFormat::Id3v2 => &["TIT1", "GRP1"],
            TagFormat::Mp4 => &["©grp"],
            TagFormat::VorbisComments => &["GROUPING"],
        }
    }
}

/// The tag values of one file that matter to a DJ library. A field is
/// `None` when the file does not have it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackTags {
    /// The kind of tag the values were read from.
    pub format: TagFormat,
    /// The track title.
    pub title: Option<String>,
    /// The track artist.
    pub artist: Option<String>,
    /// The key field, in whatever notation it was written.
    pub key: Option<String>,
    /// The tempo field as text.
    pub bpm: Option<String>,
    /// The comment.
    pub comment: Option<String>,
    /// The grouping.
    pub grouping: Option<String>,
}

/// Reads the title, artist, key, tempo, comment and grouping of a file.
pub fn read_tags(path: &Path) -> Result<TrackTags, EuphoriaError> {
    let file = TagFile::load(path, Purpose::Reading)?;
    let format = file.format();
    Ok(TrackTags {
        format,
        title: file.first_of(format.title_fields())?,
        artist: file.first_of(format.artist_fields())?,
        key: file.first_of(format.key_fields())?,
        bpm: file.first_of(format.bpm_fields())?,
        comment: file.first_of(format.comment_fields())?,
        grouping: file.first_of(format.grouping_fields())?,
    })
}

/// How a key is spelled in the key field.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum KeyNotation {
    /// Camelot codes such as "8A".
    #[default]
    Camelot,
    /// Short musical names such as "Am".
    Musical,
    /// Open Key codes such as "1m", the notation Traktor uses for its own
    /// keys.
    OpenKey,
}

impl KeyNotation {
    /// The key written in this notation.
    pub fn spell(self, key: Key) -> String {
        match self {
            KeyNotation::Camelot => key.camelot(),
            KeyNotation::Musical => key.short_name(),
            KeyNotation::OpenKey => key.open_key(),
        }
    }
}

/// What an analysis found, as far as tags record it.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TagValues {
    /// The detected key.
    pub key: Option<Key>,
    /// The detected tempo in beats per minute.
    pub bpm: Option<f32>,
    /// The energy rating, from 1 to 10.
    pub energy: Option<u8>,
}

impl From<&TrackAnalysis> for TagValues {
    fn from(analysis: &TrackAnalysis) -> Self {
        Self {
            key: analysis.key,
            bpm: analysis.bpm,
            energy: analysis.energy,
        }
    }
}

impl From<&Analysis> for TagValues {
    fn from(analysis: &Analysis) -> Self {
        Self {
            key: analysis.key,
            bpm: analysis.tempo,
            energy: None,
        }
    }
}

/// What [`plan_write`] writes and where.
///
/// The default writes the key as a Camelot code and the tempo to the fields
/// every common program reads, and puts a label such as "8A - Energy 7" in
/// front of the comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagWriteOptions {
    /// The notation of the key, in the key field and in the label.
    pub key_notation: KeyNotation,
    /// Whether the tempo is written.
    pub write_bpm: bool,
    /// Where the key and energy label goes. The energy rating has no tag
    /// field of its own that DJ programs read, so the label is the only
    /// place it is written.
    pub label: LabelPlacement,
    /// The DJ program whose fields are written.
    pub preset: DjSoftware,
}

impl Default for TagWriteOptions {
    fn default() -> Self {
        Self {
            key_notation: KeyNotation::default(),
            write_bpm: true,
            label: LabelPlacement::default(),
            preset: DjSoftware::default(),
        }
    }
}

/// One tag field and the value it holds before and after a write. `None`
/// means the field is absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldChange {
    /// The field's name in the file's tag format, such as `TKEY`,
    /// `----:com.apple.iTunes:initialkey` or `INITIALKEY`.
    pub field: String,
    /// The value in the file when the plan was made.
    pub current: Option<String>,
    /// The value the plan writes.
    pub new: Option<String>,
}

/// The changes one write would make to one file. Made by [`plan_write`],
/// shown to the user, then handed to [`apply_plan`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritePlan {
    path: PathBuf,
    format: TagFormat,
    changes: Vec<FieldChange>,
    /// For each change, the bytes the field's item has in the file now.
    current_items: Vec<Option<Vec<u8>>>,
    tag_length: u64,
    file_length: u64,
}

impl WritePlan {
    /// The file the plan is for.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The kind of tag the file has.
    pub fn format(&self) -> TagFormat {
        self.format
    }

    /// The fields that would change. Fields that already hold the wanted
    /// value are not listed.
    pub fn changes(&self) -> &[FieldChange] {
        &self.changes
    }

    /// True when the file already matches and would be left alone.
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    /// The record that undoes this plan once it is applied. It is the same
    /// record [`apply_plan`] returns, available beforehand so it can be
    /// stored before the file changes.
    pub fn backup(&self) -> TagBackup {
        TagBackup {
            path: self.path.clone(),
            original_tag_length: Some(self.tag_length),
            original_file_length: Some(self.file_length),
            fields: self
                .changes
                .iter()
                .zip(&self.current_items)
                .map(|(change, item)| BackupField {
                    field: change.field.clone(),
                    original: change.current.clone(),
                    original_item: item.clone(),
                    written: change.new.clone(),
                })
                .collect(),
        }
    }
}

/// One field touched by a write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupField {
    /// The field's name in the file's tag format.
    pub field: String,
    /// The value before the write, `None` when the field did not exist.
    pub original: Option<String>,
    /// The field's whole item before the write, as the bytes the file
    /// held: an ID3v2 frame, an MP4 item or a Vorbis comment. [`undo`] puts
    /// these bytes back, so the restored field is identical to the
    /// original and not merely equal in value. When this is `None` and
    /// `original` is not, undo writes `original` as a new value instead.
    pub original_item: Option<Vec<u8>>,
    /// The value the write left, `None` when it removed the field.
    pub written: Option<String>,
}

/// What a write replaced in one file, kept by the caller so [`undo`] can
/// restore it. All of it is plain text and bytes, so it can be stored in
/// any form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TagBackup {
    /// The file that was written.
    pub path: PathBuf,
    /// The length in bytes of the area the tag occupied before the write:
    /// the ID3v2 tag, the MP4 `moov` atom, or all FLAC metadata blocks.
    /// [`undo`] gives the area this length again when padding allows,
    /// which makes the file identical to the original down to its padding.
    /// Without it, undo still restores every field.
    pub original_tag_length: Option<u64>,
    /// The length of the file in bytes before the write, used like
    /// `original_tag_length`.
    pub original_file_length: Option<u64>,
    /// The touched fields.
    pub fields: Vec<BackupField>,
}

/// Computes the tag changes that record `values` in the file at `path`.
/// The file is only read.
///
/// `values` is a [`TagValues`], or a reference to a
/// [`TrackAnalysis`] or to a detector's
/// [`Analysis`].
pub fn plan_write(
    path: &Path,
    values: impl Into<TagValues>,
    options: &TagWriteOptions,
) -> Result<WritePlan, EuphoriaError> {
    let values = values.into();
    let refuse = |reason: String| EuphoriaError::TagWrite {
        path: path.to_path_buf(),
        source: reason.into(),
    };
    if let Some(energy) = values.energy.filter(|energy| !(1..=10).contains(energy)) {
        return Err(refuse(format!(
            "an energy rating of {energy} cannot be written"
        )));
    }
    let file = TagFile::load(path, Purpose::Rewriting)?;
    let format = file.format();
    let preset = options.preset;
    // Each field to write, its value, and whether the value is a tempo.
    let mut wanted: Vec<(&str, String, bool)> = Vec::new();

    if let Some(key) = values.key {
        let text = options.key_notation.spell(key);
        for field in preset.key_fields(format) {
            wanted.push((field, text.clone(), false));
        }
        for field in preset.key_fields_if_present(format) {
            if file.get(field)?.is_some() {
                wanted.push((field, text.clone(), false));
            }
        }
    }

    if let (true, Some(tempo)) = (options.write_bpm, values.bpm) {
        if !tempo.is_finite() || tempo <= 0.0 {
            return Err(refuse(format!("a tempo of {tempo} cannot be written")));
        }
        let text = |field: &str| {
            let fractional = preset.writes_fractional_bpm() && field != MP4_TEMPO;
            format_bpm(tempo, fractional)
        };
        for field in preset.bpm_fields(format) {
            wanted.push((field, text(field), true));
        }
        for field in preset.bpm_fields_if_present(format) {
            if file.get(field)?.is_some() {
                wanted.push((field, text(field), true));
            }
        }
    }

    let mut changes = Vec::new();
    for (field, new, is_bpm) in wanted {
        let current = file.get(field)?;
        let matches = current
            .as_deref()
            .is_some_and(|current| current == new || (is_bpm && same_bpm(current, &new)));
        if !matches {
            changes.push(FieldChange {
                field: field.to_string(),
                current,
                new: Some(new),
            });
        }
    }

    let label_field = match options.label {
        LabelPlacement::CommentStart | LabelPlacement::CommentEnd => {
            Some(format.comment_fields()[0])
        }
        LabelPlacement::Grouping => Some(format.grouping_fields()[0]),
        LabelPlacement::None => None,
    };
    let label = label::label_text(values.key, values.energy, options.key_notation);
    let at_end = options.label == LabelPlacement::CommentEnd;
    // In a WAV file the label also goes into the comment of the RIFF INFO
    // list, which is where rekordbox reads a WAV file's comment.
    let in_info_list = matches!(
        options.label,
        LabelPlacement::CommentStart | LabelPlacement::CommentEnd
    ) && file.has_info_list();
    let label_fields = label_field
        .into_iter()
        .chain(in_info_list.then_some(riff_info::COMMENT));
    for field in label_fields {
        let Some(label) = &label else {
            break;
        };
        let current = file.get(field)?;
        let new = label::with_label(label, current.as_deref(), at_end);
        if current.as_deref() != Some(new.as_str()) {
            changes.push(FieldChange {
                field: field.to_string(),
                current,
                new: Some(new),
            });
        }
    }

    let current_items = changes
        .iter()
        .map(|change| file.raw(&change.field))
        .collect::<Result<_, _>>()?;
    let planned: Vec<(&str, Option<&str>)> = changes
        .iter()
        .map(|change| (change.field.as_str(), change.new.as_deref()))
        .collect();
    file.check_can_write(&planned)?;
    Ok(WritePlan {
        path: path.to_path_buf(),
        format,
        changes,
        current_items,
        tag_length: file.tag_length(),
        file_length: file.file_length(),
    })
}

/// Writes the changes of `plan` and returns the record that undoes them.
/// Nothing else in the file changes: every other tag item, the artwork and
/// the audio keep the bytes they had. An empty plan leaves the file
/// untouched.
///
/// Fails with [`EuphoriaError::TagConflict`], writing nothing, when a field
/// no longer holds what the plan was made from.
pub fn apply_plan(plan: &WritePlan) -> Result<TagBackup, EuphoriaError> {
    apply_plan_observing(plan, || {})
}

pub(super) fn apply_plan_observing(
    plan: &WritePlan,
    after: impl FnMut(),
) -> Result<TagBackup, EuphoriaError> {
    let backup = plan.backup();
    if plan.is_empty() {
        return Ok(backup);
    }
    let mut file = TagFile::load(&plan.path, Purpose::Rewriting)?;
    for (change, planned_item) in plan.changes.iter().zip(&plan.current_items) {
        let is_as_planned =
            file.get(&change.field)? == change.current && file.raw(&change.field)? == *planned_item;
        if !is_as_planned {
            return Err(conflict(&plan.path, &change.field));
        }
    }
    for change in &plan.changes {
        match &change.new {
            Some(new) => file.set(&change.field, new)?,
            None => file.remove(&change.field)?,
        }
    }
    file.save_with_observer(after)?;
    Ok(backup)
}

/// Restores what a write replaced: each field gets back the exact bytes it
/// had, and the fields the write created are removed.
///
/// Fails with [`EuphoriaError::TagConflict`], writing nothing, when a field
/// no longer holds the value the write left, so that a later edit is not
/// overwritten.
pub fn undo(backup: &TagBackup) -> Result<(), EuphoriaError> {
    undo_observing(backup, || {})
}

pub(super) fn undo_observing(backup: &TagBackup, after: impl FnMut()) -> Result<(), EuphoriaError> {
    if backup.fields.is_empty() {
        return Ok(());
    }
    let mut file = TagFile::load(&backup.path, Purpose::Rewriting)?;
    for field in &backup.fields {
        if file.get(&field.field)? != field.written {
            return Err(conflict(&backup.path, &field.field));
        }
    }
    file.prefer_lengths(backup.original_tag_length, backup.original_file_length);
    for field in &backup.fields {
        match (&field.original_item, &field.original) {
            (Some(item), _) => file.restore(&field.field, Some(item))?,
            (None, Some(original)) => file.set(&field.field, original)?,
            (None, None) => file.restore(&field.field, None)?,
        }
    }
    file.save_with_observer(after)
}

fn conflict(path: &Path, field: &str) -> EuphoriaError {
    EuphoriaError::TagConflict {
        path: path.to_path_buf(),
        field: field.to_string(),
    }
}

fn format_bpm(tempo: f32, fractional: bool) -> String {
    if !fractional {
        return format!("{tempo:.0}");
    }
    let text = format!("{tempo:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// True when both texts are the same number, so "128.00" already matches
/// "128" and is not rewritten.
fn same_bpm(current: &str, new: &str) -> bool {
    match (current.trim().parse::<f64>(), new.parse::<f64>()) {
        (Ok(current), Ok(new)) => (current - new).abs() < BPM_MATCH_TOLERANCE,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempo_is_whole_unless_fractions_are_wanted() {
        assert_eq!(format_bpm(127.5, false), "128");
        assert_eq!(format_bpm(127.5, true), "127.5");
        assert_eq!(format_bpm(128.0, true), "128");
        assert_eq!(format_bpm(100.0, true), "100");
        assert_eq!(format_bpm(174.004, true), "174");
    }

    #[test]
    fn tempo_texts_match_by_value() {
        assert!(same_bpm("128.00", "128"));
        assert!(same_bpm(" 128 ", "128"));
        assert!(!same_bpm("127.5", "128"));
        assert!(!same_bpm("fast", "128"));
    }
}
