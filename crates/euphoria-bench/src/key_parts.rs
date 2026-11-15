// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Compares what libkeyfinder does in Euphoria with what replaces it: the
//! key over time, now followed by drone-v2, the pitch-class profile, now
//! measured by Euphoria's own note spectrum, the key engine and the key of
//! audio too short for drone-v2, and the tuning correction. Prints a
//! Markdown report with one row per genre.

use std::collections::BTreeMap;
use std::f32::consts::TAU;
use std::fs;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use cpu_time::ThreadTime;
use euphoria_core::detect::{
    DroneDetector, KeyFinderDetector, KeySection, measure_tuning, read_key,
};
use euphoria_core::{AudioBuffer, Key, KeyEngine, Mode, decode_mono, pitch_class_profile};
use rayon::prelude::*;

use crate::dataset::load_key_tracks;
use crate::scoring::{KeyRelation, key_relation};

/// Usage of the key parts mode.
pub const USAGE: &str = "usage: euphoria-bench key-parts [DATA_DIRECTORY] [--tracks all|development|held-out] [--segments GIANTSTEPS_PLUS_ANNOTATIONS]";

/// Genres with fewer previews than this are grouped, since a percentage of
/// a handful of previews says little.
const SMALLEST_GENRE: usize = 20;
/// Where the short excerpt starts, and how long it is: shorter than the
/// 1.5 seconds drone-v2 reads at least.
const SHORT_EXCERPT_START_SECONDS: f32 = 30.0;
const SHORT_EXCERPT_SECONDS: f32 = 1.0;
/// The synthesized progressions: every key at each of these offsets from
/// concert pitch.
const SYNTHESIZED_OFFSETS_CENTS: [f32; 11] = [
    -50.0, -40.0, -30.0, -20.0, -10.0, 0.0, 10.0, 20.0, 30.0, 40.0, 50.0,
];
const SYNTHESIZED_SECONDS: f32 = 16.0;
const SYNTHESIS_RATE: u32 = 22_050;

/// The parsed command line.
#[derive(Debug, PartialEq, Eq)]
pub struct Arguments {
    data_directory: PathBuf,
    track_half: Option<usize>,
    segments_directory: Option<PathBuf>,
}

/// Parses the arguments after the mode name.
pub fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Arguments> {
    let mut data_directory = None;
    let mut track_half = None;
    let mut segments_directory = None;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--tracks" => {
                let value = arguments.next().context("--tracks needs a value")?;
                track_half = match value.as_str() {
                    "all" => None,
                    "development" => Some(0),
                    "held-out" => Some(1),
                    _ => bail!("--tracks accepts all, development or held-out\n{USAGE}"),
                };
            }
            "--segments" => {
                segments_directory = Some(PathBuf::from(
                    arguments.next().context("--segments needs a directory")?,
                ));
            }
            _ if argument.starts_with("--") => bail!("unknown option {argument:?}\n{USAGE}"),
            _ if data_directory.is_some() => bail!("more than one data directory\n{USAGE}"),
            _ => data_directory = Some(PathBuf::from(argument)),
        }
    }
    Ok(Arguments {
        data_directory: data_directory.unwrap_or_else(|| PathBuf::from("bench/data")),
        track_half,
        segments_directory,
    })
}

/// What one preview gave.
struct Preview {
    group: String,
    reference: Key,
    expert_keys: Option<Vec<Option<Key>>>,
    drone_key: Option<Key>,
    uncorrected_drone_key: Option<Key>,
    keyfinder_key: Option<Key>,
    old_timeline_keys: Vec<Key>,
    new_timeline_keys: Vec<Key>,
    old_profile: Option<[f32; 12]>,
    new_profile: Option<[f32; 12]>,
    short_keyfinder_key: Option<Key>,
    short_drone_key: Option<Key>,
    old_seconds: f64,
    new_seconds: f64,
}

/// Runs the comparison and prints the report.
pub fn run(arguments: &Arguments) -> Result<()> {
    let dataset_directory = arguments.data_directory.join("key");
    let dataset = load_key_tracks(&dataset_directory)?;
    let genres = genres(
        &dataset_directory,
        dataset.tracks.iter().map(|track| &track.name),
    )?;
    let tracks: Vec<_> = dataset
        .tracks
        .iter()
        .enumerate()
        .filter(|(index, _)| arguments.track_half.is_none_or(|half| index % 2 == half))
        .map(|(_, track)| track)
        .collect();
    let previews: Vec<Preview> = tracks
        .par_iter()
        .filter_map(|track| {
            let audio = match decode_mono(&track.audio_path) {
                Ok(audio) => audio,
                Err(error) => {
                    eprintln!("{}: {}", track.name, error.chain());
                    return None;
                }
            };
            let expert_keys = arguments
                .segments_directory
                .as_deref()
                .and_then(|directory| expert_segment_keys(directory, &track.name));
            Some(measure_preview(
                &audio,
                genres[&track.name].clone(),
                track.annotation,
                expert_keys,
            ))
        })
        .collect::<Result<_>>()?;
    let synthesized = measure_synthesized()?;
    print!("{}", report(&previews, &synthesized));
    Ok(())
}

fn measure_preview(
    audio: &AudioBuffer,
    group: String,
    reference: Key,
    expert_keys: Option<Vec<Option<Key>>>,
) -> Result<Preview> {
    let started = ThreadTime::now();
    let drone = DroneDetector.read_key(audio)?;
    let old_timeline = match drone.key {
        Some(key) => KeyFinderDetector.tuning_and_timeline(audio, key)?.1,
        None => Vec::new(),
    };
    let old_seconds = started.elapsed().as_secs_f64();
    let old_profile = KeyFinderDetector.pitch_class_profile(audio)?;

    let started = ThreadTime::now();
    let reading = read_key(KeyEngine::Auto, audio)?;
    let new_profile = pitch_class_profile(audio.samples(), audio.sample_rate().get())?;
    let new_seconds = started.elapsed().as_secs_f64();

    let keyfinder_key = KeyFinderDetector.read_key(audio)?.key;
    let short = excerpt(audio, SHORT_EXCERPT_START_SECONDS, SHORT_EXCERPT_SECONDS);
    Ok(Preview {
        group,
        reference,
        expert_keys,
        drone_key: reading.key,
        uncorrected_drone_key: drone.key,
        keyfinder_key,
        old_timeline_keys: section_keys(&old_timeline),
        new_timeline_keys: section_keys(&reading.timeline),
        old_profile,
        new_profile: new_profile
            .iter()
            .any(|level| *level > 0.0)
            .then_some(new_profile),
        short_keyfinder_key: KeyFinderDetector.read_key(&short)?.key,
        short_drone_key: read_key(KeyEngine::Auto, &short)?.key,
        old_seconds,
        new_seconds,
    })
}

fn excerpt(audio: &AudioBuffer, start_seconds: f32, seconds: f32) -> AudioBuffer {
    let rate = audio.sample_rate().get() as f32;
    let samples = audio.samples();
    let first = ((start_seconds * rate) as usize).min(samples.len());
    let last = (first + (seconds * rate) as usize).min(samples.len());
    AudioBuffer::new(samples[first..last].to_vec(), audio.sample_rate())
}

fn section_keys(timeline: &[KeySection]) -> Vec<Key> {
    let mut keys: Vec<Key> = Vec::new();
    for section in timeline {
        if !keys.contains(&section.key) {
            keys.push(section.key);
        }
    }
    keys
}

/// The genre of each preview, with genres of fewer than [`SMALLEST_GENRE`]
/// previews named "other genres".
fn genres<'name>(
    dataset_directory: &Path,
    names: impl Iterator<Item = &'name String>,
) -> Result<BTreeMap<String, String>> {
    let mut genres = BTreeMap::new();
    for name in names {
        let path = dataset_directory
            .join("annotations/genre")
            .join(format!("{name}.genre"));
        let genre = fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?
            .trim()
            .to_string();
        genres.insert(name.clone(), genre);
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for genre in genres.values() {
        *counts.entry(genre.clone()).or_default() += 1;
    }
    for genre in genres.values_mut() {
        if counts[genre.as_str()] < SMALLEST_GENRE {
            *genre = "other genres".to_string();
        }
    }
    Ok(genres)
}

/// The keys of the segments a GiantSteps+ expert annotation gives the
/// preview, `None` for a segment with no plain major or minor key, or
/// `None` when the preview has no annotation.
fn expert_segment_keys(directory: &Path, name: &str) -> Option<Vec<Option<Key>>> {
    let identifier = name.split('.').next()?;
    let text = fs::read_to_string(directory.join(format!("{identifier}.json"))).ok()?;
    let start = text.find("\"key\"")?;
    let list_start = start + text[start..].find('[')?;
    let list_end = list_start + text[list_start..].find(']')?;
    Some(
        text[list_start + 1..list_end]
            .split(',')
            .map(|entry| {
                let entry = entry.trim().trim_matches('"');
                if entry.contains('|') {
                    return None;
                }
                let mut words = entry.split_whitespace();
                let tonic = words.next()?;
                let mode = words.next()?;
                if mode != "major" && mode != "minor" {
                    return None;
                }
                format!("{tonic} {mode}").parse().ok()
            })
            .collect(),
    )
}

/// The notes a synthesized progression plays: the major scale, or for a
/// minor key the harmonic minor scale, whose dominant chord is major.
fn played_notes(key: Key) -> [bool; 12] {
    let steps: [u8; 7] = match key.mode() {
        Mode::Major => [0, 2, 4, 5, 7, 9, 11],
        Mode::Minor => [0, 2, 3, 5, 7, 8, 11],
    };
    let mut notes = [false; 12];
    for step in steps {
        notes[usize::from((key.tonic() + step) % 12)] = true;
    }
    notes
}

/// The notes of the scale of `key`, the natural minor for a minor key.
fn scale_notes(key: Key) -> [bool; 12] {
    let steps: [u8; 7] = match key.mode() {
        Mode::Major => [0, 2, 4, 5, 7, 9, 11],
        Mode::Minor => [0, 2, 3, 5, 7, 8, 10],
    };
    let mut notes = [false; 12];
    for step in steps {
        notes[usize::from((key.tonic() + step) % 12)] = true;
    }
    notes
}

fn strongest_seven(profile: &[f32; 12]) -> [bool; 12] {
    let mut ranked: Vec<usize> = (0..12).collect();
    ranked.sort_by(|left, right| profile[*right].total_cmp(&profile[*left]));
    let mut strongest = [false; 12];
    for pitch_class in &ranked[..7] {
        strongest[*pitch_class] = true;
    }
    strongest
}

/// How many of the seven strongest notes are notes of the key, out of 7.
fn share_of_top_seven_in(profile: &[f32; 12], notes: &[bool; 12]) -> f64 {
    let strongest = strongest_seven(profile);
    (0..12)
        .filter(|pitch_class| strongest[*pitch_class] && notes[*pitch_class])
        .count() as f64
        / 7.0
}

/// The mean level of the notes of the key less that of the other notes,
/// with the strongest note at 1.
fn contrast(profile: &[f32; 12], notes: &[bool; 12]) -> f64 {
    let inside: Vec<f64> = (0..12)
        .filter(|pitch_class| notes[*pitch_class])
        .map(|pitch_class| f64::from(profile[pitch_class]))
        .collect();
    let outside: Vec<f64> = (0..12)
        .filter(|pitch_class| !notes[*pitch_class])
        .map(|pitch_class| f64::from(profile[pitch_class]))
        .collect();
    inside.iter().sum::<f64>() / inside.len() as f64
        - outside.iter().sum::<f64>() / outside.len().max(1) as f64
}

/// What the synthesized progressions gave at one offset.
#[derive(Default)]
struct OffsetResult {
    progressions: usize,
    largest_tuning_error_cents: f32,
    keyfinder_exact: usize,
    drone_exact: usize,
    old_profile_right: usize,
    new_profile_right: usize,
}

fn measure_synthesized() -> Result<Vec<(f32, OffsetResult)>> {
    SYNTHESIZED_OFFSETS_CENTS
        .par_iter()
        .map(|cents| {
            let mut result = OffsetResult::default();
            for class in 0..24_u8 {
                let mode = if class % 2 == 0 {
                    Mode::Major
                } else {
                    Mode::Minor
                };
                let key = Key::new(class / 2, mode);
                let audio = AudioBuffer::new(
                    progression(key, *cents),
                    NonZeroU32::new(SYNTHESIS_RATE).expect("the rate is not zero"),
                );
                // At a quarter tone either neighbouring scale is as near.
                let accepted: Vec<Key> = if cents.abs() >= 50.0 {
                    vec![key, key.transposed(cents.signum() as i8)]
                } else {
                    vec![key]
                };
                result.progressions += 1;
                if let Some(tuning) = measure_tuning(&audio) {
                    let error = (tuning.cents - cents).abs();
                    let wrapped_error = error.min((100.0 - error).abs());
                    result.largest_tuning_error_cents =
                        result.largest_tuning_error_cents.max(wrapped_error);
                } else {
                    result.largest_tuning_error_cents = f32::INFINITY;
                }
                let right_key =
                    |found: Option<Key>| found.is_some_and(|found| accepted.contains(&found));
                result.keyfinder_exact +=
                    usize::from(right_key(KeyFinderDetector.read_key(&audio)?.key));
                result.drone_exact +=
                    usize::from(right_key(read_key(KeyEngine::Auto, &audio)?.key));
                let right_profile = |profile: Option<[f32; 12]>| {
                    profile.is_some_and(|profile| {
                        accepted
                            .iter()
                            .any(|accepted| strongest_seven(&profile) == played_notes(*accepted))
                    })
                };
                result.old_profile_right += usize::from(right_profile(
                    KeyFinderDetector.pitch_class_profile(&audio)?,
                ));
                result.new_profile_right += usize::from(right_profile(Some(pitch_class_profile(
                    audio.samples(),
                    SYNTHESIS_RATE,
                )?)));
            }
            Ok((*cents, result))
        })
        .collect()
}

/// The tonic, subdominant, dominant and tonic chords of `key`, two seconds
/// each with the root in the bass and five harmonics per note, tuned `cents`
/// away from A = 440 Hz.
fn progression(key: Key, cents: f32) -> Vec<f32> {
    let third = match key.mode() {
        Mode::Major => 4,
        Mode::Minor => 3,
    };
    let tonic = 60 + i32::from(key.tonic());
    let chords = [
        (tonic, third),
        (tonic + 5, third),
        (tonic + 7, 4),
        (tonic, third),
    ];
    let chord_samples = 2 * SYNTHESIS_RATE as usize;
    let total = (SYNTHESIZED_SECONDS * SYNTHESIS_RATE as f32) as usize;
    let frequency = |note: i32| 440.0 * 2.0_f32.powf((note as f32 - 69.0 + cents / 100.0) / 12.0);
    (0..total)
        .map(|index| {
            let (root, chord_third) = chords[(index / chord_samples) % chords.len()];
            let time = index as f32 / SYNTHESIS_RATE as f32;
            [
                (root - 24, 0.2),
                (root, 0.15),
                (root + chord_third, 0.15),
                (root + 7, 0.15),
            ]
            .iter()
            .map(|(note, amplitude)| {
                let fundamental = frequency(*note);
                amplitude
                    * (1..=5)
                        .map(|harmonic| {
                            (TAU * fundamental * harmonic as f32 * time).sin()
                                / (harmonic * harmonic) as f32
                        })
                        .sum::<f32>()
            })
            .sum()
        })
        .collect()
}

/// Groups previews by genre in report order, with every preview at the end.
fn by_group(previews: &[Preview]) -> Vec<(String, Vec<&Preview>)> {
    let mut groups: BTreeMap<String, Vec<&Preview>> = BTreeMap::new();
    for preview in previews {
        groups
            .entry(preview.group.clone())
            .or_default()
            .push(preview);
    }
    let mut ordered: Vec<(String, Vec<&Preview>)> = groups
        .into_iter()
        .filter(|(group, _)| group != "other genres")
        .collect();
    ordered.sort_by(|left, right| right.1.len().cmp(&left.1.len()).then(left.0.cmp(&right.0)));
    let other: Vec<&Preview> = previews
        .iter()
        .filter(|preview| preview.group == "other genres")
        .collect();
    if !other.is_empty() {
        ordered.push(("other genres".to_string(), other));
    }
    ordered.push(("all".to_string(), previews.iter().collect()));
    ordered
}

fn percent(share: f64) -> String {
    format!("{:.1}%", 100.0 * share)
}

fn share(previews: &[&Preview], test: impl Fn(&Preview) -> Option<bool>) -> String {
    let outcomes: Vec<bool> = previews
        .iter()
        .filter_map(|preview| test(preview))
        .collect();
    if outcomes.is_empty() {
        return "-".to_string();
    }
    percent(outcomes.iter().filter(|outcome| **outcome).count() as f64 / outcomes.len() as f64)
}

fn mean_of(previews: &[&Preview], value: impl Fn(&Preview) -> Option<f64>) -> String {
    let values: Vec<f64> = previews
        .iter()
        .filter_map(|preview| value(preview))
        .collect();
    if values.is_empty() {
        return "-".to_string();
    }
    percent(values.iter().sum::<f64>() / values.len() as f64)
}

fn relation_within(found: Option<Key>, reference: Key, accepted: &[KeyRelation]) -> bool {
    found.is_some_and(|found| accepted.contains(&key_relation(reference, found)))
}

fn report(previews: &[Preview], synthesized: &[(f32, OffsetResult)]) -> String {
    let groups = by_group(previews);
    let mut text = String::new();
    text.push_str("## Pitch-class profile\n\n");
    text.push_str("Against the scale of drone-v2's key. Top 7 exact: the seven strongest notes are the scale. Top 7 share: how many of the seven strongest are in the scale. Contrast: the mean level of the scale's notes less that of the other five, with the strongest note at 100%.\n\n");
    text.push_str("| Genre | Previews | Top 7 exact, libkeyfinder | Top 7 exact, Euphoria | Top 7 share, libkeyfinder | Top 7 share, Euphoria | Contrast, libkeyfinder | Contrast, Euphoria |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n");
    let scale_of = |preview: &Preview| preview.drone_key.map(scale_notes);
    for (group, members) in &groups {
        text.push_str(&format!(
            "| {group} | {} | {} | {} | {} | {} | {} | {} |\n",
            members.len(),
            share(members, |preview| Some(
                strongest_seven(&preview.old_profile?) == scale_of(preview)?
            )),
            share(members, |preview| Some(
                strongest_seven(&preview.new_profile?) == scale_of(preview)?
            )),
            mean_of(members, |preview| Some(share_of_top_seven_in(
                &preview.old_profile?,
                &scale_of(preview)?
            ))),
            mean_of(members, |preview| Some(share_of_top_seven_in(
                &preview.new_profile?,
                &scale_of(preview)?
            ))),
            mean_of(members, |preview| Some(contrast(
                &preview.old_profile?,
                &scale_of(preview)?
            ))),
            mean_of(members, |preview| Some(contrast(
                &preview.new_profile?,
                &scale_of(preview)?
            ))),
        ));
    }

    text.push_str("\nThe same against the annotated key:\n\n| Genre | Previews | Top 7 share, libkeyfinder | Top 7 share, Euphoria | Contrast, libkeyfinder | Contrast, Euphoria |\n| --- | --- | --- | --- | --- | --- |\n");
    for (group, members) in &groups {
        let scale = |preview: &Preview| scale_notes(preview.reference);
        text.push_str(&format!(
            "| {group} | {} | {} | {} | {} | {} |\n",
            members.len(),
            mean_of(members, |preview| Some(share_of_top_seven_in(
                &preview.old_profile?,
                &scale(preview)
            ))),
            mean_of(members, |preview| Some(share_of_top_seven_in(
                &preview.new_profile?,
                &scale(preview)
            ))),
            mean_of(members, |preview| Some(contrast(
                &preview.old_profile?,
                &scale(preview)
            ))),
            mean_of(members, |preview| Some(contrast(
                &preview.new_profile?,
                &scale(preview)
            ))),
        ));
    }

    text.push_str("\n## Key over time\n\nShare of previews whose timeline holds more than one key, against drone-v2's key. With expert segment annotations, also the previews annotated with one key that are split, and the previews annotated with a change between two plain keys whose timeline shows both.\n\n| Genre | Previews | Split, libkeyfinder | Split, drone-v2 | One expert key, split, libkeyfinder | One expert key, split, drone-v2 |\n| --- | --- | --- | --- | --- | --- |\n");
    let one_expert_key = |preview: &Preview| {
        preview
            .expert_keys
            .as_ref()
            .is_some_and(|keys| keys.len() == 1)
    };
    for (group, members) in &groups {
        text.push_str(&format!(
            "| {group} | {} | {} | {} | {} | {} |\n",
            members.len(),
            share(members, |preview| preview
                .drone_key
                .map(|_| preview.old_timeline_keys.len() > 1)),
            share(members, |preview| preview
                .drone_key
                .map(|_| preview.new_timeline_keys.len() > 1)),
            share(members, |preview| one_expert_key(preview)
                .then_some(preview.old_timeline_keys.len() > 1)),
            share(members, |preview| one_expert_key(preview)
                .then_some(preview.new_timeline_keys.len() > 1)),
        ));
    }
    let changing: Vec<&Preview> = previews
        .iter()
        .filter(|preview| {
            preview.expert_keys.as_ref().is_some_and(|keys| {
                keys.iter().all(Option::is_some) && keys.iter().any(|key| *key != keys[0])
            })
        })
        .collect();
    let shows_change = |timeline: &[Key], preview: &Preview| {
        let annotated: Vec<Key> = preview
            .expert_keys
            .iter()
            .flatten()
            .flatten()
            .copied()
            .collect();
        timeline
            .iter()
            .filter(|key| annotated.contains(key))
            .count()
            >= 2
    };
    text.push_str(&format!(
        "\nPreviews annotated with a change of key: {}. Timeline shows two of the annotated keys: libkeyfinder {}, drone-v2 {}.\n",
        changing.len(),
        changing.iter().filter(|preview| shows_change(&preview.old_timeline_keys, preview)).count(),
        changing.iter().filter(|preview| shows_change(&preview.new_timeline_keys, preview)).count(),
    ));

    text.push_str("\n## Key engine\n\nThe `keyfinder` key engine against drone-v2 as the `auto` engine runs it, with tuning correction, against the annotated key. The last column is drone-v2 without tuning correction, as it shipped before.\n\n| Genre | Previews | Exact, libkeyfinder | Exact, drone-v2 | Exact or fifth, libkeyfinder | Exact or fifth, drone-v2 | Exact, fifth or relative, libkeyfinder | Exact, fifth or relative, drone-v2 | Exact, drone-v2 without tuning correction |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n");
    let exact = [KeyRelation::Exact];
    let fifth = [KeyRelation::Exact, KeyRelation::Fifth];
    let relative = [
        KeyRelation::Exact,
        KeyRelation::Fifth,
        KeyRelation::Relative,
    ];
    for (group, members) in &groups {
        let mut row = format!("| {group} | {} |", members.len());
        for accepted in [&exact[..], &fifth[..], &relative[..]] {
            row.push_str(&format!(
                " {} | {} |",
                share(members, |preview| Some(relation_within(
                    preview.keyfinder_key,
                    preview.reference,
                    accepted
                ))),
                share(members, |preview| Some(relation_within(
                    preview.drone_key,
                    preview.reference,
                    accepted
                ))),
            ));
        }
        row.push_str(&format!(
            " {} |",
            share(members, |preview| Some(relation_within(
                preview.uncorrected_drone_key,
                preview.reference,
                &exact
            )))
        ));
        text.push_str(&row);
        text.push('\n');
    }

    text.push_str(&format!(
        "\n## Audio shorter than drone-v2 reads\n\n{SHORT_EXCERPT_SECONDS} second from {SHORT_EXCERPT_START_SECONDS} seconds into each preview, which libkeyfinder decided before and drone-v2 now reads followed by silence.\n\n| Genre | Previews | Exact, libkeyfinder | Exact, drone-v2 | Exact, fifth or relative, libkeyfinder | Exact, fifth or relative, drone-v2 |\n| --- | --- | --- | --- | --- | --- |\n"
    ));
    for (group, members) in &groups {
        text.push_str(&format!(
            "| {group} | {} | {} | {} | {} | {} |\n",
            members.len(),
            share(members, |preview| Some(relation_within(
                preview.short_keyfinder_key,
                preview.reference,
                &exact
            ))),
            share(members, |preview| Some(relation_within(
                preview.short_drone_key,
                preview.reference,
                &exact
            ))),
            share(members, |preview| Some(relation_within(
                preview.short_keyfinder_key,
                preview.reference,
                &relative
            ))),
            share(members, |preview| Some(relation_within(
                preview.short_drone_key,
                preview.reference,
                &relative
            ))),
        ));
    }

    text.push_str("\n## Tuning\n\nChord progressions in all 24 keys, synthesized at each offset. The tuning is measured by the same code before and after. Key: the `keyfinder` engine and drone-v2 as the `auto` engine runs it, both with tuning correction. Profile: the seven strongest notes are the notes played. At 50 cents either neighbouring scale counts.\n\n| Offset (cents) | Largest tuning error (cents) | Key, libkeyfinder | Key, drone-v2 | Profile, libkeyfinder | Profile, Euphoria |\n| --- | --- | --- | --- | --- | --- |\n");
    for (cents, result) in synthesized {
        let count = result.progressions as f64;
        text.push_str(&format!(
            "| {cents:+.0} | {:.1} | {} | {} | {} | {} |\n",
            result.largest_tuning_error_cents,
            percent(result.keyfinder_exact as f64 / count),
            percent(result.drone_exact as f64 / count),
            percent(result.old_profile_right as f64 / count),
            percent(result.new_profile_right as f64 / count),
        ));
    }

    let mean_seconds = |value: fn(&Preview) -> f64| {
        previews.iter().map(value).sum::<f64>() / previews.len().max(1) as f64
    };
    text.push_str(&format!(
        "\n## Speed\n\nCPU seconds per preview for the key analysis of the `auto` engine beyond decoding: before, drone-v2 then libkeyfinder for the tuning and the key over time (whose chromagram also gives the profile); after, drone-v2 with the key over time, the tuning and the note spectrum.\n\n| Before | After |\n| --- | --- |\n| {:.3} | {:.3} |\n",
        mean_seconds(|preview| preview.old_seconds),
        mean_seconds(|preview| preview.new_seconds),
    ));
    text
}
