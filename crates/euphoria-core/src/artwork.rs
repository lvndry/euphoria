// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The cover art of a track, read the way Finder shows it: the picture
//! embedded in the file's tags (the front cover when there are several), or
//! else a `cover`, `folder`, `front` or `albumart` image in the same folder.
//! Nothing here writes to an audio file.

use std::fs;
use std::path::{Path, PathBuf};

use lofty::config::ParseOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::PictureType;
use lofty::probe::Probe;

/// The largest image taken from beside a track. A cover is a few hundred
/// kilobytes; anything beyond this is not a cover.
const LARGEST_FOLDER_IMAGE_BYTES: u64 = 32 * 1024 * 1024;

/// The names, without extension and in lower case, of the images that are
/// taken as the cover of a folder, in order of preference.
const FOLDER_IMAGE_NAMES: [&str; 4] = ["cover", "folder", "front", "albumart"];

/// The extensions, in lower case, of the images that can be a cover.
const FOLDER_IMAGE_EXTENSIONS: [&str; 3] = ["jpg", "jpeg", "png"];

/// Where a cover was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtworkSource {
    /// In the tags of the audio file.
    Embedded,
    /// In an image file in the folder of the audio file.
    FolderImage(PathBuf),
}

/// The encoded picture of a cover, as stored: JPEG or PNG bytes, not pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artwork {
    /// The bytes of the image file.
    pub bytes: Vec<u8>,
    /// Where they came from.
    pub source: ArtworkSource,
}

/// The cover of the track at `path`: the picture embedded in it when it has
/// one, else the image beside it. `None` when there is neither.
pub fn read_artwork(path: &Path) -> Option<Artwork> {
    if let Some(bytes) = read_embedded_artwork(path) {
        return Some(Artwork {
            bytes,
            source: ArtworkSource::Embedded,
        });
    }
    let image = folder_artwork_path(path)?;
    let bytes = fs::read(&image).ok()?;
    Some(Artwork {
        bytes,
        source: ArtworkSource::FolderImage(image),
    })
}

/// The picture embedded in the tags of the file at `path`: ID3 attached
/// pictures in MP3, AIFF and WAV, `covr` in MP4, M4A and AAC, and the
/// pictures of FLAC and Ogg. The front cover is preferred; with none, the
/// first picture. `None` when the file has no picture or cannot be read.
pub fn read_embedded_artwork(path: &Path) -> Option<Vec<u8>> {
    let tagged = Probe::open(path)
        .ok()?
        .options(ParseOptions::new().read_properties(false))
        .guess_file_type()
        .ok()?
        .read()
        .ok()?;
    let front = tagged.tags().iter().find_map(|tag| {
        tag.pictures()
            .iter()
            .find(|picture| picture.pic_type() == PictureType::CoverFront)
    });
    let picture = front.or_else(|| tagged.tags().iter().find_map(|tag| tag.pictures().first()))?;
    let bytes = picture.data();
    (!bytes.is_empty()).then(|| bytes.to_vec())
}

/// The image in the folder of the file at `path` that serves as its cover:
/// `cover`, `folder`, `front` or `albumart`, as a JPEG or PNG, in any letter
/// case, in that order. `None` when there is none.
pub fn folder_artwork_path(path: &Path) -> Option<PathBuf> {
    let folder = path.parent()?;
    let mut candidates: Vec<(usize, PathBuf)> = fs::read_dir(folder)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let candidate = entry.path();
            let stem = candidate.file_stem()?.to_str()?.to_lowercase();
            let extension = candidate.extension()?.to_str()?.to_lowercase();
            let rank = FOLDER_IMAGE_NAMES.iter().position(|name| *name == stem)?;
            let is_image = FOLDER_IMAGE_EXTENSIONS.contains(&extension.as_str());
            let size = entry.metadata().ok()?.len();
            (is_image && size > 0 && size <= LARGEST_FOLDER_IMAGE_BYTES)
                .then_some((rank, candidate))
        })
        .collect();
    candidates.sort();
    candidates
        .into_iter()
        .next()
        .map(|(_, candidate)| candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_has_no_artwork() {
        assert!(read_artwork(Path::new("/nonexistent/track.mp3")).is_none());
    }
}
