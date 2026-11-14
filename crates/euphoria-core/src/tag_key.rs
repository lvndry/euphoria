// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The key a file's tags already carry, compared with the key Euphoria
//! detects. Reading only: nothing here changes a file.

use crate::key::Key;

/// The key field of a file's tags: the text as written, and the key it
/// names when Euphoria can read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagKey {
    /// The text of the field, trimmed. Never empty.
    pub text: String,
    /// The key the text names, or `None` when the text is not a key in a
    /// notation Euphoria reads.
    pub key: Option<Key>,
}

impl TagKey {
    /// The key field holding `text`, or `None` when the text is empty or
    /// only white space.
    pub fn from_text(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        Some(Self {
            text: text.to_string(),
            key: Key::parse_file_key(text),
        })
    }

    /// How this key field compares with `detected`.
    pub fn compare(&self, detected: Key) -> TagKeyAgreement {
        match self.key {
            None => TagKeyAgreement::Unreadable,
            Some(tagged) => match KeyDifference::between(tagged, detected) {
                None => TagKeyAgreement::Same,
                Some(difference) => TagKeyAgreement::Differs(difference),
            },
        }
    }
}

/// How the key in a file's tags compares with the detected key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TagKeyAgreement {
    /// The tags name the detected key, in whatever notation.
    Same,
    /// The tags name another key.
    Differs(KeyDifference),
    /// The tags hold text that is not a key Euphoria reads.
    Unreadable,
}

impl TagKeyAgreement {
    /// True when the tags name a key other than the detected one.
    pub fn is_conflict(self) -> bool {
        matches!(self, TagKeyAgreement::Differs(_))
    }

    /// A stable name for scripts: "same", "unreadable", or the name of the
    /// difference.
    pub fn as_str(self) -> &'static str {
        match self {
            TagKeyAgreement::Same => "same",
            TagKeyAgreement::Differs(difference) => difference.as_str(),
            TagKeyAgreement::Unreadable => "unreadable",
        }
    }
}

/// How two different keys relate, named for the mistakes key detectors
/// make, so a DJ can tell which of two readings to trust.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyDifference {
    /// The same mode a fifth apart, one step on the Camelot wheel, such as
    /// 8A and 9A.
    Fifth,
    /// The relative major or minor, the same Camelot number with the other
    /// letter, such as 8A and 8B.
    Relative,
    /// The same home note in the other mode, such as A minor and A major.
    Parallel,
    /// Any other pair of keys.
    Other,
}

impl KeyDifference {
    /// Every difference, from the closest keys to the furthest.
    pub const ALL: [KeyDifference; 4] = [
        KeyDifference::Fifth,
        KeyDifference::Relative,
        KeyDifference::Parallel,
        KeyDifference::Other,
    ];

    /// How `tagged` differs from `detected`, or `None` when they are the
    /// same key.
    pub fn between(tagged: Key, detected: Key) -> Option<Self> {
        if tagged == detected {
            return None;
        }
        if tagged.mode() == detected.mode() {
            let steps = (12 + tagged.camelot_number() - detected.camelot_number()) % 12;
            return Some(if steps == 1 || steps == 11 {
                KeyDifference::Fifth
            } else {
                KeyDifference::Other
            });
        }
        if tagged.camelot_number() == detected.camelot_number() {
            return Some(KeyDifference::Relative);
        }
        if tagged.tonic() == detected.tonic() {
            return Some(KeyDifference::Parallel);
        }
        Some(KeyDifference::Other)
    }

    /// A stable name for scripts: "fifth", "relative", "parallel" or
    /// "other".
    pub fn as_str(self) -> &'static str {
        match self {
            KeyDifference::Fifth => "fifth",
            KeyDifference::Relative => "relative",
            KeyDifference::Parallel => "parallel",
            KeyDifference::Other => "other",
        }
    }

    /// A few words to show next to the key from the tags.
    pub fn label(self) -> &'static str {
        match self {
            KeyDifference::Fifth => "a fifth apart",
            KeyDifference::Relative => "relative key",
            KeyDifference::Parallel => "parallel key",
            KeyDifference::Other => "different key",
        }
    }

    /// One sentence on which reading to trust.
    pub fn advice(self) -> &'static str {
        match self {
            KeyDifference::Fifth => {
                "The keys share six of their seven notes, a common mix-up between detectors. Both mix well with the same tracks, so check by ear only if the track matters."
            }
            KeyDifference::Relative => {
                "The keys share all seven notes and differ in their home note, the most common mix-up between detectors. Listen for whether the track sounds minor or major."
            }
            KeyDifference::Parallel => {
                "The keys share their home note and differ in mode. Listen for whether the track sounds minor or major before you trust either."
            }
            KeyDifference::Other => {
                "The keys share few notes, so one reading is wrong or the track changes key. Check by ear before you mix on it."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::Mode;

    fn key(text: &str) -> Key {
        text.parse().unwrap()
    }

    #[test]
    fn differences_are_named_for_the_usual_mix_ups() {
        let a_minor = key("A minor");
        let expectations = [
            ("A minor", None),
            ("E minor", Some(KeyDifference::Fifth)),
            ("D minor", Some(KeyDifference::Fifth)),
            ("C major", Some(KeyDifference::Relative)),
            ("A major", Some(KeyDifference::Parallel)),
            ("B minor", Some(KeyDifference::Other)),
            ("G major", Some(KeyDifference::Other)),
            ("Bb minor", Some(KeyDifference::Other)),
        ];
        for (tagged, expected) in expectations {
            assert_eq!(
                KeyDifference::between(key(tagged), a_minor),
                expected,
                "{tagged}"
            );
        }
    }

    #[test]
    fn the_difference_is_the_same_whichever_key_comes_first() {
        for tonic in 0..12 {
            for mode in [Mode::Major, Mode::Minor] {
                let first = Key::new(tonic, mode);
                for other_tonic in 0..12 {
                    for other_mode in [Mode::Major, Mode::Minor] {
                        let second = Key::new(other_tonic, other_mode);
                        assert_eq!(
                            KeyDifference::between(first, second),
                            KeyDifference::between(second, first),
                            "{first} {second}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn an_empty_field_is_no_tag_key() {
        assert_eq!(TagKey::from_text(""), None);
        assert_eq!(TagKey::from_text(" \t "), None);
    }

    #[test]
    fn unreadable_text_is_kept_and_compared_as_unreadable() {
        let tag_key = TagKey::from_text("  banger!! ").unwrap();
        assert_eq!(tag_key.text, "banger!!");
        assert_eq!(tag_key.key, None);
        assert_eq!(tag_key.compare(key("A minor")), TagKeyAgreement::Unreadable);
        assert!(!tag_key.compare(key("A minor")).is_conflict());
    }

    #[test]
    fn a_readable_field_is_compared_in_any_notation() {
        let detected = key("A minor");
        for text in ["8A", "Am", "1m", "A minor", "8A - Energy 7"] {
            assert_eq!(
                TagKey::from_text(text).unwrap().compare(detected),
                TagKeyAgreement::Same,
                "{text}"
            );
        }
        let relative = TagKey::from_text("8B").unwrap().compare(detected);
        assert_eq!(relative, TagKeyAgreement::Differs(KeyDifference::Relative));
        assert!(relative.is_conflict());
        assert_eq!(relative.as_str(), "relative");
    }
}
