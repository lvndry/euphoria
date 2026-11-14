// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The key fields of the analyzed files' tags, read fresh on every run
//! since other programs change them, and compared with the detected key.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use euphoria_core::{FileResult, KeyDifference, TagKey, TagKeyAgreement, TrackAnalysis};
use rayon::prelude::*;

use crate::batch::UserEditsByFile;

/// The key field of each analyzed file that has one, and which files have
/// a key the user set by hand.
#[derive(Debug, Default)]
pub struct TagKeys {
    by_path: HashMap<PathBuf, TagKey>,
    key_set_by_user: HashSet<PathBuf>,
}

impl TagKeys {
    /// Reads the key field of every file that was analyzed. A file whose
    /// tags cannot be read counts as having none: its analysis is reported
    /// as usual.
    pub fn read(results: &[FileResult], user_edits: &UserEditsByFile) -> Self {
        let by_path = results
            .par_iter()
            .filter(|result| result.outcome.is_ok())
            .filter_map(|result| {
                let tag_key = euphoria_core::read_track_name(&result.path).ok()?.key?;
                Some((result.path.clone(), tag_key))
            })
            .collect();
        let key_set_by_user = user_edits
            .iter()
            .filter(|(_, edits)| {
                edits
                    .as_ref()
                    .is_ok_and(|edits| edits.correction.key.is_some())
            })
            .map(|(path, _)| path.clone())
            .collect();
        Self {
            by_path,
            key_set_by_user,
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
            key_set_by_user: HashSet::new(),
        }
    }

    /// The same, with the key of the file at `path` set by the user.
    #[cfg(test)]
    pub fn with_key_set_by_user(mut self, path: &Path) -> Self {
        self.key_set_by_user.insert(path.to_path_buf());
        self
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
        if self.key_set_by_user.contains(path) {
            return None;
        }
        self.agreement(path, analysis)
    }

    /// The difference between the tags and the detected key that makes the
    /// track worth a look, or `None` when they agree, either is missing or
    /// the user set the key by hand.
    pub fn conflict(&self, path: &Path, analysis: &TrackAnalysis) -> Option<KeyDifference> {
        match self.compared(path, analysis)? {
            TagKeyAgreement::Differs(difference) => Some(difference),
            TagKeyAgreement::Same | TagKeyAgreement::Unreadable => None,
        }
    }
}
