# GiantSteps results

Measured on 10 November 2026 on an Apple M5 (10 cores), macOS 27.2, with 4 worker threads.

- Key: [GiantSteps Key](https://github.com/GiantSteps/giantsteps-key-dataset), 604 two minute previews of electronic dance music. All 604 decoded.
- Tempo: [GiantSteps Tempo](https://github.com/GiantSteps/giantsteps-tempo-dataset), 664 previews, scored against the version 2 annotations. All 664 decoded.

## Key detection

| Detector | Exact | Fifth | Relative | Parallel | Other | MIREX score | CPU seconds per track |
| --- | --- | --- | --- | --- | --- | --- | --- |
| libkeyfinder 2.2.8 | 60.4% | 11.1% | 6.5% | 5.6% | 16.4% | 69.0% | 0.22 |
| stratum-dsp 1.0.0 | 30.6% | 20.7% | 6.1% | 12.9% | 29.6% | 45.4% | 2.86 |
| Essentia, bgate profile (reference) | 61.3% | 8.3% | 8.8% | 7.3% | 14.4% | 69.5% | not measured |
| Essentia, edma profile (reference) | 57.6% | 8.3% | 6.3% | 10.8% | 17.1% | 65.8% | not measured |
| Essentia, shaath profile (reference) | 55.1% | 7.9% | 7.3% | 14.1% | 15.6% | 64.1% | not measured |
| Essentia, krumhansl profile (reference) | 49.0% | 8.8% | 8.6% | 15.6% | 18.0% | 59.1% | not measured |

Fifth, relative and parallel are near misses that still mix acceptably. The MIREX score weights them 0.5, 0.3 and 0.2 against 1.0 for an exact match.

## Tempo detection

| Detector | Accuracy 1 | Accuracy 2 | CPU seconds per track |
| --- | --- | --- | --- |
| stratum-dsp 1.0.0 | 59.6% | 84.5% | 2.86 |
| Essentia RhythmExtractor2013 (reference) | 59.8% | 82.5% | not measured |

Accuracy 1 counts an estimate within 4% of the annotated tempo. Accuracy 2 also accepts 2, 1/2, 3 and 1/3 times the annotated tempo, as in the standard tempo evaluation.

## Reading the numbers

- libkeyfinder is level with Essentia's best profile on key (69.0% against 69.5% MIREX) and about 13 times cheaper than stratum-dsp.
- stratum-dsp is far behind on key (45.4%) but level with Essentia on tempo.
- CPU seconds are the CPU time of each detector call on its own thread, averaged over tracks, so they do not depend on how many threads run in parallel. stratum-dsp computes key and tempo in one analysis, so its key and tempo timings are the same work, not two costs to add.
- The whole run took 18 minutes and peaked at 4.7 GB of memory with 4 worker threads.

## Reproducing

```
./bench/download_datasets.sh
cargo run --release -p euphoria-bench
uv run bench/essentia_reference.py bench/data
```
