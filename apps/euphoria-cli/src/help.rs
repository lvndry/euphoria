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

pub fn virtualdj_export_examples() -> StyledStr {
    examples(
        "Examples",
        &[
            (
                "euphoria export virtualdj ~/Music/DJ --dry-run",
                "See what would change",
            ),
            (
                "euphoria export virtualdj ~/Music/DJ --beat-grid --cues",
                "Write grids and hot cues too",
            ),
        ],
        "",
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
