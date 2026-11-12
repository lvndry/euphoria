// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared compute-only frontend plus unchanged full/chunked inference.
use anyhow::{Context, Result, ensure};
use clap::{Parser, ValueEnum};
use drone_native_cached::{
    Bundle, Chunked, Full, bytes_sha256, check_file, compare, environment, file_sha256,
    process_usage,
};
use drone_native_cqt::{
    artifact_root,
    plan::{NumpyMath, PreparedPlan, Soxr},
    read_f32,
};
use serde_json::{Value, json};
use std::{fs, path::PathBuf, time::Instant};
#[derive(Clone, Copy, ValueEnum, PartialEq, Eq)]
enum Mode {
    Qualify,
    Benchmark,
}
#[derive(Clone, Copy, ValueEnum, PartialEq, Eq)]
enum Backend {
    Full,
    Chunked,
    Both,
}
#[derive(Parser)]
struct Args {
    #[arg(long)]
    core_output_directory: Option<PathBuf>,
    #[arg(long, value_enum)]
    mode: Mode,
    #[arg(long, value_enum)]
    backend: Backend,
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long)]
    full_model: PathBuf,
    #[arg(long)]
    bundle: Option<PathBuf>,
    #[arg(long)]
    ort_library: PathBuf,
    #[arg(long)]
    soxr_wrapper: PathBuf,
    #[arg(long)]
    soxr_sha256: String,
    #[arg(long)]
    math_library: Option<PathBuf>,
    #[arg(long)]
    math_sha256: Option<String>,
    #[arg(long)]
    output: PathBuf,
    #[arg(long)]
    exact_core: bool,
    #[arg(long)]
    qualification_report: Option<PathBuf>,
}
const MODEL: &str = "35fc1bfe5d67f7aed2c46221c15519d0f3d5822351932b5bd9899d47d48cb613";
fn core_hash(values: &[f32]) -> String {
    let bytes: Vec<_> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    bytes_sha256(&bytes)
}
fn logits(value: &Value) -> Result<[f32; 24]> {
    value
        .as_array()
        .context("reference logits")?
        .iter()
        .map(|v| v.as_f64().map(|v| v as f32).context("logit"))
        .collect::<Result<Vec<_>>>()?
        .try_into()
        .map_err(|_| anyhow::anyhow!("24 logits required"))
}
fn resident_rss() -> Result<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = fs::read_to_string("/proc/self/status")?;
        let kb = status
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))
            .context("VmRSS")?
            .split_whitespace()
            .next()
            .context("RSS value")?
            .parse::<u64>()?;
        Ok(kb * 1024)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let output = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()?;
        Ok(std::str::from_utf8(&output.stdout)?.trim().parse::<u64>()? * 1024)
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(!args.output.exists(), "immutable output exists");
    check_file(&args.full_model, MODEL)?;
    check_file(&args.soxr_wrapper, &args.soxr_sha256)?;
    if let Some(path) = &args.math_library {
        check_file(path, args.math_sha256.as_deref().context("math hash")?)?;
    }
    let executable = file_sha256(&std::env::current_exe()?)?;
    if args.mode == Mode::Benchmark {
        ensure!(
            args.backend != Backend::Both,
            "one backend per fresh benchmark worker"
        );
        let qualification: Value = serde_json::from_slice(&fs::read(
            args.qualification_report
                .as_ref()
                .context("qualification required")?,
        )?)?;
        ensure!(
            qualification["complete"] == true
                && qualification["passed"] == true
                && qualification["tracks"] == 376
                && qualification["identity"]["native_sha256"] == executable,
            "passing same-binary376 qualification required"
        );
    }
    if args.mode == Mode::Benchmark {
        let qualification: Value = serde_json::from_slice(&fs::read(
            args.qualification_report
                .as_ref()
                .context("qualification")?,
        )?)?;
        for (key, path) in [
            ("ort_sha256", Some(&args.ort_library)),
            ("soxr_sha256", Some(&args.soxr_wrapper)),
            ("math_sha256", args.math_library.as_ref()),
        ] {
            let current = path.map(|path| file_sha256(path)).transpose()?;
            ensure!(
                qualification["identity"][key].as_str() == current.as_deref(),
                "qualification/runtime artifact changed: {key}"
            );
        }
        if let Some(bundle) = &args.bundle {
            for name in ["prefix.onnx", "head.onnx", "contract.json"] {
                ensure!(
                    qualification["identity"]["bundle"][name].as_str()
                        == Some(file_sha256(&bundle.join(name))?.as_str()),
                    "chunk artifact changed"
                );
            }
        }
        ensure!(args.exact_core, "timed path requires exact core gate");
    }
    let manifest: Value = serde_json::from_slice(&fs::read(&args.manifest)?)?;
    ensure!(
        manifest["complete"] == true && manifest["full_model_sha256"] == MODEL,
        "manifest/model contract"
    );
    let cases = manifest["cases"].as_array().context("cases")?;
    ensure!(!cases.is_empty(), "empty cases");
    if args.mode == Mode::Benchmark {
        ensure!(cases.len() == 1, "fresh worker is one track");
    }
    let root = artifact_root(&args.manifest)?;
    let runtime = environment(&args.ort_library)?;
    let mut full = if args.backend != Backend::Chunked {
        Some(Full::new(&runtime, &args.full_model)?)
    } else {
        None
    };
    let mut chunked = if args.backend != Backend::Full {
        let path = args.bundle.as_ref().context("chunk bundle required")?;
        let bundle = Bundle::load(
            path,
            "dcf8a7a292b7c12076ffff63c8c6550cb09a6c41ce4d000fd7949fc6bb4b452c",
        )?;
        Some(Chunked::new(&runtime, path, &bundle)?)
    } else {
        None
    };
    let soxr = Soxr::new(&args.soxr_wrapper)?;
    let math = args
        .math_library
        .as_deref()
        .map(NumpyMath::new)
        .transpose()?;
    if let Some(directory) = &args.core_output_directory {
        ensure!(args.mode == Mode::Qualify, "core dumps are diagnostic only");
        fs::create_dir(directory)?;
    }
    let mut rows = Vec::new();
    let mut passed = true;
    for case in cases {
        let pcm = read_f32(root, &case["pcm"])?;
        let plan = PreparedPlan::load(root, case)?;
        let expected = logits(&case["reference_logits"])?;
        let expected_core = case["core"]["sha256"].as_str().context("core hash")?;
        let loaded_rss = resident_rss()?;
        let repetitions = if args.mode == Mode::Benchmark { 21 } else { 1 };
        let mut seconds = Vec::new();
        let mut results = Vec::new();
        for iteration in 0..repetitions {
            let started = Instant::now();
            let (core, frames) = plan.compute(&pcm, &soxr, math.as_ref(), true)?;
            let mut predictions = Vec::new();
            if let Some(model) = &mut full {
                predictions.push(("full", model.predict(&core, frames)?));
            }
            if let Some(model) = &mut chunked {
                predictions.push(("chunked", model.predict(&core, frames)?));
            }
            let duration = started.elapsed().as_secs_f64();
            ensure!(
                Some(frames as u64) == case["frames"].as_u64(),
                "frame mismatch"
            );
            let actual_hash = core_hash(&core);
            if iteration == 0
                && let Some(directory) = &args.core_output_directory
            {
                use std::io::Write;
                let path = directory.join(format!("{:04}.f32", rows.len()));
                let mut file = fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(path)?;
                let bytes: Vec<_> = core.iter().flat_map(|value| value.to_le_bytes()).collect();
                file.write_all(&bytes)?;
            }
            let core_matches = actual_hash == expected_core;
            if args.mode == Mode::Benchmark && iteration > 0 {
                seconds.push(duration);
            }
            let mut comparisons = Vec::new();
            let mut iteration_passed = core_matches || !args.exact_core;
            for (backend, prediction) in predictions {
                let parity = compare(&prediction, &expected)?;
                iteration_passed &= parity.allclose && parity.argmax_agrees;
                let canonical = case
                    .get("canonical_linux_logits")
                    .map(logits)
                    .transpose()?
                    .map(|reference| compare(&prediction, &reference))
                    .transpose()?;
                comparisons.push(json!({"backend":backend,"logits":prediction,"comparison":parity,"canonical_linux_comparison":canonical}));
            }
            passed &= iteration_passed;
            results.push(json!({"passed":iteration_passed,"core_hash_matches":core_matches,"core_sha256":actual_hash,"predictions":comparisons}));
        }
        rows.push(json!({"id":case["id"],"frames":case["frames"],"pcm_sha256":case["pcm"]["sha256"],"seconds":seconds,"iterations":results,"loaded_rss_bytes":loaded_rss}));
    }
    let (cpu, peak) = process_usage()?;
    let report = json!({"format":"k10-composed-pipeline-v1","passed":passed,"cases":rows,"cpu_seconds":cpu,"peak_rss_bytes":peak,"identity":{"executable_sha256":executable,"pipeline_source_sha256":bytes_sha256(include_bytes!("pipeline.rs")),"plan_source_sha256":bytes_sha256(include_bytes!("../plan.rs")),"evidence_source_sha256":bytes_sha256(include_bytes!("../evidence.rs")),"manifest_sha256":file_sha256(&args.manifest)?,"full_model_sha256":MODEL,"ort_sha256":file_sha256(&args.ort_library)?,"soxr_sha256":file_sha256(&args.soxr_wrapper)?,"math_sha256":args.math_library.as_ref().map(|p|file_sha256(p)).transpose()?,"qualification_sha256":args.qualification_report.as_ref().map(|p|file_sha256(p)).transpose()?}});
    fs::write(&args.output, serde_json::to_vec_pretty(&report)?)?;
    ensure!(passed, "composed parity gate failed; evidence saved");
    Ok(())
}
