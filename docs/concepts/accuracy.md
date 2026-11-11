---
title: Accuracy
description: How Euphoria's key and tempo detection is measured, and the results.
---

# Accuracy

Euphoria's detectors are measured on public datasets, and the numbers are published here including the misses.

## Datasets

- [GiantSteps Key](https://github.com/GiantSteps/giantsteps-key-dataset): 604 two minute previews of electronic dance music, each with a key labelled by people.
- [GiantSteps Tempo](https://github.com/GiantSteps/giantsteps-tempo-dataset): 664 previews, scored against the version 2 annotations. 3 of them are labelled as having no tempo and are left out, so 661 are scored.

## Scores

**Key.** Each estimate is an exact match, a near miss that still mixes acceptably (a fifth away, the relative key, or the parallel key), or wrong. The MIREX score is the standard summary: an exact match counts 1.0, a fifth 0.5, relative 0.3 and parallel 0.2.

**Tempo.** Accuracy 1 counts an estimate within 4% of the labelled tempo. Accuracy 2 also accepts 2, 1/2, 3 and 1/3 times the labelled tempo, as in the standard tempo evaluation.

**Cost.** CPU seconds per track, measured on the thread that ran each detector.

## Results

| | Key exact | Key MIREX score | Tempo Accuracy 1 | Tempo Accuracy 2 | CPU seconds per track |
| --- | --- | --- | --- | --- | --- |
| libkeyfinder, key | 60.4% | 69.0% | | | 0.22 |
| stratum-dsp, key | 30.6% | 45.4% | | | 1.99 |
| stratum-dsp, tempo | | | 77.6% | 85.5% | 0.38 |
| Essentia, best profile (reference) | 61.3% | 69.5% | 59.8% | 82.5% | |

Euphoria uses libkeyfinder for key and stratum-dsp for tempo. The full tables, including every Essentia profile, are in [bench/results/giantsteps.md](https://github.com/lvndry/euphoria/blob/main/bench/results/giantsteps.md).

## How the tempo is chosen

Tempo detection runs the stratum-dsp tempogram estimate on its own, without the key analysis that stratum-dsp otherwise computes with it. That estimate finds the pulse of a track well, but it often lands on half the tempo a DJ counts: before any correction, 83 of the 661 previews came out below 70 BPM, where only 3 are labelled that slow, and a 125 BPM house track could be reported as 62.5.

Euphoria therefore reports every tempo between 90 and 180 BPM. An estimate below 90 is doubled and one at 180 or above is halved until it falls in that range, which is where most dance music sits. On the benchmark:

| | Accuracy 1 | Accuracy 2 | Results below 70 BPM | Results above 180 BPM |
| --- | --- | --- | --- | --- |
| Tempogram estimate as it comes | 62.2% | 84.7% | 83 | 0 |
| Reported between 90 and 180 BPM | 77.6% | 85.5% | 0 | 0 |
| Full stratum-dsp analysis with its default settings, which Euphoria used before | 59.9% | 84.9% | 26 | 0 |

The price is that a track whose real tempo is below 90 BPM, such as slow hip hop at 85, is reported at double, 170, and one at 180 or above at half. 50 of the 661 previews are labelled below 90 BPM and 6 at 180 or above, and those count as misses in Accuracy 1 above. The harmonic matching can treat half and double tempo as a match for such tracks.

## Tuning and key over time

Euphoria measures two more things about the key of a track, and both were tried as ways to make the detected key better.

- **Tuning.** How far the track is tuned from A = 440 Hz, in cents, where 100 cents is a semitone. A track pressed from vinyl or played back at another speed can sit a quarter tone off, and the notes of a detuned track fall between the notes a key detector listens for.
- **Key over time.** The key of each stretch of the track, so that a track that changes key shows both keys.

Neither changes the key Euphoria reports. Each was measured as a correction to the key, alone and together, and neither improved it by more than the noise of the benchmark, so both are reported next to the key and the key itself is still decided by libkeyfinder from the whole track.

| On the 604 previews | Exact | Fifth | Relative | Parallel | Other | MIREX score | CPU seconds per track | Tracks scored higher | Tracks scored lower |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| libkeyfinder, as Euphoria uses it | 60.4% | 11.1% | 6.5% | 5.6% | 16.4% | 69.0% | 0.22 | | |
| With tuning correction | 60.6% | 11.3% | 6.8% | 5.5% | 15.9% | 69.4% | 0.24 | 4 | 0 |
| With a vote between sections | 60.8% | 11.1% | 6.6% | 5.6% | 15.9% | 69.4% | 0.22 | 8 | 7 |
| With both | 60.9% | 11.3% | 7.0% | 5.5% | 15.4% | 69.7% | 0.24 | 12 | 7 |

The last two columns count the previews each variant scores higher and lower than the first row. They are the fair way to read a difference this small: 0.4 points of MIREX score is two or three previews out of 604.

**Tuning correction** analyzes a track measured at least 20 cents off as if it had been tuned to 440 Hz. It acts on 5 of the 604 previews, improves 4 of them and worsens none. Four previews are too few to tell from chance, so it is off. The reason it has so little to do is that libkeyfinder tolerates detuning well: synthesized chord progressions keep their key up to 40 cents off and lose it only from about 45 cents, where the correction brings it back.

**A vote between sections** cuts the track into windows of about 7 seconds, lets every window score every key, and takes the key with the highest total, so that a loud passage counts no more than a quiet one. It changes the answer on 15 previews, for the better on 8 and for the worse on 7. Counting only the best key of each window, without the scores, was clearly worse in an early experiment, at 67.1%, and is not part of the benchmark.

### What the benchmark does and does not show

The previews are two minutes from the middle of electronic dance music. That music is produced on instruments tuned to 440 Hz and seldom changes key, and a two minute excerpt has no long intro or outro. So the benchmark shows that the two corrections do not help on dance music excerpts, and that they do no harm there. It cannot show what they would do on full length tracks with long intros and breakdowns, on music that modulates, or on a library of vinyl transfers, because it holds almost none of those. No dataset with labelled tuning or labelled key changes was used, so the tuning offsets and the key changes Euphoria reports are checked against synthesized audio only.

### How the tuning is measured

Euphoria finds the strongest partials between 200 and 2,000 Hz in up to 256 frames spread over the track, and measures how far each is from the nearest note of the 440 Hz scale. The offset most of them share is the tuning. On synthesized chord progressions rendered from 45 cents flat to 48 cents sharp the measured offset is within 1 cent, also under noise as loud as the chords.

A tuning is reported only when at least half of the measured partials lie within 10 cents of it. Synthesized noise puts 21% to 30% of them there and synthesized chords more than 70%, and half is the middle. Drums spread the partials much as noise does, so many dance tracks fall below it: a tuning is reported for 237 of the 604 previews, and for the others it is left empty instead of guessed. Of the 237, 213 are within 5 cents of 440 Hz and 5 are 20 cents or more away. At that threshold the two halves of a preview, measured separately, agree within 10 cents for 99% of previews; with no threshold they do for 83%.

The offset runs from 50 cents flat to 50 cents sharp. A track tuned a quarter tone away is as close to one semitone as to the next, so near either end the sign of the offset is uncertain.

### How the key over time is found

The track is cut into windows of about 7 seconds and libkeyfinder scores every key in every window. Euphoria then follows the best scoring key from window to window, and charges a fixed price for every change, so a change shows only when the windows after it favour the new key for long enough to pay for it.

The price was set without the key labels of any dataset. On synthesized progressions that move to another key for 30 seconds, it is the highest of the values tried that still shows a move between two minor keys a fifth apart, while four seconds in a distant key do not show. On the previews, 576 of 604 come out as one section, 26 as two and 2 as three; at half the price 17% of the previews would show a change, which is more than dance music has. A change is placed within a few seconds of where it happens, since each window is 7 seconds long. A move to the parallel key, such as D minor to D major, is the one a synthesized test missed.

When no change is found the timeline is one section in the key of the track. When changes are found, the sections carry the keys that fit each stretch best, and the key of the track is still the one that fits the whole.

### How the settings were chosen

Every threshold above was chosen on synthesized audio or on a property of the previews that does not involve their key labels. The one choice that could have been fitted to the labels, the size of the windows for the vote, was made on half of the previews (every other one in name order) and checked on the other half: on the half it was chosen on, the vote scores 5 previews higher and 3 lower, and on the other half 3 higher and 4 lower. Tuning correction acts from 20 cents, which is where a note passes libkeyfinder's filter for it at half strength, and changes 4 previews in the first half and none in the second.

### Cost

Measuring the tuning adds about 0.02 CPU seconds to the analysis of a track, 2 to 3% of a two minute preview and less of a longer track, and the key over time adds nothing measurable, because it reuses what libkeyfinder computes for the key. Peak memory is unchanged. The measurements are in [bench/results/giantsteps.md](https://github.com/lvndry/euphoria/blob/main/bench/results/giantsteps.md).

## Reproducing

See [running the accuracy benchmark](../development/building.md#accuracy-benchmark).
