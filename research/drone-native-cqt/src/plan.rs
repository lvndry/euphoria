//! Executes captured complex64 CSR kernels; constants never regenerated.
use super::read_f32;
use anyhow::{Context, Result, ensure};
use drone_native_cached::check_file;
use rustfft::{FftPlanner, num_complex::Complex};
use serde_json::{Value, json};
use std::{fs, path::Path};

pub struct Soxr {
    library: libloading::Library,
}
impl Soxr {
    pub fn new(path: &Path) -> Result<Self> {
        // SAFETY: explicit research wrapper path; library retained for all calls.
        let library = unsafe { libloading::Library::new(path) }?;
        Ok(Self { library })
    }
    fn halve(&self, input: &[f32]) -> Result<Vec<f32>> {
        type Halve = unsafe extern "C" fn(*const f32, usize, *mut f32, usize, *mut usize) -> i32;
        let mut output = vec![0.0; input.len() / 2 + 1];
        let mut written = 0;
        // SAFETY: cqt_halve ABI is defined in adjacent soxr_split.c. Buffers have
        // valid pointer/length pairs; symbol cannot outlive retained library.
        let result = unsafe {
            let function = self.library.get::<Halve>(b"cqt_halve\0")?;
            function(
                input.as_ptr(),
                input.len(),
                output.as_mut_ptr(),
                output.len(),
                &mut written,
            )
        };
        ensure!(
            result == 0 && written <= output.len(),
            "soxr split wrapper failed {result}"
        );
        output.truncate(written);
        output.resize(input.len().div_ceil(2), 0.0);
        // librosa scale=True: divide float32 output by the float64 sqrt ratio.
        for value in &mut output {
            *value = (f64::from(*value) / 0.5_f64.sqrt()) as f32;
        }
        Ok(output)
    }
}
fn bytes(root: &Path, artifact: &Value) -> Result<Vec<u8>> {
    let path = root
        .join(artifact["path"].as_str().context("artifact path")?)
        .canonicalize()?;
    ensure!(path.starts_with(root.canonicalize()?), "artifact escape");
    check_file(&path, artifact["sha256"].as_str().context("hash")?)?;
    Ok(fs::read(path)?)
}
fn indices(root: &Path, artifact: &Value) -> Result<Vec<usize>> {
    let values = bytes(root, artifact)?;
    ensure!(values.len() % 4 == 0, "indices length");
    Ok(values
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
        .collect())
}
fn complex_values(root: &Path, artifact: &Value) -> Result<Vec<Complex<f32>>> {
    let values = bytes(root, artifact)?;
    ensure!(values.len() % 8 == 0, "complex length");
    Ok(values
        .as_chunks::<8>()
        .0
        .iter()
        .map(|b| {
            Complex::new(
                f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                f32::from_le_bytes([b[4], b[5], b[6], b[7]]),
            )
        })
        .collect())
}
fn observed_error(root: &Path, actual: &[f32], observation: &Value) -> Result<Value> {
    if observation.get("path").is_some() {
        let expected = read_f32(root, observation)?;
        return super::diagnostics(actual, &expected);
    }
    let raw: Vec<u8> = actual
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect();
    let exact = drone_native_cached::bytes_sha256(&raw)
        == observation["sha256"].as_str().context("observation hash")?;
    let mut probes = Vec::new();
    let shape = observation["shape"].as_array().context("shape")?;
    let columns = shape.last().and_then(Value::as_u64).context("columns")? as usize;
    let components = if observation["dtype"].as_str() == Some("<c8") {
        2
    } else {
        1
    };
    let outer = actual.len() / (columns * components);
    for probe in observation["probes"].as_array().context("probes")? {
        let start = probe["start"].as_u64().context("probe start")? as usize;
        let reference = read_f32(root, &probe["artifact"])?;
        let width = reference.len() / (outer * components);
        ensure!(start + width <= columns, "probe outside observation");
        let mut selected = Vec::with_capacity(reference.len());
        for row in 0..outer {
            selected.extend_from_slice(
                &actual[(row * columns + start) * components
                    ..(row * columns + start + width) * components],
            );
        }
        probes.push(json!({"start":start,"diagnostics":super::diagnostics(&selected,&reference)?}));
    }
    Ok(json!({"full_hash_equal":exact,"probes":probes}))
}

pub fn features(
    root: &Path,
    case: &Value,
    pcm: &[f32],
    soxr: &Soxr,
) -> Result<(Vec<f32>, usize, Value)> {
    let stages = case["stages"].as_array().context("stages")?;
    let resampling = case["resamplings"].as_array().context("resamples")?;
    let mut samples = pcm.to_vec();
    let mut responses: Vec<Vec<Complex<f32>>> = Vec::new();
    let mut diagnostics = Vec::new();
    let mut planner = FftPlanner::<f64>::new();
    for (stage_index, stage) in stages.iter().enumerate() {
        let n = stage["n_fft"].as_u64().context("n_fft")? as usize;
        let hop = stage["hop"].as_u64().context("hop")? as usize;
        let rows = stage["basis_shape"][0].as_u64().context("rows")? as usize;
        let indptr = indices(root, &stage["indptr"])?;
        let indices = indices(root, &stage["indices"])?;
        let basis = complex_values(root, &stage["basis"])?;
        ensure!(
            indptr.len() == rows + 1 && basis.len() == indices.len(),
            "CSR dimensions"
        );
        let frames = 1 + samples.len() / hop;
        let mut response = vec![Complex::new(0.0f32, 0.0); rows * frames];
        let fft = planner.plan_fft_forward(n);
        let mut buffer = vec![Complex::new(0.0, 0.0); n];
        for frame in 0..frames {
            for (offset, value) in buffer.iter_mut().enumerate() {
                let position = frame * hop + offset;
                *value = Complex::new(
                    if position >= n / 2 {
                        samples
                            .get(position - n / 2)
                            .copied()
                            .map(f64::from)
                            .unwrap_or(0.0)
                    } else {
                        0.0
                    },
                    0.0,
                );
            }
            fft.process(&mut buffer);
            let spectrum: Vec<Complex<f32>> = buffer[..=n / 2]
                .iter()
                .map(|v| Complex::new(v.re as f32, v.im as f32))
                .collect();
            for row in 0..rows {
                let mut sum = Complex::new(0.0f32, 0.0);
                for index in indptr[row]..indptr[row + 1] {
                    ensure!(indices[index] < spectrum.len(), "CSR index");
                    sum += basis[index] * spectrum[indices[index]];
                }
                response[row * frames + frame] = sum;
            }
        }
        // Top-to-bottom stages: each stage prepends its rows, final lowest
        // partial octave keeps its highest rows (librosa __trim_stack).
        let response_components: Vec<f32> = response.iter().flat_map(|v| [v.re, v.im]).collect();
        diagnostics.push(json!({"stage":stage_index,"response":observed_error(root,&response_components,&stage["response"])?}));
        let mut stage_rows = Vec::new();
        for row in 0..rows {
            stage_rows.push(response[row * frames..(row + 1) * frames].to_vec());
        }
        stage_rows.extend(responses);
        responses = stage_rows;
        if let Some(resample) = resampling
            .iter()
            .find(|r| r["after_stage"].as_u64() == Some(stage_index as u64))
        {
            samples = soxr.halve(&samples)?;
            diagnostics.push(json!({"after_stage":stage_index,"resample":observed_error(root,&samples,&resample["output"])?}));
        }
    }
    ensure!(responses.len() >= 208, "response bins");
    if responses.len() > 208 {
        responses.drain(0..responses.len() - 208);
    }
    let frames = responses.iter().map(Vec::len).min().context("responses")?;
    let lengths = case["normalization"]["lengths"]
        .as_array()
        .context("lengths")?;
    let mut core = Vec::with_capacity(192 * frames);
    for bin in 8..200 {
        let scale = lengths[bin].as_f64().context("length")?.sqrt();
        for value in &responses[bin][..frames] {
            let re = (f64::from(value.re) / scale) as f32;
            let im = (f64::from(value.im) / scale) as f32;
            let logged = re.hypot(im).ln_1p();
            core.push(half::f16::from_f32(logged).to_f32());
        }
    }
    Ok((core, frames, json!({"resamplings":diagnostics})))
}
