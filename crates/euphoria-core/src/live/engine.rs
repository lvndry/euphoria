// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::fingerprint::{FingerprintIndex, Match, fingerprint};

use super::analyzer::{LiveAnalyzer, LiveReading, LiveSettings};
use super::capture::{AudioCapture, CaptureError, ListenSource};

/// Where the engine finds the fingerprints of a library to recognise the
/// audio against. It can be filled after listening started.
pub type MatcherHandle = Arc<RwLock<Option<Arc<FingerprintIndex>>>>;

/// How much of the latest audio is matched, and how much is needed first.
const MATCH_SECONDS: f32 = 15.0;
const MINIMUM_MATCH_SECONDS: f32 = 8.0;
/// The audio is matched on every this many readings.
const MATCH_EVERY_READINGS: u32 = 3;

/// How often the key, tempo and energy are read again.
pub const ANALYSIS_INTERVAL: Duration = Duration::from_secs(2);

/// A running live analysis: the capture feeding an analyzer on its own
/// thread. The latest reading is available at any time without waiting for
/// an analysis. Dropping the engine stops listening and forgets the audio.
pub struct LiveEngine {
    capture: Box<dyn AudioCapture>,
    latest: Arc<Mutex<LiveReading>>,
    worker: Option<JoinHandle<()>>,
    source: ListenSource,
}

impl LiveEngine {
    /// Starts listening to `source` with `capture`.
    pub fn start(
        mut capture: Box<dyn AudioCapture>,
        source: ListenSource,
        settings: LiveSettings,
        interval: Duration,
        matcher: MatcherHandle,
    ) -> Result<Self, CaptureError> {
        let (sender, receiver) = mpsc::channel::<Vec<f32>>();
        let sink_sender: Sender<Vec<f32>> = sender;
        let sample_rate = capture.start(
            &source,
            Box::new(move |samples| {
                let _ = sink_sender.send(samples.to_vec());
            }),
        )?;
        let latest = Arc::new(Mutex::new(LiveReading::listening()));
        let shared = Arc::clone(&latest);
        let worker = std::thread::Builder::new()
            .name("euphoria-live".into())
            .spawn(move || {
                let mut analyzer = LiveAnalyzer::new(sample_rate, settings);
                let mut last_analysis = Instant::now();
                let mut readings_made = 0_u32;
                let mut last_match: Option<Match> = None;
                loop {
                    match receiver.recv_timeout(Duration::from_millis(100)) {
                        Ok(samples) => {
                            analyzer.push(&samples);
                            while let Ok(more) = receiver.try_recv() {
                                analyzer.push(&more);
                            }
                        }
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                    if last_analysis.elapsed() >= interval {
                        last_analysis = Instant::now();
                        let mut reading = analyzer
                            .analyze()
                            .unwrap_or_else(|_| analyzer.listening_reading());
                        if reading.listening || reading.heard_seconds < MINIMUM_MATCH_SECONDS {
                            last_match = None;
                        } else if readings_made.is_multiple_of(MATCH_EVERY_READINGS) {
                            let index = matcher
                                .read()
                                .ok()
                                .and_then(|handle| handle.as_ref().map(Arc::clone));
                            last_match = index.and_then(|index| {
                                index.find(&fingerprint(&analyzer.recent_audio(MATCH_SECONDS)))
                            });
                        }
                        readings_made = readings_made.wrapping_add(1);
                        reading.matched = last_match;
                        if let Ok(mut current) = shared.lock() {
                            *current = reading;
                        }
                    } else if let Ok(mut current) = shared.lock() {
                        current.level = analyzer.level();
                        if analyzer.is_quiet() {
                            *current = analyzer.listening_reading();
                        }
                    }
                }
            })
            .map_err(|error| CaptureError::Failed(format!("cannot start the analysis: {error}")))?;
        Ok(Self {
            capture,
            latest,
            worker: Some(worker),
            source,
        })
    }

    /// What is being listened to.
    pub fn source(&self) -> &ListenSource {
        &self.source
    }

    /// The latest reading.
    pub fn latest(&self) -> LiveReading {
        self.latest
            .lock()
            .map(|reading| *reading)
            .unwrap_or_else(|_| LiveReading::listening())
    }

    /// Stops listening and waits for the analysis thread to end.
    pub fn stop(&mut self) {
        self.capture.stop();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for LiveEngine {
    fn drop(&mut self) {
        self.stop();
    }
}
