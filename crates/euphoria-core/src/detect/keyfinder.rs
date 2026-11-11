// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::audio::AudioBuffer;
use crate::detect::chromagram::{Chromagram, DETECTOR_NAME, KEY_COUNT, key_scores};
use crate::detect::key_sections::{highest_total, runs, steadiest_keys, window_hops};
use crate::detect::tuning::{Tuning, measure_tuning};
use crate::detect::{KeyDetector, KeyReading, KeySection};
use crate::error::EuphoriaError;
use crate::key::{Key, Mode};

const A_PITCH_CLASS: usize = 9;

/// Each analysis frame of libkeyfinder spans this many hops.
const HOPS_PER_FRAME: usize = 4;

/// Hops in each window whose key is read for the key over time, about 7.4
/// seconds: long enough to hold a bar or two of every chord of a loop.
const HOPS_PER_WINDOW: usize = 8;

/// What a change of key costs against the key scores of the windows, which
/// run from 0 to 1. See [`steadiest_keys`].
///
/// Chosen without the key labels of any dataset. On synthesized chord
/// progressions that move to another key for 30 seconds, 0.1 is the highest
/// of the values tried that still shows a move between two minor keys a
/// fifth apart, and four seconds in a distant key do not show. On two minute
/// previews of dance music, which seldom changes key, 95% come out as one
/// section, against 83% at half the value.
const KEY_CHANGE_PENALTY: f64 = 0.1;

/// The smallest tuning offset the tuning correction acts on, in cents.
/// libkeyfinder listens for each note through a window 80 cents wide that
/// fades towards its edges, and a note 20 cents off passes at half strength.
const SMALLEST_CORRECTED_OFFSET_CENTS: f32 = 20.0;

/// Key detection through libkeyfinder, as Euphoria ships it: the key of the
/// whole track at once, as libkeyfinder decides it, after a track measured
/// at least 20 cents away from concert pitch is brought back to it.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyFinderDetector;

impl KeyFinderDetector {
    /// Finds the key of a track, how far it is tuned from concert pitch, and
    /// its key over time.
    ///
    /// The key over time is reported next to the key and does not change it.
    /// A timeline with one section says that no change of key was found, and
    /// that section carries the key of the track.
    pub fn read_key(&self, audio: &AudioBuffer) -> Result<KeyReading, EuphoriaError> {
        let tuning = measure_tuning(audio);
        let analysis = ChromaAnalysis::of_audio(audio, tuning.filter(is_worth_correcting))?;
        let key = analysis.key_of_whole_track()?;
        let timeline = match key {
            Some(key) => analysis.timeline(key, audio.duration_seconds())?,
            None => Vec::new(),
        };
        Ok(KeyReading {
            key,
            tuning_cents: tuning.map(|tuning| tuning.cents),
            timeline,
        })
    }
}

impl KeyDetector for KeyFinderDetector {
    fn name(&self) -> &'static str {
        DETECTOR_NAME
    }

    fn detect_key(&self, audio: &AudioBuffer) -> Result<Option<Key>, EuphoriaError> {
        let correction = measure_tuning(audio).filter(is_worth_correcting);
        ChromaAnalysis::of_audio(audio, correction)?.key_of_whole_track()
    }
}

/// Whether a measured tuning is far enough from concert pitch for the
/// tuning correction to act on it.
fn is_worth_correcting(tuning: &Tuning) -> bool {
    tuning.cents.abs() >= SMALLEST_CORRECTED_OFFSET_CENTS
}

/// How [`KeyFinderVariant`] turns the chromagram of a track into one key.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum KeyDecision {
    /// The key that best matches the whole track at once, which is what
    /// libkeyfinder does. Loud passages count for more than quiet ones.
    #[default]
    WholeTrack,
    /// The track is cut into windows of about 7 seconds, every window scores
    /// every key, and the key with the highest total wins. Every window
    /// counts the same, however loud it is.
    SectionVote,
}

/// Ways of deciding the key with libkeyfinder that the accuracy benchmark
/// measures against [`KeyFinderDetector`], which is the variant with tuning
/// correction that decides on the whole track. Euphoria uses none of the
/// others.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyFinderVariant {
    /// Whether a track measured at least 20 cents away from concert pitch is
    /// analyzed as if it had been tuned to it.
    pub tuning_correction: bool,
    /// How the key is decided.
    pub decision: KeyDecision,
}

impl KeyDetector for KeyFinderVariant {
    fn name(&self) -> &'static str {
        match (self.tuning_correction, self.decision) {
            (true, KeyDecision::WholeTrack) => DETECTOR_NAME,
            (false, KeyDecision::WholeTrack) => "libkeyfinder, no tuning correction",
            (true, KeyDecision::SectionVote) => "libkeyfinder, sections vote",
            (false, KeyDecision::SectionVote) => {
                "libkeyfinder, no tuning correction, sections vote"
            }
        }
    }

    fn detect_key(&self, audio: &AudioBuffer) -> Result<Option<Key>, EuphoriaError> {
        let correction = self
            .tuning_correction
            .then(|| measure_tuning(audio))
            .flatten()
            .filter(is_worth_correcting);
        let analysis = ChromaAnalysis::of_audio(audio, correction)?;
        match self.decision {
            KeyDecision::WholeTrack => analysis.key_of_whole_track(),
            KeyDecision::SectionVote => {
                Ok(highest_total(&analysis.window_scores()?).map(key_from_code))
            }
        }
    }
}

/// The chromagram of a track and when each of its frames sounds.
struct ChromaAnalysis {
    chromagram: Chromagram,
    seconds_per_hop: f32,
}

impl ChromaAnalysis {
    /// With a `correction`, libkeyfinder is told a sample rate that moves
    /// every note by the tuning offset, so it hears the track at concert
    /// pitch.
    fn of_audio(audio: &AudioBuffer, correction: Option<Tuning>) -> Result<Self, EuphoriaError> {
        let sample_rate = audio.sample_rate().get();
        let told_rate = match correction {
            Some(tuning) => {
                let pitch_ratio = 2.0_f64.powf(f64::from(tuning.cents) / 1_200.0);
                (f64::from(sample_rate) / pitch_ratio).round() as u32
            }
            None => sample_rate,
        };
        let chromagram = Chromagram::of_samples(audio.samples(), told_rate)?;
        let seconds_per_hop = chromagram.samples_per_hop() as f32 / sample_rate as f32;
        Ok(Self {
            chromagram,
            seconds_per_hop,
        })
    }

    fn key_of_whole_track(&self) -> Result<Option<Key>, EuphoriaError> {
        let whole_track = self.chromagram.mean_of_hops(0..self.chromagram.hop_count());
        let scores = key_scores(&whole_track)?;
        Ok(scores.first().and_then(best_key))
    }

    /// The scores of every key in every window of the track.
    fn window_scores(&self) -> Result<Vec<[f64; KEY_COUNT]>, EuphoriaError> {
        let chroma_vectors: Vec<f64> = window_hops(self.chromagram.hop_count(), HOPS_PER_WINDOW)
            .into_iter()
            .flat_map(|hops| self.chromagram.mean_of_hops(hops))
            .collect();
        key_scores(&chroma_vectors)
    }

    /// The key of each stretch of the track. A track that stays in one key
    /// is one section in `track_key`.
    fn timeline(&self, track_key: Key, duration: f32) -> Result<Vec<KeySection>, EuphoriaError> {
        let windows = window_hops(self.chromagram.hop_count(), HOPS_PER_WINDOW);
        let keys = steadiest_keys(&self.window_scores()?, KEY_CHANGE_PENALTY);
        let runs = runs(&keys);
        if runs.len() <= 1 {
            return Ok(vec![KeySection {
                start_seconds: 0.0,
                end_seconds: duration,
                key: track_key,
            }]);
        }
        let last_run = runs.len() - 1;
        Ok(runs
            .into_iter()
            .enumerate()
            .map(|(run_index, (run_windows, code))| KeySection {
                start_seconds: if run_index == 0 {
                    0.0
                } else {
                    self.boundary_seconds(windows[run_windows.start].start, duration)
                },
                end_seconds: if run_index == last_run {
                    duration
                } else {
                    self.boundary_seconds(windows[run_windows.end - 1].end, duration)
                },
                key: key_from_code(code),
            })
            .collect())
    }

    /// The time halfway between the middle of the frame at `hop` and the
    /// middle of the frame before it.
    fn boundary_seconds(&self, hop: usize, duration: f32) -> f32 {
        let hops_from_start = hop as f32 + (HOPS_PER_FRAME as f32 - 1.0) / 2.0;
        (hops_from_start * self.seconds_per_hop).min(duration)
    }
}

/// The key with the highest score, or `None` when no key scores above zero,
/// which is how libkeyfinder reports silence. Among equal scores the first
/// in libkeyfinder key code order wins, as in libkeyfinder.
fn best_key(scores: &[f64; KEY_COUNT]) -> Option<Key> {
    let mut best: Option<(usize, f64)> = None;
    for (code, &score) in scores.iter().enumerate() {
        if score > best.map_or(0.0, |(_, best_score)| best_score) {
            best = Some((code, score));
        }
    }
    best.map(|(code, _)| key_from_code(code))
}

/// libkeyfinder numbers keys from A major upwards by semitone, with the
/// major key at the even index and its parallel minor at the odd one.
fn key_from_code(code: usize) -> Key {
    let tonic = ((A_PITCH_CLASS + code / 2) % 12) as u8;
    let mode = if code.is_multiple_of(2) {
        Mode::Major
    } else {
        Mode::Minor
    };
    Key::new(tonic, mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::synthesis::{SAMPLE_RATE, buffer, chord_progression, pink_noise};

    fn key(text: &str) -> Key {
        text.parse().unwrap()
    }

    #[test]
    fn maps_library_codes_to_keys() {
        assert_eq!(key_from_code(0), key("A major"));
        assert_eq!(key_from_code(1), key("A minor"));
        assert_eq!(key_from_code(6), key("C major"));
        assert_eq!(key_from_code(23), key("Ab minor"));
    }

    #[test]
    fn the_best_key_is_the_first_highest_score_and_no_score_is_no_key() {
        let mut scores = [0.1; KEY_COUNT];
        scores[7] = 0.9;
        scores[12] = 0.9;
        assert_eq!(best_key(&scores), Some(key_from_code(7)));
        assert_eq!(best_key(&[0.0; KEY_COUNT]), None);
    }

    #[test]
    fn a_progression_30_cents_sharp_reports_the_offset_and_the_right_key() {
        let audio = buffer(chord_progression(key("C major"), 30.0, 24.0));

        let reading = KeyFinderDetector.read_key(&audio).unwrap();

        assert_eq!(reading.key, Some(key("C major")));
        let tuning = reading.tuning_cents.expect("the progression has a tuning");
        assert!((tuning - 30.0).abs() < 2.0, "{tuning}");
        assert_eq!(
            reading.timeline,
            [KeySection {
                start_seconds: 0.0,
                end_seconds: audio.duration_seconds(),
                key: key("C major"),
            }]
        );
    }

    #[test]
    fn reading_the_key_gives_the_key_that_detecting_it_gives() {
        let audio = buffer(chord_progression(key("A minor"), -12.0, 16.0));
        assert_eq!(
            KeyFinderDetector.read_key(&audio).unwrap().key,
            KeyFinderDetector.detect_key(&audio).unwrap()
        );
    }

    const UNCORRECTED: KeyFinderVariant = KeyFinderVariant {
        tuning_correction: false,
        decision: KeyDecision::WholeTrack,
    };

    #[test]
    fn tuning_correction_finds_the_key_of_a_track_almost_a_quarter_tone_sharp() {
        let audio = buffer(chord_progression(key("A minor"), 45.0, 24.0));

        assert_eq!(
            UNCORRECTED.detect_key(&audio).unwrap(),
            Some(key("F major")),
            "the detuned track is misread without the correction"
        );
        assert_eq!(
            KeyFinderDetector.detect_key(&audio).unwrap(),
            Some(key("A minor"))
        );
        let reading = KeyFinderDetector.read_key(&audio).unwrap();
        assert_eq!(reading.key, Some(key("A minor")));
        assert_eq!(reading.timeline.len(), 1);
        assert_eq!(reading.timeline[0].key, key("A minor"));
    }

    #[test]
    fn a_track_less_than_20_cents_off_is_analyzed_as_it_is() {
        for cents in [5.0, 19.0, -19.0] {
            let audio = buffer(chord_progression(key("F# major"), cents, 16.0));
            let analyzed_as_it_is = ChromaAnalysis::of_audio(&audio, None).unwrap();
            let tuning = measure_tuning(&audio).unwrap();
            assert!(!is_worth_correcting(&tuning), "{cents}");
            assert_eq!(
                KeyFinderDetector.detect_key(&audio).unwrap(),
                analyzed_as_it_is.key_of_whole_track().unwrap()
            );
        }
        let far_off = Tuning {
            cents: -20.0,
            agreement: 0.9,
        };
        assert!(is_worth_correcting(&far_off));
    }

    #[test]
    fn the_shipped_detector_is_the_variant_with_tuning_correction_on_the_whole_track() {
        let shipped = KeyFinderVariant {
            tuning_correction: true,
            decision: KeyDecision::WholeTrack,
        };
        for cents in [0.0, 30.0, 45.0] {
            let audio = buffer(chord_progression(key("A minor"), cents, 16.0));
            assert_eq!(
                shipped.detect_key(&audio).unwrap(),
                KeyFinderDetector.detect_key(&audio).unwrap()
            );
        }
    }

    #[test]
    fn the_section_vote_finds_the_key_of_a_progression() {
        let audio = buffer(chord_progression(key("C major"), 0.0, 32.0));
        let vote = KeyFinderVariant {
            tuning_correction: true,
            decision: KeyDecision::SectionVote,
        };
        assert_eq!(vote.detect_key(&audio).unwrap(), Some(key("C major")));
    }

    #[test]
    fn a_track_that_changes_key_shows_both_keys_at_about_the_right_time() {
        let change_seconds = 48.0;
        for (first, second) in [("C major", "F# minor"), ("A minor", "E minor")] {
            let mut samples = chord_progression(key(first), 0.0, change_seconds);
            samples.extend(chord_progression(key(second), 0.0, 32.0));
            let audio = buffer(samples);

            let reading = KeyFinderDetector.read_key(&audio).unwrap();

            let [before, after] = reading.timeline[..] else {
                panic!("{first} to {second}: {:?}", reading.timeline);
            };
            assert_eq!(before.key, key(first));
            assert_eq!(after.key, key(second));
            assert_eq!(before.start_seconds, 0.0);
            assert_eq!(before.end_seconds, after.start_seconds);
            assert_eq!(after.end_seconds, audio.duration_seconds());
            assert!(
                (after.start_seconds - change_seconds).abs() < 6.0,
                "{first} to {second}: the change is reported at {} seconds",
                after.start_seconds
            );
        }
    }

    #[test]
    fn a_few_seconds_in_another_key_do_not_split_the_timeline() {
        let mut samples = chord_progression(key("C major"), 0.0, 40.0);
        samples.extend(chord_progression(key("F# major"), 0.0, 4.0));
        samples.extend(chord_progression(key("C major"), 0.0, 40.0));
        let audio = buffer(samples);

        let reading = KeyFinderDetector.read_key(&audio).unwrap();

        assert_eq!(reading.key, Some(key("C major")));
        assert_eq!(reading.timeline.len(), 1, "{:?}", reading.timeline);
    }

    #[test]
    fn silence_has_no_key_no_tuning_and_no_timeline() {
        let audio = buffer(vec![0.0; 4 * SAMPLE_RATE as usize]);
        assert_eq!(
            KeyFinderDetector.read_key(&audio).unwrap(),
            KeyReading::default()
        );
    }

    #[test]
    fn noise_has_no_tuning() {
        let audio = buffer(pink_noise(8 * SAMPLE_RATE as usize, 0.5));
        assert_eq!(
            KeyFinderDetector.read_key(&audio).unwrap().tuning_cents,
            None
        );
    }

    #[test]
    fn audio_shorter_than_a_frame_is_read_without_failing() {
        let audio = buffer(vec![0.5, -0.25, 0.75, -0.5]);
        let reading = KeyFinderDetector.read_key(&audio).unwrap();
        assert_eq!(reading.tuning_cents, None);
        assert_eq!(reading.timeline.len(), usize::from(reading.key.is_some()));
    }

    #[test]
    fn every_variant_has_a_name_of_its_own() {
        let mut names = Vec::new();
        for tuning_correction in [false, true] {
            for decision in [KeyDecision::WholeTrack, KeyDecision::SectionVote] {
                let variant = KeyFinderVariant {
                    tuning_correction,
                    decision,
                };
                names.push(variant.name());
            }
        }
        assert_eq!(names[2], KeyFinderDetector.name());
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 4);
    }
}
