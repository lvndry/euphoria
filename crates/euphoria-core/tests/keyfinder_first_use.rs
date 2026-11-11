// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Checks that libkeyfinder can be used from many threads at once from the
//! first call on, as a batch analysis does when it starts.
//!
//! libkeyfinder builds its tone profiles the first time a key is scored, so
//! what is checked happens once per process. The test therefore runs itself
//! again in fresh processes, and in each one many threads score their first
//! key at the same moment.

#![cfg(feature = "libkeyfinder")]

use std::num::NonZeroU32;
use std::process::Command;
use std::sync::{Arc, Barrier};
use std::thread;

use euphoria_core::AudioBuffer;
use euphoria_core::detect::{KeyDetector, KeyFinderDetector};

/// Set in the processes the test starts, which run the race itself.
const RACE_VARIABLE: &str = "EUPHORIA_KEYFINDER_FIRST_USE_RACE";
const TEST_NAME: &str = "many_threads_can_use_libkeyfinder_for_the_first_time_at_once";
const PROCESS_COUNT: usize = 40;
const THREAD_COUNT: usize = 32;
const SAMPLE_RATE: u32 = 44_100;

#[test]
fn many_threads_can_use_libkeyfinder_for_the_first_time_at_once() {
    if std::env::var_os(RACE_VARIABLE).is_some() {
        race_to_the_first_key();
        return;
    }
    let executable = std::env::current_exe().expect("the test knows its own executable");
    let mut failures = Vec::new();
    for _ in 0..PROCESS_COUNT {
        let output = Command::new(&executable)
            .args([TEST_NAME, "--exact", "--nocapture", "--test-threads", "1"])
            .env(RACE_VARIABLE, "1")
            .output()
            .expect("the test starts itself again");
        if !output.status.success() {
            failures.push(format!(
                "{}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
                    .lines()
                    .find(|line| line.contains("panicked") || line.contains("failed"))
                    .unwrap_or("no message")
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {PROCESS_COUNT} processes failed, the first with {}",
        failures.len(),
        failures[0]
    );
}

/// Starts every thread at once on audio so short that each reaches the
/// scoring of keys at nearly the same moment.
fn race_to_the_first_key() {
    let start = Arc::new(Barrier::new(THREAD_COUNT));
    let threads: Vec<_> = (0..THREAD_COUNT)
        .map(|_| {
            let start = Arc::clone(&start);
            thread::spawn(move || {
                let audio = a_minor_chord();
                start.wait();
                KeyFinderDetector.detect_key(&audio)
            })
        })
        .collect();
    for thread in threads {
        let key = thread
            .join()
            .expect("the thread finished")
            .unwrap_or_else(|error| panic!("key detection failed: {}", error.chain()));
        assert!(key.is_some(), "no key found in a chord");
    }
}

fn a_minor_chord() -> AudioBuffer {
    let samples = (0..SAMPLE_RATE as usize / 10)
        .map(|index| {
            let time = index as f32 / SAMPLE_RATE as f32;
            [220.0_f32, 261.63, 329.63]
                .iter()
                .map(|frequency| 0.2 * (std::f32::consts::TAU * frequency * time).sin())
                .sum()
        })
        .collect();
    AudioBuffer::new(
        samples,
        NonZeroU32::new(SAMPLE_RATE).expect("the sample rate is not zero"),
    )
}
