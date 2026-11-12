// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Measures key and tempo detectors against the GiantSteps datasets, and the
//! energy rating against a labels file, and prints a Markdown report. It also
//! scores the keys and tempos in a rekordbox collection export.

mod alternative;
mod beats;
mod confidence;
mod dataset;
mod energy;
mod evaluate;
mod quality;
mod rekordbox;
mod render;
mod scoring;
mod shift;
mod statistics;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use euphoria_core::detect::{
    KeyDecision, KeyDetector, KeyFinderDetector, KeyFinderVariant, StratumDetector,
};

use evaluate::{Report, evaluate_keys, evaluate_tempo};

const USAGE: &str = "usage: euphoria-bench [DATA_DIRECTORY] [--only key|tempo] [--tracks all|development|held-out]\n       euphoria-bench beats [DATA_DIRECTORY] [--tracks all|development|held-out]\n       euphoria-bench shift [AUDIO_DIRECTORY] [--every N]\n       euphoria-bench energy LABELS_FILE [--dump FILE]\n       euphoria-bench quality LIST_FILE\n       euphoria-bench rekordbox COLLECTION_XML [DATA_DIRECTORY] [--only key|tempo]";

/// The first argument that selects the energy benchmark.
const ENERGY_MODE: &str = "energy";
/// The first argument that selects the beat grid benchmark.
const BEATS_MODE: &str = "beats";
/// The first argument that selects the pitch shifting benchmark.
const SHIFT_MODE: &str = "shift";
/// The first argument that selects the audio quality measurement.
const QUALITY_MODE: &str = "quality";

/// The first argument that scores a rekordbox collection export.
const REKORDBOX_MODE: &str = "rekordbox";

/// The key detectors that are measured. The first is the one Euphoria
/// ships, and the others are compared with it track by track.
const KEY_DETECTORS: [&dyn KeyDetector; 4] = [
    &KeyFinderDetector,
    &KeyFinderVariant {
        tuning_correction: false,
        decision: KeyDecision::WholeTrack,
    },
    &KeyFinderVariant {
        tuning_correction: true,
        decision: KeyDecision::SectionVote,
    },
    &StratumDetector,
];

/// Which benchmarks a run covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Selection {
    Both,
    KeyOnly,
    TempoOnly,
}

/// Which tracks of each dataset a run covers. The two halves take every
/// other track in name order, so a setting can be chosen on one half and
/// checked on the half it has not seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tracks {
    All,
    Development,
    HeldOut,
}

impl Tracks {
    /// Whether the track at `index` in name order is covered.
    fn covers(self, index: usize) -> bool {
        match self {
            Self::All => true,
            Self::Development => index.is_multiple_of(2),
            Self::HeldOut => !index.is_multiple_of(2),
        }
    }

    fn keep<Annotation>(self, dataset: &mut dataset::Dataset<Annotation>) {
        let mut index = 0;
        dataset.tracks.retain(|_| {
            index += 1;
            self.covers(index - 1)
        });
    }
}

/// The parsed command line.
#[derive(Debug, PartialEq, Eq)]
struct Arguments {
    data_directory: PathBuf,
    selection: Selection,
    tracks: Tracks,
}

fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Arguments> {
    let mut data_directory = None;
    let mut selection = Selection::Both;
    let mut tracks = Tracks::All;
    let mut arguments = arguments.into_iter();
    while let Some(argument) = arguments.next() {
        if argument == "--only" {
            let value = arguments
                .next()
                .with_context(|| format!("--only needs a value\n{USAGE}"))?;
            selection = match value.as_str() {
                "key" => Selection::KeyOnly,
                "tempo" => Selection::TempoOnly,
                _ => bail!("--only accepts key or tempo, not {value:?}\n{USAGE}"),
            };
        } else if argument == "--tracks" {
            let value = arguments
                .next()
                .with_context(|| format!("--tracks needs a value\n{USAGE}"))?;
            tracks = match value.as_str() {
                "all" => Tracks::All,
                "development" => Tracks::Development,
                "held-out" => Tracks::HeldOut,
                _ => bail!("--tracks accepts all, development or held-out, not {value:?}\n{USAGE}"),
            };
        } else if argument.starts_with("--") {
            bail!("unknown option {argument:?}\n{USAGE}");
        } else if data_directory.is_some() {
            bail!("more than one data directory given\n{USAGE}");
        } else {
            data_directory = Some(PathBuf::from(argument));
        }
    }
    Ok(Arguments {
        data_directory: data_directory.unwrap_or_else(|| PathBuf::from("bench/data")),
        selection,
        tracks,
    })
}

fn main() -> Result<()> {
    let mut command_line = std::env::args().skip(1).peekable();
    if command_line.peek().map(String::as_str) == Some(SHIFT_MODE) {
        let arguments = shift::parse_arguments(command_line.skip(1))?;
        return shift::run(&arguments);
    }
    if command_line.peek().map(String::as_str) == Some(BEATS_MODE) {
        let arguments = beats::parse_arguments(command_line.skip(1))?;
        return beats::run(&arguments);
    }
    if command_line.peek().map(String::as_str) == Some(ENERGY_MODE) {
        let arguments = energy::parse_arguments(command_line.skip(1))?;
        return energy::run(&arguments);
    }
    if command_line.peek().map(String::as_str) == Some(QUALITY_MODE) {
        let arguments = quality::parse_arguments(command_line.skip(1))?;
        return quality::run(&arguments);
    }
    if command_line.peek().map(String::as_str) == Some(REKORDBOX_MODE) {
        let arguments = rekordbox::parse_arguments(command_line.skip(1))?;
        return rekordbox::run(&arguments);
    }
    let arguments = parse_arguments(command_line)?;
    let data_directory = &arguments.data_directory;

    let mut key_tracks = (arguments.selection != Selection::TempoOnly)
        .then(|| dataset::load_key_tracks(&data_directory.join("key")))
        .transpose()?;
    let mut tempo_tracks = (arguments.selection != Selection::KeyOnly)
        .then(|| dataset::load_tempo_tracks(&data_directory.join("tempo")))
        .transpose()?;
    if let Some(dataset) = &mut key_tracks {
        arguments.tracks.keep(dataset);
    }
    if let Some(dataset) = &mut tempo_tracks {
        arguments.tracks.keep(dataset);
    }
    match arguments.tracks {
        Tracks::All => {}
        Tracks::Development => println!(
            "Development half: every other track in name order, starting with the first.\n"
        ),
        Tracks::HeldOut => {
            println!("Held out half: every other track in name order, starting with the second.\n")
        }
    }

    let key = key_tracks
        .as_ref()
        .map(|dataset| evaluate_keys(dataset, &KEY_DETECTORS));
    let tempo = tempo_tracks
        .as_ref()
        .map(|dataset| evaluate_tempo(dataset, &[&StratumDetector]));
    print!("{}", render::render_report(&Report { key, tempo }));

    if let Some(dataset) = &key_tracks {
        print!("{}", confidence::evaluate_key_confidence(dataset));
    }
    if let Some(dataset) = &tempo_tracks {
        print!("{}", confidence::evaluate_tempo_confidence(dataset));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Result<Arguments> {
        parse_arguments(arguments.iter().map(|argument| argument.to_string()))
    }

    #[test]
    fn defaults_to_both_benchmarks_in_bench_data() {
        assert_eq!(
            parse(&[]).unwrap(),
            Arguments {
                data_directory: PathBuf::from("bench/data"),
                selection: Selection::Both,
                tracks: Tracks::All,
            }
        );
    }

    #[test]
    fn reads_the_data_directory_and_selection_in_either_order() {
        let expected = Arguments {
            data_directory: PathBuf::from("elsewhere"),
            selection: Selection::TempoOnly,
            tracks: Tracks::All,
        };
        assert_eq!(parse(&["elsewhere", "--only", "tempo"]).unwrap(), expected);
        assert_eq!(parse(&["--only", "tempo", "elsewhere"]).unwrap(), expected);
        assert_eq!(
            parse(&["--only", "key"]).unwrap().selection,
            Selection::KeyOnly
        );
    }

    #[test]
    fn reads_which_tracks_to_cover() {
        assert_eq!(
            parse(&["--tracks", "development"]).unwrap().tracks,
            Tracks::Development
        );
        assert_eq!(
            parse(&["--tracks", "held-out"]).unwrap().tracks,
            Tracks::HeldOut
        );
        assert_eq!(parse(&["--tracks", "all"]).unwrap().tracks, Tracks::All);
    }

    #[test]
    fn the_two_halves_share_no_track_and_together_hold_every_track() {
        let names = |tracks: Tracks| {
            let mut dataset = dataset::Dataset {
                tracks: (0..7)
                    .map(|number| dataset::Track {
                        name: number.to_string(),
                        audio_path: PathBuf::new(),
                        annotation: (),
                    })
                    .collect(),
                unannotated_count: 0,
            };
            tracks.keep(&mut dataset);
            dataset
                .tracks
                .into_iter()
                .map(|track| track.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names(Tracks::All).len(), 7);
        assert_eq!(names(Tracks::Development), ["0", "2", "4", "6"]);
        assert_eq!(names(Tracks::HeldOut), ["1", "3", "5"]);
    }

    #[test]
    fn rejects_bad_arguments() {
        for arguments in [
            &["--only"][..],
            &["--tracks"],
            &["--tracks", "half"],
            &["--only", "energy"],
            &["--fast"],
            &["one", "two"],
        ] {
            assert!(parse(arguments).is_err(), "{arguments:?} should not parse");
        }
    }
}
