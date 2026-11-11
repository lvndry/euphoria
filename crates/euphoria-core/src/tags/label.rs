// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The key and energy label, such as "8A - Energy 7", that goes into a
//! comment or grouping next to the text the field already holds.
//!
//! The label has the form Mixed In Key users know, so that their DJ
//! software sorts and searches it the way they are used to. That form
//! carries no marker, so a label written earlier is recognised by its
//! shape alone, and the rules for that are strict: text that merely looks
//! similar is the user's own and is kept.

use crate::key::Key;

use super::KeyNotation;

/// What separates the parts of a label, and the label from the text next
/// to it.
const SEPARATOR: &str = " - ";
const ENERGY_WORD: &str = "Energy ";
const HIGHEST_CAMELOT_NUMBER: u32 = 12;
const HIGHEST_ENERGY: u32 = 10;

/// Where the key and energy label is written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum LabelPlacement {
    /// In front of the comment: "8A - Energy 7 - the comment".
    #[default]
    CommentStart,
    /// After the comment: "the comment - 8A - Energy 7".
    CommentEnd,
    /// In front of the grouping.
    Grouping,
    /// Nowhere. The comment and the grouping are left alone.
    None,
}

/// The label for a key and an energy rating, or `None` when there is
/// neither.
pub(super) fn label_text(
    key: Option<Key>,
    energy: Option<u8>,
    notation: KeyNotation,
) -> Option<String> {
    let key = key.map(|key| notation.spell(key));
    let energy = energy.map(|energy| format!("{ENERGY_WORD}{energy}"));
    match (key, energy) {
        (Some(key), Some(energy)) => Some(format!("{key}{SEPARATOR}{energy}")),
        (key, energy) => key.or(energy),
    }
}

/// The length of a whole number from 1 to `highest` at the start of
/// `text`, written without a leading zero.
fn number_length(text: &str, highest: u32) -> Option<usize> {
    let length = text.bytes().take_while(u8::is_ascii_digit).count();
    let digits = &text[..length];
    let is_in_range = !digits.starts_with('0')
        && digits
            .parse::<u32>()
            .is_ok_and(|number| (1..=highest).contains(&number));
    is_in_range.then_some(length)
}

/// The length of a Camelot code such as "8A" or "12B" at the start of
/// `text`.
fn camelot_length(text: &str) -> Option<usize> {
    let digits = number_length(text, HIGHEST_CAMELOT_NUMBER)?;
    matches!(text.as_bytes().get(digits), Some(b'A' | b'B')).then_some(digits + 1)
}

/// The length of a short musical key name such as "Am", "F#" or "Bbm" at
/// the start of `text`.
fn musical_key_length(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if !matches!(bytes.first(), Some(b'A'..=b'G')) {
        return None;
    }
    let mut length = 1;
    if matches!(bytes.get(length), Some(b'b' | b'#')) {
        length += 1;
    }
    if bytes.get(length) == Some(&b'm') {
        length += 1;
    }
    Some(length)
}

/// The length of an Open Key code such as "1m" or "12d" at the start of
/// `text`.
fn open_key_length(text: &str) -> Option<usize> {
    let digits = number_length(text, HIGHEST_CAMELOT_NUMBER)?;
    matches!(text.as_bytes().get(digits), Some(b'm' | b'd')).then_some(digits + 1)
}

/// The length of "Energy 7" at the start of `text`.
fn energy_length(text: &str) -> Option<usize> {
    let number = text.strip_prefix(ENERGY_WORD)?;
    Some(ENERGY_WORD.len() + number_length(number, HIGHEST_ENERGY)?)
}

/// The length of a label at the start of `text`, when `text` starts with
/// one.
///
/// A label is a Camelot code, an energy rating, or a key followed by an
/// energy rating. A musical key name or an Open Key code on its own is not
/// taken for a label, because a comment such as "A - side" or "1m - long
/// intro" starts the same way.
fn label_length(text: &str) -> Option<usize> {
    let key_and_energy = |key_length: usize| {
        let energy = text[key_length..].strip_prefix(SEPARATOR)?;
        Some(key_length + SEPARATOR.len() + energy_length(energy)?)
    };
    let camelot = camelot_length(text);
    camelot
        .and_then(key_and_energy)
        .or(camelot)
        .or_else(|| musical_key_length(text).and_then(key_and_energy))
        .or_else(|| open_key_length(text).and_then(key_and_energy))
        .or_else(|| energy_length(text))
}

/// Which labels are looked for at one end of a field.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Recognise {
    /// Every label, and `new_label` whatever its shape. This is for the
    /// end the label is written at, so that writing twice changes nothing.
    Any,
    /// Only labels with an energy rating. This is for the other end, where
    /// a label is found only if the placement was changed between writes,
    /// and where a bare Camelot code is more likely the user's own text.
    WithEnergy,
}

fn is_whole_label(candidate: &str, new_label: &str, recognise: Recognise) -> bool {
    match recognise {
        Recognise::Any => {
            candidate == new_label || label_length(candidate) == Some(candidate.len())
        }
        Recognise::WithEnergy => {
            candidate.contains(ENERGY_WORD) && label_length(candidate) == Some(candidate.len())
        }
    }
}

/// `text` without the label it starts with. The label must be followed by
/// the separator or by nothing.
fn without_leading_label<'a>(text: &'a str, new_label: &str, recognise: Recognise) -> &'a str {
    let lengths = [
        label_length(text),
        text.starts_with(new_label).then_some(new_label.len()),
    ];
    for length in lengths.into_iter().flatten() {
        if !is_whole_label(&text[..length], new_label, recognise) {
            continue;
        }
        let rest = &text[length..];
        if rest.is_empty() {
            return rest;
        }
        if let Some(own_text) = rest.strip_prefix(SEPARATOR) {
            return own_text;
        }
    }
    text
}

/// `text` without the label it ends with. The label must follow the
/// separator or be all of `text`.
fn without_trailing_label<'a>(text: &'a str, new_label: &str, recognise: Recognise) -> &'a str {
    if is_whole_label(text, new_label, recognise) {
        return "";
    }
    // The first separator that has a label after it starts the longest
    // label the text can end with.
    text.match_indices(SEPARATOR)
        .find(|(index, _)| is_whole_label(&text[index + SEPARATOR.len()..], new_label, recognise))
        .map_or(text, |(index, _)| &text[..index])
}

/// The new contents of a field that holds `existing`: the user's own text
/// with `label` in front of it or after it. A label that an earlier write
/// left there is taken out first, so the field never holds two.
pub(super) fn with_label(label: &str, existing: Option<&str>, at_end: bool) -> String {
    let existing = existing.unwrap_or_default();
    let (leading, trailing) = if at_end {
        (Recognise::WithEnergy, Recognise::Any)
    } else {
        (Recognise::Any, Recognise::WithEnergy)
    };
    let own_text = without_trailing_label(
        without_leading_label(existing, label, leading),
        label,
        trailing,
    );
    match (own_text.is_empty(), at_end) {
        (true, _) => label.to_string(),
        (false, false) => format!("{label}{SEPARATOR}{own_text}"),
        (false, true) => format!("{own_text}{SEPARATOR}{label}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::Mode;

    #[test]
    fn open_key_labels_are_recognised_only_with_an_energy_rating() {
        assert_eq!(label_length("1m - Energy 7"), Some(13));
        assert_eq!(label_length("12d - Energy 10 - intro"), Some(15));
        assert_eq!(label_length("1m - long intro"), None);
        assert_eq!(label_length("13m - Energy 7"), None);
        assert_eq!(label_length("01m - Energy 7"), None);
        assert_eq!(
            with_label("8A - Energy 7", Some("1m - Energy 5 - warm up"), false),
            "8A - Energy 7 - warm up"
        );
        assert_eq!(
            with_label("1m - Energy 7", Some("1m - long intro"), false),
            "1m - Energy 7 - 1m - long intro"
        );
    }

    #[test]
    fn the_open_key_label_spells_the_key_as_traktor_does() {
        let a_minor = Some(Key::new(9, Mode::Minor));
        let c_major = Some(Key::new(0, Mode::Major));
        assert_eq!(
            label_text(a_minor, Some(7), KeyNotation::OpenKey).as_deref(),
            Some("1m - Energy 7")
        );
        assert_eq!(
            label_text(c_major, None, KeyNotation::OpenKey).as_deref(),
            Some("1d")
        );
    }

    const URL: &str = "https://example.com/track/123?ref=a-b - c";

    #[test]
    fn label_names_the_key_and_the_energy() {
        let a_minor = Some(Key::new(9, Mode::Minor));
        assert_eq!(
            label_text(a_minor, Some(7), KeyNotation::Camelot).as_deref(),
            Some("8A - Energy 7")
        );
        assert_eq!(
            label_text(a_minor, Some(10), KeyNotation::Musical).as_deref(),
            Some("Am - Energy 10")
        );
        assert_eq!(
            label_text(a_minor, None, KeyNotation::Camelot).as_deref(),
            Some("8A")
        );
        assert_eq!(
            label_text(None, Some(3), KeyNotation::Camelot).as_deref(),
            Some("Energy 3")
        );
        assert_eq!(label_text(None, None, KeyNotation::Camelot), None);
    }

    #[test]
    fn label_goes_in_front_of_the_existing_text_or_after_it() {
        assert_eq!(with_label("8A - Energy 7", None, false), "8A - Energy 7");
        assert_eq!(with_label("8A - Energy 7", Some(""), true), "8A - Energy 7");
        assert_eq!(
            with_label("8A - Energy 7", Some(URL), false),
            format!("8A - Energy 7 - {URL}")
        );
        assert_eq!(
            with_label("8A - Energy 7", Some(URL), true),
            format!("{URL} - 8A - Energy 7")
        );
    }

    #[test]
    fn writing_twice_never_stacks_labels() {
        for at_end in [false, true] {
            for label in [
                "8A - Energy 7",
                "8A",
                "Energy 7",
                "Am - Energy 7",
                "Am",
                "F#",
            ] {
                for existing in [None, Some(URL), Some("warm up")] {
                    let once = with_label(label, existing, at_end);
                    let twice = with_label(label, Some(&once), at_end);
                    assert_eq!(twice, once, "{label:?} on {existing:?}");
                }
            }
        }
    }

    #[test]
    fn changed_label_replaces_the_earlier_one() {
        let labels = [
            "8A - Energy 7",
            "12B - Energy 10",
            "1A",
            "Energy 1",
            "Gbm - Energy 4",
            "C - Energy 9",
        ];
        for at_end in [false, true] {
            for earlier in labels {
                for later in labels {
                    let first = with_label(earlier, Some(URL), at_end);
                    assert_eq!(
                        with_label(later, Some(&first), at_end),
                        with_label(later, Some(URL), at_end),
                        "{earlier:?} then {later:?}"
                    );
                    let alone = with_label(earlier, None, at_end);
                    assert_eq!(with_label(later, Some(&alone), at_end), later);
                }
            }
        }
    }

    #[test]
    fn moving_the_label_to_the_other_end_takes_it_from_the_first() {
        let in_front = with_label("8A - Energy 7", Some(URL), false);
        assert_eq!(
            with_label("9A - Energy 6", Some(&in_front), true),
            format!("{URL} - 9A - Energy 6")
        );
        let at_the_end = with_label("8A - Energy 7", Some(URL), true);
        assert_eq!(
            with_label("9A - Energy 6", Some(&at_the_end), false),
            format!("9A - Energy 6 - {URL}")
        );
    }

    #[test]
    fn text_that_only_resembles_a_label_is_kept() {
        let look_alikes = [
            "8A-side remix",
            "8Ax - intro",
            "8A: intro",
            "13A - intro",
            "0A - intro",
            "08A - intro",
            "8C - intro",
            "8a - intro",
            "A - side",
            "Am - my favourite",
            "E - Energy drink",
            "Energy 11 - intro",
            "Energy 0",
            "Energy 7up",
            "Energy - intro",
            "energy 7 - intro",
            "High Energy 7",
            "great 8A - Energy 7 tune",
            " 8A - Energy 7 - intro",
        ];
        for text in look_alikes {
            assert_eq!(
                with_label("9A - Energy 6", Some(text), false),
                format!("9A - Energy 6 - {text}"),
                "{text:?}"
            );
        }
        // At the far end a bare Camelot code is the user's own text.
        assert_eq!(
            with_label("9A - Energy 6", Some("peak time - 8A"), false),
            "9A - Energy 6 - peak time - 8A"
        );
        assert_eq!(
            with_label("9A - Energy 6", Some("8A - peak time"), true),
            "8A - peak time - 9A - Energy 6"
        );
        let trailing_look_alikes = ["intro - A", "intro - Am", "intro 8A", "side - 13B"];
        for text in trailing_look_alikes {
            assert_eq!(
                with_label("9A - Energy 6", Some(text), true),
                format!("{text} - 9A - Energy 6"),
                "{text:?}"
            );
        }
    }

    #[test]
    fn label_of_another_program_in_the_same_form_is_replaced() {
        assert_eq!(
            with_label("9A - Energy 6", Some("8A - Energy 7 - warm up"), false),
            "9A - Energy 6 - warm up"
        );
        assert_eq!(
            with_label("9A - Energy 6", Some("4B"), false),
            "9A - Energy 6"
        );
    }
}
