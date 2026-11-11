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
pub(crate) fn steadiest_keys<const KEY_COUNT: usize>(
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
pub(crate) fn runs(keys: &[usize]) -> Vec<(Range<usize>, usize)> {
    let mut runs: Vec<(Range<usize>, usize)> = Vec::new();
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

    fn favouring(key: usize, margin: f64) -> [f64; 3] {
        let mut scores = [0.5; 3];
        scores[key] += margin;
        scores
    }

    #[test]
    fn a_lasting_change_of_key_is_followed() {
        let mut scores = vec![favouring(0, 0.2); 6];
        scores.extend(vec![favouring(2, 0.2); 6]);
        let keys = steadiest_keys(&scores, 0.5);
        assert_eq!(keys, [0, 0, 0, 0, 0, 0, 2, 2, 2, 2, 2, 2]);
        assert_eq!(runs(&keys), [(0..6, 0), (6..12, 2)]);
    }

    #[test]
    fn a_passing_change_of_key_is_ignored() {
        let mut scores = vec![favouring(0, 0.2); 6];
        scores.insert(3, favouring(1, 0.2));
        assert_eq!(steadiest_keys(&scores, 0.5), [0; 7]);
    }

    #[test]
    fn without_a_penalty_every_window_gets_its_best_key() {
        let scores = [favouring(0, 0.1), favouring(1, 0.1), favouring(0, 0.1)];
        assert_eq!(steadiest_keys(&scores, 0.0), [0, 1, 0]);
    }

    #[test]
    fn no_windows_give_no_keys() {
        assert!(steadiest_keys::<3>(&[], 0.5).is_empty());
        assert!(runs(&[]).is_empty());
        assert_eq!(highest_total::<3>(&[]), None);
    }

    #[test]
    fn the_highest_total_wins_the_vote_and_silence_has_no_winner() {
        let scores = [favouring(0, 0.3), favouring(1, 0.2), favouring(1, 0.2)];
        assert_eq!(highest_total(&scores), Some(1));
        assert_eq!(highest_total(&[[0.0; 3]; 4]), None);
    }
}
