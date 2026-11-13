// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fmt;
use std::str::FromStr;

use super::SetError;

/// The lowest energy a curve can ask for, the bottom of the energy rating.
pub const LOWEST_ENERGY: f32 = 1.0;
/// The highest energy a curve can ask for, the top of the energy rating.
pub const HIGHEST_ENERGY: f32 = 10.0;

/// The energy a set should have from its first track to its last: levels
/// on the 1 to 10 scale of the energy rating, spread evenly from the start
/// of the set to its end, with straight lines between them.
#[derive(Debug, Clone, PartialEq)]
pub struct EnergyCurve {
    levels: Vec<f32>,
}

impl EnergyCurve {
    /// A curve through `levels`, the first at the start of the set and the
    /// last at its end. One level makes a flat curve.
    pub fn new(levels: Vec<f32>) -> Result<Self, SetError> {
        if levels.is_empty() {
            return Err(SetError::EmptyCurve);
        }
        if let Some(&level) = levels
            .iter()
            .find(|level| !(LOWEST_ENERGY..=HIGHEST_ENERGY).contains(*level))
        {
            return Err(SetError::CurveOutOfRange { level });
        }
        Ok(Self { levels })
    }

    /// The curve of a preset.
    pub fn preset(preset: CurvePreset) -> Self {
        Self {
            levels: preset.levels().to_vec(),
        }
    }

    /// The levels the curve goes through, from the start of the set to its
    /// end.
    pub fn levels(&self) -> &[f32] {
        &self.levels
    }

    /// The energy the curve asks for at `position`, from 0 at the start of
    /// the set to 1 at its end. Positions outside that range are read at
    /// the nearest end.
    pub fn target_at(&self, position: f32) -> f32 {
        let last = self.levels.len() - 1;
        if last == 0 || position.is_nan() {
            return self.levels[0];
        }
        let scaled = position.clamp(0.0, 1.0) * last as f32;
        let below = (scaled.floor() as usize).min(last - 1);
        let fraction = scaled - below as f32;
        self.levels[below] + (self.levels[below + 1] - self.levels[below]) * fraction
    }

    /// The energy the curve asks for at each of `count` tracks spread evenly
    /// over the set, the first at its start and the last at its end.
    pub fn targets(&self, count: usize) -> Vec<f32> {
        (0..count)
            .map(|index| self.target_at(normalised_position(index, count)))
            .collect()
    }
}

/// Where track `index` of `count` falls in the set, from 0 for the first
/// to 1 for the last.
pub fn normalised_position(index: usize, count: usize) -> f32 {
    if count <= 1 {
        0.0
    } else {
        index as f32 / (count - 1) as f32
    }
}

impl FromStr for EnergyCurve {
    type Err = SetError;

    /// Reads a preset name, such as `peak-time`, or levels separated by
    /// commas, such as `4,6,8,9,7,5`.
    fn from_str(text: &str) -> Result<Self, Self::Err> {
        if let Some(preset) = CurvePreset::from_name(text) {
            return Ok(Self::preset(preset));
        }
        let levels = text
            .split(',')
            .map(|part| {
                let part = part.trim();
                part.parse::<f32>()
                    .ok()
                    .filter(|level| level.is_finite())
                    .ok_or_else(|| SetError::UnreadableCurve {
                        text: text.to_string(),
                    })
            })
            .collect::<Result<Vec<f32>, SetError>>()?;
        Self::new(levels)
    }
}

impl fmt::Display for EnergyCurve {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let levels: Vec<String> = self.levels.iter().map(|level| level.to_string()).collect();
        formatter.write_str(&levels.join(","))
    }
}

/// A named curve for a common kind of set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CurvePreset {
    /// The opening slot: starts low and rises slowly, leaving the peak to
    /// the next DJ.
    WarmUp,
    /// The headline slot: starts high, stays near the top and peaks twice.
    PeakTime,
    /// A whole night in one set: a build, a first peak, a breather, a
    /// higher second peak and a cool down.
    Journey,
}

impl CurvePreset {
    /// Every preset.
    pub const ALL: [CurvePreset; 3] = [
        CurvePreset::WarmUp,
        CurvePreset::PeakTime,
        CurvePreset::Journey,
    ];

    /// The name a command line takes, such as `warm-up`.
    pub fn name(self) -> &'static str {
        match self {
            CurvePreset::WarmUp => "warm-up",
            CurvePreset::PeakTime => "peak-time",
            CurvePreset::Journey => "journey",
        }
    }

    /// A short name to show on a button.
    pub fn label(self) -> &'static str {
        match self {
            CurvePreset::WarmUp => "Warm up",
            CurvePreset::PeakTime => "Peak time",
            CurvePreset::Journey => "Journey",
        }
    }

    /// The preset called `name`, ignoring case, spaces and underscores.
    pub fn from_name(name: &str) -> Option<Self> {
        let wanted: String = name
            .trim()
            .chars()
            .map(|character| match character {
                ' ' | '_' => '-',
                other => other.to_ascii_lowercase(),
            })
            .collect();
        Self::ALL
            .into_iter()
            .find(|preset| preset.name() == wanted || preset.name().replace('-', "") == wanted)
    }

    /// The levels of the preset's curve.
    pub fn levels(self) -> &'static [f32] {
        match self {
            CurvePreset::WarmUp => &[3.0, 4.0, 4.0, 5.0, 5.0, 6.0, 6.0, 7.0],
            CurvePreset::PeakTime => &[7.0, 8.0, 9.0, 8.0, 9.0, 10.0, 9.0, 8.0],
            CurvePreset::Journey => &[4.0, 5.0, 7.0, 8.0, 6.0, 7.0, 9.0, 10.0, 7.0, 5.0],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_levels_and_preset_names() {
        let curve: EnergyCurve = "4, 6,8,9,7,5".parse().unwrap();
        assert_eq!(curve.levels(), [4.0, 6.0, 8.0, 9.0, 7.0, 5.0]);
        assert_eq!(curve.to_string(), "4,6,8,9,7,5");
        for preset in CurvePreset::ALL {
            let curve: EnergyCurve = preset.name().parse().unwrap();
            assert_eq!(curve, EnergyCurve::preset(preset));
        }
        assert_eq!(
            CurvePreset::from_name("Peak Time"),
            Some(CurvePreset::PeakTime)
        );
        assert_eq!(CurvePreset::from_name("warmup"), Some(CurvePreset::WarmUp));
        assert_eq!(CurvePreset::from_name("sunrise"), None);
    }

    #[test]
    fn refuses_empty_unreadable_and_out_of_range_curves() {
        assert!(matches!(
            "".parse::<EnergyCurve>(),
            Err(SetError::UnreadableCurve { .. })
        ));
        assert!(matches!(
            "4,loud,8".parse::<EnergyCurve>(),
            Err(SetError::UnreadableCurve { .. })
        ));
        assert!(matches!(
            "4,NaN".parse::<EnergyCurve>(),
            Err(SetError::UnreadableCurve { .. })
        ));
        assert!(matches!(
            "4,11".parse::<EnergyCurve>(),
            Err(SetError::CurveOutOfRange { level }) if level == 11.0
        ));
        assert!(matches!(
            EnergyCurve::new(Vec::new()),
            Err(SetError::EmptyCurve)
        ));
    }

    #[test]
    fn interpolates_between_evenly_spread_levels() {
        let curve = EnergyCurve::new(vec![2.0, 8.0, 4.0]).unwrap();
        assert_eq!(curve.target_at(0.0), 2.0);
        assert_eq!(curve.target_at(0.25), 5.0);
        assert_eq!(curve.target_at(0.5), 8.0);
        assert_eq!(curve.target_at(0.75), 6.0);
        assert_eq!(curve.target_at(1.0), 4.0);
        assert_eq!(curve.target_at(-1.0), 2.0);
        assert_eq!(curve.target_at(2.0), 4.0);
        assert_eq!(curve.targets(5), [2.0, 5.0, 8.0, 6.0, 4.0]);
    }

    #[test]
    fn one_level_is_a_flat_curve() {
        let curve = EnergyCurve::new(vec![6.0]).unwrap();
        assert_eq!(curve.targets(3), [6.0, 6.0, 6.0]);
        assert_eq!(curve.targets(1), [6.0]);
    }

    #[test]
    fn presets_stay_on_the_energy_scale() {
        for preset in CurvePreset::ALL {
            assert!(EnergyCurve::new(preset.levels().to_vec()).is_ok());
        }
    }
}
