# Native CQT feasibility: frozen identical-PCM protocol

Declared 2026-11-11 UTC before numerical comparisons. This is a bounded research
stage, not a released/native audio pipeline or a labeled accuracy evaluation.
Existing cached-feature measurements at Euphoria source58c290b remain immutable.

## Question and fixed acceptance gate

Can native code reproduce the actual installed librosa1.0 frontend closely enough
to preserve K10's logits and predicted key on identical reference PCM?

Require finite features/logits, identical feature shape, final logits allclose
with atol=1e-5 and rtol=1e-5, and zero argmax disagreements on every frozen case.
Feature maximum/RMS error and FP16 mismatch rate are diagnostic measurements,
not tunable acceptance thresholds. A failing case blocks any parity claim; keep
the case, original reference, gate and membership unchanged. No labels are scored.

## Frozen cases

All six synthetic signals are generated deterministically, quantized to PCM16,
written as mono22050Hz WAV, then passed through the original features.py FFmpeg
decode/read/resample path. Reference PCM is the actual decoded FP32 signal.

| Synthetic | Input sample count | Signal |
| --- | ---: | --- |
| silence | 7*4096 | Exact zeros; minimum eight centered frames |
| impulses | 8*4096+1 | Pulses at first, middle and final sample |
| tone | 15*4096-1 | 440Hz sinusoid |
| major | 16*4096 | C3/E3/G3 equal-amplitude chord |
| minor | 17*4096+17 | A2/C3/E3 equal-amplitude chord |
| noise | 31*4096+127 | PCG64 seed20261111; amplitude bounded below clipping |

Public cases are the shortest, upper-middle and longest of the existing20 FMAK
validation selection, sorted by (frames,id), without consulting labels:

- fmak:110544,217frames, audioSHA256 `77bea21b8fc4e58323d5d48953b9ce4e6b2ec0c3452663fe0f2510a9ef2f18c9`.
- fmak:107354,1295frames, audioSHA256 `22e809a87ebb977793214280cfa0953639abe4377800581d9ab24d89013faf16`.
- fmak:087373,9057frames, audioSHA256 `25d906f7a7f0b19f340972fa8b1e882c13ceb89ed4cd83487c5c3bf74255d755`.

Each public identity must match role=validation metadata and the frozen content
hash before reading audio. No new/private audio or other validation membership.
Decoder comparison is separate: this stage feeds native CQT the exact reference
PCM and does not substitute Symphonia or a new resampler for FFmpeg decoding.

## Actual source and exported plan

Require installed librosa1.0.0 and record NumPy/SciPy/soxr/FFmpeg versions and
source/binary hashes. Load the actual drone/features.py directly to avoid eager
package/Torch imports. Reference parameters are22050Hz,208bins,24bins/octave,
hop4096,fmin four semitones below C1,constant centered padding,log1p magnitude,
FP16 storage roundtrip and core slice[8:200].

Export the actual sparse complex64 FFT basis after downsampling compensation,
not a rewritten filter generator: wrap librosa's actual private filter-response
calls to capture basis CSR arrays, n_fft, hop, sample rate and stage mapping.
Capture the actual recursive-resampling calls, normalization divisors, output
stack trim and dtype boundaries. Export immutable little-endian arrays and
JSON contracts with hashes. Save intermediate arrays for the small synthetics;
public intermediates may be hashes and selected frame/sample probes if necessary
to keep storage below the fixed budget. This choice must be reported.

The Python soxr1.1.0 wheel embeds libsoxr0.1.3-14-ga66f3ee; system libsoxr0.1.3 is
not silently treated as identical. Source/API/license/build provenance must be
established before an exact-resampler claim. The one-shot C API defaults to low
quality, so explicit HQ configuration is mandatory. No approximate CQT substitute.

## Native candidates and diagnostics

First inspect/freeze a read-only snapshot of the existing uncommitted
Euphoria drone-key custom frontend as an experimental candidate, preserving its
owner worktree. Record every snapshot hash and its safetensors metadata. It
regenerates complex-f64 filters and analytically reconstructs a halving FIR; no
equivalence to the installed reference is assumed. Final features can be passed
through the already-validated fixed full ORT graph to isolate frontend error
from a new hand-written network's rounding behavior.

If that candidate fails, localize using exported kernel/resampling/STFT stages.
A controlled native plan executor may use RustFFT with the exact exported basis
and matching vendored libsoxr HQ. Keep the candidate results separately; do not
relabel a later executor as the original prototype. No edits/publication of the
other actor's uncommitted frontend and no production detector/cache integration.

## Budget, evidence and decision

Use CPUs2–3 only with library thread counts1, never CPU0 or any GPU. All generated
files go under `/home/lysk-admin/drone-experiments/native-cqt-20261111-v1` with a
2GiB storage cap. Stop feasibility work by2026-11-12 00:45UTC, within the authorized
two-hour envelope. No service changes, new large downloads, tolerance tuning,
training, private audio or label scoring.

Commit only source, small contracts/JSON and a report with an inline measured
figure. Bulky PCM/features/intermediates stay on the server. Record source,
artifact, library, model, protocol and environment hashes. Report maximum/RMS
feature errors, FP16 mismatch rates, per-case logit errors and key agreement,
including failures. Passing these nine cases means identical-PCM feasibility
only; full raw-audio376 parity is required before native end-to-end or production
claims. Persisted result cache integration follows that later parity stage.
