# Prospective Mac376 qualification and conditional paired measurement

**Proposal only. No expansion/measurement has run.** The completed08:00/08:30UTC
windows do not authorize this stage. Root must grant a fresh explicit UTC window
before any new nine/376 outcomes. Suggested allowance: three hours for streamed
qualification, followed by a separately gated30-minute measurement window. Freeze
actual orchestrator/protocol/build identities before either stage starts.

## Fixed data and comparisons

Use the SAME376 public validation tracks, order and canonical source/audio/PCM
hashes as the completed Linux expansion: no labels, private audio, new tracks,
model selection, truncation or tolerance changes. Source manifests are the
archived candidate2 study and the376 native cached-feature fixture manifest.
All376 canonical core hashes already match that fixture manifest. Use original
released full ONNX and prefix/head bundle, FP16 roundtrip/core8:200,24-key order,
stride8/halo16/core64, FP64 hidden pooling→FP32 head unchanged.

First recheck the same nine goldens through the actual prospective worker APIs,
then process all376, one track at a time. For every track compute:

- **L:** canonical Linux core/logits, retained from unchanged Linux references.
- **Mref:** actual same-version Mac Python CQT on identical PCM→Mac CPU fullORT.
- **Mfull/Mchunk:** prepared native Mac frontend on that identical PCM→Mac CPU
  full/chunked ORT. Compute the frontend once for qualification and feed both.
- **Mcore:** canonical Linux core→Mac CPU fullORT, isolating runtime drift.

Keep all four comparison families from the nine-case study, with native family
reported separately for full/chunked: Mfull/Mref, Mchunk/Mref, Mref/L, Mfull/L,
Mchunk/L, Mcore/L. Every set requires finite shape24 logits, every element
`abs(delta) ≤1e-5 +1e-5*abs(reference)`, and zero argmax disagreements over all376.
Also record full/chunk agreement directly, exact core SHA, feature max/RMS,
FP16 mismatch counts and per-case kernel/normalization differences. Feature bytes
are diagnostics, not a new tuned gate. Report all contrasts regardless of outcome.
Any failed nine or376 gate prevents performance and promotion; preserve the first
failure PCM/core/probes and bounded diagnostics, continuing fixed membership only
within the approved budgets. No formula/bias/epsilon/FP16 change from results.

Capture actual Mac private-call constants once per track with the same original
feature source/Trace, including track-dependent normalization. The nine recheck
must establish the exact prepared/timed APIs and observational output equality.
Do not reuse silence normalization or Linux constants as a Mac plan substitute.
If a structural FFT/hop/resampling contract differs, stop rather than infer it.

## Streaming transfer, exact sizes and available capacity

Existing metadata gives376 canonical FP32 PCM bytes **7,043,373,760 (6.560GiB)**,
largest **148,375,296bytes**; canonical FP32 cores total **330,250,752bytes**.
The nine-case packet was204,830,720bytes and transferred in227s by local file
birth/modified timestamps (~0.902MB/s). This is a rough observed transport rate,
not a dedicated network benchmark. Sending the entire FP32 PCM corpus would
therefore take~130minutes and require an unnecessary permanent6.56GiB packet.

Instead, prepare ONE track on the server, transfer, verify and process it, then
acknowledge/delete only its new owned spool on both hosts. Never retain a corpus
packet. Original decoding already produces signed16-bit mono22050Hz samples,
then float32(sample)/32768 without further resampling. A lossless transport can
therefore use LE int16 plus the archived FP32 PCM SHA. The producer must verify
exact int16→FP32 reconstruction SHA before sending, and Mac must independently
verify it after widening. Stop if any sample is not exactly representable; no
silent approximate/fallback representation. This cuts PCM transfer to
**3,521,686,880bytes** without changing the actual inference PCM bytes.

Canonical cores are original FP16 widened to FP32. Optionally send LE FP16 core
bytes only after exact widened-FP32 SHA verification at both ends; this adds
**165,125,376bytes**. Fixed PCM+core payload then totals **3,686,812,256bytes**,
plus small manifests/CSR/probes (~1–2%). No new lossy conversion is introduced.
At the observed rate, budget~68minutes pure transfer, conservatively90minutes
including per-track setup and modest variability. Compression may reduce actual
bytes but is not credited in the budget or required for correctness.

The new protocol must pin transport source and both representation/decoded SHA
values. Native always consumes original reconstructed LE FP32 PCM. Server audio
SHA and canonical PCM SHA must match before transfer. Canonical cores come from
existing cached-feature fixtures, never a new label/reference computation.

At07:23UTC the Mac had24GiB physical RAM,149GiB free disk, and memory_pressure
reported44% system-wide memory free. These are point observations, not reserved
resources; recheck before execution and stop if headroom is inadequate. The
existing Mac nine-case owned tree is804,692,297bytes, including preserved packet,
references, builds and repaired binary. Keep it immutable. A new streamed stage
should require<500MiB peak disk (largest PCM141.5MiB, transport70.8MiB, cores,
reference output/probes, one retained failure plus small logs). Enforce a2GiB
combined owned-tree storage stop covering the existing tree and new stage.
No deletion of historical artifacts or other actors' data is allowed.

## Exact sources and prospective repaired runtime

The measured Mac nine binary is preserved separately at
`/tmp/euphoria-native-cqt-mac-v1/measured-pipeline-c5f8c4e`, SHA
`01d51a582f815961ce67ac8263e422c8279605c2dfc0784679a3eddfc37eeb0c`.
It is not the prospective reporting binary.

The initial reporting repair was Euphoria commit **83c5145** (parent c5f8c4e),
which adds checked KiB→bytes conversion and documents the cached helper's
unchanged KiB contract. Its separate, unexecuted release target built with
Rust/Cargo 1.99.1, jobs 1, codegen-units 1/LTO off has SHA
**c694bcb3b35fbbb94c27a9790241ef9b4e03b33b39ec3db55332535153192b3c**,
`/tmp/euphoria-native-cqt-mac-v1/repaired-target/release/pipeline`.
No model/CQT execution used that candidate binary. The publication branch also
labels repaired reports as format v2 with explicit byte units. Therefore this
proposal requires a new source freeze and rebuilt executable SHA before launch;
the initial repaired candidate SHA above is historical setup evidence, not an
approved prospective execution identity. The new nine/376 proof must qualify the
actual frozen binary before timing. Never reattribute old results to a new binary.

Use existing Mac ORT1.31.1 CPU dylib SHA
`d586cbe89effc085a09d3341b3cfbd5c322517f9d397aeb20814af21433f2b36`,
and exact verified task-local soxr wrapper SHA
`49c7903d21711aff76afaaef122f7e71120f901427acb314f721f5b663b53880`.
Preserve its166-file source inventory, source tar SHA
`9f228ae21c78fa9359ca98d8a5e8e91f30639e438e574133dace62c5b5309e44`,
CMake/compiler/configuration/library hashes, task-local rpath and LGPL2.1-or-later
notices. No system installation, new wheel or SVML/x86 code on arm64. A rebuild
would require its own identity before comparisons. Full model SHA remains
`35fc1bfe5d67f7aed2c46221c15519d0f3d5822351932b5bd9899d47d48cb613`;
prefix/head hashes remain the released qualified bundle hashes.

The orchestrator/transport/benchmark Python source is not implemented/frozen yet.
Before execution, freeze its commit and SHA, Trace/features/CPU-session/guard
source SHAs, all source archives, protocol/window, original manifests, bundle,
CPU dylib/soxr/native binary, installed Python extension/source/build/CPU-dispatch
identities and exact version preflight. Require librosa1.0.0/NumPy2.5.4/
SciPy1.18.1/soxr1.1.0/ORT1.31.1 or stop before processing. The existing successful
Mac reference collector is Droneb86edd6; numerical source changes need new proof.

## CPU, wall and memory projections/caps

The completed Mac nine used10.65 reference child CPU seconds (including two CQT
calls for observational verification) and2.84 native seconds for174,658,204 PCM
bytes. Linear input-size projection for376 is~429.5 reference CPU seconds and
~114.5 native CPU seconds. Allow additional model forwards, repeated process
setup, tracing, transport checks and observer overhead: prospective Mac cap
**1200 aggregate child CPU seconds /9600 wall seconds**, earlier root UTC cutoff,
**2GiB monitored owned-tree RSS** and combined2GiB storage. These projections are
not measurements or a promise of passing. Stop when any cap is reached.

Linux's actual376 source/PCM production consumed146.35 child CPU/162.74 worker
wall seconds. A new server producer should cap **400 aggregate child CPU seconds**,
**9600 wall seconds**, **1GiB tree RSS /512MiB new disk**, CPUs4–5/env1, only one
track ready at a time. No GPU/CPU0/CPU2 work or service changes. Server transfer
wait time counts toward wall, not a reason to extend the window. Transfer and
processing remain sequential to keep accounting and storage simple.

Mac has one active numeric child, OMP/MKL/OpenBLAS/NumExpr/VECLIB all1, ORT CPU
only intra/inter1/sequential/no spin, build jobs≤2. Darwin ps RSS is KiB converted
to bytes; Darwin ru_maxrss is already bytes for Python; repaired native reports
bytes. Record CPU seconds, worker lifetime peak/loaded RSS, sampled tree RSS,
thread counts, OS/kernel/hardware/source identity and foreign concurrent load.
Do not claim Linux affinity or exclusive cores on macOS. No stopping other apps.
Sampled-tree enforcement has sampling gaps; report them honestly.

## Conditional fixed20 Mac measurement

Only if the same-source nine and all376 comparison families pass, and root's
fresh measurement window remains open, use exactly the prior20 FMAK selected
IDs. Both full and chunked are qualification requirements. Stream their existing
PCM again; never select on logits, accuracy or observed latency.

Use60 fresh workers, one per track/mode (Python full, native full, native chunk),
rotated mode order, one warmup and20 fixed timed repetitions. Timed APIs must be
exactly those qualified above: CQT→original FP16 core→inference. Decode/transfer,
input reads, plan/session loading, hashes/JSON/probes and parity comparisons are
outside timer. Preserve per-iteration integrity gates outside timing. No source
changes between376 proof and timing. Native plan construction is setup, Python's
usual CQT filter construction remains inside its API; disclose this as on Linux.

Mac nine weighted by the fixed20 PCM total531,677,184bytes suggests~700 child CPU
seconds for allthree modes×21 iterations; allocate **1200 child CPU /1800 wall
seconds**,2GiB tree RSS/combined disk, and a fresh explicit UTC stop before
outcomes. If transfer plus compute cannot fit, report incomplete rather than
reduce reps or drop tracks. No performance statement from the nine diagnostic.

Report all20 paired worker medians/CPU/peak and loaded RSS; median paired ratios,
p95 across track medians and maximum peak. Compare both native paths with Python,
and chunked with native full. Acceptance is full correctness plus visible paired
improvement; no retrospective thresholds. macOS scheduling/noise means this is a
measured host result, not a universal comparison. Failed/incomplete qualification
prevents this phase. Production integration and persisted final-result caching
remain separate work after reviewed376 parity, never an inference from nine.
