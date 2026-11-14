// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The metronome: a steady click, in 4/4 with an accented first beat, mixed
//! into the output beside the decks and the player.
//!
//! The clicks are short bursts made here, sample by sample, so there are
//! no sound files. Every click of every sound is made once, before the
//! first beat, and the audio thread only copies samples out of them and
//! counts frames, so it never allocates, locks or asks the clock the time.
//! Beats are placed by counting frames: beat `n` starts at the frame nearest
//! `n` times the beat length, so the tempo cannot drift however long it
//! runs.
//!
//! The window changes the tempo, the sound, the volume and the accent
//! through a [`MetronomeControl`] of atomics the audio thread reads as it
//! goes. A new tempo is used from the next beat on, without restarting the
//! bar.

use std::f64::consts::TAU;
use std::num::{NonZeroU16, NonZeroU32};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The sample rate the clicks are made at. The output converts it to the
/// device's own.
pub(crate) const SAMPLE_RATE: u32 = 44_100;

/// The slowest tempo the metronome keeps.
pub(crate) const MINIMUM_BPM: f32 = 30.0;

/// The fastest tempo the metronome keeps.
pub(crate) const MAXIMUM_BPM: f32 = 300.0;

/// The tempo before the user types one and before a track gives one.
pub(crate) const STARTING_BPM: f32 = 120.0;

/// Beats in a bar. The first one is accented.
pub(crate) const BEATS_PER_BAR: u64 = 4;

/// How loud the metronome is, 0 to 1, until the user moves the volume.
const STARTING_VOLUME: f32 = 0.7;

/// How loud an unaccented click is next to an accented one, which is at
/// the full level of the sound.
const PLAIN_CLICK_GAIN: f32 = 0.6;

/// The loudest sample of an accented click, leaving room under full scale
/// for the tracks it is mixed with.
const CLICK_PEAK: f32 = 0.8;

/// The tempo `bpm` kept within the range the metronome supports and
/// rounded to a tenth.
pub(crate) fn clamp_bpm(bpm: f32) -> f32 {
    if !bpm.is_finite() {
        return STARTING_BPM;
    }
    ((bpm.clamp(MINIMUM_BPM, MAXIMUM_BPM) * 10.0).round() / 10.0).clamp(MINIMUM_BPM, MAXIMUM_BPM)
}

/// The tempo typed in the field, or `None` when it is not a number from
/// [`MINIMUM_BPM`] to [`MAXIMUM_BPM`]. A comma or a point both start the
/// decimals, so "127,5" and "127.5" read alike. The tempo is rounded to a
/// tenth.
pub(crate) fn parse_bpm(text: &str) -> Option<f32> {
    let typed = text.trim().replace(',', ".");
    let bpm: f32 = typed.parse().ok()?;
    if !bpm.is_finite() || !(MINIMUM_BPM..=MAXIMUM_BPM).contains(&bpm) {
        return None;
    }
    Some(clamp_bpm(bpm))
}

/// The sounds the metronome can click with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ClickSound {
    /// A short, bright tick.
    #[default]
    Classic,
    /// A hollow knock, like a woodblock.
    Woodblock,
    /// A burst of noise, like a closed hi-hat.
    HiHat,
    /// A plain electronic beep.
    Beep,
}

impl ClickSound {
    /// Every sound, in the order the settings list them.
    pub(crate) const ALL: [ClickSound; 4] = [
        ClickSound::Classic,
        ClickSound::Woodblock,
        ClickSound::HiHat,
        ClickSound::Beep,
    ];

    /// The name the settings show.
    pub(crate) fn label(self) -> &'static str {
        match self {
            ClickSound::Classic => "Classic click",
            ClickSound::Woodblock => "Woodblock",
            ClickSound::HiHat => "Hi-hat",
            ClickSound::Beep => "Beep",
        }
    }

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|sound| *sound == self)
            .unwrap_or(0)
    }

    fn from_index(index: u8) -> Self {
        Self::ALL
            .get(usize::from(index))
            .copied()
            .unwrap_or_default()
    }
}

/// What the app remembers of the metronome.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct MetronomeSettings {
    /// The sound of the clicks.
    pub(crate) sound: ClickSound,
    /// How loud the clicks are, 0 to 1.
    pub(crate) volume: f32,
    /// Whether the first beat of every bar is accented.
    pub(crate) accent: bool,
}

impl Default for MetronomeSettings {
    fn default() -> Self {
        Self {
            sound: ClickSound::default(),
            volume: STARTING_VOLUME,
            accent: true,
        }
    }
}

struct ControlValues {
    bpm: AtomicU32,
    volume: AtomicU32,
    sound: AtomicU8,
    accent: AtomicBool,
}

/// The tempo, sound, volume and accent of a metronome, shared between the
/// window and the audio thread. Changes are heard from the next sample for
/// the volume and from the next beat for the rest.
#[derive(Clone)]
pub(crate) struct MetronomeControl(Arc<ControlValues>);

impl MetronomeControl {
    /// A control holding `settings` and the starting tempo.
    pub(crate) fn new(settings: MetronomeSettings) -> Self {
        let control = Self(Arc::new(ControlValues {
            bpm: AtomicU32::new(STARTING_BPM.to_bits()),
            volume: AtomicU32::new(0),
            sound: AtomicU8::new(0),
            accent: AtomicBool::new(true),
        }));
        control.apply(settings);
        control
    }

    /// Uses the sound, volume and accent of `settings`.
    pub(crate) fn apply(&self, settings: MetronomeSettings) {
        self.0.sound.store(
            u8::try_from(settings.sound.index()).unwrap_or(0),
            Ordering::Relaxed,
        );
        self.0
            .volume
            .store(settings.volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        self.0.accent.store(settings.accent, Ordering::Relaxed);
    }

    /// Clicks at `bpm` from the next beat on.
    pub(crate) fn set_bpm(&self, bpm: f32) {
        self.0
            .bpm
            .store(clamp_bpm(bpm).to_bits(), Ordering::Relaxed);
    }

    /// The tempo clicked at now.
    pub(crate) fn bpm(&self) -> f32 {
        f32::from_bits(self.0.bpm.load(Ordering::Relaxed))
    }

    fn volume(&self) -> f32 {
        f32::from_bits(self.0.volume.load(Ordering::Relaxed))
    }

    fn sound(&self) -> ClickSound {
        ClickSound::from_index(self.0.sound.load(Ordering::Relaxed))
    }

    fn accent(&self) -> bool {
        self.0.accent.load(Ordering::Relaxed)
    }
}

/// A beat the scheduler places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Beat {
    /// Beats since the metronome started, from zero.
    pub(crate) number: u64,
}

impl Beat {
    /// Whether this is the first beat of a bar.
    pub(crate) fn is_downbeat(self) -> bool {
        self.number.is_multiple_of(BEATS_PER_BAR)
    }
}

/// Places beats by counting frames, one call per frame.
#[derive(Debug, Clone)]
pub(crate) struct ClickScheduler {
    sample_rate: f64,
    frame: u64,
    last_beat: f64,
    beats: u64,
    cached_bpm: f32,
    beat_frames: f64,
}

impl ClickScheduler {
    /// A scheduler for frames at `sample_rate`. Its first beat is at frame
    /// zero.
    pub(crate) fn new(sample_rate: u32) -> Self {
        Self {
            sample_rate: f64::from(sample_rate),
            frame: 0,
            last_beat: 0.0,
            beats: 0,
            cached_bpm: 0.0,
            beat_frames: 0.0,
        }
    }

    /// Moves on one frame at tempo `bpm`, returning the beat that starts on
    /// it, if any. The beat after the one just placed is `bpm` beats per
    /// minute later than it, so a change of tempo is used from the next
    /// beat and the count within the bar carries on.
    pub(crate) fn next_frame(&mut self, bpm: f32) -> Option<Beat> {
        if bpm != self.cached_bpm {
            self.cached_bpm = bpm;
            self.beat_frames = 60.0 * self.sample_rate / f64::from(bpm.max(1.0));
        }
        let frame = self.frame;
        self.frame += 1;
        let due = if self.beats == 0 {
            0.0
        } else {
            self.last_beat + self.beat_frames
        };
        if (frame as f64) < due - 0.5 {
            return None;
        }
        // A tempo raised a lot can make the next beat due before this
        // frame; the beat then starts here rather than in the past.
        self.last_beat = if due < frame as f64 - 0.5 {
            frame as f64
        } else {
            due
        };
        let beat = Beat { number: self.beats };
        self.beats += 1;
        Some(beat)
    }
}

/// Every click of every sound, made once.
struct ClickBank {
    clicks: Vec<[Vec<f32>; 2]>,
}

impl ClickBank {
    fn shared() -> &'static ClickBank {
        static BANK: OnceLock<ClickBank> = OnceLock::new();
        BANK.get_or_init(|| ClickBank {
            clicks: ClickSound::ALL
                .map(|sound| [render_click(sound, false), render_click(sound, true)])
                .to_vec(),
        })
    }

    fn click(&self, sound: ClickSound, accented: bool) -> &[f32] {
        &self.clicks[sound.index()][usize::from(accented)]
    }
}

/// One click of `sound`: a burst that starts at once and dies away. An
/// accented click is higher and louder.
pub(crate) fn render_click(sound: ClickSound, accented: bool) -> Vec<f32> {
    let pitch_up = if accented { 1.0 } else { 0.0 };
    let (seconds, mut synthesize): (f64, Box<dyn FnMut(f64) -> f64>) = match sound {
        ClickSound::Classic => {
            let frequency = 1_500.0 + 700.0 * pitch_up;
            (
                0.03,
                Box::new(move |time| {
                    let attack = (time / 0.0005).min(1.0);
                    attack * (-time / 0.006).exp() * (TAU * frequency * time).sin()
                }),
            )
        }
        ClickSound::Woodblock => {
            let base = 900.0 + 300.0 * pitch_up;
            (
                0.07,
                Box::new(move |time| {
                    let attack = (time / 0.0004).min(1.0);
                    let body = (TAU * base * time).sin() + 0.5 * (TAU * base * 1.61 * time).sin();
                    attack * (-time / 0.012).exp() * body
                }),
            )
        }
        ClickSound::HiHat => {
            let decay = 0.010 + 0.012 * pitch_up;
            let mut noise_state: u32 = 0x1234_5678;
            let mut previous = 0.0;
            (
                0.05 + 0.04 * pitch_up,
                Box::new(move |time| {
                    noise_state ^= noise_state << 13;
                    noise_state ^= noise_state >> 17;
                    noise_state ^= noise_state << 5;
                    let noise = f64::from(noise_state) / f64::from(u32::MAX) * 2.0 - 1.0;
                    let bright = noise - previous;
                    previous = noise;
                    (time / 0.0003).min(1.0) * (-time / decay).exp() * bright
                }),
            )
        }
        ClickSound::Beep => {
            let frequency = 880.0 + 440.0 * pitch_up;
            let length = 0.08;
            (
                length,
                Box::new(move |time| {
                    let attack = (time / 0.003).min(1.0);
                    let release = ((length - time) / 0.012).clamp(0.0, 1.0);
                    attack * release * (TAU * frequency * time).sin()
                }),
            )
        }
    };
    let frames = (seconds * f64::from(SAMPLE_RATE)) as usize;
    let mut samples: Vec<f32> = (0..frames)
        .map(|frame| synthesize(frame as f64 / f64::from(SAMPLE_RATE)) as f32)
        .collect();
    let loudest = samples
        .iter()
        .fold(0.0_f32, |loudest, sample| loudest.max(sample.abs()));
    let gain = if accented { 1.0 } else { PLAIN_CLICK_GAIN };
    if loudest > 0.0 {
        for sample in &mut samples {
            *sample *= CLICK_PEAK * gain / loudest;
        }
    }
    samples
}

/// The metronome as a sound for the output to play: mono, ending only when
/// dropped, or after one click for a preview.
pub(crate) struct Metronome {
    control: MetronomeControl,
    bank: &'static ClickBank,
    scheduler: ClickScheduler,
    playing: &'static [f32],
    position: usize,
    is_preview: bool,
}

impl Metronome {
    /// A metronome that clicks as `control` says until it is dropped.
    pub(crate) fn running(control: MetronomeControl) -> Self {
        Self::new(control, false)
    }

    /// One accented click of the sound `control` holds, then silence, to
    /// hear a sound when it is chosen.
    pub(crate) fn preview(control: MetronomeControl) -> Self {
        Self::new(control, true)
    }

    fn new(control: MetronomeControl, is_preview: bool) -> Self {
        Self {
            control,
            bank: ClickBank::shared(),
            scheduler: ClickScheduler::new(SAMPLE_RATE),
            playing: &[],
            position: 0,
            is_preview,
        }
    }
}

impl Iterator for Metronome {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.is_preview {
            if self.scheduler.beats == 0 {
                self.scheduler.next_frame(self.control.bpm());
                self.playing = self.bank.click(self.control.sound(), true);
            } else if self.position >= self.playing.len() {
                return None;
            }
        } else if let Some(beat) = self.scheduler.next_frame(self.control.bpm()) {
            let accented = self.control.accent() && beat.is_downbeat();
            self.playing = self.bank.click(self.control.sound(), accented);
            self.position = 0;
        }
        let sample = self.playing.get(self.position).copied().unwrap_or(0.0);
        self.position += 1;
        Some(sample * self.control.volume())
    }
}

impl rodio::Source for Metronome {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> NonZeroU16 {
        NonZeroU16::MIN
    }

    fn sample_rate(&self) -> NonZeroU32 {
        NonZeroU32::new(SAMPLE_RATE).unwrap_or(NonZeroU32::MIN)
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn beat_frames(sample_rate: u32, frames: u64, bpm_at: impl Fn(u64) -> f32) -> Vec<(u64, Beat)> {
        let mut scheduler = ClickScheduler::new(sample_rate);
        (0..frames)
            .filter_map(|frame| {
                scheduler
                    .next_frame(bpm_at(frame))
                    .map(|beat| (frame, beat))
            })
            .collect()
    }

    #[test]
    fn beats_land_on_the_frames_of_the_tempo() {
        for (sample_rate, bpm, expected_gap) in [
            (48_000, 120.0, 24_000),
            (44_100, 120.0, 22_050),
            (48_000, 60.0, 48_000),
            (96_000, 150.0, 38_400),
            (44_100, 90.0, 29_400),
        ] {
            let beats = beat_frames(sample_rate, u64::from(sample_rate) * 4, |_| bpm);
            let frames: Vec<u64> = beats.iter().map(|(frame, _)| *frame).collect();
            assert_eq!(frames[0], 0);
            assert_eq!(frames[1], expected_gap, "{bpm} bpm at {sample_rate}");
            assert_eq!(frames[2], expected_gap * 2);
        }
    }

    #[test]
    fn beats_that_fall_between_frames_go_to_the_nearest_frame_without_drift() {
        let sample_rate = 44_100;
        let bpm = 127.5_f32;
        let ten_minutes = u64::from(sample_rate) * 600;
        let beats = beat_frames(sample_rate, ten_minutes, |_| bpm);
        let exact_gap = 60.0 * f64::from(sample_rate) / f64::from(bpm);
        assert_eq!(
            beats.len(),
            (600.0 * f64::from(bpm) / 60.0).round() as usize
        );
        for (frame, beat) in &beats {
            let exact = beat.number as f64 * exact_gap;
            assert!(
                (*frame as f64 - exact).abs() <= 0.5 + 1e-6,
                "beat {} at {frame}, exact {exact}",
                beat.number
            );
        }
    }

    #[test]
    fn ten_minutes_at_120_bpm_end_exactly_on_the_grid() {
        let sample_rate = 48_000;
        let beats = beat_frames(sample_rate, u64::from(sample_rate) * 600, |_| 120.0);
        assert_eq!(beats.len(), 1200);
        let (last_frame, last_beat) = beats[1199];
        assert_eq!(last_beat.number, 1199);
        assert_eq!(last_frame, 1199 * 24_000);
    }

    #[test]
    fn the_first_beat_of_every_fourth_is_the_accented_one() {
        let beats = beat_frames(44_100, 44_100 * 10, |_| 120.0);
        let downbeats: Vec<u64> = beats
            .iter()
            .filter(|(_, beat)| beat.is_downbeat())
            .map(|(_, beat)| beat.number)
            .collect();
        assert_eq!(downbeats, [0, 4, 8, 12, 16]);
    }

    #[test]
    fn a_new_tempo_starts_at_the_next_beat_and_keeps_the_bar() {
        let sample_rate = 48_000;
        let change_at = 30_000;
        let beats = beat_frames(sample_rate, 48_000 * 5, |frame| {
            if frame < change_at { 120.0 } else { 60.0 }
        });
        let frames: Vec<u64> = beats.iter().map(|(frame, _)| *frame).collect();
        assert_eq!(frames[..2], [0, 24_000]);
        assert_eq!(frames[2], 24_000 + 48_000, "one slow beat after the last");
        assert_eq!(frames[3], 24_000 + 2 * 48_000);
        let numbers: Vec<u64> = beats.iter().map(|(_, beat)| beat.number).collect();
        assert_eq!(numbers[..4], [0, 1, 2, 3], "the count carries on");
    }

    #[test]
    fn a_faster_tempo_never_places_a_beat_in_the_past() {
        let beats = beat_frames(48_000, 48_000 * 3, |frame| {
            if frame < 40_000 { 30.0 } else { 300.0 }
        });
        let frames: Vec<u64> = beats.iter().map(|(frame, _)| *frame).collect();
        assert_eq!(frames[..2], [0, 40_000]);
        assert_eq!(frames[2] - frames[1], 9_600);
    }

    #[test]
    fn tempos_are_read_with_a_comma_or_a_point() {
        assert_eq!(parse_bpm("127,5"), Some(127.5));
        assert_eq!(parse_bpm("127.5"), Some(127.5));
        assert_eq!(parse_bpm("  98 "), Some(98.0));
        assert_eq!(parse_bpm("127,54"), Some(127.5));
        assert_eq!(parse_bpm("30"), Some(30.0));
        assert_eq!(parse_bpm("300"), Some(300.0));
    }

    #[test]
    fn tempos_out_of_range_or_not_numbers_are_refused() {
        for text in ["", "abc", "29.9", "300,1", "-120", "NaN", "inf", "1,2,3"] {
            assert_eq!(parse_bpm(text), None, "{text:?}");
        }
    }

    #[test]
    fn a_tempo_is_kept_in_range_and_to_a_tenth() {
        assert_eq!(clamp_bpm(12.0), 30.0);
        assert_eq!(clamp_bpm(999.0), 300.0);
        assert_eq!(clamp_bpm(127.46), 127.5);
        assert_eq!(clamp_bpm(f32::NAN), STARTING_BPM);
    }

    #[test]
    fn every_sound_is_a_short_enveloped_burst_with_a_louder_accent() {
        let peak = |samples: &[f32]| samples.iter().fold(0.0_f32, |top, s| top.max(s.abs()));
        for sound in ClickSound::ALL {
            let plain = render_click(sound, false);
            let accented = render_click(sound, true);
            for click in [&plain, &accented] {
                assert!(click.iter().all(|sample| sample.is_finite()));
                assert!(click.len() < usize::try_from(SAMPLE_RATE / 5).unwrap());
                assert!(peak(click) <= 1.0);
                assert!(peak(click) > 0.1, "{sound:?} is silent");
                assert!(
                    click.last().unwrap().abs() < 0.02,
                    "{sound:?} ends abruptly"
                );
            }
            assert!(peak(&accented) > peak(&plain) * 1.3, "{sound:?}");
        }
    }

    fn zero_crossings(samples: &[f32]) -> usize {
        samples
            .windows(2)
            .filter(|pair| pair[0] * pair[1] < 0.0)
            .count()
    }

    #[test]
    fn the_accent_of_a_pitched_sound_is_higher() {
        for sound in [ClickSound::Classic, ClickSound::Woodblock, ClickSound::Beep] {
            let plain = render_click(sound, false);
            let accented = render_click(sound, true);
            let rate = |samples: &[f32]| zero_crossings(samples) as f64 / samples.len() as f64;
            assert!(rate(&accented) > rate(&plain), "{sound:?}");
        }
    }

    #[test]
    fn the_source_clicks_on_the_beat_and_is_quiet_between() {
        let control = MetronomeControl::new(MetronomeSettings {
            volume: 1.0,
            ..MetronomeSettings::default()
        });
        control.set_bpm(120.0);
        let mut metronome = Metronome::running(control);
        let gap = 22_050;
        let samples: Vec<f32> = metronome.by_ref().take(gap * 5).collect();
        let loudest = |from: usize, to: usize| {
            samples[from..to]
                .iter()
                .fold(0.0_f32, |top, sample| top.max(sample.abs()))
        };
        let click_length = render_click(ClickSound::Classic, true).len();
        assert!(loudest(0, 100) > 0.0);
        assert!(loudest(gap, gap + 100) > 0.0);
        assert_eq!(loudest(click_length + 10, gap), 0.0);
        assert!(loudest(0, click_length) > loudest(gap, gap + click_length) * 1.3);
        assert!(loudest(4 * gap, 4 * gap + click_length) > loudest(gap, gap + click_length) * 1.3);
    }

    #[test]
    fn without_the_accent_every_beat_sounds_the_same() {
        let control = MetronomeControl::new(MetronomeSettings {
            accent: false,
            volume: 1.0,
            ..MetronomeSettings::default()
        });
        control.set_bpm(120.0);
        let samples: Vec<f32> = Metronome::running(control).take(22_050 * 2).collect();
        let first = samples[..200]
            .iter()
            .fold(0.0_f32, |top, s| top.max(s.abs()));
        let second = samples[22_050..22_250]
            .iter()
            .fold(0.0_f32, |top, s| top.max(s.abs()));
        assert!((first - second).abs() < 1e-6);
    }

    #[test]
    fn the_volume_scales_the_clicks_and_zero_silences_them() {
        let render = |volume: f32| -> f32 {
            let control = MetronomeControl::new(MetronomeSettings {
                volume,
                ..MetronomeSettings::default()
            });
            Metronome::running(control)
                .take(2_000)
                .fold(0.0_f32, |top, s| top.max(s.abs()))
        };
        assert_eq!(render(0.0), 0.0);
        assert!((render(0.5) * 2.0 - render(1.0)).abs() < 1e-5);
    }

    #[test]
    fn a_preview_is_one_click_that_ends() {
        let control = MetronomeControl::new(MetronomeSettings::default());
        let samples: Vec<f32> = Metronome::preview(control).collect();
        assert_eq!(samples.len(), render_click(ClickSound::Classic, true).len());
    }

    #[test]
    fn settings_round_trip_through_json() {
        let settings = MetronomeSettings {
            sound: ClickSound::HiHat,
            volume: 0.35,
            accent: false,
        };
        let text = serde_json::to_string(&settings).unwrap();
        assert!(text.contains("\"hi-hat\""), "{text}");
        assert_eq!(
            serde_json::from_str::<MetronomeSettings>(&text).unwrap(),
            settings
        );
    }

    #[test]
    fn missing_fields_take_their_defaults() {
        let partial: MetronomeSettings = serde_json::from_str(r#"{"volume": 0.2}"#).unwrap();
        assert_eq!(partial.sound, ClickSound::Classic);
        assert!(partial.accent);
        assert_eq!(partial.volume, 0.2);
        let empty: MetronomeSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, MetronomeSettings::default());
    }
}
