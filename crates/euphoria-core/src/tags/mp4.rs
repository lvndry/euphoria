// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The iTunes style metadata of an MP4 file: the items of the `ilst` atom
//! in `moov.udta.meta`, kept as the bytes they were read from. Only the
//! item a caller names is ever rewritten; every other item, and every
//! other atom, goes back unchanged.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::ops::Range;

use super::replace::{Splice, same_bytes};
use crate::error::ErrorSource;

/// The tempo item, which holds a whole number.
pub(super) const TEMPO: &str = "tmpo";

/// The track number item, which holds numbers rather than text: the track
/// and how many tracks there are.
pub(super) const TRACK_NUMBER: &str = "trkn";

const ATOM_HEADER_LENGTH: usize = 8;
const FULL_BOX_HEADER_LENGTH: usize = 4;
const DATA_HEADER_LENGTH: usize = 16;
const FREEFORM_PREFIX: &str = "----:";
const COPYRIGHT_SIGN_BYTE: u8 = 0xA9;
const DATA_TYPE_UTF8: u32 = 1;
const DATA_TYPE_UTF16: u32 = 2;
const DATA_TYPE_SIGNED_INTEGER: u32 = 21;
const DATA_TYPE_UNSIGNED_INTEGER: u32 = 22;
const DATA_TYPE_IMPLICIT: u32 = 0;
const CHUNK_OFFSET_SAMPLES: usize = 512;
/// The handler atom that marks a `meta` atom as iTunes style metadata.
const METADATA_HANDLER: &[u8] = b"\x00\x00\x00\x21hdlr\x00\x00\x00\x00\x00\x00\x00\x00mdirappl\
\x00\x00\x00\x00\x00\x00\x00\x00\x00";

type Kind = [u8; 4];

#[derive(Debug, Clone, PartialEq, Eq)]
struct AtomRange {
    kind: Kind,
    range: Range<usize>,
}

fn big_endian_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let word: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
    Some(u32::from_be_bytes(word))
}

/// The atoms in `bytes[start..end]`, up to the first thing that is not an
/// atom, and the offset where they stop.
fn atoms_until_stop(bytes: &[u8], start: usize, end: usize) -> (Vec<AtomRange>, usize) {
    let mut atoms = Vec::new();
    let mut offset = start;
    while let Some(size) = big_endian_u32(bytes, offset) {
        let size = size as usize;
        if size < ATOM_HEADER_LENGTH || offset + size > end {
            break;
        }
        let kind = [
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ];
        atoms.push(AtomRange {
            kind,
            range: offset..offset + size,
        });
        offset += size;
    }
    (atoms, offset)
}

/// The atoms that fill `bytes[start..end]` exactly, or `None` when
/// something else is in there too.
fn atoms(bytes: &[u8], start: usize, end: usize) -> Option<Vec<AtomRange>> {
    let (atoms, stop) = atoms_until_stop(bytes, start, end);
    (stop == end).then_some(atoms)
}

fn first_of_kind<'a>(atoms: &'a [AtomRange], kind: &Kind) -> Option<&'a AtomRange> {
    atoms.iter().find(|atom| &atom.kind == kind)
}

fn atom(kind: &Kind, content: &[u8]) -> Result<Vec<u8>, ErrorSource> {
    let size = u32::try_from(ATOM_HEADER_LENGTH + content.len())
        .map_err(|_| "an atom would be larger than the format allows")?;
    let mut bytes = size.to_be_bytes().to_vec();
    bytes.extend(kind);
    bytes.extend(content);
    Ok(bytes)
}

/// `parent` with the bytes in `range` exchanged for `replacement` and its
/// size updated.
fn with_replaced(
    parent: &[u8],
    range: Range<usize>,
    replacement: &[u8],
) -> Result<Vec<u8>, ErrorSource> {
    let mut content = parent[ATOM_HEADER_LENGTH..range.start].to_vec();
    content.extend(replacement);
    content.extend(&parent[range.end..]);
    let kind: Kind = [parent[4], parent[5], parent[6], parent[7]];
    atom(&kind, &content)
}

/// The name of an item: four characters, or a reverse domain name and a
/// name for the `----` items programs define for themselves.
#[derive(Debug, PartialEq, Eq)]
enum ItemName {
    Fourcc(Kind),
    Freeform { mean: String, name: String },
}

impl ItemName {
    fn parse(field: &str) -> Result<Self, ErrorSource> {
        if let Some(freeform) = field.strip_prefix(FREEFORM_PREFIX) {
            let (mean, name) = freeform
                .split_once(':')
                .ok_or_else(|| format!("{field} is not a freeform item name"))?;
            return Ok(Self::Freeform {
                mean: mean.to_string(),
                name: name.to_string(),
            });
        }
        let bytes: Vec<u8> = field
            .chars()
            .map(|character| match character {
                '©' => Some(COPYRIGHT_SIGN_BYTE),
                other => u8::try_from(other).ok().filter(u8::is_ascii),
            })
            .collect::<Option<_>>()
            .ok_or_else(|| format!("{field} is not an item name"))?;
        let fourcc: Kind = bytes
            .try_into()
            .map_err(|_| format!("{field} is not a four character item name"))?;
        Ok(Self::Fourcc(fourcc))
    }

    /// Whether `item`, a whole child of `ilst`, has this name. Freeform
    /// names differ in letter case between programs, so they match in any
    /// case.
    fn names(&self, item: &[u8]) -> bool {
        match self {
            Self::Fourcc(fourcc) => item.get(4..8) == Some(fourcc),
            Self::Freeform { mean, name } => {
                let Some(parts) = atoms(item, ATOM_HEADER_LENGTH, item.len()) else {
                    return false;
                };
                let has = |kind: &Kind, wanted: &str| {
                    first_of_kind(&parts, kind).is_some_and(|part| {
                        item[part.range.clone()]
                            .get(ATOM_HEADER_LENGTH + FULL_BOX_HEADER_LENGTH..)
                            .is_some_and(|text| text.eq_ignore_ascii_case(wanted.as_bytes()))
                    })
                };
                item.get(4..8) == Some(b"----") && has(b"mean", mean) && has(b"name", name)
            }
        }
    }
}

/// The payload of a track number item for "7" or "7/12".
fn track_number_payload(value: &str) -> Result<Vec<u8>, ErrorSource> {
    let refuse = || format!("{value:?} is not a track number such as 7 or 7/12");
    let (track, total) = match value.trim().split_once('/') {
        Some((track, total)) => (track.trim(), total.trim()),
        None => (value.trim(), "0"),
    };
    let track: u16 = track.parse().map_err(|_| refuse())?;
    let total: u16 = total.parse().map_err(|_| refuse())?;
    let mut payload = vec![0, 0];
    payload.extend(track.to_be_bytes());
    payload.extend(total.to_be_bytes());
    payload.extend([0, 0]);
    Ok(payload)
}

fn data_atom(field: &str, value: &str) -> Result<Vec<u8>, ErrorSource> {
    let (data_type, payload) = if field == TRACK_NUMBER {
        (DATA_TYPE_IMPLICIT, track_number_payload(value)?)
    } else if field == TEMPO {
        let tempo: u16 = value
            .parse()
            .map_err(|_| format!("{value:?} is not a whole tempo that fits in {TEMPO}"))?;
        (DATA_TYPE_SIGNED_INTEGER, tempo.to_be_bytes().to_vec())
    } else {
        (DATA_TYPE_UTF8, value.as_bytes().to_vec())
    };
    let mut content = data_type.to_be_bytes().to_vec();
    content.extend([0; 4]);
    content.extend(payload);
    atom(b"data", &content)
}

fn text_atom(kind: &Kind, text: &str) -> Result<Vec<u8>, ErrorSource> {
    let mut content = vec![0; FULL_BOX_HEADER_LENGTH];
    content.extend(text.as_bytes());
    atom(kind, &content)
}

/// Where the metadata atoms sit inside `moov`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Layout {
    udta: Option<Range<usize>>,
    /// Where the atoms of `udta` stop. Some files end `udta` with zeros.
    udta_atoms_end: usize,
    meta: Option<Range<usize>>,
    ilst: Option<Range<usize>>,
    /// A `free` atom after `ilst` in `meta`, which is padding.
    padding: Option<Range<usize>>,
}

/// The iTunes style metadata of an MP4 file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Mp4File {
    file_length: u64,
    moov_start: u64,
    moov: Vec<u8>,
    /// Lengths `moov` and the file should get back when padding allows,
    /// set by an undo.
    preferred_moov_length: Option<u64>,
    preferred_file_length: Option<u64>,
    /// The size of a top level `free` atom right after `moov`.
    padding_after_moov: Option<u32>,
    is_fragmented: bool,
    layout: Layout,
    items: Vec<Vec<u8>>,
    problem: Option<&'static str>,
}

impl Mp4File {
    /// Reads the `moov` atom of `file` and finds the metadata in it.
    pub(super) fn load(file: &mut File) -> Result<Self, ErrorSource> {
        let length = file.metadata()?.len();
        let mut found: Option<Self> = None;
        let mut previous_was_moov = false;
        let mut is_fragmented = false;
        let mut offset = 0;
        while offset + ATOM_HEADER_LENGTH as u64 <= length {
            file.seek(SeekFrom::Start(offset))?;
            let mut header = [0; 16];
            let available = (length - offset).min(16) as usize;
            file.read_exact(&mut header[..available])?;
            let size32 = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
            let kind: Kind = [header[4], header[5], header[6], header[7]];
            let size = match size32 {
                0 => length - offset,
                1 => u64::from_be_bytes(header[8..].try_into().unwrap_or_default()),
                size => u64::from(size),
            };
            if size < ATOM_HEADER_LENGTH as u64 || offset + size > length {
                break;
            }
            match &kind {
                b"moov" => {
                    if found.is_some() {
                        return Err("the file has more than one moov atom".into());
                    }
                    if size32 < 2 {
                        return Err("the moov atom has no plain size".into());
                    }
                    let mut moov = Vec::new();
                    file.seek(SeekFrom::Start(offset))?;
                    file.take(size).read_to_end(&mut moov)?;
                    found = Some(Self::from_moov(length, offset, moov)?);
                }
                b"free" if previous_was_moov && size32 >= 2 => {
                    if let Some(found) = found.as_mut() {
                        found.padding_after_moov = Some(size32);
                    }
                }
                b"moof" | b"sidx" | b"mfra" => is_fragmented = true,
                _ => {}
            }
            previous_was_moov = &kind == b"moov";
            offset += size;
        }
        let mut found = found.ok_or("the file has no moov atom")?;
        found.is_fragmented = is_fragmented;
        Ok(found)
    }

    fn from_moov(file_length: u64, moov_start: u64, moov: Vec<u8>) -> Result<Self, ErrorSource> {
        if moov.len() < ATOM_HEADER_LENGTH {
            return Err("the moov atom is cut short".into());
        }
        let mut file = Self {
            file_length,
            moov_start,
            moov,
            preferred_moov_length: None,
            preferred_file_length: None,
            padding_after_moov: None,
            is_fragmented: false,
            layout: Layout::default(),
            items: Vec::new(),
            problem: None,
        };
        if let Err(problem) = file.find_metadata() {
            file.problem = Some(problem);
        }
        Ok(file)
    }

    fn find_metadata(&mut self) -> Result<(), &'static str> {
        let moov = &self.moov;
        let top = atoms(moov, ATOM_HEADER_LENGTH, moov.len())
            .ok_or("its moov atom holds data that is not an atom")?;
        if top.iter().filter(|atom| &atom.kind == b"udta").count() > 1 {
            return Err("its moov atom holds more than one udta atom");
        }
        let Some(udta) = first_of_kind(&top, b"udta") else {
            return Ok(());
        };
        self.layout.udta = Some(udta.range.clone());
        let (in_udta, atoms_end) =
            atoms_until_stop(moov, udta.range.start + ATOM_HEADER_LENGTH, udta.range.end);
        self.layout.udta_atoms_end = atoms_end;
        if moov[atoms_end..udta.range.end]
            .iter()
            .any(|byte| *byte != 0)
        {
            return Err("its udta atom holds data that is not an atom");
        }
        if in_udta.iter().filter(|atom| &atom.kind == b"meta").count() > 1 {
            return Err("its udta atom holds more than one meta atom");
        }
        let Some(meta) = first_of_kind(&in_udta, b"meta") else {
            return Ok(());
        };
        self.layout.meta = Some(meta.range.clone());
        let content_start = meta.range.start + ATOM_HEADER_LENGTH + FULL_BOX_HEADER_LENGTH;
        let in_meta = atoms(moov, content_start, meta.range.end)
            .ok_or("its meta atom holds data that is not an atom")?;
        if in_meta.iter().filter(|atom| &atom.kind == b"ilst").count() > 1 {
            return Err("its meta atom holds more than one ilst atom");
        }
        let Some(ilst_index) = in_meta.iter().position(|atom| &atom.kind == b"ilst") else {
            return Ok(());
        };
        let ilst = &in_meta[ilst_index];
        self.layout.ilst = Some(ilst.range.clone());
        self.layout.padding =
            first_of_kind(&in_meta[ilst_index..], b"free").map(|free| free.range.clone());
        let items = atoms(moov, ilst.range.start + ATOM_HEADER_LENGTH, ilst.range.end)
            .ok_or("its ilst atom holds data that is not an item")?;
        self.items = items
            .into_iter()
            .map(|item| moov[item.range].to_vec())
            .collect();
        Ok(())
    }

    /// The length of the `moov` atom.
    pub(super) fn moov_length(&self) -> u64 {
        self.moov.len() as u64
    }

    /// Asks for `moov` and the file to get these lengths when padding
    /// allows.
    pub(super) fn prefer_lengths(&mut self, moov: Option<u64>, file: Option<u64>) {
        self.preferred_moov_length = moov;
        self.preferred_file_length = file;
    }

    /// Why the metadata cannot be rewritten without loss, if it cannot.
    pub(super) fn problem(&self) -> Option<&'static str> {
        self.problem
    }

    /// The items of `ilst`, whole and in order.
    pub(super) fn items(&self) -> &[Vec<u8>] {
        &self.items
    }

    /// The index of the item `field` names.
    pub(super) fn position(&self, field: &str) -> Result<Option<usize>, ErrorSource> {
        let name = ItemName::parse(field)?;
        Ok(self.items.iter().position(|item| name.names(item)))
    }

    /// The bytes of the item `field` names.
    pub(super) fn raw(&self, field: &str) -> Result<Option<&[u8]>, ErrorSource> {
        Ok(self.position(field)?.map(|index| &*self.items[index]))
    }

    /// The text of `field`, or `None` when there is no such item.
    pub(super) fn get(&self, field: &str) -> Result<Option<String>, ErrorSource> {
        let Some(item) = self.raw(field)? else {
            return Ok(None);
        };
        let unreadable = || format!("{field} holds data that is not text or a number");
        let parts = atoms(item, ATOM_HEADER_LENGTH, item.len()).ok_or_else(unreadable)?;
        let Some(data) = first_of_kind(&parts, b"data") else {
            return Ok(Some(String::new()));
        };
        let data = &item[data.range.clone()];
        let data_type =
            big_endian_u32(data, ATOM_HEADER_LENGTH).ok_or_else(unreadable)? & 0x00FF_FFFF;
        let payload = data.get(DATA_HEADER_LENGTH..).ok_or_else(unreadable)?;
        if field == TRACK_NUMBER {
            let number = |offset: usize| {
                payload
                    .get(offset..offset + 2)
                    .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
            };
            let track = number(2).ok_or_else(unreadable)?;
            return Ok(Some(match number(4) {
                Some(total) if total > 0 => format!("{track}/{total}"),
                _ => track.to_string(),
            }));
        }
        match data_type {
            DATA_TYPE_UTF8 => Ok(Some(String::from_utf8_lossy(payload).into_owned())),
            DATA_TYPE_UTF16 => {
                let (pairs, _) = payload.as_chunks::<2>();
                let units: Vec<u16> = pairs.iter().map(|pair| u16::from_be_bytes(*pair)).collect();
                Ok(Some(String::from_utf16_lossy(&units)))
            }
            DATA_TYPE_SIGNED_INTEGER | DATA_TYPE_UNSIGNED_INTEGER | DATA_TYPE_IMPLICIT
                if (1..=8).contains(&payload.len()) =>
            {
                let is_negative = data_type == DATA_TYPE_SIGNED_INTEGER && payload[0] & 0x80 != 0;
                let fill = if is_negative { 0xFF } else { 0 };
                let mut wide = [fill; 8];
                wide[8 - payload.len()..].copy_from_slice(payload);
                Ok(Some(i64::from_be_bytes(wide).to_string()))
            }
            _ => Err(unreadable().into()),
        }
    }

    /// Sets `field` to `value`, or removes it when `value` is `None`. An
    /// existing item keeps its place and, for a freeform item, the spelling
    /// of its name.
    pub(super) fn set(&mut self, field: &str, value: Option<&str>) -> Result<(), ErrorSource> {
        let name = ItemName::parse(field)?;
        let position = self.position(field)?;
        let Some(value) = value else {
            if let Some(index) = position {
                self.items.remove(index);
            }
            return Ok(());
        };
        let data = data_atom(field, value)?;
        let item = match (&name, position) {
            (ItemName::Fourcc(fourcc), _) => atom(fourcc, &data)?,
            (ItemName::Freeform { .. }, Some(index)) => {
                let existing = &self.items[index];
                let parts = atoms(existing, ATOM_HEADER_LENGTH, existing.len())
                    .ok_or_else(|| format!("{field} is damaged"))?;
                let mut content: Vec<u8> = parts
                    .iter()
                    .filter(|part| &part.kind != b"data")
                    .flat_map(|part| existing[part.range.clone()].iter().copied())
                    .collect();
                content.extend(data);
                atom(b"----", &content)?
            }
            (ItemName::Freeform { mean, name }, None) => {
                let mut content = text_atom(b"mean", mean)?;
                content.extend(text_atom(b"name", name)?);
                content.extend(data);
                atom(b"----", &content)?
            }
        };
        match position {
            Some(index) => self.items[index] = item,
            None => self.items.push(item),
        }
        Ok(())
    }

    /// Puts back the exact bytes of the item `field` names, or removes the
    /// item when `raw` is `None`.
    pub(super) fn restore(&mut self, field: &str, raw: Option<&[u8]>) -> Result<(), ErrorSource> {
        if let Some(raw) = raw {
            let is_whole_item = big_endian_u32(raw, 0) == u32::try_from(raw.len()).ok()
                && raw.len() >= ATOM_HEADER_LENGTH
                && ItemName::parse(field)?.names(raw);
            if !is_whole_item {
                return Err(format!("the saved bytes are not a {field} item").into());
            }
        }
        match (self.position(field)?, raw) {
            (Some(index), Some(raw)) => self.items[index] = raw.to_vec(),
            (Some(index), None) => {
                self.items.remove(index);
            }
            (None, Some(raw)) => self.items.push(raw.to_vec()),
            (None, None) => {}
        }
        Ok(())
    }

    /// The `moov` atom with the current items in it. A `free` atom after
    /// `ilst` is padding: it is resized so that `moov` gets its preferred
    /// length, or else keeps the length it has, when that is possible.
    fn rebuilt_moov(&self) -> Result<Vec<u8>, ErrorSource> {
        let moov = &self.moov;
        let ilst = atom(b"ilst", &self.items.concat())?;
        let layout = &self.layout;

        let Some(udta_range) = layout.udta.clone() else {
            let udta = atom(b"udta", &new_meta(&ilst)?)?;
            return with_replaced(moov, moov.len()..moov.len(), &udta);
        };
        let udta = &moov[udta_range.clone()];
        let Some(meta_range) = layout.meta.clone() else {
            let end = layout.udta_atoms_end - udta_range.start;
            let udta = with_replaced(udta, end..end, &new_meta(&ilst)?)?;
            return with_replaced(moov, udta_range, &udta);
        };
        let meta = &moov[meta_range.clone()];
        let Some(ilst_range) = layout.ilst.clone() else {
            let meta = with_replaced(meta, meta.len()..meta.len(), &ilst)?;
            let meta_in_udta =
                meta_range.start - udta_range.start..meta_range.end - udta_range.start;
            let udta = with_replaced(udta, meta_in_udta, &meta)?;
            return with_replaced(moov, udta_range, &udta);
        };

        // Everything from the items to the end of the padding is rebuilt:
        // the items, what sits between them and the padding, the padding.
        let padding_range = layout
            .padding
            .clone()
            .unwrap_or(ilst_range.end..ilst_range.end);
        let between = &moov[ilst_range.end..padding_range.start];
        let old_length = moov.len() as i64;
        let length_without_padding = old_length - (padding_range.end - ilst_range.start) as i64
            + (ilst.len() + between.len()) as i64;
        let keeps_length = layout.padding.is_some().then_some(old_length);
        let padding_length = [
            self.preferred_moov_length.map(|length| length as i64),
            keeps_length,
        ]
        .into_iter()
        .flatten()
        .map(|wanted| wanted - length_without_padding)
        .find(|length| *length == 0 || *length >= ATOM_HEADER_LENGTH as i64);
        let padding = match padding_length {
            Some(0) => Vec::new(),
            Some(length) => atom(b"free", &vec![0; length as usize - ATOM_HEADER_LENGTH])?,
            None => moov[padding_range.clone()].to_vec(),
        };

        let rebuilt = [&ilst[..], between, &padding[..]].concat();
        let in_meta = ilst_range.start - meta_range.start..padding_range.end - meta_range.start;
        let meta = with_replaced(meta, in_meta, &rebuilt)?;
        let meta_in_udta = meta_range.start - udta_range.start..meta_range.end - udta_range.start;
        let udta = with_replaced(udta, meta_in_udta, &meta)?;
        with_replaced(moov, udta_range, &udta)
    }

    /// The change that writes the items back to the file.
    ///
    /// When `moov` changes length, a `free` atom right after it is resized
    /// to take up the difference, or to give the file its preferred length.
    /// Whatever then still moves has the chunk offsets that point at it
    /// moved along.
    pub(super) fn splices(&self) -> Result<Vec<Splice>, ErrorSource> {
        if self.layout.ilst.is_none() && self.items.is_empty() {
            return Ok(Vec::new());
        }
        let mut moov = self.rebuilt_moov()?;
        let old_end = self.moov_start + self.moov.len() as u64;
        let growth = moov.len() as i64 - self.moov.len() as i64;
        let old_padding = i64::from(self.padding_after_moov.unwrap_or(0));

        let restores_file_length = self
            .preferred_file_length
            .map(|length| old_padding + length as i64 - self.file_length as i64 - growth);
        let keeps_file_length = self.padding_after_moov.map(|_| old_padding - growth);
        let new_padding = [restores_file_length, keeps_file_length]
            .into_iter()
            .flatten()
            .find(|length| *length == 0 || *length >= ATOM_HEADER_LENGTH as i64)
            .unwrap_or(old_padding);

        let mut splices = Vec::new();
        if new_padding != old_padding {
            let bytes = if new_padding == 0 {
                Vec::new()
            } else {
                atom(b"free", &vec![0; new_padding as usize - ATOM_HEADER_LENGTH])?
            };
            splices.push(Splice {
                start: old_end,
                end: old_end + old_padding as u64,
                bytes,
            });
        }
        let distance = growth + new_padding - old_padding;
        if distance != 0 {
            if self.is_fragmented {
                return Err("moving the audio of a fragmented file is not supported".into());
            }
            move_chunk_offsets(&mut moov, old_end, distance)?;
        }
        splices.push(Splice {
            start: self.moov_start,
            end: old_end,
            bytes: moov,
        });
        Ok(splices)
    }

    /// Checks `written`, read back from `copy`, against this file as it
    /// was loaded from `original`: apart from the items, the padding and
    /// the chunk offsets, `moov` must be the same bytes, and every chunk
    /// offset must still point at the same audio.
    pub(super) fn check_rewrite(
        &self,
        original: &mut File,
        written: &Self,
        copy: &mut File,
    ) -> Result<(), ErrorSource> {
        if self.outside_metadata()? != written.outside_metadata()? {
            return Err("atoms outside the metadata would change".into());
        }
        let mut expected = self.around_items();
        if self.layout.meta.is_none() && written.layout.meta.is_some() {
            // The write created the metadata atoms around the items.
            if self.layout.udta.is_none() {
                expected.push(Vec::new());
            }
            expected.push(b"meta\x00\x00\x00\x00".to_vec());
            expected.push(METADATA_HANDLER.to_vec());
        }
        if expected != written.around_items() {
            return Err("atoms next to the metadata items would change".into());
        }
        let old_offsets = chunk_offsets(&self.moov)?;
        let new_offsets = chunk_offsets(&written.moov)?;
        if old_offsets.len() != new_offsets.len() {
            return Err("the number of audio chunks would change".into());
        }
        let step = (old_offsets.len() / CHUNK_OFFSET_SAMPLES).max(1);
        for (old, new) in old_offsets.iter().zip(&new_offsets).step_by(step) {
            if !same_bytes(original, *old, copy, *new, ATOM_HEADER_LENGTH as u64)? {
                return Err("a chunk offset would no longer point at its audio".into());
            }
        }
        Ok(())
    }

    /// `moov` without `udta`, its own size and the chunk offset values.
    fn outside_metadata(&self) -> Result<Vec<u8>, ErrorSource> {
        let mut moov = self.moov.clone();
        for table in chunk_offset_tables(&moov)? {
            moov[table.entries].fill(0);
        }
        if let Some(udta) = &self.layout.udta {
            moov.drain(udta.clone());
        }
        moov[..4].fill(0);
        Ok(moov)
    }

    /// What `udta` and `meta` hold besides the items and the padding.
    fn around_items(&self) -> Vec<Vec<u8>> {
        let moov = &self.moov;
        let mut kept = Vec::new();
        let Some(udta) = &self.layout.udta else {
            return kept;
        };
        let (in_udta, atoms_end) =
            atoms_until_stop(moov, udta.start + ATOM_HEADER_LENGTH, udta.end);
        kept.extend(
            in_udta
                .iter()
                .filter(|atom| &atom.kind != b"meta")
                .map(|atom| moov[atom.range.clone()].to_vec()),
        );
        kept.push(moov[atoms_end..udta.end].to_vec());
        let Some(meta) = &self.layout.meta else {
            return kept;
        };
        let content_start = meta.start + ATOM_HEADER_LENGTH + FULL_BOX_HEADER_LENGTH;
        kept.push(moov[meta.start + 4..content_start].to_vec());
        let in_meta = atoms(moov, content_start, meta.end).unwrap_or_default();
        kept.extend(
            in_meta
                .iter()
                .filter(|atom| &atom.kind != b"ilst" && &atom.kind != b"free")
                .map(|atom| moov[atom.range.clone()].to_vec()),
        );
        kept
    }
}

fn new_meta(ilst: &[u8]) -> Result<Vec<u8>, ErrorSource> {
    let mut content = vec![0; FULL_BOX_HEADER_LENGTH];
    content.extend(METADATA_HANDLER);
    content.extend(ilst);
    atom(b"meta", &content)
}

/// One `stco` or `co64` table inside `moov`.
struct ChunkOffsetTable {
    entries: Range<usize>,
    entry_length: usize,
}

fn chunk_offset_tables(moov: &[u8]) -> Result<Vec<ChunkOffsetTable>, ErrorSource> {
    let damaged = || "the track atoms are damaged";
    let inside = |range: &Range<usize>| {
        atoms(moov, range.start + ATOM_HEADER_LENGTH, range.end).ok_or_else(damaged)
    };
    let mut tables = Vec::new();
    let top = atoms(moov, ATOM_HEADER_LENGTH, moov.len()).ok_or_else(damaged)?;
    for trak in top.iter().filter(|atom| &atom.kind == b"trak") {
        let mut level = inside(&trak.range)?;
        for kind in [b"mdia", b"minf", b"stbl"] {
            level = match first_of_kind(&level, kind) {
                Some(next) => inside(&next.range)?,
                None => Vec::new(),
            };
        }
        for table in level {
            let entry_length = match &table.kind {
                b"stco" => 4,
                b"co64" => 8,
                _ => continue,
            };
            let count_at = table.range.start + ATOM_HEADER_LENGTH + FULL_BOX_HEADER_LENGTH;
            let count = big_endian_u32(moov, count_at).ok_or_else(damaged)? as usize;
            let entries = count_at + 4..count_at + 4 + count * entry_length;
            if entries.end > table.range.end {
                return Err(damaged().into());
            }
            tables.push(ChunkOffsetTable {
                entries,
                entry_length,
            });
        }
    }
    Ok(tables)
}

/// Every chunk offset in `moov`, in table order.
fn chunk_offsets(moov: &[u8]) -> Result<Vec<u64>, ErrorSource> {
    let mut offsets = Vec::new();
    for table in chunk_offset_tables(moov)? {
        for entry in moov[table.entries].chunks_exact(table.entry_length) {
            let mut wide = [0; 8];
            wide[8 - entry.len()..].copy_from_slice(entry);
            offsets.push(u64::from_be_bytes(wide));
        }
    }
    Ok(offsets)
}

/// Adds `distance` to every chunk offset at or after `moved_from`.
fn move_chunk_offsets(moov: &mut [u8], moved_from: u64, distance: i64) -> Result<(), ErrorSource> {
    for table in chunk_offset_tables(moov)? {
        for entry in moov[table.entries].chunks_exact_mut(table.entry_length) {
            let mut wide = [0; 8];
            wide[8 - entry.len()..].copy_from_slice(entry);
            let offset = u64::from_be_bytes(wide);
            if offset < moved_from {
                continue;
            }
            let moved = offset
                .checked_add_signed(distance)
                .ok_or("a chunk offset would be out of range")?;
            let bytes = moved.to_be_bytes();
            if bytes[..8 - entry.len()].iter().any(|byte| *byte != 0) {
                return Err("the audio would move past what 32 bit chunk offsets reach".into());
            }
            entry.copy_from_slice(&bytes[8 - entry.len()..]);
        }
    }
    Ok(())
}
