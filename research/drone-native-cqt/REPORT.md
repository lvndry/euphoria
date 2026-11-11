# Native CQT feasibility on identical PCM

## Problem

Euphoria needs CPU key analysis that preserves Drone's predictions while reducing
resource use. The earlier native cached-feature study establishes the inference
backend, but excludes decoding and CQT. A handwritten native frontend exists as
another contributor's uncommitted prototype. This study tests that frontend and
an alternative using the **actual exported librosa kernels**, without changing
weights, scoring labels, modifying the prototype, or integrating production code.

**Decision:** the exported-kernel candidate passes the frozen nine-case logit
parity gate. The experimental handwritten frontend fails two cases despite
preserving every predicted key. Proceed to a separately reviewed, fixed 376-track
public validation expansion; neither candidate is approved for production.

## Method

The [frozen protocol](PROTOCOL.md) lives in
the companion Drone branch (`5e536c3`, originally `4037cc6`). Its content hash is
recorded in [reference-summary.json](evidence/reference-summary.json). Reference exporter source is Drone `b1c90d3`; evidence collection records the current Drone source commit separately.

Six deterministic PCM16 signals cover silence, first/middle/final impulses,
440 Hz, major/minor chords, and seeded noise. Lengths include the minimum eight
frames and both exact-hop and irregular endpoints. Three fixed public FMAK files
are the shortest, upper-middle and longest of the existing 20-track selection:
217, 1,295 and 9,057 frames. All source audio hashes are verified. The original
FFmpeg/Drone read path produces reference PCM; both native candidates receive
**those exact FP32 PCM bytes**, separating frontend differences from decoder drift.
No native decoder is tested here.

The actual installed librosa 1.0.0 calls export the post-compensation complex64
CSR kernels, FFT sizes, hops, recursive resampling plan, and normalization
lengths. The exporter observes private calls and checks that tracing is bitwise
observational. Reference features use 22,050 Hz, 208 bins, 24 bins/octave,
hop 4,096, constant centered padding, log1p magnitude, an FP16 storage roundtrip,
and core bins `[8:200]`. These match the original Drone feature contract.
[librosa's published implementation](https://librosa.org/doc/latest/_modules/librosa/core/constantq.html)
describes the recursive CQT and its defaults; installed source hashes, rather than
an unpinned documentation page, identify the executed implementation.

The exported candidate runs a centered, zero-padded forward RustFFT in FP64,
casts the positive spectrum to complex64, applies the captured CSR in FP32,
trims/stacks all octave outputs, normalizes, computes magnitude/log1p, and rounds
through FP16. It loads the verified authoritative full ONNX graph for the final
logits through the already validated native CPU harness, ORT **1.31.1**, with
intra/inter threads one, sequential execution, CPU arena on, CPU preference and
spinning off. Rust does not invoke Python or Torch. RustFFT **6.4.1** is resolved
in the lockfile; its [planner documentation](https://docs.rs/rustfft/6.4.1/rustfft/)
notes CPU-dependent SIMD selection, so this result is tied to the recorded host.

Resampling uses a new task-local shared library built from the SHA-verified
[python-soxr 1.1.0 source distribution](https://pypi.org/project/soxr/1.1.0/),
containing the vendored libsoxr revision used by that wrapper. CMake uses Release,
shared library, OpenMP off, tests/examples/LSR bindings off, and two build jobs.
The C wrapper reproduces mono FLOAT32 split-channel HQ processing in 96,000-sample
blocks with a final null-input flush; it then reproduces librosa's ceil-length
fix and scaling. This deliberately avoids substituting system libsoxr or the
one-shot API. Source archive SHA is
`9f228ae21c78fa9359ca98d8a5e8e91f30639e438e574133dace62c5b5309e44`.
The standalone library reports `libsoxr-0.1.3`; the Python wheel reports
`0.1.3-14-ga66f3ee`. These version strings and build configurations are reported
separately: this study does **not** assert binary identity. The dynamic research
build records upstream LGPL notices and separate component license files; product packaging and
licensing remain outside this stage.

All executions are restricted to logical CPUs 2–3 with the four numeric thread
environment variables set to one. The host is the recorded AMD Ryzen Threadripper
PRO 9955WX Linux machine. Frozen numerical comparison source is Euphoria
`d8424bf`; additional response/hash diagnostics use `9fd6c34`, without modifying
the first binary or reports. Full model SHA is
`35fc1bfe5d67f7aed2c46221c15519d0f3d5822351932b5bd9899d47d48cb613`;
the checkpoint SHA recorded in build evidence is
`dcf8a7a292b7c12076ffff63c8c6550cb09a6c41ce4d000fd7949fc6bb4b452c`.

The unchanged gate requires finite features/logits, matching shapes, every logit
satisfying `abs(candidate-reference) <= 1e-5 + 1e-5*abs(reference)`, and **zero
argmax disagreements on all nine cases**. Feature error and FP16 mismatch rates
are diagnostics. Every failed case remains in the report.

## Results

![Strict logit parity and feature diagnostics for both native candidates](cqt-feasibility.png)

The dashed line is the unchanged per-logit combined absolute/relative gate.
Zero FP16 mismatch rates are displayed at the annotated plotting floor. The
[SVG](cqt-feasibility.svg), [plot script](plot_results.py) and immutable small JSON
inputs reproduce the figure; no audio or raw feature arrays are committed.

| Case | WIP max logit error / gate | Exported max logit error / gate | FP16 mismatches WIP → exported |
| --- | ---: | ---: | ---: |
| silence | 0 / pass | 0 / pass | 0 → 0 |
| impulses | 2.41e-06 / pass | 0 / pass | 6 → 0 |
| tone | 3.76e-06 / pass | 0 / pass | 1,173 → 0 |
| major | 7.27e-06 / pass | 2.38e-07 / pass | 1,084 → 1 |
| minor | 9.55e-06 / pass | 1.61e-06 / pass | 1,170 → 1 |
| noise | 1.9e-05 / **FAIL** | 0 / pass | 4 → 0 |
| fmak-110544 | 1.94e-05 / **FAIL** | 1.14e-05 / pass | 206 → 1 |
| fmak-107354 | 1.14e-05 / pass | 1.91e-06 / pass | 2,527 → 17 |
| fmak-087373 | 2.38e-06 / pass | 9.54e-07 / pass | 4,912 → 78 |

Both candidates preserve all nine predicted classes. The WIP fails the strict
logit gate for noise and FMAK 110544; unchanged argmax alone does not qualify it.
The exported candidate passes every case. Its largest absolute logit difference,
`1.144409e-5`, passes the **combined** absolute/relative criterion; it is not a
claim of an absolute-only `1e-5` bound. Its worst normalized error is below one.

The [diagnostic rerun](evidence/exported-diagnostics-9fd6c34.json) finds all **63
recursive-resampling outputs bit-identical**, including full-array hashes for
the public tracks. All **54 synthetic complex CSR responses** are bit-identical.
The major/minor signals each have one different final FP16 feature despite
identical projections, localizing those differences to later normalization,
magnitude or log1p arithmetic. Public CSR response hashes also match except
stages 1 and 4 of the longest track; the stored first/middle/end probes preserve
numerical diagnostics for those arrays. This narrows the mechanism without
claiming bitwise frontend equivalence or attributing every residual to one cause.

Reference export took 4.99 seconds for the fixed nine cases with Torch absent.
This is operational context, **not** a paired frontend latency measurement.
No latency/RSS advantage is claimed: these comparison runs load references and
intermediates and use full ONNX inference. The earlier cached-feature memory
measurements remain separate and immutable.

## Evidence and verification

- [Reference identities, versions, PCM hashes and logits](evidence/reference-summary.json).
- [WIP failed comparison](evidence/owner-d8424bf.json) and [first exported-kernel passing comparison](evidence/exported-d8424bf.json).
- [Exact source/build evidence](evidence/build-evidence-v2.json): source files,
  library/compiler/CMake/configuration/FFT binary hashes, CPU/kernel, licenses and
  the measured executable. Source build used GCC 13.3.0; see evidence for exact text.
- Euphoria `cargo test` (two focused unit tests), `cargo clippy --all-targets -- -D warnings`,
  and formatting pass. Drone ruff/ty and both tracing/generator tests pass.
  Native ORT comparisons were actually run on Linux; unit tests do not imply they
  ran on hosted CI or other platforms.

The other contributor's source remains outside Git. Only its frozen source
hashes and comparison outputs are included. Neither its worktree nor the
production detector was modified. The snapshot's checkpoint metadata was
verified against the same original checkpoint before compatibility was claimed.

## Decision and next fixed stage

Advance the **exported actual-kernel candidate** to the proposed [376-track
expansion](EXPANSION.md), subject to root review and resource approval. The WIP
failure remains an explicit blocker for substituting that frontend unchanged.
Any broader failure stops promotion with the same thresholds and membership.

Nine identical-PCM cases do not establish labeled accuracy, universal numerical
parity, different CPU/OS/libm behavior, decoder equivalence, bounded streaming
CQT memory, production integration, concurrent worker safety or a performance
win. Full PCM and recursive arrays are retained in this research executor;
streaming needs its own state/flush/boundary proof. Persisting final production
results should follow complete frontend and model provenance/parity validation.
