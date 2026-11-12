// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! A list of Vorbis comments, the tag of FLAC and Ogg Vorbis files: a
//! vendor text and `NAME=value` entries, kept as the bytes they were read
//! from.

use crate::error::ErrorSource;

/// The vendor text and the comments, in file order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Comments {
    pub(super) vendor: Vec<u8>,
    pub(super) entries: Vec<Vec<u8>>,
}

fn little_endian_u32(bytes: &[u8], offset: usize) -> Option<usize> {
    let word: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_le_bytes(word) as usize)
}

impl Comments {
    /// Reads the vendor text and the comments at the start of `bytes`, and
    /// returns them with the number of bytes they take, or `None` when the
    /// lengths they declare do not fit.
    pub(super) fn parse(bytes: &[u8]) -> Option<(Self, usize)> {
        Self::parse_bounded(bytes, usize::MAX)
    }

    /// Like [`Comments::parse`], with an entry-count limit checked before
    /// materializing comments, including those with empty values.
    pub(super) fn parse_bounded(bytes: &[u8], max_entries: usize) -> Option<(Self, usize)> {
        let vendor_length = little_endian_u32(bytes, 0)?;
        let vendor = bytes.get(4..4usize.checked_add(vendor_length)?)?.to_vec();
        let mut offset = 4 + vendor_length;
        let count = little_endian_u32(bytes, offset)?;
        if count > max_entries {
            return None;
        }
        offset += 4;
        let mut entries = Vec::new();
        for _ in 0..count {
            let length = little_endian_u32(bytes, offset)?;
            let end = (offset + 4).checked_add(length)?;
            entries.push(bytes.get(offset + 4..end)?.to_vec());
            offset = end;
        }
        Some((Self { vendor, entries }, offset))
    }

    /// The vendor text and the comments in the form [`Comments::parse`]
    /// reads.
    pub(super) fn serialize(&self) -> Result<Vec<u8>, ErrorSource> {
        let length = |bytes: usize| {
            u32::try_from(bytes).map_err(|_| -> ErrorSource {
                "a Vorbis comment would be larger than the format allows".into()
            })
        };
        let mut bytes = length(self.vendor.len())?.to_le_bytes().to_vec();
        bytes.extend(&self.vendor);
        bytes.extend(length(self.entries.len())?.to_le_bytes());
        for entry in &self.entries {
            bytes.extend(length(entry.len())?.to_le_bytes());
            bytes.extend(entry);
        }
        Ok(bytes)
    }

    /// Whether there is neither a vendor text nor a comment.
    pub(super) fn is_empty(&self) -> bool {
        self.vendor.is_empty() && self.entries.is_empty()
    }

    /// The index of the first comment named `field`, in any letter case.
    pub(super) fn index(&self, field: &str) -> Option<usize> {
        self.entries.iter().position(|entry| {
            entry
                .iter()
                .position(|byte| *byte == b'=')
                .is_some_and(|equals| entry[..equals].eq_ignore_ascii_case(field.as_bytes()))
        })
    }

    /// The whole comment `field` names, as `NAME=value` bytes.
    pub(super) fn raw(&self, field: &str) -> Option<&[u8]> {
        self.index(field).map(|index| &*self.entries[index])
    }

    /// The text of `field`, or `None` when there is no such comment.
    pub(super) fn get(&self, field: &str) -> Option<String> {
        let entry = self.raw(field)?;
        Some(String::from_utf8_lossy(&entry[field.len() + 1..]).into_owned())
    }

    /// Sets `field` to `value`, or removes it when `value` is `None`. An
    /// existing comment keeps its place and the spelling of its name.
    pub(super) fn set(&mut self, field: &str, value: Option<&str>) {
        match (self.index(field), value) {
            (Some(index), Some(value)) => {
                self.entries[index].truncate(field.len() + 1);
                self.entries[index].extend(value.as_bytes());
            }
            (Some(index), None) => {
                self.entries.remove(index);
            }
            (None, Some(value)) => self.entries.push(format!("{field}={value}").into_bytes()),
            (None, None) => {}
        }
    }

    /// Puts back the exact bytes of the comment `field` names, or removes
    /// the comment when `raw` is `None`.
    pub(super) fn restore(&mut self, field: &str, raw: Option<&[u8]>) -> Result<(), ErrorSource> {
        if let Some(raw) = raw {
            let names_field = raw.get(field.len()) == Some(&b'=')
                && raw[..field.len()].eq_ignore_ascii_case(field.as_bytes());
            if !names_field {
                return Err(format!("the saved bytes are not a {field} comment").into());
            }
        }
        match (self.index(field), raw) {
            (Some(index), Some(raw)) => self.entries[index] = raw.to_vec(),
            (Some(index), None) => {
                self.entries.remove(index);
            }
            (None, Some(raw)) => self.entries.push(raw.to_vec()),
            (None, None) => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_reads_back_what_serialize_writes() {
        let comments = Comments {
            vendor: b"Lavf".to_vec(),
            entries: vec![b"TITLE=Low Tide".to_vec(), b"initialkey=8A".to_vec()],
        };
        let mut bytes = comments.serialize().unwrap();
        let length = bytes.len();
        bytes.push(1);
        assert_eq!(Comments::parse(&bytes), Some((comments.clone(), length)));
        assert_eq!(comments.get("INITIALKEY").as_deref(), Some("8A"));
    }

    #[test]
    fn lengths_that_run_past_the_end_are_rejected() {
        let mut bytes = 4_u32.to_le_bytes().to_vec();
        bytes.extend(b"Lavf");
        bytes.extend(1_u32.to_le_bytes());
        bytes.extend(u32::MAX.to_le_bytes());
        bytes.extend(b"A=1");
        assert_eq!(Comments::parse(&bytes), None);
        assert_eq!(Comments::parse(&[0xFF, 0xFF, 0xFF, 0xFF]), None);
    }
}
