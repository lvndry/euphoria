// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Access to the tag a file format stores natively, one named field at a
//! time, so that everything the caller does not name is written back as it
//! was read. A write is checked against the original before it replaces the
//! file, and refused when anything else would change.

use std::borrow::Cow;
use std::fmt;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use lofty::TextEncoding;
use lofty::config::{ParseOptions, ParsingMode, WriteOptions};
use lofty::file::{AudioFile, FileType};
use lofty::flac::FlacFile;
use lofty::id3::v2::{CommentFrame, Frame, FrameId, Id3v2Tag, Id3v2Version, TextInformationFrame};
use lofty::iff::aiff::AiffFile;
use lofty::iff::wav::WavFile;
use lofty::mp4::{Atom, AtomData, AtomIdent, DataType, Ilst, Mp4File};
use lofty::mpeg::MpegFile;
use lofty::ogg::OggPictureStorage;
use lofty::ogg::tag::VorbisComments;
use lofty::picture::{Picture, PictureInformation};
use lofty::probe::Probe;
use lofty::tag::TagExt;

use super::TagFormat;
use super::replace::Replacement;
use crate::error::{ErrorSource, EuphoriaError};

/// The ID3v2 comment frame. Only the comment with an empty description is
/// read or written; described comments belong to other programs.
pub(super) const ID3_COMMENT: &str = "COMM";
/// The MP4 tempo atom, which holds a whole number.
pub(super) const MP4_TEMPO: &str = "tmpo";

const MP4_FREEFORM_PREFIX: &str = "----:";
const MP4_COPYRIGHT_SIGN_BYTE: u8 = 0xA9;
const ID3_DEFAULT_LANGUAGE: [u8; 3] = *b"eng";
const ID3_ENCODING_NAMES: [&str; 4] = ["UTF16BE", "UTF16", "UTF8", "Latin1"];
const ID3_HEADER_LENGTH: usize = 10;
const ID3_FOOTER_FLAG: u8 = 0x10;
const IFF_HEADER_LENGTH: u64 = 12;
const STRICT_CHECK_PADDING: usize = 512;
const UNPARSABLE: &str = "its tags hold data that cannot be parsed and would be lost";

/// What a file is loaded for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Purpose {
    /// Showing its tags. Damaged parts of a tag are skipped.
    Reading,
    /// Changing its tags. A tag with parts that cannot be parsed is refused,
    /// because writing it back would drop them.
    Rewriting,
}

enum NativeTag {
    Id3v2(Id3v2Tag),
    Mp4(Ilst),
    /// The whole file, because FLAC keeps pictures in blocks of their own
    /// that are written together with the comments.
    Flac(Box<FlacFile>),
}

/// The metadata of a file that lives outside the tag being written. A write
/// must leave all of it equal.
#[derive(PartialEq)]
struct Bystanders {
    other_tags: String,
    pictures: Vec<(Picture, PictureInformation)>,
}

struct Contents {
    tag: NativeTag,
    bystanders: Bystanders,
    id3v2_version: Option<Id3v2Version>,
}

/// Why a file that can be read is not rewritten.
#[derive(Debug)]
struct Refusal {
    reason: &'static str,
    source: Option<ErrorSource>,
}

impl Refusal {
    fn boxed(reason: &'static str) -> ErrorSource {
        Box::new(Self {
            reason,
            source: None,
        })
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "refusing to rewrite the file: {}", self.reason)
    }
}

impl std::error::Error for Refusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn std::error::Error + 'static))
    }
}

fn parse_options(purpose: Purpose) -> ParseOptions {
    let parsing_mode = match purpose {
        Purpose::Reading => ParsingMode::BestAttempt,
        Purpose::Rewriting => ParsingMode::Strict,
    };
    // Implicit conversions rewrite frames the caller never asked to touch,
    // such as ID3v2.3 dates, so they stay off.
    ParseOptions::new()
        .read_properties(false)
        .implicit_conversions(false)
        .parsing_mode(parsing_mode)
}

fn read_contents<R: Read + Seek>(reader: R, purpose: Purpose) -> Result<Contents, ErrorSource> {
    let probe = Probe::new(reader).guess_file_type()?;
    let file_type = probe.file_type();
    let mut reader = probe.into_inner();
    reader.rewind()?;
    let options = parse_options(purpose);
    // The other tags of an ID3 file are never rewritten, so their flaws must
    // not stop a write: the file is parsed leniently and the ID3v2 tag is
    // then checked strictly on its own.
    let lenient = parse_options(Purpose::Reading);
    let check_id3v2 = |reader: &mut R, layout: Id3v2Layout, found: bool| {
        if purpose == Purpose::Rewriting && found {
            check_id3v2_strictly(reader, layout)
        } else {
            Ok(())
        }
    };

    let id3v2 = |tag: Option<Id3v2Tag>, other_tags: String| Contents {
        id3v2_version: tag.as_ref().map(Id3v2Tag::original_version),
        tag: NativeTag::Id3v2(tag.unwrap_or_default()),
        bystanders: Bystanders {
            other_tags,
            pictures: Vec::new(),
        },
    };
    match file_type {
        Some(FileType::Mpeg) => {
            let mut file = MpegFile::read_from(&mut reader, lenient)?;
            check_id3v2(
                &mut reader,
                Id3v2Layout::StartOfFile,
                file.id3v2().is_some(),
            )?;
            let other_tags = format!("{:?}", (file.id3v1(), file.ape()));
            Ok(id3v2(file.remove_id3v2(), other_tags))
        }
        Some(FileType::Wav) => {
            let mut file = WavFile::read_from(&mut reader, lenient)?;
            check_id3v2(&mut reader, Id3v2Layout::RiffChunk, file.id3v2().is_some())?;
            let other_tags = format!("{:?}", file.riff_info());
            Ok(id3v2(file.remove_id3v2(), other_tags))
        }
        Some(FileType::Aiff) => {
            let mut file = AiffFile::read_from(&mut reader, lenient)?;
            check_id3v2(&mut reader, Id3v2Layout::AiffChunk, file.id3v2().is_some())?;
            let other_tags = format!("{:?}", file.text_chunks());
            Ok(id3v2(file.remove_id3v2(), other_tags))
        }
        Some(FileType::Mp4) => {
            let mut file = Mp4File::read_from(&mut reader, options)?;
            Ok(Contents {
                tag: NativeTag::Mp4(file.remove_ilst().unwrap_or_default()),
                bystanders: Bystanders {
                    other_tags: String::new(),
                    pictures: Vec::new(),
                },
                id3v2_version: None,
            })
        }
        Some(FileType::Flac) => {
            let mut file = FlacFile::read_from(&mut reader, options)?;
            // An ID3v2 tag in front of a FLAC stream is left where it is:
            // taken out of the file here, it is not written back, and the
            // writer skips over its bytes.
            let other_tags = format!("{:?}", file.remove_id3v2());
            let pictures = file.pictures().to_vec();
            if file.vorbis_comments().is_none() {
                file.set_vorbis_comments(VorbisComments::default());
            }
            Ok(Contents {
                tag: NativeTag::Flac(Box::new(file)),
                bystanders: Bystanders {
                    other_tags,
                    pictures,
                },
                id3v2_version: None,
            })
        }
        _ => Err("not an MP3, WAV, AIFF, M4A or FLAC file".into()),
    }
}

/// Where a file format keeps its ID3v2 tag.
#[derive(Clone, Copy)]
enum Id3v2Layout {
    /// At the very start, as in MP3.
    StartOfFile,
    /// In an `ID3 ` chunk with little endian sizes, as in WAV.
    RiffChunk,
    /// In an `ID3 ` chunk with big endian sizes, as in AIFF.
    AiffChunk,
}

/// The bytes of the file's ID3v2 tag, or `None` when it is not where the
/// format puts it.
fn raw_id3v2<R: Read + Seek>(
    reader: &mut R,
    layout: Id3v2Layout,
) -> Result<Option<Vec<u8>>, ErrorSource> {
    let mut tag = Vec::new();
    match layout {
        Id3v2Layout::StartOfFile => {
            reader.rewind()?;
            let mut header = [0; ID3_HEADER_LENGTH];
            if reader.read_exact(&mut header).is_err() || &header[..3] != b"ID3" {
                return Ok(None);
            }
            let body_length = header[6..].iter().fold(0_u64, |length, byte| {
                (length << 7) | u64::from(*byte & 0x7F)
            });
            let has_footer = header[5] & ID3_FOOTER_FLAG != 0;
            let footer_length = if has_footer {
                ID3_HEADER_LENGTH as u64
            } else {
                0
            };
            tag.extend(header);
            reader
                .take(body_length + footer_length)
                .read_to_end(&mut tag)?;
            let mut next = [0; 3];
            if reader.read_exact(&mut next).is_ok() && &next == b"ID3" {
                return Err(Refusal::boxed("it holds more than one ID3v2 tag"));
            }
        }
        Id3v2Layout::RiffChunk | Id3v2Layout::AiffChunk => {
            reader.seek(SeekFrom::Start(IFF_HEADER_LENGTH))?;
            loop {
                let mut chunk_header = [0; 8];
                if reader.read_exact(&mut chunk_header).is_err() {
                    return Ok(None);
                }
                let size_bytes = [
                    chunk_header[4],
                    chunk_header[5],
                    chunk_header[6],
                    chunk_header[7],
                ];
                let size = match layout {
                    Id3v2Layout::RiffChunk => u32::from_le_bytes(size_bytes),
                    _ => u32::from_be_bytes(size_bytes),
                };
                if chunk_header[..4].eq_ignore_ascii_case(b"ID3 ") {
                    reader.take(u64::from(size)).read_to_end(&mut tag)?;
                    break;
                }
                let padded_size = i64::from(size) + i64::from(size % 2);
                reader.seek(SeekFrom::Current(padded_size))?;
            }
        }
    }
    Ok(Some(tag))
}

/// Parses the file's ID3v2 tag alone and strictly, so that a frame the
/// lenient parse of the whole file skipped is reported instead of dropped
/// by the next write.
fn check_id3v2_strictly<R: Read + Seek>(
    reader: &mut R,
    layout: Id3v2Layout,
) -> Result<(), ErrorSource> {
    let Some(mut stream) = raw_id3v2(reader, layout)? else {
        return Err(Refusal::boxed(
            "its ID3v2 tag is not where the format keeps it",
        ));
    };
    // The MP3 reader is the one way to parse a bare ID3v2 tag. It looks
    // for other tags in the last bytes of the stream, which this padding
    // keeps away from the tag.
    stream.extend([0; STRICT_CHECK_PADDING]);
    MpegFile::read_from(&mut Cursor::new(stream), parse_options(Purpose::Rewriting))
        .map(|_| ())
        .map_err(|error| -> ErrorSource {
            Box::new(Refusal {
                reason: UNPARSABLE,
                source: Some(error.into()),
            })
        })
}

/// The native tag of one file, loaded in full.
pub(super) struct TagFile {
    path: PathBuf,
    contents: Contents,
    touched: Vec<String>,
}

impl TagFile {
    /// Reads the native tag of the file at `path`. A file with no tag yet
    /// loads as an empty one.
    pub(super) fn load(path: &Path, purpose: Purpose) -> Result<Self, EuphoriaError> {
        let open = || {
            File::open(path)
                .map(BufReader::new)
                .map_err(|source| EuphoriaError::Open {
                    path: path.to_path_buf(),
                    source,
                })
        };
        let contents = match read_contents(open()?, purpose) {
            Ok(contents) => contents,
            Err(source) if purpose == Purpose::Reading => {
                return Err(EuphoriaError::TagRead {
                    path: path.to_path_buf(),
                    source,
                });
            }
            Err(refusal) if refusal.is::<Refusal>() => {
                return Err(EuphoriaError::TagWrite {
                    path: path.to_path_buf(),
                    source: refusal,
                });
            }
            Err(strict_error) => {
                // A file that only a lenient parse accepts is readable, but
                // rewriting it would drop whatever the parser skipped.
                return Err(match read_contents(open()?, Purpose::Reading) {
                    Ok(_) => EuphoriaError::TagWrite {
                        path: path.to_path_buf(),
                        source: Box::new(Refusal {
                            reason: UNPARSABLE,
                            source: Some(strict_error),
                        }),
                    },
                    Err(_) => EuphoriaError::TagRead {
                        path: path.to_path_buf(),
                        source: strict_error,
                    },
                });
            }
        };
        Ok(Self {
            path: path.to_path_buf(),
            contents,
            touched: Vec::new(),
        })
    }

    pub(super) fn format(&self) -> TagFormat {
        match self.contents.tag {
            NativeTag::Id3v2(_) => TagFormat::Id3v2,
            NativeTag::Mp4(_) => TagFormat::Mp4,
            NativeTag::Flac(_) => TagFormat::VorbisComments,
        }
    }

    /// The text of `field`, or `None` when the tag does not have it.
    pub(super) fn get(&self, field: &str) -> Result<Option<String>, EuphoriaError> {
        native_get(&self.contents.tag, field).map_err(|source| EuphoriaError::TagRead {
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
        let changed = match &mut self.contents.tag {
            NativeTag::Id3v2(tag) => id3v2_set(tag, field, value),
            NativeTag::Mp4(ilst) => mp4_set(ilst, field, value),
            NativeTag::Flac(file) => {
                if let Some(comments) = file.vorbis_comments_mut() {
                    vorbis_set(comments, field, value);
                }
                Ok(())
            }
        };
        changed.map_err(|source| self.write_error(source))?;
        self.touched.push(field.to_string());
        Ok(())
    }

    /// Writes the tag to the file.
    ///
    /// The new contents are built in a copy next to the real file, read
    /// back, and compared with the original. Only when nothing but the
    /// fields passed to [`TagFile::set`] differs does the copy take the
    /// file's place, so a failed or refused write leaves the file as it was.
    /// A symbolic link is followed: the file it points to is rewritten and
    /// the link stays a link.
    pub(super) fn save(&self) -> Result<(), EuphoriaError> {
        self.save_checked()
            .map_err(|source| self.write_error(source))
    }

    fn save_checked(&self) -> Result<(), ErrorSource> {
        let target = std::fs::canonicalize(&self.path)?;
        let mut replacement = Replacement::start(&target)?;
        // An ID3v2.3 tag stays ID3v2.3 when that loses nothing. Frames that
        // only exist in ID3v2.4 are common in ID3v2.3 tags and are dropped
        // by an ID3v2.3 write, so such a tag is written as ID3v2.4 instead.
        if self.prefers_id3v23() {
            if self.write_checked(replacement.file(), true).is_ok() {
                return replacement.commit();
            }
            replacement.discard()?;
            replacement = Replacement::start(&target)?;
        }
        match self.write_checked(replacement.file(), false) {
            Ok(()) => replacement.commit(),
            Err(error) => {
                // The write error is the one reported.
                let _ = replacement.discard();
                Err(error)
            }
        }
    }

    fn prefers_id3v23(&self) -> bool {
        matches!(self.contents.tag, NativeTag::Id3v2(_))
            && self.contents.id3v2_version != Some(Id3v2Version::V4)
    }

    /// Writes the tag into `copy`, then reads `copy` back and checks it.
    fn write_checked(&self, copy: &mut File, use_id3v23: bool) -> Result<(), ErrorSource> {
        let options = WriteOptions::new()
            .use_id3v23(use_id3v23)
            .parse_options(parse_options(Purpose::Rewriting));
        copy.rewind()?;
        match &self.contents.tag {
            NativeTag::Id3v2(tag) => tag.save_to(copy, options)?,
            NativeTag::Mp4(ilst) => ilst.save_to(copy, options)?,
            NativeTag::Flac(file) => file.save_to(copy, options)?,
        }
        copy.rewind()?;
        let written = read_contents(BufReader::new(&*copy), Purpose::Rewriting)?;

        if written.bystanders != self.contents.bystanders {
            return Err(Refusal::boxed(
                "tags or pictures outside the tag being written would change",
            ));
        }
        if untouched_items(&written.tag, &self.touched)?
            != untouched_items(&self.contents.tag, &self.touched)?
        {
            return Err(Refusal::boxed(
                "tag data that was not meant to change would be lost or altered",
            ));
        }
        for field in &self.touched {
            let wanted = native_get(&self.contents.tag, field)?.filter(|text| !text.is_empty());
            let found = native_get(&written.tag, field)?.filter(|text| !text.is_empty());
            if wanted != found {
                return Err(Refusal::boxed(
                    "a field did not take the value written to it",
                ));
            }
        }
        Ok(())
    }

    fn write_error(&self, source: ErrorSource) -> EuphoriaError {
        EuphoriaError::TagWrite {
            path: self.path.clone(),
            source,
        }
    }
}

fn native_get(tag: &NativeTag, field: &str) -> Result<Option<String>, ErrorSource> {
    match tag {
        NativeTag::Id3v2(tag) => id3v2_get(tag, field),
        NativeTag::Mp4(ilst) => mp4_get(ilst, field),
        NativeTag::Flac(file) => Ok(file
            .vorbis_comments()
            .and_then(|comments| comments.get(field))
            .map(str::to_string)),
    }
}

/// Every item of the tag except the `touched` fields, as text that is equal
/// only when the item is, in an order that does not depend on the tag's.
fn untouched_items(tag: &NativeTag, touched: &[String]) -> Result<Vec<String>, ErrorSource> {
    let mut items: Vec<String> = match tag {
        NativeTag::Id3v2(tag) => tag
            .into_iter()
            .filter(|frame| !id3v2_frame_is_empty(frame))
            .filter(|frame| {
                !touched.iter().any(|field| {
                    if field == ID3_COMMENT {
                        is_plain_comment(frame)
                    } else {
                        frame.id().as_str() == field
                    }
                })
            })
            .map(id3v2_frame_text)
            .collect(),
        NativeTag::Mp4(ilst) => {
            let touched_idents = touched
                .iter()
                .map(|field| mp4_ident(field))
                .collect::<Result<Vec<_>, _>>()?;
            ilst.into_iter()
                .filter(|atom| {
                    !touched_idents
                        .iter()
                        .any(|ident| mp4_ident_matches(atom.ident(), ident))
                })
                .map(|atom| format!("{atom:?}"))
                .collect()
        }
        NativeTag::Flac(file) => file
            .vorbis_comments()
            .into_iter()
            .flat_map(VorbisComments::items)
            .filter(|(key, _)| !touched.iter().any(|field| key.eq_ignore_ascii_case(field)))
            .map(|(key, value)| format!("{key}={value}"))
            .collect(),
    };
    items.sort();
    Ok(items)
}

/// Frames without content are not written at all, so they do not count as
/// lost.
fn id3v2_frame_is_empty(frame: &Frame<'_>) -> bool {
    match frame {
        Frame::Text(text) => text.value.is_empty(),
        Frame::UserText(text) => text.content.is_empty(),
        Frame::UserUrl(link) => link.content.is_empty(),
        Frame::Comment(comment) => comment.content.is_empty(),
        Frame::UnsynchronizedText(text) => text.content.is_empty(),
        Frame::Private(private) => private.private_data.is_empty(),
        Frame::Binary(binary) => binary.data.is_empty(),
        _ => false,
    }
}

/// The frame as text, with the text encoding left out: ID3v2.3 cannot store
/// UTF-8, so a write may change the encoding without changing the text.
fn id3v2_frame_text(frame: &Frame<'_>) -> String {
    let mut text = format!("{frame:?}");
    for name in ID3_ENCODING_NAMES {
        text = text.replace(&format!("encoding: {name}"), "encoding: any");
    }
    text
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
        // The form a tempo set in memory has before it is written.
        Some(AtomData::Unknown {
            code: DataType::BeSignedInteger,
            data,
        }) if data.len() == 2 => Ok(Some(u16::from_be_bytes([data[0], data[1]]).to_string())),
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
