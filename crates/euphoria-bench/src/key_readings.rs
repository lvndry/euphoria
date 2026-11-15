// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Writes what drone-v2 reads in every GiantSteps track, one JSON line per
//! track, to check how its confidence is rated and to score it by genre.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use cpu_time::ThreadTime;
use euphoria_core::detect::{DroneDetector, measure_tuning};
use euphoria_core::{Key, Mode, decode_mono};
use rayon::prelude::*;

use crate::dataset::load_key_tracks;

/// Usage of the key readings mode.
pub const USAGE: &str = "usage: euphoria-bench key-readings [DATA_DIRECTORY]";

/// Parses the arguments after the mode name.
pub fn parse_arguments(arguments: impl IntoIterator<Item = String>) -> Result<PathBuf> {
    let mut data_directory = None;
    for argument in arguments {
        if argument.starts_with("--") || data_directory.is_some() {
            bail!("unexpected argument {argument:?}\n{USAGE}");
        }
        data_directory = Some(PathBuf::from(argument));
    }
    Ok(data_directory.unwrap_or_else(|| PathBuf::from("bench/data")))
}

fn class_of(key: Key) -> usize {
    usize::from(key.tonic()) * 2 + usize::from(key.mode() == Mode::Minor)
}

/// Prints one JSON line per track of the key dataset in `data_directory`.
pub fn run(data_directory: &Path) -> Result<()> {
    let dataset = load_key_tracks(&data_directory.join("key"))?;
    let lines: Vec<String> = dataset
        .tracks
        .par_iter()
        .map(|track| {
            let audio = match decode_mono(&track.audio_path) {
                Ok(audio) => audio,
                Err(error) => {
                    return format!("{{\"name\":{:?},\"error\":{:?}}}", track.name, error.chain());
                }
            };
            let tuning_cents = measure_tuning(&audio)
                .map_or("null".to_string(), |tuning| tuning.cents.to_string());
            let drone_started = ThreadTime::now();
            let drone = DroneDetector.probabilities(&audio);
            let drone_seconds = drone_started.elapsed().as_secs_f64();
            let drone_fields = match drone {
                Ok(Some(probabilities)) => {
                    format!("\"drone\":{:?}", probabilities.in_class_order())
                }
                Ok(None) => "\"drone\":null".to_string(),
                Err(error) => format!("\"drone_error\":{:?}", error.chain()),
            };
            format!(
                "{{\"name\":{:?},\"reference\":{},\"duration_seconds\":{},\"tuning_cents\":{tuning_cents},{drone_fields},\"drone_seconds\":{drone_seconds}}}",
                track.name,
                class_of(track.annotation),
                audio.duration_seconds(),
            )
        })
        .collect();
    for line in lines {
        println!("{line}");
    }
    Ok(())
}
