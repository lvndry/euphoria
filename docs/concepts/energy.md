---
title: Energy rating
description: What Euphoria's energy rating from 1 to 10 means, how it is computed, and where it falls short.
---

# Energy rating

Euphoria gives every track an energy rating from 1 to 10. Key and tempo tell you whether two tracks fit together. Energy tells you where a track belongs in a set: low numbers for warming up, the middle for building, high numbers for the peak.

The rating describes how intense a track sounds, not how good it is. A 4 is not a worse track than an 8.

To see where a track tends to sit in a set, from its tempo and loudness among your other tracks, use the separate [set stage](set-stage.md) suggestion: it is not the same thing, and on a DJ's own warm-up, build and peak folders it follows them where this rating does not.

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

A stretch of audio is rated from five measurements, and from the tempo when it is known. The stretch is one 8 second window of the [energy curve](#energy-curve), and a track's rating comes from the windows of its curve, as [step 4](#from-measurements-to-a-rating) explains.

| Feature | What it is | More energy when |
| --- | --- | --- |
| Integrated loudness | The overall loudness of the stretch in LUFS, measured as the EBU R128 broadcast standard describes. | louder |
| Loudness range | How far apart the loud and quiet passages are, in LU, also from EBU R128. | narrower, so the track stays at full level |
| Onset rate | Percussive events per second: kicks, claps, hats and other sharp attacks. | more events |
| Low frequency share | The share of the signal's power below 150 Hz, where the kick drum and the bass sit. | larger share |
| Spectral flux | How much the sound changes from one moment to the next. A held note scores near zero, layered percussion scores high. | busier |
| Tempo | Beats per minute, from the tempo detector. Measured, but it has no weight in the rating. | faster |

Onset rate, low frequency share and spectral flux do not depend on how loud the file is. Turning a track down changes only its loudness.

## From measurements to a rating

1. Each feature becomes a score from 0 to 1. A feature at or below its low anchor scores 0, at or above its high anchor scores 1, and in between the score rises in a straight line.
2. The scores are averaged using the weights below. When the tempo is not known, it is left out and the other weights keep their proportions.
3. The average is stretched so that 0.1 maps to the bottom of the scale and 0.9 to the top, then cut into ten equal steps.
4. A track is rated from its energy curve. Its main body is the windows that are at most 6 LU quieter than the loudness its loudest tenth of windows reach. The track's score is the mean score of those windows. A long quiet intro, a quiet outro or a breakdown far below the rest stays out, so the rating describes the part a DJ mixes and sits where the curve sits.

![Onset rate scores 0 up to 2.5 events per second and rises in a straight line to 1 at 7 per second. The weighted average then maps to the rating as a staircase: 0.1 or less gives 1, 0.9 or more gives 10, and ten equal steps lie between.](../images/energy/feature-to-rating.svg)

| Feature | Scores 0 at | Scores 1 at | Weight |
| --- | --- | --- | --- |
| Spectral flux | 0.0045 | 0.0105 | 0.65 |
| Integrated loudness | -14 LUFS | -6.5 LUFS | 0.20 |
| Onset rate | 2.5 per second | 7 per second | 0.10 |
| Loudness range | 12 LU | 1.5 LU | 0.05 |
| Low frequency share | 35% | 90% | 0 |
| Tempo, when known | 100 BPM | 140 BPM | 0 |

The anchors were chosen so that the 604 dance music previews of the [GiantSteps Key](https://github.com/GiantSteps/giantsteps-key-dataset) dataset spread over the scale. Each feature's anchors sit near the values that the lowest and highest few percent of those tracks reach, and the range of the average that is stretched over the ratings is the 2nd to the 98th percentile of the same previews rated whole, rounded to 0.1 and 0.9. Rated from the body of their curves, the previews come out as:

| Rating | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| GiantSteps previews, version 3 | 11 | 20 | 21 | 60 | 80 | 95 | 101 | 103 | 66 | 47 |
| GiantSteps previews, version 2 | 26 | 22 | 53 | 67 | 86 | 91 | 102 | 79 | 42 | 36 |

Version 3 leaves out the quiet breakdowns of the previews, so they sit a little higher on the scale than they did rated whole.

Tempo and low frequency share have no weight, so the rating is the same with and without a tempo.

![Rated from the body of their curves, the 604 GiantSteps previews cover the whole scale from 1 to 10, with most between 4 and 8, the most tracks, 103, at 8, and 47 at 10.](../images/energy/giantsteps-spread.svg)

The weights were fitted to what listeners said, as the next section describes. Spectral flux describes what happens in the music and followed listeners' arousal most closely, so it carries two thirds of the rating. Loudness comes next, but it also reflects how a track was mastered, so it does not dominate. Onset rate and loudness range add a little. Low frequency share and tempo received no weight: a deep, bass heavy warm up track is not an intense one, and tempo detection reports every tempo between 90 and 180 BPM, so a track slower or faster than that enters at double or half its tempo. Neither followed listeners' arousal in either dataset.

![Weights of the six features: spectral flux 0.65, integrated loudness 0.20, onset rate 0.10, loudness range 0.05, low frequency share and tempo 0 each.](../images/energy/weights.svg)

## How well it works

The rating is checked against what listeners said about the intensity of music. Two public datasets hold that: [DEAM](https://cvml.unige.ch/databases/DEAM/) has 1802 songs of rock, pop, folk, jazz, classical, blues, hip-hop, country and electronic music, and [PMEmo](https://github.com/HuiZhangDB/PMEmo) has 794 choruses of popular songs, many of them hip-hop, pop and country. In both, listeners rated each song for arousal, the calm to excited axis of emotion, which is the closest public measure to energy. Both also hold arousal ratings given second by second, so the energy curve can be checked too.

Arousal is not the same as DJ energy. A listener asked how excited a song sounds is not asking what it does on a dance floor, and neither dataset is dance music. Treat the numbers below as agreement with listeners, not as proof that the rating suits a set.

### Method

- Every song is rated the way `euphoria analyze` rates it, with its detected tempo, and the energy curve of each song is computed with `energy_curve`.
- Version 3 changes only how a song's score is drawn from the windows of its curve. Its figures come from the per window measurements recorded for the version 2 check, rated again with the rule of version 3. The audio had been deleted after that check, as below.
- The correlations are Spearman (rank) and Pearson (linear) between the rating and the mean arousal of the song. The 95% intervals come from resampling the songs 1000 times.
- DEAM is split once, by a hash of the song number, into 1216 songs for fitting and 586 held out. PMEmo is split the same way into 541 for fitting and 226 held out. The weights were fitted on the two fitting sets, and every figure below is from the held out songs.
- Only the weights and the stretch of the average were fitted. The anchors of the features stayed where the GiantSteps previews put them, and the range of the average was set on the GiantSteps previews, so dance music still spreads over the whole scale.
- The weights are a least squares fit of each song's rank within its dataset on the feature scores, with every weight held at zero or above, rounded to 0.05, and the range of the average comes from the GiantSteps previews.
- Both datasets are free for research, and DEAM is under Creative Commons BY-NC terms. They are downloaded to run the check and deleted afterwards. No audio, no annotation and no per song value is stored in the repository, and the only thing that comes from them is the six weights of the model.

### Agreement with listeners

Rating from 1 to 10 against the arousal of the held out songs:

| Held out songs | Version | Spearman | Pearson |
| --- | --- | --- | --- |
| DEAM, 586 songs | 1, the weights before | 0.586 (0.528 to 0.640) | 0.545 (0.486 to 0.600) |
| DEAM, 586 songs | 2 | 0.636 (0.585 to 0.686) | 0.566 (0.506 to 0.618) |
| DEAM, 586 songs | 3, this page | 0.627 (0.572 to 0.678) | 0.567 (0.510 to 0.622) |
| PMEmo, 226 songs | 1 | 0.522 (0.412 to 0.622) | 0.550 (0.437 to 0.649) |
| PMEmo, 226 songs | 2 | 0.757 (0.675 to 0.819) | 0.776 (0.706 to 0.834) |
| PMEmo, 226 songs | 3 | 0.744 (0.664 to 0.806) | 0.764 (0.691 to 0.823) |

Version 3 agrees with listeners as well as version 2: every change is far inside the intervals. Before it is cut into ten ratings, its score follows DEAM a little more closely than version 2's (0.671 against 0.656) and PMEmo as closely (0.755 against 0.760).

Weights fitted on the DEAM fitting songs alone also improved the held out PMEmo songs, from 0.526 to 0.651 (the interval of the difference is 0.06 to 0.20), so the gain is not only that of one dataset being fitted to itself.

### By genre

Spearman correlation of the score before it is cut into ten ratings, on the held out songs of each genre. A song can carry several tags in DEAM, so the genres overlap. Version 1 is the weights before version 2. The last column is the change from version 2 to version 3, with its 95% interval.

| Genre | Songs | Version 1 | Version 2 | Version 3 | Change, 2 to 3 |
| --- | --- | --- | --- | --- | --- |
| DEAM, all | 586 | 0.601 | 0.656 | 0.671 | +0.015 (-0.003 to +0.031) |
| Electronic | 110 | 0.650 | 0.709 | 0.746 | +0.037 (-0.003 to +0.085) |
| Rock | 135 | 0.570 | 0.669 | 0.684 | +0.015 (-0.018 to +0.044) |
| Folk | 99 | 0.609 | 0.732 | 0.717 | -0.015 (-0.082 to +0.052) |
| Pop | 81 | 0.729 | 0.735 | 0.728 | -0.007 (-0.044 to +0.029) |
| Jazz | 81 | 0.688 | 0.685 | 0.657 | -0.029 (-0.079 to +0.016) |
| Classical | 71 | 0.454 | 0.458 | 0.585 | +0.126 (-0.021 to +0.275) |
| Country | 71 | 0.507 | 0.474 | 0.499 | +0.025 (-0.018 to +0.079) |
| Blues | 62 | 0.405 | 0.599 | 0.610 | +0.010 (-0.034 to +0.057) |
| Experimental | 54 | 0.694 | 0.751 | 0.779 | +0.028 (-0.054 to +0.108) |
| International | 44 | 0.695 | 0.787 | 0.748 | -0.040 (-0.107 to +0.020) |
| Hip-hop | 32 | 0.442 | 0.621 | 0.643 | +0.022 (-0.063 to +0.093) |
| Soul and R&B | 36 | 0.626 | 0.805 | 0.763 | -0.042 (-0.142 to +0.027) |
| PMEmo, all | 226 | 0.526 | 0.760 | 0.755 | -0.005 (-0.017 to +0.008) |
| Hip-hop and rap | 32 | 0.600 | 0.823 | 0.804 | -0.019 (-0.067 to +0.012) |
| Pop | 26 | 0.535 | 0.631 | 0.626 | -0.005 (-0.146 to +0.120) |
| Country | 22 | 0.705 | 0.775 | 0.760 | -0.015 (-0.167 to +0.075) |
| Dance and electronic | 19 | 0.349 | 0.524 | 0.502 | -0.022 (-0.113 to +0.048) |

![Version 3 sits close to version 2 in every genre group: higher in 9 of 18, lower in 9, every change inside its interval. The largest moves are classical music, up 0.13, and soul and R&B, down 0.04.](../images/energy/agreement-by-genre.svg)

Version 2 raised agreement over version 1 in 13 of 16 genre groups. Version 3 keeps it: no genre moves by an amount its interval can tell from zero. The largest moves are classical music, up 0.13, where loud passages carry the arousal and quiet ones no longer dilute it, and soul and R&B, international and jazz, down 0.03 to 0.04. The genre groups are small, so most of their intervals are wide. PMEmo carries no genre labels of its own. They are the genre that the uploader chose on SoundCloud, available for 519 of 645 songs, so the PMEmo rows are rougher, and pop, country, hip-hop and dance are the only groups with enough held out songs to list.

Among the 166 DEAM songs whose only tag is electronic, the rating's Spearman correlation rises from 0.560 (0.444 to 0.658) to 0.660 (0.555 to 0.748). That group includes the 110 above, as well as songs that were in the fitting set, so it is not a held out figure.

### The energy curve

The curve is checked against the arousal that listeners gave over time. Each 8 second window is compared with the mean of the second by second ratings inside it, for the windows that overlap the annotated part of the song by at least 4 seconds (DEAM from second 15, PMEmo from second 15 of each chorus).

| Held out songs | Version | Spearman, all windows | Spearman, change within a song |
| --- | --- | --- | --- |
| DEAM, 586 songs, 4659 windows | 1 | 0.475 | 0.088 |
| DEAM, 586 songs, 4659 windows | 2 | 0.633 | 0.171 |
| PMEmo, 179 songs, 1192 windows | 1 | 0.552 | 0.215 |
| PMEmo, 179 songs, 1192 windows | 2 | 0.751 | 0.212 |

The curve is the same in versions 2 and 3, so these figures hold for both. The first column is mostly about which songs are intense. The second is the average, over songs, of how well the curve follows the ups and downs inside one song, and it is weak: 0.09 to 0.21. The curve tells an intense passage from a calm one in a different song far better than it follows a change inside one song. The arousal ratings of the second by second part are also noisy and move slowly, so part of that gap is the reference. PMEmo songs are 30 second choruses, and the windows overlap.

### The rating and the curve agree

A DJ reads a track's rating next to its curve, so the two have to tell the same story. Version 2 rated the features of the whole track, and a whole track measures lower than its body: a quiet intro or a breakdown lowers its spectral flux and loudness, and its loudness range is wide where that of an 8 second window is close to zero. On full length house tracks the rating sat a median of one step below the body of the curve, and two or more steps away from it for a third of them.

The check below counts, for each track, how far its rating is from the median rating of its own curve, the level that most of the curve sits at.

| Tracks | Count | Version 2, mean distance | Version 3, mean distance | Version 2, 2 or more steps away | Version 3, 2 or more steps away |
| --- | --- | --- | --- | --- | --- |
| One DJ's library, afro house | 20 | 0.95 | 0.35 | 20% | 10% |
| One DJ's library, deep house | 20 | 1.05 | 0.25 | 35% | 0% |
| One DJ's library, melodic house | 20 | 0.90 | 0.40 | 30% | 0% |
| One DJ's library, tech house | 20 | 0.82 | 0.47 | 15% | 10% |
| One DJ's library, all folders | 85 | 0.97 | 0.39 | 27% | 5% |
| GiantSteps previews | 604 | 0.81 | 0.44 | 17.5% | 5.3% |
| DEAM excerpts | 1802 | 0.23 | 0.16 | 1.9% | 0.6% |
| PMEmo choruses | 794 | 0.39 | 0.21 | 3.7% | 0.8% |

![Version 3 cuts the share of tracks rated 2 or more steps away from their curve in every group: from 27% to 5% in one DJ library of full length tracks, from 17.5% to 5.3% in the GiantSteps previews, and below 1% in DEAM and PMEmo.](../images/energy/curve-agreement.svg)

The library rows are full length tracks copied from one DJ's genre folders, 20 picked at random from each, plus the 5 tracks of two smaller folders in the total. On them version 3 rates 0.9 steps higher on average than version 2, because their intros, breakdowns and outros no longer count. The public sets are short excerpts with little quiet in them, so they move less. The few tracks still 2 steps away have a long stretch below the rest of the track that is still within 6 LU of it, such as a long breakdown with the drums still running.

### How the weights were chosen

- Each feature became a score between its anchors, as above, and the weights were fitted so that the weighted average follows the rank of the listeners' arousal.
- The first fit used the DEAM fitting songs only and was tested on PMEmo. The second pooled the DEAM and PMEmo fitting songs and was tested on both held out sets. The second was kept: against the first it gives up 0.02 on the held out DEAM songs (0.660 against 0.680, scores before the cut into ten) and gains 0.10 on PMEmo (0.752 against 0.651), and it is the one that was not fitted to a single kind of music.
- Spectral flux is the feature that matters: alone it correlates at 0.64 with arousal in DEAM and 0.73 in PMEmo. Loudness range got 0.05, and low frequency share and tempo got nothing in the pooled fit.
- The previous weights gave tempo 0.10 and low frequency share 0.10. Tempo correlated with arousal at -0.02 in DEAM and -0.12 in PMEmo. Low frequency share followed arousal in DEAM (+0.29) and ran against it in PMEmo (-0.18). Onset rate follows arousal in DEAM (+0.44) and not at all in PMEmo (+0.01), which is why it carries 0.10.

### What this does not show

- Whether the rating suits a DJ set. Arousal ratings of a chorus or a 45 second excerpt are not stage-in-the-set ratings of a full track, and no public dataset with energy ratings of dance music was found. In particular the dance music in these datasets is a small share: 166 DEAM songs tagged only electronic and the 19 to 54 PMEmo songs that uploaders called dance or electronic.
- Whether the new weights agree with a DJ's idea of energy. On one DJ's library of house tracks sorted into warm up, build and peak folders, the stage follows tempo closely (correlation 0.45 with the stage in a sample of 180 tracks: 20 warm up, 20 build and 140 peak). Version 1 followed the stage with a correlation of 0.31 (0.17 to 0.45), version 2 does not follow it at all, 0.02 (-0.13 to 0.18), and rates build tracks lowest. Spectral flux, which carries most of version 2, does not rise with the stage in those folders (-0.09). This is one person's way of sorting one genre, and nothing shows that it is the better reference, but it is the only dance music reference available, and on it version 2 is worse. Version 3 was not checked against those folders.
- Whether it holds for loud, compressed masters of every era. Mastering affects the loudness feature, and spectral flux also follows brightness.
- Lossless against lossy files, which was not measured. Encoding 60 songs again as 128 kbit/s MP3 left 80% of the version 1 ratings unchanged and moved the rest by one.

Treat the rating as a guide. It tells calm tracks from intense ones more reliably than it orders tracks that are close.

## Model version

The anchors and weights live in one place in the code, the `ENERGY_MODEL` constant, with a version number, `ENERGY_MODEL_VERSION`. Every rating carries the version that produced it, together with the measured features. The same audio and the same version always give the same rating. When the anchors or weights change, the version changes, and ratings from different versions should not be mixed in one library. The [analysis cache](../reference/cli.md#the-analysis-cache) stores the model version next to each rating and analyzes a track again when its rating comes from another version.

This page describes version 3. It keeps the anchors and weights of version 2 and changes how a track is rated: version 2 rated the features of the whole track, version 3 takes the mean score of the main body of its energy curve. The curve itself is the same in both. Version 1 weighted loudness 0.25, loudness range 0.20, onset rate 0.25, low frequency share 0.10, spectral flux 0.20 and tempo 0.10, and stretched the average from 0.2 to 0.8. Ratings from an older version are computed again when a library is analyzed with version 3.

## Energy curve

A track's energy is not constant. Euphoria also computes the same rating over a sliding window of 8 seconds that moves in steps of 4 seconds, which gives a curve an app can draw under the waveform. The curve shows where the breakdowns and the drops are. Silent stretches have no rating.

The track's rating is the mean of the body of this curve, so it sits where most of the curve sits. The peaks of the curve reach higher than the rating and the breakdowns lower.

![The energy curve of a two minute preview starts at 10, eases down, falls to 1 in a breakdown around 40 to 60 seconds and jumps back to 9 and 10 at the drop. The breakdown is more than 6 LU below the loud part, so it is left out, and the preview rates 7.](../images/energy/energy-curve.svg)

The curve of a two minute preview from the benchmark set, computed with `energy_curve`.

## Overriding the rating

When the rating disagrees with your ear, pick another from 1 to 10 in the desktop app's [correction editor](../guides/desktop-app.md#check-the-key-by-ear-and-correct-it). Your rating is kept like a corrected key or tempo, so it survives analyzing again and is used in the table, the tags you write and the rekordbox export, and **Reset to detected** brings the measured rating back.

## Limits

- **It measures sound, not feeling.** A quiet track with a tense vocal can lift a room, and the rating cannot know that.
- **Mastering affects it.** A heavily compressed master rates higher than a dynamic master of the same music. Spectral flux, which carries most of the weight, also follows brightness.
- **It follows listeners, not a dance floor.** The weights were fitted to arousal ratings of two public datasets. Arousal is not DJ energy, and inside one song the curve follows listeners only weakly.
- **The scale is set on dance music.** The anchors and the stretch of the average come from dance music previews, so most acoustic or ambient music lands at the bottom of the scale and very dense music at the top. The weights were fitted on music of many genres, and [the table by genre](#by-genre) shows how well each follows listeners.
- **The body is averaged.** The rating is the mean of the loud part of the track, so a track with a huge drop and long busy breakdowns rates below its peak. Look at the energy curve for that.
- **A busy breakdown can rate as high as the drop.** Spectral flux carries most of the rating, and a breakdown full of percussion and effects changes as much as a drop. Low frequency share, which would hear the kick and bass come back, has no weight. In 85 house tracks from one DJ's library, a drop rated above the breakdown before it in 69 of 133 pairs, equal in 14 and lower in 50. Giving low frequency share weight orders more drops correctly but follows listeners less closely, so it was left out. Look at the curve's loudness and the cues for the drop.
- **Loudness range adds little inside a window.** The loudness range of 8 seconds of music is almost always narrow, so in the windows the rating is built from, it adds nearly the same small amount to every track.
- **Tempo is not used.** Tempo detection reports every tempo between 90 and 180 BPM, so a track slower or faster than that is reported at double or half its tempo. The rating gives tempo no weight, and the model keeps its anchors in case a weight is given back.
- **Sample rates from 8 kHz to 384 kHz.** A file that claims a rate outside that range is reported as a failure, since only a damaged file does.
- **Mono analysis.** Euphoria mixes a track to mono before analysis, so stereo width plays no part.
- **Lossy files differ a little.** Encoding 60 songs again as 128 kbit/s MP3 left 80% of the ratings unchanged and moved the rest by one. A lossless file against a lossy copy of it was not measured.
