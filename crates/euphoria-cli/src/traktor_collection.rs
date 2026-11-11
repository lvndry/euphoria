// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reads a Traktor collection (`collection.nml`), to find the tracks that
//! already have cues there, which an export must not touch.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::collection::CuedTracks;

/// The type of a `CUE_V2` that marks the beat grid.
const GRID_CUE_TYPE: &str = "4";

/// The name Traktor gives the hot cue it sets on the first beat of the grid
/// it analyzes.
const AUTOMATIC_CUE_NAME: &str = "AutoGrid";

/// Reads the collection at `path`. `startup_volume` is the name of the Mac
/// volume that holds paths outside `/Volumes`.
pub fn tracks_with_cues(path: &Path, startup_volume: &str) -> Result<CuedTracks> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read the Traktor collection {}", path.display()))?;
    let cued = parse(&text, startup_volume)
        .with_context(|| format!("cannot read the Traktor collection {}", path.display()))?;
    let mut tracks = CuedTracks::default();
    for (listed, description) in cued {
        let found = listed
            .as_ref()
            .and_then(|path| std::fs::canonicalize(path).ok());
        match (listed, found) {
            (Some(listed), Some(resolved)) => {
                tracks.paths.insert(listed);
                tracks.paths.insert(resolved);
            }
            _ => tracks.unmatched.push(description),
        }
    }
    Ok(tracks)
}

/// The entries of the collection with cues set by the user: each one's path
/// on this computer when its location names one, and how to name it.
fn parse(text: &str, startup_volume: &str) -> Result<Vec<(Option<PathBuf>, String)>> {
    let document = roxmltree::Document::parse(text).context("it is not well formed XML")?;
    let root = document.root_element();
    let Some(collection) = root
        .has_tag_name("NML")
        .then(|| root.children().find(|node| node.has_tag_name("COLLECTION")))
        .flatten()
    else {
        bail!("it is not a Traktor collection: it has no NML and COLLECTION");
    };
    let entries = collection
        .children()
        .filter(|node| node.has_tag_name("ENTRY"))
        .filter(|entry| has_cues_of_its_own(*entry))
        .map(|entry| {
            let location = entry.children().find(|node| node.has_tag_name("LOCATION"));
            let shown_location = location
                .map(|location| {
                    format!(
                        "{}{}{}",
                        location.attribute("VOLUME").unwrap_or_default(),
                        location.attribute("DIR").unwrap_or_default(),
                        location.attribute("FILE").unwrap_or_default()
                    )
                })
                .unwrap_or_else(|| "(no location)".to_string());
            let description = format!(
                "{} by {} at {shown_location}",
                entry.attribute("TITLE").unwrap_or("(no title)"),
                entry.attribute("ARTIST").unwrap_or("(no artist)"),
            );
            let path = location.and_then(|location| {
                local_path(
                    location.attribute("VOLUME")?,
                    location.attribute("DIR")?,
                    location.attribute("FILE")?,
                    startup_volume,
                )
            });
            (path, description)
        })
        .collect();
    Ok(entries)
}

/// True when the entry has a cue, hot cue or loop other than its grid
/// markers and the `AutoGrid` hot cue Traktor sets on a grid marker when it
/// analyzes a track.
fn has_cues_of_its_own(entry: roxmltree::Node<'_, '_>) -> bool {
    let cues: Vec<roxmltree::Node<'_, '_>> = entry
        .children()
        .filter(|node| node.has_tag_name("CUE_V2"))
        .collect();
    let grid_starts: Vec<&str> = cues
        .iter()
        .filter(|cue| cue.attribute("TYPE") == Some(GRID_CUE_TYPE))
        .filter_map(|cue| cue.attribute("START"))
        .collect();
    cues.iter().any(|cue| {
        let is_grid = cue.attribute("TYPE") == Some(GRID_CUE_TYPE);
        let is_automatic = cue.attribute("NAME") == Some(AUTOMATIC_CUE_NAME)
            && cue.attribute("TYPE") == Some("0")
            && cue
                .attribute("START")
                .is_some_and(|start| grid_starts.contains(&start));
        !is_grid && !is_automatic
    })
}

/// The path an NML location names on this computer. A drive letter such as
/// `C:` is a Windows drive. On a Mac, `startup_volume` is the root and any
/// other volume is under `/Volumes`.
fn local_path(volume: &str, directory: &str, file: &str, startup_volume: &str) -> Option<PathBuf> {
    if file.is_empty() {
        return None;
    }
    let folders: Vec<&str> = directory
        .split("/:")
        .filter(|folder| !folder.is_empty())
        .collect();
    let volume_bytes = volume.as_bytes();
    let mut path = if volume_bytes.len() == 2
        && volume_bytes[0].is_ascii_alphabetic()
        && volume_bytes[1] == b':'
    {
        format!("{volume}/")
    } else if volume == startup_volume || volume.is_empty() {
        String::from("/")
    } else {
        format!("/Volumes/{volume}/")
    };
    for folder in folders {
        path.push_str(folder);
        path.push('/');
    }
    path.push_str(file);
    Some(PathBuf::from(path))
}

/// The name of the volume a Mac starts from, which Traktor writes as the
/// volume of every path outside `/Volumes`: the entry of `/Volumes` that
/// leads to `/`. `Macintosh HD` when none does.
pub fn startup_volume_name() -> String {
    let root = Path::new("/");
    std::fs::read_dir("/Volumes")
        .into_iter()
        .flatten()
        .flatten()
        .find(|entry| {
            std::fs::canonicalize(entry.path()).is_ok_and(|resolved| resolved.as_path() == root)
        })
        .and_then(|entry| entry.file_name().into_string().ok())
        .unwrap_or_else(|| euphoria_core::export::traktor::DEFAULT_STARTUP_VOLUME.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shaped after the Traktor Pro 4 collection in traktor-nml-utils: an
    // analyzed track with only its grid marker and AutoGrid hot cue, one
    // with a hot cue of its own, one with a loop, and one on a Windows
    // drive.
    const COLLECTION: &str = r##"<?xml version='1.0' encoding='UTF-8'?>
<NML VERSION="20"><HEAD COMPANY="www.native-instruments.com" PROGRAM="Traktor Pro 4" />
<COLLECTION ENTRIES="4">
<ENTRY TITLE="Analyzed" ARTIST="One"><LOCATION DIR="/:Users/:dj/:Music/:" FILE="analyzed.mp3" VOLUME="Macintosh HD" VOLUMEID="68d949fd" />
<CUE_V2 NAME="AutoGrid" DISPL_ORDER="0" TYPE="4" START="242.841049" LEN="0.000000" REPEATS="-1" HOTCUE="-1"><GRID BPM="122.999985" />
</CUE_V2>
<CUE_V2 NAME="AutoGrid" DISPL_ORDER="0" TYPE="0" START="242.841049" LEN="0.000000" REPEATS="-1" HOTCUE="0" COLOR="#FFFFFF" />
</ENTRY>
<ENTRY TITLE="Cued"><LOCATION DIR="/:Sets/:" FILE="a&amp;b.mp3" VOLUME="USB" VOLUMEID="1" />
<CUE_V2 NAME="n.n." DISPL_ORDER="0" TYPE="0" START="13793.576168" LEN="0.000000" REPEATS="-1" HOTCUE="2" />
</ENTRY>
<ENTRY TITLE="Looped" ARTIST="Two"><LOCATION DIR="/:Music/:" FILE="loop.wav" VOLUME="X:" VOLUMEID="2" />
<CUE_V2 NAME="Loop" DISPL_ORDER="0" TYPE="5" START="1000.0" LEN="2000.0" REPEATS="-1" HOTCUE="-1" />
</ENTRY>
<ENTRY TITLE="Bare"><LOCATION DIR="/:" FILE="bare.mp3" VOLUME="Macintosh HD" VOLUMEID="1" />
</ENTRY>
</COLLECTION></NML>"##;

    #[test]
    fn only_entries_with_cues_of_their_own_are_listed_with_their_paths() {
        let entries = parse(COLLECTION, "Macintosh HD").unwrap();
        assert_eq!(
            entries,
            vec![
                (
                    Some(PathBuf::from("/Volumes/USB/Sets/a&b.mp3")),
                    "Cued by (no artist) at USB/:Sets/:a&b.mp3".to_string()
                ),
                (
                    Some(PathBuf::from("X:/Music/loop.wav")),
                    "Looped by Two at X:/:Music/:loop.wav".to_string()
                ),
            ]
        );
    }

    #[test]
    fn a_renamed_startup_volume_is_the_root() {
        assert_eq!(
            local_path("Studio", "/:Users/:dj/:", "a.mp3", "Studio"),
            Some(PathBuf::from("/Users/dj/a.mp3"))
        );
        assert_eq!(
            local_path("Macintosh HD", "/:Users/:dj/:", "a.mp3", "Studio"),
            Some(PathBuf::from("/Volumes/Macintosh HD/Users/dj/a.mp3"))
        );
        assert_eq!(local_path("C:", "/:", "", "Studio"), None);
    }

    #[test]
    fn a_file_that_is_not_a_collection_is_an_error() {
        assert!(parse("not xml", "Macintosh HD").is_err());
        assert!(parse("<NML/>", "Macintosh HD").is_err());
        assert!(parse("<DJ_PLAYLISTS><COLLECTION/></DJ_PLAYLISTS>", "Macintosh HD").is_err());
    }
}
