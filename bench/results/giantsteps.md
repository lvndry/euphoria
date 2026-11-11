# GiantSteps results

Measured on 10 November 2026 on an Apple M5 (10 cores), macOS 27.2, with 4 worker threads.

- Key: [GiantSteps Key](https://github.com/GiantSteps/giantsteps-key-dataset), 604 two minute previews of electronic dance music. All 604 decoded.
- Tempo: [GiantSteps Tempo](https://github.com/GiantSteps/giantsteps-tempo-dataset), 664 previews, scored against the version 2 annotations. 3 previews are annotated with a tempo of zero, meaning no tempo, and are left out, so 661 are scored. All 661 decoded.

## Key detection

| Detector | Exact | Fifth | Relative | Parallel | Other | MIREX score | CPU seconds per track |
| --- | --- | --- | --- | --- | --- | --- | --- |
| libkeyfinder 2.2.8 | 60.4% | 11.1% | 6.5% | 5.6% | 16.4% | 69.0% | 0.22 |
| libkeyfinder 2.2.8, tuning corrected (not used) | 60.6% | 11.3% | 6.8% | 5.5% | 15.9% | 69.4% | 0.24 |
| libkeyfinder 2.2.8, sections vote (not used) | 60.8% | 11.1% | 6.6% | 5.6% | 15.9% | 69.4% | 0.22 |
| libkeyfinder 2.2.8, tuning corrected, sections vote (not used) | 60.9% | 11.3% | 7.0% | 5.5% | 15.4% | 69.7% | 0.24 |
| stratum-dsp 1.0.0 | 30.6% | 20.7% | 6.1% | 12.9% | 29.6% | 45.4% | 1.99 |
| Essentia, bgate profile (reference) | 61.3% | 8.3% | 8.8% | 7.3% | 14.4% | 69.5% | not measured |
| Essentia, edma profile (reference) | 57.6% | 8.3% | 6.3% | 10.8% | 17.1% | 65.8% | not measured |
| Essentia, shaath profile (reference) | 55.1% | 7.9% | 7.3% | 14.1% | 15.6% | 64.1% | not measured |
| Essentia, krumhansl profile (reference) | 49.0% | 8.8% | 8.6% | 15.6% | 18.0% | 59.1% | not measured |

Fifth, relative and parallel are near misses that still mix acceptably. The MIREX score weights them 0.5, 0.3 and 0.2 against 1.0 for an exact match.

The three rows marked not used were measured on 11 November 2026 on the same machine, in a run where libkeyfinder alone again scored 60.4% and 69.0% at 0.22 CPU seconds per track. Previews each scores higher and lower than libkeyfinder alone:

| Detector | Higher | Lower | Development half, higher and lower | Held out half, higher and lower |
| --- | --- | --- | --- | --- |
| libkeyfinder, tuning corrected | 4 | 0 | 4 and 0 | 0 and 0 |
| libkeyfinder, sections vote | 8 | 7 | 5 and 3 | 3 and 4 |
| libkeyfinder, tuning corrected, sections vote | 12 | 7 | 9 and 3 | 3 and 4 |

The halves are every other preview in name order, 302 each, as `--tracks development` and `--tracks held-out` select them.

## Tempo detection

| Detector | Accuracy 1 | Accuracy 2 | Below 70 BPM | Above 180 BPM | CPU seconds per track |
| --- | --- | --- | --- | --- | --- |
| stratum-dsp 1.0.0, tempo only, reported between 90 and 180 BPM | 77.6% | 85.5% | 0 | 0 | 0.38 |
| stratum-dsp 1.0.0, tempo only, estimate as it comes (not used) | 62.2% | 84.7% | 83 | 0 | 0.39 |
| stratum-dsp 1.0.0, full analysis with default settings (not used) | 59.9% | 84.9% | 26 | 0 | 3.30 |
| Essentia RhythmExtractor2013 (reference) | 59.8% | 82.5% | not measured | not measured | not measured |

The two count columns are how many of the 661 results fall below 70 and above 180 BPM. The annotations have 3 below 70 and 6 above 180.

Accuracy 1 counts an estimate within 4% of the annotated tempo. Accuracy 2 also accepts 2, 1/2, 3 and 1/3 times the annotated tempo, as in the standard tempo evaluation.

## Reading the numbers

- libkeyfinder is level with Essentia's best profile on key (69.0% against 69.5% MIREX) and about 9 times cheaper than stratum-dsp.
- stratum-dsp is far behind on key (45.4%). On tempo its raw estimate is level with Essentia, and reporting it between 90 and 180 BPM puts it well ahead on Accuracy 1.
- Tempo only is what `detect_tempo` runs: the stratum-dsp tempogram estimate without its multi-resolution pass, and without the onset lists, beat grid and key that a full analysis also computes. `detect_tempo` then doubles an estimate below 90 BPM and halves one at 180 or above until it lies between the two. The raw estimate often lands on half the tempo a DJ counts, which is what the 83 results below 70 BPM are. Applied to the full analysis, the same rule gives 77.5% and 85.5%, so the cheaper estimate loses nothing. 50 previews are annotated below 90 BPM and 6 at 180 or above; the rule reports those at double or half, and they count as Accuracy 1 misses. The full analysis row is `analyze_audio` with its default settings, which `detect_tempo` ran before, measured in the same session for comparison. Dropping the multi-resolution pass changes the estimate on 262 previews, for 15 more within 4% and 1 fewer within the Accuracy 2 factors.
- Tuning correction and the vote between sections each move the MIREX score by 0.4 points, which is two or three previews out of 604, and together by 0.7. Neither is more than chance: the correction acts on 5 previews and improves 4, and the vote improves 8 and worsens 7. Euphoria therefore keeps the key libkeyfinder gives for the whole track, and reports the tuning and the key over time next to it. [Accuracy](../../docs/concepts/accuracy.md#tuning-and-key-over-time) has the reasoning and what two minute previews of dance music cannot show.
- The tuning corrected rows cost 0.02 CPU seconds more per track, which is the tuning measurement. The vote costs nothing measurable, since it scores the chromagram libkeyfinder has already computed.
- CPU seconds are the CPU time of each detector call on its own thread, averaged over tracks, so they do not depend on how many threads run in parallel. They were measured while other work ran on the machine, so compare them with each other and not with other sessions.
- The tempo benchmark alone takes 83 seconds and peaks at 0.9 GB of memory with 4 worker threads. With the full analysis it took 10.6 minutes and peaked at 5.7 GB. The whole run, key and tempo, took 8.6 minutes and peaked at 4.8 GB, which comes from stratum-dsp key detection.

## Cost of a whole analysis

What `euphoria analyze --no-cache` costs for decoding, key (libkeyfinder), tempo and energy together, measured with `/usr/bin/time -l` on the same machine. Memory is the peak memory footprint of the process. The full length tracks are 40 tracks of 4 to 8 minutes from a private library, and the 80 track runs analyze each of them twice.

| Run | CPU seconds | Peak memory |
| --- | --- | --- |
| One two minute preview, 1 worker | 0.8 | 221 MB |
| One seven minute track, 1 worker | 2.6 | 559 MB |
| 40 full length tracks, 4 workers | 100 | 1.9 GB |
| 40 full length tracks, 8 workers | 121 | 4.8 GB |
| 80 full length tracks, 4 workers | 204 | 2.1 GB |
| 80 full length tracks, 8 workers | 245 | 6.2 GB |

With the full stratum-dsp analysis for tempo, the same build took 2.8 CPU seconds and 871 MB on the preview, 10.9 CPU seconds and 2.7 GB on the seven minute track, and 539 CPU seconds and 9.8 GB on the 40 tracks with 4 workers.

Measuring the tuning and the key over time is part of every analysis. Its cost was measured on 11 November 2026 against the build before it, on the same machine and files, three runs each. CPU seconds are user and system time together:

| Run | CPU seconds before | CPU seconds after | Peak memory before | Peak memory after |
| --- | --- | --- | --- | --- |
| One two minute preview, 1 worker | 0.94 | 0.96 | 221 MB | 221 MB |
| One eight minute track, 1 worker | 4.25 | 4.27 | 836 MB | 836 MB |
| 40 two minute previews, 4 workers | 36.9 | 37.8 | 0.8 to 1.2 GB | 0.8 to 1.2 GB |

The eight minute track is four previews joined into one FLAC file, so it is not comparable with the seven minute track above. The cost is the tuning measurement, about 0.02 CPU seconds per track whatever its length, since it reads at most 256 frames. The key over time reuses the chromagram libkeyfinder computes for the key. Peak memory with 4 workers varies from run to run by more than the two builds differ.

Tempo detection reads at most the middle ten minutes of a file, decimated to between 44.1 and 88.2 kHz when the file has a higher sample rate, so its share of the memory stops growing there. Decoding and key detection read the whole file. On a six minute file at 384 kHz, decoding and tempo detection together peak at 1.0 GB, of which decoding is 0.5 GB, and tempo takes 3.8 CPU seconds. Without the decimation they peaked at 4.3 GB and tempo took 10.7 CPU seconds.

## Reproducing

```
./bench/download_datasets.sh
cargo run --release -p euphoria-bench
cargo run --release -p euphoria-bench -- bench/data --only tempo
uv run bench/essentia_reference.py bench/data
```
