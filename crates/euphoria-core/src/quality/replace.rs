// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The files worth replacing with a better copy: which problems a quality
//! report shows, how the files rank worst first, and how to say it in a
//! few words. The command line's `quality` command and the desktop app
//! share it, so both list the same files in the same order.

use std::cmp::Ordering;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::{NOTICEABLE_CLIPPED_SHARE, QualityReport, QualityVerdict, typical_bitrate_for_cutoff};

/// The verdict from which a file is worth replacing when nothing else is
/// asked for: the docs' "weak on a big system", and the re-encoded files
/// that are worse.
pub const DEFAULT_REPLACE_THRESHOLD: QualityVerdict = QualityVerdict::WeakOnABigSystem;

/// Why a file is worth replacing, from the worst problem to the mildest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReplaceProblem {
    /// So much audio is lost that the beat grid and cues are not written.
    HeavilyDamaged,
    /// A lossless file whose sound stops where a lossy file's would.
    FakeLossless,
    /// A lossy file whose sound stops where a lower bit rate file's would.
    Reencoded,
    /// A low bit rate lossy file, weak on a big system.
    LowBitrate,
    /// Some audio could not be decoded or is missing at the end.
    Damaged,
    /// The waveform is flattened at full scale in places.
    Clipped,
    /// A medium bit rate lossy file, listed only when the threshold asks.
    MediumBitrate,
}

impl ReplaceProblem {
    /// The problems, worst first.
    pub const ALL: [ReplaceProblem; 7] = [
        ReplaceProblem::HeavilyDamaged,
        ReplaceProblem::FakeLossless,
        ReplaceProblem::Reencoded,
        ReplaceProblem::LowBitrate,
        ReplaceProblem::Damaged,
        ReplaceProblem::Clipped,
        ReplaceProblem::MediumBitrate,
    ];

    /// A few lowercase words for a count, as in "9 fake lossless".
    pub fn label(self) -> &'static str {
        match self {
            ReplaceProblem::HeavilyDamaged => "heavily damaged",
            ReplaceProblem::FakeLossless => "fake lossless",
            ReplaceProblem::Reencoded => "re-encoded",
            ReplaceProblem::LowBitrate => "low bit rate",
            ReplaceProblem::Damaged => "damaged",
            ReplaceProblem::Clipped => "clipped",
            ReplaceProblem::MediumBitrate => "medium bit rate",
        }
    }
}

impl QualityVerdict {
    /// How far the verdict is from the best, 0 for a lossless file with a
    /// full spectrum. A limited bandwidth is information, not a defect, so
    /// it ranks next to it, above any lossy verdict.
    pub fn severity(self) -> u8 {
        match self {
            QualityVerdict::Lossless => 0,
            QualityVerdict::LimitedBandwidth => 1,
            QualityVerdict::Good => 2,
            QualityVerdict::Acceptable => 3,
            QualityVerdict::WeakOnABigSystem => 4,
            QualityVerdict::LikelyReencoded => 5,
        }
    }
}

impl QualityReport {
    /// What makes the file worth replacing, worst first, or nothing when it
    /// is fine. A verdict counts from `threshold` down; damage and clipping
    /// always count, since a better copy fixes them too.
    pub fn replace_problems(&self, threshold: QualityVerdict) -> Vec<ReplaceProblem> {
        let mut problems = Vec::new();
        if self.is_heavily_damaged() {
            problems.push(ReplaceProblem::HeavilyDamaged);
        }
        if self.verdict.severity() >= threshold.severity() {
            match self.verdict {
                QualityVerdict::LikelyReencoded if self.lossless => {
                    problems.push(ReplaceProblem::FakeLossless);
                }
                QualityVerdict::LikelyReencoded => problems.push(ReplaceProblem::Reencoded),
                QualityVerdict::WeakOnABigSystem => problems.push(ReplaceProblem::LowBitrate),
                QualityVerdict::Acceptable => problems.push(ReplaceProblem::MediumBitrate),
                QualityVerdict::Lossless
                | QualityVerdict::LimitedBandwidth
                | QualityVerdict::Good => {}
            }
        }
        if self.is_damaged() && !self.is_heavily_damaged() {
            problems.push(ReplaceProblem::Damaged);
        }
        if self.clipped_share >= NOTICEABLE_CLIPPED_SHARE {
            problems.push(ReplaceProblem::Clipped);
        }
        problems.sort();
        problems
    }

    /// The format of the file at `path` in a word or two, with the bit rate
    /// of a lossy one: "FLAC", "WAV", "MP3, 128 kbit/s".
    pub fn format_description(&self, path: &Path) -> String {
        let name = format_name(&self.codec, path);
        match self.bitrate_kbps {
            Some(rate) if !self.lossless => format!("{name}, {rate} kbit/s"),
            _ => name,
        }
    }

    /// One short, concrete sentence for `problem`, for a row of a list:
    /// "MP3, 128 kbit/s", "FLAC, but the sound stops at 16.0 kHz like a
    /// 128 kbit/s MP3", "Clipped: 0.42% of the samples are at full scale".
    /// [`QualityReport::reason`] and [`QualityReport::notes`] say more.
    pub fn problem_reason(&self, problem: ReplaceProblem, path: &Path) -> String {
        let format = self.format_description(path);
        match problem {
            ReplaceProblem::HeavilyDamaged | ReplaceProblem::Damaged => self.damage_reason(),
            ReplaceProblem::FakeLossless | ReplaceProblem::Reencoded => match self.cutoff_hz {
                Some(hz) => format!(
                    "{format}, but the sound stops at {:.1} kHz like a {} kbit/s MP3",
                    hz / 1000.0,
                    typical_bitrate_for_cutoff(hz)
                ),
                None => format!("{format}, but the sound stops early like a lower bit rate file"),
            },
            ReplaceProblem::LowBitrate | ReplaceProblem::MediumBitrate => format,
            ReplaceProblem::Clipped => format!(
                "Clipped: {} of the samples are at full scale",
                percent(self.clipped_share)
            ),
        }
    }

    fn damage_reason(&self) -> String {
        let mut parts = Vec::new();
        if self.damage.dropped_packets > 0 {
            parts.push(format!(
                "{:.1} s of audio could not be decoded",
                self.damage.concealed_seconds
            ));
        }
        let missing = self.damage.missing_seconds();
        if missing > 0.0 {
            parts.push(format!("{missing:.1} s missing at the end"));
        }
        parts.join(", and ")
    }

    /// How a lossy file's measure orders it among files with the same
    /// worst problem: more lost, an earlier cutoff, a lower bit rate or
    /// more clipping comes first.
    fn badness_within(&self, problem: ReplaceProblem) -> f32 {
        match problem {
            ReplaceProblem::HeavilyDamaged | ReplaceProblem::Damaged => -self.damage.lost_share(),
            ReplaceProblem::FakeLossless | ReplaceProblem::Reencoded => {
                self.cutoff_hz.unwrap_or(f32::MAX)
            }
            ReplaceProblem::LowBitrate | ReplaceProblem::MediumBitrate => {
                self.bitrate_kbps.map_or(f32::MAX, |rate| rate as f32)
            }
            ReplaceProblem::Clipped => -self.clipped_share,
        }
    }
}

/// Orders two reports for a list of files to replace, worst first: by their
/// worst problem, then by how bad it is, and files with no problem after
/// every file with one, the worse verdicts first. Files that compare equal
/// keep the order they came in, as with a stable sort by path.
pub fn compare_for_replacement(
    first: &QualityReport,
    second: &QualityReport,
    threshold: QualityVerdict,
) -> Ordering {
    let worst = |report: &QualityReport| report.replace_problems(threshold).first().copied();
    match (worst(first), worst(second)) {
        (Some(first_problem), Some(second_problem)) => first_problem.cmp(&second_problem).then(
            first
                .badness_within(first_problem)
                .total_cmp(&second.badness_within(second_problem)),
        ),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => second.verdict.severity().cmp(&first.verdict.severity()),
    }
}

/// How many files of a library are worth replacing, by their worst problem.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplaceSummary {
    /// The files with a quality report.
    pub track_count: usize,
    /// The files with at least one problem.
    pub to_replace_count: usize,
    /// The files whose worst problem is each one, worst first, leaving out
    /// problems no file has.
    pub by_problem: Vec<(ReplaceProblem, usize)>,
}

impl ReplaceSummary {
    /// Counts `reports` at `threshold`.
    pub fn of<'report>(
        reports: impl IntoIterator<Item = &'report QualityReport>,
        threshold: QualityVerdict,
    ) -> Self {
        let mut track_count = 0;
        let mut counts = [0_usize; ReplaceProblem::ALL.len()];
        for report in reports {
            track_count += 1;
            if let Some(worst) = report.replace_problems(threshold).first() {
                counts[*worst as usize] += 1;
            }
        }
        Self {
            track_count,
            to_replace_count: counts.iter().sum(),
            by_problem: ReplaceProblem::ALL
                .into_iter()
                .zip(counts)
                .filter(|(_, count)| *count > 0)
                .collect(),
        }
    }

    /// One line: "37 of 576 tracks to replace: 7 damaged, 9 fake lossless,
    /// 21 low bit rate."
    pub fn line(&self) -> String {
        let noun = if self.track_count == 1 {
            "track"
        } else {
            "tracks"
        };
        if self.to_replace_count == 0 {
            return format!("0 of {} {noun} to replace.", self.track_count);
        }
        let parts: Vec<String> = self
            .by_problem
            .iter()
            .map(|(problem, count)| format!("{count} {}", problem.label()))
            .collect();
        format!(
            "{} of {} {noun} to replace: {}.",
            self.to_replace_count,
            self.track_count,
            parts.join(", ")
        )
    }
}

/// The name of a format: the container for uncompressed audio, whose codec
/// says only "PCM", and the codec otherwise.
fn format_name(codec: &str, path: &Path) -> String {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    if codec.starts_with("pcm") {
        return match extension.as_deref() {
            Some("wav" | "wave") => "WAV".to_string(),
            Some("aif" | "aiff" | "aifc") => "AIFF".to_string(),
            _ => "PCM".to_string(),
        };
    }
    match codec {
        "vorbis" => "Ogg Vorbis".to_string(),
        "wavpack" => "WavPack".to_string(),
        "monkeys_audio" | "ape" => "Monkey's Audio".to_string(),
        other => other.to_uppercase(),
    }
}

fn percent(share: f32) -> String {
    let value = share * 100.0;
    if value >= 10.0 {
        format!("{value:.0}%")
    } else if value >= 1.0 {
        format!("{value:.1}%")
    } else {
        format!("{value:.2}%")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DecodeDamage;

    fn report(codec: &str, lossless: bool, bitrate: Option<u32>) -> QualityReport {
        QualityReport {
            codec: codec.to_string(),
            lossless,
            bitrate_kbps: bitrate,
            sample_rate: 44_100,
            bit_depth: lossless.then_some(16),
            channels: Some(2),
            cutoff_hz: None,
            clipped_share: 0.0,
            integrated_loudness_lufs: Some(-9.0),
            true_peak_dbtp: Some(-0.5),
            verdict: QualityVerdict::Lossless,
            damage: DecodeDamage::default(),
            cancelled_share: 0.0,
        }
    }

    fn weak_mp3(bitrate: u32) -> QualityReport {
        QualityReport {
            verdict: QualityVerdict::WeakOnABigSystem,
            ..report("mp3", false, Some(bitrate))
        }
    }

    fn fake_flac(cutoff_hz: f32) -> QualityReport {
        QualityReport {
            verdict: QualityVerdict::LikelyReencoded,
            cutoff_hz: Some(cutoff_hz),
            ..report("flac", true, Some(900))
        }
    }

    fn truncated(lost_seconds: f32) -> QualityReport {
        QualityReport {
            verdict: QualityVerdict::Good,
            damage: DecodeDamage {
                declared_seconds: Some(300.0),
                decoded_seconds: 300.0 - lost_seconds,
                ended_on_error: true,
                ..DecodeDamage::default()
            },
            ..report("mp3", false, Some(320))
        }
    }

    fn clipped(share: f32) -> QualityReport {
        QualityReport {
            clipped_share: share,
            ..report("pcm_s16le", true, Some(1411))
        }
    }

    #[test]
    fn a_clean_file_has_no_problem() {
        let lossless = report("flac", true, Some(900));
        assert!(
            lossless
                .replace_problems(DEFAULT_REPLACE_THRESHOLD)
                .is_empty()
        );
        let good = QualityReport {
            verdict: QualityVerdict::Good,
            ..report("mp3", false, Some(320))
        };
        assert!(good.replace_problems(QualityVerdict::Acceptable).is_empty());
        let limited = QualityReport {
            verdict: QualityVerdict::LimitedBandwidth,
            cutoff_hz: Some(15_000.0),
            ..lossless
        };
        assert!(
            limited
                .replace_problems(QualityVerdict::Acceptable)
                .is_empty()
        );
    }

    #[test]
    fn the_threshold_decides_which_verdicts_count() {
        let acceptable = QualityReport {
            verdict: QualityVerdict::Acceptable,
            ..report("aac", false, Some(170))
        };
        assert!(
            acceptable
                .replace_problems(DEFAULT_REPLACE_THRESHOLD)
                .is_empty()
        );
        assert_eq!(
            acceptable.replace_problems(QualityVerdict::Acceptable),
            [ReplaceProblem::MediumBitrate]
        );
        assert!(
            weak_mp3(128)
                .replace_problems(QualityVerdict::LikelyReencoded)
                .is_empty()
        );
        assert_eq!(
            fake_flac(16_000.0).replace_problems(QualityVerdict::LikelyReencoded),
            [ReplaceProblem::FakeLossless]
        );
    }

    #[test]
    fn damage_and_clipping_count_at_any_threshold() {
        assert_eq!(
            truncated(2.0).replace_problems(QualityVerdict::LikelyReencoded),
            [ReplaceProblem::Damaged]
        );
        assert_eq!(
            truncated(30.0).replace_problems(QualityVerdict::LikelyReencoded),
            [ReplaceProblem::HeavilyDamaged]
        );
        assert_eq!(
            clipped(0.004).replace_problems(QualityVerdict::LikelyReencoded),
            [ReplaceProblem::Clipped]
        );
        assert!(
            clipped(0.0005)
                .replace_problems(DEFAULT_REPLACE_THRESHOLD)
                .is_empty()
        );
        let weak_and_clipped = QualityReport {
            clipped_share: 0.01,
            ..weak_mp3(128)
        };
        assert_eq!(
            weak_and_clipped.replace_problems(DEFAULT_REPLACE_THRESHOLD),
            [ReplaceProblem::LowBitrate, ReplaceProblem::Clipped]
        );
    }

    #[test]
    fn files_rank_worst_first() {
        let fine = report("flac", true, Some(900));
        let mut reports = [
            fine.clone(),
            clipped(0.002),
            weak_mp3(128),
            clipped(0.03),
            fake_flac(16_000.0),
            truncated(2.0),
            weak_mp3(96),
            truncated(30.0),
            fake_flac(14_000.0),
            QualityReport {
                verdict: QualityVerdict::Good,
                ..report("mp3", false, Some(320))
            },
        ];
        reports.sort_by(|first, second| {
            compare_for_replacement(first, second, DEFAULT_REPLACE_THRESHOLD)
        });
        let order: Vec<String> = reports
            .iter()
            .map(|report| {
                let problem = report
                    .replace_problems(DEFAULT_REPLACE_THRESHOLD)
                    .first()
                    .map_or("none", |problem| problem.label());
                format!("{problem} {}", measure(report))
            })
            .collect();
        assert_eq!(
            order,
            [
                "heavily damaged 30",
                "fake lossless 14000",
                "fake lossless 16000",
                "low bit rate 96",
                "low bit rate 128",
                "damaged 2",
                "clipped 0.03",
                "clipped 0.002",
                "none good",
                "none lossless",
            ]
        );
    }

    /// The measure that sets a report apart from the others in the test.
    fn measure(report: &QualityReport) -> String {
        if report.is_damaged() {
            format!("{}", report.damage.missing_seconds().round())
        } else if report.clipped_share > 0.0 {
            format!("{}", report.clipped_share)
        } else if let Some(hz) = report.cutoff_hz {
            format!("{hz}")
        } else if report.verdict == QualityVerdict::WeakOnABigSystem {
            format!("{}", report.bitrate_kbps.unwrap_or_default())
        } else {
            report.verdict.label().to_lowercase()
        }
    }

    #[test]
    fn each_problem_has_a_short_concrete_reason() {
        let path = Path::new("Artist - Track.flac");
        assert_eq!(
            weak_mp3(128).problem_reason(ReplaceProblem::LowBitrate, Path::new("a.mp3")),
            "MP3, 128 kbit/s"
        );
        assert_eq!(
            fake_flac(16_000.0).problem_reason(ReplaceProblem::FakeLossless, path),
            "FLAC, but the sound stops at 16.0 kHz like a 128 kbit/s MP3"
        );
        let reencoded = QualityReport {
            verdict: QualityVerdict::LikelyReencoded,
            cutoff_hz: Some(16_400.0),
            ..report("mp3", false, Some(320))
        };
        assert_eq!(
            reencoded.problem_reason(ReplaceProblem::Reencoded, Path::new("a.mp3")),
            "MP3, 320 kbit/s, but the sound stops at 16.4 kHz like a 128 kbit/s MP3"
        );
        assert_eq!(
            clipped(0.0042).problem_reason(ReplaceProblem::Clipped, Path::new("a.wav")),
            "Clipped: 0.42% of the samples are at full scale"
        );
        assert_eq!(
            truncated(2.6).problem_reason(ReplaceProblem::Damaged, Path::new("a.mp3")),
            "2.6 s missing at the end"
        );
        let dropped = QualityReport {
            damage: DecodeDamage {
                dropped_packets: 70,
                concealed_seconds: 1.8,
                decoded_seconds: 300.0,
                ..DecodeDamage::default()
            },
            ..report("flac", true, Some(900))
        };
        assert_eq!(
            dropped.problem_reason(ReplaceProblem::Damaged, path),
            "1.8 s of audio could not be decoded"
        );
    }

    #[test]
    fn uncompressed_audio_is_named_by_its_container() {
        let wav = clipped(0.0);
        assert_eq!(wav.format_description(Path::new("a.WAV")), "WAV");
        let aiff = report("pcm_s16be", true, Some(1411));
        assert_eq!(aiff.format_description(Path::new("a.aiff")), "AIFF");
        let vorbis = report("vorbis", false, Some(160));
        assert_eq!(
            vorbis.format_description(Path::new("a.ogg")),
            "Ogg Vorbis, 160 kbit/s"
        );
    }

    #[test]
    fn the_summary_counts_each_file_once_by_its_worst_problem() {
        let reports = [
            weak_mp3(128),
            QualityReport {
                clipped_share: 0.01,
                ..weak_mp3(128)
            },
            fake_flac(16_000.0),
            truncated(2.0),
            report("flac", true, Some(900)),
        ];
        let summary = ReplaceSummary::of(&reports, DEFAULT_REPLACE_THRESHOLD);
        assert_eq!(
            summary.line(),
            "4 of 5 tracks to replace: 1 fake lossless, 2 low bit rate, 1 damaged."
        );
        let clean = ReplaceSummary::of(&reports[4..], DEFAULT_REPLACE_THRESHOLD);
        assert_eq!(clean.line(), "0 of 1 track to replace.");
    }
}
