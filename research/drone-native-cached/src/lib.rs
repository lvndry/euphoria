//! Label-blind, CPU-only K10 cached-feature research harness.
//! No audio frontend, product detector, Python, or Torch integration.

use std::collections::HashSet;
use std::ffi::CStr;
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, ensure};
use ort::environment::Environment;
use ort::session::{
    Session,
    builder::{AutoDevicePolicy, GraphOptimizationLevel},
};
use ort::value::TensorRef;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const FEATURE_SPEC: &str = "cqt24-8oct-c1-pad4-hop4096-v2";
pub const LABEL_SCHEMA: &str = "tonic-times-two-plus-minor-v1";
pub const RUNTIME_VERSION: &str = "1.31.1";
pub const ATOL: f32 = 1e-5;
pub const RTOL: f32 = 1e-5;
pub const BANDS: usize = 192;
pub const FREQUENCIES: usize = 24;
pub const CORE_OUTPUTS: usize = 64;

pub fn bytes_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn file_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut digest = Sha256::new();
    let mut block = [0_u8; 65536];
    loop {
        let count = file.read(&mut block)?;
        if count == 0 {
            break;
        }
        digest.update(&block[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub fn check_sha256(value: &str) -> Result<()> {
    ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "Expected canonical lowercase SHA-256"
    );
    Ok(())
}

pub fn check_file(path: &Path, expected: &str) -> Result<()> {
    check_sha256(expected)?;
    ensure!(
        file_sha256(path)? == expected,
        "Changed artifact: {}",
        path.display()
    );
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub complete: bool,
    pub tracks: usize,
    pub selected_tracks: usize,
    pub format: String,
    pub feature_spec: String,
    pub label_schema: String,
    pub layout: String,
    pub dtype: String,
    pub bands: usize,
    pub full_model_sha256: String,
    pub checkpoint_sha256: String,
    pub reference_onnxruntime: String,
    pub validation_report_sha256: String,
    pub export_tool_sha256: String,
    pub rows: Vec<Fixture>,
}

#[derive(Debug, Deserialize)]
pub struct Fixture {
    pub id: String,
    pub domain: String,
    pub frames: usize,
    pub path: PathBuf,
    /// Digest of raw little-endian f32 bytes, distinct from original cache identity.
    pub sha256: String,
    pub audio_sha256: String,
    pub cache_sha256: String,
    /// May equal cache_sha256: both identify the source padded FP16 NPY file.
    pub source_sha256: String,
    pub reference_logits: [f32; 24],
    pub selection20: bool,
}

impl Manifest {
    pub fn load(path: &Path) -> Result<Self> {
        let result: Self = serde_json::from_reader(File::open(path)?)?;
        ensure!(
            result.format == "k10-core-f32le-v1"
                && result.feature_spec == FEATURE_SPEC
                && result.label_schema == LABEL_SCHEMA
                && result.layout == "bands,time"
                && result.dtype == "float32-le"
                && result.bands == BANDS
                && result.reference_onnxruntime == RUNTIME_VERSION,
            "Incompatible fixture contract"
        );
        for hash in [
            &result.full_model_sha256,
            &result.checkpoint_sha256,
            &result.validation_report_sha256,
            &result.export_tool_sha256,
        ] {
            check_sha256(hash)?;
        }
        ensure!(
            result.complete && result.tracks == 376 && result.selected_tracks == 20,
            "Require complete=true, tracks=376 and selected_tracks=20"
        );
        ensure!(
            result.rows.len() == result.tracks,
            "Require all 376 validation rows"
        );
        ensure!(
            result.rows.iter().filter(|r| r.selection20).count() == result.selected_tracks,
            "Require exactly 20 selected rows"
        );
        let mut identities = HashSet::new();
        let mut selected_audio = HashSet::new();
        for row in &result.rows {
            ensure!(row.frames >= 8, "Require T >= 8");
            ensure!(
                !row.id.is_empty() && !row.domain.is_empty(),
                "Empty identity"
            );
            ensure!(
                identities.insert((&row.domain, &row.id)),
                "Duplicate fixture identity"
            );
            ensure!(
                !row.path.as_os_str().is_empty()
                    && row
                        .path
                        .components()
                        .all(|p| matches!(p, Component::Normal(_))),
                "Fixture paths must be relative normal components"
            );
            for hash in [
                &row.sha256,
                &row.audio_sha256,
                &row.cache_sha256,
                &row.source_sha256,
            ] {
                check_sha256(hash)?;
            }
            ensure!(
                row.reference_logits.iter().all(|x| x.is_finite()),
                "Nonfinite reference"
            );
            if row.selection20 {
                ensure!(
                    selected_audio.insert(&row.audio_sha256),
                    "Duplicate selected audio"
                );
            }
        }
        Ok(result)
    }
}

pub fn load_features(root: &Path, fixture: &Fixture) -> Result<Vec<f32>> {
    // A manifest given as just its filename has an empty parent path.
    // Its feature paths are relative to the working directory.
    let root = if root.as_os_str().is_empty() {
        Path::new(".")
    } else {
        root
    };
    let root = std::fs::canonicalize(root)?;
    let path = std::fs::canonicalize(root.join(&fixture.path))?;
    ensure!(
        path.starts_with(&root),
        "Fixture symlink escapes manifest directory"
    );
    let count = BANDS
        .checked_mul(fixture.frames)
        .context("Feature dimensions overflow")?;
    let expected_bytes = count
        .checked_mul(4)
        .context("Feature dimensions overflow")?;
    let mut file = File::open(&path)?;
    ensure!(
        file.metadata()?.len() == u64::try_from(expected_bytes)?,
        "Wrong feature byte count"
    );
    // One initialized, correctly aligned allocation. All f32 bit patterns are
    // valid Rust values, and bytemuck's safe cast exposes exactly these bytes.
    let mut values = vec![0_f32; count];
    let bytes: &mut [u8] = bytemuck::cast_slice_mut(&mut values);
    file.read_exact(bytes)?;
    ensure!(
        bytes_sha256(bytes) == fixture.sha256,
        "Changed feature fixture"
    );
    ensure!(
        file.read(&mut [0_u8; 1])? == 0,
        "Feature grew while being read"
    );
    // Digest original LE bytes before host conversion.
    convert_le_values(&mut values, cfg!(target_endian = "big"));
    check_features(&values, fixture.frames)?;
    Ok(values)
}

fn convert_le_values(values: &mut [f32], swap: bool) {
    if swap {
        for value in values {
            *value = f32::from_bits(value.to_bits().swap_bytes());
        }
    }
}

/// Bind paired measurements to CPU capabilities, kernel and host identity.
pub fn hardware_identity() -> Result<serde_json::Value> {
    let mut info = std::mem::MaybeUninit::<libc::utsname>::uninit();
    // SAFETY: uname writes a complete utsname on success; checked before access.
    let status = unsafe { libc::uname(info.as_mut_ptr()) };
    ensure!(status == 0, "uname failed");
    // SAFETY: initialized by successful uname above.
    let info = unsafe { info.assume_init() };
    let text = |array: &[libc::c_char]| -> Result<String> {
        let bytes: Vec<_> = array.iter().map(|&x| x as u8).collect();
        Ok(CStr::from_bytes_until_nul(&bytes)?.to_str()?.to_owned())
    };
    Ok(serde_json::json!({
        "cpu":cpu_identity()?, "kernel_release":text(&info.release)?,
        "kernel_version":text(&info.version)?, "machine":text(&info.machine)?,
        "host_name_sha256":bytes_sha256(text(&info.nodename)?.as_bytes()),
        "machine_id_sha256":std::fs::read("/etc/machine-id").ok().map(|b| bytes_sha256(&b)),
    }))
}

#[cfg(target_os = "linux")]
fn cpu_identity() -> Result<serde_json::Value> {
    let source = std::fs::read_to_string("/proc/cpuinfo")?;
    let mut descriptions = std::collections::BTreeSet::new();
    for line in source.lines() {
        if let Some((key, value)) = line.split_once(':') {
            if [
                "vendor_id",
                "model name",
                "cpu family",
                "model",
                "stepping",
                "flags",
                "CPU implementer",
                "CPU architecture",
                "CPU variant",
                "CPU part",
                "CPU revision",
                "Features",
            ]
            .contains(&key.trim())
            {
                descriptions.insert(format!("{}:{}", key.trim(), value.trim()));
            }
        }
    }
    ensure!(!descriptions.is_empty(), "Missing CPU hardware identity");
    Ok(serde_json::json!(descriptions))
}

#[cfg(target_os = "macos")]
fn cpu_identity() -> Result<serde_json::Value> {
    let mut size = 0;
    // SAFETY: documented sysctl size query with valid static nul-terminated name.
    let status = unsafe {
        libc::sysctlbyname(
            c"machdep.cpu.brand_string".as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    ensure!(status == 0 && size > 0, "CPU model query failed");
    let mut bytes = vec![0_u8; size];
    // SAFETY: output buffer has the size returned by sysctl's size query.
    let status = unsafe {
        libc::sysctlbyname(
            c"machdep.cpu.brand_string".as_ptr(),
            bytes.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    ensure!(status == 0 && size <= bytes.len(), "CPU model read failed");
    Ok(serde_json::json!(
        CStr::from_bytes_until_nul(&bytes[..size])?.to_str()?
    ))
}

pub fn check_features(values: &[f32], frames: usize) -> Result<()> {
    ensure!(
        frames >= 8 && BANDS.checked_mul(frames) == Some(values.len()),
        "Expected [1,192,T], T>=8"
    );
    ensure!(values.iter().all(|x| x.is_finite()), "Nonfinite input");
    Ok(())
}

#[derive(Debug, Deserialize)]
pub struct Bundle {
    pub format: String,
    pub feature_spec: String,
    pub label_schema: String,
    pub class_order: Vec<String>,
    pub checkpoint_sha256: String,
    pub prefix_sha256: String,
    pub head_sha256: String,
    pub channels: usize,
    pub frequency_bins: usize,
    pub input_bands: usize,
    pub stride: usize,
    pub receptive_field: usize,
    pub halo_frames: usize,
    pub aggregation: String,
}

impl Bundle {
    pub fn load(root: &Path, checkpoint_sha256: &str) -> Result<Self> {
        let bundle: Self = serde_json::from_reader(File::open(root.join("contract.json"))?)?;
        ensure!(
            bundle.format == "k10-hidden-chunks-v1"
                && bundle.feature_spec == FEATURE_SPEC
                && bundle.label_schema == LABEL_SCHEMA
                && bundle.checkpoint_sha256 == checkpoint_sha256
                && bundle.frequency_bins == 24
                && bundle.input_bands == 192
                && bundle.channels == 64
                && bundle.stride == 8
                && bundle.receptive_field == 36
                && bundle.halo_frames == 16
                && bundle.aggregation == "hidden-fp64-mean-cast-fp32-head-once",
            "Incompatible K10 bundle"
        );
        let expected: Vec<_> = (0..24).map(key_name).collect::<Result<_>>()?;
        ensure!(bundle.class_order == expected, "Class order mismatch");
        check_file(&root.join("prefix.onnx"), &bundle.prefix_sha256)?;
        check_file(&root.join("head.onnx"), &bundle.head_sha256)?;
        Ok(bundle)
    }
}

/// NumPy-compatible first-index argmax, including exact ties and signed zero.
pub fn argmax(logits: &[f32; 24]) -> Result<usize> {
    ensure!(logits.iter().all(|x| x.is_finite()), "Nonfinite logits");
    let mut best = 0;
    for index in 1..24 {
        if logits[index] > logits[best] {
            best = index;
        }
    }
    Ok(best)
}

pub fn key_name(id: usize) -> Result<String> {
    ensure!(id < 24, "Invalid class ID");
    let tonic = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ][id / 2];
    Ok(format!(
        "{tonic} {}",
        if id.is_multiple_of(2) {
            "major"
        } else {
            "minor"
        }
    ))
}

#[derive(Debug, Serialize)]
pub struct Comparison {
    pub allclose: bool,
    pub argmax_agrees: bool,
    pub max_abs_error: f64,
    pub reference_margin: f64,
}

pub fn compare(actual: &[f32; 24], expected: &[f32; 24]) -> Result<Comparison> {
    let actual_key = argmax(actual)?;
    let expected_key = argmax(expected)?;
    let mut ordered = *expected;
    ordered.sort_by(f32::total_cmp);
    let differences: Vec<_> = actual
        .iter()
        .zip(expected)
        .map(|(&a, &e)| (f64::from(a) - f64::from(e)).abs())
        .collect();
    Ok(Comparison {
        allclose: differences
            .iter()
            .zip(expected)
            .all(|(&d, &e)| d <= f64::from(ATOL) + f64::from(RTOL) * f64::from(e).abs()),
        argmax_agrees: actual_key == expected_key,
        max_abs_error: differences.into_iter().fold(0.0, f64::max),
        reference_margin: f64::from(ordered[23]) - f64::from(ordered[22]),
    })
}

#[derive(Debug, PartialEq, Eq)]
pub struct Window {
    pub q0: usize,
    pub q1: usize,
    pub a: usize,
    pub b: usize,
}

pub fn windows(frames: usize, core: usize) -> Result<Vec<Window>> {
    ensure!(frames >= 8 && core > 0, "Require T>=8 and positive core");
    let outputs = frames / 8;
    let mut result = Vec::new();
    for q0 in (0..outputs).step_by(core) {
        let q1 = q0.saturating_add(core).min(outputs);
        result.push(Window {
            q0,
            q1,
            a: (q0 * 8).saturating_sub(16),
            b: (q1 * 8).saturating_add(16).min(frames),
        });
    }
    Ok(result)
}

pub fn slice_bands(input: &[f32], frames: usize, start: usize, end: usize) -> Vec<f32> {
    let mut result = Vec::with_capacity(BANDS * (end - start));
    for band in input.chunks_exact(frames) {
        result.extend_from_slice(&band[start..end]);
    }
    result
}

/// Load only the explicitly selected local CPU library, and reject version drift.
pub fn environment(library_path: &Path) -> Result<Environment> {
    // SAFETY: caller supplies a trusted native ORT shared library. The handle
    // remains alive until after the version string has been copied.
    let library = unsafe { libloading::Library::new(library_path) }?;
    // SAFETY: OrtGetApiBase is the public ABI-stable entry point with this signature.
    let getter: libloading::Symbol<unsafe extern "C" fn() -> *const ort::sys::OrtApiBase> =
        unsafe { library.get(b"OrtGetApiBase") }?;
    // SAFETY: calling the verified public ORT entry point; null checked below.
    let base = unsafe { getter() };
    ensure!(!base.is_null(), "Null ORT API base");
    // SAFETY: nonnull API base remains owned by the live library.
    let pointer = unsafe { ((*base).GetVersionString)() };
    ensure!(!pointer.is_null(), "Null ORT version string");
    // SAFETY: ORT documents a nul-terminated static version string.
    let version = unsafe { CStr::from_ptr(pointer) }.to_str()?.to_owned();
    ensure!(
        version == RUNTIME_VERSION,
        "Require ORT {RUNTIME_VERSION}, got {version}"
    );
    let environment = ort::init_from(library_path)?.build()?;
    Ok(environment)
}

fn session(environment: &Environment, path: &Path) -> Result<Session> {
    Ok(Session::builder(environment)?
        // CPU EP registration alone does not override ort's MaxEfficiency
        // default, which can silently select CoreML with reduced precision.
        .with_auto_device(AutoDevicePolicy::PreferCPU)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .with_execution_providers([ort::ep::CPU::default().with_arena_allocator(true).build()])
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .with_intra_threads(1)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .with_inter_threads(1)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .with_parallel_execution(false)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .with_intra_op_spinning(false)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .with_inter_op_spinning(false)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .commit_from_file(path)?)
}

fn run(
    session: &mut Session,
    input_name: &str,
    output_name: &str,
    shape: [usize; 4],
    input: &[f32],
    rank3: bool,
) -> Result<(Vec<i64>, Vec<f32>)> {
    let dimensions: Vec<_> = if rank3 {
        vec![1, BANDS, shape[3]]
    } else {
        shape.to_vec()
    };
    let tensor = TensorRef::from_array_view((dimensions, input))?;
    let outputs = session.run(ort::inputs![input_name => tensor])?;
    let (shape, values) = outputs
        .get(output_name)
        .context("Missing expected output")?
        .try_extract_tensor::<f32>()?;
    ensure!(
        values.iter().all(|x| x.is_finite()),
        "Nonfinite runtime output"
    );
    Ok((shape.to_vec(), values.to_vec()))
}

pub struct Full {
    session: Session,
}

impl Full {
    pub fn new(environment: &Environment, path: &Path) -> Result<Self> {
        Ok(Self {
            session: session(environment, path)?,
        })
    }

    pub fn predict(&mut self, input: &[f32], frames: usize) -> Result<[f32; 24]> {
        check_features(input, frames)?;
        let (shape, values) = run(
            &mut self.session,
            "features",
            "logits",
            [1, 1, BANDS, frames],
            input,
            true,
        )?;
        ensure!(shape == [1, 24], "Unexpected full output shape");
        values
            .try_into()
            .map_err(|_| anyhow::anyhow!("Expected 24 logits"))
    }
}

pub struct Chunked {
    prefix: Session,
    head: Session,
    channels: usize,
}

impl Chunked {
    pub fn new(environment: &Environment, root: &Path, bundle: &Bundle) -> Result<Self> {
        Ok(Self {
            prefix: session(environment, &root.join("prefix.onnx"))?,
            head: session(environment, &root.join("head.onnx"))?,
            channels: bundle.channels,
        })
    }

    pub fn predict(&mut self, input: &[f32], frames: usize) -> Result<[f32; 24]> {
        check_features(input, frames)?;
        let mut total = vec![0_f64; self.channels * FREQUENCIES];
        let mut count = 0;
        for window in windows(frames, CORE_OUTPUTS)? {
            let local_frames = window.b - window.a;
            let local = slice_bands(input, frames, window.a, window.b);
            let (shape, values) = run(
                &mut self.prefix,
                "features",
                "hidden",
                [1, 1, BANDS, local_frames],
                &local,
                true,
            )?;
            let q = local_frames / 8;
            ensure!(
                shape == [1, self.channels as i64, 24, q as i64],
                "Unexpected hidden shape"
            );
            let start = window.q0 - window.a / 8;
            let end = window.q1 - window.a / 8;
            ensure!(end <= q, "Incomplete chunk core");
            for (accumulator, band) in total.iter_mut().zip(values.chunks_exact(q)) {
                // Sum each chunk separately in FP64, then add, as Python does.
                let core_sum: f64 = band[start..end].iter().map(|&x| f64::from(x)).sum();
                *accumulator += core_sum;
            }
            count += end - start;
        }
        ensure!(count == frames / 8, "Incomplete output coverage");
        let pooled: Vec<_> = total
            .into_iter()
            .map(|x| (x / count as f64) as f32)
            .collect();
        let (shape, values) = run(
            &mut self.head,
            "pooled",
            "logits",
            [1, self.channels, 24, 1],
            &pooled,
            false,
        )?;
        ensure!(shape == [1, 24], "Unexpected head output shape");
        values
            .try_into()
            .map_err(|_| anyhow::anyhow!("Expected 24 logits"))
    }
}

/// Individual process CPU consumption and maximum resident memory, not system RAM.
pub fn process_usage() -> Result<(f64, u64)> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage writes the complete struct on success; checked before access.
    let status = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    ensure!(status == 0, "getrusage failed");
    // SAFETY: initialized by the successful getrusage call above.
    let usage = unsafe { usage.assume_init() };
    let seconds = usage.ru_utime.tv_sec as f64
        + usage.ru_utime.tv_usec as f64 / 1e6
        + usage.ru_stime.tv_sec as f64
        + usage.ru_stime.tv_usec as f64 / 1e6;
    let rss = u64::try_from(usage.ru_maxrss)?;
    Ok((
        seconds,
        if cfg!(target_os = "macos") {
            rss / 1024
        } else {
            rss
        },
    ))
}

/// OS-enforced CPU ceiling for this process; caller also supplies an outer timeout.
pub fn limit_cpu() -> Result<()> {
    let current = process_usage()?.0.ceil() as libc::rlim_t;
    let limit = libc::rlimit {
        rlim_cur: current + 600,
        rlim_max: current + 601,
    };
    // SAFETY: a valid, live rlimit struct with RLimit CPU's documented units.
    let status = unsafe { libc::setrlimit(libc::RLIMIT_CPU, &limit) };
    ensure!(status == 0, "setrlimit failed");
    Ok(())
}

pub fn median(values: &[f64]) -> Result<f64> {
    ensure!(
        !values.is_empty() && values.iter().all(|x| x.is_finite()),
        "Invalid measurements"
    );
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    Ok(if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_order_conversion_preserves_exact_bits_and_signed_zero() {
        let expected = [
            1.0_f32,
            -0.0,
            -13.25,
            f32::INFINITY,
            f32::from_bits(0x7fc0_0123),
        ];
        let mut native = expected;
        convert_le_values(&mut native, false);
        assert_eq!(native.map(f32::to_bits), expected.map(f32::to_bits));
        let mut swapped = expected.map(|x| f32::from_bits(x.to_bits().swap_bytes()));
        convert_le_values(&mut swapped, true);
        assert_eq!(swapped.map(f32::to_bits), expected.map(f32::to_bits));
    }

    #[test]
    fn hardware_metadata_contains_cpu_kernel_and_machine_identity() -> Result<()> {
        let hardware = hardware_identity()?;
        assert!(!hardware["cpu"].is_null());
        assert!(
            hardware["kernel_release"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
        );
        assert!(
            hardware["kernel_version"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
        );
        check_sha256(
            hardware["host_name_sha256"]
                .as_str()
                .context("host identity")?,
        )?;
        Ok(())
    }
}
