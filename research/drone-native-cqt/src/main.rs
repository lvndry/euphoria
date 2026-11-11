//! Identical-PCM research feasibility; no decoder, labels, accuracy scoring or promotion.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use drone_native_cached::{
    Full, argmax, bytes_sha256, check_file, compare, environment, file_sha256,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
mod plan;
mod owner {
    include!(concat!(env!("OUT_DIR"), "/owner.rs"));
}
#[derive(Parser)]
struct Args {
    #[arg(long, default_value = "owner")]
    candidate: String,
    #[arg(long)]
    soxr_wrapper: Option<PathBuf>,
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long)]
    full_model: PathBuf,
    #[arg(long)]
    ort_library: PathBuf,
    #[arg(long)]
    output: PathBuf,
}
fn read_f32(root: &Path, artifact: &Value) -> Result<Vec<f32>> {
    let relative = Path::new(artifact["path"].as_str().context("artifact path")?);
    ensure!(!relative.is_absolute(), "absolute artifact");
    let path = root.join(relative).canonicalize()?;
    ensure!(
        path.starts_with(root.canonicalize()?),
        "artifact escapes root"
    );
    check_file(&path, artifact["sha256"].as_str().context("artifact hash")?)?;
    let bytes = fs::read(path)?;
    ensure!(
        bytes.len() % 4 == 0
            && bytes.len() as u64 == artifact["bytes"].as_u64().context("bytes")?,
        "artifact size"
    );
    let values: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    ensure!(values.iter().all(|x| x.is_finite()), "nonfinite input");
    Ok(values)
}
fn diagnostics(actual: &[f32], reference: &[f32]) -> Result<Value> {
    ensure!(actual.len() == reference.len(), "feature size mismatch");
    ensure!(actual.iter().all(|v| v.is_finite()), "nonfinite features");
    let mut maximum = 0.0f64;
    let mut squares = 0.0;
    let mut changed = 0;
    for (&a, &b) in actual.iter().zip(reference) {
        let d = (f64::from(a) - f64::from(b)).abs();
        maximum = maximum.max(d);
        squares += d * d;
        changed +=
            usize::from(half::f16::from_f32(a).to_bits() != half::f16::from_f32(b).to_bits());
    }
    Ok(
        json!({"max_abs":maximum,"rms":(squares/actual.len() as f64).sqrt(),"fp16_mismatches":changed,"values":actual.len(),"fp16_mismatch_rate":changed as f64/actual.len() as f64}),
    )
}
fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(!args.output.exists(), "immutable output exists");
    ensure!(
        matches!(args.candidate.as_str(), "owner" | "exported"),
        "unknown candidate"
    );
    let manifest: Value = serde_json::from_slice(&fs::read(&args.manifest)?)?;
    ensure!(
        manifest["complete"] == true
            && manifest["tracks"] == 9
            && manifest["format"] == "k10-cqt-goldens-v1",
        "incomplete/wrong goldens"
    );
    check_file(
        &args.full_model,
        manifest["full_model_sha256"]
            .as_str()
            .context("model hash")?,
    )?;
    let environment = environment(&args.ort_library)?;
    let mut model = Full::new(&environment, &args.full_model)?;
    let root = args.manifest.parent().context("golden root")?;
    let mut rows = Vec::new();
    let mut passed = true;
    for case in manifest["cases"].as_array().context("cases")? {
        let pcm = read_f32(root, &case["pcm"])?;
        let reference = read_f32(root, &case["core"])?;
        let (actual, frames, stage_diagnostics) = if args.candidate == "exported" {
            let soxr = plan::Soxr::new(
                args.soxr_wrapper
                    .as_deref()
                    .context("exported requires --soxr-wrapper")?,
            )?;
            plan::features(root, case, &pcm, &soxr)?
        } else {
            let (values, frames) = owner::features(&pcm);
            (values, frames, json!({}))
        };
        ensure!(
            Some(frames as u64) == case["frames"].as_u64(),
            "frame mismatch"
        );
        let logits = model.predict(&actual, frames)?;
        let expected: Vec<f32> = case["reference_logits"]
            .as_array()
            .context("logits")?
            .iter()
            .map(|x| x.as_f64().map(|v| v as f32).context("logit"))
            .collect::<Result<_>>()?;
        let expected: [f32; 24] = expected
            .try_into()
            .map_err(|_| anyhow::anyhow!("24 logits"))?;
        let parity = compare(&logits, &expected)?;
        passed &= parity.allclose && parity.argmax_agrees;
        let row = json!({"id":case["id"],"frames":frames,"stage_diagnostics":stage_diagnostics,"features":diagnostics(&actual,&reference)?,"comparison":parity,"logits":logits,"argmax":argmax(&logits)?});
        eprintln!("{}", row);
        rows.push(row);
    }
    let result = json!({"format":"k10-native-cqt-feasibility-v1","candidate":args.candidate,"passed":passed,"gate":"finite logits; atol=1e-5 rtol=1e-5; zero argmax disagreement; nine fixed identical-PCM cases","manifest_sha256":file_sha256(&args.manifest)?,"full_model_sha256":file_sha256(&args.full_model)?,"ort_library_sha256":file_sha256(&args.ort_library)?,"executable_sha256":file_sha256(&std::env::current_exe()?)?,"source_sha256":bytes_sha256(include_bytes!("main.rs")),"plan_source_sha256":bytes_sha256(include_bytes!("plan.rs")),"soxr_wrapper_sha256":args.soxr_wrapper.as_ref().map(|p|file_sha256(p)).transpose()?,"owner_source_sha256":owner::identity(),"cases":rows});
    fs::write(&args.output, serde_json::to_vec_pretty(&result)?)?;
    ensure!(
        passed,
        "Frozen final-logit gate failed (diagnostics preserved)"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_preserve_half_mismatches_and_finites() -> Result<()> {
        let result = diagnostics(&[0., 1.], &[0., 1.001])?;
        ensure!(result["fp16_mismatches"] == 1);
        ensure!(diagnostics(&[f32::NAN], &[0.]).is_err());
        ensure!(diagnostics(&[0.], &[]).is_err());
        Ok(())
    }
    #[test]
    fn raw_artifact_integrity_and_size() -> Result<()> {
        let root = std::env::temp_dir().join(format!("cqt-test-{}", std::process::id()));
        fs::create_dir(&root)?;
        let path = root.join("pcm");
        let bytes = [0u8, 0, 128, 63];
        fs::write(&path, bytes)?;
        let mut artifact = json!({"path":"pcm","sha256":bytes_sha256(&bytes),"bytes":4});
        ensure!(read_f32(&root, &artifact)? == [1.]);
        artifact["bytes"] = json!(8);
        ensure!(read_f32(&root, &artifact).is_err());
        artifact["bytes"] = json!(4);
        artifact["sha256"] = json!("0".repeat(64));
        ensure!(read_f32(&root, &artifact).is_err());
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
