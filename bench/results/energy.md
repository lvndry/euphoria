# Energy rating results

Measured on 11 November 2026 on an Apple M5 (10 cores), macOS 27.2, with 4 worker threads, by `euphoria-bench energy`.

Two models are scored on the same tracks:

- **The shipped rating** is what Euphoria reports: energy model version 1, six inputs (loudness, loudness range, onset rate, low frequency share, spectral flux, tempo) with weights set by hand.
- **The fitted model** is an alternative with three features (spectral flux, onset rate, loudness) and weights fitted to listener ratings. It is not shipped. It exists in the benchmark only, so that this comparison can be run again.

## Outcome

The rating was not changed. The fitted model agrees better with how listeners rated the arousal of music of many genres, but that gain is not certain at the level of the rating from 1 to 10, and on the one dance music library available it does worse than the shipped rating. Neither reference is the music Euphoria is for, so the evidence does not justify changing every user's ratings. The public dataset is also under non commercial terms, and no weights that Euphoria ships are fitted on it.

What would settle it: reference ratings of dance music. A few hundred tracks rated from 1 to 10 by hand, or a library whose tracks carry energy values from other DJ software, in a labels file for this benchmark.

## Reference data

- **DEAM**, the [MediaEval Database for Emotional Analysis in Music](https://cvml.unige.ch/databases/DEAM/): 1802 songs under Creative Commons licences, most of them 45 second excerpts, each with the arousal that listeners gave it on a scale from 1 to 9. Arousal is the calm to excited axis of emotion ratings, which is the closest public measure to energy. Its terms of use are Creative Commons BY-NC, free for non profit work. The data is downloaded by whoever runs the benchmark and is not part of the repository.
- DEAM is not a dance music dataset. It is rock, pop, folk, jazz, classical and electronic music from free music archives. 374 of its songs are tagged as electronic, and they are reported on their own below.
- The songs are split once, by a hash of the song number, into 1216 for tuning and 586 held out. Everything about the fitted model (features, anchors, weights) was chosen on the tuning songs. The numbers below are from the held out songs.
- **GiantSteps Key**: 604 two minute previews of electronic dance music. It has no energy labels and is used only to see how ratings of dance music are distributed.
- **One DJ's folders**: 494 full length house tracks that their owner had sorted into warm up (20), build (159) and peak (315) folders. This is one person's way of organising one genre, not a ground truth, and it is not part of the repository. It is a second opinion only and took no part in fitting.

No public dataset with energy ratings of dance music was found. Energy values written by other DJ software into a library's tags would be a more direct reference, and none were available.

## Agreement with listeners, held out songs

Spearman correlation with the arousal ratings, and the share of song pairs with different ratings that are put in the right order (50% is chance). The weighted average is a model's result before it is spread over the scale from 1 to 10.

| 586 held out songs | Shipped rating | Fitted model | Difference, 95% interval |
| --- | --- | --- | --- |
| Rating from 1 to 10, correlation | 0.588 | 0.630 | +0.042, from -0.001 to +0.084 |
| Rating from 1 to 10, pairs in order | 70.2% | 70.6% | |
| Score before it is cut into ten, correlation | 0.601 | 0.667 | +0.066, from +0.026 to +0.105 |
| Score before it is cut into ten, pairs in order | 71.3% | 72.9% | |
| Weighted average, correlation | 0.603 | 0.700 | +0.097, from +0.061 to +0.133 |
| Weighted average, pairs in order | 71.4% | 75.6% | |

| 110 held out songs tagged electronic | Shipped rating | Fitted model | Difference, 95% interval |
| --- | --- | --- | --- |
| Rating from 1 to 10, correlation | 0.645 | 0.699 | +0.054, from -0.028 to +0.141 |
| Rating from 1 to 10, pairs in order | 72.8% | 74.1% | |
| Score before it is cut into ten, correlation | 0.650 | 0.718 | +0.068, from -0.011 to +0.150 |
| Weighted average, correlation | 0.648 | 0.734 | +0.086, from +0.011 to +0.167 |

The intervals come from resampling the songs 2000 times and scoring both models on each sample.

What this shows:

- The fitted model orders songs clearly better before the result is spread over the scale: the interval of the difference stays above zero.
- At the level of the rating a user sees, the gain is small and not certain: the interval includes zero. Both scales are set so that dance music uses all of them, and DEAM is mostly quieter music, so 301 of the 586 held out songs rate 1 with the fitted model (190 with the shipped rating) and cannot be told apart by their rating.
- The songs tagged electronic point the same way with wider intervals, because there are only 110 of them.

## One DJ's set stage folders

| 494 tracks | Shipped rating | Fitted model |
| --- | --- | --- |
| Correlation of the rating with the stage | +0.120 | -0.078 |
| Pairs in order | 56.8% | 45.6% |
| Average rating, warm up (20 tracks) | 4.95 | 6.10 |
| Average rating, build (159 tracks) | 5.91 | 5.59 |
| Average rating, peak (315 tracks) | 6.28 | 5.30 |

On these folders the fitted model does worse than the shipped rating, and slightly worse than chance: warm up tracks rate higher on average than peak tracks. The difference in correlation is -0.198, with a 95% interval from -0.262 to -0.133.

The reason is visible in the inputs. In these folders the stage follows the tempo: the middle half of the build tracks lies between 117 and 120 BPM, and of the peak tracks between 122 and 125 BPM. Tempo alone puts 91.7% of the pairs in order, and loudness 66.9%. Spectral flux and onset rate run slightly against the stage (43.6% and 45.5%). The shipped rating gives tempo and loudness more weight than the listeners' ratings support, and the fitted model gives tempo none.

The two references disagree, and this measurement cannot say which one is right about dance floor energy. Weightings between the two were tried on the tuning songs. Every step towards the stage folders costs agreement with the listeners:

| Weights: flux, onset rate, loudness, tempo, loudness range | Correlation with listeners, tuning songs | Correlation with the stage |
| --- | --- | --- |
| 0.55, 0.25, 0.20, 0, 0 (the fitted model, rounded) | 0.686 | -0.062 |
| 0.45, 0.20, 0.30, 0, 0.05 | 0.676 | +0.003 |
| 0.35, 0.15, 0.40, 0.05, 0.05 | 0.646 | +0.101 |
| 0.35, 0, 0.50, 0.10, 0.05 | 0.589 | +0.204 |
| 0.20, 0, 0.55, 0.15, 0.10 | 0.507 | +0.304 |

For comparison, the weighted average of the shipped rating reaches 0.58 with listeners on the tuning songs and +0.12 with the stage. The third row is better with listeners and about level on the stage, but choosing it would mean fitting to one person's folders.

## Features

Each input alone against the arousal ratings of the 1216 tuning songs. A negative correlation means the input falls as arousal rises.

| Input | Correlation | In the shipped rating | In the fitted model |
| --- | --- | --- | --- |
| Spectral flux | +0.639 | yes | yes |
| Onset rate | +0.437 | yes | yes |
| Integrated loudness | +0.375 | yes | yes |
| Loudness range | -0.373 | yes | no |
| Low frequency share | +0.283 | yes | no |
| Tempo, reported between 90 and 180 BPM | -0.007 | yes | no |
| Candidate: share of power above 5 kHz | +0.523 | no | no |
| Candidate: onset rate above 5 kHz | +0.490 | no | no |
| Candidate: onset rate above 150 Hz, bands merged | +0.474 | no | no |
| Candidate: onset rate from 150 Hz to 5 kHz | +0.382 | no | no |
| Candidate: onset rate, three bands merged | +0.019 | no | no |
| Candidate: onset rate below 150 Hz | -0.218 | no | no |
| Candidate: kick rate, onsets in the level below 150 Hz | -0.101 | no | no |
| Candidate: kick strength, rise of the level below 150 Hz | +0.238 | no | no |
| Candidate: flux below 150 Hz, relative to that band | +0.245 | no | no |
| Candidate: flux from 150 Hz to 5 kHz, relative to that band | +0.426 | no | no |
| Candidate: flux above 5 kHz, relative to that band | +0.140 | no | no |
| Candidate: flux above 150 Hz, relative to that band | +0.407 | no | no |
| Candidate: share of the flux that is above 5 kHz | +0.371 | no | no |
| Candidate: share of the time at full level | +0.400 | no | no |

The candidates were measured in a working version of the analysis and are not in the code. They are the feature fixes that were proposed for the rating: onsets per frequency band, flux without the bass, kick strength, brightness, and time at full level.

A feature counts for what it adds to the others, measured by five fold cross validation on the tuning songs. In each fold the anchors and weights come from the other four fifths of the tuning songs and from the GiantSteps previews, never from the songs being scored:

| Features, weights fitted | Cross validated correlation |
| --- | --- |
| Spectral flux | 0.639 |
| Spectral flux, onset rate | 0.668 |
| Spectral flux, onset rate, integrated loudness (the fitted model) | 0.682 |
| The five audio features of the shipped rating, weights fitted again | 0.680 |
| The fitted model with the onset rate from 150 Hz to 5 kHz in place of the onset rate | 0.683 |
| Eight features and candidates together | 0.688 |

What this shows:

- Against listener ratings, the gain of the fitted model comes from fitting the weights, not from new features. Forward selection added none of the candidates: none raised the cross validated correlation by the 0.003 that the fit script requires. All eight features and candidates together reached 0.688 against 0.682, a gain of 0.006, small next to the uncertainty of the held out comparisons above.
- Loudness range and low frequency share received weights of 0.03 and 0.02 when fitted. Tempo received a weight of zero.
- Onsets in dense mixes are not fixed. On GiantSteps the onset rate falls as loudness rises (correlation -0.23), and so do the candidate onset rates from 150 Hz to 5 kHz (-0.22) and above 5 kHz (-0.16). Onsets below 150 Hz came out as noise: the band is only a few frequency bins wide.
- Spectral flux follows the share of power above 5 kHz closely (correlation 0.88), so it works in part as a measure of brightness.

### Tempo

With the tempo detector reporting every tempo between 90 and 180 BPM, tempo has no relation to the arousal ratings (-0.007), including among the songs tagged electronic (+0.038). Giving it a fixed share of the weight in the fitted model, with tempo scored between the shipped rating's anchors of 100 and 140 BPM, lowers agreement with listeners:

| Share of the weight given to tempo | Cross validated correlation, tuning songs |
| --- | --- |
| 0 | 0.682 |
| 0.05 | 0.676 |
| 0.10 | 0.660 |
| 0.15 | 0.632 |
| 0.20 | 0.592 |

The one DJ's folders say the opposite, as described above. Tempo stays in the shipped rating with its weight of 0.10.

## Spread of the ratings

| Rating | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| GiantSteps Key, 604 previews, shipped rating | 13 | 13 | 30 | 63 | 65 | 115 | 114 | 102 | 56 | 33 |
| GiantSteps Key, 604 previews, fitted model | 21 | 20 | 43 | 59 | 79 | 107 | 96 | 85 | 59 | 35 |
| One DJ's house library, 564 tracks, shipped rating | 14 | 13 | 29 | 62 | 82 | 110 | 110 | 82 | 48 | 14 |
| One DJ's house library, 564 tracks, fitted model | 25 | 34 | 63 | 75 | 91 | 89 | 81 | 45 | 37 | 24 |
| DEAM, 586 held out songs, shipped rating | 190 | 96 | 87 | 73 | 60 | 48 | 18 | 8 | 4 | 2 |
| DEAM, 586 held out songs, fitted model | 301 | 70 | 45 | 62 | 40 | 22 | 19 | 10 | 9 | 8 |

Dance music uses the whole scale with both, including a library of a single genre. By style, the house library with the shipped rating: afro house (309 tracks) 7, 3, 14, 28, 42, 57, 62, 53, 32, 11; melodic house (119) 3, 5, 6, 15, 24, 29, 18, 10, 8, 1; deep house (72) 1, 2, 5, 9, 10, 14, 15, 9, 5, 2; tech house (60) 3, 2, 4, 10, 6, 10, 13, 9, 3, 0. Nothing here calls for a rating relative to the user's own library.

## Cost

Measuring and rating energy with the shipped rating, after decoding and tempo detection, costs 0.126 CPU seconds for a two minute preview (80 GiantSteps previews) and 0.417 CPU seconds for a full length track (40 tracks, 6.6 minutes on average).

An analysis that measured only the three features of the fitted model was run on the same tracks in turn and cost 0.095 and 0.306 CPU seconds, about a quarter less. The saving comes from not measuring loudness range and low frequency share, which the shipped rating needs, so it cannot be had without changing the rating.

## Lossy files

60 DEAM songs tagged electronic, which are MP3 files of about 107 kbit/s, were encoded again and rated with the shipped rating:

| Encoded again as | Same rating | Within one |
| --- | --- | --- |
| WAV | 98% | 100% |
| MP3, 128 kbit/s | 80% | 100% |
| MP3, 96 kbit/s | 77% | 100% |

A lossless original against a lossy copy of it was not measured.

## Not established

- Whether the fitted model, or any other change, would be better for dance music. The public reference is listeners' arousal ratings of mostly other music, and the one dance music library available says the shipped rating is closer to its owner's folders.
- Whether tempo belongs in the rating. It adds nothing against the listeners' ratings, and it is nearly all that separates build from peak in the one DJ's folders.
- How the rating compares with the energy values of other DJ software. No library with such values was available.
- How a lossless file and a lossy copy of it compare.
- Whether weights fitted on DEAM may be shipped under its non commercial terms. Nobody has checked, and none are shipped.

## Reproducing

See [the energy benchmark](../../docs/development/building.md#energy-benchmark).
