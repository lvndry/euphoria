// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tags are changed in three steps. [`plan_write`] computes the changes
//! without touching the file, so they can be shown first. [`apply_plan`]
//! writes them and returns a [`TagBackup`] of the values it replaced.
//! [`undo`] puts those values back.

mod native;
mod preset;

use std::path::{Path, PathBuf};

use crate::detect::Analysis;
use crate::error::EuphoriaError;
use native::{ID3_COMMENT, MP4_TEMPO, TagFile};
pub use preset::DjSoftware;

const LABEL_SEPARATOR: &str = " - ";
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
            TagFormat::Id3v2 => &[ID3_COMMENT],
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
    let file = TagFile::load(path)?;
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
}

/// The field an extra label is written to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LabelTarget {
    /// The comment field.
    Comment,
    /// The grouping field.
    Grouping,
}

/// Extra text, such as "8A", put in front of what a field already holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    /// The text to put first.
    pub text: String,
    /// The field that receives it.
    pub target: LabelTarget,
}

/// What [`plan_write`] writes and where.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TagWriteOptions {
    /// The notation of the key field.
    pub key_notation: KeyNotation,
    /// Whether the tempo is written.
    pub write_bpm: bool,
    /// An extra label, when one is wanted.
    pub label: Option<Label>,
    /// The DJ program whose fields are written.
    pub preset: DjSoftware,
}

/// One tag field and the value it holds before and after a write. `None`
/// means the field is absent.
#[derive(Debug, Clone, PartialEq, Eq)]
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
            fields: self
                .changes
                .iter()
                .map(|change| BackupField {
                    field: change.field.clone(),
                    original: change.current.clone(),
                    written: change.new.clone(),
                })
                .collect(),
        }
    }
}

/// One field touched by a write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupField {
    /// The field's name in the file's tag format.
    pub field: String,
    /// The value before the write, `None` when the field did not exist.
    pub original: Option<String>,
    /// The value the write left, `None` when it removed the field.
    pub written: Option<String>,
}

/// What a write replaced in one file, kept by the caller so [`undo`] can
/// restore it. All of it is plain text, so it can be stored in any form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagBackup {
    /// The file that was written.
    pub path: PathBuf,
    /// The touched fields.
    pub fields: Vec<BackupField>,
}

/// Computes the tag changes that record `analysis` in the file at `path`.
/// The file is only read.
pub fn plan_write(
    path: &Path,
    analysis: &Analysis,
    options: &TagWriteOptions,
) -> Result<WritePlan, EuphoriaError> {
    let file = TagFile::load(path)?;
    let format = file.format();
    let mut wanted: Vec<(&str, String)> = Vec::new();

    if let Some(key) = analysis.key {
        let text = match options.key_notation {
            KeyNotation::Camelot => key.camelot(),
            KeyNotation::Musical => key.short_name(),
        };
        for field in options.preset.key_fields(format) {
            wanted.push((field, text.clone()));
        }
    }

    if let (true, Some(tempo)) = (options.write_bpm, analysis.tempo) {
        if !tempo.is_finite() || tempo <= 0.0 {
            return Err(EuphoriaError::TagWrite {
                path: path.to_path_buf(),
                source: format!("a tempo of {tempo} cannot be written").into(),
            });
        }
        for field in options.preset.bpm_fields(format) {
            let fractional = options.preset.writes_fractional_bpm() && *field != MP4_TEMPO;
            wanted.push((field, format_bpm(tempo, fractional)));
        }
    }

    let mut changes = Vec::new();
    for (field, new) in wanted {
        let current = file.get(field)?;
        let is_bpm = options.preset.bpm_fields(format).contains(&field);
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

    if let Some(label) = options
        .label
        .as_ref()
        .filter(|label| !label.text.is_empty())
    {
        let field = match label.target {
            LabelTarget::Comment => format.comment_fields()[0],
            LabelTarget::Grouping => format.grouping_fields()[0],
        };
        let current = file.get(field)?;
        let new = with_label(&label.text, current.as_deref());
        if current.as_deref() != Some(new.as_str()) {
            changes.push(FieldChange {
                field: field.to_string(),
                current,
                new: Some(new),
            });
        }
    }

    Ok(WritePlan {
        path: path.to_path_buf(),
        format,
        changes,
    })
}

/// Writes the changes of `plan` and returns the record that undoes them.
/// Every other tag and the audio are kept. An empty plan leaves the file
/// untouched.
///
/// Fails with [`EuphoriaError::TagConflict`], writing nothing, when a field
/// no longer holds the value the plan was made from.
pub fn apply_plan(plan: &WritePlan) -> Result<TagBackup, EuphoriaError> {
    let backup = plan.backup();
    write_fields(
        &plan.path,
        plan.changes
            .iter()
            .map(|change| (change.field.as_str(), &change.current, &change.new)),
    )?;
    Ok(backup)
}

/// Restores the values a write replaced, removing the fields it created.
///
/// Fails with [`EuphoriaError::TagConflict`], writing nothing, when a field
/// no longer holds the value the write left, so that a later edit is not
/// overwritten.
pub fn undo(backup: &TagBackup) -> Result<(), EuphoriaError> {
    write_fields(
        &backup.path,
        backup
            .fields
            .iter()
            .map(|field| (field.field.as_str(), &field.written, &field.original)),
    )
}

/// Moves each field from its expected value to its new one, after checking
/// that every field still holds what is expected.
fn write_fields<'a>(
    path: &Path,
    fields: impl Iterator<Item = (&'a str, &'a Option<String>, &'a Option<String>)> + Clone,
) -> Result<(), EuphoriaError> {
    if fields.clone().next().is_none() {
        return Ok(());
    }
    let mut file = TagFile::load(path)?;
    for (field, expected, _) in fields.clone() {
        if file.get(field)? != *expected {
            return Err(EuphoriaError::TagConflict {
                path: path.to_path_buf(),
                field: field.to_string(),
            });
        }
    }
    for (field, _, new) in fields {
        file.set(field, new.as_deref())?;
    }
    file.save()
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

/// Puts `label` in front of `existing`, unless it is already there.
fn with_label(label: &str, existing: Option<&str>) -> String {
    match existing {
        None | Some("") => label.to_string(),
        Some(existing) if existing == label => existing.to_string(),
        Some(existing)
            if existing
                .strip_prefix(label)
                .is_some_and(|rest| rest.starts_with(LABEL_SEPARATOR)) =>
        {
            existing.to_string()
        }
        Some(existing) => format!("{label}{LABEL_SEPARATOR}{existing}"),
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

    #[test]
    fn label_goes_first_and_is_added_once() {
        assert_eq!(with_label("8A", None), "8A");
        assert_eq!(with_label("8A", Some("")), "8A");
        assert_eq!(with_label("8A", Some("great intro")), "8A - great intro");
        assert_eq!(
            with_label("8A", Some("8A - great intro")),
            "8A - great intro"
        );
        assert_eq!(with_label("8A", Some("8A")), "8A");
        assert_eq!(with_label("8A", Some("8Ax")), "8A - 8Ax");
    }
}
