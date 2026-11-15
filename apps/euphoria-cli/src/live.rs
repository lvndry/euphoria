// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The `live` command: the key and tempo of the audio the computer is
//! playing, one line per update, from the same capture and analysis as the
//! Live window of the desktop app. The audio stays in memory.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Result, bail};
use clap::Args;
use euphoria_core::live::{
    AudioCapture, CaptureError, ListenSource, LiveEngine, LiveReading, LiveSettings, MatcherHandle,
};
use serde_json::json;

const DEFAULT_INTERVAL_SECONDS: f32 = 2.0;
const SHORTEST_INTERVAL_SECONDS: f32 = 0.5;
const HEARD_NOTHING_HINT_SECONDS: f32 = 10.0;
const STEADY_AFTER: Duration = Duration::from_secs(6);

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
    /// Recognise which of these tracks is playing, also when it is played
    /// faster or slower by up to 10 percent: folders or audio files. Their
    /// fingerprints are made once and kept in the cache, so the first run
    /// over a large folder takes a while.
    #[arg(long, value_name = "FOLDER|FILE")]
    from: Vec<PathBuf>,
}

/// The tracks Live recognises what it hears as.
pub(crate) struct LiveLibrary {
    pub(crate) matcher: MatcherHandle,
    pub(crate) paths: Vec<PathBuf>,
}

fn learn_library(
    arguments: &LiveArguments,
    messages: &mut dyn Write,
) -> Result<Option<LiveLibrary>> {
    if arguments.from.is_empty() {
        return Ok(None);
    }
    let cache_path = euphoria_core::default_cache_path().map_err(crate::with_causes)?;
    let analyzer = euphoria_core::Cache::open(&cache_path)
        .and_then(euphoria_core::CachedAnalyzer::new)
        .map_err(crate::with_causes)?;
    let scan = euphoria_core::scan_audio_files(&arguments.from).map_err(crate::with_causes)?;
    let paths = scan.files;
    if paths.is_empty() {
        bail!("no audio files found in --from");
    }
    if !arguments.json {
        writeln!(
            messages,
            "Learning {} tracks to recognise them; the first time takes a while.",
            paths.len()
        )?;
    }
    let learned = analyzer.fingerprint_library(&paths, &|_| {}, &|| false);
    if !arguments.json {
        writeln!(
            messages,
            "Recognising {} tracks ({} learned now, {} unreadable).",
            paths.len() - learned.failed,
            learned.made,
            learned.failed
        )?;
    }
    let matcher = MatcherHandle::default();
    if let Ok(mut handle) = matcher.write() {
        *handle = Some(Arc::new(learned.index));
    }
    Ok(Some(LiveLibrary { matcher, paths }))
}

pub fn run(arguments: &LiveArguments) -> Result<ExitCode> {
    let capture = platform_capture()?;
    let mut standard_output = std::io::stdout().lock();
    let mut messages = std::io::stderr().lock();
    let library = learn_library(arguments, &mut messages)?;
    run_with(
        capture,
        arguments,
        library,
        &mut standard_output,
        &mut messages,
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
    library: Option<LiveLibrary>,
    output: &mut dyn Write,
    messages: &mut dyn Write,
    should_stop: &dyn Fn() -> bool,
) -> Result<ExitCode> {
    if !arguments.interval.is_finite() || arguments.interval < SHORTEST_INTERVAL_SECONDS {
        bail!("--interval must be at least {SHORTEST_INTERVAL_SECONDS} seconds");
    }
    let interval = Duration::from_secs_f32(arguments.interval);
    let source = choose_source(capture.as_ref(), arguments.source.as_deref())?;
    let (matcher, paths) = match library {
        Some(library) => (library.matcher, library.paths),
        None => (MatcherHandle::default(), Vec::new()),
    };
    let engine = LiveEngine::start(
        capture,
        source.clone(),
        LiveSettings::default(),
        interval,
        matcher,
    )
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
    let mut stability = Stability::default();
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
            let steady = stability.steady(&reading);
            let matched_path = reading
                .matched
                .and_then(|found| paths.get(found.track))
                .cloned();
            let matched_name = matched_path.as_deref().map(track_name);
            if arguments.json {
                let mut object = reading_json(&reading, source.label(), &paths);
                object["steady"] = json!(steady);
                if let (Some(name), Some(found)) = (&matched_name, object["match"].as_object_mut())
                {
                    found.insert("title".into(), json!(name.title));
                    found.insert("artist".into(), json!(name.artist));
                }
                writeln!(output, "{object}")?;
            } else {
                writeln!(
                    output,
                    "{}",
                    reading_line(
                        &reading,
                        steady,
                        matched_name.as_ref().map(|name| name.title.as_str())
                    )
                )?;
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

pub(crate) fn reading_line(
    reading: &LiveReading,
    steady: bool,
    matched_title: Option<&str>,
) -> String {
    let key = reading.key.map_or_else(
        || "--  --".to_string(),
        |live| format!("{}  {}", live.key.camelot(), live.key),
    );
    let tempo = reading.tempo.map_or_else(
        || "-- BPM".to_string(),
        |live| format!("{:.1} BPM", live.bpm),
    );
    let energy = reading
        .energy
        .map_or_else(String::new, |energy| format!("  energy {energy}/10"));
    let state = if steady { "steady" } else { "settling" };
    let matched = matched_title
        .map(|title| format!("  matched: {title}"))
        .unwrap_or_default();
    format!("{key}  {tempo}{energy}  {state}{matched}")
}

/// How long the key and tempo have stayed the same, so a line can say the
/// reading is steady.
#[derive(Default)]
struct Stability {
    signature: Option<(euphoria_core::Key, i32)>,
    since: Option<std::time::Instant>,
}

impl Stability {
    fn steady(&mut self, reading: &LiveReading) -> bool {
        let signature = reading.key.map(|live| {
            (
                live.key,
                reading.tempo.map_or(0, |tempo| tempo.bpm.round() as i32),
            )
        });
        if signature != self.signature || self.since.is_none() {
            self.signature = signature;
            self.since = Some(std::time::Instant::now());
        }
        self.since
            .is_some_and(|since| since.elapsed() >= STEADY_AFTER)
    }
}

/// The title and artist of a track as its tags say, or its file name.
struct TrackName {
    title: String,
    artist: Option<String>,
}

fn track_name(path: &std::path::Path) -> TrackName {
    let tags = euphoria_core::read_track_name(path).ok();
    let title = tags
        .as_ref()
        .and_then(|name| name.title.clone())
        .or_else(|| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
        })
        .unwrap_or_default();
    TrackName {
        title,
        artist: tags.and_then(|name| name.artist),
    }
}

pub(crate) fn reading_json(
    reading: &LiveReading,
    source: &str,
    paths: &[PathBuf],
) -> serde_json::Value {
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
        "match": reading.matched.and_then(|found| {
            paths.get(found.track).map(|path| json!({
                "path": path,
                "position_seconds": (found.start_seconds
                    + reading.heard_seconds.min(15.0) * found.playback.tempo_ratio)
                    .round(),
                "tempo_ratio": (found.playback.tempo_ratio * 100.0).round() / 100.0,
                "pitch_follows_tempo": found.playback.pitch_follows_tempo,
            }))
        }),
        "level_db": reading.level.map(|level| level.rms_db.round()),
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::f32::consts::TAU;
    use std::io::Write;
    use std::num::NonZeroU32;
    use std::rc::Rc;
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
            from: Vec::new(),
        }
    }

    #[derive(Clone, Default)]
    struct Shared(Rc<RefCell<Vec<u8>>>);

    impl Write for Shared {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn is_reading(line: &str) -> bool {
        !line.starts_with("listening") && !line.contains("\"listening\":true")
    }

    fn run_for(audio: Vec<f32>, json: bool, readings: usize, polls: usize) -> (String, String) {
        run_library(audio, json, readings, polls, None)
    }

    fn run_library(
        audio: Vec<f32>,
        json: bool,
        readings: usize,
        polls: usize,
        library: Option<LiveLibrary>,
    ) -> (String, String) {
        let capture = Playing {
            audio,
            sink: None,
            sources: Vec::new(),
        };
        let counter = Arc::new(AtomicUsize::new(0));
        let mut output = Shared::default();
        let seen = output.0.clone();
        let mut messages = Vec::new();
        let _ = run_with(
            Box::new(capture),
            &arguments(json),
            library,
            &mut output,
            &mut messages,
            &|| {
                let text = String::from_utf8_lossy(&seen.borrow()).into_owned();
                text.lines().filter(|line| is_reading(line)).count() >= readings
                    || counter.fetch_add(1, Ordering::SeqCst) + 1 >= polls
            },
        )
        .unwrap();
        let text = String::from_utf8(seen.borrow().clone()).unwrap();
        (text, String::from_utf8(messages).unwrap())
    }

    fn near_the_intended_key(camelot: &str) -> bool {
        let number: u8 = camelot.trim_end_matches(['A', 'B']).parse().unwrap();
        (7..=9).contains(&number)
    }

    #[test]
    fn it_prints_one_line_per_update_with_key_and_tempo() {
        let (output, messages) = run_for(programme(14.0), false, 2, 240);
        let lines: Vec<&str> = output.lines().filter(|line| is_reading(line)).collect();
        assert!(lines.len() >= 2, "{output}");
        let last = lines.last().unwrap();
        assert!(
            near_the_intended_key(last.split("  ").next().unwrap()),
            "{last}"
        );
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
        let (output, messages) = run_for(programme(14.0), true, 2, 240);
        let last: serde_json::Value = serde_json::from_str(output.lines().last().unwrap()).unwrap();
        assert_eq!(last["listening"], false);
        assert!(near_the_intended_key(
            last["key"]["camelot"].as_str().unwrap()
        ));
        assert!((last["bpm"].as_f64().unwrap() - 126.0).abs() <= 2.0);
        assert!(last["energy"].is_number());
        assert!(messages.is_empty());
        for line in output.lines() {
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        }
    }

    #[test]
    fn silence_says_listening_once() {
        let (output, _) = run_for(vec![0.0; SAMPLE_RATE as usize * 10], false, usize::MAX, 5);
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
            None,
            &mut Vec::new(),
            &mut Vec::new(),
            &|| true,
        );
        assert!(result.is_err());
    }

    fn melody(seed: u32, seconds: f32) -> Vec<f32> {
        let total = (seconds * SAMPLE_RATE as f32) as usize;
        let mut samples = vec![0.0; total];
        let mut state = seed.wrapping_mul(2_654_435_761) | 1;
        let step = SAMPLE_RATE as usize / 4;
        let mut start = 0;
        while start < total {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let frequency = 220.0 * 2.0_f32.powf(((state >> 24) % 24) as f32 / 12.0);
            for offset in 0..(step * 2).min(total - start) {
                let time = offset as f32 / SAMPLE_RATE as f32;
                samples[start + offset] += 0.3
                    * (-time * 5.0).exp()
                    * ((TAU * frequency * time).sin() + 0.4 * (TAU * 2.0 * frequency * time).sin());
            }
            start += step;
        }
        samples
    }

    fn library_of_melodies() -> (Vec<Vec<f32>>, LiveLibrary) {
        let tracks: Vec<Vec<f32>> = (1..=4).map(|seed| melody(seed, 60.0)).collect();
        let mut index = euphoria_core::fingerprint::FingerprintIndex::new();
        for (id, track) in tracks.iter().enumerate() {
            let audio = euphoria_core::AudioBuffer::new(
                track.clone(),
                NonZeroU32::new(SAMPLE_RATE).unwrap(),
            );
            index.add(id, &euphoria_core::fingerprint::fingerprint(&audio));
        }
        index.freeze();
        let matcher = MatcherHandle::default();
        *matcher.write().unwrap() = Some(Arc::new(index));
        let paths = (1..=4)
            .map(|number| PathBuf::from(format!("/music/track {number}.flac")))
            .collect();
        (tracks, LiveLibrary { matcher, paths })
    }

    #[test]
    fn a_recognised_track_is_named_on_the_reading_line() {
        let (tracks, library) = library_of_melodies();
        let played = tracks[2][SAMPLE_RATE as usize * 15..SAMPLE_RATE as usize * 30].to_vec();
        let (output, _) = run_library(played, false, 3, 240, Some(library));
        let titled: Vec<&str> = output
            .lines()
            .filter(|line| line.ends_with("matched: track 3"))
            .collect();
        assert!(!titled.is_empty(), "{output}");
        assert!(
            titled
                .iter()
                .all(|line| line.contains("energy ") && line.contains("/10"))
        );
    }

    #[test]
    fn json_names_the_match_and_how_it_is_played() {
        let (tracks, library) = library_of_melodies();
        let played = tracks[0][SAMPLE_RATE as usize * 15..SAMPLE_RATE as usize * 30].to_vec();
        let (output, _) = run_library(played, true, 3, 240, Some(library));
        let last: serde_json::Value = serde_json::from_str(output.lines().last().unwrap()).unwrap();
        assert_eq!(last["match"]["path"], "/music/track 1.flac");
        assert_eq!(last["match"]["pitch_follows_tempo"], false);
        assert!((last["match"]["tempo_ratio"].as_f64().unwrap() - 1.0).abs() <= 0.021);
        let position = last["match"]["position_seconds"].as_f64().unwrap();
        assert!((position - 30.0).abs() <= 3.0, "{position}");
    }

    #[test]
    fn without_a_library_json_has_no_match() {
        let (output, _) = run_for(programme(14.0), true, 2, 240);
        let last: serde_json::Value = serde_json::from_str(output.lines().last().unwrap()).unwrap();
        assert!(last["match"].is_null());
    }

    #[test]
    fn a_reading_is_settling_until_it_has_held_for_a_few_seconds() {
        let mut reading = LiveReading::listening();
        reading.listening = false;
        reading.key = Some(euphoria_core::live::LiveKey {
            key: "8B".parse().unwrap(),
            confidence: euphoria_core::Confidence::High,
            runner_up: None,
        });
        let mut stability = Stability::default();
        assert!(!stability.steady(&reading));
        stability.since = Some(std::time::Instant::now() - STEADY_AFTER);
        assert!(stability.steady(&reading));
        reading.key = Some(euphoria_core::live::LiveKey {
            key: "9B".parse().unwrap(),
            ..reading.key.unwrap()
        });
        assert!(!stability.steady(&reading), "a new key starts again");
        assert!(reading_line(&reading, false, None).ends_with("settling"));
        assert!(
            reading_line(&reading, true, Some("Out Out")).ends_with("steady  matched: Out Out")
        );
    }
}
