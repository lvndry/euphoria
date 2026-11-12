// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Turning numbers into the text shown in the interface.

use euphoria_core::quality::{QualityReport, QualityVerdict};

/// A length in seconds as minutes and seconds, for example "6:48".
pub(crate) fn duration(seconds: f32) -> String {
    let whole_seconds = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", whole_seconds / 60, whole_seconds % 60)
}

/// A length that may run past an hour, for example "1:53:50", or "48:30"
/// under an hour.
pub(crate) fn long_duration(seconds: f32) -> String {
    let whole_seconds = seconds.max(0.0).round() as u64;
    let hours = whole_seconds / 3600;
    if hours == 0 {
        return duration(seconds);
    }
    format!(
        "{hours}:{:02}:{:02}",
        whole_seconds % 3600 / 60,
        whole_seconds % 60
    )
}

/// The time of a cue as minutes and seconds to a tenth, for example
/// "1:02.5".
pub(crate) fn cue_time(seconds: f64) -> String {
    let tenths = (seconds.max(0.0) * 10.0).round() as u64;
    format!("{}:{:02}.{}", tenths / 600, tenths / 10 % 60, tenths % 10)
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

/// A quality verdict in a word or two, for the table.
pub(crate) fn quality_verdict(verdict: QualityVerdict) -> &'static str {
    match verdict {
        QualityVerdict::Lossless => "Lossless",
        QualityVerdict::Good => "Good",
        QualityVerdict::Acceptable => "Acceptable",
        QualityVerdict::WeakOnABigSystem => "Weak",
        QualityVerdict::LikelyReencoded => "Re-encoded?",
    }
}

/// What a quality verdict rests on, for example
/// "AAC, 128 kbit/s, 44.1 kHz, 2 channels. Content up to 16.1 kHz.
/// -9.3 LUFS, true peak +0.8 dB".
pub(crate) fn quality_details(report: &QualityReport) -> String {
    let mut encoding = vec![report.codec.to_uppercase()];
    if let Some(bitrate) = report.bitrate_kbps {
        encoding.push(format!("{bitrate} kbit/s"));
    }
    encoding.push(format!("{:.1} kHz", report.sample_rate as f32 / 1000.0));
    if let Some(depth) = report.bit_depth {
        encoding.push(format!("{depth} bit"));
    }
    if let Some(channels) = report.channels {
        encoding.push(match channels {
            1 => "mono".to_string(),
            count => format!("{count} channels"),
        });
    }
    let mut details = format!("{}.", encoding.join(", "));
    details.push_str(&match report.cutoff_hz {
        Some(hz) => format!(" Content up to {:.1} kHz.", hz / 1000.0),
        None => " Content across the whole spectrum.".to_string(),
    });
    let mut levels = Vec::new();
    if let Some(lufs) = report.integrated_loudness_lufs {
        levels.push(format!("{lufs:.1} LUFS"));
    }
    if let Some(peak) = report.true_peak_dbtp {
        levels.push(format!("true peak {peak:+.1} dB"));
    }
    if !levels.is_empty() {
        details.push_str(&format!(" {}", levels.join(", ")));
    }
    details
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_durations_show_hours_once_there_are_any() {
        assert_eq!(long_duration(2_910.0), "48:30");
        assert_eq!(long_duration(6_830.0), "1:53:50");
    }

    #[test]
    fn quality_details_name_what_the_verdict_rests_on() {
        let mut report = QualityReport {
            codec: "aac".to_string(),
            lossless: false,
            bitrate_kbps: Some(128),
            sample_rate: 44_100,
            bit_depth: None,
            channels: Some(2),
            cutoff_hz: Some(16_123.0),
            clipped_share: 0.0,
            integrated_loudness_lufs: Some(-9.25),
            true_peak_dbtp: Some(0.8),
            verdict: QualityVerdict::WeakOnABigSystem,
        };
        assert_eq!(
            quality_details(&report),
            "AAC, 128 kbit/s, 44.1 kHz, 2 channels. Content up to 16.1 kHz. -9.2 LUFS, true peak +0.8 dB"
        );
        report.codec = "flac".to_string();
        report.lossless = true;
        report.bit_depth = Some(24);
        report.channels = Some(1);
        report.cutoff_hz = None;
        report.integrated_loudness_lufs = None;
        report.true_peak_dbtp = None;
        assert_eq!(
            quality_details(&report),
            "FLAC, 128 kbit/s, 44.1 kHz, 24 bit, mono. Content across the whole spectrum."
        );
        assert_eq!(
            quality_verdict(QualityVerdict::LikelyReencoded),
            "Re-encoded?"
        );
    }

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
    fn formats_cue_times_to_a_tenth_of_a_second() {
        assert_eq!(cue_time(0.0), "0:00.0");
        assert_eq!(cue_time(62.46), "1:02.5");
        assert_eq!(cue_time(59.97), "1:00.0");
        assert_eq!(cue_time(-1.0), "0:00.0");
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
