// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Access to the tag a file format stores natively, one named field at a
//! time, so that everything the caller does not name is written back as it
//! was read.

use std::borrow::Cow;
use std::ffi::OsString;
use std::fs::File;
use std::io::{BufReader, Seek};
use std::path::{Path, PathBuf};

use lofty::TextEncoding;
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, FileType};
use lofty::flac::FlacFile;
use lofty::id3::v2::{CommentFrame, Frame, FrameId, Id3v2Tag, Id3v2Version, TextInformationFrame};
use lofty::iff::aiff::AiffFile;
use lofty::iff::wav::WavFile;
use lofty::mp4::{Atom, AtomData, AtomIdent, DataType, Ilst, Mp4File};
use lofty::mpeg::MpegFile;
use lofty::ogg::tag::VorbisComments;
use lofty::probe::Probe;
use lofty::tag::TagExt;

use super::TagFormat;
use crate::error::{ErrorSource, EuphoriaError};

/// The ID3v2 comment frame. Only the comment with an empty description is
/// read or written; described comments belong to other programs.
pub(super) const ID3_COMMENT: &str = "COMM";
/// The MP4 tempo atom, which holds a whole number.
pub(super) const MP4_TEMPO: &str = "tmpo";

const MP4_FREEFORM_PREFIX: &str = "----:";
const MP4_COPYRIGHT_SIGN_BYTE: u8 = 0xA9;
const ID3_DEFAULT_LANGUAGE: [u8; 3] = *b"eng";
const TEMPORARY_SUFFIX: &str = ".euphoria-tmp";

enum NativeTag {
    Id3v2(Id3v2Tag),
    Mp4(Ilst),
    Vorbis(VorbisComments),
}

/// The native tag of one file, loaded in full.
pub(super) struct TagFile {
    path: PathBuf,
    tag: NativeTag,
    write_id3v23: bool,
}

impl TagFile {
    /// Reads the native tag of the file at `path`. A file with no tag yet
    /// loads as an empty one.
    pub(super) fn load(path: &Path) -> Result<Self, EuphoriaError> {
        let read_error = |source: ErrorSource| EuphoriaError::TagRead {
            path: path.to_path_buf(),
            source,
        };

        let file = File::open(path).map_err(|source| EuphoriaError::Open {
            path: path.to_path_buf(),
            source,
        })?;
        let probe = Probe::new(BufReader::new(file))
            .guess_file_type()
            .map_err(|error| read_error(error.into()))?;
        let file_type = probe.file_type();
        let mut reader = probe.into_inner();
        reader.rewind().map_err(|error| read_error(error.into()))?;

        // Implicit conversions rewrite frames the caller never asked to
        // touch, such as ID3v2.3 dates, so they stay off.
        let options = ParseOptions::new()
            .read_properties(false)
            .implicit_conversions(false);
        let mut write_id3v23 = false;
        let mut id3v2 = |tag: Option<Id3v2Tag>| {
            write_id3v23 = tag
                .as_ref()
                .is_none_or(|tag| tag.original_version() != Id3v2Version::V4);
            NativeTag::Id3v2(tag.unwrap_or_default())
        };
        let tag = match file_type {
            Some(FileType::Mpeg) => {
                MpegFile::read_from(&mut reader, options).map(|mut file| id3v2(file.remove_id3v2()))
            }
            Some(FileType::Wav) => {
                WavFile::read_from(&mut reader, options).map(|mut file| id3v2(file.remove_id3v2()))
            }
            Some(FileType::Aiff) => {
                AiffFile::read_from(&mut reader, options).map(|mut file| id3v2(file.remove_id3v2()))
            }
            Some(FileType::Mp4) => Mp4File::read_from(&mut reader, options)
                .map(|mut file| NativeTag::Mp4(file.remove_ilst().unwrap_or_default())),
            Some(FileType::Flac) => FlacFile::read_from(&mut reader, options).map(|mut file| {
                NativeTag::Vorbis(file.remove_vorbis_comments().unwrap_or_default())
            }),
            _ => {
                return Err(read_error("not an MP3, WAV, AIFF, M4A or FLAC file".into()));
            }
        }
        .map_err(|error| read_error(error.into()))?;

        Ok(Self {
            path: path.to_path_buf(),
            tag,
            write_id3v23,
        })
    }

    pub(super) fn format(&self) -> TagFormat {
        match self.tag {
            NativeTag::Id3v2(_) => TagFormat::Id3v2,
            NativeTag::Mp4(_) => TagFormat::Mp4,
            NativeTag::Vorbis(_) => TagFormat::VorbisComments,
        }
    }

    /// The text of `field`, or `None` when the tag does not have it.
    pub(super) fn get(&self, field: &str) -> Result<Option<String>, EuphoriaError> {
        match &self.tag {
            NativeTag::Id3v2(tag) => id3v2_get(tag, field),
            NativeTag::Mp4(ilst) => mp4_get(ilst, field),
            NativeTag::Vorbis(comments) => Ok(comments.get(field).map(str::to_string)),
        }
        .map_err(|source| EuphoriaError::TagRead {
            path: self.path.clone(),
            source,
        })
    }

    /// The first of `fields` that holds text.
    pub(super) fn first_of(&self, fields: &[&str]) -> Result<Option<String>, EuphoriaError> {
        for field in fields {
            if let Some(value) = self.get(field)? {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    /// Sets `field` to `value` in memory, or removes it when `value` is
    /// `None`. A field that already exists keeps its place in the tag.
    pub(super) fn set(&mut self, field: &str, value: Option<&str>) -> Result<(), EuphoriaError> {
        match &mut self.tag {
            NativeTag::Id3v2(tag) => id3v2_set(tag, field, value),
            NativeTag::Mp4(ilst) => mp4_set(ilst, field, value),
            NativeTag::Vorbis(comments) => {
                vorbis_set(comments, field, value);
                Ok(())
            }
        }
        .map_err(|source| self.write_error(source))
    }

    /// Writes the tag to the file. The new contents are built in a copy next
    /// to the file and moved over it in one step, so an interrupted write
    /// leaves the original whole.
    pub(super) fn save(&self) -> Result<(), EuphoriaError> {
        let mut temporary_name = OsString::from(".");
        temporary_name.push(self.path.file_name().unwrap_or_default());
        temporary_name.push(TEMPORARY_SUFFIX);
        let temporary_path = self.path.with_file_name(temporary_name);

        let written = self.save_through(&temporary_path);
        if written.is_err() {
            // The write error is the one worth reporting; a leftover copy
            // is replaced by the next write to the same file.
            let _ = std::fs::remove_file(&temporary_path);
        }
        written.map_err(|source| self.write_error(source))
    }

    fn save_through(&self, temporary_path: &Path) -> Result<(), ErrorSource> {
        std::fs::copy(&self.path, temporary_path)?;
        let options = WriteOptions::new().use_id3v23(self.write_id3v23);
        match &self.tag {
            NativeTag::Id3v2(tag) => tag.save_to_path(temporary_path, options)?,
            NativeTag::Mp4(ilst) => ilst.save_to_path(temporary_path, options)?,
            NativeTag::Vorbis(comments) => comments.save_to_path(temporary_path, options)?,
        }
        std::fs::rename(temporary_path, &self.path)?;
        Ok(())
    }

    fn write_error(&self, source: ErrorSource) -> EuphoriaError {
        EuphoriaError::TagWrite {
            path: self.path.clone(),
            source,
        }
    }
}

fn is_plain_comment(frame: &Frame<'_>) -> bool {
    matches!(frame, Frame::Comment(comment) if comment.description.is_empty())
}

fn id3v2_get(tag: &Id3v2Tag, field: &str) -> Result<Option<String>, ErrorSource> {
    if field == ID3_COMMENT {
        return Ok(tag
            .comments()
            .next()
            .map(|comment| comment.content.to_string()));
    }
    let frame_id = FrameId::new(field)?;
    match tag.get(&frame_id) {
        None => Ok(None),
        Some(Frame::Text(frame)) => Ok(Some(frame.value.to_string())),
        Some(_) => Err(format!("{field} is not a text frame").into()),
    }
}

/// The encoding to store `value` in, given the encoding the frame has now.
/// Latin-1 frames stay Latin-1 while the text fits, which keeps them
/// identical when a change is undone.
fn id3v2_encoding(current: TextEncoding, value: &str) -> TextEncoding {
    let fits_latin1 = value.chars().all(|character| u32::from(character) <= 0xFF);
    if current == TextEncoding::Latin1 && !fits_latin1 {
        TextEncoding::UTF16
    } else {
        current
    }
}

fn id3v2_set(tag: &mut Id3v2Tag, field: &str, value: Option<&str>) -> Result<(), ErrorSource> {
    let frame_id = FrameId::new(field)?.into_owned();
    let is_target = |frame: &Frame<'_>| {
        if field == ID3_COMMENT {
            is_plain_comment(frame)
        } else {
            *frame.id() == frame_id
        }
    };

    let mut found = false;
    tag.retain_mut(|frame| {
        if found || !is_target(frame) {
            return true;
        }
        found = true;
        let Some(value) = value else {
            return false;
        };
        match frame {
            Frame::Comment(comment) => {
                comment.encoding = id3v2_encoding(comment.encoding, value);
                comment.content = Cow::Owned(value.to_string());
            }
            Frame::Text(text) => {
                text.encoding = id3v2_encoding(text.encoding, value);
                text.value = Cow::Owned(value.to_string());
            }
            other => {
                *other = Frame::Text(TextInformationFrame::new(
                    frame_id.clone(),
                    TextEncoding::UTF8,
                    value.to_string(),
                ));
            }
        }
        true
    });

    if let (false, Some(value)) = (found, value) {
        let frame = if field == ID3_COMMENT {
            Frame::Comment(CommentFrame::new(
                TextEncoding::UTF8,
                ID3_DEFAULT_LANGUAGE,
                "",
                value.to_string(),
            ))
        } else {
            Frame::Text(TextInformationFrame::new(
                frame_id,
                TextEncoding::UTF8,
                value.to_string(),
            ))
        };
        tag.insert(frame);
    }
    Ok(())
}

fn mp4_ident(field: &str) -> Result<AtomIdent<'static>, ErrorSource> {
    if let Some(freeform) = field.strip_prefix(MP4_FREEFORM_PREFIX) {
        let (mean, name) = freeform
            .split_once(':')
            .ok_or_else(|| format!("{field} is not a freeform atom name"))?;
        return Ok(AtomIdent::Freeform {
            mean: Cow::Owned(mean.to_string()),
            name: Cow::Owned(name.to_string()),
        });
    }
    let bytes: Vec<u8> = field
        .chars()
        .map(|character| match character {
            '©' => Some(MP4_COPYRIGHT_SIGN_BYTE),
            other => u8::try_from(other).ok().filter(u8::is_ascii),
        })
        .collect::<Option<_>>()
        .ok_or_else(|| format!("{field} is not an atom name"))?;
    let fourcc: [u8; 4] = bytes
        .try_into()
        .map_err(|_| format!("{field} is not a four character atom name"))?;
    Ok(AtomIdent::Fourcc(fourcc))
}

/// Freeform atom names differ in letter case between programs, so they
/// match in any case. Four character names match exactly.
fn mp4_ident_matches(candidate: &AtomIdent<'_>, wanted: &AtomIdent<'_>) -> bool {
    match (candidate, wanted) {
        (
            AtomIdent::Freeform { mean, name },
            AtomIdent::Freeform {
                mean: wanted_mean,
                name: wanted_name,
            },
        ) => mean.eq_ignore_ascii_case(wanted_mean) && name.eq_ignore_ascii_case(wanted_name),
        _ => candidate == wanted,
    }
}

fn mp4_get(ilst: &Ilst, field: &str) -> Result<Option<String>, ErrorSource> {
    let ident = mp4_ident(field)?;
    let Some(atom) = ilst
        .into_iter()
        .find(|atom| mp4_ident_matches(atom.ident(), &ident))
    else {
        return Ok(None);
    };
    match atom.data().next() {
        None => Ok(None),
        Some(AtomData::UTF8(text) | AtomData::UTF16(text)) => Ok(Some(text.clone())),
        Some(AtomData::SignedInteger(number)) => Ok(Some(number.to_string())),
        Some(AtomData::UnsignedInteger(number)) => Ok(Some(number.to_string())),
        // Refusing here keeps the planner from replacing data that a
        // backup could not hold as text.
        Some(_) => Err(format!("{field} holds data that is not text or a number").into()),
    }
}

fn mp4_atom(
    ident: AtomIdent<'static>,
    field: &str,
    value: &str,
) -> Result<Atom<'static>, ErrorSource> {
    if field != MP4_TEMPO {
        return Ok(Atom::new(ident, AtomData::UTF8(value.to_string())));
    }
    let tempo: u16 = value
        .parse()
        .map_err(|_| format!("{value:?} is not a whole tempo that fits in {MP4_TEMPO}"))?;
    // Players expect exactly two bytes here, and the integer variant of
    // AtomData picks its own width.
    Ok(Atom::new(
        ident,
        AtomData::Unknown {
            code: DataType::BeSignedInteger,
            data: tempo.to_be_bytes().to_vec(),
        },
    ))
}

fn mp4_set(ilst: &mut Ilst, field: &str, value: Option<&str>) -> Result<(), ErrorSource> {
    let ident = mp4_ident(field)?;
    let mut replacement = None;
    if let Some(value) = value {
        let existing_ident = (&*ilst)
            .into_iter()
            .find(|atom| mp4_ident_matches(atom.ident(), &ident))
            .map(|atom| atom.ident().clone().into_owned());
        replacement = Some(mp4_atom(
            existing_ident.unwrap_or(ident.clone()),
            field,
            value,
        )?);
    }

    // Ilst has no way to edit an atom where it sits, so the list is rebuilt
    // in its original order.
    let atoms: Vec<Atom<'static>> = std::mem::take(ilst).into_iter().collect();
    for atom in atoms {
        if !mp4_ident_matches(atom.ident(), &ident) {
            ilst.insert(atom);
        } else if let Some(replacement) = replacement.take() {
            ilst.insert(replacement);
        }
    }
    if let Some(replacement) = replacement {
        ilst.insert(replacement);
    }
    Ok(())
}

fn vorbis_set(comments: &mut VorbisComments, field: &str, value: Option<&str>) {
    let mut found = false;
    let items: Vec<(String, String)> = comments.take_items().collect();
    for (key, existing) in items {
        if found || !key.eq_ignore_ascii_case(field) {
            comments.push(key, existing);
            continue;
        }
        found = true;
        if let Some(value) = value {
            comments.push(key, value.to_string());
        }
    }
    if let (false, Some(value)) = (found, value) {
        comments.push(field.to_string(), value.to_string());
    }
}
