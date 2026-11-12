// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! An ID3v2 tag as a list of frames kept as the bytes they were read from.
//! Only the frame a caller names is ever decoded or encoded; every other
//! frame goes back into the tag unchanged.

use super::base64;
use crate::error::ErrorSource;

/// The comment frame. Only the comment with an empty description is read
/// or written; described comments belong to other programs.
pub(super) const COMMENT: &str = "COMM";
/// The start of the field name of a general encapsulated object, followed
/// by the object's description, such as `GEOB:Serato Markers2`. Its value is
/// the object's data in base64.
pub(super) const OBJECT_PREFIX: &str = "GEOB:";
const OBJECT_FRAME: &str = "GEOB";
const OBJECT_MIME_TYPE: &[u8] = b"application/octet-stream";

pub(super) const HEADER_LENGTH: usize = 10;
const DEFAULT_VERSION: u8 = 3;
const DEFAULT_PADDING: usize = 1024;
const DEFAULT_LANGUAGE: &[u8; 3] = b"eng";
const MULTIPLE_VALUE_SEPARATOR: &str = "; ";

const FLAG_UNSYNCHRONISATION: u8 = 0x80;
const FLAG_EXTENDED_HEADER: u8 = 0x40;
const FLAG_FOOTER: u8 = 0x10;
const VERSION_3_FRAME_FORMAT_FLAGS: u8 = 0xE0;
const VERSION_4_FRAME_FORMAT_FLAGS: u8 = 0x4F;

const LATIN1: u8 = 0;
const UTF16_WITH_BOM: u8 = 1;
const UTF16_BIG_ENDIAN: u8 = 2;
const UTF8: u8 = 3;

/// Frame names in ID3v2.2, which has three letters where later versions
/// have four.
const VERSION_2_NAMES: [(&str, &str); 10] = [
    ("TKEY", "TKE"),
    ("TBPM", "TBP"),
    ("COMM", "COM"),
    ("TIT1", "TT1"),
    ("TIT2", "TT2"),
    ("TPE1", "TP1"),
    ("TALB", "TAL"),
    ("TCON", "TCO"),
    ("TYER", "TYE"),
    ("TRCK", "TRK"),
];

/// The length of the whole tag that starts with `header`, footer included,
/// or `None` when `header` does not start a tag.
///
/// The size in a tag header is syncsafe: each of its four bytes uses seven
/// bits. A header that sets the eighth bit does not say how long its tag
/// is, so it is an error rather than a guess.
pub(super) fn total_length(header: &[u8]) -> Result<Option<u64>, ErrorSource> {
    if header.len() < HEADER_LENGTH || &header[..3] != b"ID3" {
        return Ok(None);
    }
    let size = &header[6..10];
    if size.iter().any(|byte| byte & 0x80 != 0) {
        return Err("the size in the ID3v2 tag header is damaged".into());
    }
    let footer = if header[5] & FLAG_FOOTER != 0 {
        HEADER_LENGTH as u64
    } else {
        0
    };
    Ok(Some(
        HEADER_LENGTH as u64 + u64::from(syncsafe_to_number(size)) + footer,
    ))
}

fn syncsafe_to_number(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0, |number, byte| (number << 7) | u32::from(*byte & 0x7F))
}

fn number_to_syncsafe(number: u32) -> [u8; 4] {
    [
        (number >> 21) as u8 & 0x7F,
        (number >> 14) as u8 & 0x7F,
        (number >> 7) as u8 & 0x7F,
        number as u8 & 0x7F,
    ]
}

fn big_endian(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0, |number, byte| (number << 8) | u32::from(*byte))
}

fn is_frame_name(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

/// Removes the zero byte that unsynchronisation puts after every 0xFF.
fn remove_unsynchronisation(bytes: &[u8]) -> Vec<u8> {
    let mut plain = Vec::with_capacity(bytes.len());
    let mut after_ff = false;
    for byte in bytes {
        if after_ff && *byte == 0 {
            after_ff = false;
            continue;
        }
        plain.push(*byte);
        after_ff = *byte == 0xFF;
    }
    plain
}

/// Puts a zero byte after every 0xFF that would otherwise look like the
/// start of an MPEG frame, or that is followed by a zero byte.
fn add_unsynchronisation(bytes: &[u8]) -> Vec<u8> {
    let mut safe = Vec::with_capacity(bytes.len());
    for (index, byte) in bytes.iter().enumerate() {
        safe.push(*byte);
        let next = bytes.get(index + 1).copied();
        if *byte == 0xFF && next.is_none_or(|next| next == 0 || next >= 0xE0) {
            safe.push(0);
        }
    }
    safe
}

/// An ID3v2 tag. Frames are whole, header included, and in file order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Id3Tag {
    version: u8,
    revision: u8,
    flags: u8,
    frames: Vec<Vec<u8>>,
    problem: Option<&'static str>,
}

impl Id3Tag {
    /// The major version: 2, 3 or 4.
    pub(super) fn version(&self) -> u8 {
        self.version
    }

    /// An empty ID3v2.3 tag, the version the widest range of software
    /// reads.
    pub(super) fn new() -> Self {
        Self {
            version: DEFAULT_VERSION,
            revision: 0,
            flags: 0,
            frames: Vec::new(),
            problem: None,
        }
    }

    /// Reads a tag from all of its bytes, header first. Frames are read up
    /// to the first thing that is not a frame; what stopped the reading is
    /// kept as the tag's problem.
    pub(super) fn parse(bytes: &[u8]) -> Result<Self, ErrorSource> {
        let declared = total_length(bytes)?.ok_or("the ID3v2 tag has no header")?;
        if (bytes.len() as u64) < declared {
            return Err("the ID3v2 tag is cut short".into());
        }
        let mut tag = Self {
            version: bytes[3],
            revision: bytes[4],
            flags: bytes[5],
            frames: Vec::new(),
            problem: None,
        };
        if !(2..=4).contains(&tag.version) {
            return Err(format!("ID3v2.{} tags are not supported", tag.version).into());
        }
        let has_footer = tag.version == 4 && tag.flags & FLAG_FOOTER != 0;
        let body_end = declared as usize - if has_footer { HEADER_LENGTH } else { 0 };
        let stored_body = &bytes[HEADER_LENGTH..body_end];

        if tag.flags & FLAG_EXTENDED_HEADER != 0 {
            tag.problem = Some(if tag.version == 2 {
                "its ID3v2.2 tag is compressed"
            } else {
                "its ID3v2 tag has an extended header"
            });
            return Ok(tag);
        }
        let body = if tag.unsynchronises_whole_body() {
            remove_unsynchronisation(stored_body)
        } else {
            stored_body.to_vec()
        };
        tag.read_frames(&body);
        Ok(tag)
    }

    /// Before ID3v2.4, unsynchronisation covers the tag body as a whole. In
    /// ID3v2.4 each frame carries its own.
    fn unsynchronises_whole_body(&self) -> bool {
        self.version < 4 && self.flags & FLAG_UNSYNCHRONISATION != 0
    }

    fn frame_header_length(&self) -> usize {
        if self.version == 2 { 6 } else { 10 }
    }

    fn name_length(&self) -> usize {
        if self.version == 2 { 3 } else { 4 }
    }

    fn read_frames(&mut self, body: &[u8]) {
        let header_length = self.frame_header_length();
        let name_length = self.name_length();
        let mut offset = 0;
        while offset < body.len() {
            let rest = &body[offset..];
            if rest[0] == 0 {
                if rest.iter().any(|byte| *byte != 0) {
                    self.problem = Some("its ID3v2 tag holds data after the last frame");
                }
                return;
            }
            if rest.len() < header_length || !is_frame_name(&rest[..name_length]) {
                self.problem = Some("its ID3v2 tag holds data that is not a frame");
                return;
            }
            let Some(size) = self.frame_size(body, offset) else {
                self.problem = Some("a frame runs past the end of its ID3v2 tag");
                return;
            };
            let end = offset + header_length + size;
            self.frames.push(body[offset..end].to_vec());
            offset = end;
        }
    }

    /// The size of the content of the frame at `offset`, when the frame
    /// fits in the body.
    fn frame_size(&self, body: &[u8], offset: usize) -> Option<usize> {
        let header_length = self.frame_header_length();
        let fits = |size: usize| offset + header_length + size <= body.len();
        let size_bytes = if self.version == 2 {
            &body[offset + 3..offset + 6]
        } else {
            &body[offset + 4..offset + 8]
        };
        let plain = big_endian(size_bytes) as usize;
        if self.version < 4 {
            return fits(plain).then_some(plain);
        }
        // ID3v2.4 sizes are syncsafe, but some programs write plain ones.
        // The reading that lands on another frame, on padding, or on the
        // end of the tag is the right one.
        let syncsafe = syncsafe_to_number(size_bytes) as usize;
        let name_length = self.name_length();
        let lands_well = |size: usize| {
            let next = offset + header_length + size;
            next == body.len()
                || body.get(next) == Some(&0)
                || body
                    .get(next..next + name_length)
                    .is_some_and(is_frame_name)
        };
        let has_high_bit = size_bytes.iter().any(|byte| byte & 0x80 != 0);
        if !has_high_bit && fits(syncsafe) && (lands_well(syncsafe) || !lands_well(plain)) {
            return Some(syncsafe);
        }
        fits(plain).then_some(plain)
    }

    /// Why the tag cannot be rewritten without loss, if it cannot.
    pub(super) fn problem(&self) -> Option<&'static str> {
        self.problem
    }

    /// The frames, whole and in order.
    pub(super) fn frames(&self) -> &[Vec<u8>] {
        &self.frames
    }

    fn native_name<'a>(&self, field: &'a str) -> &'a str {
        if field.starts_with(OBJECT_PREFIX) {
            return OBJECT_FRAME;
        }
        if self.version != 2 {
            return field;
        }
        VERSION_2_NAMES
            .iter()
            .find(|(name, _)| *name == field)
            .map_or(field, |(_, short)| short)
    }

    fn content<'a>(&self, frame: &'a [u8]) -> &'a [u8] {
        &frame[self.frame_header_length()..]
    }

    /// True when the frame's content is stored as it is, without
    /// compression, encryption or other wrapping.
    fn is_stored_plainly(&self, frame: &[u8]) -> bool {
        match self.version {
            2 => true,
            3 => frame[9] & VERSION_3_FRAME_FORMAT_FLAGS == 0,
            _ => frame[9] & VERSION_4_FRAME_FORMAT_FLAGS == 0,
        }
    }

    /// The index of the frame `field` names.
    pub(super) fn position(&self, field: &str) -> Option<usize> {
        if let Some(description) = field.strip_prefix(OBJECT_PREFIX) {
            if self.version == 2 {
                return None;
            }
            return self.frames.iter().position(|frame| {
                frame.starts_with(OBJECT_FRAME.as_bytes())
                    && self.is_stored_plainly(frame)
                    && split_object(self.content(frame))
                        .is_some_and(|object| object.description == description)
            });
        }
        let name = self.native_name(field).as_bytes();
        self.frames.iter().position(|frame| {
            frame.starts_with(name)
                && (field != COMMENT
                    || split_comment(self.content(frame))
                        .is_some_and(|comment| comment.description_is_empty))
        })
    }

    /// The bytes of the frame `field` names.
    pub(super) fn raw(&self, field: &str) -> Option<&[u8]> {
        self.position(field).map(|index| &*self.frames[index])
    }

    /// The text of `field`, or `None` when the tag does not have it.
    pub(super) fn get(&self, field: &str) -> Result<Option<String>, ErrorSource> {
        let Some(frame) = self.raw(field) else {
            return Ok(None);
        };
        if !self.is_stored_plainly(frame) {
            return Err(format!("{field} is compressed, encrypted or unsynchronised").into());
        }
        let content = self.content(frame);
        if field.starts_with(OBJECT_PREFIX) {
            let object = split_object(content).ok_or("the object frame is damaged")?;
            return Ok(Some(base64::encode(&content[object.data_start..], true)));
        }
        let Some((encoding, text)) = content.split_first() else {
            return Ok(Some(String::new()));
        };
        let text = if field == COMMENT {
            let comment = split_comment(content).ok_or("the comment frame is damaged")?;
            &content[comment.text_start..]
        } else {
            text
        };
        let decoded = decode_text(*encoding, text);
        let mut values = decoded.split('\0').filter(|value| !value.is_empty());
        let first = values.next().unwrap_or_default().to_string();
        if self.version < 4 {
            return Ok(Some(first));
        }
        Ok(Some(values.fold(first, |joined, value| {
            joined + MULTIPLE_VALUE_SEPARATOR + value
        })))
    }

    /// Sets `field` to `value`, or removes it when `value` is `None`. An
    /// existing frame keeps its place, its flags and, for a comment, its
    /// language.
    pub(super) fn set(&mut self, field: &str, value: Option<&str>) -> Result<(), ErrorSource> {
        let position = self.position(field);
        let Some(value) = value else {
            if let Some(index) = position {
                self.frames.remove(index);
            }
            return Ok(());
        };
        let existing = position.map(|index| self.frames[index].clone());
        if existing
            .as_ref()
            .is_some_and(|frame| !self.is_stored_plainly(frame))
        {
            return Err(format!("{field} is compressed, encrypted or unsynchronised").into());
        }
        if let Some(description) = field.strip_prefix(OBJECT_PREFIX) {
            if self.version == 2 {
                return Err("an ID3v2.2 tag cannot hold this object".into());
            }
            let data = base64::decode(value.as_bytes())
                .ok_or_else(|| format!("the value of {field} is not base64"))?;
            let mut content = match existing.as_deref() {
                Some(frame) => {
                    let content = self.content(frame);
                    let object = split_object(content).ok_or("the object frame is damaged")?;
                    content[..object.data_start].to_vec()
                }
                None => {
                    if !description.is_ascii() {
                        return Err(format!("{description:?} is not an ASCII description").into());
                    }
                    let mut header = vec![LATIN1];
                    header.extend(OBJECT_MIME_TYPE);
                    header.push(0);
                    header.push(0);
                    header.extend(description.as_bytes());
                    header.push(0);
                    header
                }
            };
            content.extend(data);
            let frame = self.build_frame(field, existing.as_deref(), &content)?;
            match position {
                Some(index) => self.frames[index] = frame,
                None => self.frames.push(frame),
            }
            return Ok(());
        }
        let existing_content = existing.as_deref().map(|frame| self.content(frame));
        let existing_encoding = existing_content.and_then(|content| content.first().copied());
        let encoding = self.encoding_for(existing_encoding, value);

        let mut content = vec![encoding];
        if field == COMMENT {
            let comment = existing_content.and_then(split_comment);
            match (comment, existing_content) {
                (Some(comment), Some(existing_content)) if Some(encoding) == existing_encoding => {
                    content.extend(&existing_content[1..comment.text_start]);
                }
                (_, existing_content) => {
                    let language = existing_content
                        .and_then(|content| content.get(1..4))
                        .unwrap_or(DEFAULT_LANGUAGE);
                    content.extend(language);
                    content.extend(encode_text(encoding, ""));
                    content.extend(terminator(encoding));
                }
            }
        }
        content.extend(encode_text(encoding, value));

        let frame = self.build_frame(field, existing.as_deref(), &content)?;
        match position {
            Some(index) => self.frames[index] = frame,
            None => self.frames.push(frame),
        }
        Ok(())
    }

    /// Puts back the exact bytes of the frame `field` names, or removes the
    /// frame when `raw` is `None`.
    pub(super) fn restore(&mut self, field: &str, raw: Option<&[u8]>) -> Result<(), ErrorSource> {
        if let Some(raw) = raw {
            let name = self.native_name(field).as_bytes();
            let header_length = self.frame_header_length();
            let is_whole_frame = raw.starts_with(name)
                && raw.len() >= header_length
                && self.frame_size(raw, 0) == Some(raw.len() - header_length)
                && field.strip_prefix(OBJECT_PREFIX).is_none_or(|description| {
                    split_object(&raw[header_length..])
                        .is_some_and(|object| object.description == description)
                });
            if !is_whole_frame {
                return Err(format!("the saved bytes are not a {field} frame").into());
            }
        }
        match (self.position(field), raw) {
            (Some(index), Some(raw)) => self.frames[index] = raw.to_vec(),
            (Some(index), None) => {
                self.frames.remove(index);
            }
            (None, Some(raw)) => self.frames.push(raw.to_vec()),
            (None, None) => {}
        }
        Ok(())
    }

    /// The encoding for `text`: the one the frame has now when it can hold
    /// the text, and otherwise the simplest one the tag version offers.
    fn encoding_for(&self, existing: Option<u8>, text: &str) -> u8 {
        let fits_latin1 = text.chars().all(|character| u32::from(character) <= 0xFF);
        // ID3v2.4 frames are not unsynchronised when written here, so they
        // must not contain 0xFF, which Latin-1 and UTF-16 can.
        let unicode = if self.version == 4 {
            UTF8
        } else {
            UTF16_WITH_BOM
        };
        match existing {
            Some(LATIN1) | None if text.is_ascii() => LATIN1,
            Some(LATIN1) | None if fits_latin1 && self.version < 4 => LATIN1,
            Some(encoding @ (UTF16_WITH_BOM | UTF16_BIG_ENDIAN)) if self.version < 4 => encoding,
            Some(UTF8) if self.version == 4 => UTF8,
            _ => unicode,
        }
    }

    fn build_frame(
        &self,
        field: &str,
        existing: Option<&[u8]>,
        content: &[u8],
    ) -> Result<Vec<u8>, ErrorSource> {
        let too_large = || format!("{field} is too long for an ID3v2 frame");
        let size = u32::try_from(content.len()).map_err(|_| too_large())?;
        let mut frame = match existing {
            Some(existing) => existing[..self.name_length()].to_vec(),
            None => self.native_name(field).as_bytes().to_vec(),
        };
        match self.version {
            2 => {
                if size > 0x00FF_FFFF {
                    return Err(too_large().into());
                }
                frame.extend(&size.to_be_bytes()[1..]);
            }
            3 => frame.extend(size.to_be_bytes()),
            _ => {
                if size > 0x0FFF_FFFF {
                    return Err(too_large().into());
                }
                frame.extend(number_to_syncsafe(size));
            }
        }
        if self.version > 2 {
            frame.extend(existing.map_or(&[0, 0][..], |existing| &existing[8..10]));
        }
        frame.extend(content);
        Ok(frame)
    }

    /// The tag as bytes, padded to exactly the first of the `rooms` lengths
    /// it fits in, so it can be written where an old one was. When it fits
    /// in none, it gets fresh padding.
    pub(super) fn serialize(&self, rooms: &[usize]) -> Result<Vec<u8>, ErrorSource> {
        let frames: Vec<u8> = self.frames.concat();
        let body = if self.unsynchronises_whole_body() {
            add_unsynchronisation(&frames)
        } else {
            frames
        };
        let has_footer = self.version == 4 && self.flags & FLAG_FOOTER != 0;
        let padding = if has_footer {
            0
        } else {
            rooms
                .iter()
                .find_map(|room| room.checked_sub(HEADER_LENGTH + body.len()))
                .unwrap_or(DEFAULT_PADDING)
        };
        let size = u32::try_from(body.len() + padding)
            .ok()
            .filter(|size| *size <= 0x0FFF_FFFF)
            .ok_or("the ID3v2 tag would be larger than the format allows")?;

        let mut header = b"ID3".to_vec();
        header.extend([self.version, self.revision, self.flags]);
        header.extend(number_to_syncsafe(size));
        let mut tag = header.clone();
        tag.extend(body);
        tag.resize(tag.len() + padding, 0);
        if has_footer {
            tag.extend(b"3DI");
            tag.extend(&header[3..]);
        }
        Ok(tag)
    }
}

struct ObjectParts {
    description: String,
    data_start: usize,
}

/// Finds the description of a general encapsulated object and where its
/// data starts, after the encoding, the MIME type, the file name and the
/// description.
fn split_object(content: &[u8]) -> Option<ObjectParts> {
    let encoding = *content.first()?;
    let mime_end = 1 + content[1..].iter().position(|byte| *byte == 0)?;
    let terminator = terminator(encoding);
    let text_end = |start: usize| -> Option<usize> {
        let rest = content.get(start..)?;
        let step = terminator.len();
        (0..rest.len())
            .step_by(step)
            .find(|offset| rest[*offset..].starts_with(terminator))
            .map(|offset| start + offset)
    };
    let file_name_start = mime_end + 1;
    let file_name_end = text_end(file_name_start)?;
    let description_start = file_name_end + terminator.len();
    let description_end = text_end(description_start)?;
    Some(ObjectParts {
        description: decode_text(encoding, &content[description_start..description_end]),
        data_start: description_end + terminator.len(),
    })
}

struct CommentParts {
    description_is_empty: bool,
    text_start: usize,
}

/// Finds where the text of a comment frame starts, after the encoding, the
/// language and the terminated description.
fn split_comment(content: &[u8]) -> Option<CommentParts> {
    const DESCRIPTION_START: usize = 4;
    let encoding = *content.first()?;
    let description = content.get(DESCRIPTION_START..)?;
    let (length, terminator_length) = match encoding {
        UTF16_WITH_BOM | UTF16_BIG_ENDIAN => {
            let (units, _) = description.as_chunks::<2>();
            (
                units.iter().take_while(|unit| **unit != [0, 0]).count() * 2,
                2,
            )
        }
        _ => (description.iter().take_while(|byte| **byte != 0).count(), 1),
    };
    if length + terminator_length > description.len() {
        return None;
    }
    let description = &description[..length];
    let is_only_a_byte_order_mark =
        encoding == UTF16_WITH_BOM && matches!(description, [0xFF, 0xFE] | [0xFE, 0xFF]);
    Some(CommentParts {
        description_is_empty: description.is_empty() || is_only_a_byte_order_mark,
        text_start: DESCRIPTION_START + length + terminator_length,
    })
}

fn terminator(encoding: u8) -> &'static [u8] {
    match encoding {
        UTF16_WITH_BOM | UTF16_BIG_ENDIAN => &[0, 0],
        _ => &[0],
    }
}

fn decode_text(encoding: u8, bytes: &[u8]) -> String {
    let utf16 = |bytes: &[u8], big_endian: bool| {
        let (pairs, _) = bytes.as_chunks::<2>();
        let units: Vec<u16> = pairs
            .iter()
            .map(|pair| {
                if big_endian {
                    u16::from_be_bytes(*pair)
                } else {
                    u16::from_le_bytes(*pair)
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    };
    match encoding {
        UTF16_WITH_BOM => match bytes {
            [0xFE, 0xFF, rest @ ..] => utf16(rest, true),
            [0xFF, 0xFE, rest @ ..] => utf16(rest, false),
            _ => utf16(bytes, false),
        },
        UTF16_BIG_ENDIAN => utf16(bytes, true),
        UTF8 => String::from_utf8_lossy(bytes).into_owned(),
        _ => bytes.iter().map(|byte| char::from(*byte)).collect(),
    }
}

fn encode_text(encoding: u8, text: &str) -> Vec<u8> {
    match encoding {
        UTF16_WITH_BOM => [0xFF, 0xFE]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect(),
        UTF16_BIG_ENDIAN => text.encode_utf16().flat_map(u16::to_be_bytes).collect(),
        UTF8 => text.as_bytes().to_vec(),
        // The caller picks Latin-1 only for text that fits in it.
        _ => text.chars().map(|character| character as u8).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(name: &str, content: &[u8]) -> Vec<u8> {
        let mut frame = name.as_bytes().to_vec();
        frame.extend((content.len() as u32).to_be_bytes());
        frame.extend([0, 0]);
        frame.extend(content);
        frame
    }

    fn tag_bytes(version: u8, flags: u8, body: &[u8]) -> Vec<u8> {
        let mut tag = b"ID3".to_vec();
        tag.extend([version, 0, flags]);
        tag.extend(number_to_syncsafe(body.len() as u32));
        tag.extend(body);
        tag
    }

    fn version_3(frames: &[Vec<u8>], padding: usize) -> Vec<u8> {
        let mut body = frames.concat();
        body.resize(body.len() + padding, 0);
        tag_bytes(3, 0, &body)
    }

    #[test]
    fn untouched_frames_come_back_byte_for_byte() {
        let frames = [
            frame("TIT2", b"\x00Title\x00"),
            frame("TPE1", b""),
            frame("TCON", b"\x01\xFE\xFF\x00H\x00o\x00u\x00s\x00e"),
            frame("PRIV", b"owner\x00\xFF\xE0\x00binary"),
            frame("COMM", b"\x00engiTunNORM\x00 0000"),
            frame("TKEY", b"\x00Cm"),
        ];
        let original = version_3(&frames, 40);
        let mut tag = Id3Tag::parse(&original).unwrap();
        assert_eq!(tag.problem(), None);
        assert_eq!(tag.frames(), frames);
        assert_eq!(tag.serialize(&[original.len()]).unwrap(), original);

        tag.set("TKEY", Some("8A")).unwrap();
        tag.set("TBPM", Some("128")).unwrap();
        let rewritten = Id3Tag::parse(&tag.serialize(&[original.len()]).unwrap()).unwrap();
        assert_eq!(rewritten.frames()[..5], frames[..5]);
        assert_eq!(rewritten.frames()[5], frame("TKEY", b"\x008A"));
        assert_eq!(rewritten.frames()[6], frame("TBPM", b"\x00128"));

        tag.restore("TKEY", Some(&frames[5])).unwrap();
        tag.restore("TBPM", None).unwrap();
        assert_eq!(tag.serialize(&[original.len()]).unwrap(), original);
    }

    #[test]
    fn reads_every_text_encoding() {
        let frames = [
            frame("TIT2", b"\x00Caf\xE9\x00"),
            frame("TPE1", b"\x01\xFF\xFEA\x00\xE9\x00"),
            frame("TIT1", b"\x02\x00A\x00\xE9"),
            frame("TKEY", b"\x03A\xC3\xA9"),
        ];
        let tag = Id3Tag::parse(&version_3(&frames, 0)).unwrap();
        assert_eq!(tag.get("TIT2").unwrap().as_deref(), Some("Café"));
        assert_eq!(tag.get("TPE1").unwrap().as_deref(), Some("Aé"));
        assert_eq!(tag.get("TIT1").unwrap().as_deref(), Some("Aé"));
        assert_eq!(tag.get("TKEY").unwrap().as_deref(), Some("Aé"));
        assert_eq!(tag.get("TBPM").unwrap(), None);
    }

    #[test]
    fn comment_is_the_one_without_a_description() {
        let frames = [
            frame("COMM", b"\x00engiTunNORM\x00 0000"),
            frame("COMM", b"\x01fra\xFF\xFE\x00\x00\xFF\xFEo\x00k\x00"),
        ];
        let mut tag = Id3Tag::parse(&version_3(&frames, 0)).unwrap();
        assert_eq!(tag.position(COMMENT), Some(1));
        assert_eq!(tag.get(COMMENT).unwrap().as_deref(), Some("ok"));

        tag.set(COMMENT, Some("{8A} ok")).unwrap();
        assert_eq!(tag.frames()[0], frames[0]);
        let mut expected = b"\x01fra\xFF\xFE\x00\x00\xFF\xFE".to_vec();
        expected.extend("{8A} ok".encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(tag.frames()[1], frame("COMM", &expected));
    }

    #[test]
    fn new_text_uses_the_simplest_encoding_the_version_allows() {
        let mut version_3_tag = Id3Tag::new();
        version_3_tag.set("TKEY", Some("8A")).unwrap();
        version_3_tag.set(COMMENT, Some("été")).unwrap();
        version_3_tag.set("TIT1", Some("日本")).unwrap();
        assert_eq!(version_3_tag.frames()[0], frame("TKEY", b"\x008A"));
        assert_eq!(
            version_3_tag.frames()[1],
            frame("COMM", b"\x00eng\x00\xE9t\xE9")
        );
        assert_eq!(version_3_tag.frames()[2][10..13], [1, 0xFF, 0xFE]);
        assert_eq!(version_3_tag.get("TIT1").unwrap().as_deref(), Some("日本"));

        let mut version_4_tag = Id3Tag::parse(&tag_bytes(4, 0, &[])).unwrap();
        version_4_tag.set(COMMENT, Some("été")).unwrap();
        let mut expected = b"COMM".to_vec();
        expected.extend(number_to_syncsafe(10));
        expected.extend(b"\x00\x00\x03eng\x00\xC3\xA9t\xC3\xA9");
        assert_eq!(version_4_tag.frames()[0], expected);
    }

    #[test]
    fn latin1_frame_that_gets_wider_text_becomes_utf16() {
        let frames = [frame("COMM", b"\x00fra\x00plain")];
        let mut tag = Id3Tag::parse(&version_3(&frames, 0)).unwrap();
        tag.set(COMMENT, Some("日本 plain")).unwrap();
        assert_eq!(tag.frames()[0][10..18], *b"\x01fra\xFF\xFE\x00\x00");
        assert_eq!(tag.get(COMMENT).unwrap().as_deref(), Some("日本 plain"));
    }

    #[test]
    fn version_2_frames_have_three_letter_names() {
        let mut body = b"TT2\x00\x00\x06\x00Title".to_vec();
        body.extend(b"TKE\x00\x00\x03\x00Cm");
        let mut tag = Id3Tag::parse(&tag_bytes(2, 0, &body)).unwrap();
        assert_eq!(tag.get("TIT2").unwrap().as_deref(), Some("Title"));
        assert_eq!(tag.get("TKEY").unwrap().as_deref(), Some("Cm"));
        tag.set("TKEY", Some("8A")).unwrap();
        tag.set("TBPM", Some("128")).unwrap();
        assert_eq!(tag.frames()[1], b"TKE\x00\x00\x03\x008A");
        assert_eq!(tag.frames()[2], b"TBP\x00\x00\x04\x00128");
    }

    #[test]
    fn whole_tag_unsynchronisation_is_undone_and_redone() {
        let plain = frame("PRIV", b"x\x00\xFF\xE0\xFF\x00\xFF");
        let stored = add_unsynchronisation(&plain);
        assert_eq!(stored.len(), plain.len() + 3);
        let original = tag_bytes(3, FLAG_UNSYNCHRONISATION, &stored);
        let tag = Id3Tag::parse(&original).unwrap();
        assert_eq!(tag.frames(), [plain]);
        assert_eq!(tag.serialize(&[original.len()]).unwrap(), original);
    }

    #[test]
    fn version_4_sizes_are_read_syncsafe_or_plain() {
        let content = [b'\x00'; 200];
        let mut syncsafe = b"TIT2".to_vec();
        syncsafe.extend(number_to_syncsafe(200));
        syncsafe.extend([0, 0]);
        syncsafe.extend(content);
        let plain = frame("TIT2", &content);
        let next = frame("TKEY", b"\x00Cm");

        for first in [syncsafe, plain] {
            let body = [first.clone(), next.clone()].concat();
            let tag = Id3Tag::parse(&tag_bytes(4, 0, &body)).unwrap();
            assert_eq!(tag.problem(), None);
            assert_eq!(tag.frames(), [first, next.clone()]);
        }
    }

    #[test]
    fn footer_is_kept_and_such_a_tag_has_no_padding() {
        let mut original = tag_bytes(4, FLAG_FOOTER, b"TKEY\x00\x00\x00\x03\x00\x00\x00Cm");
        original.extend(b"3DI\x04\x00\x10\x00\x00\x00\x0D");
        assert_eq!(
            total_length(&original).unwrap(),
            Some(original.len() as u64)
        );
        let mut tag = Id3Tag::parse(&original).unwrap();
        assert_eq!(tag.serialize(&[original.len()]).unwrap(), original);
        tag.set("TKEY", Some("10A")).unwrap();
        let grown = tag.serialize(&[original.len()]).unwrap();
        assert_eq!(grown.len(), original.len() + 1);
        assert!(grown.ends_with(b"3DI\x04\x00\x10\x00\x00\x00\x0E"));
    }

    #[test]
    fn header_size_with_the_eighth_bit_set_is_refused() {
        assert_eq!(
            total_length(b"ID3\x03\x00\x00\x00\x00\x02\x01").unwrap(),
            Some(267)
        );
        assert_eq!(total_length(b"RIFF\x00\x00\x00\x80\x00\x00").unwrap(), None);
        for position in 6..10 {
            let mut header = *b"ID3\x03\x00\x00\x00\x00\x00\x0A";
            header[position] |= 0x80;
            assert!(total_length(&header).is_err(), "byte {position}");
            let mut tag = header.to_vec();
            tag.resize(300, 0);
            assert!(Id3Tag::parse(&tag).is_err(), "byte {position}");
        }
    }

    #[test]
    fn data_that_is_not_a_frame_is_a_problem() {
        let after_padding = [frame("TKEY", b"\x00Cm"), vec![0, 0, 0, 7]].concat();
        let not_a_frame = [frame("TKEY", b"\x00Cm"), b"junk and more".to_vec()].concat();
        let too_long = b"TKEY\x00\x00\x01\x00\x00\x00\x00Cm".to_vec();
        for body in [after_padding, not_a_frame, too_long] {
            let tag = Id3Tag::parse(&tag_bytes(3, 0, &body)).unwrap();
            assert!(tag.problem().is_some(), "{body:?}");
        }
        let extended = Id3Tag::parse(&tag_bytes(3, FLAG_EXTENDED_HEADER, &[0; 10])).unwrap();
        assert!(extended.problem().is_some());
    }

    #[test]
    fn compressed_frame_is_kept_but_not_decoded() {
        let mut compressed = frame("TKEY", b"\x00\x00\x00\x02xx");
        compressed[9] = 0x80;
        let mut tag = Id3Tag::parse(&version_3(&[compressed.clone()], 0)).unwrap();
        assert!(tag.get("TKEY").is_err());
        assert!(tag.set("TKEY", Some("8A")).is_err());
        assert_eq!(tag.frames(), [compressed]);
    }

    #[test]
    fn restore_refuses_bytes_that_are_not_the_frame() {
        let mut tag = Id3Tag::new();
        assert!(
            tag.restore("TKEY", Some(b"TBPM\x00\x00\x00\x01\x00\x00\x00"))
                .is_err()
        );
        assert!(
            tag.restore("TKEY", Some(b"TKEY\x00\x00\x00\x09\x00\x00\x00"))
                .is_err()
        );
        assert!(tag.frames().is_empty());
    }
}
