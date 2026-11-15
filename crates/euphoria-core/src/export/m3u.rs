// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The extended M3U playlist format in UTF-8, read by nearly every music
//! and DJ program: Serato, Traktor, Engine DJ, VirtualDJ, Mixxx, music
//! players and more.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use super::rekordbox::{ExistingFile, write_file};
use crate::error::EuphoriaError;

/// One track of an M3U8 playlist.
#[derive(Debug, Clone, PartialEq)]
pub struct M3uTrack {
    /// The audio file, written as an absolute path.
    pub path: PathBuf,
    /// The length of the track, when known.
    pub duration_seconds: Option<f32>,
    /// What a program shows for the track, such as "Artist - Title".
    pub display_name: String,
}

/// The text of an M3U8 playlist called `name` holding `tracks` in order.
///
/// Line breaks in the name and the display names become spaces, so each
/// entry keeps to its two lines.
pub fn render_m3u8(name: &str, tracks: &[M3uTrack]) -> String {
    let one_line = |text: &str| text.replace(['\r', '\n'], " ");
    let mut document = String::from("#EXTM3U\n");
    let _ = writeln!(document, "#PLAYLIST:{}", one_line(name));
    for track in tracks {
        let seconds = track
            .duration_seconds
            .map_or(-1, |seconds| seconds.round() as i64);
        let _ = writeln!(
            document,
            "#EXTINF:{seconds},{}",
            one_line(&track.display_name)
        );
        let _ = writeln!(document, "{}", track.path.display());
    }
    document
}

/// Writes the M3U8 playlist `name` to `destination`, in one step as
/// [`write_xml_file`](super::rekordbox::write_xml_file) does.
///
/// # Errors
///
/// Returns [`EuphoriaError::Export`] when a track path is not absolute, is
/// not valid UTF-8 or holds a line break, which the format cannot carry, [`EuphoriaError::ExportDestinationExists`] when the file
/// exists and `existing` is [`ExistingFile::Keep`], and
/// [`EuphoriaError::ExportFile`] when it cannot be written.
pub fn write_m3u8_file(
    destination: &Path,
    name: &str,
    tracks: &[M3uTrack],
    existing: ExistingFile,
) -> Result<(), EuphoriaError> {
    if let Some(track) = tracks.iter().find(|track| match track.path.to_str() {
        Some(text) => !track.path.is_absolute() || text.contains(['\r', '\n']),
        None => true,
    }) {
        return Err(EuphoriaError::Export {
            path: track.path.clone(),
            problem: crate::error::ExportProblem::NotAbsolutePath,
        });
    }
    write_file(destination, render_m3u8(name, tracks).as_bytes(), existing)
}

/// The audio files an M3U or M3U8 playlist lists, in order. Lines that
/// start with `#` are comments; a relative path is read from the folder of
/// the playlist, and a `file://` address as the path it names.
pub fn parse_m3u(text: &str, playlist_folder: &Path) -> Vec<PathBuf> {
    text.trim_start_matches('\u{feff}')
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let line = line
                .strip_prefix("file://")
                .map_or_else(|| line.to_string(), percent_decoded);
            let path = PathBuf::from(line);
            if path.is_absolute() {
                path
            } else {
                playlist_folder.join(path)
            }
        })
        .collect()
}

/// Reads the playlist at `path` with [`parse_m3u`].
pub fn read_m3u_file(path: &Path) -> std::io::Result<Vec<PathBuf>> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let folder = path.parent().unwrap_or(Path::new(""));
    Ok(parse_m3u(&text, folder))
}

fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let escaped = (bytes[index] == b'%')
            .then(|| bytes.get(index + 1..index + 3))
            .flatten()
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match escaped {
            Some(byte) => {
                decoded.push(byte);
                index += 3;
            }
            None => {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_playlist_is_read_back_in_order_from_its_folder() {
        let text = "\u{feff}#EXTM3U\n#EXTINF:302,Artist - Opener\n/Music/Opener.mp3\n\n\
                    sub/Second.flac\r\nfile:///Music/Caf%C3%A9%20closing.flac\n";
        assert_eq!(
            parse_m3u(text, Path::new("/Sets")),
            vec![
                PathBuf::from("/Music/Opener.mp3"),
                PathBuf::from("/Sets/sub/Second.flac"),
                PathBuf::from("/Music/Caf\u{e9} closing.flac"),
            ]
        );
    }

    #[test]
    fn a_playlist_lists_its_tracks_in_order_with_their_length_and_name() {
        let tracks = [
            M3uTrack {
                path: PathBuf::from("/Music/Opener.mp3"),
                duration_seconds: Some(301.6),
                display_name: "Artist - Opener".to_string(),
            },
            M3uTrack {
                path: PathBuf::from("/Music/Caf\u{e9} closing.flac"),
                duration_seconds: None,
                display_name: "Closing\ntrack".to_string(),
            },
        ];
        assert_eq!(
            render_m3u8("Peak\ntime", &tracks),
            "#EXTM3U\n#PLAYLIST:Peak time\n#EXTINF:302,Artist - Opener\n/Music/Opener.mp3\n\
             #EXTINF:-1,Closing track\n/Music/Caf\u{e9} closing.flac\n"
        );
    }

    #[test]
    fn a_path_the_format_cannot_carry_is_refused_before_anything_is_written() {
        let destination =
            std::env::temp_dir().join(format!("euphoria-m3u-relative-{}.m3u8", std::process::id()));
        for path in ["relative.mp3", "/Music/two\nlines.mp3"] {
            let tracks = [M3uTrack {
                path: PathBuf::from(path),
                duration_seconds: None,
                display_name: "refused".to_string(),
            }];
            assert!(write_m3u8_file(&destination, "Set", &tracks, ExistingFile::Replace).is_err());
            assert!(!destination.exists());
        }
    }
}
