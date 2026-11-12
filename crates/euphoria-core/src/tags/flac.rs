// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The metadata blocks of a FLAC file, kept as the bytes they were read
//! from. Only the Vorbis comment a caller names is ever rewritten; every
//! other comment, the pictures and all other blocks go back unchanged.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use super::id3v2;
use super::replace::Splice;
use super::vorbis::Comments;
use crate::error::ErrorSource;

const MARKER: &[u8; 4] = b"fLaC";
const BLOCK_HEADER_LENGTH: usize = 4;
const LAST_BLOCK_FLAG: u8 = 0x80;
const PADDING: u8 = 1;
const VORBIS_COMMENT: u8 = 4;
const LARGEST_BLOCK: usize = 0x00FF_FFFF;

/// One metadata block without its header.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Block {
    kind: u8,
    content: Vec<u8>,
}

/// The metadata of a FLAC file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FlacFile {
    metadata_start: u64,
    metadata_end: u64,
    blocks: Vec<Block>,
    comment_block: Option<usize>,
    comments: Comments,
    /// A length the metadata should get back when padding allows, set by
    /// an undo.
    preferred_length: Option<u64>,
    problem: Option<&'static str>,
}

impl FlacFile {
    /// Reads every metadata block of `file`. `start` is where the FLAC
    /// stream begins, after an ID3v2 tag if the file has one in front.
    pub(super) fn load(file: &mut File, start: u64) -> Result<Self, ErrorSource> {
        file.seek(SeekFrom::Start(start))?;
        let mut marker = [0; 4];
        file.read_exact(&mut marker)?;
        if &marker != MARKER {
            return Err("the FLAC stream marker is missing".into());
        }
        let metadata_start = start + MARKER.len() as u64;
        let mut flac = Self {
            metadata_start,
            metadata_end: metadata_start,
            blocks: Vec::new(),
            comment_block: None,
            comments: Comments::default(),
            preferred_length: None,
            problem: None,
        };
        loop {
            let mut header = [0; BLOCK_HEADER_LENGTH];
            file.read_exact(&mut header)?;
            let length = u64::from(u32::from_be_bytes([0, header[1], header[2], header[3]]));
            let mut content = Vec::new();
            file.take(length).read_to_end(&mut content)?;
            if content.len() as u64 != length {
                return Err("a FLAC metadata block is cut short".into());
            }
            flac.metadata_end += BLOCK_HEADER_LENGTH as u64 + length;
            let kind = header[0] & !LAST_BLOCK_FLAG;
            if kind == VORBIS_COMMENT {
                flac.read_comments(&content);
            }
            if kind == PADDING && content.iter().any(|byte| *byte != 0) {
                flac.problem = Some("its FLAC padding holds data");
            }
            flac.blocks.push(Block { kind, content });
            if header[0] & LAST_BLOCK_FLAG != 0 {
                return Ok(flac);
            }
        }
    }

    fn read_comments(&mut self, content: &[u8]) {
        if self.comment_block.is_some() {
            self.problem = Some("it holds more than one Vorbis comment block");
            return;
        }
        self.comment_block = Some(self.blocks.len());
        match Comments::parse(content) {
            Some((comments, length)) if length == content.len() => self.comments = comments,
            _ => self.problem = Some("its Vorbis comment block is damaged"),
        }
    }

    /// The length of all metadata blocks together.
    pub(super) fn metadata_length(&self) -> u64 {
        self.metadata_end - self.metadata_start
    }

    /// Asks for the metadata to get `length` bytes when padding allows.
    pub(super) fn prefer_metadata_length(&mut self, length: Option<u64>) {
        self.preferred_length = length;
    }

    /// Why the metadata cannot be rewritten without loss, if it cannot.
    pub(super) fn problem(&self) -> Option<&'static str> {
        self.problem
    }

    /// Everything a rewrite must keep, in order: the vendor text, every
    /// comment, then every block that is neither comments nor padding.
    pub(super) fn items(&self) -> Vec<Vec<u8>> {
        let other_blocks = self
            .blocks
            .iter()
            .filter(|block| block.kind != VORBIS_COMMENT && block.kind != PADDING)
            .map(|block| {
                let mut item = vec![block.kind];
                item.extend(&block.content);
                item
            });
        std::iter::once(self.comments.vendor.clone())
            .chain(self.comments.entries.iter().cloned())
            .chain(other_blocks)
            .collect()
    }

    /// The index of the comment `field` names among [`FlacFile::items`].
    pub(super) fn position(&self, field: &str) -> Option<usize> {
        self.comments.index(field).map(|index| index + 1)
    }

    /// The whole comment `field` names, as `NAME=value` bytes.
    pub(super) fn raw(&self, field: &str) -> Option<&[u8]> {
        self.comments.raw(field)
    }

    /// The text of `field`, or `None` when there is no such comment.
    pub(super) fn get(&self, field: &str) -> Option<String> {
        self.comments.get(field)
    }

    /// Sets `field` to `value`, or removes it when `value` is `None`. An
    /// existing comment keeps its place and the spelling of its name.
    pub(super) fn set(&mut self, field: &str, value: Option<&str>) {
        self.comments.set(field, value);
    }

    /// Puts back the exact bytes of the comment `field` names, or removes
    /// the comment when `raw` is `None`.
    pub(super) fn restore(&mut self, field: &str, raw: Option<&[u8]>) -> Result<(), ErrorSource> {
        self.comments.restore(field, raw)
    }

    /// The change that writes the metadata back to the file. Padding is
    /// resized to give the metadata its preferred length, or else the
    /// length it has now, when there is enough of it.
    pub(super) fn splices(&self) -> Result<Vec<Splice>, ErrorSource> {
        let mut blocks = self.blocks.clone();
        // A block with no vendor text and no comments is what a write to a
        // file without comments creates, so it goes away with its last
        // comment.
        let is_empty = self.comments.is_empty();
        match (self.comment_block, is_empty) {
            (Some(index), true) => {
                blocks.remove(index);
            }
            (Some(index), false) => blocks[index].content = self.comments.serialize()?,
            (None, true) => {}
            (None, false) => {
                let before_padding = blocks
                    .iter()
                    .position(|block| block.kind == PADDING)
                    .unwrap_or(blocks.len());
                let comments = Block {
                    kind: VORBIS_COMMENT,
                    content: self.comments.serialize()?,
                };
                blocks.insert(before_padding, comments);
            }
        }

        let length = |blocks: &[Block]| -> u64 {
            blocks
                .iter()
                .map(|block| (BLOCK_HEADER_LENGTH + block.content.len()) as u64)
                .sum()
        };
        let new_length = length(&blocks);
        if let Some(padding) = blocks.iter_mut().rfind(|block| block.kind == PADDING) {
            let resized = [self.preferred_length, Some(self.metadata_length())]
                .into_iter()
                .flatten()
                .filter_map(|wanted| {
                    (padding.content.len() as u64 + wanted).checked_sub(new_length)
                })
                .find(|resized| *resized <= LARGEST_BLOCK as u64);
            if let Some(resized) = resized {
                padding.content = vec![0; resized as usize];
            }
        }

        let mut bytes = Vec::new();
        let last = blocks.len().saturating_sub(1);
        for (index, block) in blocks.iter().enumerate() {
            if block.content.len() > LARGEST_BLOCK {
                return Err("a FLAC metadata block would be larger than the format allows".into());
            }
            let last_flag = if index == last { LAST_BLOCK_FLAG } else { 0 };
            bytes.push(block.kind | last_flag);
            bytes.extend(&(block.content.len() as u32).to_be_bytes()[1..]);
            bytes.extend(&block.content);
        }
        Ok(vec![Splice {
            start: self.metadata_start,
            end: self.metadata_end,
            bytes,
        }])
    }
}

/// Where the FLAC stream starts in a file that begins with `beginning`:
/// at the start, or after an ID3v2 tag.
pub(super) fn stream_start(beginning: &[u8]) -> Result<u64, ErrorSource> {
    Ok(id3v2::total_length(beginning)?.unwrap_or(0))
}
