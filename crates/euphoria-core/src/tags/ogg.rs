// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The Vorbis comments of an Ogg Vorbis file. They sit in the second of the
//! three header packets, which share the pages after the first with the
//! third, the setup header. A rewrite lays the comment and setup packets
//! out again over exactly as many pages as before, so that the audio pages
//! after them keep their sequence numbers and every one of their bytes.

use std::fs::File;

use super::native::read_exactly;
use super::replace::Splice;
use super::vorbis::Comments;
use crate::error::ErrorSource;

/// Every Ogg page starts with this.
pub(super) const CAPTURE_PATTERN: &[u8; 4] = b"OggS";
const PAGE_HEADER_LENGTH: u64 = 27;
const CRC_OFFSET: usize = 22;
const CONTINUED_PACKET: u8 = 0x01;
const FIRST_PAGE: u8 = 0x02;
const LAST_PAGE: u8 = 0x04;
const LARGEST_SEGMENT: u8 = 255;
const MOST_SEGMENTS: usize = 255;
/// The granule position of a page on which no packet ends.
const NO_PACKET_ENDS: u64 = u64::MAX;
/// The header packets of a Vorbis stream start with their type and this.
const VORBIS: &[u8; 6] = b"vorbis";
const IDENTIFICATION_HEADER: u8 = 1;
const COMMENT_HEADER: u8 = 3;
const SETUP_HEADER: u8 = 5;
const HEADER_PACKETS: usize = 3;

/// One page header, without its segment table and checksum, which a
/// rewrite works out again.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Page {
    flags: u8,
    granule_position: u64,
    serial_number: u32,
    sequence_number: u32,
    segments: usize,
    /// Whether a packet ends on this page.
    ends_a_packet: bool,
}

/// The header packets of an Ogg Vorbis file and the pages that hold the
/// comment and setup headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OggVorbisFile {
    /// The identification header, which stays on the first page.
    identification: Vec<u8>,
    comments: Comments,
    /// The framing bit after the comments, and any bytes a program put
    /// after it.
    comment_end: Vec<u8>,
    setup: Vec<u8>,
    /// The pages after the first one, up to the page the setup header ends
    /// on, and where they lie in the file.
    pages: Vec<Page>,
    pages_start: u64,
    pages_end: u64,
    /// The number of lacing values the comment header has in the file.
    comment_segments: usize,
    problem: Option<&'static str>,
}

/// The CRC-32 of Ogg pages: polynomial 0x04C11DB7, not reflected, starting
/// from zero.
fn checksum(bytes: &[u8]) -> u32 {
    const fn table() -> [u32; 256] {
        let mut table = [0; 256];
        let mut index = 0;
        while index < 256 {
            let mut value = (index as u32) << 24;
            let mut bit = 0;
            while bit < 8 {
                value = if value & 0x8000_0000 != 0 {
                    (value << 1) ^ 0x04C1_1DB7
                } else {
                    value << 1
                };
                bit += 1;
            }
            table[index] = value;
            index += 1;
        }
        table
    }
    const TABLE: [u32; 256] = table();
    bytes.iter().fold(0, |crc, byte| {
        (crc << 8) ^ TABLE[(((crc >> 24) as u8) ^ byte) as usize]
    })
}

/// The lacing values of a packet of `length` bytes.
fn lacing(length: usize) -> Vec<u8> {
    let mut values = vec![LARGEST_SEGMENT; length / usize::from(LARGEST_SEGMENT)];
    values.push((length % usize::from(LARGEST_SEGMENT)) as u8);
    values
}

/// One page as it is read: its header, its lacing values, its data, and
/// whether its checksum matches.
struct ReadPage {
    page: Page,
    lacing: Vec<u8>,
    data: Vec<u8>,
    length: u64,
    is_intact: bool,
}

fn read_page(file: &mut File, offset: u64) -> Result<ReadPage, ErrorSource> {
    let header =
        read_exactly(file, offset, PAGE_HEADER_LENGTH).map_err(|_| "an Ogg page is cut short")?;
    if &header[..4] != CAPTURE_PATTERN || header[4] != 0 {
        return Err("an Ogg page is damaged".into());
    }
    let word = |start: usize| {
        u32::from_le_bytes([
            header[start],
            header[start + 1],
            header[start + 2],
            header[start + 3],
        ])
    };
    let granule_position = u64::from(word(6)) | (u64::from(word(10)) << 32);
    let segments = usize::from(header[26]);
    let lacing = read_exactly(file, offset + PAGE_HEADER_LENGTH, segments as u64)
        .map_err(|_| "an Ogg page is cut short")?;
    let data_length: u64 = lacing.iter().map(|value| u64::from(*value)).sum();
    let data_start = offset + PAGE_HEADER_LENGTH + segments as u64;
    let data =
        read_exactly(file, data_start, data_length).map_err(|_| "an Ogg page is cut short")?;

    let mut whole = header.clone();
    whole[CRC_OFFSET..CRC_OFFSET + 4].fill(0);
    whole.extend(&lacing);
    whole.extend(&data);
    let is_intact = checksum(&whole) == word(CRC_OFFSET);
    Ok(ReadPage {
        page: Page {
            flags: header[5],
            granule_position,
            serial_number: word(14),
            sequence_number: word(18),
            segments,
            ends_a_packet: lacing.iter().any(|value| *value < LARGEST_SEGMENT),
        },
        lacing,
        data,
        length: PAGE_HEADER_LENGTH + segments as u64 + data_length,
        is_intact,
    })
}

fn header_body(packet: &[u8], kind: u8) -> Option<&[u8]> {
    match packet {
        [first, rest @ ..] if *first == kind && rest.starts_with(VORBIS) => {
            Some(&rest[VORBIS.len()..])
        }
        _ => None,
    }
}

impl OggVorbisFile {
    /// Reads the three header packets of the Ogg Vorbis stream that starts
    /// the file.
    pub(super) fn load(file: &mut File) -> Result<Self, ErrorSource> {
        let mut problem = None;
        let mut packets: Vec<Vec<u8>> = Vec::new();
        let mut packet = Vec::new();
        let mut pages = Vec::new();
        let mut offset = 0;
        let mut first_page_end = 0;
        let mut serial_number = None;
        let mut expected_sequence = 0;
        let mut comment_segments = 0;
        while packets.len() < HEADER_PACKETS {
            let read = read_page(file, offset)?;
            let is_first = offset == 0;
            if *serial_number.get_or_insert(read.page.serial_number) != read.page.serial_number {
                return Err("the Ogg file holds more than one stream".into());
            }
            if is_first && read.page.flags & FIRST_PAGE == 0 {
                return Err("the Ogg stream does not start where the file does".into());
            }
            if read.page.sequence_number != expected_sequence {
                problem = Some("its Ogg pages are out of order");
            }
            expected_sequence = read.page.sequence_number.wrapping_add(1);
            if !read.is_intact {
                problem = Some("an Ogg page does not match its checksum");
            }
            if read.page.flags & LAST_PAGE != 0 {
                problem = Some("its Ogg stream ends among its header pages");
            }
            let continues = read.page.flags & CONTINUED_PACKET != 0;
            if continues == packet.is_empty() {
                // A page continues a packet exactly when the page before it
                // ended inside one.
                problem = Some("its Ogg pages do not join up");
            }
            let mut position = 0;
            let mut consumed_segments = 0;
            for value in &read.lacing {
                consumed_segments += 1;
                let end = position + usize::from(*value);
                packet.extend(&read.data[position..end]);
                position = end;
                if packets.len() == 1 {
                    comment_segments += 1;
                }
                if *value < LARGEST_SEGMENT {
                    packets.push(std::mem::take(&mut packet));
                    if packets.len() == HEADER_PACKETS {
                        break;
                    }
                }
            }
            if packets.len() == HEADER_PACKETS && consumed_segments != read.lacing.len() {
                problem = Some("its first audio packet shares a page with its setup header");
            }
            if is_first {
                first_page_end = read.length;
                if packets.len() != 1 || !packet.is_empty() {
                    problem = Some("its first Ogg page holds more than the identification header");
                }
            } else {
                pages.push(read.page);
            }
            offset += read.length;
        }

        let identification = &packets[0];
        if header_body(identification, IDENTIFICATION_HEADER).is_none() {
            return Err("the Ogg file does not hold Vorbis audio".into());
        }
        let comment_body = header_body(&packets[1], COMMENT_HEADER)
            .ok_or("the Vorbis comment header is missing")?;
        if header_body(&packets[2], SETUP_HEADER).is_none() {
            return Err("the Vorbis setup header is missing".into());
        }
        let (comments, length) =
            Comments::parse(comment_body).ok_or("the Vorbis comment header is damaged")?;
        let comment_end = comment_body[length..].to_vec();
        if comment_end.first().is_none_or(|framing| framing & 1 == 0) {
            problem = Some("its Vorbis comment header is damaged");
        }
        Ok(Self {
            identification: identification.clone(),
            comments,
            comment_end,
            setup: packets[2].clone(),
            pages,
            pages_start: first_page_end,
            pages_end: offset,
            comment_segments,
            problem,
        })
    }

    /// Why the file cannot be rewritten without loss, if it cannot.
    pub(super) fn problem(&self) -> Option<&'static str> {
        self.problem
    }

    /// The length of the pages that hold the comment and setup headers.
    pub(super) fn header_length(&self) -> u64 {
        self.pages_end - self.pages_start
    }

    /// Everything a rewrite must keep, in order: the identification header,
    /// the vendor text, every comment, what follows the comments, and the
    /// setup header.
    pub(super) fn items(&self) -> Vec<Vec<u8>> {
        std::iter::once(self.identification.clone())
            .chain(std::iter::once(self.comments.vendor.clone()))
            .chain(self.comments.entries.iter().cloned())
            .chain([self.comment_end.clone(), self.setup.clone()])
            .collect()
    }

    /// The index of the comment `field` names among [`OggVorbisFile::items`].
    pub(super) fn position(&self, field: &str) -> Option<usize> {
        self.comments.index(field).map(|index| index + 2)
    }

    /// The whole comment `field` names, as `NAME=value` bytes.
    pub(super) fn raw(&self, field: &str) -> Option<&[u8]> {
        self.comments.raw(field)
    }

    /// The text of `field`, or `None` when there is no such comment.
    pub(super) fn get(&self, field: &str) -> Option<String> {
        self.comments.get(field)
    }

    /// Sets `field` to `value`, or removes it when `value` is `None`.
    pub(super) fn set(&mut self, field: &str, value: Option<&str>) {
        self.comments.set(field, value);
    }

    /// Puts back the exact bytes of the comment `field` names, or removes
    /// the comment when `raw` is `None`.
    pub(super) fn restore(&mut self, field: &str, raw: Option<&[u8]>) -> Result<(), ErrorSource> {
        self.comments.restore(field, raw)
    }

    fn comment_packet(&self) -> Result<Vec<u8>, ErrorSource> {
        let mut packet = vec![COMMENT_HEADER];
        packet.extend(VORBIS);
        packet.extend(self.comments.serialize()?);
        packet.extend(&self.comment_end);
        Ok(packet)
    }

    /// How many lacing values each page gets. The change in the comment
    /// header's length is taken up by the page its last lacing value was
    /// on, and every other page keeps its count. A write and its undo
    /// change that page by the same amount in opposite directions, so the
    /// undone file has the original layout.
    fn page_segments(&self, comment_segments: usize) -> Result<Vec<usize>, ErrorSource> {
        let too_large = || -> ErrorSource {
            "the new comments do not fit the pages the old ones were on, and the audio pages \
             after them would have to be renumbered"
                .into()
        };
        let mut counts: Vec<usize> = self.pages.iter().map(|page| page.segments).collect();
        let mut before = 0;
        let last_comment_page = counts
            .iter()
            .position(|count| {
                before += count;
                before >= self.comment_segments
            })
            .ok_or("the Vorbis comment header is cut short")?;
        let before_that: usize = counts[..last_comment_page].iter().sum();
        let count = (counts[last_comment_page] + comment_segments)
            .checked_sub(self.comment_segments)
            .filter(|count| (1..=MOST_SEGMENTS).contains(count))
            .ok_or_else(too_large)?;
        if comment_segments <= before_that {
            return Err(too_large());
        }
        counts[last_comment_page] = count;
        Ok(counts)
    }

    /// The change that writes the comment and setup headers back over the
    /// pages they were on.
    pub(super) fn splices(&self) -> Result<Vec<Splice>, ErrorSource> {
        let comment = self.comment_packet()?;
        let comment_lacing = lacing(comment.len());
        let mut values = comment_lacing.clone();
        values.extend(lacing(self.setup.len()));
        let mut data = comment;
        data.extend(&self.setup);

        let counts = self.page_segments(comment_lacing.len())?;
        let mut bytes = Vec::new();
        let mut value_start = 0;
        let mut data_start = 0;
        let mut continues = false;
        for (page, count) in self.pages.iter().zip(counts) {
            let page_values = &values[value_start..value_start + count];
            let data_length: usize = page_values.iter().map(|value| usize::from(*value)).sum();
            let ends_a_packet = page_values.iter().any(|value| *value < LARGEST_SEGMENT);
            let granule_position = if ends_a_packet == page.ends_a_packet {
                page.granule_position
            } else if ends_a_packet {
                0
            } else {
                NO_PACKET_ENDS
            };
            let flags = if continues {
                page.flags | CONTINUED_PACKET
            } else {
                page.flags & !CONTINUED_PACKET
            };
            let mut page_bytes = CAPTURE_PATTERN.to_vec();
            page_bytes.push(0);
            page_bytes.push(flags);
            page_bytes.extend(granule_position.to_le_bytes());
            page_bytes.extend(page.serial_number.to_le_bytes());
            page_bytes.extend(page.sequence_number.to_le_bytes());
            page_bytes.extend([0; 4]);
            page_bytes.push(count as u8);
            page_bytes.extend(page_values);
            page_bytes.extend(&data[data_start..data_start + data_length]);
            let crc = checksum(&page_bytes);
            page_bytes[CRC_OFFSET..CRC_OFFSET + 4].copy_from_slice(&crc.to_le_bytes());
            bytes.extend(page_bytes);

            continues = page_values.last() == Some(&LARGEST_SEGMENT);
            value_start += count;
            data_start += data_length;
        }
        if value_start != values.len() {
            return Err("the Vorbis headers do not fill their pages".into());
        }
        Ok(vec![Splice {
            start: self.pages_start,
            end: self.pages_end,
            bytes,
        }])
    }
}

/// Whether a file that starts with `beginning` is an Ogg file.
pub(super) fn is_ogg(beginning: &[u8]) -> bool {
    beginning.starts_with(CAPTURE_PATTERN)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_matches_the_ogg_reference() {
        // The CRC of an Ogg page is the MSB first CRC-32 with polynomial
        // 0x04C11DB7 and no inversion; this is its check value.
        assert_eq!(checksum(b"123456789"), 0x89A1_897F);
    }

    #[test]
    fn lacing_ends_with_a_short_value() {
        assert_eq!(lacing(0), [0]);
        assert_eq!(lacing(254), [254]);
        assert_eq!(lacing(255), [255, 0]);
        assert_eq!(lacing(600), [255, 255, 90]);
    }

    fn file_with(pages: &[usize], comment_segments: usize) -> OggVorbisFile {
        OggVorbisFile {
            identification: Vec::new(),
            comments: Comments::default(),
            comment_end: vec![1],
            setup: Vec::new(),
            pages: pages
                .iter()
                .enumerate()
                .map(|(index, segments)| Page {
                    flags: 0,
                    granule_position: 0,
                    serial_number: 1,
                    sequence_number: index as u32 + 1,
                    segments: *segments,
                    ends_a_packet: true,
                })
                .collect(),
            pages_start: 0,
            pages_end: 0,
            comment_segments,
            problem: None,
        }
    }

    #[test]
    fn the_page_where_the_comments_end_takes_up_the_change() {
        // Comments on page one, setup over pages one and two.
        let file = file_with(&[20, 30], 3);
        assert_eq!(file.page_segments(3).unwrap(), [20, 30]);
        assert_eq!(file.page_segments(5).unwrap(), [22, 30]);
        assert_eq!(file.page_segments(1).unwrap(), [18, 30]);
        // Comments over two full pages and part of a third.
        let file = file_with(&[255, 255, 40], 520);
        assert_eq!(file.page_segments(530).unwrap(), [255, 255, 50]);
        assert!(file.page_segments(800).is_err());
        assert!(file.page_segments(500).is_err());
    }
}
