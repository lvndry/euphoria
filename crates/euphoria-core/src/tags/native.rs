// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The tag a file format stores natively, as a list of items kept as the
//! bytes they were read from. A write exchanges the items a caller names
//! and carries every other byte of the file over unchanged. Before the
//! result replaces the file it is read back and compared, item by item,
//! with the original.

use std::fmt;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use super::TagFormat;
use super::flac::{self, FlacFile};
use super::id3v2::{self, Id3Tag};
use super::mp4::Mp4File;
use super::replace::{Replacement, Splice, carried_over, compose, has_several_links};
use crate::error::{ErrorSource, EuphoriaError};

const PROBE_LENGTH: usize = 16;
const MPEG_SEARCH_LENGTH: u64 = 64 * 1024;
const IFF_HEADER_LENGTH: u64 = 12;
const CHUNK_HEADER_LENGTH: u64 = 8;
const ID3_CHUNK: &[u8; 4] = b"ID3 ";
const SEVERAL_LINKS: &str = "it has more than one hard link, and replacing it would leave its other names \
     with the old contents";

/// What a file is loaded for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Purpose {
    /// Showing its tags. Damaged parts of a tag are skipped.
    Reading,
    /// Changing its tags. A tag with parts that cannot be told apart is
    /// refused, because writing it back could lose them.
    Rewriting,
}

/// Why a file that can be read is not rewritten.
#[derive(Debug)]
struct Refusal(&'static str);

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "refusing to rewrite the file: {}", self.0)
    }
}

impl std::error::Error for Refusal {}

fn refusal(reason: &'static str) -> ErrorSource {
    Box::new(Refusal(reason))
}

/// Where a file keeps its ID3v2 tag.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Id3Place {
    /// At the very start, as in MP3. `length` is zero when there is none.
    Start { length: u64 },
    /// In a chunk of a RIFF or AIFF container.
    Chunk {
        little_endian: bool,
        /// The chunk with its header and padding byte, when there is one.
        chunk: Option<std::ops::Range<u64>>,
        /// The size the chunk declares, which leaves out its padding byte.
        tag_length: u64,
        /// The chunk's name as the file spells it.
        name: [u8; 4],
        /// The size the container declares for itself.
        container_size: u32,
        /// Where the container's chunks end.
        container_end: u64,
    },
}

/// A file whose native tag is ID3v2: MP3, WAV or AIFF.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Id3File {
    tag: Id3Tag,
    place: Id3Place,
    /// A length the tag should get back when it fits, set by an undo.
    preferred_length: Option<u64>,
    problem: Option<&'static str>,
}

fn read_exactly(file: &mut File, start: u64, length: u64) -> Result<Vec<u8>, ErrorSource> {
    let mut bytes = Vec::new();
    file.seek(SeekFrom::Start(start))?;
    file.take(length).read_to_end(&mut bytes)?;
    if bytes.len() as u64 == length {
        Ok(bytes)
    } else {
        Err("the file is shorter than its structure says".into())
    }
}

impl Id3File {
    fn at_start(file: &mut File, beginning: &[u8]) -> Result<Self, ErrorSource> {
        let Some(length) = id3v2::total_length(beginning)? else {
            return Ok(Self {
                tag: Id3Tag::new(),
                place: Id3Place::Start { length: 0 },
                preferred_length: None,
                problem: None,
            });
        };
        let tag = Id3Tag::parse(&read_exactly(file, 0, length)?)?;
        let mut next = [0; 3];
        let has_second_tag = file.read_exact(&mut next).is_ok() && &next == b"ID3";
        Ok(Self {
            tag,
            place: Id3Place::Start { length },
            preferred_length: None,
            problem: has_second_tag.then_some("it holds more than one ID3v2 tag"),
        })
    }

    fn in_chunk(
        file: &mut File,
        beginning: &[u8],
        little_endian: bool,
    ) -> Result<Self, ErrorSource> {
        let number = |bytes: &[u8]| {
            let word = [bytes[0], bytes[1], bytes[2], bytes[3]];
            if little_endian {
                u32::from_le_bytes(word)
            } else {
                u32::from_be_bytes(word)
            }
        };
        let container_size = number(&beginning[4..8]);
        let file_length = file.metadata()?.len();
        let container_end = CHUNK_HEADER_LENGTH + u64::from(container_size);
        let mut problem = None;
        if container_end > file_length {
            problem = Some("its container is larger than the file");
        }

        let mut found: Option<(std::ops::Range<u64>, u64, [u8; 4], Id3Tag)> = None;
        let mut offset = IFF_HEADER_LENGTH;
        while offset < container_end.min(file_length) {
            let Ok(header) = read_exactly(file, offset, CHUNK_HEADER_LENGTH) else {
                problem = Some("a chunk is cut short");
                break;
            };
            let size = u64::from(number(&header[4..8]));
            let unpadded_end = offset + CHUNK_HEADER_LENGTH + size;
            // Some programs leave out the padding byte of the last chunk.
            let end = if unpadded_end == container_end {
                unpadded_end
            } else {
                unpadded_end + size % 2
            };
            if end > container_end.min(file_length) {
                problem = Some("a chunk runs past the end of its container");
                break;
            }
            if header[..4].eq_ignore_ascii_case(ID3_CHUNK) {
                if found.is_some() {
                    problem = Some("it holds more than one ID3v2 tag");
                    break;
                }
                let content = read_exactly(file, offset + CHUNK_HEADER_LENGTH, size)?;
                let name = [header[0], header[1], header[2], header[3]];
                found = Some((offset..end, size, name, Id3Tag::parse(&content)?));
            }
            offset = end;
        }

        let (chunk, tag_length, name, tag) = match found {
            Some((chunk, tag_length, name, tag)) => (Some(chunk), tag_length, name, tag),
            None => (None, 0, *ID3_CHUNK, Id3Tag::new()),
        };
        Ok(Self {
            tag,
            place: Id3Place::Chunk {
                little_endian,
                chunk,
                tag_length,
                name,
                container_size,
                container_end,
            },
            preferred_length: None,
            problem,
        })
    }

    fn problem(&self) -> Option<&'static str> {
        self.problem.or(self.tag.problem())
    }

    /// The length of the tag in the file, zero when there is none.
    fn tag_length(&self) -> u64 {
        match &self.place {
            Id3Place::Start { length } => *length,
            Id3Place::Chunk { tag_length, .. } => *tag_length,
        }
    }

    /// The change that writes the tag back to the file. A tag that fits
    /// in the preferred length, or where the old one was, is padded to
    /// that length. A tag without frames is taken out of the file.
    fn splices(&self) -> Result<Vec<Splice>, ErrorSource> {
        let has_frames = !self.tag.frames().is_empty();
        let rooms: Vec<usize> = [self.preferred_length, Some(self.tag_length())]
            .into_iter()
            .flatten()
            .filter(|length| *length > 0)
            .map(|length| length as usize)
            .collect();
        match &self.place {
            Id3Place::Start { length } => {
                let bytes = if has_frames {
                    self.tag.serialize(&rooms)?
                } else {
                    Vec::new()
                };
                Ok(vec![Splice {
                    start: 0,
                    end: *length,
                    bytes,
                }])
            }
            Id3Place::Chunk {
                little_endian,
                chunk,
                name,
                container_size,
                container_end,
                ..
            } => {
                let number = |number: u32| {
                    if *little_endian {
                        number.to_le_bytes()
                    } else {
                        number.to_be_bytes()
                    }
                };
                let mut bytes = Vec::new();
                if has_frames {
                    let tag = self.tag.serialize(&rooms)?;
                    let size = u32::try_from(tag.len())
                        .map_err(|_| "the tag is too large for its chunk")?;
                    bytes.extend(name);
                    bytes.extend(number(size));
                    bytes.extend(tag);
                    if bytes.len() % 2 != 0 {
                        bytes.push(0);
                    }
                }
                if chunk.is_none() && !bytes.is_empty() && !container_end.is_multiple_of(2) {
                    // The last chunk lacks its padding byte, and a chunk
                    // must start at an even offset.
                    bytes.insert(0, 0);
                }
                let place = chunk.clone().unwrap_or(*container_end..*container_end);
                let old_length = place.end - place.start;
                let new_container_size = (u64::from(*container_size) + bytes.len() as u64)
                    .checked_sub(old_length)
                    .and_then(|size| u32::try_from(size).ok())
                    .ok_or("the file would be larger than its container format allows")?;
                Ok(vec![
                    Splice {
                        start: 4,
                        end: 8,
                        bytes: number(new_container_size).to_vec(),
                    },
                    Splice {
                        start: place.start,
                        end: place.end,
                        bytes,
                    },
                ])
            }
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
enum Document {
    Id3(Id3File),
    Mp4(Mp4File),
    Flac(FlacFile),
}

/// Whether `bytes` start with a plausible MPEG audio frame header.
fn is_mpeg_frame_header(bytes: &[u8]) -> bool {
    matches!(bytes, [0xFF, second, third, ..]
        if second & 0xE0 == 0xE0
            && second & 0x18 != 0x08
            && second & 0x06 != 0
            && third & 0xF0 != 0xF0
            && third & 0x0C != 0x0C)
}

fn has_mpeg_audio(file: &mut File, start: u64) -> Result<bool, ErrorSource> {
    let mut window = Vec::new();
    file.seek(SeekFrom::Start(start))?;
    file.take(MPEG_SEARCH_LENGTH).read_to_end(&mut window)?;
    Ok(window.windows(3).any(is_mpeg_frame_header))
}

impl Document {
    fn load(file: &mut File) -> Result<Self, ErrorSource> {
        let mut beginning = [0; PROBE_LENGTH];
        let mut read = 0;
        file.seek(SeekFrom::Start(0))?;
        while read < PROBE_LENGTH {
            match file.read(&mut beginning[read..])? {
                0 => break,
                count => read += count,
            }
        }
        let beginning = &beginning[..read];
        let unsupported = || "not an MP3, WAV, AIFF, M4A or FLAC file".into();
        if beginning.len() < PROBE_LENGTH {
            return Err(unsupported());
        }

        let after_id3 = flac::stream_start(beginning)?;
        let marker_after_id3 = read_exactly(file, after_id3, 4).unwrap_or_default();
        if marker_after_id3 == b"fLaC" {
            return Ok(Self::Flac(FlacFile::load(file, after_id3)?));
        }
        match (&beginning[..4], &beginning[8..12]) {
            (b"RIFF", b"WAVE") => Ok(Self::Id3(Id3File::in_chunk(file, beginning, true)?)),
            (b"FORM", b"AIFF" | b"AIFC") => {
                Ok(Self::Id3(Id3File::in_chunk(file, beginning, false)?))
            }
            (_, _) if &beginning[4..8] == b"ftyp" => Ok(Self::Mp4(Mp4File::load(file)?)),
            _ if has_mpeg_audio(file, after_id3)? => {
                Ok(Self::Id3(Id3File::at_start(file, beginning)?))
            }
            _ => Err(unsupported()),
        }
    }

    fn problem(&self) -> Option<&'static str> {
        match self {
            Self::Id3(file) => file.problem(),
            Self::Mp4(file) => file.problem(),
            Self::Flac(file) => file.problem(),
        }
    }

    /// The length of the area the tag occupies in the file: the ID3v2 tag,
    /// the `moov` atom, or all FLAC metadata blocks.
    fn tag_length(&self) -> u64 {
        match self {
            Self::Id3(file) => file.tag_length(),
            Self::Mp4(file) => file.moov_length(),
            Self::Flac(file) => file.metadata_length(),
        }
    }

    fn prefer_lengths(&mut self, tag: Option<u64>, file_length: Option<u64>) {
        match self {
            Self::Id3(file) => file.preferred_length = tag,
            Self::Mp4(file) => file.prefer_lengths(tag, file_length),
            Self::Flac(file) => file.prefer_metadata_length(tag),
        }
    }

    /// Every item of the tag, whole and in order.
    fn items(&self) -> Vec<Vec<u8>> {
        match self {
            Self::Id3(file) => file.tag.frames().to_vec(),
            Self::Mp4(file) => file.items().to_vec(),
            Self::Flac(file) => file.items(),
        }
    }

    /// The index among [`Document::items`] of the item `field` names.
    fn position(&self, field: &str) -> Result<Option<usize>, ErrorSource> {
        match self {
            Self::Id3(file) => Ok(file.tag.position(field)),
            Self::Mp4(file) => file.position(field),
            Self::Flac(file) => Ok(file.position(field)),
        }
    }

    fn raw(&self, field: &str) -> Result<Option<Vec<u8>>, ErrorSource> {
        Ok(match self {
            Self::Id3(file) => file.tag.raw(field).map(<[u8]>::to_vec),
            Self::Mp4(file) => file.raw(field)?.map(<[u8]>::to_vec),
            Self::Flac(file) => file.raw(field).map(<[u8]>::to_vec),
        })
    }

    fn get(&self, field: &str) -> Result<Option<String>, ErrorSource> {
        match self {
            Self::Id3(file) => file.tag.get(field),
            Self::Mp4(file) => file.get(field),
            Self::Flac(file) => Ok(file.get(field)),
        }
    }

    fn set(&mut self, field: &str, value: Option<&str>) -> Result<(), ErrorSource> {
        match self {
            Self::Id3(file) => file.tag.set(field, value),
            Self::Mp4(file) => file.set(field, value),
            Self::Flac(file) => {
                file.set(field, value);
                Ok(())
            }
        }
    }

    fn restore(&mut self, field: &str, raw: Option<&[u8]>) -> Result<(), ErrorSource> {
        match self {
            Self::Id3(file) => file.tag.restore(field, raw),
            Self::Mp4(file) => file.restore(field, raw),
            Self::Flac(file) => file.restore(field, raw),
        }
    }

    fn splices(&self) -> Result<Vec<Splice>, ErrorSource> {
        match self {
            Self::Id3(file) => file.splices(),
            Self::Mp4(file) => file.splices(),
            Self::Flac(file) => file.splices(),
        }
    }
}

/// The native tag of one file.
pub(super) struct TagFile {
    path: PathBuf,
    file_length: u64,
    /// The tag as it was loaded, to tell at the end of a write whether the
    /// file changed in the meantime.
    loaded: Document,
    document: Document,
    /// The items as they were loaded, with the index each touched field
    /// had among them.
    original_items: Vec<Vec<u8>>,
    touched: Vec<(String, Option<usize>)>,
}

impl TagFile {
    /// Reads the native tag of the file at `path`. A file with no tag yet
    /// loads as an empty one.
    pub(super) fn load(path: &Path, purpose: Purpose) -> Result<Self, EuphoriaError> {
        let mut file = File::open(path).map_err(|source| EuphoriaError::Open {
            path: path.to_path_buf(),
            source,
        })?;
        let document = Document::load(&mut file).map_err(|source| EuphoriaError::TagRead {
            path: path.to_path_buf(),
            source,
        })?;
        let metadata = file.metadata().map_err(|source| EuphoriaError::Open {
            path: path.to_path_buf(),
            source,
        })?;
        let problem = if has_several_links(&metadata) {
            Some(SEVERAL_LINKS)
        } else {
            document.problem()
        };
        if let (Purpose::Rewriting, Some(problem)) = (purpose, problem) {
            return Err(EuphoriaError::TagWrite {
                path: path.to_path_buf(),
                source: refusal(problem),
            });
        }
        Ok(Self {
            path: path.to_path_buf(),
            file_length: metadata.len(),
            original_items: document.items(),
            loaded: document.clone(),
            document,
            touched: Vec::new(),
        })
    }

    pub(super) fn format(&self) -> TagFormat {
        match self.document {
            Document::Id3(_) => TagFormat::Id3v2,
            Document::Mp4(_) => TagFormat::Mp4,
            Document::Flac(_) => TagFormat::VorbisComments,
        }
    }

    /// The text of `field`, or `None` when the tag does not have it.
    pub(super) fn get(&self, field: &str) -> Result<Option<String>, EuphoriaError> {
        self.document
            .get(field)
            .map_err(|source| self.read_error(source))
    }

    /// The whole item `field` names, as the bytes the file holds.
    pub(super) fn raw(&self, field: &str) -> Result<Option<Vec<u8>>, EuphoriaError> {
        self.document
            .raw(field)
            .map_err(|source| self.read_error(source))
    }

    /// The length of the area the tag occupies in the file.
    pub(super) fn tag_length(&self) -> u64 {
        self.document.tag_length()
    }

    /// The length of the file when it was loaded.
    pub(super) fn file_length(&self) -> u64 {
        self.file_length
    }

    /// Asks for the tag area and the file to get these lengths again when
    /// padding allows, which is what makes an undone file identical to
    /// the original.
    pub(super) fn prefer_lengths(&mut self, tag: Option<u64>, file_length: Option<u64>) {
        self.document.prefer_lengths(tag, file_length);
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

    fn touch(&mut self, field: &str) -> Result<(), ErrorSource> {
        if !self.touched.iter().any(|(touched, _)| touched == field) {
            let position = self.document.position(field)?;
            self.touched.push((field.to_string(), position));
        }
        Ok(())
    }

    /// Sets `field` to `value` in memory. A field that already exists
    /// keeps its place in the tag.
    pub(super) fn set(&mut self, field: &str, value: &str) -> Result<(), EuphoriaError> {
        self.touch(field)
            .and_then(|()| self.document.set(field, Some(value)))
            .map_err(|source| self.write_error(source))
    }

    /// Puts the exact bytes of an item back in memory, or removes the item
    /// when `raw` is `None`.
    pub(super) fn restore(&mut self, field: &str, raw: Option<&[u8]>) -> Result<(), EuphoriaError> {
        self.touch(field)
            .and_then(|()| self.document.restore(field, raw))
            .map_err(|source| self.write_error(source))
    }

    /// Removes `field` in memory.
    pub(super) fn remove(&mut self, field: &str) -> Result<(), EuphoriaError> {
        self.touch(field)
            .and_then(|()| self.document.set(field, None))
            .map_err(|source| self.write_error(source))
    }

    /// Writes the tag to the file.
    ///
    /// The new file is built next to the real one: the original with the
    /// tag's bytes exchanged and everything else copied. It is then read
    /// back and compared with the original, and takes the file's place
    /// only when nothing but the touched fields differs. A failed or
    /// refused write leaves the file as it was. A symbolic link is
    /// followed: the file it points to is rewritten and the link stays a
    /// link.
    pub(super) fn save(&self) -> Result<(), EuphoriaError> {
        self.save_checked()
            .map_err(|source| self.write_error(source))
    }

    fn save_checked(&self) -> Result<(), ErrorSource> {
        self.save_after(|_| Ok(()))
    }

    /// Builds the new file, runs `before_checks` on it, checks it, and puts
    /// it in place. `before_checks` does nothing outside tests, where it
    /// stands in for whatever could damage the new file or change the
    /// original while a write is under way.
    fn save_after(
        &self,
        before_checks: impl FnOnce(&mut File) -> std::io::Result<()>,
    ) -> Result<(), ErrorSource> {
        let target = std::fs::canonicalize(&self.path)?;
        let mut original = File::open(&target)?;
        let mut splices = self.document.splices()?;
        let mut replacement = Replacement::start(&target)?;
        let written = compose(&mut original, &mut splices, replacement.file())
            .and_then(|()| before_checks(replacement.file()))
            .map_err(ErrorSource::from)
            .and_then(|()| self.check(&mut original, &splices, replacement.file()))
            .and_then(|()| self.check_source_is_unchanged(&target));
        match written {
            Ok(()) => replacement.commit(),
            Err(error) => {
                // The write error is the one reported.
                let _ = replacement.discard();
                Err(error)
            }
        }
    }

    /// The new file was built from the tag as it was loaded. If another
    /// program has changed the file since, by editing it or by putting a
    /// new file in its place, replacing it now would undo that change, so
    /// the file at `target` is read once more, last thing before the
    /// replacement, and must hold the tag that was loaded.
    fn check_source_is_unchanged(&self, target: &Path) -> Result<(), ErrorSource> {
        let mut current = File::open(target)?;
        let is_unchanged = current.metadata()?.len() == self.file_length
            && Document::load(&mut current)? == self.loaded;
        if is_unchanged {
            Ok(())
        } else {
            Err(refusal(
                "another program changed it while its tags were being written",
            ))
        }
    }

    /// Compares the new file with the original, without relying on how the
    /// new file was put together: every byte outside the rewritten ranges,
    /// every untouched item, and the value of every touched field.
    fn check(
        &self,
        original: &mut File,
        splices: &[Splice],
        copy: &mut File,
    ) -> Result<(), ErrorSource> {
        if !carried_over(original, splices, copy)? {
            return Err(refusal("data outside the tag would change"));
        }
        let written = Document::load(copy)?;
        if written.problem().is_some() {
            return Err(refusal("the rewritten tag would not be readable"));
        }

        let without = |mut items: Vec<Vec<u8>>, mut positions: Vec<usize>| {
            positions.sort_unstable();
            for position in positions.into_iter().rev() {
                items.remove(position);
            }
            items
        };
        let original_positions = self
            .touched
            .iter()
            .filter_map(|(_, position)| *position)
            .collect();
        let mut written_positions = Vec::new();
        for (field, _) in &self.touched {
            written_positions.extend(written.position(field)?);
        }
        if without(self.original_items.clone(), original_positions)
            != without(written.items(), written_positions)
        {
            return Err(refusal(
                "tag data that was not meant to change would be lost or altered",
            ));
        }
        for (field, _) in &self.touched {
            if written.raw(field)? != self.document.raw(field)? {
                return Err(refusal("a field did not take the value written to it"));
            }
        }
        if let (Document::Mp4(loaded), Document::Mp4(written)) = (&self.document, &written) {
            loaded
                .check_rewrite(original, written, copy)
                .map_err(|error| -> ErrorSource {
                    format!("refusing to rewrite the file: {error}").into()
                })?;
        }
        Ok(())
    }

    fn read_error(&self, source: ErrorSource) -> EuphoriaError {
        EuphoriaError::TagRead {
            path: self.path.clone(),
            source,
        }
    }

    fn write_error(&self, source: ErrorSource) -> EuphoriaError {
        EuphoriaError::TagWrite {
            path: self.path.clone(),
            source,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    const KEY_FIELDS: [(&str, &str); 5] = [
        ("sine-440hz-mono.mp3", "TKEY"),
        ("sine-440hz-mono.wav", "TKEY"),
        ("sine-440hz-mono.aiff", "TKEY"),
        ("sine-440hz-mono.flac", "INITIALKEY"),
        ("sine-440hz-mono.m4a", "----:com.apple.iTunes:initialkey"),
    ];

    /// Writes a key to a copy of `fixture`, lets `tamper` change the new
    /// file, and returns what the check before the replacement says.
    fn check_after(
        fixture: &str,
        field: &str,
        tamper: impl Fn(&mut Vec<u8>),
    ) -> Result<(), ErrorSource> {
        static NEXT_DIRECTORY: std::sync::atomic::AtomicUsize =
            std::sync::atomic::AtomicUsize::new(0);
        let unique = NEXT_DIRECTORY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("euphoria-native-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(fixture);
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(fixture);
        std::fs::copy(source, &path).unwrap();

        let mut tag_file = TagFile::load(&path, Purpose::Rewriting).unwrap();
        tag_file.set(field, "8A").unwrap();
        let mut original = File::open(&path).unwrap();
        let mut splices = tag_file.document.splices().unwrap();
        let mut replacement = Replacement::start(&path).unwrap();
        compose(&mut original, &mut splices, replacement.file()).unwrap();

        let copy = replacement.file();
        let mut bytes = Vec::new();
        copy.rewind().unwrap();
        copy.read_to_end(&mut bytes).unwrap();
        tamper(&mut bytes);
        copy.set_len(0).unwrap();
        copy.rewind().unwrap();
        copy.write_all(&bytes).unwrap();

        let checked = tag_file.check(&mut original, &splices, replacement.file());
        replacement.discard().unwrap();
        std::fs::remove_dir_all(&directory).unwrap();
        checked
    }

    fn flip_last(bytes: &mut [u8], pattern: &[u8]) {
        let position = bytes
            .windows(pattern.len())
            .rposition(|window| window == pattern)
            .unwrap();
        bytes[position] ^= 0x01;
    }

    #[test]
    fn check_accepts_an_exact_write() {
        for (fixture, field) in KEY_FIELDS {
            check_after(fixture, field, |_| {}).unwrap();
        }
    }

    #[test]
    fn check_refuses_a_change_to_data_outside_the_plan() {
        for (fixture, field) in KEY_FIELDS {
            // Every fixture but the AIFF names its encoder, in a tag item
            // or in a chunk of its own.
            let untouched = if fixture.ends_with("aiff") {
                &b"SSND"[..]
            } else {
                &b"Lavf"[..]
            };
            let error =
                check_after(fixture, field, |bytes| flip_last(bytes, untouched)).unwrap_err();
            assert!(error.to_string().contains("refusing"), "{fixture}: {error}");
        }
    }

    #[test]
    fn check_refuses_a_field_that_did_not_take_its_value() {
        for (fixture, field) in KEY_FIELDS {
            let error = check_after(fixture, field, |bytes| flip_last(bytes, b"8A")).unwrap_err();
            assert!(
                error.to_string().contains("did not take the value"),
                "{fixture}: {error}"
            );
        }
    }

    #[test]
    fn check_refuses_a_change_to_the_audio() {
        for (fixture, field) in KEY_FIELDS {
            let tamper = |bytes: &mut Vec<u8>| {
                let middle = bytes.len() / 2;
                bytes[middle] ^= 0x01;
            };
            let error = check_after(fixture, field, tamper).unwrap_err();
            assert!(error.to_string().contains("refusing"), "{fixture}: {error}");
        }
    }

    /// A copy of `fixture` in a directory of its own.
    fn fixture_copy(fixture: &str, test: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "euphoria-native-{}-{test}-{fixture}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join(fixture);
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(fixture);
        std::fs::copy(source, &path).unwrap();
        path
    }

    fn directory_entries(path: &Path) -> Vec<std::ffi::OsString> {
        std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect()
    }

    #[test]
    fn damaged_replacement_is_discarded_and_the_original_is_untouched() {
        for (fixture, field) in KEY_FIELDS {
            let path = fixture_copy(fixture, "damaged");
            let original = std::fs::read(&path).unwrap();
            let mut tag_file = TagFile::load(&path, Purpose::Rewriting).unwrap();
            tag_file.set(field, "8A").unwrap();

            let error = tag_file
                .save_after(|copy| {
                    let middle = copy.metadata()?.len() / 2;
                    let mut byte = [0];
                    copy.seek(SeekFrom::Start(middle))?;
                    copy.read_exact(&mut byte)?;
                    copy.seek(SeekFrom::Start(middle))?;
                    copy.write_all(&[byte[0] ^ 0x01])
                })
                .unwrap_err();
            assert!(error.to_string().contains("refusing"), "{fixture}: {error}");
            assert!(std::fs::read(&path).unwrap() == original, "{fixture}");
            assert_eq!(directory_entries(&path), [fixture], "{fixture}");

            tag_file.save().unwrap();
            assert!(std::fs::read(&path).unwrap() != original, "{fixture}");
            std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn write_is_refused_when_another_program_changed_the_file_meanwhile() {
        for (fixture, field) in KEY_FIELDS {
            for replaces_the_file in [false, true] {
                let path = fixture_copy(fixture, &format!("raced-{replaces_the_file}"));
                let mut ours = TagFile::load(&path, Purpose::Rewriting).unwrap();
                ours.set(field, "8A").unwrap();

                // Another program writes the same field after our load,
                // either by putting a new file in place or by overwriting
                // the file where it is.
                let written_file = if replaces_the_file {
                    path.clone()
                } else {
                    let other = path.with_extension("other");
                    std::fs::copy(&path, &other).unwrap();
                    other
                };
                let mut theirs = TagFile::load(&written_file, Purpose::Rewriting).unwrap();
                theirs.set(field, "11B").unwrap();
                theirs.save().unwrap();
                if !replaces_the_file {
                    std::fs::write(&path, std::fs::read(&written_file).unwrap()).unwrap();
                    std::fs::remove_file(&written_file).unwrap();
                }
                let after_their_write = std::fs::read(&path).unwrap();

                let error = ours.save().unwrap_err();
                assert!(
                    error.chain().contains("another program changed it"),
                    "{fixture}: {}",
                    error.chain()
                );
                assert!(
                    std::fs::read(&path).unwrap() == after_their_write,
                    "{fixture}"
                );
                let read_back = TagFile::load(&path, Purpose::Reading).unwrap();
                assert_eq!(read_back.get(field).unwrap().as_deref(), Some("11B"));
                assert_eq!(directory_entries(&path), [fixture], "{fixture}");
                std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
            }
        }
    }

    #[test]
    fn change_made_during_the_write_itself_is_caught_before_the_replacement() {
        for (fixture, field) in KEY_FIELDS {
            let path = fixture_copy(fixture, "raced-late");
            let mut ours = TagFile::load(&path, Purpose::Rewriting).unwrap();
            ours.set(field, "8A").unwrap();
            let mut theirs = TagFile::load(&path, Purpose::Rewriting).unwrap();
            theirs.set(field, "11B").unwrap();

            // Their write lands after our new file is built.
            let error = ours
                .save_after(|_| theirs.save().map_err(std::io::Error::other))
                .unwrap_err();
            assert!(
                error.to_string().contains("another program changed it"),
                "{fixture}: {error}"
            );
            let read_back = TagFile::load(&path, Purpose::Reading).unwrap();
            assert_eq!(read_back.get(field).unwrap().as_deref(), Some("11B"));
            assert_eq!(directory_entries(&path), [fixture], "{fixture}");
            std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn check_refuses_a_lost_or_added_byte() {
        for (fixture, field) in KEY_FIELDS {
            check_after(fixture, field, |bytes| bytes.truncate(bytes.len() - 1)).unwrap_err();
            check_after(fixture, field, |bytes| bytes.push(0)).unwrap_err();
        }
    }
}
