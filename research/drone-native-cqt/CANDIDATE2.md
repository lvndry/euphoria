# Frozen arithmetic ablation: two source-defined native CQT arms

Authorized NEW execution window on2026-11-12 after the old00:45UTC window expired;
new deadline06:00UTC. Frozen before new nine-case/376 outcomes.
Original failed expansion source Drone b5ad965 / Euphoria66db00c, its373/376 strict
passes, all376 unchanged keys, and first-failure artifacts remain untouched.
This is numerical compatibility research, not label scoring or model selection.

## Fixed changes and evidence

Run two explicitly named arms on **the same per-track reference PCM/features**:

1. `fma`: NumPy-source finite complex magnitude
   `larger * sqrt(fma(smaller/larger, smaller/larger, 1))`, followed by unchanged
   Rust `f32::ln_1p` / glibc log1pf.
2. `fma-svml`: identical magnitude, then the byte-identical upstream NumPy/SVML
   `__svml_log1pf16` routine via a separate guarded research shared library.

The first-failure arithmetic diagnostic found exact normalization,43,431 scalar
hypot-vs-NumPy magnitude differences and20,730 libm-vs-NumPy log1p differences over
124,032 values. Five of its six FP16 mismatches originate in magnitude, one in
log1p. The source-defined magnitude matches NumPy on all124,032 values; the
isolated upstream SVML routine also matches all124,032 reference logarithms bit
for bit. These operation-order and upstream-kernel changes are justified by
source and intermediates, with no epsilon, bias, labels or final-key adjustment.

SVML submodule revision `bd9d46e0c627ab16f3d2641f7cdb1bc15d303ba4`, pinned by
NumPy2.5.4; source Gitblob `36107575d7d88a074b7e828eddfc9a6b034186d8`.
Original assembly bytes and BSD-3-Clause notices are preserved under `vendor/`.
The executed research library SHA256 is
`8749faf3d54f5256f87bfac7a1a4bcfff56b17deed61686bebb3efb43c65a3bc`.
Hash-check it in every native process. The wrapper requires Linux x86-64 with
AVX512F/DQ/BW/VL and FMA, fails explicitly otherwise, batches16 float32 lanes and
zero-pads only the tail. Native execution links no Python or NumPy.
No Mac, portable SVML, production-packaging or performance claim is established.

## Unchanged contracts and gates

Use the unchanged actual-librosa reference producer, original FFmpeg PCM path,
fixed376 validation identities sorted by(domain,id), audio SHA checks, kernels,
soxr plan, per-track actual normalization capture, released full ONNX weights,
ORT1.31.1, class order and FP16 roundtrip. No truncation or altered labels.

Recheck all nine original goldens independently for both arms under the newly
frozen binary. The `fma-svml` primary must pass nine before the expanded run; a
failed FMA-only nine check is retained and disqualifies that arm, while its376
results may continue as declared diagnostics. **Neither arm qualifies unless
its own nine and all376 checks pass.** Require finite values, matching shapes,
per-logit `abs(Δ) <= 1e-5 + 1e-5*abs(reference)` and zero argmax disagreements.
Report both arms independently regardless of outcome; do not hide a failed arm
or choose using labels. Every per-track reference spool feeds fresh FMA then
fresh FMA+SVML processes before cleanup. Preserve the first failed spool per arm
(with shared storage when both first fail on the same track).

## Fixed resources and stop conditions

One sequential per-track reference producer, then the two fresh native processes;
no concurrent ORT sessions across processes. CPUs2–3, all four numeric thread
environment variables1, intra/inter1/sequential/no spin. Same aggregate child CPU
cap1,200seconds, wall cap1,500seconds, sampled owned-tree RSS stop2GiB and total
dedicated experiment storage stop2GiB, including previous artifacts. Stop earlier
at2026-11-12 06:00UTC. The supervisor records sampling/resource limits explicitly;
no timing advantage is claimed from these parity runs.

Keep reference and both-arm hashes/logits/metrics, source/build/executable/math
identities, nine-case reports, first failures and complete/failure status. Any
later math/source change needs a new identity and new nine+376 proof. Only root
may authorize subsequent isolated public20 frontend/inference timing.
