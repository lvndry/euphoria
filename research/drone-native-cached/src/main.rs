//! One-stage-per-process research CLI; no audio decoding or label scoring.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result, ensure};
use clap::{Parser, ValueEnum};
use drone_native_cached::{
    ATOL, Bundle, Chunked, Full, Manifest, RTOL, RUNTIME_VERSION, argmax, bytes_sha256, check_file,
    compare, environment, file_sha256, key_name, limit_cpu, load_features, median, process_usage,
};
use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Mode {
    Validate,
    Full,
    Chunked,
}

#[derive(Debug, Parser)]
#[command(about = "CPU-only K10 cached-feature parity and fresh-process benchmark")]
struct Arguments {
    #[arg(long, value_enum)]
    mode: Mode,
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long)]
    bundle: PathBuf,
    #[arg(long)]
    full_model: PathBuf,
    #[arg(long)]
    full_model_sha256: String,
    #[arg(long)]
    ort_library: PathBuf,
    #[arg(long)]
    output: PathBuf,
    /// Required for full/chunked benchmark, forbidden for validation.
    #[arg(long)]
    selection_only: bool,
    /// Exact passing native validation report; required for benchmarks.
    #[arg(long)]
    validation_report: Option<PathBuf>,
    /// Optional fresh full report to compute paired chunked resource comparison.
    #[arg(long)]
    full_report: Option<PathBuf>,
}

fn thread_environment() -> BTreeMap<&'static str, Option<String>> {
    [
        "OMP_NUM_THREADS",
        "MKL_NUM_THREADS",
        "OPENBLAS_NUM_THREADS",
        "NUMEXPR_NUM_THREADS",
    ]
    .into_iter()
    .map(|name| (name, std::env::var(name).ok()))
    .collect()
}

fn cpu_affinity() -> Option<String> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| {
            line.strip_prefix("Cpus_allowed_list:")
                .map(str::trim)
                .map(str::to_owned)
        })
}

fn process_threads() -> Option<usize> {
    Some(std::fs::read_dir("/proc/self/task").ok()?.count())
}

fn identity(args: &Arguments) -> Result<Value> {
    Ok(json!({
        "manifest_sha256": file_sha256(&args.manifest)?,
        "contract_sha256": file_sha256(&args.bundle.join("contract.json"))?,
        "prefix_sha256": file_sha256(&args.bundle.join("prefix.onnx"))?,
        "head_sha256": file_sha256(&args.bundle.join("head.onnx"))?,
        "full_model_sha256": file_sha256(&args.full_model)?,
        "ort_library_sha256": file_sha256(&args.ort_library)?,
        "executable_sha256": file_sha256(&std::env::current_exe()?)?,
        "library_source_sha256": bytes_sha256(include_bytes!("lib.rs")),
        "cli_source_sha256": bytes_sha256(include_bytes!("main.rs")),
        "cargo_lock_sha256": bytes_sha256(include_bytes!("../Cargo.lock")),
        "ort_crate": "2.0.0-rc.15", "onnxruntime": RUNTIME_VERSION,
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "cpu_affinity": cpu_affinity(), "thread_environment": thread_environment(),
        "atol": ATOL, "rtol": RTOL, "core_outputs": 64,
        "intra_threads": 1, "inter_threads": 1,
        "execution_mode": "sequential", "spinning": false,
        "device_policy":"PreferCPU", "cpu_arena":true,
    }))
}

fn verify_validation(report: &Value, identity: &Value) -> Result<()> {
    ensure!(
        report["mode"] == "validate"
            && report["passed"] == true
            && report["tracks"] == 376
            && report["identity"] == *identity,
        "Validation report does not pass for this exact binary/runtime/experiment"
    );
    Ok(())
}

fn execute(args: &Arguments, report: &mut Value) -> Result<()> {
    let manifest = Manifest::load(&args.manifest)?;
    let bundle = Bundle::load(&args.bundle, &manifest.checkpoint_sha256)?;
    ensure!(
        manifest.full_model_sha256 == args.full_model_sha256,
        "Full model identity differs from reference fixture manifest"
    );
    check_file(&args.full_model, &args.full_model_sha256)?;
    report["identity"] = identity(args)?;
    report["checkpoint_sha256"] = json!(manifest.checkpoint_sha256);
    report["reference_validation_report_sha256"] = json!(manifest.validation_report_sha256);
    report["fixture_export_tool_sha256"] = json!(manifest.export_tool_sha256);
    if args.mode == Mode::Validate {
        ensure!(
            !args.selection_only && args.validation_report.is_none() && args.full_report.is_none(),
            "Validation requires all376 and no previous reports"
        );
    } else {
        ensure!(
            args.selection_only,
            "Benchmarks require exactly selection20"
        );
        let path = args
            .validation_report
            .as_ref()
            .context("Benchmark requires --validation-report")?;
        let validation: Value = serde_json::from_reader(File::open(path)?)?;
        verify_validation(&validation, &report["identity"])?;
        report["validation_report_sha256"] = json!(file_sha256(path)?);
        ensure!(
            args.full_report.is_none() || args.mode == Mode::Chunked,
            "--full-report is only for chunked benchmark"
        );
    }
    let environment = environment(&std::fs::canonicalize(&args.ort_library)?)?;
    report["ort_build_info"] = json!(ort::info());
    // Each measurement process constructs only its own mode's sessions.
    let mut full = if args.mode != Mode::Chunked {
        Some(Full::new(&environment, &args.full_model)?)
    } else {
        None
    };
    let mut chunked = if args.mode != Mode::Full {
        Some(Chunked::new(&environment, &args.bundle, &bundle)?)
    } else {
        None
    };
    report["loaded_peak_rss_kib"] = json!(process_usage()?.1);
    let root = args
        .manifest
        .parent()
        .context("Manifest requires a parent directory")?;
    let selected: Vec<_> = manifest
        .rows
        .iter()
        .filter(|r| !args.selection_only || r.selection20)
        .collect();
    let mut records = Vec::new();
    let mut passed = true;
    let mut max_error = 0_f64;
    let mut disagreements = 0;
    for row in &selected {
        let started = Instant::now();
        let input = load_features(root, row)?;
        let fixture_load_seconds = started.elapsed().as_secs_f64();
        let record = if args.mode == Mode::Validate {
            let full_logits = full
                .as_mut()
                .context("Missing full session")?
                .predict(&input, row.frames)?;
            let chunk_logits = chunked
                .as_mut()
                .context("Missing chunk sessions")?
                .predict(&input, row.frames)?;
            let full_comparison = compare(&full_logits, &row.reference_logits)?;
            let chunk_comparison = compare(&chunk_logits, &row.reference_logits)?;
            let native_comparison = compare(&chunk_logits, &full_logits)?;
            for comparison in [&full_comparison, &chunk_comparison, &native_comparison] {
                passed &= comparison.allclose && comparison.argmax_agrees;
                max_error = max_error.max(comparison.max_abs_error);
                disagreements += usize::from(!comparison.argmax_agrees);
            }
            json!({"full_vs_reference": full_comparison, "chunked_vs_reference": chunk_comparison,
                "chunked_vs_native_full": native_comparison,
                "full_logits": full_logits, "chunked_logits": chunk_logits})
        } else {
            let mut predict = || -> Result<[f32; 24]> {
                match args.mode {
                    Mode::Full => full
                        .as_mut()
                        .context("Missing full session")?
                        .predict(&input, row.frames),
                    Mode::Chunked => chunked
                        .as_mut()
                        .context("Missing chunk sessions")?
                        .predict(&input, row.frames),
                    Mode::Validate => unreachable!(),
                }
            };
            predict()?; // Exactly one warmup; not included in timed repetitions.
            let mut elapsed = Vec::with_capacity(20);
            let mut cpu_elapsed = Vec::with_capacity(20);
            let mut logits = [0_f32; 24];
            for _ in 0..20 {
                let before_cpu = process_usage()?.0;
                let start = Instant::now();
                logits = predict()?;
                elapsed.push(start.elapsed().as_secs_f64());
                cpu_elapsed.push(process_usage()?.0 - before_cpu);
            }
            let comparison = compare(&logits, &row.reference_logits)?;
            passed &= comparison.allclose && comparison.argmax_agrees;
            json!({"seconds": elapsed, "cpu_seconds": cpu_elapsed,
                "median_seconds": median(&elapsed)?, "median_cpu_seconds": median(&cpu_elapsed)?,
                "reference_comparison": comparison, "logits": logits,
                "class_id": argmax(&logits)?, "key": key_name(argmax(&logits)?)?})
        };
        let mut record = record;
        record["id"] = json!(row.id);
        record["domain"] = json!(row.domain);
        record["audio_sha256"] = json!(row.audio_sha256);
        record["fixture_sha256"] = json!(row.sha256);
        record["cache_sha256"] = json!(row.cache_sha256);
        record["source_sha256"] = json!(row.source_sha256);
        record["frames"] = json!(row.frames);
        record["fixture_load_seconds"] = json!(fixture_load_seconds);
        record["process_threads"] = json!(process_threads());
        records.push(record);
        if records.len() % 20 == 0 {
            eprintln!("Processed {}/{}", records.len(), selected.len());
        }
    }
    report["tracks"] = json!(records.len());
    report["passed"] = json!(passed);
    report["records"] = json!(records);
    if args.mode == Mode::Validate {
        report["argmax_disagreements_across_comparisons"] = json!(disagreements);
        report["max_abs_logit_error"] = json!(max_error);
    } else {
        report["repetitions"] = json!(20);
        report["warmups_per_track"] = json!(1);
        report["median_track_median_seconds"] = json!(median(
            &records
                .iter()
                .map(|r| r["median_seconds"].as_f64().context("Missing median"))
                .collect::<Result<Vec<_>>>()?
        )?);
    }
    report["peak_process_rss_kib"] = json!(process_usage()?.1);
    if let Some(path) = &args.full_report {
        let baseline: Value = serde_json::from_reader(File::open(path)?)?;
        ensure!(
            baseline["mode"] == "full"
                && baseline["passed"] == true
                && baseline["identity"] == report["identity"]
                && baseline["tracks"] == 20,
            "Full benchmark report differs from this experiment"
        );
        let previous = baseline["records"]
            .as_array()
            .context("Missing baseline records")?;
        let mut ratios = Vec::new();
        for current in &records {
            let matches: Vec<_> = previous
                .iter()
                .filter(|old| {
                    old["audio_sha256"] == current["audio_sha256"]
                        && old["fixture_sha256"] == current["fixture_sha256"]
                })
                .collect();
            ensure!(
                matches.len() == 1,
                "Missing/duplicate paired benchmark identity"
            );
            ratios.push(
                current["median_seconds"]
                    .as_f64()
                    .context("Missing current median")?
                    / matches[0]["median_seconds"]
                        .as_f64()
                        .context("Missing baseline median")?,
            );
        }
        let latency_ratio = median(&ratios)?;
        let rss_ratio = baseline["peak_process_rss_kib"]
            .as_f64()
            .context("Missing baseline RSS")?
            / process_usage()?.1 as f64;
        report["paired_median_latency_ratio"] = json!(latency_ratio);
        report["paired_peak_rss_reduction"] = json!(rss_ratio);
        report["historical_535mib_peak_rss_reduction"] =
            json!(535.0 * 1024.0 / process_usage()?.1 as f64);
        report["performance_target_met"] = json!(
            passed
                && latency_ratio <= 1.1
                && rss_ratio >= 2.0
                && process_usage()?.1 <= 535 * 1024 / 2
        );
        report["full_report_sha256"] = json!(file_sha256(path)?);
    }
    ensure!(passed, "Parity gate failed; no promotion");
    Ok(())
}

fn main() -> Result<()> {
    let args = Arguments::parse();
    ensure!(
        !args.output.exists(),
        "Choose a new report path; previous evidence is immutable"
    );
    limit_cpu()?;
    let started = Instant::now();
    let cpu_start = process_usage()?.0;
    let mut report = json!({"format": "k10-native-cached-report-v1",
        "mode": match args.mode { Mode::Validate => "validate", Mode::Full => "full", Mode::Chunked => "chunked" },
        "labels_scored": false, "promotion": false, "cpu_budget_seconds": 600,
        "note": "Native cached-feature inference only. No decoding/CQT; not an end-to-end audio measurement."});
    let outcome = execute(&args, &mut report);
    if let Err(error) = &outcome {
        report["passed"] = json!(false);
        report["error"] = json!(format!("{error:#}"));
    }
    report["total_wall_seconds"] = json!(started.elapsed().as_secs_f64());
    report["total_cpu_seconds"] = json!(process_usage()?.0 - cpu_start);
    report["final_peak_process_rss_kib"] = json!(process_usage()?.1);
    if let Some(parent) = args.output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args.output)?;
    serde_json::to_writer_pretty(&mut output, &report)?;
    writeln!(output)?;
    eprintln!(
        "Report: {} (passed={})",
        args.output.display(),
        report["passed"]
    );
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_rejects_code_runtime_or_environment_drift() {
        let identity = json!({"executable_sha256":"a", "ort_library_sha256":"b", "thread_environment":{"OMP_NUM_THREADS":"1"}});
        let report = json!({"mode":"validate", "passed":true, "tracks":376, "identity":identity});
        assert!(verify_validation(&report, &identity).is_ok());
        for key in [
            "executable_sha256",
            "ort_library_sha256",
            "thread_environment",
        ] {
            let mut changed = identity.clone();
            changed[key] = json!("changed");
            assert!(verify_validation(&report, &changed).is_err());
        }
    }
}
