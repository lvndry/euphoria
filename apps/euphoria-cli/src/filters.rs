// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The filters `analyze` and `next` share: tempo, energy and sound
//! quality. The matching is the core's, the same the desktop app's filter
//! bar uses; this module reads the flags and describes them.

use anyhow::{Result, anyhow};
use clap::{Args, ValueEnum};
use euphoria_core::quality::{QualityReport, QualityVerdict};
use euphoria_core::{
    FilterableTrack, LibraryDistribution, SetStage, TrackAnalysis, TrackFilters, stage_of,
};

#[derive(Debug, Clone, Default, Args)]
pub struct FilterArguments {
    /// List only the tracks with one of these sound qualities, separated by
    /// commas, such as weak,reencoded to find the files to replace.
    #[arg(long, value_enum, value_delimiter = ',', value_name = "QUALITY")]
    pub quality: Vec<QualityChoice>,
    /// List only the tracks in this tempo range, in beats per minute: 120-126,
    /// 120- for 120 and faster or -126 for 126 and slower. A comma works as a
    /// decimal point, as in 120,5-126. A tempo you set yourself is the one
    /// used.
    #[arg(long, value_name = "RANGE", allow_hyphen_values = true)]
    pub bpm: Option<String>,
    /// With --bpm, also list the tracks at half or double a tempo in the
    /// range, such as 70 for 140.
    #[arg(long, requires = "bpm")]
    pub half_double: bool,
    /// List only the tracks in this energy range, from 1 to 10: 6-8, 6- or
    /// -8. An energy you set yourself is the one used.
    #[arg(long, value_name = "RANGE", allow_hyphen_values = true)]
    pub energy: Option<String>,
}

impl FilterArguments {
    /// The filters the flags ask for.
    pub fn track_filters(&self) -> Result<TrackFilters> {
        let mut filters = TrackFilters::default();
        if let Some(text) = &self.bpm {
            filters = filters
                .with_bpm_text(text)
                .map_err(|problem| anyhow!("--bpm {text}: {problem}"))?;
            filters.half_or_double = self.half_double;
        }
        if let Some(text) = &self.energy {
            filters = filters
                .with_energy_text(text)
                .map_err(|problem| anyhow!("--energy {text}: {problem}"))?;
        }
        filters.qualities = self
            .quality
            .iter()
            .map(|choice| euphoria_core::QualityChoice::Verdict(choice.verdict()))
            .collect();
        Ok(filters)
    }

    /// The sound qualities asked for.
    pub fn verdicts(&self) -> Vec<QualityVerdict> {
        self.quality.iter().map(|choice| choice.verdict()).collect()
    }

    /// What the tempo and energy flags narrow to, in a few words each.
    pub fn describe_ranges(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if let Some(text) = &self.bpm {
            parts.push(format!(
                "a tempo of {text} BPM{}",
                if self.half_double {
                    ", or half or double that"
                } else {
                    ""
                }
            ));
        }
        if let Some(text) = &self.energy {
            parts.push(format!("an energy of {text}"));
        }
        parts
    }
}

/// An analysis, with its corrected values and its set stage, as a filter
/// reads it. `next` has no stages and passes `None`.
pub struct Filtered<'analysis>(pub &'analysis TrackAnalysis, pub Option<SetStage>);

impl FilterableTrack for Filtered<'_> {
    fn is_analyzed(&self) -> bool {
        true
    }

    fn bpm(&self) -> Option<f32> {
        self.0.bpm
    }

    fn energy(&self) -> Option<u8> {
        self.0.energy
    }

    fn quality(&self) -> Option<&QualityReport> {
        self.0.quality.as_ref()
    }

    fn stage(&self) -> Option<SetStage> {
        self.1
    }
}

/// A set stage to list with `--stage`. Closing is not one: the tracks that
/// close a set cannot be told from the rest by tempo and loudness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StageChoice {
    /// Slower and quieter than the rest of the library.
    #[value(name = "warm-up", alias = "warmup")]
    WarmUp,
    /// Between the warm-up and the peak.
    Build,
    /// Among the fastest and loudest tracks of the library.
    Peak,
}

impl StageChoice {
    pub fn stage(self) -> SetStage {
        match self {
            StageChoice::WarmUp => SetStage::WarmUp,
            StageChoice::Build => SetStage::Build,
            StageChoice::Peak => SetStage::Peak,
        }
    }
}

/// The set stage of each analyzed track, placed among the tracks analyzed
/// together.
#[derive(Debug, Default)]
pub struct LibraryStages {
    by_path: std::collections::HashMap<std::path::PathBuf, SetStage>,
}

impl LibraryStages {
    /// The stages of the analyzed files in `results`. Tempos are the ones
    /// the user corrected to, and fewer than ten tracks with a tempo have
    /// no stages.
    pub fn of(results: &[euphoria_core::FileResult]) -> Self {
        let analyses: Vec<(&std::path::Path, &TrackAnalysis)> = results
            .iter()
            .filter_map(|result| {
                result
                    .outcome
                    .as_ref()
                    .ok()
                    .map(|analysis| (result.path.as_path(), analysis))
            })
            .collect();
        let library = LibraryDistribution::from_features(
            analyses
                .iter()
                .map(|(_, analysis)| analysis.stage_features()),
        );
        let by_path = analyses
            .iter()
            .filter_map(|(path, analysis)| {
                stage_of(&analysis.stage_features(), &library)
                    .map(|stage| (path.to_path_buf(), stage))
            })
            .collect();
        Self { by_path }
    }

    pub fn get(&self, path: &std::path::Path) -> Option<SetStage> {
        self.by_path.get(path).copied()
    }
}

/// A sound quality to list with `--quality`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum QualityChoice {
    /// Lossless files with a full spectrum.
    Lossless,
    /// Lossless files whose content stops below 19 kHz with a gradual roll-off.
    #[value(alias = "limited-bandwidth")]
    Limited,
    /// High bit rate files with the spectrum that bit rate keeps.
    Good,
    /// Medium bit rate files, fine in most rooms.
    Acceptable,
    /// Low bit rate files, whose top end suffers on a big system.
    #[value(alias = "weak-on-a-big-system")]
    Weak,
    /// Files whose sound stops where a lower quality file's would.
    #[value(alias = "likely-reencoded", alias = "re-encoded")]
    Reencoded,
}

impl QualityChoice {
    pub fn verdict(self) -> QualityVerdict {
        match self {
            QualityChoice::Lossless => QualityVerdict::Lossless,
            QualityChoice::Limited => QualityVerdict::LimitedBandwidth,
            QualityChoice::Good => QualityVerdict::Good,
            QualityChoice::Acceptable => QualityVerdict::Acceptable,
            QualityChoice::Weak => QualityVerdict::WeakOnABigSystem,
            QualityChoice::Reencoded => QualityVerdict::LikelyReencoded,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_quality_choice_names_its_own_verdict() {
        let verdicts: Vec<QualityVerdict> = QualityChoice::value_variants()
            .iter()
            .map(|choice| choice.verdict())
            .collect();
        for verdict in [
            QualityVerdict::Lossless,
            QualityVerdict::LimitedBandwidth,
            QualityVerdict::Good,
            QualityVerdict::Acceptable,
            QualityVerdict::WeakOnABigSystem,
            QualityVerdict::LikelyReencoded,
        ] {
            assert!(verdicts.contains(&verdict), "{verdict:?}");
        }
        assert_eq!(
            QualityChoice::from_str("weak-on-a-big-system", false).unwrap(),
            QualityChoice::Weak
        );
    }
    fn arguments(bpm: Option<&str>, energy: Option<&str>, half_double: bool) -> FilterArguments {
        FilterArguments {
            bpm: bpm.map(str::to_string),
            energy: energy.map(str::to_string),
            half_double,
            ..FilterArguments::default()
        }
    }

    #[test]
    fn the_flags_become_the_core_filters() {
        let filters = arguments(Some("120,5-126"), Some("6-"), true)
            .track_filters()
            .unwrap();
        assert_eq!(
            (filters.bpm_min, filters.bpm_max),
            (Some(120.5), Some(126.0))
        );
        assert_eq!((filters.energy_min, filters.energy_max), (Some(6), None));
        assert!(filters.half_or_double);
        assert!(
            !arguments(None, None, false)
                .track_filters()
                .unwrap()
                .is_active()
        );
    }

    #[test]
    fn a_range_that_does_not_read_names_the_flag_and_how_to_write_it() {
        let message = arguments(Some("fast"), None, false)
            .track_filters()
            .unwrap_err()
            .to_string();
        assert!(
            message.starts_with("--bpm fast: a tempo range is written 120-126"),
            "{message}"
        );
        let message = arguments(None, Some("11"), false)
            .track_filters()
            .unwrap_err()
            .to_string();
        assert!(message.starts_with("--energy 11:"), "{message}");
    }
}
