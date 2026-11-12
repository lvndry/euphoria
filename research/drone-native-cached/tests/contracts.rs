use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Result;
use drone_native_cached::{
    ATOL, BANDS, Bundle, Chunked, Fixture, Full, Manifest, argmax, bytes_sha256, check_features,
    check_file, compare, environment, key_name, load_features, slice_bands, windows,
};
use serde_json::json;

static SEQUENCE: AtomicUsize = AtomicUsize::new(0);

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!(
            "drone-native-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn output_coverage_preserves_phase_tail_and_receptive_fields() -> Result<()> {
    for core in [1, 2, 7, 64] {
        for frames in 8..1104 {
            let mut coverage = Vec::new();
            for w in windows(frames, core)? {
                assert_eq!(w.a % 8, 0);
                assert!(w.b - w.a <= core * 8 + 32);
                assert!(w.q1 - w.a / 8 <= (w.b - w.a) / 8);
                for q in w.q0..w.q1 {
                    assert!(w.a == 0 || w.a + 14 <= 8 * q);
                    assert!(w.b == frames || w.b > 8 * q + 21);
                    coverage.push(q);
                }
            }
            assert_eq!(coverage, (0..frames / 8).collect::<Vec<_>>());
        }
    }
    assert!(windows(7, 64).is_err());
    assert!(windows(8, 0).is_err());
    Ok(())
}

#[test]
fn mapping_ties_and_near_tie_gate_match_numpy() -> Result<()> {
    assert_eq!(key_name(0)?, "C major");
    assert_eq!(key_name(1)?, "C minor");
    assert_eq!(key_name(18)?, "A major");
    assert_eq!(key_name(19)?, "A minor");
    assert_eq!(key_name(23)?, "B minor");
    assert!(key_name(24).is_err());
    let mut logits = [-0.0; 24];
    logits[23] = 0.0;
    assert_eq!(argmax(&logits)?, 0);
    logits[7] = 2.0;
    logits[8] = 2.0;
    assert_eq!(argmax(&logits)?, 7);
    let baseline = [0_f32; 24];
    let mut changed = baseline;
    changed[1] = ATOL / 4.0;
    let comparison = compare(&changed, &baseline)?;
    assert!(comparison.allclose);
    assert!(!comparison.argmax_agrees);
    changed[0] = f32::NAN;
    assert!(argmax(&changed).is_err());
    assert!(compare(&changed, &baseline).is_err());
    Ok(())
}

#[test]
fn band_slicing_is_not_a_contiguous_time_major_slice() {
    let frames = 17;
    let input: Vec<_> = (0..BANDS * frames).map(|n| n as f32).collect();
    let output = slice_bands(&input, frames, 3, 11);
    assert_eq!(output.len(), BANDS * 8);
    for band in 0..BANDS {
        assert_eq!(
            &output[band * 8..(band + 1) * 8],
            &input[band * frames + 3..band * frames + 11]
        );
    }
}

#[test]
fn raw_little_endian_fixtures_are_finite_and_integrity_checked() -> Result<()> {
    let scratch = Scratch::new()?;
    let values: Vec<_> = (0..BANDS * 8).map(|n| n as f32 - 13.0).collect();
    let bytes: Vec<_> = values.iter().flat_map(|n| n.to_le_bytes()).collect();
    let path = scratch.0.join("sample.f32");
    fs::write(&path, &bytes)?;
    let mut fixture = Fixture {
        id: "synthetic".to_owned(),
        domain: "test".to_owned(),
        frames: 8,
        path: PathBuf::from("sample.f32"),
        sha256: bytes_sha256(&bytes),
        audio_sha256: "0".repeat(64),
        cache_sha256: "1".repeat(64),
        source_sha256: "1".repeat(64),
        reference_logits: [0.0; 24],
        selection20: false,
    };
    assert_eq!(load_features(&scratch.0, &fixture)?, values);
    assert!(check_file(&path, &"0".repeat(64)).is_err());
    let mut changed = bytes.clone();
    changed[0] ^= 1;
    fs::write(&path, &changed)?;
    let error = load_features(&scratch.0, &fixture).expect_err("changed fixture must fail");
    assert!(error.to_string().contains("Changed feature fixture"));
    fs::write(&path, &bytes)?;
    fixture.frames = 9;
    assert!(load_features(&scratch.0, &fixture).is_err());
    fixture.frames = 8;
    let mut nonfinite = values.clone();
    nonfinite[0] = f32::NAN;
    let nonfinite_bytes: Vec<_> = nonfinite.iter().flat_map(|x| x.to_le_bytes()).collect();
    fs::write(&path, &nonfinite_bytes)?;
    fixture.sha256 = bytes_sha256(&nonfinite_bytes);
    assert!(load_features(&scratch.0, &fixture).is_err());
    assert!(check_features(&vec![0.0; BANDS * 7], 7).is_err());
    assert!(check_features(&vec![f32::INFINITY; BANDS * 8], 8).is_err());
    Ok(())
}

#[test]
fn feature_symlinks_must_resolve_inside_the_fixture_root() -> Result<()> {
    let scratch = Scratch::new()?;
    let outside = Scratch::new()?;
    let bytes = vec![0_u8; BANDS * 8 * 4];
    fs::write(outside.0.join("outside.f32"), &bytes)?;
    fs::write(scratch.0.join("inside.f32"), &bytes)?;
    std::os::unix::fs::symlink(outside.0.join("outside.f32"), scratch.0.join("escape.f32"))?;
    std::os::unix::fs::symlink(scratch.0.join("inside.f32"), scratch.0.join("internal.f32"))?;
    let mut fixture = Fixture {
        id: "synthetic".to_owned(),
        domain: "test".to_owned(),
        frames: 8,
        path: PathBuf::from("escape.f32"),
        sha256: bytes_sha256(&bytes),
        audio_sha256: "0".repeat(64),
        cache_sha256: "1".repeat(64),
        source_sha256: "1".repeat(64),
        reference_logits: [0.0; 24],
        selection20: false,
    };
    let error = load_features(&scratch.0, &fixture).expect_err("symlink escape must fail");
    assert!(error.to_string().contains("escapes"));
    fixture.path = PathBuf::from("internal.f32");
    assert_eq!(load_features(&scratch.0, &fixture)?, vec![0.0; BANDS * 8]);
    Ok(())
}

#[test]
fn manifest_requires_unique_complete_public_validation_and_exact_version() -> Result<()> {
    let scratch = Scratch::new()?;
    let path = scratch.0.join("manifest.json");
    let mut manifest = json!({
        "complete":true, "tracks":376, "selected_tracks":20,
        "format":"k10-core-f32le-v1", "feature_spec":drone_native_cached::FEATURE_SPEC,
        "label_schema":drone_native_cached::LABEL_SCHEMA, "layout":"bands,time",
        "dtype":"float32-le", "bands":192, "full_model_sha256":"0".repeat(64),
        "checkpoint_sha256":"1".repeat(64), "reference_onnxruntime":"1.31.1",
        "validation_report_sha256":"2".repeat(64), "export_tool_sha256":"3".repeat(64),
        "rows":(0..376).map(|id| json!({
            "id":id.to_string(), "domain":"synthetic", "frames":8,
            "path":format!("{id}.f32"), "sha256":"0".repeat(64),
            "audio_sha256":format!("{id:064x}"), "cache_sha256":"1".repeat(64),
            "source_sha256":"1".repeat(64), "reference_logits":vec![0.0;24], "selection20":id<20,
        })).collect::<Vec<_>>()
    });
    fs::write(&path, serde_json::to_vec(&manifest)?)?;
    assert!(Manifest::load(&path).is_ok());
    for (field, wrong) in [
        ("complete", json!(false)),
        ("tracks", json!(375)),
        ("selected_tracks", json!(19)),
    ] {
        let mut invalid = manifest.clone();
        invalid[field] = wrong;
        fs::write(&path, serde_json::to_vec(&invalid)?)?;
        assert!(Manifest::load(&path).is_err());
        invalid
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("object"))?
            .remove(field);
        fs::write(&path, serde_json::to_vec(&invalid)?)?;
        assert!(Manifest::load(&path).is_err());
    }
    manifest["reference_onnxruntime"] = json!("1.31.2");
    fs::write(&path, serde_json::to_vec(&manifest)?)?;
    assert!(Manifest::load(&path).is_err());
    manifest["reference_onnxruntime"] = json!("1.31.1");
    manifest["rows"][1]["id"] = json!("0");
    fs::write(&path, serde_json::to_vec(&manifest)?)?;
    assert!(Manifest::load(&path).is_err());
    Ok(())
}

#[test]
fn bundle_rejects_changed_weights_class_order_and_checkpoint() -> Result<()> {
    let scratch = Scratch::new()?;
    fs::write(scratch.0.join("prefix.onnx"), b"synthetic-prefix")?;
    fs::write(scratch.0.join("head.onnx"), b"synthetic-head")?;
    let mut metadata = json!({
        "format":"k10-hidden-chunks-v1", "feature_spec":drone_native_cached::FEATURE_SPEC,
        "label_schema":drone_native_cached::LABEL_SCHEMA,
        "checkpoint_sha256":"1".repeat(64),
        "prefix_sha256":bytes_sha256(b"synthetic-prefix"),
        "head_sha256":bytes_sha256(b"synthetic-head"),
        "class_order":(0..24).map(key_name).collect::<Result<Vec<_>>>()?,
        "channels":64, "frequency_bins":24, "input_bands":192,
        "stride":8, "receptive_field":36, "halo_frames":16,
        "aggregation":"hidden-fp64-mean-cast-fp32-head-once",
    });
    let path = scratch.0.join("contract.json");
    fs::write(&path, serde_json::to_vec(&metadata)?)?;
    assert!(Bundle::load(&scratch.0, &"1".repeat(64)).is_ok());
    assert!(Bundle::load(&scratch.0, &"2".repeat(64)).is_err());
    metadata["class_order"][0] = json!("C minor");
    fs::write(&path, serde_json::to_vec(&metadata)?)?;
    assert!(Bundle::load(&scratch.0, &"1".repeat(64)).is_err());
    metadata["class_order"][0] = json!("C major");
    fs::write(&path, serde_json::to_vec(&metadata)?)?;
    fs::write(scratch.0.join("head.onnx"), b"changed-weights")?;
    assert!(Bundle::load(&scratch.0, &"1".repeat(64)).is_err());
    Ok(())
}

/// Run explicitly with DRONE_ORT_LIBRARY, DRONE_BUNDLE and DRONE_FULL_MODEL.
/// Uses synthetic inputs only and never launches a Python/Torch subprocess.
#[test]
#[ignore = "requires explicit pinned ORT library and unchanged model bundle"]
fn pinned_runtime_full_chunk_edges_stress_and_all_remainders() -> Result<()> {
    let library = PathBuf::from(std::env::var("DRONE_ORT_LIBRARY")?);
    let root = PathBuf::from(std::env::var("DRONE_BUNDLE")?);
    let full_path = PathBuf::from(std::env::var("DRONE_FULL_MODEL")?);
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("contract.json"))?)?;
    let checkpoint = metadata["checkpoint_sha256"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("checkpoint hash"))?;
    let bundle = Bundle::load(&root, checkpoint)?;
    let environment = environment(&library)?;
    let mut full = Full::new(&environment, &full_path)?;
    let mut chunked = Chunked::new(&environment, &root, &bundle)?;
    for remainder in 0..8 {
        for frames in [
            8 + remainder,
            512 + remainder,
            520 + remainder,
            1032 + remainder,
        ] {
            for kind in 0..5 {
                let mut input = vec![0_f32; BANDS * frames];
                for band in 0..BANDS {
                    for t in 0..frames {
                        let value = (((band * 17 + t * 31 + 13) % 103) as f32 / 51.0) - 1.0;
                        input[band * frames + t] = match kind {
                            0 => value,
                            1 => 0.7,
                            2 => -value.abs(),
                            3 => value * 10_f32.powf(-3.0 + 6.0 * t as f32 / frames as f32),
                            _ => {
                                if [0, frames - 1, 511.min(frames - 1), 512.min(frames - 1)]
                                    .contains(&t)
                                {
                                    value
                                } else {
                                    0.0
                                }
                            }
                        };
                    }
                }
                let expected = full.predict(&input, frames)?;
                let actual = chunked.predict(&input, frames)?;
                let comparison = compare(&actual, &expected)?;
                if !comparison.allclose {
                    eprintln!("full={expected:?}\nchunked={actual:?}");
                }
                assert!(
                    comparison.allclose && comparison.argmax_agrees,
                    "frames={frames}, kind={kind}: {comparison:?}"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn basename_manifest_resolves_features_from_the_working_directory() -> Result<()> {
    const CHILD: &str = "DRONE_NATIVE_RELATIVE_FIXTURE_TEST";
    if std::env::var_os(CHILD).is_some() {
        let bytes = fs::read("sample.f32")?;
        let fixture = Fixture {
            id: "relative".to_owned(),
            domain: "synthetic".to_owned(),
            frames: 8,
            path: PathBuf::from("sample.f32"),
            sha256: bytes_sha256(&bytes),
            audio_sha256: "0".repeat(64),
            cache_sha256: "1".repeat(64),
            source_sha256: "1".repeat(64),
            reference_logits: [0.0; 24],
            selection20: false,
        };
        let manifest = std::path::Path::new("manifest.json");
        let root = manifest
            .parent()
            .ok_or_else(|| anyhow::anyhow!("manifest parent"))?;
        assert_eq!(load_features(root, &fixture)?, vec![0.0; BANDS * 8]);
        return Ok(());
    }
    // A subprocess gives this test its own working directory without
    // changing the directory of parallel tests in this process.
    let scratch = Scratch::new()?;
    fs::write(scratch.0.join("sample.f32"), vec![0_u8; BANDS * 8 * 4])?;
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "basename_manifest_resolves_features_from_the_working_directory",
            "--nocapture",
        ])
        .current_dir(&scratch.0)
        .env(CHILD, "1")
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
