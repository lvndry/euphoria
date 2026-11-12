// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Measures what pitch shifting does to the key and the tempo Euphoria
//! detects: every preview is shifted by a few amounts, and the shifted
//! audio is analyzed again.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use euphoria_core::detect::{KeyFinderDetector, StratumDetector};
use euphoria_core::shift::PitchShifter;
use euphoria_core::{AudioBuffer, Key, TempoRange, decode_mono};
use rayon::prelude::*;

use crate::scoring::{KeyRelation, key_relation};

pub const USAGE: &str = "usage: euphoria-bench shift [AUDIO_DIRECTORY] [--every N]";

/// The shifts measured, in semitones: one and two up, as a DJ moves a track
/// to a neighbouring key, three down, and the furthest shift, six.
const SHIFTS: [i8; 4] = [1, 2, -3, 6];
/// A tempo counts as kept when it is within this share of the original's.
const TEMPO_TOLERANCE: f64 = 0.005;

#[derive(Debug, PartialEq, Eq)]
pub struct Arguments {
    audio_directory: PathBuf,
    every: usize,
}

pub fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Arguments> {
    let mut audio_directory = None;
    let mut every = 1;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "--every" {
            every = arguments
                .next()
                .and_then(|value| value.parse().ok())
                .filter(|value| *value > 0)
                .with_context(|| format!("--every needs a whole number from 1\n{USAGE}"))?;
        } else if argument.starts_with("--") {
            bail!("unknown option {argument:?}\n{USAGE}");
        } else if audio_directory.is_some() {
            bail!("more than one audio directory given\n{USAGE}");
        } else {
            audio_directory = Some(PathBuf::from(argument));
        }
    }
    Ok(Arguments {
        audio_directory: audio_directory.unwrap_or_else(|| PathBuf::from("bench/data/key/audio")),
        every,
    })
}

/// What one preview gave, before and after each shift.
struct Outcome {
    original_key: Option<Key>,
    original_tempo: Option<f64>,
    /// Per shift: the detected key, the detected tempo and whether the
    /// shifted audio was exactly as long.
    shifted: Vec<(Option<Key>, Option<f64>, bool)>,
}

fn shift_mono(audio: &AudioBuffer, semitones: i8) -> Result<AudioBuffer> {
    let mut shifter = PitchShifter::new(1, audio.sample_rate().get(), semitones)
        .map_err(|reason| anyhow::anyhow!(reason))?;
    let mut samples = Vec::with_capacity(audio.samples().len());
    for chunk in audio.samples().chunks(65_536) {
        samples.extend(
            shifter
                .process(chunk)
                .map_err(|reason| anyhow::anyhow!(reason))?,
        );
    }
    samples.extend(shifter.finish().map_err(|reason| anyhow::anyhow!(reason))?);
    Ok(AudioBuffer::new(samples, audio.sample_rate()))
}

fn key_and_tempo(audio: &AudioBuffer) -> Result<(Option<Key>, Option<f64>)> {
    let key = KeyFinderDetector
        .read_key(audio)
        .map_err(|error| anyhow::anyhow!(error.chain()))?
        .key;
    let tempo = StratumDetector
        .read_tempo(audio)
        .map_err(|error| anyhow::anyhow!(error.chain()))?
        .map(|reading| f64::from(reading.tempo_in(TempoRange::DEFAULT)));
    Ok((key, tempo))
}

fn measure(path: &Path) -> Result<Outcome> {
    let audio = decode_mono(path).map_err(|error| anyhow::anyhow!(error.chain()))?;
    let (original_key, original_tempo) = key_and_tempo(&audio)?;
    let mut shifted = Vec::new();
    for semitones in SHIFTS {
        let moved = shift_mono(&audio, semitones)?;
        let same_length = moved.samples().len() == audio.samples().len();
        let (key, tempo) = key_and_tempo(&moved)?;
        shifted.push((key, tempo, same_length));
    }
    Ok(Outcome {
        original_key,
        original_tempo,
        shifted,
    })
}

fn share(count: usize, total: usize) -> String {
    if total == 0 {
        return "-".to_string();
    }
    format!("{:.1}%", 100.0 * count as f64 / total as f64)
}

fn render(outcomes: &[Outcome]) -> String {
    let mut report = String::new();
    let with_key: Vec<&Outcome> = outcomes
        .iter()
        .filter(|outcome| outcome.original_key.is_some())
        .collect();
    let _ = writeln!(report, "## Pitch shifting\n");
    let _ = writeln!(
        report,
        "{} previews; {} with a key detected before shifting, which the shifted keys are compared with.\n",
        outcomes.len(),
        with_key.len()
    );
    let _ = writeln!(
        report,
        "| Shift | Exact shifted key | A fifth away | Relative | Parallel | Other | No key detected | Still the original key | Tempo kept | Same length |"
    );
    let _ = writeln!(
        report,
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |"
    );
    for (index, semitones) in SHIFTS.iter().enumerate() {
        let mut counts = [0_usize; 5];
        let mut missing = 0;
        let mut unchanged = 0;
        for outcome in &with_key {
            let original = outcome
                .original_key
                .unwrap_or(Key::new(0, euphoria_core::Mode::Major));
            let target = original.transposed(*semitones);
            match outcome.shifted[index].0 {
                Some(found) => {
                    let relation = key_relation(target, found);
                    let position = KeyRelation::ALL
                        .iter()
                        .position(|candidate| *candidate == relation)
                        .unwrap_or(4);
                    counts[position] += 1;
                    if found == original {
                        unchanged += 1;
                    }
                }
                None => missing += 1,
            }
        }
        let with_tempo: Vec<&Outcome> = outcomes
            .iter()
            .filter(|outcome| outcome.original_tempo.is_some())
            .collect();
        let tempo_kept = with_tempo
            .iter()
            .filter(|outcome| {
                let original = outcome.original_tempo.unwrap_or(0.0);
                outcome.shifted[index]
                    .1
                    .is_some_and(|tempo| (tempo - original).abs() <= TEMPO_TOLERANCE * original)
            })
            .count();
        let same_length = outcomes
            .iter()
            .filter(|outcome| outcome.shifted[index].2)
            .count();
        let _ = writeln!(
            report,
            "| {semitones:+} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            share(counts[0], with_key.len()),
            share(counts[1], with_key.len()),
            share(counts[2], with_key.len()),
            share(counts[3], with_key.len()),
            share(counts[4], with_key.len()),
            share(missing, with_key.len()),
            share(unchanged, with_key.len()),
            share(tempo_kept, with_tempo.len()),
            share(same_length, outcomes.len()),
        );
    }
    let _ = writeln!(
        report,
        "\nThe key columns compare the key detected after shifting with the key detected before, moved by the shift. Their percentages use the previews with an original key detected; Other means a detected key with none of the listed relations, and No key detected means the shifted key was missing. The tempo is kept when it is within 0.5% of the tempo detected before."
    );
    report
}

fn preview_files(
    directory: &Path,
    entries: impl IntoIterator<Item = std::io::Result<PathBuf>>,
) -> Result<Vec<PathBuf>> {
    let files: Vec<PathBuf> = entries
        .into_iter()
        .collect::<std::io::Result<_>>()
        .with_context(|| format!("reading {}", directory.display()))?;
    Ok(files
        .into_iter()
        .filter(|path| euphoria_core::is_supported_audio_file(path))
        .collect())
}

pub fn run(arguments: &Arguments) -> Result<()> {
    let entries = std::fs::read_dir(&arguments.audio_directory).with_context(|| {
        format!(
            "reading {}; run bench/download_datasets.sh first",
            arguments.audio_directory.display()
        )
    })?;
    let mut files = preview_files(
        &arguments.audio_directory,
        entries.map(|entry| entry.map(|entry| entry.path())),
    )?;
    files.sort();
    let chosen: Vec<PathBuf> = files.into_iter().step_by(arguments.every).collect();
    if chosen.is_empty() {
        bail!("no audio found in {}", arguments.audio_directory.display());
    }
    let outcomes: Vec<Result<Outcome>> = chosen.par_iter().map(|path| measure(path)).collect();
    let mut measured = Vec::with_capacity(outcomes.len());
    for (path, outcome) in chosen.iter().zip(outcomes) {
        measured.push(outcome.with_context(|| format!("measuring {}", path.display()))?);
    }
    print!("{}", render(&measured));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use euphoria_core::Mode;

    #[test]
    fn unreadable_directory_entries_fail_instead_of_measuring_a_partial_set() {
        let entries = [
            Ok(PathBuf::from("previews/first.wav")),
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "entry became unreadable",
            )),
        ];
        let error = preview_files(Path::new("previews"), entries).unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("previews"), "{message}");
        assert!(message.contains("entry became unreadable"), "{message}");
    }

    #[test]
    fn successful_enumeration_still_ignores_non_audio_files() {
        let entries = [
            Ok(PathBuf::from("previews/song.wav")),
            Ok(PathBuf::from("previews/cover.png")),
        ];
        assert_eq!(
            preview_files(Path::new("previews"), entries).unwrap(),
            [PathBuf::from("previews/song.wav")]
        );
    }

    #[test]
    fn parses_the_directory_and_the_sampling() {
        let parse = |arguments: &[&str]| {
            parse_arguments(arguments.iter().map(|argument| argument.to_string()))
        };
        assert_eq!(
            parse(&[]).unwrap(),
            Arguments {
                audio_directory: PathBuf::from("bench/data/key/audio"),
                every: 1
            }
        );
        assert_eq!(parse(&["clips", "--every", "4"]).unwrap().every, 4);
        assert!(parse(&["--every", "0"]).is_err());
        assert!(parse(&["--fast"]).is_err());
    }

    #[test]
    fn report_distinguishes_exact_wrong_and_missing_key_detections() {
        let original = Key::new(0, Mode::Major);
        let outcomes: Vec<_> = [
            Some(original.transposed(1)),
            Some(original.transposed(3)),
            None,
        ]
        .into_iter()
        .map(|key| Outcome {
            original_key: Some(original),
            original_tempo: None,
            shifted: vec![(key, None, true); SHIFTS.len()],
        })
        .collect();
        let report = render(&outcomes);
        assert!(report.contains("| Exact shifted key |"), "{report}");
        assert!(report.contains("| No key detected |"), "{report}");
        assert!(
            report.contains(
                "| +1 | 33.3% | 0.0% | 0.0% | 0.0% | 33.3% | 33.3% | 0.0% | - | 100.0% |"
            ),
            "{report}"
        );
    }

    #[test]
    fn a_shifted_key_is_compared_with_the_original_moved_by_the_shift() {
        let a_minor = Key::new(9, Mode::Minor);
        let outcome = Outcome {
            original_key: Some(a_minor),
            original_tempo: Some(124.0),
            shifted: vec![
                (Some(a_minor.transposed(1)), Some(124.1), true),
                (Some(a_minor), Some(124.0), true),
                (
                    Some(a_minor.transposed(-3).transposed(7)),
                    Some(130.0),
                    true,
                ),
                (None, None, false),
            ],
        };

        let report = render(&[outcome]);

        assert!(
            report.contains(
                "| +1 | 100.0% | 0.0% | 0.0% | 0.0% | 0.0% | 0.0% | 0.0% | 100.0% | 100.0% |"
            ),
            "{report}"
        );
        assert!(
            report.contains(
                "| +2 | 0.0% | 0.0% | 0.0% | 0.0% | 100.0% | 0.0% | 100.0% | 100.0% | 100.0% |"
            ),
            "{report}"
        );
        assert!(report.contains("| -3 | 0.0% | 100.0% |"), "{report}");
        assert!(
            report.contains(
                "| +6 | 0.0% | 0.0% | 0.0% | 0.0% | 0.0% | 100.0% | 0.0% | 0.0% | 0.0% |"
            ),
            "{report}"
        );
    }
}
