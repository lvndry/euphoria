// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The key fields of the analyzed files' tags, read fresh on every run
//! since other programs change them, and compared with the detected key.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use euphoria_core::{
    Correction, FileResult, KeyDifference, ReviewReason, TagKey, TagKeyAgreement, TempoRange,
    TrackAnalysis,
};
use rayon::prelude::*;

use crate::batch::UserEditsByFile;

/// The key field of each analyzed file that has one, what the user set or
/// confirmed for each file in the desktop app, and the range tempos are
/// reported in, which decides whether a doubtful tempo is worth checking.
#[derive(Debug, Default)]
pub struct TagKeys {
    by_path: HashMap<PathBuf, TagKey>,
    corrections: HashMap<PathBuf, Correction>,
    tempo_range: TempoRange,
}

impl TagKeys {
    /// Reads the key field of every file that was analyzed. A file whose
    /// tags cannot be read counts as having none: its analysis is reported
    /// as usual. Tempos are taken to be reported in `tempo_range`.
    pub fn read(
        results: &[FileResult],
        user_edits: &UserEditsByFile,
        tempo_range: TempoRange,
    ) -> Self {
        let by_path = results
            .par_iter()
            .filter(|result| result.outcome.is_ok())
            .filter_map(|result| {
                let tag_key = euphoria_core::read_track_name(&result.path).ok()?.key?;
                Some((result.path.clone(), tag_key))
            })
            .collect();
        let corrections = user_edits
            .iter()
            .filter_map(|(path, edits)| Some((path.clone(), edits.as_ref().ok()?.correction)))
            .collect();
        Self {
            by_path,
            corrections,
            tempo_range,
        }
    }

    /// Key fields given directly, for tests.
    #[cfg(test)]
    pub fn from_texts(entries: &[(&Path, &str)]) -> Self {
        Self {
            by_path: entries
                .iter()
                .filter_map(|(path, text)| Some((path.to_path_buf(), TagKey::from_text(text)?)))
                .collect(),
            corrections: HashMap::new(),
            tempo_range: TempoRange::DEFAULT,
        }
    }

    /// The same, with tempos reported in `tempo_range`.
    #[cfg(test)]
    pub fn with_tempo_range(mut self, tempo_range: TempoRange) -> Self {
        self.tempo_range = tempo_range;
        self
    }

    /// The same, with the key of the file at `path` set by the user.
    #[cfg(test)]
    pub fn with_key_set_by_user(self, path: &Path) -> Self {
        self.with_correction(
            path,
            Correction::of_key(euphoria_core::Key::new(0, euphoria_core::Mode::Major)),
        )
    }

    /// The same, with `correction` kept for the file at `path`.
    #[cfg(test)]
    pub fn with_correction(mut self, path: &Path, correction: Correction) -> Self {
        self.corrections.insert(path.to_path_buf(), correction);
        self
    }

    /// What the user set or confirmed for the file at `path`.
    pub fn correction(&self, path: &Path) -> Correction {
        self.corrections.get(path).copied().unwrap_or_default()
    }

    /// Why the file at `path` is worth checking by ear, as the desktop app
    /// lists it: a key rated low, or a tempo whose plausible half or double
    /// the onsets back more strongly, that the user has not set or confirmed, or tags that name
    /// another key.
    pub fn review_reasons(&self, path: &Path, analysis: &TrackAnalysis) -> Vec<ReviewReason> {
        euphoria_core::review_reasons(
            analysis,
            &self.correction(path),
            self.get(path),
            self.tempo_range,
        )
    }

    /// True when the user set or confirmed the key or the tempo of the file
    /// at `path`.
    pub fn reviewed(&self, path: &Path) -> bool {
        euphoria_core::is_reviewed(&self.correction(path))
    }

    /// The key field of the file at `path`, when it has one.
    pub fn get(&self, path: &Path) -> Option<&TagKey> {
        self.by_path.get(path)
    }

    /// How the key field of the file at `path` compares with the key of
    /// `analysis`, or `None` when the file has no key field or no key was
    /// found.
    pub fn agreement(&self, path: &Path, analysis: &TrackAnalysis) -> Option<TagKeyAgreement> {
        Some(self.get(path)?.compare(analysis.key?))
    }

    /// How the key field compares with the detected key, or `None` when
    /// either is missing or the user set the key by hand, which settles it.
    pub fn compared(&self, path: &Path, analysis: &TrackAnalysis) -> Option<TagKeyAgreement> {
        if self.correction(path).key.is_some() {
            return None;
        }
        self.agreement(path, analysis)
    }

    /// The difference between the tags and the detected key that makes the
    /// track worth a look, or `None` when they agree, either is missing or
    /// the user set or confirmed the key.
    pub fn conflict(&self, path: &Path, analysis: &TrackAnalysis) -> Option<KeyDifference> {
        if self.correction(path).key_confirmed {
            return None;
        }
        match self.compared(path, analysis)? {
            TagKeyAgreement::Differs(difference) => Some(difference),
            TagKeyAgreement::Same | TagKeyAgreement::Unreadable => None,
        }
    }
}
