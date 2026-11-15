---
title: Set stage
description: What Euphoria's set stage (warm-up, build or peak) means, how it differs from the energy rating, how it is computed, how well it matches a DJ's own folders, and where it falls short.
---

# Set stage

Euphoria suggests where each track tends to sit in a set: **Warm-up**, **Build** or **Peak**. It is a separate suggestion from the [energy rating](energy.md), and it is computed from different things.

## How it differs from energy

| | Energy | Set stage |
| --- | --- | --- |
| Question | How intense does this track sound? | Where does a track like this usually go in a set? |
| Scale | 1 to 10, the same for every library | Warm-up, build or peak, relative to your library |
| Measured from | Spectral flux, loudness, onset rate and loudness range of the audio | Tempo, integrated loudness and loudness range, each compared with your other tracks |
| Changes when you add tracks | Never | Yes: it is a position among your tracks |

The two disagree on purpose. A calm, slow track can sound intense, and a fast, loud one can sound plain. On the maintainer's library the energy rating has no relation to the folders a DJ files tracks in (Spearman −0.04 against warm-up, build and peak), while tempo and loudness follow them closely. That is why the stage is its own suggestion and the energy rating was left alone.

## How it is computed

1. Each of three measurements of a track is turned into a **percentile among the tracks of your library**: the share of your tracks it is faster than, louder than, and narrower in loudness range than. A track exactly as fast as half your library sits at 0.5, counting ties by half.
2. The percentiles are combined with fixed weights into a **stage score** from 0 to 1.
3. Two cut points turn the score into a stage.

| Measurement | Weight | Higher percentile means |
| --- | --- | --- |
| Tempo | 0.7 | Faster than more of your tracks. A tempo you corrected is the one used. |
| Integrated loudness | 0.2 | Louder than more of your tracks. |
| Loudness range | 0.1 | Narrower range, that is more constant loudness, than more of your tracks. |

| Score | Stage |
| --- | --- |
| Below 0.145 | Warm-up |
| 0.145 up to 0.46 | Build |
| 0.46 and above | Peak |

A track with no tempo has no stage. A track whose loudness range is not known, such as one analyzed before the stage existed, is scored from tempo and loudness alone, with their weights scaled up to fill the score; analyze it again (`euphoria analyze --reanalyze`, or add it to the desktop app again) to bring the range in. With fewer than 10 tracks that have a tempo, percentiles mean nothing, so no track has a stage.

The weights are not fitted to one genre and there is nothing genre specific in the model. They start from what the study of the maintainer's folders found: tempo says most, then loudness, then loudness range. Fitting the weights on the data alone gave tempo 0.75 to 0.95 and almost nothing for loudness and loudness range; the rounded 0.7, 0.2, 0.1 were kept because they score the same within a point on every measure below and help in genres whose tracks all share a tempo. Only the two cut points were fitted, so the numbers below test them on genres they were not fitted on.

### Relative to your library

The percentiles are taken among your own tracks, so the same track can have another stage in another library. A 140 BPM track is a peak track in a house library and a warm-up track in a techno library. The same holds for a hip hop library: its slowest tracks warm up and its fastest peak. The percentiles are recomputed from the stored analyses whenever the library changes; no track is analyzed again for it.

- **Desktop app:** the tracks of the library, all of them, whatever folder or playlist is on show.
- **Command line:** the tracks analyzed in that run. `euphoria analyze ~/Music/DJ --stage peak` places each track among the files of `~/Music/DJ`; one file alone has no stage.

## Where it shows

- The **Stage** line of the inspector, with a tooltip.
- An optional **Stage** column of the track table, off until you turn on **Stage column** in the settings.
- The **Stage** chips of the filter bar, which combine with the other filters.
- `stage` in the [JSON output](../reference/cli.md#json-output) of `euphoria analyze`, a Stage line in its detailed view, and `--stage warm-up|build|peak` to list only some stages.

## How well it works

The folders of the maintainer's library are named `<genre>/warmup`, `<genre>/build`, `<genre>/peak` and `<genre>/closing`. The stage was tested against those, leaving one genre out at a time: the cut points were fitted on the other genres and the held out genre was staged with them. Four genres had enough tracks: afro house, deep house, melodic house and tech house, with 491 tracks in the warmup, build and peak folders (20, 158 and 313) and 70 in closing.

| Measure | Result |
| --- | --- |
| Spearman correlation of the score with the folder (warmup, build, peak) | 0.75 (95% interval 0.69 to 0.79) |
| A warmup track scores below a peak track in the same genre | 89% of pairs |
| A build track scores below a peak track in the same genre | 96% of pairs |
| A warmup track scores below a build track in the same genre | 92% of pairs |
| The right one of the three stages | 84% of tracks, 82% averaged over the three folders |

By genre, the Spearman correlation is 0.82 in afro house (255 tracks), 0.87 in deep house (66), 0.38 in tech house (56) and 0.12 in melodic house (114). The pooled number is helped by the tempo differences between genres, so read the per genre numbers as the fairer measure of telling stages apart inside one kind of music. Melodic house tracks sit close to one tempo whatever the folder, so only their loudness can tell them apart, and it tells little.

For comparison, the energy rating scores −0.04 on the same tracks and the tempo alone scores 0.75.

![Stacked bars of the stage given to tracks by the folder they are filed in. Of 20 warmup tracks 85 percent get warm-up, of 158 build tracks 70 percent get build, of 313 peak tracks 91 percent get peak, and of 70 closing tracks 10 percent get warm-up, 51 percent build and 39 percent peak.](../images/set-stage/folders-by-stage.svg)

These are measurements on one library. The warmup folder holds only 20 tracks, so its 85% rests on 17 tracks. Nothing here was tested on another DJ's folders, and the percentile weights were not tested against what a crowd responds to.

## Limits

- **There is no Closing.** The tracks the maintainer files as closing sit just below the peak on tempo and loudness: the model gives 51% of them build and 39% peak, about as it does for tracks of the build and peak folders. A closing track is chosen by when it is played, not by how it sounds, so Euphoria does not offer it as a computed stage.
- **It is relative to your library.** Add a lot of fast tracks and slow ones move toward warm-up. A library with little spread, such as one genre at one tempo, gets stages from small differences.
- **It reflects common DJ practice, not a universal truth.** Sets usually rise in tempo and loudness, and the stage encodes that. A DJ who opens with a loud track or plays a flat set does not follow it, and the stage says where a track tends to sit, not where it must go.
- **It is only as good as the tempo.** A track detected at half or double its tempo is placed wrongly until you correct the tempo, which the stage then uses. The tempo used is the detected one in the default range of 90 to 180 BPM, whatever range you show tempos in.
- **It does not shape the set builder.** The [set builder](../guides/build-a-set.md) still follows the energy curve; the stage is a filter and a label.
