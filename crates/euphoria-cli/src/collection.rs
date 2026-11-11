// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads a rekordbox collection exported as XML, to find the tracks that
//! already have cues there, which an export must not replace.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

const LOCATION_PREFIX: &str = "file://localhost";

/// The paths of the tracks in the collection at `path` that have at least
/// one cue, memory cue, hot cue or loop. Each path is listed as rekordbox
/// wrote it and, when the file exists, also with every symbolic link in it
/// followed, as the export writes paths.
pub fn tracks_with_cues(path: &Path) -> Result<HashSet<PathBuf>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read the rekordbox collection {}", path.display()))?;
    let listed = parse(&text)
        .with_context(|| format!("cannot read the rekordbox collection {}", path.display()))?;
    let mut paths = HashSet::with_capacity(listed.len());
    for track in listed {
        if let Ok(resolved) = std::fs::canonicalize(&track) {
            paths.insert(resolved);
        }
        paths.insert(track);
    }
    Ok(paths)
}

fn parse(text: &str) -> Result<Vec<PathBuf>> {
    let document = roxmltree::Document::parse(text).context("it is not well formed XML")?;
    let root = document.root_element();
    let Some(collection) = root
        .has_tag_name("DJ_PLAYLISTS")
        .then(|| root.children().find(|node| node.has_tag_name("COLLECTION")))
        .flatten()
    else {
        bail!("it is not a rekordbox XML collection: it has no DJ_PLAYLISTS and COLLECTION");
    };
    let mut paths = Vec::new();
    for track in collection
        .children()
        .filter(|node| node.has_tag_name("TRACK"))
    {
        if !track
            .children()
            .any(|node| node.has_tag_name("POSITION_MARK"))
        {
            continue;
        }
        let Some(location) = track.attribute("Location") else {
            continue;
        };
        paths.push(location_path(location).with_context(|| {
            format!("the location {location:?} is not a file:// URL of a local file")
        })?);
    }
    Ok(paths)
}

/// The path of a `file://localhost/` URL as rekordbox writes it, with its
/// percent escapes decoded.
fn location_path(location: &str) -> Option<PathBuf> {
    let encoded = location.strip_prefix(LOCATION_PREFIX)?;
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut rest = encoded.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        if byte == b'%' {
            let hex = std::str::from_utf8(after.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &after[2..];
        } else {
            bytes.push(byte);
            rest = after;
        }
    }
    let path = String::from_utf8(bytes).ok()?;
    path.starts_with('/').then(|| PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLLECTION: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <PRODUCT Name="rekordbox" Version="7.2.19" Company="AlphaTheta"/>
  <COLLECTION Entries="3">
    <TRACK TrackID="1" Name="Cued" Location="file://localhost/Users/dj/Music/Caf%C3%A9%20Del%20Mar.mp3">
      <TEMPO Inizio="0.025" Bpm="128.00" Metro="4/4" Battito="1"/>
      <POSITION_MARK Name="" Type="0" Start="0.025" Num="-1"/>
    </TRACK>
    <TRACK TrackID="2" Name="Loop" Location="file://localhost/Users/dj/Music/a&amp;b.mp3">
      <POSITION_MARK Name="" Type="4" Start="10.0" End="12.0" Num="0"/>
    </TRACK>
    <TRACK TrackID="3" Name="Bare" Location="file://localhost/Users/dj/Music/bare.mp3"/>
  </COLLECTION>
</DJ_PLAYLISTS>"#;

    #[test]
    fn only_tracks_with_a_cue_or_a_loop_are_listed_with_their_paths_decoded() {
        assert_eq!(
            parse(COLLECTION).unwrap(),
            vec![
                PathBuf::from("/Users/dj/Music/Café Del Mar.mp3"),
                PathBuf::from("/Users/dj/Music/a&b.mp3"),
            ]
        );
    }

    #[test]
    fn a_file_that_is_not_a_collection_is_an_error() {
        assert!(parse("not xml").is_err());
        assert!(parse("<DJ_PLAYLISTS/>").is_err());
        assert!(parse("<PLAYLISTS><COLLECTION/></PLAYLISTS>").is_err());
        let bad_location = r#"<DJ_PLAYLISTS><COLLECTION>
            <TRACK Location="http://example.com/a.mp3"><POSITION_MARK/></TRACK>
        </COLLECTION></DJ_PLAYLISTS>"#;
        assert!(parse(bad_location).is_err());
    }

    #[test]
    fn locations_decode_percent_escapes_and_refuse_broken_ones() {
        assert_eq!(
            location_path("file://localhost/Volumes/My%20Disk/k%C3%AFt.wav"),
            Some(PathBuf::from("/Volumes/My Disk/kït.wav"))
        );
        assert_eq!(location_path("file://localhost/a%2"), None);
        assert_eq!(location_path("file://localhost/a%zz"), None);
        assert_eq!(location_path("file:///a.wav"), None);
    }
}
