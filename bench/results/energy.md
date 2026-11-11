# Energy rating results

Measured on 11 November 2026 on an Apple M5 (10 cores), macOS 27.2, with 4 worker threads, by `euphoria-bench energy`.

Model 1 is the energy model as it was before this measurement: six inputs with hand set weights. Model 2 is the model fitted here: three features with fitted weights.

## Reference data

- **DEAM**, the [MediaEval Database for Emotional Analysis in Music](https://cvml.unige.ch/databases/DEAM/): 1802 songs under Creative Commons licences, most of them 45 second excerpts, each with the arousal that listeners gave it on a scale from 1 to 9. Arousal is the calm to excited axis of emotion ratings, which is the closest public measure to energy. Its terms of use are Creative Commons BY-NC.
- DEAM is not a dance music dataset. It is rock, pop, folk, jazz, classical and electronic music from free music archives. 374 of its songs are tagged as electronic, and they are reported on their own below.
- The songs are split once, by a hash of the song number, into 1216 for tuning and 586 held out. Everything that was chosen (features, anchors, weights) was chosen on the tuning songs. The held out songs were scored once for model 1 and once for model 2.
- **GiantSteps Key**: 604 two minute previews of electronic dance music. It has no energy labels and is used only to see how ratings of dance music are distributed.
- **One DJ's library**: 494 full length house tracks that their owner had sorted into warm up (20), build (159) and peak (315) folders. This is one person's way of organising one genre, not a ground truth, and it is not part of the repository. It is used as a second opinion only and took no part in fitting.

No public dataset with energy ratings of dance music was found. Energy values written by other DJ software into a library's tags would be a more direct reference, and none were available.

## Agreement with listeners, held out songs

Spearman correlation with the arousal ratings, and the share of song pairs with different ratings that are put in the right order (50% is chance).

| 586 held out songs | Model 1 | Model 2 | Difference, 95% interval |
| --- | --- | --- | --- |
| Rating from 1 to 10, correlation | 0.588 | 0.627 | +0.039, from -0.003 to +0.080 |
| Rating from 1 to 10, pairs in order | 70.2% | 70.5% | |
| Score before it is cut into ten, correlation | 0.601 | 0.667 | +0.066, from +0.026 to +0.105 |
| Score before it is cut into ten, pairs in order | 71.3% | 72.9% | |
| Weighted average before it is spread over the scale, correlation | 0.603 | 0.700 | +0.097, from +0.061 to +0.134 |
| Weighted average before it is spread over the scale, pairs in order | 71.4% | 75.6% | |

| 110 held out songs tagged electronic | Model 1 | Model 2 | Difference, 95% interval |
| --- | --- | --- | --- |
| Rating from 1 to 10, correlation | 0.645 | 0.699 | +0.054, from -0.028 to +0.141 |
| Rating from 1 to 10, pairs in order | 72.8% | 74.1% | |
| Score before it is cut into ten, correlation | 0.650 | 0.718 | +0.068, from -0.011 to +0.150 |
| Weighted average before it is spread over the scale, correlation | 0.648 | 0.734 | +0.086, from +0.011 to +0.167 |

The intervals come from resampling the songs 2000 times and scoring both models on each sample.

What this shows:

- Model 2 orders songs clearly better than model 1 before the result is spread over the scale: the interval of the difference stays above zero.
- At the level of the rating a user sees, the gain on this data is small and not certain. The scale is set so that dance music uses all of it, and DEAM is mostly quieter music, so half of the held out songs rate 1 with model 2 (302 of 586, against 190 with model 1) and cannot be told apart by their rating.
- The songs tagged electronic point the same way with wider intervals, because there are only 110 of them.

## One DJ's set stage folders

| 494 tracks | Model 1 | Model 2 |
| --- | --- | --- |
| Correlation of the rating with the stage | +0.120 | -0.079 |
| Pairs in order | 56.8% | 45.5% |
| Average rating, warm up (20 tracks) | 4.95 | 6.10 |
| Average rating, build (159 tracks) | 5.91 | 5.59 |
| Average rating, peak (315 tracks) | 6.28 | 5.30 |

On this library model 2 does worse than model 1, and slightly worse than chance: warm up tracks rate higher on average than peak tracks.

The reason is visible in the inputs. In this library the stage follows the tempo: the middle half of the build tracks lies between 117 and 120 BPM, and of the peak tracks between 122 and 125 BPM. Tempo alone puts 91.7% of the pairs in order, and loudness 66.9%. Spectral flux and onset rate run slightly against the stage (43.6% and 45.5%). Model 1 gave tempo and loudness more weight than the listeners' ratings support, and model 2 gives tempo none.

The two references disagree, and this measurement cannot say which one is right about dance floor energy. Weightings between the two were tried on the tuning songs. Every step towards the stage folders costs agreement with the listeners:

| Weights: flux, onset rate, loudness, tempo, loudness range | Correlation with listeners, tuning songs | Correlation with the stage |
| --- | --- | --- |
| 0.55, 0.25, 0.20, 0, 0 (model 2, rounded) | 0.686 | -0.062 |
| 0.45, 0.20, 0.30, 0, 0.05 | 0.676 | +0.003 |
| 0.35, 0.15, 0.40, 0.05, 0.05 | 0.646 | +0.101 |
| 0.35, 0, 0.50, 0.10, 0.05 | 0.589 | +0.204 |
| 0.20, 0, 0.55, 0.15, 0.10 | 0.507 | +0.304 |

Model 2 is the first row, because the public data is the reference that others can check. The third row would agree with listeners better than model 1 and with the stage folders about as well as model 1.

## Features

Each feature alone against the arousal ratings of the 1216 tuning songs. A negative correlation means the feature falls as arousal rises.

| Feature | Correlation | In model 2 |
| --- | --- | --- |
| Spectral flux | +0.639 | yes |
| Onset rate | +0.437 | yes |
| Integrated loudness | +0.375 | yes |
| Loudness range | -0.373 | no |
| Low frequency share | +0.283 | no |
| Tempo, reported between 90 and 180 BPM | -0.007 | no |
| Candidate: share of power above 5 kHz | +0.523 | no |
| Candidate: onset rate above 5 kHz | +0.490 | no |
| Candidate: onset rate above 150 Hz, bands merged | +0.474 | no |
| Candidate: onset rate from 150 Hz to 5 kHz | +0.382 | no |
| Candidate: onset rate, three bands merged | +0.019 | no |
| Candidate: onset rate below 150 Hz | -0.218 | no |
| Candidate: kick rate, onsets in the level below 150 Hz | -0.101 | no |
| Candidate: kick strength, rise of the level below 150 Hz | +0.238 | no |
| Candidate: flux below 150 Hz, relative to that band | +0.245 | no |
| Candidate: flux from 150 Hz to 5 kHz, relative to that band | +0.426 | no |
| Candidate: flux above 5 kHz, relative to that band | +0.140 | no |
| Candidate: flux above 150 Hz, relative to that band | +0.407 | no |
| Candidate: share of the flux that is above 5 kHz | +0.371 | no |
| Candidate: share of the time at full level | +0.400 | no |

A feature counts for what it adds to the others, measured by five fold cross validation on the tuning songs:

| Features, weights fitted | Cross validated correlation |
| --- | --- |
| Spectral flux | 0.639 |
| Spectral flux, onset rate | 0.669 |
| Spectral flux, onset rate, integrated loudness (model 2) | 0.683 |
| The five audio features of model 1, weights fitted again | 0.681 |
| Model 2 with the onset rate from 150 Hz to 5 kHz in place of the onset rate | 0.684 |
| Eight features and candidates together | 0.688 |

What this shows:

- The gain of model 2 comes from fitting the weights, not from new features. None of the candidates added more than 0.005, which is within the noise of this measurement, so none was kept.
- Loudness range and low frequency share received weights of 0.03 and 0.02 when fitted and are no longer measured. Tempo received a weight of zero.
- Onsets in dense mixes are not fixed. On GiantSteps the onset rate still falls as loudness rises (correlation -0.23), and so do the onset rates from 150 Hz to 5 kHz (-0.22) and above 5 kHz (-0.16). Onsets below 150 Hz came out as noise: the band is only a few frequency bins wide.
- Spectral flux carries most of the rating. It follows the share of power above 5 kHz closely (correlation 0.88), so it works in part as a measure of brightness.

### Tempo

With the tempo detector reporting every tempo between 90 and 180 BPM, tempo has no relation to the arousal ratings (-0.007), including among the songs tagged electronic (+0.038). Giving it a fixed share of the weight lowers agreement with listeners:

| Share of the weight given to tempo | Cross validated correlation, tuning songs |
| --- | --- |
| 0 | 0.683 |
| 0.05 | 0.677 |
| 0.10 | 0.661 |
| 0.15 | 0.632 |
| 0.20 | 0.590 |

Tempo therefore takes no part in model 2. The one DJ's library says the opposite, as described above.

## Spread of the ratings

| Rating | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| GiantSteps Key, 604 previews, model 1 | 13 | 13 | 30 | 63 | 65 | 115 | 114 | 102 | 56 | 33 |
| GiantSteps Key, 604 previews, model 2 | 21 | 20 | 43 | 59 | 79 | 107 | 97 | 84 | 59 | 35 |
| One DJ's house library, 564 tracks, model 1 | 14 | 13 | 29 | 62 | 82 | 110 | 110 | 82 | 48 | 14 |
| One DJ's house library, 564 tracks, model 2 | 25 | 34 | 63 | 76 | 90 | 89 | 81 | 45 | 37 | 24 |
| DEAM, 586 held out songs, model 1 | 190 | 96 | 87 | 73 | 60 | 48 | 18 | 8 | 4 | 2 |
| DEAM, 586 held out songs, model 2 | 302 | 69 | 45 | 62 | 40 | 22 | 19 | 10 | 9 | 8 |

Dance music uses the whole scale with both models, including a library of a single genre. By style, the house library with model 2: afro house (309 tracks) 4, 8, 23, 34, 49, 51, 62, 30, 29, 19; melodic house (119) 7, 15, 19, 22, 22, 15, 7, 10, 1, 1; deep house (72) 4, 9, 11, 9, 9, 17, 5, 1, 3, 4; tech house (60) 10, 1, 10, 11, 10, 6, 5, 4, 3, 0.

## Cost

CPU seconds per track for measuring and rating energy, after decoding, with both models run on the same tracks in turn:

| | Model 1 | Model 2 |
| --- | --- | --- |
| 80 GiantSteps previews, 2 minutes each | 0.126 | 0.095 |
| 40 full length tracks, 6.6 minutes on average | 0.417 | 0.306 |

Model 2 is about a quarter cheaper, because loudness range and low frequency share are no longer measured.

## Lossy files

60 DEAM songs tagged electronic, which are MP3 files of about 107 kbit/s, were encoded again and rated with model 2:

| Encoded again as | Same rating | Within one |
| --- | --- | --- |
| WAV | 100% | 100% |
| MP3, 128 kbit/s | 88% | 100% |
| MP3, 96 kbit/s | 73% | 100% |

A lossless original against a lossy copy of it was not measured.

## Not established

- Whether model 2 is better for dance music. The public reference is listeners' arousal ratings of mostly other music, and the one dance music library available says model 1 was closer to its owner's folders.
- Whether tempo belongs in the rating. It adds nothing against the listeners' ratings, and it is nearly all that separates build from peak in the one DJ's library.
- How the rating compares with the energy values of other DJ software. No library with such values was available.
- How a lossless file and a lossy copy of it compare.

## Reproducing

See [the energy benchmark](../../docs/development/building.md#energy-benchmark).
