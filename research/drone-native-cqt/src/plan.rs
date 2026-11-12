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
/// Explicit host-specific research math library; never a portable fallback.
pub struct NumpyMath {
    library: libloading::Library,
}
impl NumpyMath {
    pub fn new(path: &Path) -> Result<Self> {
        // SAFETY: caller-selected pinned research library; retained for calls.
        let library = unsafe { libloading::Library::new(path) }?;
        let result = Self { library };
        result.log1p(&[])?;
        Ok(result)
    }
    pub fn log1p(&self, input: &[f32]) -> Result<Vec<f32>> {
        type Log1p = unsafe extern "C" fn(*const f32, *mut f32, usize) -> i32;
        let mut output = vec![0.0; input.len()];
        // SAFETY: adjacent svml_wrapper.c defines this ABI and checks CPU/OS
        // support before any AVX512 execution. Buffers have matching lengths;
        // zero length is valid and performs only the feature guard.
        let status = unsafe {
            let function = self.library.get::<Log1p>(b"cqt_svml_log1p\0")?;
            function(input.as_ptr(), output.as_mut_ptr(), input.len())
        };
        ensure!(
            status == 0,
            "Pinned SVML math rejected CPU or input: status {status}"
        );
        ensure!(
            output.iter().all(|v| v.is_finite()),
            "nonfinite SVML output"
        );
        Ok(output)
    }
}
fn numpy_finite_magnitude(re: f32, im: f32) -> Result<f32> {
    ensure!(
        re.is_finite() && im.is_finite(),
        "nonfinite normalized coefficient"
    );
    let larger = re.abs().max(im.abs());
    let smaller = re.abs().min(im.abs());
    if larger == 0.0 {
        return Ok(0.0);
    }
    let ratio = smaller / larger;
    Ok(ratio.mul_add(ratio, 1.0).sqrt() * larger)
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
    let expected_values = shape
        .iter()
        .try_fold(components, |count: usize, dimension| {
            usize::try_from(dimension.as_u64().context("observation dimension")?)
                .ok()
                .and_then(|dimension| count.checked_mul(dimension))
                .context("observation size overflow")
        })?;
    ensure!(
        columns > 0 && expected_values == actual.len() && !actual.is_empty(),
        "observation shape mismatch"
    );
    let outer = actual.len() / (columns * components);
    for probe in observation["probes"].as_array().context("probes")? {
        let start = probe["start"].as_u64().context("probe start")? as usize;
        let reference = read_f32(root, &probe["artifact"])?;
        ensure!(
            reference.len().is_multiple_of(outer * components),
            "probe shape mismatch"
        );
        let width = reference.len() / (outer * components);
        ensure!(
            start.checked_add(width).is_some_and(|end| end <= columns),
            "probe outside observation"
        );
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
    math: Option<&NumpyMath>,
    numpy_magnitude: bool,
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
        ensure!(n > 0 && hop > 0, "FFT size and hop must be positive");
        let indptr = indices(root, &stage["indptr"])?;
        let indices = indices(root, &stage["indices"])?;
        let basis = complex_values(root, &stage["basis"])?;
        ensure!(
            rows.checked_add(1) == Some(indptr.len()) && basis.len() == indices.len(),
            "CSR dimensions"
        );
        ensure!(
            indptr.first() == Some(&0)
                && indptr.last() == Some(&basis.len())
                && indptr.windows(2).all(|pair| pair[0] <= pair[1]),
            "invalid CSR row pointers"
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
    ensure!(
        lengths.len() == 208
            && lengths.iter().all(|length| length
                .as_f64()
                .is_some_and(|value| value.is_finite() && value > 0.0)),
        "normalization requires 208 positive finite lengths"
    );
    let mut magnitudes = Vec::with_capacity(192 * frames);
    for bin in 8..200 {
        let scale = lengths[bin].as_f64().context("length")?.sqrt();
        for value in &responses[bin][..frames] {
            let re = (f64::from(value.re) / scale) as f32;
            let im = (f64::from(value.im) / scale) as f32;
            magnitudes.push(if numpy_magnitude || math.is_some() {
                numpy_finite_magnitude(re, im)?
            } else {
                re.hypot(im)
            });
        }
    }
    let logged = if let Some(math) = math {
        math.log1p(&magnitudes)?
    } else {
        magnitudes.iter().map(|value| value.ln_1p()).collect()
    };
    let core = logged
        .into_iter()
        .map(|value| half::f16::from_f32(value).to_f32())
        .collect();
    Ok((core, frames, json!({"resamplings":diagnostics})))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    struct SyntheticCase {
        root: std::path::PathBuf,
        case: Value,
    }
    #[cfg(unix)]
    impl SyntheticCase {
        fn new() -> Result<Self> {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "cqt-plan-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root)?;
            let mut result = Self {
                root,
                case: json!({}),
            };
            let indptr = result.artifact(
                "indptr",
                &(0u32..=208).flat_map(u32::to_le_bytes).collect::<Vec<_>>(),
            )?;
            let indices = result.artifact("indices", &[0u8; 208 * 4])?;
            let basis = result.artifact(
                "basis",
                &(0..208)
                    .flat_map(|_| [1.0f32, 0.0])
                    .flat_map(f32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )?;
            let response = result.artifact(
                "response",
                &(0..208)
                    .flat_map(|_| [1.0f32, 0.0, 1.0, 0.0, 0.0, 0.0])
                    .flat_map(f32::to_le_bytes)
                    .collect::<Vec<_>>(),
            )?;
            result.case = json!({"stages":[{"n_fft":2,"hop":1,"basis_shape":[208,2],"indptr":indptr,"indices":indices,"basis":basis,"response":response}],"resamplings":[],"normalization":{"lengths":vec![1.0;208]}});
            Ok(result)
        }
        fn artifact(&self, name: &str, bytes: &[u8]) -> Result<Value> {
            fs::write(self.root.join(name), bytes)?;
            Ok(
                json!({"path":name,"sha256":drone_native_cached::bytes_sha256(bytes),"bytes":bytes.len()}),
            )
        }
        fn run(&self) -> Result<(Vec<f32>, usize, Value)> {
            // No resampling is requested, so no symbols from this handle are called.
            let soxr = Soxr {
                library: libloading::os::unix::Library::this().into(),
            };
            features(&self.root, &self.case, &[1.0, 0.0], &soxr, None, false)
        }
    }
    #[cfg(unix)]
    impl Drop for SyntheticCase {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    #[cfg(unix)]
    fn synthetic_centered_impulse_preserves_frames_and_fp16_boundary() -> Result<()> {
        let fixture = SyntheticCase::new()?;
        let (core, frames, _) = fixture.run()?;
        ensure!(frames == 3 && core.len() == 192 * frames);
        let expected = half::f16::from_f32(2.0f32.ln()).to_f32();
        ensure!(
            core.as_chunks::<3>()
                .0
                .iter()
                .all(|row| *row == [expected, expected, 0.0])
        );
        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn malformed_captured_plans_return_errors_instead_of_panicking() -> Result<()> {
        let mut accepted_or_panicked = Vec::new();
        for invalid in ["hop", "fft", "csr", "lengths", "scale", "observation"] {
            let mut fixture = SyntheticCase::new()?;
            match invalid {
                "hop" => fixture.case["stages"][0]["hop"] = json!(0),
                "fft" => fixture.case["stages"][0]["n_fft"] = json!(0),
                "csr" => {
                    let ptr = (0u32..208)
                        .chain([209])
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>();
                    fixture.case["stages"][0]["indptr"] = fixture.artifact("bad-pointers", &ptr)?;
                }
                "lengths" => fixture.case["normalization"]["lengths"] = json!(vec![1.0; 199]),
                "scale" => fixture.case["normalization"]["lengths"][8] = json!(0),
                "observation" => {
                    fixture.case["stages"][0]["response"] =
                        json!({"sha256":"0".repeat(64),"shape":[208,0],"dtype":"<c8","probes":[]})
                }
                _ => unreachable!(),
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fixture.run()));
            if !matches!(result, Ok(Err(_))) {
                accepted_or_panicked.push(invalid);
            }
        }
        ensure!(
            accepted_or_panicked.is_empty(),
            "malformed plans did not return errors: {accepted_or_panicked:?}"
        );
        Ok(())
    }
    #[test]
    fn finite_numpy_magnitude_preserves_contract() -> Result<()> {
        ensure!(numpy_finite_magnitude(0.0, -0.0)?.to_bits() == 0);
        ensure!(numpy_finite_magnitude(-3.0, 4.0)? == 5.0);
        ensure!(numpy_finite_magnitude(f32::NAN, 1.0).is_err());
        ensure!(numpy_finite_magnitude(f32::INFINITY, 0.0).is_err());
        Ok(())
    }
}
