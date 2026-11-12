# Proposed fixed public-376 identical-PCM expansion

Authorized by root on 2026-11-11 before execution. This fixed expansion does not
change the nine-case study, its failed WIP report, or any existing parity gate.

## Fixed membership and acceptance

Use **every role=validation row** of the unchanged prepared-data manifest:
376 identities, 126 Beatport and 250 FMAK, sorted by `(domain,id)`. Load typed
identities/cache hashes; never read ground-truth references. Stat inventory finds
all 376 original audio paths present. Verify every audio SHA before decoding.
Use the original FFmpeg/Drone PCM path and pin its executable/source hashes.

Export one common kernel/normalization/resampling plan from the fixed silence
case, verifying that all nine existing captured plans agree. Do not regenerate
filters, approximate resampling, choose examples by outcomes, crop input, change
weights, or adjust tolerances. Adapt the research contract explicitly for 376
cases rather than bypassing the current CLI's strict nine-case guard.

For each file, obtain identical PCM and actual installed librosa features,
round through FP16, and compute reference logits with the unchanged released
full ONNX/ORT1.31.1. Independently run native exported-kernel CQT on the exact PCM
and the same full ONNX. Require finite values, matching frame/core shapes,
`abs(Δ) <= 1e-5 + 1e-5*abs(reference)` for all logits, and **zero argmax changes on
all 376**. Feature maximum/RMS/FP16 mismatch rates are diagnostics. Preserve the
first failure, continue collecting bounded diagnostics if safe, and refuse
promotion if any case fails. No label score is reported.

Record original audio/cache/PCM/source/plan/model/library/executable hashes,
reference and candidate logits, per-case numerical metrics, stage status and
resource totals. Record source changes as a new execution identity. Check the
new runtime against the existing nine cases before the expanded run.

## Resource projection and bounded execution

Metadata inventory (read-only; no additional audio processing):

| Quantity | Value |
| --- | ---: |
| Tracks available | 376 / 376 |
| Total feature frames | 430,014 |
| Longest track | 9,057 frames |
| Original compressed audio bytes, already present | 2,396,087,234 |
| Estimated all-track FP32 PCM bytes | 7,045,349,376 (~6.56 GiB) |
| Estimated largest FP32 PCM bytes | 148,389,888 (~141.52 MiB) |

Do **not** retain all-track PCM. Use one per-track spool directory and remove
only that directory's bulky generated PCM/features after hashes, metrics and
logits are atomically recorded. Keep original audio, caches and nine-case goldens
untouched. A fresh Python reference producer exits before a fresh Rust candidate
starts, avoiding two simultaneously resident ORT allocation histories. The
controller uses the standard library only; Rust never invokes Python. Persistent
small records survive interruptions and may not be silently reused under changed
source identities.

The nine-case reference export took 4.99 seconds for 10,668 total frames.
Frame-linear extrapolation is approximately **201 seconds**, but process startup,
format/duration distribution, I/O and the independent Rust pass make this an
estimate rather than a measured expansion runtime. Propose **1,200 aggregate
child CPU seconds / 1,500 wall seconds** for the complete sequential stage,
including reference production, native inference and diagnostics. Stop before
the fixed feasibility deadline if it arrives first.

Pin workers to CPUs **2–3**, all four numeric thread variables to one, ORT
intra/inter one/sequential/no spin. Run one track and one producer/candidate
process at a time. A supervisor should enforce a **2 GiB peak RSS stop** on its
owned process tree and stop at **2 GiB total new experiment storage**, including
existing nine-case artifacts. Current dedicated-root evidence records ~561 MiB;
a single ~142 MiB PCM plus features/temporary decode files leaves substantial
disk headroom. Full ONNX inference has previously approached ~0.8 GiB by itself;
~1–2 GiB per-process RSS is a projection, not a measured native-CQT bound.
If the stop is reached, retain failure/resource evidence and redesign the
research orchestration; do not truncate features or relax parity.

## What passing would permit

Passing qualifies the native frontend on these identical decoded PCM inputs and
this pinned CPU/runtime configuration. It permits a later decoder parity study
and a separately isolated full/chunk frontend+inference benchmark. It does not
qualify an Euphoria decoder, production streaming, persisted results, different
hardware or new model weights. Final-result caching follows complete frontend,
model, decoder and version provenance validation.

The supervisor rechecks all nine existing goldens under the new binary first.
It preserves the first failed case spool, keeps subsequent small diagnostic
records, and stops on any reference/identity/resource error. Per-track actual
normalization calls are recaptured; no silence-derived statistics are reused.
