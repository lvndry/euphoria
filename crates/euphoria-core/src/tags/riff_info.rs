// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `INFO` list of a WAV file, the tag that WAV defines for itself. Some
//! programs, rekordbox among them, read a WAV file's title and comment from
//! here and not from its ID3 chunk.
//!
//! The list is kept as its values, each with the bytes it was read from, so
//! that only the value a caller names is ever rewritten.

use std::fs::File;
use std::ops::Range;

use super::replace::Splice;
use crate::error::ErrorSource;

/// The comment value, under the name the field has in a write plan.
pub(super) const COMMENT: &str = "INFO ICMT";

const COMMENT_ID: &[u8; 4] = b"ICMT";
const LIST_ID: &[u8; 4] = b"LIST";
const LIST_TYPE: &[u8; 4] = b"INFO";
const CHUNK_HEADER_LENGTH: usize = 8;
const LIST_TYPE_LENGTH: usize = 4;
/// Longer lists are refused. Real lists hold a few short texts, and the
/// size field of a damaged file can claim gigabytes.
const LARGEST_LIST: u64 = 16 * 1024 * 1024;

fn little_endian_u32(bytes: &[u8], offset: usize) -> Option<usize> {
    let word: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(word) as usize)
}

/// The end of a whole value, allowing an omitted padding byte only for
/// the last value. Declared sizes can span all of u32 even on 32-bit hosts.
fn value_end(offset: usize, size: usize, body_length: usize) -> Option<usize> {
    let unpadded_end = offset.checked_add(CHUNK_HEADER_LENGTH)?.checked_add(size)?;
    let end = if unpadded_end == body_length {
        unpadded_end
    } else {
        unpadded_end.checked_add(size % 2)?
    };
    (end <= body_length).then_some(end)
}

/// The `INFO` list of a WAV file, or the place one would go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct InfoList {
    /// The `LIST` chunk with its header and padding byte, when the file has
    /// one.
    chunk: Option<Range<u64>>,
    /// Where a new list goes: in front of the `data` chunk, where ffmpeg
    /// and the RIFF specification's examples put it, so that a reader
    /// that stops at the audio still finds it.
    insert_at: u64,
    /// The values, each whole with its header and padding byte, in order.
    values: Vec<Vec<u8>>,
    /// Whether the values differ from those in the file.
    changed: bool,
    problem: Option<&'static str>,
}

impl InfoList {
    /// The list of a WAV file that has none, to be put at `insert_at`.
    pub(super) fn absent(insert_at: u64) -> Self {
        Self {
            chunk: None,
            insert_at,
            values: Vec::new(),
            changed: false,
            problem: None,
        }
    }

    /// Whether the content of a `LIST` chunk starts with the `INFO` type.
    pub(super) fn is_info(list_type: &[u8]) -> bool {
        list_type == LIST_TYPE
    }

    /// Reads the `LIST` chunk at `chunk`, whose content, after the chunk
    /// header, is `size` bytes long.
    pub(super) fn load(
        file: &mut File,
        chunk: Range<u64>,
        size: u64,
        insert_at: u64,
    ) -> Result<Self, ErrorSource> {
        let mut list = Self {
            chunk: Some(chunk.clone()),
            ..Self::absent(insert_at)
        };
        if size > LARGEST_LIST {
            list.problem = Some("its RIFF INFO list is larger than any real one");
            return Ok(list);
        }
        let content = super::native::read_exactly(file, chunk.start + 8, size)?;
        list.read_values(&content[LIST_TYPE_LENGTH..]);
        Ok(list)
    }

    fn read_values(&mut self, body: &[u8]) {
        let mut offset = 0;
        while offset < body.len() {
            let Some(size) = little_endian_u32(body, offset + 4) else {
                self.problem = Some("its RIFF INFO list holds data that is not a value");
                return;
            };
            let Some(end) = value_end(offset, size, body.len()) else {
                self.problem = Some("a value runs past the end of its RIFF INFO list");
                return;
            };
            let value = &body[offset..end];
            if value.starts_with(COMMENT_ID) && self.position().is_some() {
                self.problem = Some("it holds more than one RIFF INFO comment");
            }
            self.values.push(value.to_vec());
            offset = end;
        }
    }

    /// Why the list cannot be rewritten without loss, if it cannot.
    pub(super) fn problem(&self) -> Option<&'static str> {
        self.problem
    }

    /// The values, whole and in order.
    pub(super) fn values(&self) -> &[Vec<u8>] {
        &self.values
    }

    /// The index of the comment among [`InfoList::values`].
    pub(super) fn position(&self) -> Option<usize> {
        self.values
            .iter()
            .position(|value| value.starts_with(COMMENT_ID))
    }

    /// The bytes of the comment value, header included.
    pub(super) fn raw(&self) -> Option<&[u8]> {
        self.position().map(|index| &*self.values[index])
    }

    /// The text of a value without its header, ending at the first zero
    /// byte, as readers take it.
    fn text_bytes(value: &[u8]) -> &[u8] {
        let size = little_endian_u32(value, 4).unwrap_or(0);
        let text = &value[CHUNK_HEADER_LENGTH..(CHUNK_HEADER_LENGTH + size).min(value.len())];
        let end = text
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(text.len());
        &text[..end]
    }

    /// The comment, or `None` when the list has none. The format names no
    /// encoding: UTF-8 is tried first, and anything else is read as
    /// Latin-1, the usual alternative.
    pub(super) fn get(&self) -> Option<String> {
        let text = Self::text_bytes(self.raw()?);
        Some(match std::str::from_utf8(text) {
            Ok(text) => text.to_string(),
            Err(_) => text.iter().copied().map(char::from).collect(),
        })
    }

    /// Sets the comment to `value`, or removes it when `value` is `None`.
    /// An existing comment keeps its place.
    ///
    /// The text is stored in the encoding the comment has now: a comment
    /// that is not UTF-8 is read as Latin-1 and written back as Latin-1,
    /// so that the user's own text keeps its exact bytes after a label is
    /// put next to it.
    pub(super) fn set(&mut self, value: Option<&str>) -> Result<(), ErrorSource> {
        let position = self.position();
        let Some(value) = value else {
            if let Some(index) = position {
                self.values.remove(index);
                self.changed = true;
            }
            return Ok(());
        };
        let existing_is_latin1 = self
            .raw()
            .is_some_and(|raw| std::str::from_utf8(Self::text_bytes(raw)).is_err());
        let fits_latin1 = value.chars().all(|character| u32::from(character) <= 0xFF);
        let mut text: Vec<u8> = if existing_is_latin1 && fits_latin1 {
            value.chars().map(|character| character as u8).collect()
        } else {
            value.as_bytes().to_vec()
        };
        if text.contains(&0) {
            return Err("a RIFF INFO value cannot hold a zero byte".into());
        }
        text.push(0);
        let size = u32::try_from(text.len()).map_err(|_| "the comment is too long")?;
        let mut item = COMMENT_ID.to_vec();
        item.extend(size.to_le_bytes());
        item.extend(text);
        if !item.len().is_multiple_of(2) {
            item.push(0);
        }
        match position {
            Some(index) => self.values[index] = item,
            None => self.values.push(item),
        }
        self.changed = true;
        Ok(())
    }

    /// Puts back the exact bytes of the comment value, or removes it when
    /// `raw` is `None`.
    pub(super) fn restore(&mut self, raw: Option<&[u8]>) -> Result<(), ErrorSource> {
        if let Some(raw) = raw {
            let is_whole_value = raw.starts_with(COMMENT_ID)
                && little_endian_u32(raw, 4)
                    .is_some_and(|size| value_end(0, size, raw.len()) == Some(raw.len()));
            if !is_whole_value {
                return Err("the saved bytes are not a RIFF INFO comment".into());
            }
        }
        match (self.position(), raw) {
            (Some(index), Some(raw)) => self.values[index] = raw.to_vec(),
            (Some(index), None) => {
                self.values.remove(index);
            }
            (None, Some(raw)) => self.values.push(raw.to_vec()),
            (None, None) => return Ok(()),
        }
        self.changed = true;
        Ok(())
    }

    /// The change that writes the list back, or `None` when nothing in it
    /// changed. A list left without values is taken out of the file.
    pub(super) fn splice(&self) -> Result<Option<Splice>, ErrorSource> {
        if !self.changed {
            return Ok(None);
        }
        let mut bytes = Vec::new();
        if !self.values.is_empty() {
            let body: Vec<u8> = self.values.concat();
            let size = u32::try_from(LIST_TYPE_LENGTH + body.len())
                .map_err(|_| "the RIFF INFO list would be too large")?;
            bytes.extend(LIST_ID);
            bytes.extend(size.to_le_bytes());
            bytes.extend(LIST_TYPE);
            bytes.extend(body);
            if !bytes.len().is_multiple_of(2) {
                bytes.push(0);
            }
        }
        let place = self.chunk.clone().unwrap_or(self.insert_at..self.insert_at);
        Ok(Some(Splice {
            start: place.start,
            end: place.end,
            bytes,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(id: &[u8; 4], text: &[u8]) -> Vec<u8> {
        let mut item = id.to_vec();
        item.extend((text.len() as u32).to_le_bytes());
        item.extend(text);
        if !item.len().is_multiple_of(2) {
            item.push(0);
        }
        item
    }

    fn list(values: &[Vec<u8>]) -> InfoList {
        let mut list = InfoList::absent(0);
        list.chunk = Some(0..0);
        list.read_values(&values.concat());
        list
    }

    #[test]
    fn reads_the_comment_as_utf8_or_latin1() {
        let utf8 = list(&[
            value(b"INAM", b"Title\0"),
            value(b"ICMT", "Été\0".as_bytes()),
        ]);
        assert_eq!(utf8.problem(), None);
        assert_eq!(utf8.position(), Some(1));
        assert_eq!(utf8.get().as_deref(), Some("Été"));
        let latin1 = list(&[value(b"ICMT", b"\xC9t\xE9\0")]);
        assert_eq!(latin1.get().as_deref(), Some("Été"));
        let without_terminator = list(&[value(b"ICMT", b"plain")]);
        assert_eq!(without_terminator.get().as_deref(), Some("plain"));
        assert_eq!(list(&[value(b"INAM", b"x\0")]).get(), None);
    }

    #[test]
    fn latin1_comment_keeps_its_bytes_next_to_a_label() {
        let mut latin1 = list(&[value(b"INAM", b"Title\0"), value(b"ICMT", b"caf\xE9\0")]);
        latin1.set(Some("8A - Energy 7 - café")).unwrap();
        assert_eq!(latin1.values()[0], value(b"INAM", b"Title\0"));
        assert_eq!(
            latin1.values()[1],
            value(b"ICMT", b"8A - Energy 7 - caf\xE9\0")
        );
        assert_eq!(latin1.get().as_deref(), Some("8A - Energy 7 - café"));

        let mut utf8 = list(&[value(b"ICMT", "café\0".as_bytes())]);
        utf8.set(Some("8A - café")).unwrap();
        assert_eq!(utf8.values()[0], value(b"ICMT", "8A - café\0".as_bytes()));
    }

    #[test]
    fn values_are_padded_to_an_even_length() {
        let mut new = InfoList::absent(36);
        new.set(Some("8A")).unwrap();
        assert_eq!(new.values()[0], b"ICMT\x03\x00\x00\x008A\x00\x00");
        let splice = new.splice().unwrap().unwrap();
        assert_eq!((splice.start, splice.end), (36, 36));
        assert_eq!(
            splice.bytes,
            b"LIST\x10\x00\x00\x00INFOICMT\x03\x00\x00\x008A\x00\x00"
        );
    }

    #[test]
    fn unchanged_list_is_not_written_and_an_emptied_one_is_removed() {
        let mut existing = list(&[value(b"ICMT", b"x\0")]);
        existing.chunk = Some(100..122);
        assert_eq!(existing.splice().unwrap(), None);
        existing.set(None).unwrap();
        let splice = existing.splice().unwrap().unwrap();
        assert_eq!((splice.start, splice.end), (100, 122));
        assert!(splice.bytes.is_empty());
    }

    #[test]
    fn restore_puts_back_the_exact_bytes_and_refuses_others() {
        let original = value(b"ICMT", b"odd\0\0\0");
        let mut existing = list(&[value(b"INAM", b"t\0"), original.clone()]);
        existing.set(Some("8A")).unwrap();
        existing.restore(Some(&original)).unwrap();
        assert_eq!(existing.values()[1], original);
        assert!(
            existing
                .restore(Some(b"INAM\x02\x00\x00\x00t\x00"))
                .is_err()
        );
        assert!(
            existing
                .restore(Some(b"ICMT\x09\x00\x00\x00t\x00"))
                .is_err()
        );
    }

    #[test]
    fn damaged_list_is_a_problem() {
        assert!(
            list(&[b"ICMT\xFF\x00\x00\x00short".to_vec()])
                .problem()
                .is_some()
        );
        assert!(
            list(&[value(b"INAM", b"t\0"), b"IC".to_vec()])
                .problem()
                .is_some()
        );
        let unpadded_last = [value(b"INAM", b"t\0"), b"ICMT\x01\x00\x00\x00x".to_vec()];
        assert_eq!(list(&unpadded_last).problem(), None);
    }

    #[test]
    fn a_zero_byte_cannot_be_written() {
        let mut new = InfoList::absent(0);
        assert!(new.set(Some("a\0b")).is_err());
    }

    #[test]
    fn declared_value_lengths_cannot_overflow_the_host_range() {
        assert_eq!(value_end(0, usize::MAX, 16), None);
        assert_eq!(
            value_end(usize::MAX - CHUNK_HEADER_LENGTH, 1, usize::MAX),
            None
        );
        assert_eq!(value_end(0, usize::MAX - CHUNK_HEADER_LENGTH, 16), None);
        let malformed = b"ICMT\xff\xff\xff\xff".to_vec();
        assert!(list(std::slice::from_ref(&malformed)).problem().is_some());
        assert!(InfoList::absent(0).restore(Some(&malformed)).is_err());
    }

    #[test]
    fn duplicate_comments_are_read_but_cannot_be_rewritten() {
        let duplicate = list(&[value(b"ICMT", b"first\0"), value(b"ICMT", b"second\0")]);
        assert_eq!(duplicate.get().as_deref(), Some("first"));
        assert_eq!(duplicate.values().len(), 2);
        assert_eq!(
            duplicate.problem(),
            Some("it holds more than one RIFF INFO comment")
        );
    }
}
