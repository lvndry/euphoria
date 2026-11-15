// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Where each track starts in a mix, as a CUE sheet, a tracklist and
//! chapter lines. A track starts where it is first heard.

use std::fmt::Write as _;

use super::timeline::MixTimeline;
use super::{MixFormat, RenderTrack};

/// CUE sheet frames per second.
const CUE_FRAMES_PER_SECOND: f64 = 75.0;

/// `seconds` as "m:ss", or "h:mm:ss" from an hour, rounded down.
pub fn format_mix_time(seconds: f64) -> String {
    let whole = seconds.max(0.0).floor() as u64;
    let hours = whole / 3600;
    let minutes = whole / 60 % 60;
    let seconds = whole % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn cue_time(seconds: f64) -> String {
    let frames = (seconds.max(0.0) * CUE_FRAMES_PER_SECOND).floor() as u64;
    let per_second = CUE_FRAMES_PER_SECOND as u64;
    format!(
        "{:02}:{:02}:{:02}",
        frames / per_second / 60,
        frames / per_second % 60,
        frames % per_second
    )
}

fn cue_text(text: &str) -> String {
    text.replace(['\r', '\n'], " ").replace('"', "'")
}

/// A CUE sheet for the mix written to the file `audio_file_name`, named
/// `title`, with one track per track of the set at the time it starts.
pub fn cue_sheet(
    title: &str,
    audio_file_name: &str,
    format: MixFormat,
    tracks: &[RenderTrack],
    timeline: &MixTimeline,
) -> String {
    let file_type = match format {
        MixFormat::Wav | MixFormat::Flac => "WAVE",
        MixFormat::Mp3 { .. } => "MP3",
    };
    let mut sheet = String::new();
    let _ = writeln!(sheet, "TITLE \"{}\"", cue_text(title));
    let _ = writeln!(sheet, "FILE \"{}\" {file_type}", cue_text(audio_file_name));
    for (index, (track, placement)) in tracks.iter().zip(&timeline.placements).enumerate() {
        let _ = writeln!(sheet, "  TRACK {:02} AUDIO", index + 1);
        let _ = writeln!(
            sheet,
            "    TITLE \"{}\"",
            cue_text(&track.title_or_file_name())
        );
        if let Some(artist) = &track.artist {
            let _ = writeln!(sheet, "    PERFORMER \"{}\"", cue_text(artist));
        }
        let _ = writeln!(sheet, "    INDEX 01 {}", cue_time(placement.start_seconds));
    }
    sheet
}

/// A numbered tracklist: the title, then one line per track with the time
/// it starts, then the length of the mix.
pub fn tracklist(title: &str, tracks: &[RenderTrack], timeline: &MixTimeline) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "{}", title.replace(['\r', '\n'], " "));
    let _ = writeln!(text);
    let width = tracks.len().to_string().len().max(2);
    for (index, (track, placement)) in tracks.iter().zip(&timeline.placements).enumerate() {
        let _ = writeln!(
            text,
            "{:0width$}. {} {}",
            index + 1,
            format_mix_time(placement.start_seconds),
            track.display_name().replace(['\r', '\n'], " "),
        );
    }
    let _ = writeln!(text);
    let _ = writeln!(text, "Length {}", format_mix_time(timeline.length_seconds));
    text
}

/// One line per track, its start time then its name, the way a video
/// description lists chapters. The first starts at 0:00.
pub fn chapters(tracks: &[RenderTrack], timeline: &MixTimeline) -> String {
    let mut text = String::new();
    for (track, placement) in tracks.iter().zip(&timeline.placements) {
        let _ = writeln!(
            text,
            "{} {}",
            format_mix_time(placement.start_seconds),
            track.display_name().replace(['\r', '\n'], " ")
        );
    }
    text
}
