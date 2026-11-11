---
title: Energy rating
description: What Euphoria's energy rating from 1 to 10 means, how it is computed, how it was measured, and where it falls short.
---

# Energy rating

Euphoria gives every track an energy rating from 1 to 10. Key and tempo tell you whether two tracks fit together. Energy tells you how intense a track sounds, which helps decide where it belongs in a set.

The rating describes how intense a track sounds, not how good it is. A 4 is not a worse track than an 8.

## What the numbers mean

| Rating | How it sounds |
| --- | --- |
| 1 to 3 | Calm. Sparse or soft percussion, a dark or mellow sound, a quiet master. |
| 4 to 6 | A steady groove with room in it. |
| 7 to 8 | Busy and bright, with percussion that keeps moving, at a loud level. |
| 9 to 10 | Relentless. Dense, bright percussion at full level. |

The scale is set so that dance music uses all of it. Quiet acoustic or ambient music mostly rates 1.

## Euphoria's own scale

Other tools also rate energy from 1 to 10, each with its own method, and the best known ones do not publish theirs. Euphoria's rating is computed in the open, from the measurements described on this page. The numbers are not comparable between tools: a 7 in Euphoria is not a 7 anywhere else. Use the ratings to compare tracks inside one Euphoria library.

## What is measured

The rating is built from three measurements of the audio.

| Feature | What it is | More energy when |
| --- | --- | --- |
| Spectral flux | How much the sound changes from one moment to the next, relative to the level of the track. A held note scores near zero. Layered percussion with bright hats and claps scores high. | busier and brighter |
| Onset rate | Percussive events per second: kicks, claps, hats and other sharp attacks. | more events |
| Integrated loudness | The overall loudness of the track in LUFS, measured as the EBU R128 broadcast standard describes. | louder |

Spectral flux and onset rate do not depend on how loud the file is. Turning a track down changes only its loudness.

Tempo is not part of the rating. The BPM column already shows it, and in the measurements below it told nothing about energy that the audio had not already told.

## From measurements to a rating

1. Each feature becomes a score from 0 to 1. A feature at or below its low anchor scores 0, at or above its high anchor scores 1, and in between the score rises in a straight line.
2. The scores are averaged using the weights below.
3. The average is stretched so that 0.41 maps to the bottom of the scale and 0.90 to the top, then cut into ten equal steps.

| Feature | Scores 0 at | Scores 1 at | Weight |
| --- | --- | --- | --- |
| Spectral flux | 0.0017 | 0.0104 | 0.55 |
| Onset rate | 0.7 per second | 7.2 per second | 0.26 |
| Integrated loudness | -23.3 LUFS | -6.5 LUFS | 0.19 |

Where the numbers come from:

- **Weights.** They are fitted to how people rated music. The [DEAM](https://cvml.unige.ch/databases/DEAM/) dataset holds 1802 songs with the arousal, from calm to excited, that listeners gave each one. The weights are the ones that best reproduce those ratings on 1216 of the songs, with the rule that no weight may be negative, so louder and busier never rate lower.
- **Anchors.** Each pair of anchors sits at the values that the lowest and the highest 3% of tracks reach, over those songs and over the 604 dance music previews of the [GiantSteps Key](https://github.com/GiantSteps/giantsteps-key-dataset) dataset.
- **The stretch.** 0.41 and 0.90 are the averages that the lowest and the highest 2% of the GiantSteps previews reach, so that dance music spreads over the whole scale:

| Rating | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| GiantSteps previews | 21 | 20 | 43 | 59 | 79 | 107 | 97 | 84 | 59 | 35 |

## How well it works

The rating was measured against the 586 DEAM songs that took no part in the fitting.

- Before the result is stretched over the scale, it puts 75.6% of song pairs in the order that listeners put them, where 50% is chance. The model before this one reached 71.4%.
- The rating from 1 to 10 puts 70.5% of pairs in order on the same songs, about the same as the model before. Half of those songs are quiet music that rates 1, and a rating cannot tell two tracks apart that both rate 1.
- Among the 110 songs tagged as electronic, the rating puts 74.1% of pairs in order, against 72.8% before. With so few songs that difference could be chance.

What these numbers do not show:

- **DEAM is not dance music.** It is rock, pop, folk, jazz, classical and electronic music, and its listeners rated how excited the music sounds, not how it works on a dance floor. No public dataset with energy ratings of dance music was found.
- **One DJ's library disagrees.** In a library of 494 house tracks that their owner had sorted into warm up, build and peak folders, this model rates warm up tracks a little higher than peak tracks on average (6.1 against 5.3), and the model before did the opposite (5.0 against 6.3). In that library the folders follow tempo closely, and this model does not use tempo. It is one person's way of sorting one genre, so it is a second opinion and not a verdict, but it is the only dance music library the rating was checked against.
- **Features that were tried and dropped.** Bass weight, loudness range, brightness, onsets counted per frequency band, kick strength and the share of a track at full level were all measured. None improved the agreement with listeners beyond the three features above, so none is part of the rating.

Treat the rating as a guide. It tells calm tracks from intense ones more reliably than it orders tracks that are close. The full tables are in [bench/results/energy.md](https://github.com/lvndry/euphoria/blob/main/bench/results/energy.md), and [the energy benchmark](../development/building.md#energy-benchmark) reproduces them.

## Model version

The anchors and weights live in one place in the code, the `ENERGY_MODEL` constant, with a version number, `ENERGY_MODEL_VERSION`. Every rating carries the version that produced it, together with the measured features. The same audio and the same version always give the same rating. When the anchors or weights change, the version changes, and ratings from different versions should not be mixed in one library. The [analysis cache](../reference/cli.md#the-analysis-cache) stores the model version next to each rating and analyzes a track again when its rating comes from another version.

This page describes version 2. Version 1 used six inputs with weights set by hand: loudness, loudness range, onset rate, bass weight, spectral flux and tempo. The first analysis after an update from version 1 computes every rating again.

## Energy curve

A track's energy is not constant. Euphoria also computes the same rating over a sliding window of 8 seconds that moves in steps of 4 seconds, which gives a curve an app can draw under the waveform. The curve shows where the breakdowns and the drops are. Silent stretches have no rating.

A window is rated on its own, so the curve usually reaches higher and lower than the rating of the whole track.

## Limits

- **It measures sound, not feeling.** A quiet track with a tense vocal can lift a room, and the rating cannot know that.
- **Mastering affects it.** A louder master rates higher than a quieter master of the same music.
- **Dense mixes hide percussion.** Across the GiantSteps previews the measured onset rate falls as loudness rises. In a loud, full mix fewer attacks stand out from what surrounds them, so some are not counted.
- **It is scaled for dance music.** Other genres get a rating, but most acoustic or ambient music lands at the bottom of the scale.
- **The whole track is averaged.** A track with a long quiet intro and a huge drop rates lower than its peak. Look at the energy curve for that.
- **Sample rates from 8 kHz to 384 kHz.** A file that claims a rate outside that range is reported as a failure, since only a damaged file does.
- **Mono analysis.** Euphoria mixes a track to mono before analysis, so stereo width plays no part.
- **Lossy files differ a little.** Encoding 60 songs again as 128 kbit/s MP3 left 88% of the ratings unchanged and moved the rest by one. A lossless file against a lossy copy of it was not measured.
