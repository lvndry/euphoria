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

/// What it takes for the key over time to leave the key of the track.
///
/// A section in another key is a stretch of at least
/// [`ChangeRules::minimum_windows`] windows in a row, with no silence in
/// it, over which that key beats the key of the track by more than
/// [`ChangeRules::per_window_margin`] in every window on average, and by
/// more than [`ChangeRules::section_cost`] in total beyond that.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ChangeRules {
    /// How much another key must beat the key of the track by in each
    /// window, on average over a section.
    pub(crate) per_window_margin: f64,
    /// What each section in another key costs, once, wherever it is in the
    /// track. Going back to the key of the track costs nothing.
    pub(crate) section_cost: f64,
    /// The fewest windows in a row a section in another key lasts.
    pub(crate) minimum_windows: usize,
}

/// The key of every window, as an index into its scores, held to
/// `track_key` except where [`ChangeRules`] finds a section in another key.
///
/// A window given as `None` holds no evidence of a key, such as silence. It
/// gets no key and ends any section in another key, so each stretch of
/// windows with sound between two silences is followed on its own.
pub(crate) fn keys_against_track_key<const KEY_COUNT: usize>(
    window_scores: &[Option<[f64; KEY_COUNT]>],
    track_key: usize,
    rules: ChangeRules,
) -> Vec<Option<usize>> {
    let mut keys = vec![None; window_scores.len()];
    let mut stretch_start = 0;
    while stretch_start < window_scores.len() {
        if window_scores[stretch_start].is_none() {
            stretch_start += 1;
            continue;
        }
        let mut stretch_end = stretch_start;
        while stretch_end < window_scores.len() && window_scores[stretch_end].is_some() {
            stretch_end += 1;
        }
        let advantages: Vec<[f64; KEY_COUNT]> = window_scores[stretch_start..stretch_end]
            .iter()
            .flatten()
            .map(|scores| advantage_over_track_key(scores, track_key, rules.per_window_margin))
            .collect();
        let mut path = path_from_track_key(&advantages, track_key, rules.section_cost);
        drop_short_sections(&mut path, track_key, rules.minimum_windows);
        for (offset, key) in path.into_iter().enumerate() {
            keys[stretch_start + offset] = Some(key);
        }
        stretch_start = stretch_end;
    }
    keys
}

/// Zero for the track key, and for every other key how far it beats the
/// track key, less `per_window_margin`.
fn advantage_over_track_key<const KEY_COUNT: usize>(
    scores: &[f64; KEY_COUNT],
    track_key: usize,
    per_window_margin: f64,
) -> [f64; KEY_COUNT] {
    let mut advantage = [0.0; KEY_COUNT];
    for (key, value) in advantage.iter_mut().enumerate() {
        if key != track_key {
            *value = scores[key] - scores[track_key] - per_window_margin;
        }
    }
    advantage
}

/// The key of every window, chosen so that the advantages along the way add
/// up to the most after `section_cost` is taken off each time the path
/// enters a key other than `track_key`, including at the first window.
fn path_from_track_key<const KEY_COUNT: usize>(
    advantages: &[[f64; KEY_COUNT]],
    track_key: usize,
    section_cost: f64,
) -> Vec<usize> {
    let entry_cost = |key: usize| if key == track_key { 0.0 } else { section_cost };
    let Some((first, later)) = advantages.split_first() else {
        return Vec::new();
    };
    // `totals[key]` is the best total of any way through the windows so far
    // that ends in `key`, and `arrived_from` records the key before it.
    let mut totals = [0.0; KEY_COUNT];
    for (key, total) in totals.iter_mut().enumerate() {
        *total = first[key] - entry_cost(key);
    }
    let mut arrived_from: Vec<[usize; KEY_COUNT]> = Vec::with_capacity(later.len());
    for advantage in later {
        let previous_totals = totals;
        let mut previous_keys = [0; KEY_COUNT];
        for key in 0..KEY_COUNT {
            let mut best_total = previous_totals[key];
            let mut best_previous = key;
            for (previous, previous_total) in previous_totals.iter().enumerate() {
                let total = previous_total - entry_cost(key);
                if previous != key && total > best_total {
                    best_total = total;
                    best_previous = previous;
                }
            }
            totals[key] = best_total + advantage[key];
            previous_keys[key] = best_previous;
        }
        arrived_from.push(previous_keys);
    }

    let mut path = vec![best_index(&totals)];
    for previous_keys in arrived_from.iter().rev() {
        let next_key = path[path.len() - 1];
        path.push(previous_keys[next_key]);
    }
    path.reverse();
    path
}

/// Gives a section in another key that lasts fewer than `minimum_windows`
/// windows back to `track_key`.
fn drop_short_sections(path: &mut [usize], track_key: usize, minimum_windows: usize) {
    for (windows, key) in runs(path) {
        if key != track_key && windows.len() < minimum_windows {
            path[windows].fill(track_key);
        }
    }
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

    const RULES: ChangeRules = ChangeRules {
        per_window_margin: 0.025,
        section_cost: 0.05,
        minimum_windows: 3,
    };

    /// Scores in which `stronger` beats key 0, the track key, by `margin`.
    fn ahead_of_the_track_key(stronger: usize, margin: f64) -> Option<[f64; 3]> {
        let mut scores = [0.7, 0.6, 0.6];
        scores[stronger] = 0.7 + margin;
        Some(scores)
    }

    fn track_key_windows(count: usize) -> Vec<Option<[f64; 3]>> {
        vec![ahead_of_the_track_key(0, 0.0); count]
    }

    /// Three windows of a modulation a fifth away: about 0.045 per window.
    fn fifth_away_for_three_windows() -> Vec<Option<[f64; 3]>> {
        vec![ahead_of_the_track_key(2, 0.045); 3]
    }

    #[test]
    fn a_key_only_slightly_ahead_of_the_track_key_does_not_take_over() {
        let mut scores = track_key_windows(4);
        scores.extend(vec![ahead_of_the_track_key(1, 0.02); 10]);
        assert_eq!(
            keys_against_track_key(&scores, 0, RULES),
            [Some(0); 14],
            "ten windows 0.02 ahead are within what real music does without changing key"
        );
    }

    #[test]
    fn three_windows_clearly_ahead_show_at_the_start_in_the_middle_and_at_the_end() {
        let at_the_end = [track_key_windows(6), fifth_away_for_three_windows()].concat();
        assert_eq!(
            runs(&keys_against_track_key(&at_the_end, 0, RULES)),
            [(0..6, Some(0)), (6..9, Some(2))]
        );

        let in_the_middle = [
            track_key_windows(6),
            fifth_away_for_three_windows(),
            track_key_windows(6),
        ]
        .concat();
        assert_eq!(
            runs(&keys_against_track_key(&in_the_middle, 0, RULES)),
            [(0..6, Some(0)), (6..9, Some(2)), (9..15, Some(0))],
            "a section surrounded by the track key pays for itself once, as one at the end does"
        );

        let at_the_start = [fifth_away_for_three_windows(), track_key_windows(6)].concat();
        assert_eq!(
            runs(&keys_against_track_key(&at_the_start, 0, RULES)),
            [(0..3, Some(2)), (3..9, Some(0))]
        );
    }

    #[test]
    fn a_section_must_beat_the_track_key_by_more_than_the_margin_and_its_cost() {
        // 3 x (0.04 - 0.025) = 0.045, short of the cost of 0.05.
        let just_short = [
            track_key_windows(4),
            vec![ahead_of_the_track_key(2, 0.04); 3],
            track_key_windows(4),
        ]
        .concat();
        assert_eq!(keys_against_track_key(&just_short, 0, RULES), [Some(0); 11]);
        // 4 x (0.04 - 0.025) = 0.06, over the cost.
        let long_enough = [
            track_key_windows(4),
            vec![ahead_of_the_track_key(2, 0.04); 4],
            track_key_windows(4),
        ]
        .concat();
        assert_eq!(
            runs(&keys_against_track_key(&long_enough, 0, RULES)),
            [(0..4, Some(0)), (4..8, Some(2)), (8..12, Some(0))]
        );
    }

    #[test]
    fn a_short_stretch_in_another_key_goes_back_to_the_track_key() {
        let scores = [
            track_key_windows(5),
            vec![ahead_of_the_track_key(1, 0.2); 2],
            track_key_windows(5),
        ]
        .concat();
        assert_eq!(keys_against_track_key(&scores, 0, RULES), [Some(0); 12]);
    }

    #[test]
    fn silence_ends_a_section_so_its_windows_do_not_add_up_across_it() {
        let pieced_together = [
            ahead_of_the_track_key(0, 0.0),
            ahead_of_the_track_key(1, 0.2),
            None,
            None,
            None,
            ahead_of_the_track_key(1, 0.2),
            ahead_of_the_track_key(1, 0.2),
            ahead_of_the_track_key(0, 0.0),
        ];
        assert_eq!(
            keys_against_track_key(&pieced_together, 0, RULES),
            [
                Some(0),
                Some(0),
                None,
                None,
                None,
                Some(0),
                Some(0),
                Some(0)
            ],
            "one window, a silence and two windows are not a section of three"
        );

        let either_side = [
            vec![ahead_of_the_track_key(1, 0.2); 3],
            vec![None; 2],
            vec![ahead_of_the_track_key(1, 0.2); 3],
        ]
        .concat();
        assert_eq!(
            runs(&keys_against_track_key(&either_side, 0, RULES)),
            [(0..3, Some(1)), (3..5, None), (5..8, Some(1))],
            "a stretch long enough on each side of a silence is a section on each side"
        );
    }

    #[test]
    fn windows_without_evidence_get_no_key_and_keep_their_place() {
        let scores = [
            None,
            ahead_of_the_track_key(0, 0.0),
            None,
            ahead_of_the_track_key(0, 0.0),
        ];
        assert_eq!(
            keys_against_track_key(&scores, 0, RULES),
            [None, Some(0), None, Some(0)]
        );
        assert_eq!(
            keys_against_track_key::<3>(&[None, None], 0, RULES),
            [None, None]
        );
        assert!(keys_against_track_key::<3>(&[], 0, RULES).is_empty());
    }

    #[test]
    fn windows_that_score_zero_for_every_key_keep_the_track_key() {
        let scores = [
            track_key_windows(3),
            vec![Some([0.0; 3]); 4],
            track_key_windows(3),
        ]
        .concat();
        assert_eq!(keys_against_track_key(&scores, 0, RULES), [Some(0); 10]);
    }

    #[test]
    fn runs_group_equal_neighbours() {
        assert!(runs::<usize>(&[]).is_empty());
        assert_eq!(
            runs(&[Some(1), Some(1), None, Some(1)]),
            [(0..2, Some(1)), (2..3, None), (3..4, Some(1))]
        );
    }

    #[test]
    fn the_highest_total_wins_the_vote_and_silence_has_no_winner() {
        let scores = [[0.8, 0.5, 0.5], [0.5, 0.7, 0.5], [0.5, 0.7, 0.5]];
        assert_eq!(highest_total(&scores), Some(1));
        assert_eq!(highest_total(&[[0.0; 3]; 4]), None);
        assert_eq!(highest_total::<3>(&[]), None);
    }
}
