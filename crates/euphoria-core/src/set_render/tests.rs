// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::path::{Path, PathBuf};

use super::render::limit_for_test;
use super::*;
use crate::TrackAnalysis;
use crate::audio_stream::AudioStream;
use crate::detect::BeatGrid;
use crate::harmony::transition::PlanTrack;
use crate::shift::ExistingOutput;

fn grid(bpm: f64, first_downbeat_seconds: f64) -> BeatGrid {
    BeatGrid {
        first_downbeat_seconds,
        bpm,
        salience: 3.0,
        drift_seconds: None,
    }
}

fn write_mono(path: &Path, sample_rate: u32, samples: &[f32]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for sample in samples {
        writer
            .write_sample((sample.clamp(-1.0, 1.0) * 32_767.0) as i16)
            .unwrap();
    }
    writer.finalize().unwrap();
}

/// Short high clicks at `bpm` from `first_beat` seconds, for `seconds`.
fn clicks(sample_rate: u32, bpm: f64, first_beat: f64, seconds: f64) -> Vec<f32> {
    let rate = f64::from(sample_rate);
    let mut samples = vec![0.0; (seconds * rate) as usize];
    let mut beat = first_beat;
    while beat < seconds {
        let start = (beat * rate) as usize;
        for offset in 0..(0.003 * rate) as usize {
            if let Some(sample) = samples.get_mut(start + offset) {
                let time = offset as f64 / rate;
                *sample =
                    (0.8 * (-time / 0.001).exp() * (std::f64::consts::TAU * 2_000.0 * time).sin())
                        as f32;
            }
        }
        beat += 60.0 / bpm;
    }
    samples
}

fn ramp(sample_rate: u32, seconds: f64, level: f32) -> Vec<f32> {
    let count = (seconds * f64::from(sample_rate)) as usize;
    (0..count)
        .map(|index| level * (index % 100) as f32 / 100.0)
        .collect()
}

fn decode(path: &Path) -> (Vec<f32>, u32) {
    let mut stream = AudioStream::open(path).unwrap();
    assert_eq!(stream.channel_count().get(), 2);
    let rate = stream.sample_rate().get();
    let mut all = Vec::new();
    let mut chunk = Vec::new();
    while stream.read_chunk(&mut chunk).unwrap() {
        all.extend_from_slice(&chunk);
    }
    (all, rate)
}

/// Seconds at which the left channel rises above a threshold after a
/// quiet stretch.
fn onsets(mix: &[f32], sample_rate: u32) -> Vec<f64> {
    let mut found = Vec::new();
    let mut quiet = usize::MAX;
    for (frame, pair) in mix.as_chunks::<2>().0.iter().enumerate() {
        if pair[0].abs() > 0.1 {
            if quiet > 2_000 {
                found.push(frame as f64 / f64::from(sample_rate));
            }
            quiet = 0;
        } else {
            quiet = quiet.saturating_add(1);
        }
    }
    found
}

fn track(path: &Path, seconds: f64, grid: Option<BeatGrid>) -> RenderTrack {
    RenderTrack {
        path: path.to_path_buf(),
        artist: None,
        title: None,
        duration_seconds: seconds,
        grid,
    }
}

fn blend(a_start_bar: usize, b_start_bar: usize, bars: usize) -> MixTransition {
    MixTransition {
        mix: MixMove::Blend(Blend {
            a_start_bar,
            b_start_bar,
            bars,
            bass_swap_bar: None,
        }),
        reason: "test".to_string(),
    }
}

fn wav_options() -> RenderOptions {
    RenderOptions {
        format: MixFormat::Wav,
        existing: ExistingOutput::Replace,
    }
}

#[test]
fn the_incoming_track_lands_on_the_beats_of_the_outgoing_one_then_returns_to_its_tempo() {
    let folder = tempfile::tempdir().unwrap();
    let outgoing = folder.path().join("outgoing.wav");
    let incoming = folder.path().join("incoming.wav");
    write_mono(&outgoing, 44_100, &vec![0.0; 24 * 44_100]);
    write_mono(&incoming, 48_000, &clicks(48_000, 125.0, 0.3, 50.0));
    let plan = MixPlan {
        tracks: vec![
            track(&outgoing, 24.0, Some(grid(120.0, 0.5))),
            track(&incoming, 50.0, Some(grid(125.0, 0.3))),
        ],
        transitions: vec![blend(6, 1, 4)],
    };
    let output = folder.path().join("mix.wav");
    let rendered = render_mix(&plan, &output, &wav_options(), &mut NoProgress).unwrap();
    assert_eq!(rendered.sample_rate, 44_100);
    let placement = &rendered.timeline.placements[1];
    assert!((placement.start_seconds - 12.5).abs() < 1e-9);
    assert!((placement.entry_rate - 120.0 / 125.0).abs() < 1e-12);

    let (mix, rate) = decode(&output);
    assert_eq!(mix.len() as u64, rendered.frames * 2);
    let heard = onsets(&mix, rate);
    let in_blend: Vec<f64> = heard
        .iter()
        .copied()
        .filter(|seconds| (13.0..20.4).contains(seconds))
        .collect();
    assert_eq!(in_blend.len(), 15, "{heard:?}");
    for (beat, seconds) in in_blend.iter().enumerate() {
        let expected = 13.0 + beat as f64 * 0.5;
        assert!(
            (seconds - expected).abs() < 0.004,
            "beat {beat}: {seconds}, expected {expected}"
        );
    }
    let late: Vec<f64> = heard
        .iter()
        .copied()
        .filter(|seconds| *seconds > 40.0)
        .collect();
    let gaps: Vec<f64> = late.windows(2).map(|pair| pair[1] - pair[0]).collect();
    assert!(!gaps.is_empty());
    for gap in gaps {
        assert!((gap - 0.48).abs() < 0.004, "{gap}");
    }
}

#[test]
fn tracks_without_a_grid_play_one_after_the_other_untouched() {
    let folder = tempfile::tempdir().unwrap();
    let first = folder.path().join("first.wav");
    let second = folder.path().join("second.wav");
    let first_audio = ramp(44_100, 1.0, 0.5);
    let second_audio = ramp(44_100, 0.5, -0.4);
    write_mono(&first, 44_100, &first_audio);
    write_mono(&second, 44_100, &second_audio);
    let plan = MixPlan {
        tracks: vec![track(&first, 1.0, None), track(&second, 0.5, None)],
        transitions: vec![MixTransition {
            mix: MixMove::Sequence,
            reason: "test".to_string(),
        }],
    };
    let output = folder.path().join("mix.wav");
    let rendered = render_mix(&plan, &output, &wav_options(), &mut NoProgress).unwrap();
    assert_eq!(rendered.frames, 44_100 + 22_050);
    let (mix, _) = decode(&output);
    let expected: Vec<f32> = first_audio.iter().chain(&second_audio).copied().collect();
    assert_eq!(mix.len(), expected.len() * 2);
    for (index, (frame, wanted)) in mix.as_chunks::<2>().0.iter().zip(&expected).enumerate() {
        let wanted = (wanted * 32_767.0).trunc() / 32_767.0;
        assert!((frame[0] - wanted).abs() < 1e-4, "frame {index}");
        assert_eq!(frame[0], frame[1], "frame {index}");
    }
}

#[test]
fn flac_holds_the_same_mix_as_wav() {
    let folder = tempfile::tempdir().unwrap();
    let source = folder.path().join("source.wav");
    write_mono(&source, 44_100, &ramp(44_100, 0.7, 0.3));
    let plan = MixPlan {
        tracks: vec![track(&source, 0.7, None)],
        transitions: Vec::new(),
    };
    let wav = folder.path().join("mix.wav");
    let flac = folder.path().join("mix.flac");
    render_mix(&plan, &wav, &wav_options(), &mut NoProgress).unwrap();
    let options = RenderOptions {
        format: MixFormat::Flac,
        existing: ExistingOutput::Replace,
    };
    render_mix(&plan, &flac, &options, &mut NoProgress).unwrap();
    assert_eq!(decode(&wav), decode(&flac));
}

struct CancelAfter {
    blocks: usize,
}

impl RenderProgress for CancelAfter {
    fn rendered(&mut self, _done: f64, _total: f64) {
        self.blocks = self.blocks.saturating_sub(1);
    }

    fn is_cancelled(&self) -> bool {
        self.blocks == 0
    }
}

#[test]
fn a_cancelled_render_leaves_no_file_behind() {
    let folder = tempfile::tempdir().unwrap();
    let source = folder.path().join("source.wav");
    write_mono(&source, 44_100, &ramp(44_100, 2.0, 0.3));
    let plan = MixPlan {
        tracks: vec![track(&source, 2.0, None)],
        transitions: Vec::new(),
    };
    let output = folder.path().join("mix.wav");
    let result = render_mix(
        &plan,
        &output,
        &wav_options(),
        &mut CancelAfter { blocks: 3 },
    );
    assert!(matches!(result, Err(crate::EuphoriaError::MixCancelled)));
    let left: Vec<PathBuf> = std::fs::read_dir(folder.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(left, vec![source]);
}

#[test]
fn a_track_of_the_set_is_never_the_output() {
    let folder = tempfile::tempdir().unwrap();
    let source = folder.path().join("source.wav");
    write_mono(&source, 44_100, &ramp(44_100, 0.2, 0.3));
    let before = std::fs::read(&source).unwrap();
    let plan = MixPlan {
        tracks: vec![track(&source, 0.2, None)],
        transitions: Vec::new(),
    };
    assert!(render_mix(&plan, &source, &wav_options(), &mut NoProgress).is_err());
    assert_eq!(std::fs::read(&source).unwrap(), before);
}

#[test]
fn the_limiter_keeps_peaks_under_the_ceiling_and_leaves_quiet_sound_alone() {
    let quiet: Vec<f32> = (0..2_000)
        .map(|index| (index as f32 * 0.01).sin() * 0.5)
        .collect();
    assert_eq!(limit_for_test(&quiet, 44_100), quiet);

    let mut loud = quiet.clone();
    for sample in &mut loud[1_000..1_100] {
        *sample *= 3.0;
    }
    let limited = limit_for_test(&loud, 44_100);
    assert_eq!(limited.len(), loud.len());
    assert!(limited.iter().all(|sample| sample.abs() <= MIX_CEILING));
}

#[test]
fn the_timeline_glides_back_to_the_own_tempo_and_hands_over_the_bass() {
    let plan = MixPlan {
        tracks: vec![
            track(Path::new("/a.wav"), 120.0, Some(grid(120.0, 0.0))),
            track(Path::new("/b.wav"), 120.0, Some(grid(126.0, 0.0))),
        ],
        transitions: vec![blend(32, 0, 16)],
    };
    let timeline = mix_timeline(&plan).unwrap();
    let [outgoing, incoming] = timeline.placements.as_slice() else {
        panic!("two placements");
    };
    assert!((incoming.start_seconds - 64.0).abs() < 1e-9);
    assert!((outgoing.end_seconds - 96.0).abs() < 1e-9);
    let rate = 120.0 / 126.0;
    assert!((incoming.rate_at(1.0) - rate).abs() < 1e-12);
    assert!((incoming.rate_at(32.0 + TEMPO_RAMP_SECONDS / 2.0) - (rate + 1.0) / 2.0).abs() < 1e-9);
    assert!((incoming.rate_at(32.0 + TEMPO_RAMP_SECONDS + 1.0) - 1.0).abs() < 1e-12);
    let blend_source = 16.0 * 240.0 / 126.0;
    assert!((incoming.source_offset_at(32.0) - blend_source).abs() < 1e-9);

    let (outgoing_volume, outgoing_bass) = outgoing.gains_at(65.0);
    let (incoming_volume, incoming_bass) = incoming.gains_at(65.0);
    assert_eq!((outgoing_volume, outgoing_bass), (1.0, 1.0));
    assert!(incoming_volume > 0.0 && incoming_volume < 1.0);
    assert_eq!(incoming_bass, 0.0);
    let swapped = 64.0 + 8.0 * 2.0 + 1.0;
    assert_eq!(outgoing.gains_at(swapped).1, 0.0);
    assert_eq!(incoming.gains_at(swapped).1, 1.0);
    assert_eq!(incoming.gains_at(81.0).0, 1.0);
    assert!(outgoing.gains_at(95.99).0 < 0.01);
}

#[test]
fn a_blend_out_before_the_blend_in_ends_is_refused() {
    let plan = MixPlan {
        tracks: vec![
            track(Path::new("/a.wav"), 120.0, Some(grid(120.0, 0.0))),
            track(Path::new("/b.wav"), 120.0, Some(grid(120.0, 0.0))),
            track(Path::new("/c.wav"), 120.0, Some(grid(120.0, 0.0))),
        ],
        transitions: vec![blend(32, 0, 16), blend(8, 0, 16)],
    };
    let error = mix_timeline(&plan).unwrap_err();
    assert!(error.contains("before its blend in ends"), "{error}");
}

fn analysis(bpm: f64, bars: usize, has_grid: bool) -> TrackAnalysis {
    let beat_grid = has_grid.then(|| grid(bpm, 0.0));
    TrackAnalysis {
        duration_seconds: (bars as f64 * 240.0 / bpm) as f32,
        key: None,
        bpm: Some(bpm as f32),
        energy: Some(6),
        tuning_cents: None,
        key_timeline: Vec::new(),
        quality: None,
        key_runner_up: None,
        key_score_margin: None,
        key_source: None,
        loudness_range_lu: None,
        tempo_estimate: Some(bpm as f32),
        tempo_score_margin: None,
        beat_grid,
        cues: Vec::new(),
        pitch_class_profile: None,
    }
}

#[test]
fn planning_keeps_each_blend_out_after_the_blend_in_and_says_why() {
    let analyses = [
        analysis(124.0, 96, true),
        analysis(125.0, 96, true),
        analysis(126.0, 96, false),
    ];
    let inputs: Vec<MixInput<'_>> = analyses
        .iter()
        .enumerate()
        .map(|(index, analysis)| MixInput {
            track: track(
                Path::new(&format!("/{index}.wav")),
                f64::from(analysis.duration_seconds),
                analysis.beat_grid,
            ),
            plan: PlanTrack {
                analysis,
                sections: &[],
                energy: &[],
                stems: None,
            },
            locked_exit: None,
        })
        .collect();
    let plan = plan_mix(&inputs);
    assert_eq!(plan.tracks.len(), 3);
    let MixMove::Blend(first) = plan.transitions[0].mix else {
        panic!("{:?}", plan.transitions[0]);
    };
    assert!(plan.transitions[0].reason.contains(" bars: "));
    assert_eq!(plan.transitions[1].mix, MixMove::Sequence);
    assert_eq!(
        plan.transitions[1].reason,
        "Played one after the other: the incoming track has no beat grid"
    );
    assert!(first.a_start_bar.is_multiple_of(8));
    mix_timeline(&plan).unwrap();
}

#[test]
fn the_sheets_list_each_track_where_it_starts() {
    let mut first = track(Path::new("/music/one.wav"), 120.0, Some(grid(120.0, 0.0)));
    first.artist = Some("Someone".to_string());
    first.title = Some("Say \"hi\"".to_string());
    let second = track(Path::new("/music/two.wav"), 4_000.0, Some(grid(120.0, 0.0)));
    let plan = MixPlan {
        tracks: vec![first, second],
        transitions: vec![blend(32, 0, 16)],
    };
    let timeline = mix_timeline(&plan).unwrap();
    let sheet = cue_sheet("My set", "mix.wav", MixFormat::Wav, &plan.tracks, &timeline);
    assert_eq!(
        sheet,
        "TITLE \"My set\"\nFILE \"mix.wav\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"Say 'hi'\"\n    \
         PERFORMER \"Someone\"\n    INDEX 01 00:00:00\n  TRACK 02 AUDIO\n    TITLE \"two\"\n    \
         INDEX 01 01:04:00\n"
    );
    assert_eq!(
        chapters(&plan.tracks, &timeline),
        "0:00 Someone - Say \"hi\"\n1:04 two\n"
    );
    let list = tracklist("My set", &plan.tracks, &timeline);
    assert!(list.starts_with("My set\n\n01. 0:00 Someone - Say \"hi\"\n02. 1:04 two\n"));
    assert!(list.ends_with("Length 1:07:44\n"), "{list}");
    assert_eq!(format_mix_time(3_661.9), "1:01:01");
}

#[test]
fn a_locked_blend_is_kept_and_the_blend_before_it_makes_room() {
    let analyses = [
        analysis(124.0, 96, true),
        analysis(124.0, 96, true),
        analysis(124.0, 96, true),
    ];
    let locked = Blend {
        a_start_bar: 40,
        b_start_bar: 8,
        bars: 16,
        bass_swap_bar: Some(5),
    };
    let inputs: Vec<MixInput<'_>> = analyses
        .iter()
        .enumerate()
        .map(|(index, analysis)| MixInput {
            track: track(
                Path::new(&format!("/{index}.wav")),
                f64::from(analysis.duration_seconds),
                analysis.beat_grid,
            ),
            plan: PlanTrack {
                analysis,
                sections: &[],
                energy: &[],
                stems: None,
            },
            locked_exit: (index == 1).then_some(locked),
        })
        .collect();
    let plan = plan_mix(&inputs);
    assert_eq!(plan.transitions[1].mix, MixMove::Blend(locked));
    assert!(
        plan.transitions[1]
            .reason
            .starts_with("Locked, placed by hand. "),
        "{}",
        plan.transitions[1].reason
    );
    let MixMove::Blend(before) = plan.transitions[0].mix else {
        panic!("{:?}", plan.transitions[0]);
    };
    assert!(before.b_start_bar + before.bars <= locked.a_start_bar);
    mix_timeline(&plan).unwrap();
}
