// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The examples at the end of each help page, styled like the headings clap
//! writes, so that the help reads like a short guide.

use clap::builder::StyledStr;
use clap::builder::styling::Styles;

/// A heading, then one example per line: the command, then what it does
/// in a quieter style when there is room for it.
fn examples(heading: &str, lines: &[(&str, &str)], closing: &str) -> StyledStr {
    let styles = Styles::default();
    let header = styles.get_header();
    let literal = styles.get_literal();
    let command_width = lines
        .iter()
        .map(|(command, _)| command.chars().count())
        .max()
        .unwrap_or(0);
    let mut text = format!("{header}{heading}:{header:#}\n");
    for (command, purpose) in lines {
        let padding = " ".repeat(command_width - command.chars().count());
        text.push_str(&format!(
            "  {literal}{command}{literal:#}{padding}  {purpose}\n"
        ));
    }
    if !closing.is_empty() {
        text.push('\n');
        text.push_str(closing);
    }
    StyledStr::from(text.trim_end().to_string())
}

pub fn main_examples() -> StyledStr {
    examples(
        "Examples",
        &[
            ("euphoria analyze ~/Music/DJ", "A table of every track"),
            ("euphoria analyze track.mp3", "One track in detail"),
            (
                "euphoria analyze --needs-review ~/Music/DJ",
                "Only the tracks worth checking by ear",
            ),
            (
                "euphoria tags write --dry-run ~/Music/DJ",
                "Preview the tags it would write",
            ),
            (
                "euphoria export rekordbox ~/Music/DJ -o euphoria.xml",
                "A file for rekordbox",
            ),
            (
                "euphoria set build ~/Music/DJ --curve journey",
                "A set that follows an energy curve",
            ),
        ],
        "Analyses are saved, so the second run over a folder is instant.\n\
         Run `euphoria <COMMAND> --help` for the options of a command.\n\
         Guides: https://github.com/lvndry/euphoria/tree/main/docs",
    )
}

#[cfg(feature = "stems")]
pub fn stems_examples() -> StyledStr {
    examples(
        "Examples",
        &[
            (
                "euphoria stems track.mp3 --output stems",
                "stems/vocals.wav, drums.wav, bass.wav and other.wav",
            ),
            (
                "euphoria stems track.mp3 -o stems --force",
                "The same, replacing earlier files",
            ),
        ],
        "Stems are 16 bit stereo WAV files at 44.1 kHz. Separation is never perfect: \
         expect a little bleed between stems. See docs/concepts/stems.md.",
    )
}

pub fn analyze_examples() -> StyledStr {
    examples(
        "Examples",
        &[
            ("euphoria analyze ~/Music/DJ", "A table of every track"),
            (
                "euphoria analyze --detail ~/Music/DJ/new",
                "Every track in detail",
            ),
            (
                "euphoria analyze --quality weak,reencoded ~/Music/DJ",
                "The files worth replacing",
            ),
            (
                "euphoria analyze --json ~/Music/DJ > library.json",
                "Everything, for a script",
            ),
        ],
        "In the table, ●●● is high confidence, ●●○ medium and ●○○ low: worth checking by ear.",
    )
}

pub fn tags_examples() -> StyledStr {
    examples(
        "Examples",
        &[
            (
                "euphoria tags write --dry-run ~/Music/DJ",
                "Show what would change, write nothing",
            ),
            (
                "euphoria tags write --preset rekordbox ~/Music/DJ",
                "Write the fields rekordbox reads",
            ),
            (
                "euphoria tags undo --list",
                "See the writes that can be undone",
            ),
            (
                "euphoria tags undo --dry-run",
                "See the files the next undo restores",
            ),
            (
                "euphoria tags undo",
                "Put back the tags the last write replaced",
            ),
        ],
        "",
    )
}

pub fn export_examples() -> StyledStr {
    examples(
        "Examples",
        &[(
            "euphoria export rekordbox ~/Music/DJ -o euphoria.xml",
            "A playlist per folder",
        )],
        "",
    )
}

pub fn corrections_examples() -> StyledStr {
    examples(
        "Examples",
        &[
            (
                "euphoria corrections export -o corrections.jsonl",
                "Every correction, named by content hash",
            ),
            (
                "euphoria corrections export --include-paths",
                "With file paths, for use on this computer",
            ),
        ],
        "",
    )
}

pub fn shift_examples() -> StyledStr {
    examples(
        "Examples",
        &[
            (
                "euphoria shift -s 2 track.flac -o 'track (+2).flac'",
                "Two semitones up",
            ),
            (
                "euphoria shift -s -1 track.mp3 -o 'track (-1).wav'",
                "One semitone down, as WAV",
            ),
        ],
        "The copy is tagged with the new key, and the tempo stays the same.",
    )
}

pub fn prepare_examples() -> StyledStr {
    examples(
        "Examples",
        &[
            (
                "euphoria prepare ~/Music/Friday --dry-run",
                "Show what would change, write nothing",
            ),
            (
                "euphoria prepare ~/Music/Friday --out ~/Music/Prepared",
                "Bring every track to -9 LUFS",
            ),
            (
                "euphoria prepare set/*.flac --loudness -11 --true-peak -1.5 --tune",
                "A quieter target, with tuning correction",
            ),
        ],
        "Copies are named like the originals plus \" (prepared)\"; originals are never changed.",
    )
}

pub fn traktor_export_examples() -> StyledStr {
    examples(
        "Examples",
        &[(
            "euphoria export traktor ~/Music/DJ -o euphoria.nml",
            "A playlist per folder",
        )],
        "",
    )
}

pub fn engine_export_examples() -> StyledStr {
    examples(
        "Examples",
        &[
            (
                "euphoria export engine ~/Music/DJ --dry-run",
                "Check the library, write nothing",
            ),
            (
                "euphoria export engine ~/Music/DJ --beat-grid --cues",
                "Write grids and hot cues too",
            ),
            (
                "euphoria export engine ~/Music/DJ --yes",
                "Write without asking first",
            ),
        ],
        "",
    )
}

pub fn set_examples() -> StyledStr {
    examples(
        "Examples",
        &[
            (
                "euphoria set build ~/Music/DJ --curve 4,6,8,9,7,5 --tracks 20",
                "Twenty tracks that rise, peak and land",
            ),
            (
                "euphoria set build \"Friday\" --curve peak-time --minutes 90",
                "Ninety minutes from a playlist",
            ),
            (
                "euphoria set build ~/Music/DJ --first opener.flac --m3u set.m3u8",
                "Open with a track, save the set",
            ),
        ],
        "Curve presets: warm-up, peak-time and journey.",
    )
}

pub fn live_examples() -> StyledStr {
    examples(
        "Examples",
        &[
            ("euphoria live", "The key and tempo of everything playing"),
            (
                "euphoria live --source rekordbox",
                "Only the audio of rekordbox",
            ),
            (
                "euphoria live --json --interval 1",
                "One JSON object per second, for an overlay",
            ),
        ],
        "Ctrl-C stops it. The audio is heard in memory only.",
    )
}

pub fn next_examples() -> StyledStr {
    examples(
        "Examples",
        &[
            (
                "euphoria next playing.flac",
                "The best tracks to play after it, from its folder",
            ),
            (
                "euphoria next playing.flac --from ~/Music/DJ --lift --limit 5",
                "Five tracks that lift the energy",
            ),
            (
                "euphoria next playing.flac --bpm 122-128 --energy 6- --json",
                "Only tracks from 122 to 128 BPM with energy 6 or more, as JSON",
            ),
        ],
        "Rows: key, tempo, how the tempo moves, how the key moves and the energy change.",
    )
}

pub fn plan_examples() -> StyledStr {
    examples(
        "Examples",
        &[
            (
                "euphoria plan playing.flac next.flac",
                "The best windows to mix the first into the second",
            ),
            (
                "euphoria plan playing.flac next.flac --json",
                "Every lane, clash and window as JSON",
            ),
        ],
        "Run euphoria stems on both tracks first so vocals and bass can be told apart.",
    )
}

pub fn virtualdj_export_examples() -> StyledStr {
    examples(
        "Examples",
        &[
            (
                "euphoria export virtualdj ~/Music/DJ --dry-run",
                "See what would change",
            ),
            (
                "euphoria export virtualdj ~/Music/DJ --yes",
                "Write without asking first",
            ),
            (
                "euphoria export virtualdj ~/Music/DJ --beat-grid --cues",
                "Write grids and hot cues too",
            ),
        ],
        "",
    )
}

pub fn completions_examples() -> StyledStr {
    examples(
        "Examples",
        &[
            (
                "euphoria completions zsh > ~/.zfunc/_euphoria",
                "Install for zsh, with ~/.zfunc in fpath",
            ),
            (
                "euphoria completions bash > ~/.local/share/bash-completion/completions/euphoria",
                "Install for bash",
            ),
            (
                "euphoria completions fish > ~/.config/fish/completions/euphoria.fish",
                "Install for fish",
            ),
        ],
        "Homebrew installs them for you.",
    )
}

pub fn cache_examples() -> StyledStr {
    examples(
        "Examples",
        &[
            ("euphoria cache path", "Where the saved analyses are"),
            ("euphoria cache clear", "Analyze everything again next time"),
        ],
        "",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples_line_up_under_a_heading() {
        let text = examples("Examples", &[("a", "first"), ("abc", "second")], "").to_string();
        assert_eq!(text, "Examples:\n  a    first\n  abc  second");
    }
}
