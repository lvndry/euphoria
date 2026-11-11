# Beat grid results

Measured on 11 November 2026 on an Apple M5 (10 cores), macOS 27.2, by `euphoria-bench beats`, analyzer version 9.

## Reference data

No openly available dataset of electronic dance music has both audio and beat annotations. The Harmonix Set and the HJDB set have annotations, but their audio is not distributed with them, and the one open set with audio that was found is synthesized. The grid is measured instead on 200 excerpts of steady 4/4 drum music:

- **Audio**: the 100 disco and 100 hip hop excerpts of [GTZAN](https://huggingface.co/datasets/marsyas/gtzan), 30 seconds each. GTZAN comes without a licence and is widely used for research. It is downloaded by whoever runs the benchmark and is not part of the repository.
- **Annotations**: beats and downbeats from the GTZAN-Rhythm work of Marchand and Peeters, as collected in [beat_this_annotations](https://github.com/CPJKU/beat_this_annotations) under the MIT licence.

The excerpts are split in name order: the development half (every other excerpt, starting with the first) is where every setting of the grid was chosen, and the held out half was not looked at until the settings were fixed.

## Scores

- **Beat F-measure**: a predicted beat counts when it lies within 70 ms of an annotated one, each used once, as in the standard beat evaluation. The grid's beats are compared over the span of the annotated ones.
- **Grid right**: the beat F-measure is at least 0.9.
- **Downbeat right**: the same F-measure on the downbeats alone is at least 0.9.

Each excerpt is scored twice: with the tempo Euphoria reports, between 90 and 180 BPM, and with the octave of the tempo taken from the annotation. The second leaves the grid itself to be judged, since a grid at double or half the annotated tempo misses half the beats or adds as many.

## Development half

| Tempo | Beat F-measure | Grid right | Downbeat right | Downbeat right when the grid is right |
| --- | --- | --- | --- | --- |
| As reported, between 90 and 180 BPM | 0.911 | 80.0% | 48.0% | 60.0% |
| Octave taken from the annotation | 0.936 | 87.0% | 51.0% | 58.6% |

| Grid confidence, octave from the annotation | Excerpts | Grid right |
| --- | --- | --- |
| high | 36 (36.0%) | 97.2% |
| medium | 33 (33.0%) | 100.0% |
| low | 31 (31.0%) | 61.3% |

## Held out half

| Tempo | Beat F-measure | Grid right | Downbeat right | Downbeat right when the grid is right |
| --- | --- | --- | --- | --- |
| As reported, between 90 and 180 BPM | 0.897 | 80.0% | 45.0% | 55.0% |
| Octave taken from the annotation | 0.908 | 84.0% | 49.0% | 57.1% |

| Grid confidence, octave from the annotation | Excerpts | Grid right |
| --- | --- | --- |
| high | 36 (36.0%) | 97.2% |
| medium | 22 (22.0%) | 100.0% |
| low | 42 (42.0%) | 64.3% |

Every excerpt got a tempo and a grid.

## How the settings were chosen

All on the development half:

- **Timing offset.** Before it, the beats came out 18.7 ms early on average, since an onset is found at the middle of a window and a listener taps slightly later. The grid adds 18.7 ms.
- **Downbeat.** Four cues are scored on each beat of the bar: the accent of the kick, snares and claps one and three beats later, the change of the overall sound, and the change of bass and chords. Weights of 0.5, 1, 1 and 1 picked the annotated downbeat for 59.6% of the development excerpts that have one and 60.6% of the held out ones; the first three cues with equal weights did so for 48%. The snare cue alone finds the right pair of beats, 1 and 3 against 2 and 4, for 76%; telling 1 from 3 is where most misses are.
- **Confidence.** How far the onsets on the beats stand out from every other phase of the same tempo separates right grids from wrong ones with an area under the curve of 0.889, and how far the beats of 8 second stretches drift from the grid with 0.893. Low is below 6.5 or a drift above 35 ms, half the window in which a beat counts; high is from 9.5.

## A check on dance music without beat labels

The 661 GiantSteps Tempo previews have no beat positions, so they can only test whether the grid is consistent. These figures come from a script run once during development, not from `euphoria-bench`:

- Fitted separately, the two halves of a preview agree within 1% on tempo and within 35 ms on where the beats fall for 77% of previews.
- 86% of the grid tempos lie within 0.05 BPM of a whole number, as tempos set in production software do. On synthesized click tracks the grid finds the tempo within 0.011 BPM, also at 127.5 BPM and starting from an estimate 1 BPM off.
- The annotated tempos are whole numbers. The grid is on the annotated tempo for 32.1% of previews, on a neighbouring whole number for 52.5%, and on no whole number for 15.4%. Annotations made by tapping are not precise to 1 BPM, so this cannot tell which is right.

## Cost

`euphoria analyze --no-cache --json`, user CPU time, the mean of three runs each, analyzer version 8 on main against version 9:

| | Before | After |
| --- | --- | --- |
| One two minute preview, 1 worker | 0.93 s | 1.03 s |
| One eight minute track, 1 worker | 4.00 s | 4.49 s |
| 40 previews, 4 workers | 44.0 s | 49.4 s |

The grid adds 11 to 12% to an analysis. Peak memory for the eight minute track varied between 661 and 836 MB from run to run before the change and between 685 and 863 MB after it, so the grid adds about 25 MB, its copy of the audio at a quarter of the sample rate and the onset envelope.

## Reproducing

```
./bench/download_beat_dataset.sh
cargo run --release -p euphoria-bench -- beats --tracks development
cargo run --release -p euphoria-bench -- beats --tracks held-out
```
