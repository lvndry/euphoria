# Native CQT composition and Apple M5 portability

## Problem and result

A cached-feature benchmark does not establish that a native frontend preserves
the released classifier, nor measure its memory cost. This study composes the
captured CQT frontend with the unchanged full and chunked ONNX paths, and tests
whether the platform math works on Apple M5 without x86 SVML.

On Linux, the new compute-only API passed nine goldens and all **376 public
validation tracks**. Every FP16-roundtripped core hash matched the canonical
reference. Native full logits were bitwise equal to the reference; chunked logits
had maximum absolute error **1.90735e-6**. Both had **zero key changes**. No labels
were used. Qualification includes FMAK and Beatport; the fixed performance subset
contains **20 FMAK tracks and no Beatport tracks**.

On those 20 tracks, native full and chunked paths lowered median paired latency
by **27.19%** and **26.92%** versus Python full, and median paired worker peak RSS
by **53.11%** and **64.94%**. Chunking added **0.897%** median paired latency versus
native full while reducing median paired peak RSS by **25.85%**. The longest
case still reached **477.5 MiB** with chunking: PCM and frontend allocations
remain material. This is not the earlier cached-only memory result.

![Measured Linux latency and corrected peak RSS](linux-composed.png)

The figure shows fresh-process peaks and within-worker medians, not model-only
latency. All timing workers were restricted to Linux logical CPU2. A separate
late observer saw two OS threads restricted to that CPU; it did not sample every
worker. All four numeric thread environment variables were1.

| Mode | Median track median | p95 track median | Median worker peak | Maximum worker peak |
| --- | ---: | ---: | ---: | ---: |
| Python full |90.791ms |186.535ms |383.783MiB |1241.738MiB |
| Native full |64.985ms |157.664ms |181.264MiB |1076.336MiB |
| Native chunked |67.063ms |157.219ms |132.838MiB |477.500MiB |

These summary medians are not used to calculate paired effects. Per-track ratios
are calculated first, then their median. p95 uses linear interpolation across20
track medians. The frozen composed protocol required a visible paired improvement
with full correctness. The older program target of2× maximum RSS reduction and
≤10% latency regression was not a numeric preregistered gate for this stage;
maximum native-full/native-chunked RSS happened to improve2.254×.

## Method

The shared `PreparedPlan` preloads verified CSR/FFT constants and each track's
normalization/resampling plan. Its compute path performs CQT, original log1p and
FP16 roundtrip, then either unchanged full ONNX or the qualified overlap/trim
prefix, FP64 hidden mean and unchanged tiny ONNX head. Linux uses FMA-ratio
magnitude plus the pinned upstream SVML kernel. Mac uses FMA-ratio magnitude plus
platform log1p, with actual Mac-captured constants and exact-revision task-local
libsoxr. No NumPy/Python runtime is called by native inference.

All376 references were recreated from SHA-verified public audio through the
original decode/PCM preparation; PCM hashes had to match the archived canonical
references. Qualification ran on CPUs4–5/env1 and explicitly exercised the new
prepared API with full and chunked inference. It passed before timing. The
qualification and timing reports have identical source/config/artifact bindings,
including the supervisor and CPU-session source hashes. No numerical source
changed between them.

Timing used60 fresh workers: Python full, native full and native chunked for each
of20 fixed tracks, with mode order rotated by track. Each worker loaded the same
PCM before timing, warmed once, then performed20 timed repetitions. Timers cover
CQT→FP16-roundtripped core→logits. Decode, input reads, session/plan setup, kernel
hashing, evidence JSON/probes and output integrity comparisons are outside the
timed region; returned-input/API checks remain inside the compute calls. Python's
normal CQT filter construction remains in its timed API; native plan construction
is setup. This measures the usable prepared native API, not just identical
low-level operator work. Every timed iteration retained core-hash, finite/logit
and key checks outside its timer. RSS covers the complete fresh worker lifetime,
including setup, PCM, frontend and inference, rather than only the timed region.

The Linux host was AMD Ryzen Threadripper PRO9955WX. ORT1.31.1 CPU only used
intra/inter1, sequential execution and disabled spinning. Unchanged model SHA is
`35fc1bfe5d67f7aed2c46221c15519d0f3d5822351932b5bd9899d47d48cb613`.
Source/binary/library/config/hardware bindings and every raw report are in the
[lossless evidence index](evidence/index.json). No ML runtime download is needed
for the evidence/unit tests.

Qualification consumed218.19 aggregate child CPU seconds and269.87 wall seconds;
timing consumed162.55 child CPU and168.74 wall seconds. Both completed before the
authorized2026-11-12 08:30UTC deadline. The stage used the reviewed1200/1500s
qualification and1800/2100s timing CPU/wall limits,2GiB monitored tree RSS/storage
limits, and streamed/deleted each track spool. These are research-supervisor
observations and stops, not a universal allocation bound.

## RSS unit erratum and reproducibility

**The immutable native `pipeline` reports mislabeled KiB as `peak_rss_bytes`.**
At executed source `c5f8c4e`, `drone_native_cached::process_usage()` returns KiB on
both platforms (Linux ru_maxrss directly, Darwin ru_maxrss/1024). `pipeline` stored
that value unchanged under a byte field. All native peak RSS values in this
report/checker are recovered as **raw ×1024**. Original bytes, SHA values and
server artifacts are preserved. Python peak values, loaded-RSS values and
supervisor sampled tree RSS already use bytes and are not multiplied.

[The checked summarizer](summarize.py) verifies compressed and decoded bundle
hashes, every raw-file hash, track/repetition counts, qualification identity and
peak≥loaded RSS. Its regression fixes the known105408KiB→107937792byte example.
The post-measurement source fixes future reports by checked conversion once; it
was not used to produce these measurements and no timing binary was rebuilt.

```sh
python3 research/drone-native-cqt/composed/summarize.py
python3 -m unittest discover -s research/drone-native-cqt/composed -p 'test_*.py'
uv run --script research/drone-native-cqt/composed/plot_results.py
```

The bundles preserve original UTF-8 file bytes losslessly, including native's
incorrect field name, with SHA256 for every file and for compressed/decoded
bundles. Figures derive from those verified bytes and the documented unit
recovery. PNG and SVG are adjacent to this report.

## Apple M5 nine-case diagnostic

Mac processing used the same six synthetic and three public canonical PCM arrays,
without new decoding or SVML. Exact preflight versions matched Linux:
librosa1.0.0, NumPy2.5.4, SciPy1.18.1, python-soxr1.1.0, ORT1.31.1. The verified
166-file vendored soxr tree was built task-locally, Release/shared/OpenMPoff,
jobs≤2, with no installation or production change. Source/compiler/CMake/library
hashes, dependency resolution and LGPL2.1-or-later provenance are preserved.

All nine cases passed every named comparison using the unchanged finite/shape,
`abs(delta) ≤1e-5 +1e-5*abs(reference)` and zero-argmax-change gates:

| Comparison | Cases passed | Maximum absolute logit error | Key changes |
| --- | ---: | ---: | ---: |
| Native Mac / actual Mac Python |9/9 |7.153e-6 |0 |
| Actual Mac Python / canonical Linux |9/9 |2.027e-5 |0 |
| Native Mac / canonical Linux |9/9 |1.407e-5 |0 |
| Canonical Linux core through Mac ORT / Linux ORT |9/9 |1.335e-5 |0 |

The relative term allows some absolute errors above1e-5. These are not bitwise
comparisons. Native/Mac reference cores differed at303 of2,048,256 values;
feature max/RMS and per-case mismatches remain in the raw comparisons.

![All fixed Mac comparisons and visible feature differences](mac-nine.png)

The Mac reference and native workers consumed10.65 and2.84 child CPU seconds,
respectively, with9.33 and2.95 wall seconds. These are instrumented diagnostic
worker costs, not performance results. Sampled owned-tree peak RSS was
1,268,514,816bytes. There was one numeric worker, thread variables and VECLIB set1;
macOS offers no Linux-style hard affinity here. The retained executor completed
exit0 before08:00UTC. An earlier detached launch was cleaned up before any stage
existed; its empty log and absent PID were investigated, and the retained session
is the authoritative execution record.

## Decision and limits

The prepared Linux frontend→full/chunked API is qualified on the fixed376 public
tracks, and its20-case FMAK performance improvement is measured. Preserve both
paths and prefer chunked inference for long inputs. The complete frontend still
materializes PCM and CQT arrays, so production memory requires further work.

The Mac nine-case result justifies proposing the same376-case expansion and
conditional performance measurement under a new window. It does not establish
Mac376 parity, production integration, decoder parity for a different decoder,
or key-label accuracy. No final-result cache, Euphoria production key detector,
default dependency/MSRV, model weights or training changed.
