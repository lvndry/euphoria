// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Measures key and tempo detectors against the GiantSteps datasets and
//! prints a Markdown report.

mod dataset;
mod evaluate;
mod render;
mod scoring;

use std::path::PathBuf;

use anyhow::Result;
use euphoria_core::detect::{KeyFinderDetector, StratumDetector};

use evaluate::{Report, evaluate_keys, evaluate_tempo};

fn main() -> Result<()> {
    let data_directory = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("bench/data"));
    let key_tracks = dataset::load_key_tracks(&data_directory.join("key"))?;
    let tempo_tracks = dataset::load_tempo_tracks(&data_directory.join("tempo"))?;

    let report = Report {
        key: evaluate_keys(&key_tracks, &[&KeyFinderDetector, &StratumDetector]),
        tempo: evaluate_tempo(&tempo_tracks, &[&StratumDetector]),
    };
    print!("{}", render::render_report(&report));
    Ok(())
}
