// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Measures key and tempo detectors against the GiantSteps datasets and
//! prints a Markdown report.

mod dataset;
mod evaluate;
mod render;
mod scoring;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use euphoria_core::detect::{KeyFinderDetector, StratumDetector};

use evaluate::{Report, evaluate_keys, evaluate_tempo};

const USAGE: &str = "usage: euphoria-bench [DATA_DIRECTORY] [--only key|tempo]";

/// Which benchmarks a run covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Selection {
    Both,
    KeyOnly,
    TempoOnly,
}

/// The parsed command line.
#[derive(Debug, PartialEq, Eq)]
struct Arguments {
    data_directory: PathBuf,
    selection: Selection,
}

fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<Arguments> {
    let mut data_directory = None;
    let mut selection = Selection::Both;
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
    })
}

fn main() -> Result<()> {
    let arguments = parse_arguments(std::env::args().skip(1))?;
    let data_directory = &arguments.data_directory;

    let key_tracks = (arguments.selection != Selection::TempoOnly)
        .then(|| dataset::load_key_tracks(&data_directory.join("key")))
        .transpose()?;
    let tempo_tracks = (arguments.selection != Selection::KeyOnly)
        .then(|| dataset::load_tempo_tracks(&data_directory.join("tempo")))
        .transpose()?;

    let key =
        key_tracks.map(|tracks| evaluate_keys(&tracks, &[&KeyFinderDetector, &StratumDetector]));
    let tempo = tempo_tracks.map(|tracks| evaluate_tempo(&tracks, &[&StratumDetector]));

    print!("{}", render::render_report(&Report { key, tempo }));
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
            }
        );
    }

    #[test]
    fn reads_the_data_directory_and_selection_in_either_order() {
        let expected = Arguments {
            data_directory: PathBuf::from("elsewhere"),
            selection: Selection::TempoOnly,
        };
        assert_eq!(parse(&["elsewhere", "--only", "tempo"]).unwrap(), expected);
        assert_eq!(parse(&["--only", "tempo", "elsewhere"]).unwrap(), expected);
        assert_eq!(
            parse(&["--only", "key"]).unwrap().selection,
            Selection::KeyOnly
        );
    }

    #[test]
    fn rejects_bad_arguments() {
        for arguments in [
            &["--only"][..],
            &["--only", "energy"],
            &["--fast"],
            &["one", "two"],
        ] {
            assert!(parse(arguments).is_err(), "{arguments:?} should not parse");
        }
    }
}
