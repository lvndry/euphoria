// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};

use ebur128::{Channel, EbuR128, Mode};
use lofty::config::WriteOptions;
use lofty::file::TaggedFileExt;
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::probe::Probe;
use lofty::tag::{Accessor, Tag, TagExt, TagType};

use super::*;

const RATE: u32 = 48_000;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn write_wav(path: &Path, channels: u16, samples: &[f32]) {
    let spec = hound::WavSpec {
        channels,
        sample_rate: RATE,
        bits_per_sample: 24,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for sample in samples {
        writer
            .write_sample((sample.clamp(-1.0, 1.0) * 8_388_607.0).round() as i32)
            .unwrap();
    }
    writer.finalize().unwrap();
}

fn stereo(seconds: f32, mut value: impl FnMut(usize) -> f32) -> Vec<f32> {
    (0..(RATE as f32 * seconds) as usize)
        .flat_map(|index| {
            let sample = value(index);
            [sample, sample]
        })
        .collect()
}

fn sine(frequency: f32, amplitude: f32, seconds: f32) -> Vec<f32> {
    stereo(seconds, |index| {
        (TAU * frequency * index as f32 / RATE as f32).sin() * amplitude
    })
}

fn noise(amplitude: f32, seconds: f32) -> Vec<f32> {
    let mut state = 0x2545_f491_u32;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as f32 / u32::MAX as f32 * 2.0 - 1.0
    };
    (0..(RATE as f32 * seconds) as usize)
        .flat_map(|_| [next() * amplitude, next() * amplitude])
        .collect()
}

fn decibels(amplitude: f32) -> f32 {
    20.0 * amplitude.log10()
}

fn amplitude(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

/// Loudness in LUFS and true peak in dBTP, measured on the decoded file
/// with libebur128's 4x oversampling.
fn measure_file(path: &Path) -> (f32, f32) {
    let mut stream = AudioStream::open(path).unwrap();
    let channels = u32::from(stream.channel_count().get());
    let mut meter = EbuR128::new(
        channels,
        stream.sample_rate().get(),
        Mode::I | Mode::TRUE_PEAK,
    )
    .unwrap();
    if channels == 1 {
        meter.set_channel(0, Channel::DualMono).unwrap();
    }
    let mut chunk = Vec::new();
    while stream.read_chunk(&mut chunk).unwrap() {
        meter.add_frames_f32(&chunk).unwrap();
    }
    let peak = (0..channels)
        .map(|channel| meter.true_peak(channel).unwrap())
        .fold(0.0_f64, f64::max);
    (
        meter.loudness_global().unwrap() as f32,
        decibels(peak as f32),
    )
}

fn decode(path: &Path) -> (Vec<f32>, u32, u16) {
    let mut stream = AudioStream::open(path).unwrap();
    let mut all = Vec::new();
    let mut chunk = Vec::new();
    while stream.read_chunk(&mut chunk).unwrap() {
        all.extend_from_slice(&chunk);
    }
    (
        all,
        stream.sample_rate().get(),
        stream.channel_count().get(),
    )
}

struct Run {
    plan: TrackPlan,
    prepared: PreparedTrack,
}

fn run(
    folder: &Path,
    name: &str,
    samples: &[f32],
    settings: &PrepareSettings,
    tuning_cents: Option<f32>,
) -> Run {
    let input = folder.join(name);
    write_wav(&input, 2, samples);
    prepare_file(&input, settings, tuning_cents, LossyOutput::Flac)
}

fn prepare_file(
    input: &Path,
    settings: &PrepareSettings,
    tuning_cents: Option<f32>,
    lossy: LossyOutput,
) -> Run {
    let plan = plan_track(input, settings, tuning_cents, lossy).unwrap();
    let output = prepared_path(
        input,
        None,
        DEFAULT_SUFFIX,
        plan.output_format,
        &HashSet::new(),
    );
    let prepared = prepare_track(input, &plan, &output, settings, &mut |_| {}).unwrap();
    Run { plan, prepared }
}

#[test]
fn tones_and_noise_reach_the_target_within_half_a_loudness_unit() {
    let folder = tempfile::tempdir().unwrap();
    let signals = [
        ("quiet-tone", sine(997.0, amplitude(-34.0), 6.0)),
        ("bass-tone", sine(60.0, amplitude(-20.0), 6.0)),
        ("loud-tone", sine(1_500.0, amplitude(-3.0), 6.0)),
        ("noise", noise(amplitude(-22.0), 6.0)),
        ("loud-noise", noise(amplitude(-2.0), 6.0)),
    ];
    for target in [-14.0, -11.0, -9.0] {
        let settings = PrepareSettings {
            target_lufs: target,
            ..PrepareSettings::default()
        };
        for (name, samples) in &signals {
            let result = run(
                folder.path(),
                &format!("{name}-{target}.wav"),
                samples,
                &settings,
                None,
            );
            assert!(!result.plan.gain_capped, "{name} at {target}");
            let (lufs, true_peak) = measure_file(&result.prepared.output);
            assert!(
                (lufs - target).abs() < 0.5,
                "{name} at {target}: measured {lufs}"
            );
            assert!(
                true_peak <= settings.true_peak_ceiling_db,
                "{name}: {true_peak}"
            );
        }
    }
}

fn peaky_track() -> Vec<f32> {
    let mut samples = sine(220.0, amplitude(-13.0), 8.0);
    for burst in 0..16 {
        let start = (burst * RATE as usize / 2 + 10_000) * 2;
        for index in 0..8 {
            samples[start + index * 2] = amplitude(-5.5) * if index < 4 { 1.0 } else { -1.0 };
            samples[start + index * 2 + 1] = samples[start + index * 2];
        }
    }
    samples
}

#[test]
fn the_true_peak_stays_under_the_ceiling_with_limiting_inside_the_allowance() {
    let folder = tempfile::tempdir().unwrap();
    let settings = PrepareSettings::default();
    let result = run(folder.path(), "peaky.wav", &peaky_track(), &settings, None);
    assert!(!result.plan.gain_capped);
    assert!(result.plan.limiting_db > 0.5, "{}", result.plan.limiting_db);
    assert!(result.plan.limiting_db <= settings.maximum_limiting_db);
    let (lufs, true_peak) = measure_file(&result.prepared.output);
    assert!(true_peak <= settings.true_peak_ceiling_db, "{true_peak}");
    assert!(
        true_peak > settings.true_peak_ceiling_db - 0.6,
        "limiter overshoots downward: {true_peak}"
    );
    assert!((lufs - settings.target_lufs).abs() < 0.8, "{lufs}");
}

#[test]
fn an_inter_sample_peak_is_limited_too() {
    let folder = tempfile::tempdir().unwrap();
    let quarter = RATE as f32 / 4.0;
    let samples = stereo(6.0, |index| {
        (TAU * quarter * index as f32 / RATE as f32 + std::f32::consts::FRAC_PI_4).sin()
            * amplitude(-8.0)
    });
    let settings = PrepareSettings {
        target_lufs: -7.0,
        true_peak_ceiling_db: -2.0,
        ..PrepareSettings::default()
    };
    let result = run(folder.path(), "quarter.wav", &samples, &settings, None);
    let (_, true_peak) = measure_file(&result.prepared.output);
    assert!(true_peak <= -2.0, "{true_peak}");
}

#[test]
fn the_limiting_allowance_caps_the_gain_and_says_so() {
    let folder = tempfile::tempdir().unwrap();
    let mut samples = sine(300.0, amplitude(-32.0), 8.0);
    for index in 0..6 {
        samples[(40_000 + index) * 2] = amplitude(-0.9);
        samples[(40_000 + index) * 2 + 1] = amplitude(-0.9);
    }
    let settings = PrepareSettings::default();
    let result = run(folder.path(), "capped.wav", &samples, &settings, None);
    let plan = &result.plan;
    assert!(plan.gain_capped);
    assert!(plan.wanted_gain_db > plan.gain_db + 10.0);
    assert!((plan.limiting_db - settings.maximum_limiting_db).abs() < 0.01);
    assert!(plan.predicted_lufs.unwrap() < settings.target_lufs - 5.0);
    let (_, true_peak) = measure_file(&result.prepared.output);
    assert!(true_peak <= settings.true_peak_ceiling_db, "{true_peak}");
    assert!(result.prepared.limiter_reduction_db <= settings.maximum_limiting_db + 0.3);
}

fn frequency_of(samples: &[f32]) -> f32 {
    let mono: Vec<f32> = samples.iter().step_by(2).copied().collect();
    let middle = &mono[mono.len() / 4..mono.len() * 3 / 4];
    let mut crossings = Vec::new();
    for index in 1..middle.len() {
        if middle[index - 1] < 0.0 && middle[index] >= 0.0 {
            let fraction = -middle[index - 1] / (middle[index] - middle[index - 1]);
            crossings.push(index as f32 - 1.0 + fraction);
        }
    }
    let cycles = (crossings.len() - 1) as f32;
    cycles * RATE as f32 / (crossings.last().unwrap() - crossings[0])
}

fn cents(frequency: f32, reference: f32) -> f32 {
    1_200.0 * (frequency / reference).log2()
}

#[test]
fn tuning_correction_brings_445_hz_to_440_within_two_cents() {
    let folder = tempfile::tempdir().unwrap();
    let samples = sine(445.0, amplitude(-18.0), 6.0);
    let measured_cents = cents(445.0, 440.0);
    let settings = PrepareSettings {
        correct_tuning: true,
        ..PrepareSettings::default()
    };
    let result = run(
        folder.path(),
        "sharp.wav",
        &samples,
        &settings,
        Some(measured_cents),
    );
    let correction = result.plan.tuning_correction_cents.unwrap();
    assert!((correction + measured_cents).abs() < 0.001);
    let (shifted, _, _) = decode(&result.prepared.output);
    assert_eq!(shifted.len(), samples.len(), "tempo is unchanged");
    let found = cents(frequency_of(&shifted), 440.0);
    assert!(found.abs() < 2.0, "{found} cents off 440 Hz");

    let flat = PrepareSettings {
        correct_tuning: false,
        ..settings
    };
    let untouched = run(
        folder.path(),
        "sharp-off.wav",
        &samples,
        &flat,
        Some(measured_cents),
    );
    assert_eq!(untouched.plan.tuning_correction_cents, None);
    let (kept, _, _) = decode(&untouched.prepared.output);
    assert!(cents(frequency_of(&kept), 440.0) > 15.0);

    let in_tune = run(folder.path(), "in-tune.wav", &samples, &settings, Some(1.5));
    assert_eq!(in_tune.plan.tuning_correction_cents, None);
}

fn tag_with_picture(path: &Path, tag_type: TagType) {
    let mut tag = Tag::new(tag_type);
    tag.set_title("Night Drive".to_string());
    tag.set_artist("Test Artist".to_string());
    tag.set_album("Test Album".to_string());
    tag.push_picture(
        Picture::unchecked(vec![0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4])
            .pic_type(PictureType::CoverFront)
            .mime_type(MimeType::Jpeg)
            .build(),
    );
    tag.save_to_path(path, WriteOptions::default()).unwrap();
}

#[test]
fn tags_and_cover_art_are_copied_and_the_original_is_untouched() {
    let folder = tempfile::tempdir().unwrap();
    let cases = [
        (
            "wav",
            "sine-440hz-stereo.wav",
            TagType::Id3v2,
            OutputFormat::Wav,
        ),
        (
            "aiff",
            "sine-440hz-mono.aiff",
            TagType::Id3v2,
            OutputFormat::Aiff,
        ),
        (
            "flac",
            "sine-440hz-mono.flac",
            TagType::VorbisComments,
            OutputFormat::Flac,
        ),
        (
            "mp3",
            "sine-440hz-mono.mp3",
            TagType::Id3v2,
            OutputFormat::Flac,
        ),
    ];
    for (label, fixture_name, tag_type, expected_format) in cases {
        let extension = fixture_name.rsplit('.').next().unwrap();
        let input = folder.path().join(format!("{label} source.{extension}"));
        std::fs::copy(fixture(fixture_name), &input).unwrap();
        tag_with_picture(&input, tag_type);
        let before = std::fs::read(&input).unwrap();

        let result = prepare_file(&input, &PrepareSettings::default(), None, LossyOutput::Flac);
        assert_eq!(result.plan.output_format, expected_format, "{label}");
        assert!(result.prepared.tags_copied, "{label}");
        assert_eq!(
            result
                .prepared
                .output
                .file_name()
                .unwrap()
                .to_string_lossy(),
            format!("{label} source (prepared).{}", expected_format.extension())
        );
        assert_eq!(
            std::fs::read(&input).unwrap(),
            before,
            "{label} original changed"
        );

        let copy = Probe::open(&result.prepared.output)
            .unwrap()
            .read()
            .unwrap();
        let tag = copy.primary_tag().expect("the copy has tags");
        assert_eq!(tag.title().as_deref(), Some("Night Drive"), "{label}");
        assert_eq!(tag.artist().as_deref(), Some("Test Artist"), "{label}");
        assert_eq!(tag.album().as_deref(), Some("Test Album"), "{label}");
        assert_eq!(tag.picture_count(), 1, "{label}");
        assert_eq!(
            &tag.pictures()[0].data()[..4],
            &[0xFF, 0xD8, 0xFF, 0xE0],
            "{label}"
        );
    }
}

#[test]
fn lossy_sources_never_become_lossy_files() {
    let folder = tempfile::tempdir().unwrap();
    for name in [
        "sine-440hz-mono.mp3",
        "sine-440hz-mono.m4a",
        "sine-440hz-mono.mp4",
        "sine-440hz-mono.ogg",
    ] {
        for (lossy, magic, extension) in [
            (LossyOutput::Flac, &b"fLaC"[..], "flac"),
            (LossyOutput::Wav, &b"RIFF"[..], "wav"),
        ] {
            let input = folder.path().join(format!("{extension}-{name}"));
            std::fs::copy(fixture(name), &input).unwrap();
            let result = prepare_file(&input, &PrepareSettings::default(), None, lossy);
            let output = &result.prepared.output;
            assert_eq!(output.extension().unwrap(), extension, "{name}");
            let bytes = std::fs::read(output).unwrap();
            assert_eq!(&bytes[..4], magic, "{name}");
            assert_eq!(output_format(&input, lossy).extension(), extension);
        }
    }
}

#[test]
fn the_dry_run_numbers_are_the_applied_numbers() {
    let folder = tempfile::tempdir().unwrap();
    let input = folder.path().join("track.wav");
    write_wav(&input, 2, &peaky_track());
    let settings = PrepareSettings::default();
    let dry = plan_track(&input, &settings, None, LossyOutput::Flac).unwrap();
    let again = plan_track(&input, &settings, None, LossyOutput::Flac).unwrap();
    assert_eq!(dry, again);
    let output = prepared_path(
        &input,
        None,
        " (prepared)",
        dry.output_format,
        &HashSet::new(),
    );
    let applied = prepare_track(&input, &dry, &output, &settings, &mut |_| {}).unwrap();
    assert_eq!(applied.plan, dry);
    let (lufs, _) = measure_file(&applied.output);
    assert!((lufs - dry.predicted_lufs.unwrap()).abs() < 0.8, "{lufs}");
}

#[test]
fn an_existing_file_is_never_overwritten() {
    let folder = tempfile::tempdir().unwrap();
    let input = folder.path().join("track.wav");
    write_wav(&input, 2, &sine(440.0, 0.1, 1.0));
    let first = prepared_path(
        &input,
        None,
        " (prepared)",
        OutputFormat::Wav,
        &HashSet::new(),
    );
    std::fs::write(&first, b"keep me").unwrap();
    let second = prepared_path(
        &input,
        None,
        " (prepared)",
        OutputFormat::Wav,
        &HashSet::new(),
    );
    assert_ne!(first, second);
    assert!(second.to_string_lossy().ends_with("(prepared) 2.wav"));
    let reserved = HashSet::from([second.clone()]);
    let third = prepared_path(&input, None, " (prepared)", OutputFormat::Wav, &reserved);
    assert!(third.to_string_lossy().ends_with("(prepared) 3.wav"));

    let settings = PrepareSettings::default();
    let plan = plan_track(&input, &settings, None, LossyOutput::Flac).unwrap();
    let refused = prepare_track(&input, &plan, &first, &settings, &mut |_| {});
    assert!(refused.is_err());
    assert_eq!(std::fs::read(&first).unwrap(), b"keep me");
    let onto_itself = prepare_track(&input, &plan, &input, &settings, &mut |_| {});
    assert!(onto_itself.is_err());
}

#[test]
fn clipped_passages_are_reported_and_silence_is_skipped() {
    let folder = tempfile::tempdir().unwrap();
    let mut samples = sine(200.0, 0.2, 4.0);
    for index in 0..10 {
        samples[(RATE as usize + index) * 2] = 1.0;
        samples[(RATE as usize + index) * 2 + 1] = 1.0;
    }
    let input = folder.path().join("clipped.wav");
    write_wav(&input, 2, &samples);
    let plan = plan_track(&input, &PrepareSettings::default(), None, LossyOutput::Flac).unwrap();
    assert_eq!(plan.clipping.runs, 1);
    assert_eq!(plan.clipping.clipped_frames, 10);
    assert_eq!(plan.clipping.longest_run_frames, 10);
    assert!((plan.clipping.first_run_seconds.unwrap() - 1.0).abs() < 0.001);

    let clean = folder.path().join("clean.wav");
    write_wav(&clean, 2, &sine(200.0, 0.2, 2.0));
    let plan = plan_track(&clean, &PrepareSettings::default(), None, LossyOutput::Flac).unwrap();
    assert_eq!(plan.clipping.runs, 0);

    let silent = folder.path().join("silent.wav");
    write_wav(&silent, 2, &vec![0.0; RATE as usize * 2]);
    let plan = plan_track(
        &silent,
        &PrepareSettings::default(),
        None,
        LossyOutput::Flac,
    )
    .unwrap();
    assert_eq!(plan.skipped, Some(SkipReason::Silent));
}

#[test]
fn aiff_output_keeps_rate_channels_and_length() {
    let folder = tempfile::tempdir().unwrap();
    let input = folder.path().join("mono.aiff");
    std::fs::copy(fixture("sine-440hz-mono.aiff"), &input).unwrap();
    let (original, rate, channels) = decode(&input);
    let result = prepare_file(&input, &PrepareSettings::default(), None, LossyOutput::Flac);
    let (copy, copy_rate, copy_channels) = decode(&result.prepared.output);
    assert_eq!((rate, channels), (copy_rate, copy_channels));
    assert_eq!(original.len(), copy.len());
}

#[test]
fn tag_only_writes_replaygain_without_touching_the_audio_and_reverts() {
    let folder = tempfile::tempdir().unwrap();
    let settings = PrepareSettings::default();
    for (name, label) in [
        ("sine-440hz-stereo.wav", "wav"),
        ("sine-440hz-mono.flac", "flac"),
        ("sine-440hz-mono.aiff", "aiff"),
        ("sine-440hz-mono.mp3", "mp3"),
        ("sine-440hz-mono.m4a", "m4a"),
    ] {
        let extension = name.rsplit('.').next().unwrap();
        let input = folder.path().join(format!("{label}.{extension}"));
        std::fs::copy(fixture(name), &input).unwrap();
        tag_with_picture(
            &input,
            if extension == "flac" {
                TagType::VorbisComments
            } else if extension == "m4a" {
                TagType::Mp4Ilst
            } else {
                TagType::Id3v2
            },
        );
        let (audio_before, _, _) = decode(&input);
        let plan = plan_track(&input, &settings, None, LossyOutput::Flac).unwrap();
        let fields = ReplayGainFields::from_plan(&plan).unwrap();
        assert_eq!(
            fields.gain.as_deref().unwrap(),
            format!(
                "{:+.2} dB",
                REPLAYGAIN_REFERENCE_LUFS - plan.measured_lufs.unwrap()
            )
        );

        let mut manifest = Manifest::new(RunMode::TagOnly, &settings);
        tag_track(&mut manifest, &input, &fields).unwrap();
        assert_eq!(read_replaygain(&input).unwrap(), fields, "{label}");
        let (audio_after, _, _) = decode(&input);
        assert_eq!(audio_before, audio_after, "{label}: the audio changed");
        let tagged = Probe::open(&input).unwrap().read().unwrap();
        assert_eq!(
            tagged.primary_tag().unwrap().title().as_deref(),
            Some("Night Drive"),
            "{label}"
        );

        let manifest_path = manifest.save(folder.path()).unwrap();
        let report = revert(&manifest_path).unwrap();
        assert_eq!(report.restored, std::slice::from_ref(&input), "{label}");
        assert_eq!(
            read_replaygain(&input).unwrap(),
            ReplayGainFields::default(),
            "{label}"
        );
        assert!(!manifest_path.exists());
    }
}

#[test]
fn revert_removes_only_what_the_run_wrote_and_checks_the_checksum() {
    let folder = tempfile::tempdir().unwrap();
    let settings = PrepareSettings::default();
    let mut manifest = Manifest::new(RunMode::Copies, &settings);
    let mut outputs = Vec::new();
    let mut inputs = Vec::new();
    for name in ["one", "two"] {
        let input = folder.path().join(format!("{name}.wav"));
        write_wav(&input, 2, &sine(440.0, 0.1, 1.0));
        let run = prepare_file(&input, &settings, None, LossyOutput::Flac);
        manifest.record_copy(&input, &run.prepared.output).unwrap();
        outputs.push(run.prepared.output);
        inputs.push(input);
    }
    let unrelated = folder.path().join("unrelated (prepared).wav");
    std::fs::write(&unrelated, b"not ours").unwrap();
    std::fs::write(&outputs[1], b"edited by the user").unwrap();
    let manifest_path = manifest.save(folder.path()).unwrap();
    assert_eq!(
        manifests_in(folder.path()),
        std::slice::from_ref(&manifest_path)
    );

    let report = revert(&manifest_path).unwrap();
    assert_eq!(report.removed, [outputs[0].clone()]);
    assert_eq!(report.kept.len(), 1);
    assert_eq!(report.kept[0].0, outputs[1]);
    assert!(!outputs[0].exists());
    assert!(outputs[1].exists(), "a changed file is kept");
    assert!(unrelated.exists());
    assert!(inputs.iter().all(|input| input.exists()));
    assert!(manifest_path.exists(), "what was not undone stays recorded");
    assert_eq!(Manifest::load(&manifest_path).unwrap().entries.len(), 1);
}

#[test]
fn an_unsaved_manifest_can_put_back_tag_only_changes() {
    let folder = tempfile::tempdir().unwrap();
    let input = folder.path().join("track.mp3");
    std::fs::copy(fixture("sine-440hz-mono.mp3"), &input).unwrap();
    let settings = PrepareSettings::default();
    let plan = plan_track(&input, &settings, None, LossyOutput::Flac).unwrap();
    let fields = ReplayGainFields::from_plan(&plan).unwrap();
    let mut manifest = Manifest::new(RunMode::TagOnly, &settings);
    tag_track(&mut manifest, &input, &fields).unwrap();

    let report = revert_unrecorded(&manifest);

    assert_eq!(report.restored.as_slice(), std::slice::from_ref(&input));
    assert!(report.kept.is_empty());
    assert_eq!(
        read_replaygain(&input).unwrap(),
        ReplayGainFields::default()
    );
}

#[test]
fn a_prepare_plan_refuses_new_audio_with_the_same_format() {
    let folder = tempfile::tempdir().unwrap();
    let input = folder.path().join("track.wav");
    let output = folder.path().join("prepared.wav");
    write_wav(&input, 2, &sine(440.0, 0.1, 1.0));
    let settings = PrepareSettings::default();
    let plan = plan_track(&input, &settings, None, LossyOutput::Flac).unwrap();
    write_wav(&input, 2, &sine(440.0, 0.8, 1.0));

    let error = prepare_track(&input, &plan, &output, &settings, &mut |_| {}).unwrap_err();

    assert!(error.chain().contains("changed since it was measured"));
    assert!(!output.exists());
    assert!(
        plan.verify_source(&input).is_err(),
        "tag-only must also refuse it"
    );
}

#[test]
fn the_comparison_clip_is_level_matched_and_the_same_length() {
    let folder = tempfile::tempdir().unwrap();
    let input = folder.path().join("peaky.wav");
    write_wav(&input, 2, &peaky_track());
    let settings = PrepareSettings::default();
    let plan = plan_track(&input, &settings, None, LossyOutput::Flac).unwrap();
    let clip = comparison_clip(&input, &plan, &settings, 1.0, 3.0).unwrap();
    assert_eq!(clip.original.len(), clip.prepared.len());
    assert_eq!(clip.original.len(), 3 * RATE as usize * 2);
    let loudness = |samples: &[f32]| {
        let mut meter = EbuR128::new(2, RATE, Mode::I).unwrap();
        meter.add_frames_f32(samples).unwrap();
        meter.loudness_global().unwrap()
    };
    assert!((loudness(&clip.original) - loudness(&clip.prepared)).abs() < 0.1);
    assert!(!folder.path().read_dir().unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("prepared")
    }));
}
