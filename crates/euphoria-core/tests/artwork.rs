// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reading the cover of a track from copies of the fixtures: the picture
//! embedded in each format, the front cover when there are several, and the
//! image beside the file when there is no picture inside it.

use std::fs;
use std::path::{Path, PathBuf};

use euphoria_core::artwork::{ArtworkSource, folder_artwork_path, read_artwork};
use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::probe::Probe;
use lofty::tag::{Tag, TagExt};

const FRONT: &[u8] = b"\x89PNG-front-cover";
const BACK: &[u8] = b"\x89PNG-back-cover";

fn scratch_folder(test: &str) -> PathBuf {
    let folder = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("artwork-{test}"));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    folder
}

fn copy_fixture(fixture: &str, folder: &Path) -> PathBuf {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(fixture);
    let path = folder.join(fixture);
    fs::copy(source, &path).unwrap();
    path
}

fn picture(bytes: &[u8], kind: PictureType) -> Picture {
    Picture::unchecked(bytes.to_vec())
        .pic_type(kind)
        .mime_type(MimeType::Png)
        .build()
}

fn embed(path: &Path, pictures: Vec<Picture>) {
    let mut tagged = Probe::open(path).unwrap().read().unwrap();
    let tag_type = tagged.primary_tag_type();
    if tagged.primary_tag().is_none() {
        tagged.insert_tag(Tag::new(tag_type));
    }
    let tag = tagged.primary_tag_mut().unwrap();
    for picture in pictures {
        tag.push_picture(picture);
    }
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

#[test]
fn reads_the_embedded_cover_of_every_format() {
    let folder = scratch_folder("formats");
    for fixture in [
        "sine-440hz-mono.mp3",
        "sine-440hz-mono.wav",
        "sine-440hz-mono.aiff",
        "sine-440hz-mono.m4a",
        "sine-440hz-mono-alac.m4a",
        "sine-440hz-mono.flac",
        "sine-440hz-mono.ogg",
    ] {
        let path = copy_fixture(fixture, &folder);
        assert!(read_artwork(&path).is_none(), "{fixture} starts bare");
        embed(&path, vec![picture(FRONT, PictureType::CoverFront)]);
        let artwork = read_artwork(&path).unwrap_or_else(|| panic!("{fixture} has a cover"));
        assert_eq!(artwork.bytes, FRONT, "{fixture}");
        assert_eq!(artwork.source, ArtworkSource::Embedded, "{fixture}");
    }
}

#[test]
fn prefers_the_front_cover_when_there_are_several_pictures() {
    let folder = scratch_folder("front");
    for fixture in ["sine-440hz-mono.mp3", "sine-440hz-mono.flac"] {
        let path = copy_fixture(fixture, &folder);
        embed(
            &path,
            vec![
                picture(BACK, PictureType::CoverBack),
                picture(FRONT, PictureType::CoverFront),
            ],
        );
        assert_eq!(read_artwork(&path).unwrap().bytes, FRONT, "{fixture}");
    }
}

#[test]
fn takes_the_first_picture_when_none_is_the_front_cover() {
    let folder = scratch_folder("first");
    let path = copy_fixture("sine-440hz-mono.flac", &folder);
    embed(&path, vec![picture(BACK, PictureType::CoverBack)]);
    assert_eq!(read_artwork(&path).unwrap().bytes, BACK);
}

#[test]
fn falls_back_to_the_image_beside_the_file() {
    let folder = scratch_folder("folder");
    let path = copy_fixture("sine-440hz-mono.mp3", &folder);
    fs::write(folder.join("notes.jpg"), b"not a cover").unwrap();
    assert!(read_artwork(&path).is_none());

    fs::write(folder.join("Folder.JPG"), b"folder-image").unwrap();
    let artwork = read_artwork(&path).unwrap();
    assert_eq!(artwork.bytes, b"folder-image");
    assert_eq!(
        artwork.source,
        ArtworkSource::FolderImage(folder.join("Folder.JPG"))
    );

    fs::write(folder.join("cover.png"), b"cover-image").unwrap();
    assert_eq!(read_artwork(&path).unwrap().bytes, b"cover-image");
    assert_eq!(folder_artwork_path(&path), Some(folder.join("cover.png")));
}

#[test]
fn the_embedded_picture_wins_over_the_folder_image() {
    let folder = scratch_folder("embedded-wins");
    let path = copy_fixture("sine-440hz-mono.flac", &folder);
    fs::write(folder.join("cover.jpg"), b"folder-image").unwrap();
    embed(&path, vec![picture(FRONT, PictureType::CoverFront)]);
    assert_eq!(read_artwork(&path).unwrap().bytes, FRONT);
}

#[test]
fn ignores_an_empty_folder_image() {
    let folder = scratch_folder("empty");
    let path = copy_fixture("sine-440hz-mono.mp3", &folder);
    fs::write(folder.join("cover.jpg"), b"").unwrap();
    assert!(read_artwork(&path).is_none());
}

#[test]
fn reading_leaves_the_audio_file_untouched() {
    let folder = scratch_folder("untouched");
    let path = copy_fixture("sine-440hz-mono.mp3", &folder);
    embed(&path, vec![picture(FRONT, PictureType::CoverFront)]);
    let before = fs::read(&path).unwrap();
    let _ = read_artwork(&path);
    assert_eq!(fs::read(&path).unwrap(), before);
}
