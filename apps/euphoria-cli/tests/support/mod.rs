// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runs the `euphoria` binary with its data in a folder of its own, so tests
//! never read or write the cache of the person running them.

pub mod pty;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A folder under the system's temporary folder, deleted when dropped.
pub struct TemporaryFolder(PathBuf);

impl TemporaryFolder {
    pub fn new() -> Self {
        static CREATED: AtomicUsize = AtomicUsize::new(0);
        let folder = std::env::temp_dir().join(format!(
            "euphoria-cli-test-{}-{}",
            std::process::id(),
            CREATED.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&folder).expect("the temporary folder is created");
        Self(folder)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryFolder {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The `euphoria` binary with `EUPHORIA_DATA_DIR` pointing at a temporary
/// folder that lasts as long as this value.
pub struct Euphoria {
    data_directory: TemporaryFolder,
}

impl Euphoria {
    pub fn new() -> Self {
        Self {
            data_directory: TemporaryFolder::new(),
        }
    }

    #[allow(dead_code)]
    pub fn run(&self, arguments: &[&str]) -> Output {
        self.command(arguments)
            .output()
            .expect("the euphoria binary runs")
    }

    pub fn command(&self, arguments: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_euphoria"));
        command
            .args(arguments)
            .env("EUPHORIA_DATA_DIR", self.data_directory.path())
            .env("EUPHORIA_ASSUME_NO_DJ_PROGRAMS", "1");
        command
    }

    #[allow(dead_code)]
    pub fn data_directory(&self) -> &Path {
        self.data_directory.path()
    }

    #[allow(dead_code)]
    pub fn cache_path(&self) -> PathBuf {
        self.data_directory.path().join("analysis-cache.sqlite3")
    }
}

#[allow(dead_code)]
pub fn fixtures_directory() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/euphoria-core/tests/fixtures")
}

#[allow(dead_code)]
pub fn standard_error(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Writes 30 seconds of a 16 bit mono WAV file holding a decaying low tone
/// on every beat at `tempo`, starting `first_beat` seconds in.
#[allow(dead_code)]
pub fn write_kick_track(path: &Path, tempo: f64, first_beat: f64) {
    write_kicks(path, first_beat, |_| tempo);
}

/// Writes 30 seconds of a 16 bit mono WAV file holding a decaying low tone
/// on every beat from `first_beat`, each beat following the one before at
/// `tempo_at` its time.
#[allow(dead_code)]
pub fn write_kicks(path: &Path, first_beat: f64, tempo_at: impl Fn(f64) -> f64) {
    const SAMPLE_RATE: u32 = 44_100;
    let sample_count = 30 * SAMPLE_RATE as usize;
    let mut samples = vec![0.0_f64; sample_count];
    let mut beat_start = first_beat;
    while beat_start < 30.0 {
        let first_sample = (beat_start * f64::from(SAMPLE_RATE)) as usize;
        for (offset, sample) in samples
            .iter_mut()
            .skip(first_sample)
            .take(8_000)
            .enumerate()
        {
            let time = offset as f64 / f64::from(SAMPLE_RATE);
            *sample += (std::f64::consts::TAU * 60.0 * time).sin() * (-time / 0.04).exp();
        }
        beat_start += 60.0 / tempo_at(beat_start);
    }
    let data_length = (sample_count * 2) as u32;
    let mut bytes = Vec::with_capacity(44 + sample_count * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_length).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    bytes.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_length.to_le_bytes());
    for sample in samples {
        let scaled = (sample.clamp(-1.0, 1.0) * 0.8 * f64::from(i16::MAX)) as i16;
        bytes.extend_from_slice(&scaled.to_le_bytes());
    }
    std::fs::write(path, bytes).expect("the track is written");
}
