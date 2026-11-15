// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The artists an artist tag names, and how they are shown. Tags list
//! several artists with a NUL (ID3v2.4), a semicolon, or a slash with
//! spaces around it. A bare slash is kept, since it is part of names like
//! "AC/DC". The tag itself is never changed: this only shapes what is shown.

use std::borrow::Cow;

/// What goes between two artists when they are shown.
const SHOWN_SEPARATOR: &str = ", ";

/// The artists `artist` names, in tag order, each once, compared without
/// regard to case. A tag with no separator gives its text, trimmed.
pub fn artist_names(artist: &str) -> Vec<&str> {
    let mut names: Vec<&str> = Vec::new();
    for part in artist.split(['\0', ';']).flat_map(|part| part.split(" / ")) {
        let name = part.trim();
        let is_new = !names
            .iter()
            .any(|known| known.to_lowercase() == name.to_lowercase());
        if !name.is_empty() && is_new {
            names.push(name);
        }
    }
    names
}

/// The artist tag as it is shown: its artists joined by ", ", so that
/// "BlaQRhythm;Elo Mude" reads "BlaQRhythm, Elo Mude". A tag that names one
/// artist is returned as it is.
pub fn shown_artist(artist: &str) -> Cow<'_, str> {
    if !artist.contains(['\0', ';']) && !artist.contains(" / ") {
        return Cow::Borrowed(artist);
    }
    Cow::Owned(artist_names(artist).join(SHOWN_SEPARATOR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_semicolon_separates_artists() {
        assert_eq!(shown_artist("BlaQRhythm;Elo Mude"), "BlaQRhythm, Elo Mude");
        assert_eq!(
            artist_names("BlaQRhythm;Elo Mude"),
            ["BlaQRhythm", "Elo Mude"]
        );
    }

    #[test]
    fn spaces_after_a_semicolon_are_dropped() {
        assert_eq!(
            shown_artist("Ana Sol; Marine Layer"),
            "Ana Sol, Marine Layer"
        );
        assert_eq!(
            shown_artist(" Ana Sol ;  Marine Layer ;"),
            "Ana Sol, Marine Layer"
        );
    }

    #[test]
    fn a_nul_separates_artists() {
        assert_eq!(
            shown_artist("Ana Sol\0Marine Layer"),
            "Ana Sol, Marine Layer"
        );
        assert_eq!(shown_artist("Ana Sol\0"), "Ana Sol");
    }

    #[test]
    fn a_slash_inside_a_name_is_kept() {
        assert_eq!(shown_artist("AC/DC"), "AC/DC");
        assert_eq!(artist_names("AC/DC"), ["AC/DC"]);
        assert_eq!(shown_artist("AC/DC;Ana Sol"), "AC/DC, Ana Sol");
    }

    #[test]
    fn a_slash_with_spaces_separates_artists() {
        assert_eq!(
            shown_artist("Ana Sol / Marine Layer"),
            "Ana Sol, Marine Layer"
        );
    }

    #[test]
    fn an_artist_named_twice_is_shown_once() {
        assert_eq!(shown_artist("Ana Sol;Ana Sol"), "Ana Sol");
        assert_eq!(
            shown_artist("Ana Sol;ana sol;Marine Layer"),
            "Ana Sol, Marine Layer"
        );
        assert_eq!(
            shown_artist("Ana Sol\0Marine Layer\0Ana Sol"),
            "Ana Sol, Marine Layer"
        );
    }

    #[test]
    fn a_single_artist_is_shown_as_it_is() {
        assert!(matches!(shown_artist("Ana Sol"), Cow::Borrowed("Ana Sol")));
        assert_eq!(
            shown_artist("Ana Sol & Marine Layer"),
            "Ana Sol & Marine Layer"
        );
        assert_eq!(
            shown_artist("Ana Sol, Marine Layer"),
            "Ana Sol, Marine Layer"
        );
        assert!(artist_names("").is_empty());
    }
}
