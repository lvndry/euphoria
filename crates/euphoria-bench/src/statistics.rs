// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Rank statistics for comparing an estimate with a reference.

use std::cmp::Ordering;

/// Spearman's rank correlation between two equally long lists, from -1.0 to
/// 1.0. Ties share the average of their ranks. Returns `None` when there are
/// fewer than two pairs or when one list is constant.
pub fn spearman_correlation(estimates: &[f64], references: &[f64]) -> Option<f64> {
    if estimates.len() != references.len() || estimates.len() < 2 {
        return None;
    }
    pearson_correlation(&average_ranks(estimates), &average_ranks(references))
}

/// Of the pairs whose references differ, the share that the estimates put in
/// the same order. A pair with equal estimates counts as half. Returns
/// `None` when no pair has different references.
///
/// 0.5 is what a constant or random estimate scores, 1.0 a perfect order.
pub fn ordered_pair_share(estimates: &[f64], references: &[f64]) -> Option<f64> {
    let mut comparable_pairs = 0_u64;
    let mut credit = 0.0_f64;
    for first in 0..estimates.len().min(references.len()) {
        for second in first + 1..estimates.len().min(references.len()) {
            let reference_order = references[first].total_cmp(&references[second]);
            if reference_order == Ordering::Equal {
                continue;
            }
            comparable_pairs += 1;
            let estimate_order = estimates[first].total_cmp(&estimates[second]);
            if estimate_order == reference_order {
                credit += 1.0;
            } else if estimate_order == Ordering::Equal {
                credit += 0.5;
            }
        }
    }
    (comparable_pairs > 0).then(|| credit / comparable_pairs as f64)
}

/// The value below which `fraction` of the values lie, taking the nearest
/// value at or below that position. Returns `None` for an empty list.
pub fn percentile(values: &[f64], fraction: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let position = (fraction.clamp(0.0, 1.0) * (sorted.len() - 1) as f64).floor() as usize;
    Some(sorted[position])
}

/// The arithmetic mean, or `None` for an empty list.
pub fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn average_ranks(values: &[f64]) -> Vec<f64> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|first, second| values[*first].total_cmp(&values[*second]));
    let mut ranks = vec![0.0; values.len()];
    let mut group_start = 0;
    while group_start < order.len() {
        let mut group_end = group_start + 1;
        while group_end < order.len() && values[order[group_end]] == values[order[group_start]] {
            group_end += 1;
        }
        let shared_rank = (group_start + group_end - 1) as f64 / 2.0 + 1.0;
        for index in &order[group_start..group_end] {
            ranks[*index] = shared_rank;
        }
        group_start = group_end;
    }
    ranks
}

fn pearson_correlation(first: &[f64], second: &[f64]) -> Option<f64> {
    let first_mean = mean(first)?;
    let second_mean = mean(second)?;
    let mut covariance = 0.0;
    let mut first_variance = 0.0;
    let mut second_variance = 0.0;
    for (first_value, second_value) in first.iter().zip(second) {
        let first_deviation = first_value - first_mean;
        let second_deviation = second_value - second_mean;
        covariance += first_deviation * second_deviation;
        first_variance += first_deviation * first_deviation;
        second_variance += second_deviation * second_deviation;
    }
    let scale = (first_variance * second_variance).sqrt();
    (scale > 0.0).then(|| covariance / scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spearman_is_one_for_any_rising_relation_and_minus_one_for_a_falling_one() {
        let references = [1.0, 2.0, 3.0, 4.0, 5.0];
        let rising = [1.0, 10.0, 100.0, 1_000.0, 10_000.0];
        let falling = [5.0, 4.0, 3.0, 2.0, 1.0];
        assert!((spearman_correlation(&rising, &references).unwrap() - 1.0).abs() < 1e-12);
        assert!((spearman_correlation(&falling, &references).unwrap() + 1.0).abs() < 1e-12);
    }

    #[test]
    fn spearman_gives_ties_their_average_rank() {
        // Ranks 1, 2.5, 2.5, 4 against 1, 2, 3, 4.
        let correlation = spearman_correlation(&[1.0, 2.0, 2.0, 3.0], &[1.0, 2.0, 3.0, 4.0]);
        assert!((correlation.unwrap() - 0.948_683_298).abs() < 1e-6);
    }

    #[test]
    fn spearman_is_undefined_for_constant_or_too_short_lists() {
        assert_eq!(
            spearman_correlation(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]),
            None
        );
        assert_eq!(spearman_correlation(&[1.0], &[1.0]), None);
        assert_eq!(spearman_correlation(&[1.0, 2.0], &[1.0]), None);
    }

    #[test]
    fn ordered_pair_share_counts_ties_in_the_estimate_as_half() {
        let references = [1.0, 2.0, 3.0];
        assert_eq!(ordered_pair_share(&[1.0, 2.0, 3.0], &references), Some(1.0));
        assert_eq!(ordered_pair_share(&[3.0, 2.0, 1.0], &references), Some(0.0));
        assert_eq!(ordered_pair_share(&[5.0, 5.0, 5.0], &references), Some(0.5));
        // Pairs: (1,2) tied, (1,3) right, (2,3) right.
        let share = ordered_pair_share(&[4.0, 4.0, 9.0], &references).unwrap();
        assert!((share - 2.5 / 3.0).abs() < 1e-12);
    }

    #[test]
    fn ordered_pair_share_ignores_pairs_with_equal_references() {
        assert_eq!(ordered_pair_share(&[1.0, 2.0], &[3.0, 3.0]), None);
        assert_eq!(
            ordered_pair_share(&[2.0, 1.0, 3.0], &[1.0, 1.0, 2.0]),
            Some(1.0)
        );
    }

    #[test]
    fn percentile_takes_the_value_at_or_below_the_position() {
        let values = [5.0, 1.0, 4.0, 2.0, 3.0];
        assert_eq!(percentile(&values, 0.0), Some(1.0));
        assert_eq!(percentile(&values, 0.5), Some(3.0));
        assert_eq!(percentile(&values, 0.6), Some(3.0));
        assert_eq!(percentile(&values, 1.0), Some(5.0));
        assert_eq!(percentile(&[], 0.5), None);
    }

    #[test]
    fn mean_of_nothing_is_undefined() {
        assert_eq!(mean(&[]), None);
        assert_eq!(mean(&[1.0, 2.0, 6.0]), Some(3.0));
    }
}
