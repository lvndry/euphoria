//! Diagnostic only: reproduce the executed scalar finalization and report bits.
//! No predictions, labels, replacement frontend or parity qualification.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use drone_native_cached::{bytes_sha256, check_file, file_sha256};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
#[derive(Parser)]
struct Args {
    #[arg(long)]
    input: PathBuf,
    #[arg(long)]
    output: PathBuf,
}
fn raw(root: &Path, value: &Value) -> Result<Vec<u8>> {
    let path = root
        .join(value["path"].as_str().context("path")?)
        .canonicalize()?;
    ensure!(
        path.starts_with(root.canonicalize()?),
        "outside diagnostic root"
    );
    check_file(&path, value["sha256"].as_str().context("hash")?)?;
    Ok(fs::read(path)?)
}
fn floats(root: &Path, value: &Value) -> Result<Vec<f32>> {
    let bytes = raw(root, value)?;
    ensure!(bytes.len() % 4 == 0, "f32 bytes");
    let values: Vec<f32> = bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    ensure!(
        values.iter().all(|v| v.is_finite()),
        "nonfinite diagnostic input"
    );
    Ok(values)
}
fn bits(value: f32) -> Value {
    json!({"value":value,"bits":format!("{:08x}",value.to_bits())})
}
fn tie(value: f32) -> Value {
    let half = half::f16::from_f32(value);
    let rounded = half.to_f32();
    let (lower, upper) = if rounded <= value {
        (rounded, half::f16::from_bits(half.to_bits() + 1).to_f32())
    } else {
        (half::f16::from_bits(half.to_bits() - 1).to_f32(), rounded)
    };
    let midpoint = (f64::from(lower) + f64::from(upper)) / 2.;
    json!({"lower":lower,"upper":upper,"midpoint":midpoint,"signed_distance":f64::from(value)-midpoint,"rounded_bits":format!("{:04x}",half.to_bits())})
}
fn source_finite_abs(re: f32, im: f32) -> f32 {
    let larger = re.abs().max(im.abs());
    let smaller = re.abs().min(im.abs());
    if larger == 0. {
        return 0.;
    }
    let ratio = smaller / larger;
    ratio.mul_add(ratio, 1.).sqrt() * larger
}
fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(!args.output.exists(), "immutable output exists");
    let input: Value = serde_json::from_slice(&fs::read(&args.input)?)?;
    let root = args.input.parent().context("root")?;
    let frames = input["frames"].as_u64().context("frames")? as usize;
    let pre = floats(root, &input["pre"])?;
    let normalized = floats(root, &input["normalized"])?;
    let magnitude = floats(root, &input["magnitude"])?;
    let logged = floats(root, &input["logged"])?;
    let reference = floats(root, &input["reference"])?;
    let length_bytes = raw(root, &input["lengths"])?;
    ensure!(length_bytes.len() == 192 * 8, "lengths");
    let lengths: Vec<f64> = length_bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|b| f64::from_le_bytes(*b))
        .collect();
    ensure!(
        pre.len() == 192 * frames * 2
            && normalized.len() == pre.len()
            && magnitude.len() == 192 * frames
            && logged.len() == magnitude.len()
            && reference.len() == magnitude.len(),
        "array contract"
    );
    let mut rows = Vec::new();
    let mut normalization_differences = 0;
    let mut magnitude_differences = 0;
    let mut log1p_differences_on_reference_magnitude = 0;
    let mut source_formula_differences = 0;
    let mut maximum = 0f64;
    let mut squares = 0f64;
    for position in 0..reference.len() {
        let scale = lengths[position / frames].sqrt();
        let re = (f64::from(pre[2 * position]) / scale) as f32;
        let im = (f64::from(pre[2 * position + 1]) / scale) as f32;
        normalization_differences += usize::from(
            re.to_bits() != normalized[2 * position].to_bits()
                || im.to_bits() != normalized[2 * position + 1].to_bits(),
        );
        let mag = re.hypot(im);
        let log = mag.ln_1p();
        let refmag = magnitude[position];
        let reflog = logged[position];
        magnitude_differences += usize::from(mag.to_bits() != refmag.to_bits());
        let scalar_log_refmag = refmag.ln_1p();
        log1p_differences_on_reference_magnitude +=
            usize::from(scalar_log_refmag.to_bits() != reflog.to_bits());
        let source = source_finite_abs(re, im);
        source_formula_differences += usize::from(source.to_bits() != refmag.to_bits());
        let rounded = half::f16::from_f32(log).to_f32();
        let difference = (f64::from(rounded) - f64::from(reference[position])).abs();
        maximum = maximum.max(difference);
        squares += difference * difference;
        if rounded.to_bits() != reference[position].to_bits() {
            rows.push(json!({"core_bin":position/frames,"padded_bin":position/frames+8,"frame":position%frames,"pre_re":bits(pre[2*position]),"pre_im":bits(pre[2*position+1]),"normalization_length":lengths[position/frames],"normalization_sqrt":scale,"rust_normalized_re":bits(re),"rust_normalized_im":bits(im),"numpy_normalized_re":bits(normalized[2*position]),"numpy_normalized_im":bits(normalized[2*position+1]),"rust_hypot":bits(mag),"numpy_vector_magnitude":bits(refmag),"source_defined_finite_simd_abs":bits(source),"rust_log1p_on_numpy_magnitude":bits(scalar_log_refmag),"numpy_vector_log1p":bits(reflog),"rust_pipeline_log1p":bits(log),"numpy_half":reference[position],"rust_half":rounded,"numpy_half_tie":tie(reflog),"rust_half_tie":tie(log)}));
        }
    }
    let rms = (squares / reference.len() as f64).sqrt();
    ensure!(
        Some(maximum) == input["expected_features"]["max_abs"].as_f64()
            && Some(rms) == input["expected_features"]["rms"].as_f64(),
        "Diagnostic scalar finalization did not reproduce frozen feature metrics"
    );
    ensure!(
        rows.len() == 6,
        "expected frozen six mismatches, observed {}",
        rows.len()
    );
    let output = json!({"format":"k10-cqt-first-failure-arithmetic-v1","diagnostic_only":true,"input_sha256":file_sha256(&args.input)?,"source_sha256":bytes_sha256(include_bytes!("arithmetic_probe.rs")),"executable_sha256":file_sha256(&std::env::current_exe()?)?,"normalization_differing_positions":normalization_differences,"magnitude_differing_positions":magnitude_differences,"libm_log1p_differences_on_reference_magnitude":log1p_differences_on_reference_magnitude,"source_defined_abs_differences":source_formula_differences,"fp16_mismatches":rows.len(),"max_feature_abs":maximum,"feature_rms":(squares/reference.len() as f64).sqrt(),"positions":rows});
    fs::write(args.output, serde_json::to_vec_pretty(&output)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finite_source_formula_handles_zero_and_exact_triangle() {
        assert_eq!(source_finite_abs(0.0, -0.0), 0.0);
        assert_eq!(source_finite_abs(3.0, -4.0), 5.0);
    }
    #[test]
    fn half_tie_preserves_nearest_even_boundary() {
        let midpoint = 1.00048828125f32;
        assert_eq!(tie(midpoint)["signed_distance"], 0.0);
        assert_eq!(half::f16::from_f32(midpoint).to_bits(), 0x3c00);
        assert_eq!(
            half::f16::from_f32(f32::from_bits(midpoint.to_bits() + 1)).to_bits(),
            0x3c01
        );
    }
}
