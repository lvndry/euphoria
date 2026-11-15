// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Why a track is worth checking by ear, the same list in the desktop app
//! and the command line.

use crate::analysis::TrackAnalysis;
use crate::confidence::Confidence;
use crate::correction::Correction;
use crate::key::Key;
use crate::tag_key::{TagKey, TagKeyAgreement};

/// One reason a track is listed under Needs review.
#[derive(Debug, Clone, PartialEq)]
pub enum ReviewReason {
    /// Analysis failed; the text says why.
    Failed(String),
    /// The key is rated low confidence.
    KeyUnsure,
    /// The tempo is rated low confidence.
    TempoUnsure,
    /// The key field of the file's tags names another key.
    KeyConflict {
        /// The key the tags name.
        tag_key: Key,
    },
}

impl ReviewReason {
    /// The name of the reason in JSON: `failed`, `key_unsure`,
    /// `tempo_unsure` or `key_conflict`.
    pub fn code(&self) -> &'static str {
        match self {
            ReviewReason::Failed(_) => "failed",
            ReviewReason::KeyUnsure => "key_unsure",
            ReviewReason::TempoUnsure => "tempo_unsure",
            ReviewReason::KeyConflict { .. } => "key_conflict",
        }
    }
}

/// Why an analyzed track is worth checking: the core rates its key or its
/// tempo low, or its tags name another key. A value the user set or
/// confirmed with "Looks right" is no longer in doubt. `analysis` may carry
/// the correction already, or not: the checks give the same answer.
pub fn review_reasons(
    analysis: &TrackAnalysis,
    correction: &Correction,
    tag_key: Option<&TagKey>,
) -> Vec<ReviewReason> {
    let mut reasons = Vec::new();
    if correction.key.is_none()
        && !correction.key_confirmed
        && analysis.key_confidence() == Some(Confidence::Low)
    {
        reasons.push(ReviewReason::KeyUnsure);
    }
    if correction.bpm.is_none()
        && !correction.bpm_confirmed
        && analysis.tempo_confidence() == Some(Confidence::Low)
    {
        reasons.push(ReviewReason::TempoUnsure);
    }
    if correction.key.is_none()
        && !correction.key_confirmed
        && let Some(tag_key) = tag_key
        && let Some(detected) = analysis.key
        && let TagKeyAgreement::Differs(_) = tag_key.compare(detected)
        && let Some(tagged) = tag_key.key
    {
        reasons.push(ReviewReason::KeyConflict { tag_key: tagged });
    }
    reasons
}

/// The reason of a track whose analysis failed.
pub fn failed_reason(message: impl Into<String>) -> Vec<ReviewReason> {
    vec![ReviewReason::Failed(message.into())]
}

/// Whether the user already settled the key or the tempo of the track by
/// setting it or confirming it with "Looks right".
pub fn is_reviewed(correction: &Correction) -> bool {
    correction.key.is_some()
        || correction.bpm.is_some()
        || correction.key_confirmed
        || correction.bpm_confirmed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::Mode;

    fn rated(key_margin: f32, bpm: f32, tempo_margin: f32) -> TrackAnalysis {
        TrackAnalysis {
            duration_seconds: 200.0,
            key: Some("8A".parse().unwrap()),
            bpm: Some(bpm),
            energy: Some(5),
            tuning_cents: None,
            key_timeline: Vec::new(),
            quality: None,
            key_runner_up: Some("3A".parse().unwrap()),
            key_score_margin: Some(key_margin),
            key_source: None,
            loudness_range_lu: None,
            tempo_estimate: Some(bpm),
            tempo_score_margin: Some(tempo_margin),
            beat_grid: None,
            cues: Vec::new(),
        }
    }

    fn tagged(text: &str) -> TagKey {
        TagKey::from_text(text).expect("a key field")
    }

    #[test]
    fn a_doubtful_key_and_tempo_are_two_reasons() {
        let reasons = review_reasons(&rated(0.001, 87.0, 0.01), &Correction::default(), None);
        assert_eq!(
            reasons,
            [ReviewReason::KeyUnsure, ReviewReason::TempoUnsure]
        );
        assert!(review_reasons(&rated(0.02, 124.0, 0.6), &Correction::default(), None).is_empty());
    }

    #[test]
    fn looks_right_ends_one_doubt_at_a_time() {
        let analysis = rated(0.001, 87.0, 0.01);
        let key_confirmed = Correction::default().confirming_key();
        assert_eq!(
            review_reasons(&analysis, &key_confirmed, None),
            [ReviewReason::TempoUnsure]
        );
        let both = key_confirmed.confirming_bpm();
        assert!(review_reasons(&analysis, &both, None).is_empty());
        assert!(is_reviewed(&key_confirmed));
        assert!(!is_reviewed(&Correction::default()));
    }

    #[test]
    fn a_value_the_user_set_is_not_in_doubt() {
        let analysis = rated(0.001, 87.0, 0.01);
        let set = Correction::of_key(Key::new(4, Mode::Minor))
            .with_bpm(Some(90.0))
            .unwrap();
        assert!(review_reasons(&analysis, &set, Some(&tagged("9A"))).is_empty());
        assert!(is_reviewed(&set));
    }

    #[test]
    fn tags_that_name_another_key_are_a_conflict_until_confirmed() {
        let analysis = rated(0.02, 124.0, 0.6);
        let tag = tagged("9A");
        let reasons = review_reasons(&analysis, &Correction::default(), Some(&tag));
        assert_eq!(
            reasons,
            [ReviewReason::KeyConflict {
                tag_key: "9A".parse().unwrap()
            }]
        );
        assert_eq!(reasons[0].code(), "key_conflict");
        let confirmed = Correction::default().confirming_key();
        assert!(review_reasons(&analysis, &confirmed, Some(&tag)).is_empty());
        assert!(review_reasons(&analysis, &Correction::default(), Some(&tagged("8A"))).is_empty());
    }

    #[test]
    fn a_track_can_have_every_reason_and_failures_stand_alone() {
        let reasons = review_reasons(
            &rated(0.001, 87.0, 0.01),
            &Correction::default(),
            Some(&tagged("9A")),
        );
        let codes: Vec<&str> = reasons.iter().map(ReviewReason::code).collect();
        assert_eq!(codes, ["key_unsure", "tempo_unsure", "key_conflict"]);
        assert_eq!(failed_reason("cannot decode")[0].code(), "failed");
    }
}
