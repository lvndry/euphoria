// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Why a track is worth checking by ear, the same list in the desktop app
//! and the command line.

use crate::analysis::TrackAnalysis;
use crate::confidence::Confidence;
use crate::correction::Correction;
use crate::key::Key;
use crate::tag_key::{TagKey, TagKeyAgreement};
use crate::tempo_range::TempoRange;

/// One reason a track is listed under Needs review.
#[derive(Debug, Clone, PartialEq)]
pub enum ReviewReason {
    /// Analysis failed; the text says why.
    Failed(String),
    /// The key is rated low confidence.
    KeyUnsure,
    /// Half or double the tempo is a tempo a DJ could count and the
    /// onsets back it more strongly, so the track may be counted in the
    /// wrong octave. See [`TrackAnalysis::tempo_octave_in_doubt`].
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

/// Why an analyzed track is worth checking: the core rates its key low,
/// the octave of its tempo, reported in `range`, is in doubt, or its tags
/// name another key. A value the user set or confirmed with "Looks right"
/// is no longer in doubt. `analysis` may carry the correction already, or
/// not: the checks give the same answer.
///
/// The tempo's confidence rating does not list a track by itself: it
/// measures how close the best tempo that is not an octave of it came, not
/// the octave, which is where tempo detection is most often wrong.
pub fn review_reasons(
    analysis: &TrackAnalysis,
    correction: &Correction,
    tag_key: Option<&TagKey>,
    range: TempoRange,
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
        && analysis.tempo_octave_in_doubt(range)
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
    use crate::detect::OctaveEvidence;
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
            pitch_class_profile: None,
            tempo_estimate: Some(bpm),
            tempo_score_margin: Some(tempo_margin),
            tempo_octave_evidence: None,
            beat_grid: None,
            cues: Vec::new(),
        }
    }

    fn tagged(text: &str) -> TagKey {
        TagKey::from_text(text).expect("a key field")
    }

    #[test]
    fn a_doubtful_key_and_tempo_are_two_reasons() {
        let reasons = review_reasons(
            &rated(0.001, 87.0, 0.01),
            &Correction::default(),
            None,
            TempoRange::DEFAULT,
        );
        assert_eq!(
            reasons,
            [ReviewReason::KeyUnsure, ReviewReason::TempoUnsure]
        );
        assert!(
            review_reasons(
                &rated(0.02, 124.0, 0.6),
                &Correction::default(),
                None,
                TempoRange::DEFAULT
            )
            .is_empty()
        );
    }

    #[test]
    fn looks_right_ends_one_doubt_at_a_time() {
        let analysis = rated(0.001, 87.0, 0.01);
        let key_confirmed = Correction::default().confirming_key();
        assert_eq!(
            review_reasons(&analysis, &key_confirmed, None, TempoRange::DEFAULT),
            [ReviewReason::TempoUnsure]
        );
        let both = key_confirmed.confirming_bpm();
        assert!(review_reasons(&analysis, &both, None, TempoRange::DEFAULT).is_empty());
        assert!(is_reviewed(&key_confirmed));
        assert!(!is_reviewed(&Correction::default()));
    }

    #[test]
    fn a_value_the_user_set_is_not_in_doubt() {
        let analysis = rated(0.001, 87.0, 0.01);
        let set = Correction::of_key(Key::new(4, Mode::Minor))
            .with_bpm(Some(90.0))
            .unwrap();
        assert!(
            review_reasons(&analysis, &set, Some(&tagged("9A")), TempoRange::DEFAULT).is_empty()
        );
        assert!(is_reviewed(&set));
    }

    #[test]
    fn tags_that_name_another_key_are_a_conflict_until_confirmed() {
        let analysis = rated(0.02, 124.0, 0.6);
        let tag = tagged("9A");
        let reasons = review_reasons(
            &analysis,
            &Correction::default(),
            Some(&tag),
            TempoRange::DEFAULT,
        );
        assert_eq!(
            reasons,
            [ReviewReason::KeyConflict {
                tag_key: "9A".parse().unwrap()
            }]
        );
        assert_eq!(reasons[0].code(), "key_conflict");
        let confirmed = Correction::default().confirming_key();
        assert!(review_reasons(&analysis, &confirmed, Some(&tag), TempoRange::DEFAULT).is_empty());
        assert!(
            review_reasons(
                &analysis,
                &Correction::default(),
                Some(&tagged("8A")),
                TempoRange::DEFAULT
            )
            .is_empty()
        );
    }

    #[test]
    fn a_track_can_have_every_reason_and_failures_stand_alone() {
        let reasons = review_reasons(
            &rated(0.001, 87.0, 0.01),
            &Correction::default(),
            Some(&tagged("9A")),
            TempoRange::DEFAULT,
        );
        let codes: Vec<&str> = reasons.iter().map(ReviewReason::code).collect();
        assert_eq!(codes, ["key_unsure", "tempo_unsure", "key_conflict"]);
        assert_eq!(failed_reason("cannot decode")[0].code(), "failed");
    }

    fn tempo_reasons(estimate: f32, tempo_margin: f32, range: TempoRange) -> Vec<ReviewReason> {
        review_reasons(
            &rated(0.02, estimate, tempo_margin).with_tempo_range(range),
            &Correction::default(),
            None,
            range,
        )
    }

    fn alternative(estimate: f32, range: TempoRange) -> Option<f32> {
        rated(0.02, estimate, 0.01).tempo_alternative_in(range)
    }

    #[test]
    fn a_doubtful_tempo_with_no_plausible_half_or_double_is_not_listed() {
        assert_eq!(alternative(119.8, TempoRange::DEFAULT), None);
        assert!(tempo_reasons(119.8, 0.01, TempoRange::DEFAULT).is_empty());
        assert!(tempo_reasons(124.0, -0.05, TempoRange::DEFAULT).is_empty());
    }

    #[test]
    fn a_doubtful_tempo_with_a_plausible_octave_is_listed_with_it() {
        let hip_hop: TempoRange = "70-140".parse().unwrap();
        assert_eq!(alternative(87.0, hip_hop), Some(174.0));
        assert_eq!(
            tempo_reasons(87.0, 0.01, hip_hop),
            [ReviewReason::TempoUnsure]
        );
        assert_eq!(alternative(174.0, TempoRange::DEFAULT), Some(87.0));
        assert_eq!(
            tempo_reasons(174.0, 0.01, TempoRange::DEFAULT),
            [ReviewReason::TempoUnsure]
        );
        assert_eq!(alternative(70.0, hip_hop), Some(140.0));
        assert_eq!(
            tempo_reasons(70.0, 0.01, hip_hop),
            [ReviewReason::TempoUnsure]
        );
    }

    #[test]
    fn half_of_140_is_offered_only_when_the_range_makes_70_plausible() {
        assert_eq!(alternative(140.0, TempoRange::DEFAULT), None);
        assert!(tempo_reasons(140.0, 0.01, TempoRange::DEFAULT).is_empty());

        let hip_hop: TempoRange = "70-140".parse().unwrap();
        assert_eq!(rated(0.02, 140.0, 0.01).tempo_in(hip_hop), Some(70.0));
        assert_eq!(alternative(140.0, hip_hop), Some(140.0));
        assert_eq!(
            tempo_reasons(140.0, 0.01, hip_hop),
            [ReviewReason::TempoUnsure]
        );
    }

    fn with_evidence(estimate: f32, tempo_margin: f32, half: f32, double: f32) -> TrackAnalysis {
        TrackAnalysis {
            tempo_octave_evidence: Some(OctaveEvidence { half, double }),
            ..rated(0.02, estimate, tempo_margin)
        }
    }

    fn reasons_in(analysis: TrackAnalysis, range: TempoRange) -> Vec<ReviewReason> {
        review_reasons(
            &analysis.with_tempo_range(range),
            &Correction::default(),
            None,
            range,
        )
    }

    #[test]
    fn a_tempo_is_listed_when_the_onsets_back_its_plausible_octave() {
        let backed_half = with_evidence(170.0, 0.6, 0.2, -0.4);
        assert_eq!(
            reasons_in(backed_half.clone(), TempoRange::DEFAULT),
            [ReviewReason::TempoUnsure]
        );
        let backed_double = with_evidence(95.0, 0.6, -0.4, 0.15);
        assert_eq!(
            reasons_in(backed_double, TempoRange::DEFAULT),
            [ReviewReason::TempoUnsure]
        );
        let backed_only_double = with_evidence(170.0, 0.6, -0.1, 0.5);
        assert!(reasons_in(backed_only_double, TempoRange::DEFAULT).is_empty());
    }

    #[test]
    fn a_doubtful_rating_lists_nothing_when_the_onsets_back_the_reported_octave() {
        assert!(reasons_in(with_evidence(174.0, 0.01, -0.2, -0.3), TempoRange::DEFAULT).is_empty());
    }

    #[test]
    fn evidence_without_a_plausible_octave_lists_nothing() {
        assert!(reasons_in(with_evidence(119.8, 0.01, 0.9, 0.9), TempoRange::DEFAULT).is_empty());
    }

    #[test]
    fn the_evidence_follows_the_tempo_into_another_range() {
        let drum_and_bass: TempoRange = "120-200".parse().unwrap();
        let hip_hop: TempoRange = "70-140".parse().unwrap();
        // Shown at 190 in the drum and bass range, the alternative is 95,
        // the tempo the evidence was measured at, which it backs over 190.
        let doubled = with_evidence(95.0, 0.6, -0.4, -0.3);
        assert_eq!(
            reasons_in(doubled.clone(), drum_and_bass),
            [ReviewReason::TempoUnsure]
        );
        assert!(reasons_in(doubled, TempoRange::DEFAULT).is_empty());
        // Shown at 70 in the hip hop range, the alternative is 140, which
        // the evidence backs over 70 when half of 140 scored low.
        let halved = with_evidence(140.0, 0.6, -0.3, 0.0);
        assert_eq!(reasons_in(halved, hip_hop), [ReviewReason::TempoUnsure]);
    }

    #[test]
    fn a_tempo_rated_medium_or_high_is_not_listed_even_with_a_plausible_octave() {
        assert!(tempo_reasons(174.0, 0.2, TempoRange::DEFAULT).is_empty());
        assert!(tempo_reasons(87.0, 0.6, TempoRange::DEFAULT).is_empty());
    }

    #[test]
    fn the_tempo_range_decides_whether_a_doubtful_tempo_is_listed() {
        let hip_hop: TempoRange = "70-140".parse().unwrap();
        let drum_and_bass: TempoRange = "120-200".parse().unwrap();
        assert!(tempo_reasons(140.0, 0.01, TempoRange::DEFAULT).is_empty());
        assert_eq!(
            tempo_reasons(140.0, 0.01, hip_hop),
            [ReviewReason::TempoUnsure]
        );
        assert!(tempo_reasons(140.0, 0.01, drum_and_bass).is_empty());

        assert_eq!(alternative(100.0, TempoRange::DEFAULT), Some(200.0));
        assert_eq!(
            tempo_reasons(100.0, 0.01, TempoRange::DEFAULT),
            [ReviewReason::TempoUnsure]
        );
    }
}
