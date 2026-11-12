# Proposed Linux composed correctness and frontend/inference performance

Authorized before outcomes: new Linux window ends 2026-11-12 08:30 UTC.
Source/protocol are frozen before nine/376 qualification and conditional timing.
The old candidate2 06:00 UTC window and artifacts remain immutable.

## Question and fixed paths

Measure the actual cost of identical decoded PCM → original FP16 core features →
24 logits for Python-full, native-full and native-chunked. Use the existing fixed
20 public FMAK selection, without labels or changes to membership. The native
frontend is the qualified FMA + pinned SVML path on the recorded Linux host.
Python uses actual same-version `features.cqt`, not a rewritten approximation.

All modes use the unchanged released full model or unchanged-weight chunk bundle,
ORT1.31.1, class order `tonic-times-two-plus-minor-v1`, and the same PCM bytes.
No native decoder, final-result cache, truncation, quantization or early exit.

## Evidence already available, and its limit

The accompanying `METADATA_CHECK.json` records a metadata-only check matching all376 new reference core SHA256 values to the
original376 native cached-feature fixture SHA256 values (zero differences).
Candidate2's native FMA+SVML core values match those references bit for bit.
The old native chunked proof passed all376 original cached inputs with zero key
changes and maximum logit error1.90735e-6.

Thus, for identical deterministic inputs, the old chunk proof and new frontend
proof compose transitively. This is useful evidence, but it does not qualify a
new timed entrypoint, its buffers/state, or a compute-only refactor. Prefer the
following explicit composed proof before timing.

## Smallest clean implementation

Introduce one shared numerical API used by both validation and timing:
`FrontendPlan::compute(pcm) -> core`, followed by the existing `Full::predict` or
`Chunked::predict`. Separate provenance hashing, reference loading, per-stage
array probes, JSON formatting and evidence writes from that API. Preserve every
arithmetic operation and original FP16 roundtrip. Preload verified kernels,
resampler handle/configuration and model sessions as worker setup.

This is a NEW core/source identity because it removes instrumentation from the
current executor. Freeze runtime, evaluator, source archive/lockfiles, options
and all artifacts before qualification; no subsequent edits between proof and
timing. Source-only/shape/finite checks needed by the numerical API remain in the
timed path. Do not benchmark the instrumented parity CLI and label it compute-only.

A lightweight reference producer may regenerate PCM through the unchanged
FFmpeg/Drone decode/read/resample functions. Each PCM SHA must equal the already
recorded candidate2 PCM SHA, and each original audio SHA must match the fixed
manifest. Use the existing canonical core hashes/logits as references, avoiding
another unnecessary376 Python CQT run. No labels enter this producer.

## Correctness stage before timing

1. Recheck all nine existing goldens through the actual new compute API, both
   native-full and native-chunked. Keep the min-eight-frame/endpoints/tails and
   existing chunk geometry/boundary unit checks.
2. Run every one of the fixed376 public validation tracks through native CQT,
   then both full and chunked inference on the SAME newly produced native core.
   Reuse each track's CQT result within this qualification pass; do not recompute
   CQT separately for the two modes.
3. Require finite values, exact shapes and native core SHA equal to the canonical
   raw-FP32 fixture SHA. Independently require every full and chunked logit to
   satisfy `abs(delta) <= 1e-5 + 1e-5*abs(reference)` and zero argmax changes on
   all376, against unchanged Python-full reference logits. Also report full vs
   chunked differences. Preserve first failures and refuse timing if either path
   fails. No tolerance/FP16/model/membership changes.

Chunking stays64 output positions, stride8, halo16, exact overlap/trim, full tail,
FP64 hidden sum/mean → FP32 → unchanged tiny head once. The exported contract's
historical `validation_passed:false` flag is not rewritten; separate passing
reports bind the actual artifacts.

## Timing stage, only after passing qualification

Exactly60 fresh workers: one per(mode,track), with one frontend plan and the
appropriate model session(s) per worker. Counterbalance the three mode orders by
track index modulo3, fixed before outcomes. Run one worker at a time on one
reserved logical CPU (reserved CPU2), all four
numeric thread variables1, ORT intra/inter1/sequential/no spin. No other team
work on that CPU and no service/GPU changes.

The timer includes CQT, magnitude/log1p, original FP16 roundtrip/core formation,
and model inference (including chunk trim/pooling/head for native-chunked).
It excludes common decoding, PCM/file reads, hash verification, plan/session
creation, JSON/probe/file writes and reference comparisons. Explain that native
precompiled invariant kernels are setup, while any filter construction performed
inside the unchanged Python `features.cqt` call remains part of that API's cost.
The result measures these worker APIs, not isolated FFT throughput.

Use one untimed warmup, checked against the reference, then20 timed repetitions;
check each resulting logit vector outside the timed interval. No extra repeated
baseline pass. Record per-track median/dispersion and paired native/Python ratios;
report overall median paired ratio and p95 across the20 per-track medians,
explicitly distinguished from pooled-call p95. Capture loaded RSS after setup
and true OS high-water RSS for the fresh whole worker, including PCM/setup and
allocator retention. Model-only chunk memory bounds do not bound full CQT/PCM.

## Artifacts and provenance

Bind full model SHA `35fc1bfe5d67f7aed2c46221c15519d0f3d5822351932b5bd9899d47d48cb613`,
prefix `61fa16b909865c1eb490581e2452790f921e39deb835181f430ec62d38fd0f9c`,
head `9a04ea48929564f3cbbeca4d68c74fae202162e0588335c55e43c751c3c6eff5`,
contract/checkpoint/class schema, ORT/soxr/SVML binaries and source/license hashes,
front-end plan, raw PCM/core/reference hashes, selection manifest, executable,
runtime/evaluator sources, thread options, CPU model/kernel/affinity. Both timed
reports must bind the SAME passing new qualification and same fixed membership.

Existing bundle is `/home/lysk-admin/drone-experiments/k10-chunked-20261111-v1/bundle`;
original cached fixture manifest is under `k10-native-fixtures-20261111-v1/fixtures`.
Keep all old source tags/windows and artifacts immutable.

## Requested resources and acceptance

| Stage | Proposed hard stop |
| --- | --- |
| One nine+376 composed proof |1200 aggregate child CPU seconds /1500 wall seconds |
| Entire three-mode timing stage |1800 aggregate child CPU seconds /2100 wall seconds |
| Each worker tree |2GiB RSS stop; sampled monitor plus OS high-water reporting |
| New dedicated stage storage |2GiB; stream one spool, no all-track PCM retention |

The fixed20 have32,451 frames; retaining all PCM would need at most531,677,184
bytes. Longest per-track PCM is bounded by148,389,888bytes. Existing artifacts
are read-only; a new root holds source/build/results and at most one active spool
plus bounded retained failures. Build jobs2 outside timing, then one active
numerical worker. Source/setup is expected below~500MiB plus~142MiB active PCM;
stop on observed resource excess rather than delete historical evidence.

The old539CPU/608wall376 run included a fresh Python CQT reference per track;
the lighter PCM-only producer should reduce qualification work, but this is a
projection. The budgets are caps, not measured speed claims. Report all timings
and memory ratios even if native is slower. A performance win requires a visible
paired improvement with the full correctness gate intact; production remains a
separate integration decision. Root review is required before timing starts.

Suggested implementation: split numerical operations from observation in
`research/drone-native-cqt/src/plan.rs`, expose one shared compute API, add a
validation/measurement driver using that API, and a lightweight PCM producer
that calls the original decode/read/resample functions. Build all modes before
qualification and bind the timing evaluator source hash to the passing report.

CPU placement clarification approved prospectively before outcomes: producer and
composed qualification use logical CPUs4–5 with env1. Every actual timed Python,
native-full and native-chunked worker uses CPU2 alone. The controller uses4–5.
Identity binds evaluator, guard-supervisor and experiments/chunked.py CPU-session
source hashes in addition to numerical binary, artifacts and configuration.
