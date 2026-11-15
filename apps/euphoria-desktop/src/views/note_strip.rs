// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The 12 notes the key analysis heard in the selected track, as a strip of
//! bars with the notes of the key lit, so a doubtful key can be checked by
//! eye.

use euphoria_core::{Key, PITCH_CLASS_COUNT, note_names, notes_in_key};
use gpui::component::tooltip::Tooltip;
use gpui::prelude::*;
use gpui::{AnyElement, Hsla, TestSupportExt, div, px};

use crate::camelot::CamelotPosition;
use crate::library::Track;
use crate::tag_settings::NotationChoice;
use crate::theme::AppTheme;
use crate::views::{GAP_TIGHT, LABEL_TEXT_SIZE, RADIUS_SMALL};

/// Height of the tallest bar.
const BAR_AREA_HEIGHT: f32 = 28.0;

/// The shortest a bar is drawn, so a note barely heard still shows.
const SHORTEST_BAR: f32 = 2.0;

/// How much of the colour of a note outside the key shows.
const OUTSIDE_KEY_OPACITY: f32 = 0.35;

/// What hovering the strip says.
pub(crate) const NOTES_EXPLANATION: &str = "How strongly each note sounds in the track, \
     every octave together. The notes of the key are lit: they should be the tallest. \
     A minor key and its relative major share the same notes, so one is easily heard as the other.";

/// What shows for a track analyzed before notes were measured.
pub(crate) const NOT_MEASURED: &str = "Analyze this track again to see its notes.";

/// One note of the strip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct NoteBar {
    /// The note's name, spelled as the key writes it.
    pub(crate) name: &'static str,
    /// How strongly the note sounds, from 0 to 1.
    pub(crate) level: f32,
    /// Whether the note belongs to the key.
    pub(crate) in_key: bool,
}

/// The 12 bars of `profile`, C first, with the notes of `key` lit.
pub(crate) fn note_bars(profile: [f32; PITCH_CLASS_COUNT], key: Option<Key>) -> Vec<NoteBar> {
    let names = note_names(key);
    let in_key = key.map(notes_in_key).unwrap_or([false; PITCH_CLASS_COUNT]);
    (0..PITCH_CLASS_COUNT)
        .map(|pitch_class| NoteBar {
            name: names[pitch_class],
            level: profile[pitch_class].clamp(0.0, 1.0),
            in_key: in_key[pitch_class],
        })
        .collect()
}

/// The strip for `track`, or one line asking to analyze it again when its
/// analysis has a key but no notes. Nothing for a track without a key.
pub(crate) fn note_strip(
    track: &Track,
    theme: &AppTheme,
    notation: NotationChoice,
) -> Option<AnyElement> {
    let analysis = track.analysis()?;
    analysis.key?;
    let palette = &theme.palette;
    let Some(profile) = analysis.pitch_class_profile else {
        return Some(
            div()
                .id("notes-not-measured")
                .text_size(LABEL_TEXT_SIZE)
                .text_color(palette.text_tertiary)
                .child(NOT_MEASURED)
                .test_support()
                .into_any_element(),
        );
    };
    let key = track.key();
    let lit: Hsla = key
        .map(|key| theme.key_colours(CamelotPosition::of(&key)).foreground)
        .unwrap_or(palette.text_secondary);
    let unlit = palette.text_tertiary.opacity(OUTSIDE_KEY_OPACITY);
    let heading = div()
        .flex()
        .items_center()
        .gap(GAP_TIGHT)
        .text_size(LABEL_TEXT_SIZE)
        .text_color(palette.text_tertiary)
        .child("Notes")
        .children(key.map(|key| {
            div()
                .id("notes-key")
                .text_color(lit)
                .child(format!("in {}", notation.spell(&key)))
                .test_support()
        }));
    let bars = note_bars(profile, key)
        .into_iter()
        .enumerate()
        .map(|(pitch_class, bar)| {
            let height = (bar.level * BAR_AREA_HEIGHT).max(SHORTEST_BAR);
            div()
                .id(("note-bar", pitch_class))
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(GAP_TIGHT)
                .child(
                    div()
                        .flex()
                        .items_end()
                        .w_full()
                        .h(px(BAR_AREA_HEIGHT))
                        .child(
                            div()
                                .id(("note-level", pitch_class))
                                .w_full()
                                .h(px(height))
                                .rounded(RADIUS_SMALL)
                                .bg(if bar.in_key { lit } else { unlit })
                                .test_support(),
                        ),
                )
                .child(
                    div()
                        .text_size(LABEL_TEXT_SIZE)
                        .text_color(if bar.in_key {
                            palette.text
                        } else {
                            palette.text_tertiary
                        })
                        .child(bar.name),
                )
                .test_support()
        });
    Some(
        div()
            .id("note-strip")
            .flex()
            .flex_col()
            .gap(GAP_TIGHT)
            .child(heading)
            .child(div().flex().gap(GAP_TIGHT).children(bars))
            .tooltip(|window, cx| Tooltip::new(NOTES_EXPLANATION).build(window, cx))
            .test_support()
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> Key {
        text.parse().unwrap()
    }

    #[test]
    fn the_notes_of_the_key_are_lit_and_spelled_as_the_key_writes_them() {
        let mut profile = [0.2; PITCH_CLASS_COUNT];
        profile[4] = 1.0;
        let bars = note_bars(profile, Some(key("E major")));
        let lit: Vec<&str> = bars
            .iter()
            .filter(|bar| bar.in_key)
            .map(|bar| bar.name)
            .collect();
        assert_eq!(lit, vec!["C#", "D#", "E", "F#", "G#", "A", "B"]);
        assert_eq!(bars[4].level, 1.0);
        assert_eq!(bars[0].level, 0.2);
    }

    #[test]
    fn without_a_key_no_note_is_lit() {
        let bars = note_bars([0.5; PITCH_CLASS_COUNT], None);
        assert!(bars.iter().all(|bar| !bar.in_key));
        assert_eq!(bars[10].name, "Bb");
    }
}
