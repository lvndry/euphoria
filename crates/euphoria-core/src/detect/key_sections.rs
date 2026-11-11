// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Follows the key through a track from the key scores of short windows.

use std::ops::Range;

/// The hops of each window: the track is cut into windows of about
/// `hops_per_window` hops that together cover every hop. A track shorter
/// than one window is one window.
pub(crate) fn window_hops(hop_count: usize, hops_per_window: usize) -> Vec<Range<usize>> {
    let window_count = ((hop_count + hops_per_window / 2) / hops_per_window).max(1);
    (0..window_count)
        .map(|window| hop_count * window / window_count..hop_count * (window + 1) / window_count)
        .collect()
}

/// The key of every window, as an index into its scores, chosen so that the
/// scores along the way add up to the most after `switch_penalty` is taken
/// off for every change of key. A key change therefore shows only when the
/// windows after it favour the new key by more than the penalty in total.
///
/// A window given as `None` holds no evidence of a key, such as silence. It
/// gets no key, and the windows on either side of it are followed as if
/// they were next to each other, so a change of key across it costs the
/// penalty like any other.
pub(crate) fn steadiest_keys<const KEY_COUNT: usize>(
    window_scores: &[Option<[f64; KEY_COUNT]>],
    switch_penalty: f64,
) -> Vec<Option<usize>> {
    let evidence: Vec<[f64; KEY_COUNT]> = window_scores.iter().flatten().copied().collect();
    let mut keys = steadiest_path(&evidence, switch_penalty).into_iter();
    window_scores
        .iter()
        .map(|scores| scores.and_then(|_| keys.next()))
        .collect()
}

/// What it takes for the key over time to leave the key of the track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ChangeRules {
    /// How much another key must beat the key of the track by in every
    /// window, on average over a section, for the section to be in it.
    pub(crate) per_window_margin: f64,
    /// What each change of key costs on top of that.
    pub(crate) change_penalty: f64,
    /// The fewest windows with evidence a section in another key can last.
    pub(crate) minimum_windows: usize,
}

/// The key of every window, as an index into its scores, held to
/// `track_key` unless another key is clearly and lastingly stronger.
///
/// Each window scores the track key at zero and every other key at how far
/// it beats the track key, less [`ChangeRules::per_window_margin`], and
/// [`steadiest_keys`] follows those scores with
/// [`ChangeRules::change_penalty`] for every change. A section in another
/// key shorter than [`ChangeRules::minimum_windows`] goes back to the track
/// key. A window without evidence gets no key, as in [`steadiest_keys`].
pub(crate) fn keys_against_track_key<const KEY_COUNT: usize>(
    window_scores: &[Option<[f64; KEY_COUNT]>],
    track_key: usize,
    rules: ChangeRules,
) -> Vec<Option<usize>> {
    let advantages: Vec<Option<[f64; KEY_COUNT]>> = window_scores
        .iter()
        .map(|scores| {
            scores.map(|scores| {
                let mut advantage = [0.0; KEY_COUNT];
                for (key, value) in advantage.iter_mut().enumerate() {
                    if key != track_key {
                        *value = scores[key] - scores[track_key] - rules.per_window_margin;
                    }
                }
                advantage
            })
        })
        .collect();
    let mut keys = steadiest_keys(&advantages, rules.change_penalty);

    let with_evidence: Vec<usize> = keys
        .iter()
        .enumerate()
        .filter_map(|(window, key)| key.map(|_| window))
        .collect();
    let mut run_start = 0;
    while run_start < with_evidence.len() {
        let run_key = keys[with_evidence[run_start]];
        let mut run_end = run_start;
        while run_end < with_evidence.len() && keys[with_evidence[run_end]] == run_key {
            run_end += 1;
        }
        if run_key != Some(track_key) && run_end - run_start < rules.minimum_windows {
            for window in &with_evidence[run_start..run_end] {
                keys[*window] = Some(track_key);
            }
        }
        run_start = run_end;
    }
    keys
}

/// [`steadiest_keys`] for windows that all hold evidence.
fn steadiest_path<const KEY_COUNT: usize>(
    window_scores: &[[f64; KEY_COUNT]],
    switch_penalty: f64,
) -> Vec<usize> {
    let Some((first_scores, later_scores)) = window_scores.split_first() else {
        return Vec::new();
    };
    // `totals[key]` is the best total of any way through the windows so far
    // that ends in `key`, and `arrived_from` records the key before it.
    let mut totals = *first_scores;
    let mut arrived_from: Vec<[usize; KEY_COUNT]> = Vec::with_capacity(later_scores.len());
    for scores in later_scores {
        let best_previous = best_index(&totals);
        let total_after_switch = totals[best_previous] - switch_penalty;
        let mut previous_keys = [0; KEY_COUNT];
        for key in 0..KEY_COUNT {
            if total_after_switch > totals[key] {
                previous_keys[key] = best_previous;
                totals[key] = total_after_switch;
            } else {
                previous_keys[key] = key;
            }
            totals[key] += scores[key];
        }
        arrived_from.push(previous_keys);
    }

    let mut keys = vec![best_index(&totals)];
    for previous_keys in arrived_from.iter().rev() {
        let next_key = keys[keys.len() - 1];
        keys.push(previous_keys[next_key]);
    }
    keys.reverse();
    keys
}

/// The key whose scores add up to the most over every window, or `None`
/// when no key scores above zero.
pub(crate) fn highest_total<const KEY_COUNT: usize>(
    window_scores: &[[f64; KEY_COUNT]],
) -> Option<usize> {
    let mut totals = [0.0; KEY_COUNT];
    for scores in window_scores {
        for (total, score) in totals.iter_mut().zip(scores) {
            *total += score;
        }
    }
    let best = best_index(&totals);
    (totals[best] > 0.0).then_some(best)
}

/// The index of the highest value, the first one among equals.
pub(crate) fn best_index<const KEY_COUNT: usize>(values: &[f64; KEY_COUNT]) -> usize {
    let mut best = 0;
    for (index, value) in values.iter().enumerate() {
        if *value > values[best] {
            best = index;
        }
    }
    best
}

/// Runs of windows with the same key, as the range of windows and the key.
pub(crate) fn runs<Value: PartialEq + Copy>(keys: &[Value]) -> Vec<(Range<usize>, Value)> {
    let mut runs: Vec<(Range<usize>, Value)> = Vec::new();
    for (window, &key) in keys.iter().enumerate() {
        match runs.last_mut() {
            Some((windows, run_key)) if *run_key == key => windows.end = window + 1,
            _ => runs.push((window..window + 1, key)),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_cover_every_hop_once_and_are_about_the_asked_length() {
        for hop_count in [1, 3, 7, 8, 11, 12, 13, 64, 129, 1_000] {
            let windows = window_hops(hop_count, 8);
            assert_eq!(windows[0].start, 0);
            assert_eq!(windows[windows.len() - 1].end, hop_count);
            for pair in windows.windows(2) {
                assert_eq!(pair[0].end, pair[1].start);
            }
            if hop_count >= 8 {
                for window in &windows {
                    assert!((6..=11).contains(&window.len()), "{hop_count}: {window:?}");
                }
            }
        }
        assert_eq!(window_hops(0, 8), vec![0..0]);
        assert_eq!(window_hops(3, 8), vec![0..3]);
    }

    fn favouring(key: usize, margin: f64) -> Option<[f64; 3]> {
        let mut scores = [0.5; 3];
        scores[key] += margin;
        Some(scores)
    }

    #[test]
    fn a_lasting_change_of_key_is_followed() {
        let mut scores = vec![favouring(0, 0.2); 6];
        scores.extend(vec![favouring(2, 0.2); 6]);
        let keys = steadiest_keys(&scores, 0.5);
        let expected: Vec<Option<usize>> = [0, 0, 0, 0, 0, 0, 2, 2, 2, 2, 2, 2]
            .into_iter()
            .map(Some)
            .collect();
        assert_eq!(keys, expected);
        assert_eq!(runs(&keys), [(0..6, Some(0)), (6..12, Some(2))]);
    }

    #[test]
    fn a_passing_change_of_key_is_ignored() {
        let mut scores = vec![favouring(0, 0.2); 6];
        scores.insert(3, favouring(1, 0.2));
        assert_eq!(steadiest_keys(&scores, 0.5), [Some(0); 7]);
    }

    #[test]
    fn without_a_penalty_every_window_gets_its_best_key() {
        let scores = [favouring(0, 0.1), favouring(1, 0.1), favouring(0, 0.1)];
        assert_eq!(steadiest_keys(&scores, 0.0), [Some(0), Some(1), Some(0)]);
    }

    #[test]
    fn no_windows_give_no_keys() {
        assert!(steadiest_keys::<3>(&[], 0.5).is_empty());
        assert!(runs::<usize>(&[]).is_empty());
        assert_eq!(highest_total::<3>(&[]), None);
    }

    #[test]
    fn windows_without_evidence_get_no_key_and_keep_their_place() {
        let scores = [None, favouring(2, 0.2), None, None, favouring(2, 0.2), None];
        assert_eq!(
            steadiest_keys(&scores, 0.5),
            [None, Some(2), None, None, Some(2), None]
        );
        assert_eq!(
            runs(&steadiest_keys(&scores, 0.5)),
            [
                (0..1, None),
                (1..2, Some(2)),
                (2..4, None),
                (4..5, Some(2)),
                (5..6, None)
            ]
        );
        assert_eq!(steadiest_keys::<3>(&[None, None], 0.5), [None, None]);
    }

    #[test]
    fn a_gap_without_evidence_does_not_make_a_change_of_key_free() {
        let mut scores = vec![favouring(0, 0.2); 6];
        scores.extend([None; 4]);
        scores.push(favouring(1, 0.2));
        scores.extend(vec![favouring(0, 0.2); 6]);
        let keys = steadiest_keys(&scores, 0.5);
        assert!(
            keys.iter().flatten().all(|key| *key == 0),
            "one window in another key after a gap is not worth a change: {keys:?}"
        );
    }

    const RULES: ChangeRules = ChangeRules {
        per_window_margin: 0.025,
        change_penalty: 0.05,
        minimum_windows: 3,
    };

    /// Scores in which `stronger` beats key 0, the track key, by `margin`.
    fn ahead_of_the_track_key(stronger: usize, margin: f64) -> Option<[f64; 3]> {
        let mut scores = [0.7, 0.6, 0.6];
        scores[stronger] = 0.7 + margin;
        Some(scores)
    }

    #[test]
    fn a_key_only_slightly_ahead_of_the_track_key_does_not_take_over() {
        let mut scores = vec![ahead_of_the_track_key(0, 0.0); 4];
        scores.extend(vec![ahead_of_the_track_key(1, 0.02); 10]);
        assert_eq!(
            keys_against_track_key(&scores, 0, RULES),
            [Some(0); 14],
            "ten windows 0.02 ahead are within what real music does without changing key"
        );
    }

    #[test]
    fn a_key_clearly_ahead_for_long_enough_takes_over() {
        let mut scores = vec![ahead_of_the_track_key(0, 0.0); 6];
        scores.extend(vec![ahead_of_the_track_key(2, 0.045); 3]);
        let keys = keys_against_track_key(&scores, 0, RULES);
        assert_eq!(
            runs(&keys),
            [(0..6, Some(0)), (6..9, Some(2))],
            "three windows of a modulation a fifth away are kept"
        );
    }

    #[test]
    fn a_short_stretch_in_another_key_goes_back_to_the_track_key() {
        let mut scores = vec![ahead_of_the_track_key(0, 0.0); 5];
        scores.extend(vec![ahead_of_the_track_key(1, 0.2); 2]);
        scores.extend(vec![ahead_of_the_track_key(0, 0.0); 5]);
        assert_eq!(keys_against_track_key(&scores, 0, RULES), [Some(0); 12]);
    }

    #[test]
    fn the_minimum_counts_windows_with_evidence_and_keeps_gaps_without_it() {
        let scores = [
            ahead_of_the_track_key(0, 0.0),
            ahead_of_the_track_key(1, 0.2),
            None,
            ahead_of_the_track_key(1, 0.2),
            ahead_of_the_track_key(1, 0.2),
            None,
        ];
        assert_eq!(
            keys_against_track_key(&scores, 0, RULES),
            [Some(0), Some(1), None, Some(1), Some(1), None]
        );
        let shorter = [
            ahead_of_the_track_key(0, 0.0),
            ahead_of_the_track_key(1, 0.2),
            None,
            ahead_of_the_track_key(1, 0.2),
        ];
        assert_eq!(
            keys_against_track_key(&shorter, 0, RULES),
            [Some(0), Some(0), None, Some(0)]
        );
    }

    #[test]
    fn windows_that_score_zero_for_every_key_keep_the_key_around_them() {
        let mut scores = vec![favouring(2, 0.2); 3];
        scores.extend([Some([0.0; 3]); 4]);
        scores.extend(vec![favouring(2, 0.2); 3]);
        assert_eq!(steadiest_keys(&scores, 0.5), [Some(2); 10]);
    }

    #[test]
    fn the_highest_total_wins_the_vote_and_silence_has_no_winner() {
        let scores = [favouring(0, 0.3), favouring(1, 0.2), favouring(1, 0.2)].map(Option::unwrap);
        assert_eq!(highest_total(&scores), Some(1));
        assert_eq!(highest_total(&[[0.0; 3]; 4]), None);
    }
}
