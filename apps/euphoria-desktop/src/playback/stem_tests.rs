// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! Stems played back against the track they were separated from. The
//! stems are made here, the way the separator leaves them: four 16 bit
//! stereo WAV files at 44.1 kHz that add up to the track. The track itself
//! is written at 44.1 kHz, at 48 kHz and in mono, from the same functions
//! of time, so the stems at 44.1 kHz are exactly what resampling the track
//! to 44.1 kHz would give, without a resampler in the test.

use std::f64::consts::TAU;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use euphoria_core::stems::{STEMS_SAMPLE_RATE, Stem, StemFiles};

use super::{StemMix, TrackSource};

const TRACK_SECONDS: f64 = 3.0;

/// Full scale is shared between the four stems so their sum never clips,
/// as in a mastered track.
const STEM_PEAK: f64 = 0.2;

/// Below this, the stems played together are not the track. Four stems
/// each rounded to 16 bits come out about 76 dB under this material, so
/// 60 dB leaves room for that rounding and little else: stems out of step
/// by a few frames lose tens of dB on the drums, and reading at the wrong
/// speed gives about -3 dB.
const MINIMUM_SNR_DB: f64 = 60.0;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "euphoria-stem-playback-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        Self(directory)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A little pseudo random noise, the same for the same `seed`.
fn noise(seed: u64) -> f64 {
    let mut state = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    state ^= state >> 33;
    state = state.wrapping_mul(0xff51_afd7_ed55_8ccd);
    state ^= state >> 33;
    (state >> 11) as f64 / (1_u64 << 53) as f64 * 2.0 - 1.0
}

/// The value of `stem` on `channel` at `seconds`: drums are decaying
/// noise bursts on every beat, bass a low tone, the rest a chord, and the
/// vocals a tone with vibrato, each panned its own way.
fn stem_value(stem: Stem, channel: usize, seconds: f64) -> f64 {
    let pan = |left: f64| if channel == 0 { left } else { 1.0 - left };
    let value = match stem {
        Stem::Drums => {
            let beat = 0.5;
            let since_beat = seconds % beat;
            let grain = (seconds * 22_050.0) as u64;
            (-since_beat * 30.0).exp() * noise(grain * 2 + channel as u64) * pan(0.5)
        }
        Stem::Bass => (TAU * 55.0 * seconds).sin() * pan(0.5),
        Stem::Other => {
            [220.0, 277.18, 329.63]
                .iter()
                .map(|frequency| (TAU * frequency * seconds).sin())
                .sum::<f64>()
                / 3.0
                * pan(0.3)
        }
        Stem::Vocals => {
            let vibrato = 6.0 * (TAU * 5.0 * seconds).sin();
            (TAU * (440.0 * seconds) + vibrato / 5.0).sin() * pan(0.6)
        }
    };
    value * STEM_PEAK
}

fn mono_value(seconds: f64) -> f64 {
    Stem::ALL
        .iter()
        .map(|stem| (stem_value(*stem, 0, seconds) + stem_value(*stem, 1, seconds)) / 2.0)
        .sum()
}

fn stereo_value(channel: usize, seconds: f64) -> f64 {
    Stem::ALL
        .iter()
        .map(|stem| stem_value(*stem, channel, seconds))
        .sum()
}

/// Samples of a sound of `channels` channels at `sample_rate`, `value`
/// giving each sample of each channel at a time.
fn render(channels: usize, sample_rate: u32, value: impl Fn(usize, f64) -> f64) -> Vec<f32> {
    let frames = (TRACK_SECONDS * f64::from(sample_rate)).round() as usize;
    (0..frames)
        .flat_map(|frame| {
            let seconds = frame as f64 / f64::from(sample_rate);
            (0..channels).map(move |channel| (channel, seconds))
        })
        .map(|(channel, seconds)| value(channel, seconds) as f32)
        .collect()
}

fn write_wav(path: &Path, interleaved: &[f32], channels: u16, sample_rate: u32) {
    let data_bytes = (interleaved.len() * 2) as u32;
    let mut writer = BufWriter::new(File::create(path).unwrap());
    writer.write_all(b"RIFF").unwrap();
    writer.write_all(&(36 + data_bytes).to_le_bytes()).unwrap();
    writer.write_all(b"WAVEfmt ").unwrap();
    writer.write_all(&16_u32.to_le_bytes()).unwrap();
    writer.write_all(&1_u16.to_le_bytes()).unwrap();
    writer.write_all(&channels.to_le_bytes()).unwrap();
    writer.write_all(&sample_rate.to_le_bytes()).unwrap();
    writer
        .write_all(&(sample_rate * u32::from(channels) * 2).to_le_bytes())
        .unwrap();
    writer.write_all(&(channels * 2).to_le_bytes()).unwrap();
    writer.write_all(&16_u16.to_le_bytes()).unwrap();
    writer.write_all(b"data").unwrap();
    writer.write_all(&data_bytes.to_le_bytes()).unwrap();
    for sample in interleaved {
        let quantised = (f64::from(*sample) * 32_767.0)
            .round()
            .clamp(-32_768.0, 32_767.0) as i16;
        writer.write_all(&quantised.to_le_bytes()).unwrap();
    }
    writer.flush().unwrap();
}

/// A track as the user has it, at `sample_rate`, mono or stereo, and the
/// stems the separator would have cached for it.
struct SeparatedTrack {
    _scratch: Scratch,
    original: PathBuf,
    stems: StemFiles,
    original_channels: usize,
}

fn separated_track(name: &str, sample_rate: u32, original_channels: usize) -> SeparatedTrack {
    let scratch = Scratch::new(name);
    let original = scratch.0.join("track.wav");
    let samples = if original_channels == 1 {
        render(1, sample_rate, |_, seconds| mono_value(seconds))
    } else {
        render(2, sample_rate, stereo_value)
    };
    write_wav(&original, &samples, original_channels as u16, sample_rate);

    let stems = StemFiles::in_directory(scratch.0.join("stems"));
    std::fs::create_dir_all(stems.directory()).unwrap();
    for stem in Stem::ALL {
        let samples = render(2, STEMS_SAMPLE_RATE, |channel, seconds| {
            if original_channels == 1 {
                (stem_value(stem, 0, seconds) + stem_value(stem, 1, seconds)) / 2.0
            } else {
                stem_value(stem, channel, seconds)
            }
        });
        write_wav(&stems.path(stem), &samples, 2, STEMS_SAMPLE_RATE);
    }
    SeparatedTrack {
        _scratch: scratch,
        original,
        stems,
        original_channels,
    }
}

impl SeparatedTrack {
    /// The track as it would sound resampled to the stems' rate, in stereo,
    /// from `from_seconds` on.
    fn reference_at_stem_rate(&self, from_seconds: f64) -> Vec<f32> {
        let skipped = (from_seconds * f64::from(STEMS_SAMPLE_RATE)).round() as usize * 2;
        let channels = self.original_channels;
        render(2, STEMS_SAMPLE_RATE, |channel, seconds| {
            if channels == 1 {
                mono_value(seconds)
            } else {
                stereo_value(channel, seconds)
            }
        })
        .split_off(skipped)
    }
}

fn stems_with(
    track: &SeparatedTrack,
    audible: impl Fn(Stem) -> bool,
    from_seconds: f32,
) -> TrackSource {
    let mix = Arc::new(StemMix::default());
    mix.set_audible(audible);
    TrackSource::open_stems(&track.stems, mix, from_seconds)
        .unwrap()
        .patient()
}

/// Every sample the source hands out until it ends, without the silence
/// played while the decoder was behind.
fn play_to_the_end(source: &mut TrackSource) -> Vec<f32> {
    let state = source.state();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut samples = Vec::new();
    loop {
        assert!(Instant::now() < deadline, "the source never finished");
        let silence_before = state.silent_frames();
        let Some(sample) = source.next() else {
            return samples;
        };
        if state.silent_frames() == silence_before {
            samples.push(sample);
        }
    }
}

/// The ratio of the reference's power to the power of what `played` adds
/// to it or takes from it, in dB, over the frames both have.
fn snr_db(reference: &[f32], played: &[f32]) -> f64 {
    let length = reference.len().min(played.len());
    let signal: f64 = reference[..length]
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum();
    let error: f64 = reference[..length]
        .iter()
        .zip(&played[..length])
        .map(|(expected, heard)| (f64::from(*expected) - f64::from(*heard)).powi(2))
        .sum();
    10.0 * (signal / error.max(f64::MIN_POSITIVE)).log10()
}

fn frames_of(samples: &[f32], channels: usize) -> f64 {
    (samples.len() / channels) as f64
}

#[test]
fn all_four_stems_together_sound_like_the_track() {
    for (name, sample_rate, channels) in [
        ("full-44k", 44_100, 2),
        ("full-48k", 48_000, 2),
        ("full-mono", 48_000, 1),
    ] {
        let track = separated_track(name, sample_rate, channels);
        let played = play_to_the_end(&mut stems_with(&track, |_| true, 0.0));
        let reference = track.reference_at_stem_rate(0.0);
        let snr = snr_db(&reference, &played);
        let length_ratio = played.len() as f64 / reference.len() as f64;
        assert!(snr > MINIMUM_SNR_DB, "{name}: {snr:.1} dB");
        assert!(
            (length_ratio - 1.0).abs() < 1.0e-3,
            "{name}: length ratio {length_ratio}"
        );
    }
}

#[test]
fn acapella_and_instrumental_add_up_to_the_track() {
    for (name, sample_rate, channels) in [
        ("parts-44k", 44_100, 2),
        ("parts-48k", 48_000, 2),
        ("parts-mono", 44_100, 1),
    ] {
        let track = separated_track(name, sample_rate, channels);
        let acapella = play_to_the_end(&mut stems_with(&track, |stem| stem == Stem::Vocals, 0.0));
        let instrumental =
            play_to_the_end(&mut stems_with(&track, |stem| stem != Stem::Vocals, 0.0));
        assert_eq!(acapella.len(), instrumental.len(), "{name}");
        let summed: Vec<f32> = acapella
            .iter()
            .zip(&instrumental)
            .map(|(vocals, rest)| vocals + rest)
            .collect();
        let snr = snr_db(&track.reference_at_stem_rate(0.0), &summed);
        assert!(snr > MINIMUM_SNR_DB, "{name}: {snr:.1} dB");
    }
}

#[test]
fn stems_last_as_long_as_the_track_at_any_sample_rate() {
    for (name, sample_rate, channels) in [
        ("length-44k", 44_100, 2),
        ("length-48k", 48_000, 2),
        ("length-mono", 48_000, 1),
    ] {
        let track = separated_track(name, sample_rate, channels);
        let mut original = TrackSource::open(&track.original, 0.0).unwrap().patient();
        let original_rate = f64::from(rodio::Source::sample_rate(&original).get());
        let original_seconds = frames_of(&play_to_the_end(&mut original), channels) / original_rate;

        let mut stems = stems_with(&track, |stem| stem == Stem::Vocals, 0.0);
        let stems_rate = f64::from(rodio::Source::sample_rate(&stems).get());
        let stems_state = stems.state();
        let stems_seconds = frames_of(&play_to_the_end(&mut stems), 2) / stems_rate;

        assert!(
            (stems_seconds - original_seconds).abs() < 1.0e-3,
            "{name}: {stems_seconds} s against {original_seconds} s"
        );
        assert!(
            (f64::from(stems_state.position_seconds()) - original_seconds).abs() < 1.0e-3,
            "{name}"
        );
        assert!(
            (f64::from(stems.duration_seconds().unwrap()) - original_seconds).abs() < 1.0e-3,
            "{name}"
        );
    }
}

#[test]
fn a_seek_through_stems_lands_where_it_lands_in_the_track() {
    for (name, sample_rate) in [("seek-44k", 44_100), ("seek-48k", 48_000)] {
        let track = separated_track(name, sample_rate, 2);
        let mut original = TrackSource::open(&track.original, 0.0).unwrap().patient();
        let mut stems = stems_with(&track, |_| true, 0.0);
        for source in [&mut original, &mut stems] {
            for _ in 0..20_000 {
                source.next();
            }
        }
        let target = 1.25_f32;
        original.state().seek_to(target);
        stems.state().seek_to(target);
        let after_original = play_to_the_end(&mut original);
        let after_stems = play_to_the_end(&mut stems);

        let original_seconds = frames_of(&after_original, 2) / f64::from(sample_rate);
        let stems_seconds = frames_of(&after_stems, 2) / f64::from(STEMS_SAMPLE_RATE);
        assert!(
            (original_seconds - stems_seconds).abs() < 1.0e-3,
            "{name}: {stems_seconds} s left against {original_seconds} s"
        );

        let snr = snr_db(
            &track.reference_at_stem_rate(f64::from(target)),
            &after_stems,
        );
        assert!(snr > MINIMUM_SNR_DB, "{name}: {snr:.1} dB after the seek");
    }
}

#[test]
fn stems_opened_part_of_the_way_in_start_where_the_track_would() {
    let track = separated_track("start-48k", 48_000, 2);
    let mut stems = stems_with(&track, |_| true, 2.0);
    assert!((stems.state().position_seconds() - 2.0).abs() < 1.0e-3);
    let played = play_to_the_end(&mut stems);
    let snr = snr_db(&track.reference_at_stem_rate(2.0), &played);
    assert!(snr > MINIMUM_SNR_DB, "{snr:.1} dB");
}

#[test]
fn a_key_shift_moves_each_stem_as_it_moves_the_stem_alone() {
    let track = separated_track("shift-48k", 48_000, 2);
    let semitones = 5;
    let mix = Arc::new(StemMix::default());
    mix.set_audible(|stem| stem == Stem::Vocals);
    let mut stems = TrackSource::open_stems_shifted(&track.stems, mix, 0.0, semitones)
        .unwrap()
        .patient();
    let through_stems = play_to_the_end(&mut stems);
    let mut alone = TrackSource::open_shifted(&track.stems.path(Stem::Vocals), 0.0, semitones)
        .unwrap()
        .patient();
    let shifted_alone = play_to_the_end(&mut alone);
    assert_eq!(through_stems.len(), shifted_alone.len());
    let snr = snr_db(&shifted_alone, &through_stems);
    assert!(snr > MINIMUM_SNR_DB, "{snr:.1} dB");
}
