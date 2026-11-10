# GiantSteps results

Measured on 10 November 2026 on an Apple M5 (10 cores), macOS 27.2, with 4 worker threads.

- Key: [GiantSteps Key](https://github.com/GiantSteps/giantsteps-key-dataset), 604 two minute previews of electronic dance music. All 604 decoded.
- Tempo: [GiantSteps Tempo](https://github.com/GiantSteps/giantsteps-tempo-dataset), 664 previews, scored against the version 2 annotations. 3 previews are annotated as having no tempo and are left out. All of the other 661 decoded.

## Key detection

| Detector | Exact | Fifth | Relative | Parallel | Other | MIREX score | CPU seconds per track |
| --- | --- | --- | --- | --- | --- | --- | --- |
| libkeyfinder 2.2.8 | 60.4% | 11.1% | 6.5% | 5.6% | 16.4% | 69.0% | 0.22 |
| stratum-dsp 1.0.0 | 30.6% | 20.7% | 6.1% | 12.9% | 29.6% | 45.4% | 1.99 |
| Essentia, bgate profile (reference) | 61.3% | 8.3% | 8.8% | 7.3% | 14.4% | 69.5% | not measured |
| Essentia, edma profile (reference) | 57.6% | 8.3% | 6.3% | 10.8% | 17.1% | 65.8% | not measured |
| Essentia, shaath profile (reference) | 55.1% | 7.9% | 7.3% | 14.1% | 15.6% | 64.1% | not measured |
| Essentia, krumhansl profile (reference) | 49.0% | 8.8% | 8.6% | 15.6% | 18.0% | 59.1% | not measured |

Fifth, relative and parallel are near misses that still mix acceptably. The MIREX score weights them 0.5, 0.3 and 0.2 against 1.0 for an exact match.

## Tempo detection

| Detector | Accuracy 1 | Accuracy 2 | CPU seconds per track |
| --- | --- | --- | --- |
| stratum-dsp 1.0.0, tempo only | 62.2% | 84.7% | 0.39 |
| stratum-dsp 1.0.0, full analysis with default settings | 59.9% | 84.9% | 3.30 |
| Essentia RhythmExtractor2013 (reference) | 59.8% | 82.5% | not measured |

Accuracy 1 counts an estimate within 4% of the annotated tempo. Accuracy 2 also accepts 2, 1/2, 3 and 1/3 times the annotated tempo, as in the standard tempo evaluation.

## Reading the numbers

- libkeyfinder is level with Essentia's best profile on key (69.0% against 69.5% MIREX) and about 9 times cheaper than stratum-dsp.
- stratum-dsp is far behind on key (45.4%) but at least level with Essentia on tempo.
- Tempo only is what `detect_tempo` runs: the stratum-dsp tempogram estimate without its multi-resolution pass, and without the onset lists, beat grid and key that a full analysis also computes. The full analysis row is what `detect_tempo` ran before, measured in the same session for comparison. Dropping the multi-resolution pass changes the estimate on 262 previews, for 15 more within 4% and 1 fewer within the Accuracy 2 factors.
- CPU seconds are the CPU time of each detector call on its own thread, averaged over tracks, so they do not depend on how many threads run in parallel. They were measured while other work ran on the machine, so compare them with each other and not with other sessions.
- The tempo benchmark alone took 85 seconds and peaked at 2.0 GB of memory with 4 worker threads. With the full analysis it took 10.6 minutes and peaked at 5.7 GB. The whole run, key and tempo, took 8.6 minutes and peaked at 4.8 GB, which comes from stratum-dsp key detection.

## Memory of tempo detection

Peak memory of one process that decodes one file and detects its tempo, measured with `/usr/bin/time -l`:

| Audio | Tempo only | Full analysis with default settings |
| --- | --- | --- |
| Two minute preview | 222 MB | 839 MB |
| Six minute file | 497 MB | 2513 MB |

Decoding alone accounts for 44 MB and 125 MB of these. Tempo detection reads at most the middle ten minutes of a file, so longer files cost no more than a ten minute one.

## Reproducing

```
./bench/download_datasets.sh
cargo run --release -p euphoria-bench
cargo run --release -p euphoria-bench -- bench/data --only tempo
uv run bench/essentia_reference.py bench/data
```
