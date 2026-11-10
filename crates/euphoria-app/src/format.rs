// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Turning numbers into the text shown in the interface.

/// A length in seconds as minutes and seconds, for example "6:48".
pub(crate) fn duration(seconds: f32) -> String {
    let whole_seconds = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", whole_seconds / 60, whole_seconds % 60)
}

/// A tempo with one decimal, for example "124.0".
pub(crate) fn bpm(beats_per_minute: f32) -> String {
    format!("{beats_per_minute:.1}")
}

/// A count with a comma between thousands, for example "1,284".
pub(crate) fn count(value: usize) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

/// A signed percentage with one decimal, for example "+1.6%" or "-0.8%".
pub(crate) fn signed_percent(percent: f32) -> String {
    if percent.abs() < 0.05 {
        return "0.0%".to_string();
    }
    format!("{percent:+.1}%")
}

/// How many tracks the table shows out of the library, and how many of the
/// library's tracks were filled in from the cache, for example
/// "48 tracks, 44 from cache" or "18 of 1,284 tracks".
pub(crate) fn track_summary(shown: usize, total: usize, from_cache: usize) -> String {
    let noun = if total == 1 { "track" } else { "tracks" };
    let mut summary = if shown == total {
        format!("{} {noun}", count(total))
    } else {
        format!("{} of {} {noun}", count(shown), count(total))
    };
    if from_cache > 0 {
        summary.push_str(&format!(", {} from cache", count(from_cache)));
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarises_the_tracks_shown_and_those_from_the_cache() {
        assert_eq!(track_summary(48, 48, 44), "48 tracks, 44 from cache");
        assert_eq!(track_summary(48, 48, 0), "48 tracks");
        assert_eq!(track_summary(18, 1284, 0), "18 of 1,284 tracks");
        assert_eq!(
            track_summary(18, 1284, 1200),
            "18 of 1,284 tracks, 1,200 from cache"
        );
        assert_eq!(track_summary(1, 1, 1), "1 track, 1 from cache");
        assert_eq!(track_summary(0, 0, 0), "0 tracks");
    }

    #[test]
    fn formats_signed_percentages() {
        assert_eq!(signed_percent(1.61), "+1.6%");
        assert_eq!(signed_percent(-0.8), "-0.8%");
        assert_eq!(signed_percent(0.01), "0.0%");
        assert_eq!(signed_percent(-0.01), "0.0%");
    }

    #[test]
    fn formats_durations_as_minutes_and_seconds() {
        assert_eq!(duration(0.0), "0:00");
        assert_eq!(duration(1.0), "0:01");
        assert_eq!(duration(408.2), "6:48");
        assert_eq!(duration(3599.6), "60:00");
    }

    #[test]
    fn formats_tempo_with_one_decimal() {
        assert_eq!(bpm(124.0), "124.0");
        assert_eq!(bpm(127.96), "128.0");
    }

    #[test]
    fn groups_thousands_with_commas() {
        assert_eq!(count(0), "0");
        assert_eq!(count(340), "340");
        assert_eq!(count(1284), "1,284");
        assert_eq!(count(1_000_000), "1,000,000");
    }
}
