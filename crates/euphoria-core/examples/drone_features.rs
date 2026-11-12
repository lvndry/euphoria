// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Writes the drone-v1 features and key probabilities of audio files, to
//! check them against drone's Python code with `bench/drone_parity.py`.
//!
//! Reads lines of `MODE<TAB>AUDIO<TAB>FEATURES` on standard input. MODE is
//! `model-input` for a 16 bit mono WAV at 22,050 Hz, as drone has ffmpeg
//! make, or `decoded` for any file, decoded and converted by Euphoria. The
//! features go to FEATURES as little endian float32, 192 rows of frames, and
//! one JSON line per file goes to standard output.

use std::io::{BufRead, Write};
use std::path::Path;
use std::time::Instant;

use euphoria_core::decode_mono;
use euphoria_core::detect::DroneDetector;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let standard_output = std::io::stdout();
    let mut output = standard_output.lock();
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let fields: Vec<&str> = line.split('\t').collect();
        let [mode, audio_path, features_path] = fields[..] else {
            return Err(format!("expected three tab separated fields: {line}").into());
        };
        let started = Instant::now();
        let audio = decode_mono(Path::new(audio_path))?;
        let decoded_seconds = started.elapsed().as_secs_f64();
        let (features, frame_count, probabilities) = match mode {
            "model-input" => DroneDetector.inspect_model_input(audio.samples())?,
            "decoded" => DroneDetector.inspect_decoded(&audio)?,
            _ => return Err(format!("unknown mode {mode}").into()),
        };
        let bytes: Vec<u8> = features.iter().flat_map(|value| value.to_le_bytes()).collect();
        std::fs::write(features_path, bytes)?;
        let probabilities: Vec<f32> = probabilities
            .map(|probabilities| probabilities.in_class_order().to_vec())
            .unwrap_or_default();
        writeln!(
            output,
            "{{\"audio\":{audio_path:?},\"frames\":{frame_count},\"decode_seconds\":{decoded_seconds},\"total_seconds\":{},\"probabilities\":{probabilities:?}}}",
            started.elapsed().as_secs_f64()
        )?;
    }
    Ok(())
}
