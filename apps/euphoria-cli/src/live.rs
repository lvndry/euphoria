// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `live` command: the key and tempo of the audio the computer is
//! playing, one line per update, from the same capture and analysis as the
//! Live window of the desktop app. The audio stays in memory.

use std::io::Write;
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Result, bail};
use clap::Args;
use euphoria_core::live::{
    AudioCapture, CaptureError, ListenSource, LiveEngine, LiveReading, LiveSettings,
};
use serde_json::json;

const DEFAULT_INTERVAL_SECONDS: f32 = 2.0;
const SHORTEST_INTERVAL_SECONDS: f32 = 0.5;
const HEARD_NOTHING_HINT_SECONDS: f32 = 10.0;

#[derive(Debug, Args)]
pub struct LiveArguments {
    /// Listen to one application, by the start of its name, such as
    /// "rekordbox". Without it, everything the computer plays is heard.
    #[arg(long, value_name = "PROCESS")]
    source: Option<String>,
    /// Seconds between two readings.
    #[arg(long, default_value_t = DEFAULT_INTERVAL_SECONDS, value_name = "SECONDS")]
    interval: f32,
    /// Print one JSON object per line, for scripts and stream overlays.
    #[arg(long)]
    json: bool,
}

pub fn run(arguments: &LiveArguments) -> Result<ExitCode> {
    let capture = platform_capture()?;
    let mut standard_output = std::io::stdout().lock();
    run_with(
        capture,
        arguments,
        &mut standard_output,
        &mut std::io::stderr().lock(),
        &|| false,
    )
}

#[cfg(target_os = "macos")]
fn platform_capture() -> Result<Box<dyn AudioCapture>> {
    if !euphoria_core::live::ProcessTapCapture::is_supported() {
        bail!("Live needs macOS 14.2 or later.");
    }
    Ok(Box::new(euphoria_core::live::ProcessTapCapture::new()))
}

#[cfg(not(target_os = "macos"))]
fn platform_capture() -> Result<Box<dyn AudioCapture>> {
    bail!("Live listens to the audio of the Mac it runs on; Linux is not supported yet.")
}

pub(crate) fn run_with(
    capture: Box<dyn AudioCapture>,
    arguments: &LiveArguments,
    output: &mut dyn Write,
    messages: &mut dyn Write,
    should_stop: &dyn Fn() -> bool,
) -> Result<ExitCode> {
    if !arguments.interval.is_finite() || arguments.interval < SHORTEST_INTERVAL_SECONDS {
        bail!("--interval must be at least {SHORTEST_INTERVAL_SECONDS} seconds");
    }
    let interval = Duration::from_secs_f32(arguments.interval);
    let source = choose_source(capture.as_ref(), arguments.source.as_deref())?;
    let engine = LiveEngine::start(capture, source.clone(), LiveSettings::default(), interval)
        .map_err(describe_capture_error)?;
    if !arguments.json {
        writeln!(
            messages,
            "Listening to {}. Nothing is recorded; press Ctrl-C to stop.",
            source.label()
        )?;
    }
    let mut was_listening = false;
    let mut hinted = false;
    loop {
        std::thread::sleep(interval);
        let reading = engine.latest();
        if reading.listening || (reading.key.is_none() && reading.tempo.is_none()) {
            if !was_listening {
                if arguments.json {
                    writeln!(output, "{}", json!({ "listening": true }))?;
                } else {
                    writeln!(output, "listening\u{2026}")?;
                }
                output.flush()?;
                was_listening = true;
            }
            if reading.no_signal_seconds >= HEARD_NOTHING_HINT_SECONDS && !hinted && !arguments.json
            {
                writeln!(
                    messages,
                    "Only silence so far. If audio is playing, allow Euphoria or your terminal \
                     under System Settings > Privacy & Security > Screen & System Audio Recording."
                )?;
                hinted = true;
            }
        } else {
            was_listening = false;
            hinted = false;
            if arguments.json {
                writeln!(output, "{}", reading_json(&reading, source.label()))?;
            } else {
                writeln!(output, "{}", reading_line(&reading))?;
            }
            output.flush()?;
        }
        if should_stop() {
            return Ok(ExitCode::SUCCESS);
        }
    }
}

fn choose_source(capture: &dyn AudioCapture, wanted: Option<&str>) -> Result<ListenSource> {
    let Some(wanted) = wanted else {
        return Ok(ListenSource::AllAudio);
    };
    let wanted_lowercase = wanted.to_lowercase();
    let sources = capture.sources();
    if let Some(source) = sources
        .iter()
        .find(|source| source.label().to_lowercase().starts_with(&wanted_lowercase))
    {
        return Ok(source.clone());
    }
    let playing: Vec<&str> = sources.iter().map(ListenSource::label).collect();
    if playing.is_empty() {
        bail!("no application named {wanted} is playing audio right now");
    }
    bail!(
        "no application named {wanted} is playing audio right now; playing: {}",
        playing.join(", ")
    )
}

fn describe_capture_error(error: CaptureError) -> anyhow::Error {
    anyhow::anyhow!("{error}")
}

pub(crate) fn reading_line(reading: &LiveReading) -> String {
    let key = reading.key.map_or_else(
        || "--  --".to_string(),
        |live| format!("{}  {}", live.key.camelot(), live.key),
    );
    let tempo = reading.tempo.map_or_else(
        || "-- BPM".to_string(),
        |live| format!("{:.1} BPM", live.bpm),
    );
    let confidence = reading
        .key
        .map(|live| live.confidence)
        .or(reading.tempo.map(|live| live.confidence))
        .map_or("", |confidence| confidence.as_str());
    format!("{key}  {tempo}  {confidence}")
        .trim_end()
        .to_string()
}

pub(crate) fn reading_json(reading: &LiveReading, source: &str) -> serde_json::Value {
    json!({
        "listening": false,
        "source": source,
        "key": reading.key.map(|live| json!({
            "camelot": live.key.camelot(),
            "name": live.key.to_string(),
            "confidence": live.confidence.as_str(),
        })),
        "bpm": reading.tempo.map(|live| f64::from((live.bpm * 10.0).round() as i32) / 10.0),
        "bpm_confidence": reading.tempo.map(|live| live.confidence.as_str()),
        "energy": reading.energy,
        "level_db": reading.level.map(|level| level.rms_db.round()),
    })
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;
    use std::num::NonZeroU32;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use euphoria_core::live::AudioSink;

    use super::*;

    const SAMPLE_RATE: u32 = 44_100;

    fn programme(seconds: f32) -> Vec<f32> {
        let total = (seconds * SAMPLE_RATE as f32) as usize;
        let beat = (60.0 / 126.0 * SAMPLE_RATE as f32) as usize;
        let mut samples = vec![0.0; total];
        for (index, sample) in samples.iter_mut().enumerate() {
            let time = index as f32 / SAMPLE_RATE as f32;
            for frequency in [130.81_f32, 164.81, 196.0] {
                for harmonic in 1..=4 {
                    *sample +=
                        0.08 * (TAU * frequency * harmonic as f32 * time).sin() / harmonic as f32;
                }
            }
            let since_beat = (index % beat) as f32 / SAMPLE_RATE as f32;
            let frequency = 50.0 + 110.0 * (-since_beat * 30.0).exp();
            *sample += 0.7 * (-since_beat * 14.0).exp() * (TAU * frequency * since_beat).sin();
        }
        samples
    }

    struct Playing {
        audio: Vec<f32>,
        sink: Option<AudioSink>,
        sources: Vec<ListenSource>,
    }

    impl AudioCapture for Playing {
        fn sources(&self) -> Vec<ListenSource> {
            self.sources.clone()
        }

        fn start(
            &mut self,
            _source: &ListenSource,
            mut sink: AudioSink,
        ) -> Result<NonZeroU32, CaptureError> {
            for chunk in self.audio.chunks(441) {
                sink(chunk);
            }
            self.sink = Some(sink);
            Ok(NonZeroU32::new(SAMPLE_RATE).unwrap())
        }

        fn stop(&mut self) {
            self.sink = None;
        }
    }

    fn arguments(json: bool) -> LiveArguments {
        LiveArguments {
            source: None,
            interval: 0.5,
            json,
        }
    }

    fn run_for(audio: Vec<f32>, json: bool, polls: usize) -> (String, String) {
        let capture = Playing {
            audio,
            sink: None,
            sources: Vec::new(),
        };
        let counter = Arc::new(AtomicUsize::new(0));
        let mut output = Vec::new();
        let mut messages = Vec::new();
        run_with(
            Box::new(capture),
            &arguments(json),
            &mut output,
            &mut messages,
            &|| counter.fetch_add(1, Ordering::SeqCst) + 1 >= polls,
        )
        .unwrap();
        (
            String::from_utf8(output).unwrap(),
            String::from_utf8(messages).unwrap(),
        )
    }

    #[test]
    fn it_prints_one_line_per_update_with_key_and_tempo() {
        let (output, messages) = run_for(programme(14.0), false, 4);
        let lines: Vec<&str> = output.lines().collect();
        assert!(lines.len() >= 2, "{output}");
        let last = lines.last().unwrap();
        assert!(last.starts_with("8B  C"), "{last}");
        let bpm: f32 = last
            .split("  ")
            .find_map(|part| part.strip_suffix(" BPM"))
            .and_then(|text| text.parse().ok())
            .expect("a tempo in the line");
        assert!((bpm - 126.0).abs() <= 2.0, "{last}");
        assert!(messages.contains("Nothing is recorded"));
    }

    #[test]
    fn json_is_one_object_per_line() {
        let (output, messages) = run_for(programme(14.0), true, 4);
        let last: serde_json::Value = serde_json::from_str(output.lines().last().unwrap()).unwrap();
        assert_eq!(last["listening"], false);
        assert_eq!(last["key"]["camelot"], "8B");
        assert!((last["bpm"].as_f64().unwrap() - 126.0).abs() <= 2.0);
        assert!(last["energy"].is_number());
        assert!(messages.is_empty());
        for line in output.lines() {
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        }
    }

    #[test]
    fn silence_says_listening_once() {
        let (output, _) = run_for(vec![0.0; SAMPLE_RATE as usize * 10], false, 5);
        assert_eq!(output, "listening\u{2026}\n");
    }

    #[test]
    fn an_unknown_source_names_what_is_playing() {
        let capture = Playing {
            audio: Vec::new(),
            sink: None,
            sources: vec![ListenSource::Process {
                process_id: 7,
                name: "rekordbox".into(),
            }],
        };
        let found = choose_source(&capture, Some("Rekord")).unwrap();
        assert_eq!(found.label(), "rekordbox");
        let error = choose_source(&capture, Some("spotify")).unwrap_err();
        assert!(error.to_string().contains("playing: rekordbox"), "{error}");
    }

    #[test]
    fn the_interval_has_a_floor() {
        let capture = Playing {
            audio: Vec::new(),
            sink: None,
            sources: Vec::new(),
        };
        let mut options = arguments(false);
        options.interval = 0.1;
        let result = run_with(
            Box::new(capture),
            &options,
            &mut Vec::new(),
            &mut Vec::new(),
            &|| true,
        );
        assert!(result.is_err());
    }
}
