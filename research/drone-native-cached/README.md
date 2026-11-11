# Native K10 cached-feature research

This standalone Rust 1.92+ workspace leaves Euphoria's production workspace,
MSRV, dependencies, detector and cache unchanged. It measures **native cached
feature inference**, not audio-to-key end-to-end behavior. It does not decode,
resample, compute CQT, invoke Python/Torch, score labels, or authorize promotion.

## Fixed runtime and parity contract

`ort = 2.0.0-rc.15`, API 31, dynamically loads a caller-supplied shared library.
The library's C API version string must equal **1.31.1**, including patch version.
No automatic binary/model downloads or GPU crate features are enabled. Cargo.lock
is committed. Sessions use PreferCPU, CPU arena enabled (matching Python),
sequential execution, intra/inter threads=1 and spinning disabled. Each full
benchmark creates only a full-model session; each chunked benchmark only a prefix
and a head session. Validation uses both modes in its own separate process.

Important current runtime behavior: registering `CPU` alone does not override
the Rust wrapper's default MaxEfficiency device policy, which can select CoreML
on macOS. An initial local synthetic 8-frame test exposed FP16-rounded full logits
and a 4.78e-4 chunk/full discrepancy. Explicit PreferCPU fixes the provider choice;
the numerical gate was not relaxed. See the [October 13, 2026 upstream issue and
maintainer explanation](https://github.com/pykeio/ort/issues/662) and
[pinned builder API](https://docs.rs/ort/2.0.0-rc.15/ort/session/builder/struct.SessionBuilder.html).

The eval prefix preserves 24 frequency bins, temporal stride 8 and receptive
field `[8q-14,8q+21]`. For disjoint cores `[q0,q1)` of 64 outputs, read
`[max(0,8q0-16),min(T,8q1+16))`, retaining local indices
`[q0-a/8,q1-a/8)`. Preserve the actual T%8 tail and only the network's real-end
padding. Maximum prefix input is 544 frames. Accumulate hidden features in FP64,
divide by floor(T/8), cast to FP32 once and apply the unchanged ONNX head once.
No quantization, sampling, early exit or input truncation.

Class ID is tonic*2+minor, with C=0. Argmax selects the first index on an exact
tie, matching NumPy. All inputs/outputs must be finite; T>=8 and batch=1 only.
Fixed gate: atol=rtol=1e-5 **and zero argmax disagreements**, on all376 fixtures.
Compare native full/reference, native chunked/reference and native chunked/full.
Real-arithmetic equivalence does not promise bitwise floating-point parity.

## Fixture format (exported by Drone)

`manifest.json` contains:

```json
{
  "format": "k10-core-f32le-v1",
  "feature_spec": "cqt24-8oct-c1-pad4-hop4096-v2",
  "label_schema": "tonic-times-two-plus-minor-v1",
  "layout": "bands,time",
  "dtype": "float32-le",
  "bands": 192,
  "full_model_sha256": "64 lowercase hex characters",
  "checkpoint_sha256": "64 lowercase hex characters",
  "reference_onnxruntime": "1.31.1",
  "validation_report_sha256": "64 lowercase hex characters",
  "export_tool_sha256": "64 lowercase hex characters",
  "rows": [{
    "id": "existing-prefixed-public-id",
    "domain": "fmak",
    "frames": 320,
    "path": "features/public-id.f32",
    "sha256": "hash of this raw f32 file",
    "audio_sha256": "hash of the public source audio",
    "cache_sha256": "hash of original padded FP16 NPY cache",
    "source_sha256": "same original cache hash when that is the source",
    "reference_logits": [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
    "selection20": true
  }]
}
```

Require exactly376 unique domain/id rows and exactly20 selected unique audio
identities. Relative feature paths contain normal components only. Raw files
are headerless, contiguous band-major `[192,T]` little-endian f32: take the
original padded cache `[8:200]`, widen its FP16 values to FP32, **without recomputing
or renormalizing features**. The raw hash is separate from source/cache hashes.
Reference logits come from the unchanged authoritative full ORT artifact; the
exporter reads no ground-truth key references. No labels appear in Rust structs.

Bundle `contract.json`, prefix/head ONNX hashes, dimensions, class order,
checkpoint identity, feature schema and aggregation are checked. The caller's
full-model hash must equal the fixture manifest's full-model hash.

## Build and run

```sh
cargo test --locked --manifest-path research/drone-native-cached/Cargo.toml
cargo clippy --locked --manifest-path research/drone-native-cached/Cargo.toml --all-targets -- -D warnings
cargo build --release --locked --manifest-path research/drone-native-cached/Cargo.toml
```

Use the executable directly when measuring; do not include Cargo compilation.
On the research server the verified Python ORT CPU library is:
`/home/lysk-admin/drone/.venv/lib/python3.12/site-packages/onnxruntime/capi/libonnxruntime.so.1.31.1`.
Rust/cargo were initially absent. With explicit experiment-owner authorization,
minimal Rust1.99.1 was installed under
`/home/lysk-admin/drone-experiments/native-runtime-20261111-v1/toolchain/{rustup,cargo}`.
No shell profile/system changes were made. The official installer and channel
manifest were checked against published SHA-256 files; the manifest lists pinned
component hashes. `toolchain/provision-plan.json` records URLs, hashes and byte
sizes (about148MB compressed downloads,664MB installed at initial measurement).
All Cargo/Rustup environment paths remain task-local, build jobs are capped at2,
and storage/download budget is2GiB.

Example argument contract (set these paths and the frozen full-model digest first):

```sh
NATIVE=research/drone-native-cached/target/release/drone-native-cached
ORT_LIBRARY=/home/lysk-admin/drone/.venv/lib/python3.12/site-packages/onnxruntime/capi/libonnxruntime.so.1.31.1
FIXTURES=/path/to/export/manifest.json
BUNDLE=/path/to/unchanged/chunk-bundle
FULL_MODEL=/path/to/authoritative/drone-v1.onnx
FULL_SHA256=the_frozen_64_character_full_model_digest
```

Each command below runs in a **fresh process**. Set one CPU affinity externally;
the process enforces a 600 CPU-second ceiling and the outer timeout bounds wall
time. Existing output files are refused. Run validation first, then full and
chunked measurements with 20 timed repetitions and one warmup per selected track.

```sh
env OMP_NUM_THREADS=1 MKL_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 NUMEXPR_NUM_THREADS=1 timeout 600 taskset -c 0 "$NATIVE" --mode validate --manifest "$FIXTURES" --bundle "$BUNDLE" --full-model "$FULL_MODEL" --full-model-sha256 "$FULL_SHA256" --ort-library "$ORT_LIBRARY" --output /tmp/native-validation-v1.json
env OMP_NUM_THREADS=1 MKL_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 NUMEXPR_NUM_THREADS=1 timeout 600 taskset -c 0 "$NATIVE" --mode full --selection-only --manifest "$FIXTURES" --bundle "$BUNDLE" --full-model "$FULL_MODEL" --full-model-sha256 "$FULL_SHA256" --ort-library "$ORT_LIBRARY" --validation-report /tmp/native-validation-v1.json --output /tmp/native-full-v1.json
env OMP_NUM_THREADS=1 MKL_NUM_THREADS=1 OPENBLAS_NUM_THREADS=1 NUMEXPR_NUM_THREADS=1 timeout 600 taskset -c 0 "$NATIVE" --mode chunked --selection-only --manifest "$FIXTURES" --bundle "$BUNDLE" --full-model "$FULL_MODEL" --full-model-sha256 "$FULL_SHA256" --ort-library "$ORT_LIBRARY" --validation-report /tmp/native-validation-v1.json --full-report /tmp/native-full-v1.json --output /tmp/native-chunked-v1.json
```

Reports bind the executable, compiled source, lockfile, ORT library, model,
bundle and fixture hashes, CPU affinity and thread environment. Changing code or
runtime requires validation again. Timings include input validation and tensor
construction; fixture I/O/hash/parse time is reported separately. RSS covers the
whole native process, including feature loading. Linux uses ru_maxrss KiB;
macOS bytes are converted to KiB. CPU time includes all threads in this process.
No child processes run. Process thread counts are reported on Linux.

Only paired fresh reports can emit `performance_target_met`: >=2x RSS reduction
against fresh full and historical535MiB, median paired latency ratio<=1.1 and
parity still passing. This is a tentative model-runtime target, not a native
audio pipeline or production readiness claim. `promotion` is always false.

## Explicit runtime integration test

Set `DRONE_ORT_LIBRARY`, `DRONE_BUNDLE`, `DRONE_FULL_MODEL` to trusted local artifacts:

```sh
cargo test --locked --manifest-path research/drone-native-cached/Cargo.toml pinned_runtime -- --ignored --nocapture
```

This test runs synthetic inputs only, including all remainders, min8, seams,
tails, constants, negatives, impulses and a 1e-3..1e3 amplitude range. It invokes
no Python. Ordinary tests independently cover geometry, row-major slicing,
mapping/ties, nonfinite/short input rejection, integrity and provenance guards.
