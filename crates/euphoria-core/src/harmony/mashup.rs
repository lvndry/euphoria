// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Ranking the tracks that work layered over a chosen one, playing at the
//! same time rather than one after the other.
//!
//! Two layered tracks sound together for as long as the mashup lasts, so
//! the key has to fit far more closely than for a mix: only keys that share
//! all or six of their seven notes count, possibly after shifting the
//! candidate's pitch by a semitone or two. The candidate's tempo is
//! stretched to the chosen track's, so it has to be close enough to stretch
//! without sounding worked. When both tracks have been separated into
//! stems, the vocals of one are checked against the instruments of the
//! other: a voice over instruments that sit in the same range collides.
//!
//! Following the decks, the chosen track is called A and a candidate B.

use std::cmp::Ordering;
use std::fmt;

use super::mix::{
    BpmRange, EnergyRange, MixTrack, TempoFit, TempoScaling, TrackFeatures, pool_fits, tempo_fit,
    usable_bpm, usable_energy,
};
use super::relation::Relation;
use crate::key::Key;

/// The furthest [`MashupOptions::default`] shifts a candidate's key, in
/// semitones either way.
pub const DEFAULT_MAX_KEY_SHIFT: i8 = 2;

/// The largest tempo difference, in percent, that
/// [`MashupOptions::default`] stretches a candidate across.
pub const DEFAULT_MASHUP_TEMPO_PERCENT: f32 = 8.0;

/// The frequency bands a [`StemPresence`] measures, an octave each.
pub const STEM_BANDS: usize = 8;

/// The lower edge of each band in hertz, and the upper edge of the last:
/// octaves centred from 63 Hz to 8 kHz.
pub const STEM_BAND_EDGES_HZ: [f32; STEM_BANDS + 1] = [
    44.0, 88.0, 177.0, 355.0, 710.0, 1_420.0, 2_840.0, 5_680.0, 11_360.0,
];

/// The share of the track a stem has to sound in for a track to count as
/// having it.
const PRESENT_FRACTION: f32 = 0.2;

/// The share of the track's energy the drums, bass and other stems need
/// together for its instrumental to carry a mashup.
const FULL_INSTRUMENTAL_SHARE: f32 = 0.4;

/// How one separated stem of a track sounds over the whole track.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StemPresence {
    /// The stem's share of the energy of all four stems, 0 to 1.
    pub share: f32,
    /// The share of the track's length in which the stem is clearly heard,
    /// 0 to 1.
    pub active_fraction: f32,
    /// Where the stem's energy lies, over the octaves of
    /// [`STEM_BAND_EDGES_HZ`], summing to 1, or all 0 for a silent stem.
    pub bands: [f32; STEM_BANDS],
}

impl StemPresence {
    /// True when the stem sounds in a fair part of the track.
    pub fn is_present(&self) -> bool {
        self.active_fraction >= PRESENT_FRACTION
    }
}

/// How the four stems of a separated track sound, enough to tell whether
/// its vocals would collide with another track's instruments.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StemProfile {
    /// The drums and percussion.
    pub drums: StemPresence,
    /// The bass.
    pub bass: StemPresence,
    /// Everything that is not vocals, drums or bass: chords, leads, pads.
    pub other: StemPresence,
    /// The lead and backing vocals.
    pub vocals: StemPresence,
}

impl StemProfile {
    /// True when the track has a vocal worth lifting as an a cappella.
    pub fn has_vocals(&self) -> bool {
        self.vocals.is_present()
    }

    /// The drums, bass and other stems' share of the energy together.
    pub fn instrumental_share(&self) -> f32 {
        self.drums.share + self.bass.share + self.other.share
    }

    /// True when the track without its vocals still carries a mashup.
    pub fn has_full_instrumental(&self) -> bool {
        self.instrumental_share() >= FULL_INSTRUMENTAL_SHARE
    }
}

/// What the finder needs to know about a track: its key, tempo and energy,
/// and the profile of its stems when it has been separated.
pub trait MashupTrack: MixTrack {
    /// The profile of the track's stems, or `None` when it has not been
    /// separated.
    fn stem_profile(&self) -> Option<&StemProfile> {
        None
    }
}

impl<Track: MashupTrack + ?Sized> MashupTrack for &Track {
    fn stem_profile(&self) -> Option<&StemProfile> {
        (**self).stem_profile()
    }
}

impl MashupTrack for TrackFeatures {}

/// Which candidates count as layering over the reference track.
#[derive(Debug, Clone, PartialEq)]
pub struct MashupOptions {
    /// The key relations that count, after any shift. Only relations
    /// between keys that share most of their notes suit layering.
    pub relations: Vec<Relation>,
    /// The furthest the candidate's key may be shifted, in semitones either
    /// way. 0 allows no shift.
    pub max_key_shift: i8,
    /// The largest tempo difference the candidate is stretched across, in
    /// percent of the reference tempo.
    pub max_tempo_difference_percent: f32,
    /// Whether a candidate at about half or double the reference tempo
    /// counts.
    pub allow_half_and_double_tempo: bool,
    /// The tempo of the candidates in beats per minute, or `None` for any
    /// tempo. A candidate's own tempo must lie in the range; one without a
    /// tempo is left out, as it cannot be checked.
    pub bpm_range: Option<BpmRange>,
    /// The energy of the candidates, on the scale the tracks are rated on,
    /// or `None` for any energy. A candidate's energy must lie in the
    /// range; one without an energy is left out of a pool narrowed to one.
    pub energy_range: Option<EnergyRange>,
}

impl Default for MashupOptions {
    /// Same, relative and adjacent keys, shifting up to two semitones,
    /// within 8 percent of the reference tempo, at half or double too.
    fn default() -> Self {
        Self {
            relations: vec![Relation::Same, Relation::Relative, Relation::Adjacent],
            max_key_shift: DEFAULT_MAX_KEY_SHIFT,
            max_tempo_difference_percent: DEFAULT_MASHUP_TEMPO_PERCENT,
            allow_half_and_double_tempo: true,
            bpm_range: None,
            energy_range: None,
        }
    }
}

/// How the candidate's key fits the reference's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyFit {
    /// The candidate's own key.
    pub key: Key,
    /// Semitones to shift the candidate by, 0 for none.
    pub shift_semitones: i8,
    /// The candidate's key after the shift.
    pub shifted_key: Key,
    /// How the shifted key relates to the reference key.
    pub relation: Relation,
}

/// Which of the two tracks a part comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// The reference track, A.
    Reference,
    /// The candidate track, B.
    Candidate,
}

impl Side {
    /// The other track.
    pub fn other(self) -> Side {
        match self {
            Side::Reference => Side::Candidate,
            Side::Candidate => Side::Reference,
        }
    }

    /// The deck letter of the track: A for the reference, B for the
    /// candidate.
    pub fn letter(self) -> &'static str {
        match self {
            Side::Reference => "A",
            Side::Candidate => "B",
        }
    }
}

/// How two separated tracks layer, from their stems.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Layering {
    /// The vocals of one track over the instrumental of the other.
    VocalsOver {
        /// The track whose vocals are on top.
        vocals_from: Side,
        /// How much the vocals share their range with the instrumental's
        /// chords and leads: 0 for not at all, 1 for entirely.
        collision: f32,
        /// True when the instrumental's own vocals have to be muted for
        /// room.
        instrumental_has_vocals: bool,
    },
    /// Neither track has vocals: the instruments of both play together.
    Instrumentals {
        /// How much the two tracks' chords and leads share their range,
        /// 0 to 1.
        collision: f32,
    },
}

impl Layering {
    /// How much the layered parts share their range, 0 to 1.
    pub fn collision(&self) -> f32 {
        match self {
            Layering::VocalsOver { collision, .. } | Layering::Instrumentals { collision } => {
                *collision
            }
        }
    }
}

/// A candidate that layers over the reference track.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MashupMatch<'candidates, Track> {
    /// The candidate track.
    pub track: &'candidates Track,
    /// How its key fits, with the shift it needs.
    pub key: KeyFit,
    /// How its tempo is stretched to the reference tempo.
    pub tempo: TempoFit,
    /// The candidate's energy minus the reference's, when both are known.
    pub energy_difference: Option<f32>,
    /// How the two layer, when both have been separated into stems.
    pub layering: Option<Layering>,
    /// The cost the ranking orders by, lowest best.
    pub cost: f32,
}

impl<Track> MashupMatch<'_, Track> {
    /// Why the candidate was suggested, one reason per part of the match,
    /// key first.
    pub fn reasons(&self) -> Vec<MashupReason> {
        let mut reasons = vec![MashupReason::Key(self.key), MashupReason::Tempo(self.tempo)];
        reasons.push(match self.layering {
            Some(layering) => MashupReason::Stems(layering),
            None => MashupReason::StemsUnknown,
        });
        if let Some(difference) = self.energy_difference {
            reasons.push(MashupReason::Energy(difference));
        }
        reasons
    }
}

/// One reason a candidate layers over the reference.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MashupReason {
    /// The key fits, possibly after a shift.
    Key(KeyFit),
    /// The tempo is close enough to stretch.
    Tempo(TempoFit),
    /// How the stems of the two layer.
    Stems(Layering),
    /// At least one of the tracks has not been separated, so the stems
    /// were not compared.
    StemsUnknown,
    /// The candidate's energy minus the reference's.
    Energy(f32),
}

impl MashupReason {
    /// The reason as a sentence, naming keys with `key_name` so a front end
    /// can use the notation its user chose.
    pub fn describe(&self, key_name: impl Fn(&Key) -> String) -> String {
        match self {
            MashupReason::Key(fit) => {
                let relation = match fit.relation {
                    Relation::Same => "the same key as A".to_string(),
                    Relation::Relative => "A's relative key, with the same notes".to_string(),
                    Relation::Adjacent => "next to A's key, sharing six of seven notes".to_string(),
                    other => format!("{} with A", other.label().to_lowercase()),
                };
                if fit.shift_semitones == 0 {
                    format!("{} is {relation}", key_name(&fit.key))
                } else {
                    let semitones = fit.shift_semitones.unsigned_abs();
                    format!(
                        "Shift B {} {semitones} semitone{}: {} becomes {}, {relation}",
                        if fit.shift_semitones > 0 {
                            "up"
                        } else {
                            "down"
                        },
                        if semitones == 1 { "" } else { "s" },
                        key_name(&fit.key),
                        key_name(&fit.shifted_key),
                    )
                }
            }
            MashupReason::Tempo(fit) => {
                let difference = fit.difference_percent;
                let stretch = if difference.abs() < 0.05 {
                    "same tempo, no stretch".to_string()
                } else {
                    format!(
                        "stretched {:.1}% {} to A's tempo",
                        difference.abs(),
                        if difference > 0.0 { "down" } else { "up" }
                    )
                };
                match fit.scaling {
                    TempoScaling::Direct => capitalised(&stretch),
                    TempoScaling::HalfTempo => format!("Half tempo, counted double: {stretch}"),
                    TempoScaling::DoubleTempo => format!("Double tempo, counted half: {stretch}"),
                }
            }
            MashupReason::Stems(Layering::VocalsOver {
                vocals_from,
                collision,
                instrumental_has_vocals,
            }) => {
                let instrumental = vocals_from.other().letter();
                let overlap = describe_overlap(*collision);
                let muted = if *instrumental_has_vocals {
                    format!(", with {instrumental}'s own vocals muted")
                } else {
                    format!(", and {instrumental} has no vocals of its own")
                };
                format!(
                    "Vocals of {} over the instrumental of {instrumental}{muted}; {overlap}",
                    vocals_from.letter()
                )
            }
            MashupReason::Stems(Layering::Instrumentals { collision }) => format!(
                "Neither has vocals, so the instruments layer; {}",
                describe_overlap(*collision)
            ),
            MashupReason::StemsUnknown => {
                "Stems not compared: separate both tracks to check vocals against instruments"
                    .to_string()
            }
            MashupReason::Energy(difference) => {
                if difference.abs() < 0.5 {
                    "Same energy".to_string()
                } else {
                    format!("Energy {difference:+.0} against A")
                }
            }
        }
    }
}

impl fmt::Display for MashupReason {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.describe(Key::camelot))
    }
}

fn describe_overlap(collision: f32) -> String {
    let percent = (collision * 100.0).round();
    if collision < 0.35 {
        format!("they sit in different ranges ({percent:.0}% overlap)")
    } else if collision < 0.6 {
        format!("their ranges overlap in part ({percent:.0}%)")
    } else {
        format!("their ranges collide ({percent:.0}% overlap)")
    }
}

fn capitalised(text: &str) -> String {
    let mut characters = text.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

/// How much each part weighs in the ranking. One step of key relation, from
/// same to relative to adjacent, weighs as much as two points of tempo
/// difference in percent; a semitone of shift costs a little more, since
/// shifting changes the sound.
const RELATION_STEP: f32 = 2.0;
const SHIFT_SEMITONE: f32 = 2.5;
const TEMPO_PERCENT: f32 = 1.0;
const SCALED_TEMPO: f32 = 1.5;
const ENERGY_POINT: f32 = 0.5;
const UNKNOWN_ENERGY: f32 = 0.75;
/// A full collision of the layered ranges costs this much.
const COLLISION: f32 = 6.0;
/// Muting a track's vocals leaves traces of them behind.
const MUTED_VOCALS: f32 = 1.0;
/// Two instrumentals layered also double their drums and bass.
const INSTRUMENTALS: f32 = 2.0;
/// The cost of not knowing how the stems layer: between a good and a poor
/// layering.
const UNKNOWN_LAYERING: f32 = 3.0;

/// The candidates that layer over `reference`, best first, with the reasons
/// each one was kept in [`MashupMatch::reasons`].
///
/// A candidate is kept when its key, shifted by at most
/// [`MashupOptions::max_key_shift`] semitones, has one of
/// [`MashupOptions::relations`] to the reference key, and its tempo is
/// within [`MashupOptions::max_tempo_difference_percent`] of the reference
/// tempo. Layered tracks have to be beatmatched, so candidates without a
/// tempo are left out, as is everything when the reference has no key or
/// no tempo. Of the shifts that fit, the one with the smoothest relation
/// for the least shift is chosen.
///
/// The order weighs the key relation and shift, the stretch, how the stems
/// layer when both tracks have a [`StemProfile`], and the energy
/// difference. Candidates that tie keep the order they were given in.
/// A caller ranking a library against one of its own tracks leaves that
/// track out of `candidates`.
pub fn mashups_for<'candidates, Reference, Candidate>(
    reference: &Reference,
    candidates: impl IntoIterator<Item = &'candidates Candidate>,
    options: &MashupOptions,
) -> Vec<MashupMatch<'candidates, Candidate>>
where
    Reference: MashupTrack + ?Sized,
    Candidate: MashupTrack + 'candidates,
{
    let Some(reference_key) = reference.key() else {
        return Vec::new();
    };
    let Some(reference_bpm) = usable_bpm(reference.bpm()) else {
        return Vec::new();
    };
    let reference_energy = usable_energy(reference.energy());
    let reference_profile = reference.stem_profile();

    let mut ranked: Vec<MashupMatch<'candidates, Candidate>> = candidates
        .into_iter()
        .filter(|candidate| pool_fits(candidate, options.bpm_range, options.energy_range))
        .filter_map(|candidate| {
            let key = key_fit(&reference_key, &candidate.key()?, options)?;
            let tempo = tempo_fit(
                reference_bpm,
                usable_bpm(candidate.bpm())?,
                options.max_tempo_difference_percent,
                options.allow_half_and_double_tempo,
            )?;
            let energy_difference = usable_energy(candidate.energy())
                .zip(reference_energy)
                .map(|(candidate_energy, reference_energy)| candidate_energy - reference_energy);
            let layering = reference_profile.zip(candidate.stem_profile()).map(
                |(reference_profile, candidate_profile)| {
                    layering(reference_profile, candidate_profile)
                },
            );
            let mut found = MashupMatch {
                track: candidate,
                key,
                tempo,
                energy_difference,
                layering,
                cost: 0.0,
            };
            found.cost = cost(&found);
            Some(found)
        })
        .collect();
    ranked.sort_by(|first, second| {
        first
            .cost
            .partial_cmp(&second.cost)
            .unwrap_or(Ordering::Equal)
    });
    ranked
}

/// The best fit of `candidate` to `reference` within the shifts allowed:
/// the smoothest relation for the least shift, no shift and downward shifts
/// first among equals.
fn key_fit(reference: &Key, candidate: &Key, options: &MashupOptions) -> Option<KeyFit> {
    let reach = options.max_key_shift.clamp(0, 6);
    let mut shifts = vec![0_i8];
    for step in 1..=reach {
        shifts.push(-step);
        shifts.push(step);
    }
    shifts
        .into_iter()
        .filter_map(|shift| {
            let shifted_key = candidate.transposed(shift);
            let relation = layering_relation(reference, &shifted_key)?;
            options.relations.contains(&relation).then_some(KeyFit {
                key: *candidate,
                shift_semitones: shift,
                shifted_key,
                relation,
            })
        })
        .min_by(|first, second| key_cost(first).total_cmp(&key_cost(second)))
}

/// How two keys relate for layering, where neither comes after the other:
/// the smoother of the two directions.
fn layering_relation(first: &Key, second: &Key) -> Option<Relation> {
    match (first.relation_to(second), second.relation_to(first)) {
        (Some(forward), Some(backward)) => Some(forward.min(backward)),
        (forward, backward) => forward.or(backward),
    }
}

fn relation_rank(relation: Relation) -> f32 {
    match relation {
        Relation::Same => 0.0,
        Relation::Relative => 1.0,
        Relation::Adjacent => 2.0,
        Relation::Diagonal => 3.0,
        Relation::EnergyBoost => 4.0,
        Relation::MoodChange => 5.0,
        Relation::SemitoneBoost => 6.0,
    }
}

fn key_cost(fit: &KeyFit) -> f32 {
    relation_rank(fit.relation) * RELATION_STEP
        + f32::from(fit.shift_semitones.unsigned_abs()) * SHIFT_SEMITONE
}

/// How the two separated tracks layer best.
fn layering(reference: &StemProfile, candidate: &StemProfile) -> Layering {
    let vocals_over = |vocals_from: Side| {
        let (vocal, instrumental) = match vocals_from {
            Side::Candidate => (candidate, reference),
            Side::Reference => (reference, candidate),
        };
        (vocal.has_vocals() && instrumental.has_full_instrumental()).then(|| Layering::VocalsOver {
            vocals_from,
            collision: overlap(&vocal.vocals.bands, &instrumental.other.bands),
            instrumental_has_vocals: instrumental.has_vocals(),
        })
    };
    [Side::Candidate, Side::Reference]
        .into_iter()
        .filter_map(vocals_over)
        .min_by(|first, second| layering_cost(first).total_cmp(&layering_cost(second)))
        .unwrap_or_else(|| Layering::Instrumentals {
            collision: overlap(&reference.other.bands, &candidate.other.bands),
        })
}

/// The share of two band distributions they have in common, 0 to 1.
fn overlap(first: &[f32; STEM_BANDS], second: &[f32; STEM_BANDS]) -> f32 {
    first
        .iter()
        .zip(second)
        .map(|(first, second)| first.min(*second))
        .sum::<f32>()
        .clamp(0.0, 1.0)
}

fn layering_cost(layering: &Layering) -> f32 {
    match layering {
        Layering::VocalsOver {
            collision,
            instrumental_has_vocals,
            ..
        } => {
            collision * COLLISION
                + if *instrumental_has_vocals {
                    MUTED_VOCALS
                } else {
                    0.0
                }
        }
        Layering::Instrumentals { collision } => INSTRUMENTALS + collision * COLLISION,
    }
}

fn cost<Track>(found: &MashupMatch<'_, Track>) -> f32 {
    let tempo = found.tempo.difference_percent.abs() * TEMPO_PERCENT
        + if found.tempo.scaling == TempoScaling::Direct {
            0.0
        } else {
            SCALED_TEMPO
        };
    let energy = found
        .energy_difference
        .map_or(UNKNOWN_ENERGY, |difference| difference.abs() * ENERGY_POINT);
    let layering = found
        .layering
        .as_ref()
        .map_or(UNKNOWN_LAYERING, layering_cost);
    key_cost(&found.key) + tempo + energy + layering
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Default)]
    struct Separated {
        features: TrackFeatures,
        profile: Option<StemProfile>,
    }

    impl MixTrack for Separated {
        fn key(&self) -> Option<Key> {
            self.features.key
        }

        fn bpm(&self) -> Option<f32> {
            self.features.bpm
        }

        fn energy(&self) -> Option<f32> {
            self.features.energy
        }
    }

    impl MashupTrack for Separated {
        fn stem_profile(&self) -> Option<&StemProfile> {
            self.profile.as_ref()
        }
    }

    fn track(code: &str, bpm: f32) -> Separated {
        Separated {
            features: TrackFeatures {
                key: Some(code.parse().unwrap()),
                bpm: Some(bpm),
                energy: Some(6.0),
            },
            profile: None,
        }
    }

    fn band(index: usize) -> [f32; STEM_BANDS] {
        let mut bands = [0.0; STEM_BANDS];
        bands[index] = 1.0;
        bands
    }

    fn presence(share: f32, active_fraction: f32, centre_band: usize) -> StemPresence {
        StemPresence {
            share,
            active_fraction,
            bands: band(centre_band),
        }
    }

    /// A track whose chords and leads sit in `other_band`, with a vocal in
    /// `vocal_band` or none.
    fn profile(other_band: usize, vocal_band: Option<usize>) -> StemProfile {
        let vocals = match vocal_band {
            Some(band) => presence(0.3, 0.6, band),
            None => presence(0.0, 0.0, 0),
        };
        StemProfile {
            drums: presence(0.3, 0.9, 1),
            bass: presence(0.2, 0.9, 0),
            other: presence(0.2, 0.9, other_band),
            vocals,
        }
    }

    fn keys(ranked: &[MashupMatch<'_, Separated>]) -> Vec<(String, i8)> {
        ranked
            .iter()
            .map(|found| (found.key.key.camelot(), found.key.shift_semitones))
            .collect()
    }

    #[test]
    fn a_key_two_semitones_away_is_shifted_and_says_by_how_much() {
        let chosen = track("8A", 124.0);
        // 10A is A minor raised a whole tone: B minor. Down two is A minor.
        let candidates = [track("10A", 124.0)];
        let ranked = mashups_for(&chosen, &candidates, &MashupOptions::default());
        assert_eq!(keys(&ranked), [("10A".to_string(), -2)]);
        assert_eq!(ranked[0].key.relation, Relation::Same);
        assert_eq!(ranked[0].key.shifted_key.camelot(), "8A");
        let reason = ranked[0].reasons()[0].to_string();
        assert_eq!(
            reason,
            "Shift B down 2 semitones: 10A becomes 8A, the same key as A"
        );
    }

    #[test]
    fn a_tritone_is_one_semitone_from_an_adjacent_key_and_out_of_reach_unshifted() {
        let chosen = track("8A", 124.0);
        // 2A, E flat minor, is a tritone from A minor. One semitone down is
        // D minor, 7A, next to 8A on the wheel.
        let candidates = [track("2A", 124.0)];
        let ranked = mashups_for(&chosen, &candidates, &MashupOptions::default());
        assert_eq!(keys(&ranked), [("2A".to_string(), -1)]);
        assert_eq!(ranked[0].key.relation, Relation::Adjacent);
        let no_shift = MashupOptions {
            max_key_shift: 0,
            ..MashupOptions::default()
        };
        assert!(mashups_for(&chosen, &candidates, &no_shift).is_empty());
    }

    #[test]
    fn the_same_key_unshifted_beats_a_shifted_match() {
        let chosen = track("8A", 124.0);
        let candidates = [track("10A", 124.0), track("8A", 124.0), track("8B", 124.0)];
        let ranked = mashups_for(&chosen, &candidates, &MashupOptions::default());
        assert_eq!(
            keys(&ranked),
            [
                ("8A".to_string(), 0),
                ("8B".to_string(), 0),
                ("10A".to_string(), -2)
            ]
        );
        assert_eq!(ranked[1].key.relation, Relation::Relative);
    }

    #[test]
    fn layering_is_judged_both_ways_round() {
        // 7A over 8A is adjacent going down the wheel; the mix relation
        // names only one direction of some moves, but adjacent is both.
        let chosen = track("8A", 124.0);
        let candidates = [track("7A", 124.0)];
        let ranked = mashups_for(&chosen, &candidates, &MashupOptions::default());
        assert_eq!(ranked[0].key.shift_semitones, 0);
        assert_eq!(ranked[0].key.relation, Relation::Adjacent);
    }

    #[test]
    fn the_tempo_has_to_be_close_enough_to_stretch() {
        let chosen = track("8A", 124.0);
        let candidates = [
            track("8A", 140.0),
            track("8A", 130.0),
            track("8A", 63.0),
            Separated {
                features: TrackFeatures {
                    bpm: None,
                    ..track("8A", 0.0).features
                },
                profile: None,
            },
        ];
        let ranked = mashups_for(&chosen, &candidates, &MashupOptions::default());
        let tempos: Vec<f32> = ranked
            .iter()
            .map(|found| found.track.features.bpm.unwrap())
            .collect();
        assert_eq!(
            tempos,
            [63.0, 130.0],
            "140 is 13% off and no tempo is unusable"
        );
        assert_eq!(ranked[0].tempo.scaling, TempoScaling::HalfTempo);
        assert_eq!(
            ranked[1].reasons()[1].to_string(),
            "Stretched 4.8% down to A's tempo"
        );
        assert!(mashups_for(&track("8A", 0.0), &candidates, &MashupOptions::default()).is_empty());
    }

    #[test]
    fn a_vocal_over_an_instrumental_in_another_range_ranks_first() {
        let mut chosen = track("8A", 124.0);
        // Chosen: an instrumental whose chords sit around 355 to 710 Hz.
        chosen.profile = Some(profile(3, None));
        let mut clashing = track("8A", 124.0);
        clashing.profile = Some(profile(5, Some(3)));
        let mut clear = track("8A", 124.0);
        clear.profile = Some(profile(5, Some(5)));
        let unknown = track("8A", 124.0);
        let candidates = [clashing, unknown, clear];
        let ranked = mashups_for(&chosen, &candidates, &MashupOptions::default());
        let order: Vec<Option<usize>> = ranked
            .iter()
            .map(|found| {
                found
                    .track
                    .profile
                    .map(|profile| profile.vocals.bands.iter().position(|band| *band > 0.5))
                    .unwrap_or(None)
            })
            .collect();
        assert_eq!(order, [Some(5), None, Some(3)], "unknown sits between");
        let best = ranked[0].layering.unwrap();
        assert_eq!(
            best,
            Layering::VocalsOver {
                vocals_from: Side::Candidate,
                collision: 0.0,
                instrumental_has_vocals: false,
            }
        );
        assert_eq!(
            ranked[0].reasons()[2].to_string(),
            "Vocals of B over the instrumental of A, and A has no vocals of its own; they sit in different ranges (0% overlap)"
        );
        assert_eq!(
            ranked[1].reasons()[2],
            MashupReason::StemsUnknown,
            "a track without stems says so"
        );
    }

    #[test]
    fn the_vocal_can_come_from_the_chosen_track() {
        let mut chosen = track("8A", 124.0);
        chosen.profile = Some(profile(3, Some(6)));
        let mut instrumental = track("8A", 124.0);
        instrumental.profile = Some(profile(2, None));
        let candidates = [instrumental];
        let ranked = mashups_for(&chosen, &candidates, &MashupOptions::default());
        match ranked[0].layering {
            Some(Layering::VocalsOver {
                vocals_from,
                instrumental_has_vocals,
                ..
            }) => {
                assert_eq!(vocals_from, Side::Reference);
                assert!(!instrumental_has_vocals);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn two_instrumentals_layer_but_cost_more_than_a_vocal() {
        let mut chosen = track("8A", 124.0);
        chosen.profile = Some(profile(3, None));
        let mut instrumental = track("8A", 124.0);
        instrumental.profile = Some(profile(5, None));
        let mut vocal = track("8A", 124.0);
        vocal.profile = Some(profile(5, Some(5)));
        let candidates = [instrumental, vocal];
        let ranked = mashups_for(&chosen, &candidates, &MashupOptions::default());
        assert!(matches!(
            ranked[0].layering,
            Some(Layering::VocalsOver { .. })
        ));
        assert!(matches!(
            ranked[1].layering,
            Some(Layering::Instrumentals { collision }) if collision == 0.0
        ));
    }

    #[test]
    fn overlap_is_the_shared_part_of_two_distributions() {
        let half = [0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let shifted = [0.0, 0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0];
        assert!((overlap(&half, &shifted) - 0.5).abs() < 1e-6);
        assert!((overlap(&half, &half) - 1.0).abs() < 1e-6);
        assert_eq!(overlap(&band(0), &band(7)), 0.0);
    }

    #[test]
    fn reasons_use_the_key_notation_asked_for() {
        let chosen = track("8A", 124.0);
        let candidates = [track("9A", 124.0)];
        let ranked = mashups_for(&chosen, &candidates, &MashupOptions::default());
        // 9A is E minor; down one semitone is Eb minor, 2A, which does not
        // fit, and down seven would be A minor. Unshifted it is adjacent.
        assert_eq!(ranked[0].key.shift_semitones, 0);
        let reason = ranked[0].reasons()[0].describe(Key::open_key);
        assert_eq!(reason, "2m is next to A's key, sharing six of seven notes");
    }

    fn separated(code: &str, bpm: Option<f32>, energy: Option<f32>) -> Separated {
        Separated {
            features: TrackFeatures {
                key: Some(code.parse().unwrap()),
                bpm,
                energy,
            },
            profile: None,
        }
    }

    #[test]
    fn a_tempo_range_keeps_only_the_candidates_in_it() {
        let chosen = track("8A", 124.0);
        let candidates = [
            separated("8A", Some(118.0), Some(6.0)),
            separated("8A", Some(126.0), Some(6.0)),
            separated("8A", Some(140.0), Some(6.0)),
        ];
        let options = MashupOptions {
            bpm_range: Some(BpmRange::new(115.0, 130.0).unwrap()),
            ..MashupOptions::default()
        };
        let ranked = mashups_for(&chosen, &candidates, &options);
        assert_eq!(
            keys(&ranked),
            [("8A".to_string(), 0), ("8A".to_string(), 0)]
        );
    }

    #[test]
    fn an_energy_range_keeps_only_the_candidates_in_it() {
        let chosen = separated("8A", Some(124.0), Some(6.0));
        let candidates = [
            separated("8A", Some(124.0), Some(4.0)),
            separated("8A", Some(124.0), Some(8.0)),
            separated("8A", Some(124.0), None),
        ];
        let options = MashupOptions {
            energy_range: Some(EnergyRange::new(5.0, 10.0).unwrap()),
            ..MashupOptions::default()
        };
        let ranked = mashups_for(&chosen, &candidates, &options);
        assert_eq!(keys(&ranked), [("8A".to_string(), 0)]);
    }

    #[test]
    fn the_pool_ranges_are_open_when_the_options_do_not_name_them() {
        let chosen = separated("8A", Some(124.0), None);
        let candidates = [separated("8A", Some(124.0), None)];
        let options = MashupOptions::default();
        assert!(options.bpm_range.is_none());
        assert!(options.energy_range.is_none());
        let ranked = mashups_for(&chosen, &candidates, &options);
        assert_eq!(keys(&ranked), [("8A".to_string(), 0)]);
    }
}
