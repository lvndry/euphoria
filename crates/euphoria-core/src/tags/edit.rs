// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Editing tag fields by hand, for one file or thousands: set a title, an
//! artist, a genre and so on, find and replace in a field, or add and
//! remove a prefix or a suffix.
//!
//! An edit becomes a [`WritePlan`] like an analysis does, so it goes through
//! the same preview, the same safe write and the same [`WriteLog`] and undo.
//! A comment or grouping keeps the key and energy label Euphoria wrote in
//! it: an edit changes the user's own text beside the label, never the
//! label, so a comment edit cannot duplicate or lose it.
//!
//! [`WriteLog`]: super::WriteLog

use std::fmt;
use std::path::Path;

use super::label::{join_labels, split_labels};
use super::mp4::{TEMPO as MP4_TEMPO, TRACK_NUMBER as MP4_TRACK_NUMBER};
use super::native::{Purpose, TagFile};
use super::riff_info;
use super::{FieldChange, TagFormat, WritePlan, format_bpm, preset, same_bpm};
use crate::error::EuphoriaError;

/// A tag field the user can edit, the same in every file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TagField {
    /// The track title.
    Title,
    /// The artist.
    Artist,
    /// The album.
    Album,
    /// The genre.
    Genre,
    /// The year, or a full date.
    Year,
    /// The track number, such as "7" or "7/12".
    TrackNumber,
    /// The comment, without the key and energy label.
    Comment,
    /// The grouping, without the key and energy label.
    Grouping,
    /// The key field, as text in any notation.
    Key,
    /// The tempo field, in beats per minute.
    Bpm,
}

impl TagField {
    /// Every field, in the order an editor shows them.
    pub const ALL: [TagField; 10] = [
        TagField::Title,
        TagField::Artist,
        TagField::Album,
        TagField::Genre,
        TagField::Year,
        TagField::TrackNumber,
        TagField::Comment,
        TagField::Grouping,
        TagField::Key,
        TagField::Bpm,
    ];

    /// The name the command line uses, such as `genre` or `track`.
    pub fn name(self) -> &'static str {
        match self {
            TagField::Title => "title",
            TagField::Artist => "artist",
            TagField::Album => "album",
            TagField::Genre => "genre",
            TagField::Year => "year",
            TagField::TrackNumber => "track",
            TagField::Comment => "comment",
            TagField::Grouping => "grouping",
            TagField::Key => "key",
            TagField::Bpm => "bpm",
        }
    }

    /// The name to show, such as "Track number".
    pub fn label(self) -> &'static str {
        match self {
            TagField::Title => "Title",
            TagField::Artist => "Artist",
            TagField::Album => "Album",
            TagField::Genre => "Genre",
            TagField::Year => "Year",
            TagField::TrackNumber => "Track number",
            TagField::Comment => "Comment",
            TagField::Grouping => "Grouping",
            TagField::Key => "Key",
            TagField::Bpm => "BPM",
        }
    }

    /// The field called `name`, ignoring case, by [`TagField::name`] or a
    /// common other spelling such as `tempo` or `tracknumber`.
    pub fn from_name(name: &str) -> Option<Self> {
        let wanted = name
            .trim()
            .to_ascii_lowercase()
            .replace(['-', '_', ' '], "");
        match wanted.as_str() {
            "tempo" => Some(TagField::Bpm),
            "tracknumber" | "number" => Some(TagField::TrackNumber),
            "date" => Some(TagField::Year),
            other => TagField::ALL
                .into_iter()
                .find(|field| field.name() == other),
        }
    }

    /// The native fields that hold this field in a file, the main one
    /// first. The others are kept in agreement when the file has them.
    fn native_fields(self, format: TagFormat, id3_version: Option<u8>) -> Vec<&'static str> {
        match (self, format) {
            (TagField::Title, TagFormat::Id3v2) => vec!["TIT2", riff_info::TITLE],
            (TagField::Title, _) => format.title_fields().to_vec(),
            (TagField::Artist, TagFormat::Id3v2) => vec!["TPE1", riff_info::ARTIST],
            (TagField::Artist, _) => format.artist_fields().to_vec(),
            (TagField::Album, TagFormat::Id3v2) => vec!["TALB", riff_info::ALBUM],
            (TagField::Album, TagFormat::Mp4) => vec!["©alb"],
            (TagField::Album, TagFormat::VorbisComments) => vec!["ALBUM"],
            (TagField::Genre, TagFormat::Id3v2) => vec!["TCON", riff_info::GENRE],
            (TagField::Genre, TagFormat::Mp4) => vec!["©gen"],
            (TagField::Genre, TagFormat::VorbisComments) => vec!["GENRE"],
            (TagField::Year, TagFormat::Id3v2) if id3_version == Some(4) => {
                vec!["TDRC", riff_info::DATE]
            }
            (TagField::Year, TagFormat::Id3v2) => vec!["TYER", riff_info::DATE],
            (TagField::Year, TagFormat::Mp4) => vec!["©day"],
            (TagField::Year, TagFormat::VorbisComments) => vec!["DATE"],
            (TagField::TrackNumber, TagFormat::Id3v2) => vec!["TRCK", riff_info::TRACK_NUMBER],
            (TagField::TrackNumber, TagFormat::Mp4) => vec![MP4_TRACK_NUMBER],
            (TagField::TrackNumber, TagFormat::VorbisComments) => vec!["TRACKNUMBER"],
            (TagField::Comment, _) => format.comment_fields().to_vec(),
            (TagField::Grouping, _) => format.grouping_fields().to_vec(),
            (TagField::Key, _) => format.key_fields().to_vec(),
            (TagField::Bpm, TagFormat::Id3v2) => vec![preset::ID3_BPM],
            (TagField::Bpm, TagFormat::Mp4) => vec![preset::MP4_FREEFORM_BPM, MP4_TEMPO],
            (TagField::Bpm, TagFormat::VorbisComments) => {
                vec![preset::VORBIS_BPM, preset::VORBIS_TEMPO]
            }
        }
    }

    /// True for the fields that can hold the key and energy label.
    fn may_hold_label(self) -> bool {
        matches!(self, TagField::Comment | TagField::Grouping)
    }
}

impl fmt::Display for TagField {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.label())
    }
}

/// A change to the text of a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextChange {
    /// Gives the field this text. An empty text removes the field.
    Set(String),
    /// Removes the field. A comment or grouping keeps its label.
    Clear,
    /// Replaces every occurrence of `find`. Without `match_case`, letters
    /// match in either case.
    Replace {
        /// The text to look for. An empty text changes nothing.
        find: String,
        /// What takes its place.
        replace: String,
        /// Whether "house" and "House" are different.
        match_case: bool,
    },
    /// Puts text in front, unless the field already starts with it. An
    /// absent or empty field stays so: a title that is only `"[Edit] "` helps
    /// nobody.
    AddPrefix(String),
    /// Puts text at the end, unless the field already ends with it. An
    /// absent or empty field stays so.
    AddSuffix(String),
    /// Takes text off the front, when the field starts with it.
    RemovePrefix(String),
    /// Takes text off the end, when the field ends with it.
    RemoveSuffix(String),
}

impl TextChange {
    /// The text a field holding `current` gets, `None` when the field is
    /// removed. An empty field is no field: a change that leaves nothing
    /// removes it, and a prefix or a suffix is only added to a field that
    /// has text.
    pub fn apply(&self, current: Option<&str>) -> Option<String> {
        let current = current.unwrap_or_default();
        if current.is_empty() && matches!(self, TextChange::AddPrefix(_) | TextChange::AddSuffix(_))
        {
            return None;
        }
        let new = match self {
            TextChange::Set(text) => text.clone(),
            TextChange::Clear => String::new(),
            TextChange::Replace {
                find,
                replace,
                match_case,
            } => replace_all(current, find, replace, *match_case),
            TextChange::AddPrefix(prefix) if current.starts_with(prefix.as_str()) => {
                current.to_string()
            }
            TextChange::AddPrefix(prefix) => format!("{prefix}{current}"),
            TextChange::AddSuffix(suffix) if current.ends_with(suffix.as_str()) => {
                current.to_string()
            }
            TextChange::AddSuffix(suffix) => format!("{current}{suffix}"),
            TextChange::RemovePrefix(prefix) => current
                .strip_prefix(prefix.as_str())
                .unwrap_or(current)
                .to_string(),
            TextChange::RemoveSuffix(suffix) => current
                .strip_suffix(suffix.as_str())
                .unwrap_or(current)
                .to_string(),
        };
        (!new.is_empty()).then_some(new)
    }
}

/// `text` with every occurrence of `find` replaced, left to right, without
/// looking again at what was put in.
fn replace_all(text: &str, find: &str, replace: &str, match_case: bool) -> String {
    if find.is_empty() {
        return text.to_string();
    }
    if match_case {
        return text.replace(find, replace);
    }
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(length) = matches_ignoring_case(rest, find) {
            result.push_str(replace);
            rest = &rest[length..];
        } else {
            let mut characters = rest.chars();
            if let Some(character) = characters.next() {
                result.push(character);
            }
            rest = characters.as_str();
        }
    }
    result
}

/// The length in bytes of the start of `text` that is `pattern` in either
/// case, when it is.
fn matches_ignoring_case(text: &str, pattern: &str) -> Option<usize> {
    let mut text_characters = text.char_indices();
    for wanted in pattern.chars() {
        let (_, found) = text_characters.next()?;
        if !found.to_lowercase().eq(wanted.to_lowercase()) {
            return None;
        }
    }
    Some(
        text_characters
            .next()
            .map_or(text.len(), |(index, _)| index),
    )
}

/// One field and the change to make to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldEdit {
    /// The field.
    pub field: TagField,
    /// What happens to its text.
    pub change: TextChange,
}

/// The editable fields of one file, as an editor shows them: the comment
/// and the grouping without the key and energy label.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditableTags {
    values: Vec<(TagField, Option<String>)>,
}

impl EditableTags {
    /// Tags holding these values; the other fields are absent.
    pub fn from_values(values: Vec<(TagField, String)>) -> Self {
        Self {
            values: values
                .into_iter()
                .map(|(field, text)| (field, (!text.is_empty()).then_some(text)))
                .collect(),
        }
    }

    /// The text of `field`, `None` when the file does not have it.
    pub fn get(&self, field: TagField) -> Option<&str> {
        self.values
            .iter()
            .find(|(candidate, _)| *candidate == field)
            .and_then(|(_, value)| value.as_deref())
    }
}

/// Reads the editable fields of the file at `path`.
pub fn read_editable_tags(path: &Path) -> Result<EditableTags, EuphoriaError> {
    let file = TagFile::load(path, Purpose::Reading)?;
    let format = file.format();
    let id3_version = file.id3_version();
    let values = TagField::ALL
        .into_iter()
        .map(|field| {
            let native = field.native_fields(format, id3_version);
            let value = file.first_of(&native)?.map(|text| {
                if field.may_hold_label() {
                    split_labels(&text).1.to_string()
                } else {
                    text
                }
            });
            Ok((field, value.filter(|text| !text.is_empty())))
        })
        .collect::<Result<_, EuphoriaError>>()?;
    Ok(EditableTags { values })
}

/// Computes the tag changes that `edits` make to the file at `path`, in
/// order: several edits of one field apply one after the other. The file
/// is only read.
///
/// The main native field of each edited field is written, and the other
/// native fields of the same meaning that the file has are kept in
/// agreement, such as the comment of a WAV file's `INFO` list or the
/// integer tempo of an M4A file.
///
/// # Errors
///
/// [`EuphoriaError::TagWrite`] when an edit leaves a value the field cannot
/// hold, such as a tempo that is not a positive number or a track number
/// an M4A file cannot store, and the errors of reading the file.
pub fn plan_edit(path: &Path, edits: &[FieldEdit]) -> Result<WritePlan, EuphoriaError> {
    let refuse = |reason: String| EuphoriaError::TagWrite {
        path: path.to_path_buf(),
        source: reason.into(),
    };
    let file = TagFile::load(path, Purpose::Rewriting)?;
    let format = file.format();
    let id3_version = file.id3_version();
    let mut edited: Vec<TagField> = edits.iter().map(|edit| edit.field).collect();
    edited.sort();
    edited.dedup();

    let mut changes = Vec::new();
    for field in edited {
        let field_edits: Vec<&TextChange> = edits
            .iter()
            .filter(|edit| edit.field == field)
            .map(|edit| &edit.change)
            .collect();
        for (position, native) in field
            .native_fields(format, id3_version)
            .into_iter()
            .enumerate()
        {
            let current = file.get(native)?;
            // A WAV file's INFO list is where rekordbox and others read its
            // title and comment, so it gets the value whether it had it or
            // not; any other second field only when the file has it.
            let is_info_value = riff_info::value_id(native).is_some();
            let writes_absent = if is_info_value {
                file.has_info_list()
            } else {
                position == 0
            };
            if current.is_none() && !writes_absent {
                continue;
            }
            let new = edited_text(field, current.as_deref(), &field_edits);
            let new = match (field, new) {
                (TagField::Bpm, Some(text)) => Some(bpm_text(native, &text).map_err(refuse)?),
                (TagField::TrackNumber, Some(text)) => {
                    Some(track_number_text(&text).map_err(refuse)?)
                }
                (_, new) => new,
            };
            let unchanged = match (&current, &new) {
                (Some(current), Some(new)) => {
                    current == new || (field == TagField::Bpm && same_bpm(current, new))
                }
                (None, None) => true,
                _ => false,
            };
            if !unchanged {
                changes.push(FieldChange {
                    field: native.to_string(),
                    current,
                    new,
                });
            }
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

/// The text of a native field after `changes`, keeping a label the field
/// holds around the user's own text.
fn edited_text(field: TagField, current: Option<&str>, changes: &[&TextChange]) -> Option<String> {
    let (leading, own, trailing) = match current {
        Some(text) if field.may_hold_label() => split_labels(text),
        Some(text) => (None, text, None),
        None => (None, "", None),
    };
    let own = changes.iter().fold(
        (!own.is_empty()).then(|| own.to_string()),
        |text, change| change.apply(text.as_deref()),
    );
    let own = own.unwrap_or_default();
    // Text that brings a label of its own, such as a whole comment typed
    // again with its label, takes the old label's place rather than
    // adding a second one.
    let (typed_leading, own, typed_trailing) = if field.may_hold_label() {
        split_labels(&own)
    } else {
        (None, own.as_str(), None)
    };
    let joined = join_labels(typed_leading.or(leading), own, typed_trailing.or(trailing));
    (!joined.is_empty()).then_some(joined)
}

/// The tempo as `native` stores it: an M4A file's integer tempo item holds
/// whole numbers.
fn bpm_text(native: &str, text: &str) -> Result<String, String> {
    let tempo: f32 = text
        .trim()
        .parse()
        .ok()
        .filter(|tempo: &f32| tempo.is_finite() && *tempo > 0.0 && *tempo < 1000.0)
        .ok_or_else(|| format!("{text:?} is not a tempo in beats per minute"))?;
    if native == MP4_TEMPO {
        return Ok(format_bpm(tempo, false));
    }
    let typed = text.trim();
    Ok(if typed.parse::<f32>().is_ok() {
        typed.to_string()
    } else {
        format_bpm(tempo, true)
    })
}

/// A track number such as "7" or "7/12".
fn track_number_text(text: &str) -> Result<String, String> {
    let refuse = || format!("{text:?} is not a track number such as 7 or 7/12");
    let (track, total) = match text.trim().split_once('/') {
        Some((track, total)) => (track.trim(), Some(total.trim())),
        None => (text.trim(), None),
    };
    let track: u16 = track.parse().map_err(|_| refuse())?;
    match total {
        Some(total) => {
            let total: u16 = total.parse().map_err(|_| refuse())?;
            Ok(format!("{track}/{total}"))
        }
        None => Ok(track.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(text: &str) -> TextChange {
        TextChange::Set(text.to_string())
    }

    #[test]
    fn changes_give_the_text_a_field_gets() {
        assert_eq!(set("House").apply(Some("Techno")).as_deref(), Some("House"));
        assert_eq!(set("").apply(Some("Techno")), None);
        assert_eq!(TextChange::Clear.apply(Some("Techno")), None);
        let prefix = TextChange::AddPrefix("[DJ] ".to_string());
        assert_eq!(prefix.apply(Some("Mix")).as_deref(), Some("[DJ] Mix"));
        assert_eq!(prefix.apply(Some("[DJ] Mix")).as_deref(), Some("[DJ] Mix"));
        assert_eq!(prefix.apply(None), None, "an absent field stays absent");
        assert_eq!(prefix.apply(Some("")), None);
        let suffix = TextChange::AddSuffix(" (Edit)".to_string());
        assert_eq!(suffix.apply(Some("Song")).as_deref(), Some("Song (Edit)"));
        assert_eq!(
            suffix.apply(Some("Song (Edit)")).as_deref(),
            Some("Song (Edit)")
        );
        let unprefix = TextChange::RemovePrefix("The ".to_string());
        assert_eq!(unprefix.apply(Some("The Band")).as_deref(), Some("Band"));
        assert_eq!(unprefix.apply(Some("Band")).as_deref(), Some("Band"));
        assert_eq!(
            TextChange::RemoveSuffix(" (Edit)".to_string()).apply(Some(" (Edit)")),
            None
        );
    }

    #[test]
    fn replace_finds_every_occurrence_with_or_without_case() {
        let replace = |find: &str, with: &str, match_case: bool, text: &str| {
            TextChange::Replace {
                find: find.to_string(),
                replace: with.to_string(),
                match_case,
            }
            .apply(Some(text))
        };
        assert_eq!(
            replace("house", "House", true, "deep house / house").as_deref(),
            Some("deep House / House")
        );
        assert_eq!(
            replace("HOUSE", "House", false, "deep house / HoUsE").as_deref(),
            Some("deep House / House")
        );
        assert_eq!(
            replace("house", "House", true, "Deep HOUSE").as_deref(),
            Some("Deep HOUSE")
        );
        assert_eq!(
            replace("é", "e", false, "Café CAFÉ").as_deref(),
            Some("Cafe CAFe")
        );
        assert_eq!(
            replace("a", "aa", false, "banana").as_deref(),
            Some("baanaanaa"),
            "what was put in is not looked at again"
        );
        assert_eq!(replace("", "x", false, "text").as_deref(), Some("text"));
        assert_eq!(replace("text", "", true, "text"), None);
    }

    #[test]
    fn fields_are_found_by_name_or_a_common_spelling() {
        for field in TagField::ALL {
            assert_eq!(TagField::from_name(field.name()), Some(field));
        }
        assert_eq!(TagField::from_name("Tempo"), Some(TagField::Bpm));
        assert_eq!(
            TagField::from_name("track-number"),
            Some(TagField::TrackNumber)
        );
        assert_eq!(
            TagField::from_name("TRACKNUMBER"),
            Some(TagField::TrackNumber)
        );
        assert_eq!(TagField::from_name("date"), Some(TagField::Year));
        assert_eq!(TagField::from_name("mood"), None);
    }

    #[test]
    fn an_edit_of_a_labelled_comment_keeps_the_label_once() {
        let comment = |current: &str, change: TextChange| {
            edited_text(TagField::Comment, Some(current), &[&change])
        };
        assert_eq!(
            comment("8A - Energy 7 - Purchased", set("Peak opener")).as_deref(),
            Some("8A - Energy 7 - Peak opener")
        );
        assert_eq!(
            comment("8A - Energy 7 - Purchased", TextChange::Clear).as_deref(),
            Some("8A - Energy 7")
        );
        assert_eq!(
            comment("8A - Energy 7", set("8A - Energy 7 - Opener")).as_deref(),
            Some("8A - Energy 7 - Opener"),
            "a comment typed again with its label does not get a second one"
        );
        assert_eq!(
            comment("8A - Energy 7 - Old", set("9A - Energy 8 - New")).as_deref(),
            Some("9A - Energy 8 - New")
        );
        assert_eq!(
            comment("Plain", set("8A - side B")).as_deref(),
            Some("8A - side B"),
            "a key without an energy rating is the user's own text"
        );
        assert_eq!(
            comment(
                "Purchased - 2A - Energy 3",
                TextChange::AddPrefix("Old: ".to_string())
            )
            .as_deref(),
            Some("Old: Purchased - 2A - Energy 3")
        );
        assert_eq!(
            edited_text(
                TagField::Title,
                Some("8A - Energy 7"),
                &[&TextChange::Clear]
            ),
            None,
            "only the comment and the grouping hold labels"
        );
    }

    #[test]
    fn tempos_and_track_numbers_are_checked_and_stored_as_the_field_needs() {
        assert_eq!(bpm_text("TBPM", "124.50").unwrap(), "124.50");
        assert_eq!(bpm_text(MP4_TEMPO, "124.6").unwrap(), "125");
        assert!(bpm_text("TBPM", "fast").is_err());
        assert!(bpm_text("TBPM", "-3").is_err());
        assert_eq!(track_number_text(" 7 / 12 ").unwrap(), "7/12");
        assert_eq!(track_number_text("07").unwrap(), "7");
        assert!(track_number_text("A1").is_err());
    }
}
