---
title: Accuracy
description: How Euphoria's key and tempo detection is measured, and the results.
---

# Accuracy

Euphoria's detectors are measured on public datasets, and the numbers are published here including the misses.

## Datasets

- [GiantSteps Key](https://github.com/GiantSteps/giantsteps-key-dataset): 604 two minute previews of electronic dance music, each with a key labelled by people.
- [GiantSteps Tempo](https://github.com/GiantSteps/giantsteps-tempo-dataset): 664 previews with labelled tempo, scored against the version 2 annotations.

## Scores

**Key.** Each estimate is an exact match, a near miss that still mixes acceptably (a fifth away, the relative key, or the parallel key), or wrong. The MIREX score is the standard summary: an exact match counts 1.0, a fifth 0.5, relative 0.3 and parallel 0.2.

**Tempo.** Accuracy 1 counts an estimate within 4% of the labelled tempo. Accuracy 2 also accepts 2, 1/2, 3 and 1/3 times the labelled tempo, as in the standard tempo evaluation.

**Cost.** CPU seconds per track, measured on the thread that ran each detector.

## Results

| | Key exact | Key MIREX score | Tempo Accuracy 1 | Tempo Accuracy 2 | CPU seconds per track |
| --- | --- | --- | --- | --- | --- |
| libkeyfinder | 60.4% | 69.0% | | | 0.22 |
| stratum-dsp | 30.6% | 45.4% | 59.6% | 84.5% | 2.86 |
| Essentia, best profile (reference) | 61.3% | 69.5% | 59.8% | 82.5% | |

Euphoria uses libkeyfinder for key and stratum-dsp for tempo. The full tables, including every Essentia profile, are in [bench/results/giantsteps.md](https://github.com/lvndry/euphoria/blob/main/bench/results/giantsteps.md).

## Reproducing

See [running the accuracy benchmark](../development/building.md#accuracy-benchmark).
