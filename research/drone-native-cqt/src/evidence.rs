// Copyright 2026 The Euphoria contributors
// SPDX-License-Identifier: GPL-3.0-or-later
use anyhow::{Context, Result, ensure};
use drone_native_cached::check_file;
use serde_json::{Value, json};
use std::{fs, path::Path};
pub fn read_f32(root: &Path, artifact: &Value) -> Result<Vec<f32>> {
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
pub fn diagnostics(actual: &[f32], reference: &[f32]) -> Result<Value> {
    ensure!(!actual.is_empty(), "empty features");
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
pub fn artifact_root(manifest: &Path) -> Result<&Path> {
    let root = manifest.parent().context("golden root")?;
    Ok(if root.as_os_str().is_empty() {
        Path::new(".")
    } else {
        root
    })
}
