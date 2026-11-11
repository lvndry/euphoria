// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::ops::Range;

use crate::audio::AudioBuffer;
use crate::detect::chromagram::{Chromagram, DETECTOR_NAME, KEY_COUNT, key_scores};
use crate::detect::key_sections::{
    ChangeRules, highest_total, keys_against_track_key, runs, window_hops,
};
use crate::detect::tuning::{Tuning, measure_tuning};
use crate::detect::{KeyDetector, KeyReading, KeySection};
use crate::error::EuphoriaError;
use crate::key::{Key, Mode};

const A_PITCH_CLASS: usize = 9;

/// Each analysis frame of libkeyfinder starts at its hop and spans this
/// many hops, so it is heard most at its middle, half that many hops after
/// its start.
const HOPS_PER_FRAME: usize = 4;

/// A window whose chroma is this far below the loudest window of the track,
/// 60 dB, or that has none at all, is silence and holds no evidence of a
/// key. Chroma grows with the amplitude of the audio.
///
/// Only silence is left out. A passage with sound but no harmony, such as
/// a bare kick drum or a noise riser, cannot be told from music by its key
/// scores: on two minute previews of dance music one window in ten matches
/// its best key no more clearly than a synthesized kick drum does.
const SILENT_WINDOW_LEVEL: f64 = 1.0e-3;

/// Hops in each window whose key is read for the key over time, about 7.4
/// seconds: long enough to hold a bar or two of every chord of a loop.
const HOPS_PER_WINDOW: usize = 8;

/// When the key over time leaves the key of the track. See
/// [`keys_against_track_key`].
///
/// Key scores run from 0 to 1. In real music the best key of a seven second
/// window changes often and by little: on a GiantSteps preview whose key
/// does not change, other keys beat the key of the track by up to 0.02 per
/// window for half a minute. On synthesized chord progressions that move to
/// another key, the new key wins by 0.04 to 0.05 per window when it is a
/// fifth away and 0.2 when it is distant. The margin of 0.025 is the
/// largest step of 0.005 with which a move a fifth away that lasts three
/// windows, 22 seconds, still shows. Checked on the GiantSteps previews
/// against the earlier rule, which followed the best key from window to
/// window with a penalty of 0.1: previews with more than one section fell
/// from 4.3% to 1.0% on the half the setting was chosen on and from 5.3% to
/// 1.0% on the other half, and none lost the key of the track, which 1.0%
/// did before.
const CHANGE_RULES: ChangeRules = ChangeRules {
    per_window_margin: 0.025,
    change_penalty: 0.05,
    minimum_windows: 3,
};

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
        let ranking = analysis.ranking_of_whole_track()?;
        let key = ranking.map(|ranking| ranking.key);
        let timeline = match key {
            Some(key) => analysis.timeline(key, audio.duration_seconds())?,
            None => Vec::new(),
        };
        Ok(KeyReading {
            key,
            runner_up: ranking.map(|ranking| ranking.runner_up),
            score_margin: ranking.map(|ranking| ranking.score_margin),
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
                let window_scores: Vec<[f64; KEY_COUNT]> =
                    analysis.window_scores()?.into_iter().flatten().collect();
                Ok(highest_total(&window_scores).map(key_from_code))
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
        Ok(self.ranking_of_whole_track()?.map(|ranking| ranking.key))
    }

    /// The two keys that best match the whole track at once, or `None` when
    /// no key scores above zero.
    fn ranking_of_whole_track(&self) -> Result<Option<Ranking>, EuphoriaError> {
        let whole_track = self.chromagram.mean_of_hops(0..self.chromagram.hop_count());
        let scores = key_scores(&whole_track)?;
        Ok(scores.first().and_then(rank))
    }

    /// The scores of every key in every window of the track, or `None` for
    /// a silent window, which holds no evidence of a key.
    fn window_scores(&self) -> Result<Vec<Option<[f64; KEY_COUNT]>>, EuphoriaError> {
        let chroma_vectors: Vec<Vec<f64>> =
            window_hops(self.chromagram.hop_count(), HOPS_PER_WINDOW)
                .into_iter()
                .map(|hops| self.chromagram.mean_of_hops(hops))
                .collect();
        let levels: Vec<f64> = chroma_vectors
            .iter()
            .map(|chroma| chroma.iter().map(|value| value * value).sum::<f64>().sqrt())
            .collect();
        let loudest = levels.iter().copied().fold(0.0, f64::max);
        let scores = key_scores(&chroma_vectors.concat())?;
        Ok(scores
            .into_iter()
            .zip(levels)
            .map(|(scores, level)| {
                (level > 0.0 && level >= SILENT_WINDOW_LEVEL * loudest).then_some(scores)
            })
            .collect())
    }

    /// The key of each stretch of the track.
    ///
    /// Silence belongs to no section, so the timeline has a hole where the
    /// track is silent, and starts late or ends early when the track starts
    /// or ends in silence. When every stretch with sound is in one key, each
    /// section carries `track_key`, and a track without silence is then one
    /// section from start to end.
    fn timeline(&self, track_key: Key, duration: f32) -> Result<Vec<KeySection>, EuphoriaError> {
        let windows = window_hops(self.chromagram.hop_count(), HOPS_PER_WINDOW);
        let keys = keys_against_track_key(
            &self.window_scores()?,
            code_from_key(track_key),
            CHANGE_RULES,
        );
        let keyed_runs: Vec<(Range<usize>, usize)> = runs(&keys)
            .into_iter()
            .filter_map(|(run_windows, code)| Some((run_windows, code?)))
            .collect();
        let stays_in_one_key = keyed_runs.windows(2).all(|pair| pair[0].1 == pair[1].1);
        Ok(keyed_runs
            .into_iter()
            .map(|(run_windows, code)| KeySection {
                start_seconds: if run_windows.start == 0 {
                    0.0
                } else {
                    self.boundary_seconds(windows[run_windows.start].start, duration)
                },
                end_seconds: if run_windows.end == windows.len() {
                    duration
                } else {
                    self.boundary_seconds(windows[run_windows.end].start, duration)
                },
                key: if stays_in_one_key {
                    track_key
                } else {
                    key_from_code(code)
                },
            })
            .collect())
    }

    /// Where the window that starts at `hop` takes over from the window
    /// before it: halfway between the middles of their two nearest frames,
    /// the frame at `hop` and the frame before it.
    fn boundary_seconds(&self, hop: usize, duration: f32) -> f32 {
        let hop = hop as f32;
        let midpoint_hops = (frame_middle_hops(hop - 1.0) + frame_middle_hops(hop)) / 2.0;
        (midpoint_hops * self.seconds_per_hop).min(duration)
    }
}

/// Where the frame that starts at `hop` is heard most, in hops from the
/// start of the track.
fn frame_middle_hops(hop: f32) -> f32 {
    hop + HOPS_PER_FRAME as f32 / 2.0
}

/// The two best matching keys of a track and how far apart their scores are.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Ranking {
    key: Key,
    runner_up: Key,
    score_margin: f32,
}

/// The best and second best keys, or `None` when no key scores above zero,
/// which is how libkeyfinder reports silence. Among equal scores the first
/// in libkeyfinder key code order ranks higher, as in libkeyfinder.
fn rank(scores: &[f64; KEY_COUNT]) -> Option<Ranking> {
    let mut codes: Vec<usize> = (0..KEY_COUNT).collect();
    codes.sort_by(|left, right| {
        scores[*right]
            .total_cmp(&scores[*left])
            .then(left.cmp(right))
    });
    let best = codes[0];
    let second = codes[1];
    (scores[best] > 0.0).then(|| Ranking {
        key: key_from_code(best),
        runner_up: key_from_code(second),
        score_margin: (scores[best] - scores[second]) as f32,
    })
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

/// The libkeyfinder code of a key, the inverse of [`key_from_code`].
fn code_from_key(key: Key) -> usize {
    let semitones_above_a = (usize::from(key.tonic()) + 12 - A_PITCH_CLASS) % 12;
    let minor = usize::from(key.mode() == Mode::Minor);
    semitones_above_a * 2 + minor
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;
    use crate::detect::synthesis::{SAMPLE_RATE, buffer, chord_progression, pink_noise};

    fn key(text: &str) -> Key {
        text.parse().unwrap()
    }

    #[test]
    fn codes_and_keys_map_both_ways() {
        for code in 0..KEY_COUNT {
            assert_eq!(code_from_key(key_from_code(code)), code);
        }
        assert_eq!(code_from_key(key("A major")), 0);
        assert_eq!(code_from_key(key("Ab minor")), 23);
    }

    #[test]
    fn maps_library_codes_to_keys() {
        assert_eq!(key_from_code(0), key("A major"));
        assert_eq!(key_from_code(1), key("A minor"));
        assert_eq!(key_from_code(6), key("C major"));
        assert_eq!(key_from_code(23), key("Ab minor"));
    }

    #[test]
    fn the_runner_up_is_the_second_best_score_and_the_margin_their_difference() {
        let mut scores = [0.1; KEY_COUNT];
        scores[4] = 0.8;
        scores[9] = 0.75;
        scores[17] = 0.75;
        let ranking = rank(&scores).unwrap();
        assert_eq!(ranking.key, key_from_code(4));
        assert_eq!(ranking.runner_up, key_from_code(9));
        assert!((ranking.score_margin - 0.05).abs() < 1.0e-6);
        assert_eq!(rank(&[0.0; KEY_COUNT]), None);
    }

    #[test]
    fn a_reading_carries_the_runner_up_and_a_confidence() {
        let audio = buffer(chord_progression(key("C major"), 0.0, 24.0));

        let reading = KeyFinderDetector.read_key(&audio).unwrap();

        assert_eq!(reading.key, Some(key("C major")));
        let runner_up = reading.runner_up.expect("a progression has a runner up");
        assert_ne!(runner_up, key("C major"));
        assert!(reading.score_margin.unwrap() > 0.0);
        assert!(reading.confidence().is_some());
        let silent = KeyFinderDetector
            .read_key(&buffer(vec![0.0; 4 * SAMPLE_RATE as usize]))
            .unwrap();
        assert_eq!((silent.runner_up, silent.score_margin), (None, None));
        assert_eq!(silent.confidence(), None);
    }

    #[test]
    fn the_best_key_is_the_first_highest_score_and_no_score_is_no_key() {
        let mut scores = [0.1; KEY_COUNT];
        scores[7] = 0.9;
        scores[12] = 0.9;
        let ranking = rank(&scores).unwrap();
        assert_eq!(ranking.key, key_from_code(7));
        assert_eq!(ranking.runner_up, key_from_code(12));
        assert_eq!(ranking.score_margin, 0.0);
        assert_eq!(rank(&[0.0; KEY_COUNT]), None);
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

    fn silence(seconds: f32) -> Vec<f32> {
        vec![0.0; (seconds * SAMPLE_RATE as f32) as usize]
    }

    fn progression(name: &str, seconds: f32) -> Vec<f32> {
        chord_progression(key(name), 0.0, seconds)
    }

    /// A kick drum on every beat at 120 BPM, with no other instrument.
    fn kick_drum(seconds: f32) -> Vec<f32> {
        let samples_per_beat = SAMPLE_RATE as usize / 2;
        let sample_count = (seconds * SAMPLE_RATE as f32) as usize;
        (0..sample_count)
            .map(|index| {
                let since_beat = (index % samples_per_beat) as f32 / SAMPLE_RATE as f32;
                let frequency = 50.0 + 100.0 * (-since_beat / 0.03).exp();
                0.8 * (TAU * frequency * since_beat).sin() * (-since_beat / 0.15).exp()
            })
            .collect()
    }

    /// Seconds in one window of the key over time, the furthest a section
    /// boundary can be from where the music changes.
    fn window_seconds() -> f32 {
        let probe = ChromaAnalysis::of_audio(&buffer(silence(1.0)), None).unwrap();
        HOPS_PER_WINDOW as f32 * probe.seconds_per_hop
    }

    #[test]
    fn a_frame_is_heard_most_halfway_through_its_four_hops() {
        let probe = ChromaAnalysis::of_audio(&buffer(silence(1.0)), None).unwrap();
        let samples_per_hop = probe.chromagram.samples_per_hop();
        let burst_half_length = SAMPLE_RATE as usize / 100;
        // A burst short against a frame, placed so the frames that hear it
        // are sampled symmetrically about it, shows where frames listen.
        for burst_hop in [20.0, 20.25, 20.5, 20.75] {
            let mut samples = vec![0.0_f32; 44 * samples_per_hop];
            let centre = (burst_hop * samples_per_hop as f64) as usize;
            let first_sample = centre - burst_half_length;
            let burst = &mut samples[first_sample..centre + burst_half_length];
            for (offset, sample) in burst.iter_mut().enumerate() {
                let phase = offset as f32 / (2 * burst_half_length) as f32;
                let envelope = 0.5 - 0.5 * (TAU * phase).cos();
                let time = (first_sample + offset) as f32 / SAMPLE_RATE as f32;
                *sample = envelope * (TAU * 440.0 * time).sin();
            }
            let analysis = ChromaAnalysis::of_audio(&buffer(samples), None).unwrap();
            let energies: Vec<f64> = (0..analysis.chromagram.hop_count())
                .map(|hop| analysis.chromagram.mean_of_hops(hop..hop + 1).iter().sum())
                .collect();
            let total: f64 = energies.iter().sum();
            let heard_by_hop = energies
                .iter()
                .enumerate()
                .map(|(hop, energy)| hop as f64 * energy)
                .sum::<f64>()
                / total;

            let middle_after_start = burst_hop - heard_by_hop;

            assert!(
                (middle_after_start - HOPS_PER_FRAME as f64 / 2.0).abs() < 0.05,
                "burst at hop {burst_hop}: frames are heard most {middle_after_start} hops \
                 after they start"
            );
            assert!(
                (f64::from(frame_middle_hops(20.0)) - (20.0 + middle_after_start)).abs() < 0.05
            );
        }
    }

    #[test]
    fn sections_change_halfway_between_the_middles_of_two_frames() {
        let analysis = ChromaAnalysis::of_audio(&buffer(silence(1.0)), None).unwrap();
        for hop in [1, 8, 32] {
            let expected_hops = hop as f32 + 1.5;
            let boundary = analysis.boundary_seconds(hop, f32::MAX);
            assert!(
                (boundary / analysis.seconds_per_hop - expected_hops).abs() < 1.0e-3,
                "hop {hop}: {boundary} seconds"
            );
        }
        assert_eq!(analysis.boundary_seconds(32, 1.0), 1.0);
    }

    #[test]
    fn a_change_of_key_is_reported_within_a_quarter_hop_of_where_it_happens() {
        let probe = ChromaAnalysis::of_audio(&buffer(silence(1.0)), None).unwrap();
        let samples_per_hop = probe.chromagram.samples_per_hop();
        let hop_count = 64;
        let window_start = 32;
        // The windows split cleanly where every frame of one window is
        // heard before the change and every frame of the next after it.
        let change_hops = window_start as f32 + 1.5;
        let change_sample = (change_hops * samples_per_hop as f32) as usize;
        let total_samples = hop_count * samples_per_hop;
        let mut samples = progression("C major", 60.0)[..change_sample].to_vec();
        samples.extend_from_slice(&progression("F# minor", 60.0)[..total_samples - change_sample]);
        let audio = buffer(samples);

        let reading = KeyFinderDetector.read_key(&audio).unwrap();

        let analysis = ChromaAnalysis::of_audio(&audio, None).unwrap();
        assert_eq!(analysis.chromagram.hop_count(), hop_count);
        assert_eq!(
            window_hops(hop_count, HOPS_PER_WINDOW)[4].start,
            window_start
        );
        let [before, after] = reading.timeline[..] else {
            panic!("{:?}", reading.timeline);
        };
        assert_eq!((before.key, after.key), (key("C major"), key("F# minor")));
        let error_hops = (after.start_seconds - change_sample as f32 / SAMPLE_RATE as f32)
            / analysis.seconds_per_hop;
        assert!(
            error_hops.abs() < 0.25,
            "the change is reported {error_hops} hops from where it happens"
        );
    }

    #[test]
    fn silence_between_two_keys_is_a_hole_in_the_timeline() {
        let audio = buffer(
            [
                progression("C major", 40.0),
                silence(30.0),
                progression("F# minor", 40.0),
            ]
            .concat(),
        );
        let window = window_seconds();

        let reading = KeyFinderDetector.read_key(&audio).unwrap();

        let [before, after] = reading.timeline[..] else {
            panic!("{:?}", reading.timeline);
        };
        assert_eq!((before.key, after.key), (key("C major"), key("F# minor")));
        assert_eq!(before.start_seconds, 0.0);
        assert_eq!(after.end_seconds, audio.duration_seconds());
        assert!(
            (40.0 - window..=40.0 + window).contains(&before.end_seconds),
            "{:?}",
            reading.timeline
        );
        assert!(
            (70.0 - window..=70.0 + window).contains(&after.start_seconds),
            "{:?}",
            reading.timeline
        );
        assert!(after.start_seconds - before.end_seconds > 30.0 - 2.0 * window);
    }

    #[test]
    fn silence_inside_a_track_in_one_key_splits_it_into_two_sections_in_that_key() {
        let audio = buffer(
            [
                progression("C major", 40.0),
                silence(30.0),
                progression("C major", 40.0),
            ]
            .concat(),
        );

        let reading = KeyFinderDetector.read_key(&audio).unwrap();

        assert_eq!(reading.key, Some(key("C major")));
        let [before, after] = reading.timeline[..] else {
            panic!("{:?}", reading.timeline);
        };
        assert_eq!((before.key, after.key), (key("C major"), key("C major")));
        assert!(before.end_seconds < 55.0 && after.start_seconds > 55.0);
    }

    #[test]
    fn a_track_that_starts_or_ends_in_silence_has_no_section_there() {
        let window = window_seconds();

        let starts_silent = buffer([silence(20.0), progression("C major", 60.0)].concat());
        let reading = KeyFinderDetector.read_key(&starts_silent).unwrap();
        let [section] = reading.timeline[..] else {
            panic!("{:?}", reading.timeline);
        };
        assert_eq!(section.key, key("C major"));
        assert!((20.0 - window..=20.0 + window).contains(&section.start_seconds));
        assert_eq!(section.end_seconds, starts_silent.duration_seconds());

        let ends_silent = buffer([progression("C major", 60.0), silence(20.0)].concat());
        let reading = KeyFinderDetector.read_key(&ends_silent).unwrap();
        let [section] = reading.timeline[..] else {
            panic!("{:?}", reading.timeline);
        };
        assert_eq!(section.start_seconds, 0.0);
        assert!((60.0 - window..=60.0 + window).contains(&section.end_seconds));
    }

    #[test]
    fn a_passage_far_quieter_than_the_track_counts_as_silence() {
        let whisper: Vec<f32> = progression("F# minor", 30.0)
            .iter()
            .map(|sample| sample * 3.0e-4)
            .collect();
        let audio = buffer(
            [
                progression("C major", 40.0),
                whisper,
                progression("C major", 40.0),
            ]
            .concat(),
        );

        let reading = KeyFinderDetector.read_key(&audio).unwrap();

        assert_eq!(reading.timeline.len(), 2, "{:?}", reading.timeline);
        assert!(
            reading
                .timeline
                .iter()
                .all(|section| section.key == key("C major"))
        );
    }

    #[test]
    fn a_passage_with_sound_but_no_harmony_keeps_the_key_around_it() {
        for (label, passage) in [
            ("kick drum", kick_drum(30.0)),
            ("noise", pink_noise(30 * SAMPLE_RATE as usize, 0.3)),
        ] {
            let audio = buffer(
                [
                    progression("C major", 40.0),
                    passage,
                    progression("C major", 40.0),
                ]
                .concat(),
            );

            let reading = KeyFinderDetector.read_key(&audio).unwrap();

            assert_eq!(
                reading.timeline,
                [KeySection {
                    start_seconds: 0.0,
                    end_seconds: audio.duration_seconds(),
                    key: key("C major"),
                }],
                "{label}"
            );
        }
    }

    #[test]
    fn a_change_of_key_across_a_drum_break_falls_within_the_break() {
        let audio = buffer(
            [
                progression("C major", 40.0),
                kick_drum(30.0),
                progression("F# minor", 40.0),
            ]
            .concat(),
        );
        let window = window_seconds();

        let reading = KeyFinderDetector.read_key(&audio).unwrap();

        let [before, after] = reading.timeline[..] else {
            panic!("{:?}", reading.timeline);
        };
        assert_eq!((before.key, after.key), (key("C major"), key("F# minor")));
        assert_eq!(before.end_seconds, after.start_seconds);
        assert!((40.0 - window..=70.0 + window).contains(&after.start_seconds));
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
