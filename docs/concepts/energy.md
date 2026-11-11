---
title: Energy rating
description: What Euphoria's energy rating from 1 to 10 means, how it is computed, and where it falls short.
---

# Energy rating

Euphoria gives every track an energy rating from 1 to 10. Key and tempo tell you whether two tracks fit together. Energy tells you where a track belongs in a set: low numbers for warming up, the middle for building, high numbers for the peak.

The rating describes how intense a track sounds, not how good it is. A 4 is not a worse track than an 8.

## What the numbers mean

| Rating | How it sounds |
| --- | --- |
| 1 to 3 | Calm. Sparse drums or none, quiet or very dynamic, long breakdowns. |
| 4 to 6 | A steady groove with room in it. Most house music lands here. |
| 7 to 8 | Full, loud and busy, with few quiet passages. |
| 9 to 10 | Relentless. Dense percussion at full level from start to end. |

## Euphoria's own scale

Other tools also rate energy from 1 to 10, each with its own method, and the best known ones do not publish theirs. Euphoria's rating is computed in the open, from the measurements described on this page. The numbers are not comparable between tools: a 7 in Euphoria is not a 7 anywhere else. Use the ratings to compare tracks inside one Euphoria library.

## What is measured

The rating is built from five measurements of the audio, and from the tempo when it is known.

| Feature | What it is | More energy when |
| --- | --- | --- |
| Integrated loudness | The overall loudness of the track in LUFS, measured as the EBU R128 broadcast standard describes. | louder |
| Loudness range | How far apart the loud and quiet passages are, in LU, also from EBU R128. | narrower, so the track stays at full level |
| Onset rate | Percussive events per second: kicks, claps, hats and other sharp attacks. | more events |
| Low frequency share | The share of the signal's power below 150 Hz, where the kick drum and the bass sit. | larger share |
| Spectral flux | How much the sound changes from one moment to the next. A held note scores near zero, layered percussion scores high. | busier |
| Tempo | Beats per minute, from the tempo detector. | faster |

Onset rate, low frequency share and spectral flux do not depend on how loud the file is. Turning a track down changes only its loudness.

## From measurements to a rating

1. Each feature becomes a score from 0 to 1. A feature at or below its low anchor scores 0, at or above its high anchor scores 1, and in between the score rises in a straight line.
2. The scores are averaged using the weights below. When the tempo is not known, it is left out and the other weights keep their proportions.
3. The average is stretched so that 0.2 maps to the bottom of the scale and 0.8 to the top, then cut into ten equal steps.

| Feature | Scores 0 at | Scores 1 at | Weight |
| --- | --- | --- | --- |
| Integrated loudness | -14 LUFS | -6.5 LUFS | 0.25 |
| Loudness range | 12 LU | 1.5 LU | 0.20 |
| Onset rate | 2.5 per second | 7 per second | 0.25 |
| Low frequency share | 35% | 90% | 0.10 |
| Spectral flux | 0.0045 | 0.0105 | 0.20 |
| Tempo, when known | 100 BPM | 140 BPM | 0.10 |

The anchors were chosen so that the 604 dance music previews of the [GiantSteps Key](https://github.com/GiantSteps/giantsteps-key-dataset) dataset spread over the scale. Each feature's anchors sit near the values that the lowest and highest few percent of those tracks reach. The dataset comes out as:

| Rating | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Tracks, rated without tempo | 20 | 21 | 41 | 56 | 76 | 98 | 109 | 96 | 55 | 32 |
| Tracks, rated with the detected tempo | 13 | 13 | 30 | 63 | 65 | 115 | 114 | 102 | 56 | 33 |

The weights reflect how much each feature can be trusted. Onset rate and spectral flux describe what happens in the music, so together they carry the most. Loudness matters, but it also reflects how a track was mastered, so it does not dominate. Loudness range rewards tracks that stay at full level. Low frequency share weighs little, because a deep, bass heavy warm up track is not an intense one. Tempo weighs little because tempo detection reports every tempo between 90 and 180 BPM, so a track slower or faster than that enters at double or half its tempo.

## How well it works so far

The scale was checked against a sample of 60 full length tracks that a DJ had sorted into warm up, build, peak and closing folders, across five styles of house music. Rated the way `euphoria analyze` rated them, with the detected tempo, at a time when the tempo detector still reported some tracks at half their tempo:

| Folder | Average rating |
| --- | --- |
| Warm up | 4.5 |
| Build | 5.4 |
| Closing | 5.4 |
| Peak | 5.8 |

The average rises from warm up to peak, which is the direction a DJ expects. The steps are small, though, and single tracks often sit two or three points away from the average of their folder. The styles did not separate: tech house and deep house, for example, averaged about the same.

Treat the rating as a first version. It tells calm tracks from intense ones more reliably than it orders tracks that are close. The scale will be recalibrated as better reference data comes in, and every stored rating records the model version that produced it, so Euphoria knows which ratings to compute again.

## Model version

The anchors and weights live in one place in the code, the `ENERGY_MODEL` constant, with a version number, `ENERGY_MODEL_VERSION`. Every rating carries the version that produced it, together with the measured features. The same audio and the same version always give the same rating. When the anchors or weights change, the version changes, and ratings from different versions should not be mixed in one library. The [analysis cache](../reference/cli.md#the-analysis-cache) stores the model version next to each rating and analyzes a track again when its rating comes from another version.

This page describes version 1.

## Energy curve

A track's energy is not constant. Euphoria also computes the same rating over a sliding window of 8 seconds that moves in steps of 4 seconds, which gives a curve an app can draw under the waveform. The curve shows where the breakdowns and the drops are. Silent stretches have no rating.

A window is rated on its own, so the curve usually reaches higher and lower than the rating of the whole track.

## Limits

- **It measures sound, not feeling.** A quiet track with a tense vocal can lift a room, and the rating cannot know that.
- **Mastering affects it.** A heavily compressed master rates higher than a dynamic master of the same music.
- **It is tuned on dance music.** Other genres get a rating, but the anchors were not chosen for them, so most acoustic or ambient music lands at the bottom of the scale and very dense music at the top.
- **The whole track is averaged.** A track with a long quiet intro and a huge drop rates lower than its peak. Look at the energy curve for that.
- **Tempo errors carry over.** Tempo is reported between 90 and 180 BPM, so a track with a real tempo outside that range enters the rating at double or half its tempo, which can move the rating by one or two.
- **Sample rates from 8 kHz to 384 kHz.** A file that claims a rate outside that range is reported as a failure, since only a damaged file does.
- **Mono analysis.** Euphoria mixes a track to mono before analysis, so stereo width plays no part.
- **Lossy files differ a little.** The same track as MP3 and as FLAC can differ by a small amount in every feature, which now and then moves a rating by one.
