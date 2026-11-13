// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads a rekordbox collection exported as XML, to find the tracks that
//! already have cues there, which an export must not touch.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::checks::{CheckError, Result};

const LOCATION_PREFIX: &str = "file://localhost";

/// The tracks of a collection that have at least one cue, memory cue, hot
/// cue or loop.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CuedTracks {
    /// The files on this computer the cued tracks are, each as rekordbox
    /// wrote its path and, when that differs, with every symbolic link in it
    /// followed, as the export writes paths.
    pub paths: HashSet<PathBuf>,
    /// The cued tracks that cannot be matched to a file on this computer:
    /// with no location, a location that is not a local file URL, or a path
    /// where no file is. Each is described by title, artist and location.
    pub unmatched: Vec<String>,
}

/// Reads the collection at `path`.
pub fn tracks_with_cues(path: &Path) -> Result<CuedTracks> {
    let text = std::fs::read_to_string(path).map_err(|error| {
        CheckError::new(
            format!("cannot read the rekordbox collection {}", path.display()),
            error,
        )
    })?;
    let cued = parse(&text).map_err(|error| {
        error.context(format!(
            "cannot read the rekordbox collection {}",
            path.display()
        ))
    })?;
    let mut tracks = CuedTracks::default();
    for track in cued {
        let found = track
            .path
            .as_ref()
            .and_then(|path| std::fs::canonicalize(path).ok());
        match (track.path, found) {
            (Some(listed), Some(resolved)) => {
                tracks.paths.insert(listed);
                tracks.paths.insert(resolved);
            }
            _ => tracks.unmatched.push(track.description),
        }
    }
    Ok(tracks)
}

/// A track of the collection with cues: its path when its location is a
/// local file URL, and how to name it to the user.
#[derive(Debug, PartialEq, Eq)]
struct CuedTrack {
    path: Option<PathBuf>,
    description: String,
}

fn parse(text: &str) -> Result<Vec<CuedTrack>> {
    let document = roxmltree::Document::parse(text)
        .map_err(|error| CheckError::new("it is not well formed XML", error))?;
    let root = document.root_element();
    let Some(collection) = root
        .has_tag_name("DJ_PLAYLISTS")
        .then(|| root.children().find(|node| node.has_tag_name("COLLECTION")))
        .flatten()
    else {
        return Err(CheckError::message(
            "it is not a rekordbox XML collection: it has no DJ_PLAYLISTS and COLLECTION",
        ));
    };
    let tracks = collection
        .children()
        .filter(|node| node.has_tag_name("TRACK"))
        .filter(|track| {
            track
                .children()
                .any(|node| node.has_tag_name("POSITION_MARK"))
        })
        .map(|track| {
            let location = track.attribute("Location");
            let description = format!(
                "{} by {} at {}",
                track.attribute("Name").unwrap_or("(no title)"),
                track.attribute("Artist").unwrap_or("(no artist)"),
                location.unwrap_or("(no location)"),
            );
            CuedTrack {
                path: location.and_then(location_path),
                description,
            }
        })
        .collect();
    Ok(tracks)
}

/// The path of a `file://localhost/` URL as rekordbox writes it, with its
/// percent escapes decoded: `/Users/dj/a.mp3` for a Mac location, and
/// `C:/Music/a.mp3` for a Windows one. `None` for any other URL.
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
    let without_slash = path.strip_prefix('/')?;
    let drive = without_slash.as_bytes();
    let has_drive_letter =
        drive.len() >= 3 && drive[0].is_ascii_alphabetic() && drive[1] == b':' && drive[2] == b'/';
    Some(PathBuf::from(if has_drive_letter {
        without_slash
    } else {
        &path
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLLECTION: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <PRODUCT Name="rekordbox" Version="7.2.19" Company="AlphaTheta"/>
  <COLLECTION Entries="5">
    <TRACK TrackID="1" Name="Cued" Artist="Someone" Location="file://localhost/Users/dj/Music/Caf%C3%A9%20Del%20Mar.mp3">
      <TEMPO Inizio="0.025" Bpm="128.00" Metro="4/4" Battito="1"/>
      <POSITION_MARK Name="" Type="0" Start="0.025" Num="-1"/>
    </TRACK>
    <TRACK TrackID="2" Name="Loop" Location="file://localhost/Users/dj/Music/a&amp;b.mp3">
      <POSITION_MARK Name="" Type="4" Start="10.0" End="12.0" Num="0"/>
    </TRACK>
    <TRACK TrackID="3" Name="Bare" Location="file://localhost/Users/dj/Music/bare.mp3"/>
    <TRACK TrackID="4" Name="Windows" Artist="Other" Location="file://localhost/C:/Music/w.mp3">
      <POSITION_MARK Name="" Type="0" Start="1.0" Num="1"/>
    </TRACK>
    <TRACK TrackID="5" Name="Nowhere" Artist="Lost">
      <POSITION_MARK Name="" Type="0" Start="1.0" Num="2"/>
    </TRACK>
  </COLLECTION>
</DJ_PLAYLISTS>"#;

    #[test]
    fn only_tracks_with_a_cue_or_a_loop_are_listed_with_their_paths_decoded() {
        let paths: Vec<Option<PathBuf>> = parse(COLLECTION)
            .unwrap()
            .into_iter()
            .map(|track| track.path)
            .collect();
        assert_eq!(
            paths,
            vec![
                Some(PathBuf::from("/Users/dj/Music/Café Del Mar.mp3")),
                Some(PathBuf::from("/Users/dj/Music/a&b.mp3")),
                Some(PathBuf::from("C:/Music/w.mp3")),
                None,
            ]
        );
    }

    #[test]
    fn cued_tracks_that_match_no_file_here_are_listed_by_title_artist_and_location() {
        let folder =
            std::env::temp_dir().join(format!("euphoria-collection-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let present = folder.join("present.mp3");
        std::fs::write(&present, b"").unwrap();
        let collection = folder.join("collection.xml");
        std::fs::write(
            &collection,
            format!(
                r#"<DJ_PLAYLISTS><COLLECTION>
                <TRACK Name="Here" Location="file://localhost{}"><POSITION_MARK/></TRACK>
                <TRACK Name="Gone" Artist="Someone" Location="file://localhost/nowhere/gone.mp3"><POSITION_MARK/></TRACK>
                <TRACK Name="Windows" Location="file://localhost/C:/Music/w.mp3"><POSITION_MARK/></TRACK>
                <TRACK Name="Streamed" Location="https://example.com/a"><POSITION_MARK/></TRACK>
                <TRACK Name="Nowhere"><POSITION_MARK/></TRACK>
                </COLLECTION></DJ_PLAYLISTS>"#,
                present.display()
            ),
        )
        .unwrap();

        let tracks = tracks_with_cues(&collection).unwrap();
        let resolved = std::fs::canonicalize(&present).unwrap();
        std::fs::remove_dir_all(&folder).unwrap();

        assert!(tracks.paths.contains(&present));
        assert!(tracks.paths.contains(&resolved));
        assert_eq!(
            tracks.unmatched,
            vec![
                "Gone by Someone at file://localhost/nowhere/gone.mp3".to_string(),
                "Windows by (no artist) at file://localhost/C:/Music/w.mp3".to_string(),
                "Streamed by (no artist) at https://example.com/a".to_string(),
                "Nowhere by (no artist) at (no location)".to_string(),
            ]
        );
    }

    #[test]
    fn a_file_that_is_not_a_collection_is_an_error() {
        assert!(parse("not xml").is_err());
        assert!(parse("<DJ_PLAYLISTS/>").is_err());
        assert!(parse("<PLAYLISTS><COLLECTION/></PLAYLISTS>").is_err());
    }

    #[test]
    fn locations_decode_percent_escapes_drive_letters_and_refuse_broken_ones() {
        assert_eq!(
            location_path("file://localhost/Volumes/My%20Disk/k%C3%AFt.wav"),
            Some(PathBuf::from("/Volumes/My Disk/kït.wav"))
        );
        assert_eq!(
            location_path("file://localhost/D:/DJ/My%20Track.flac"),
            Some(PathBuf::from("D:/DJ/My Track.flac"))
        );
        assert_eq!(location_path("file://localhost/a%2"), None);
        assert_eq!(location_path("file://localhost/a%zz"), None);
        assert_eq!(location_path("file:///a.wav"), None);
    }
}
